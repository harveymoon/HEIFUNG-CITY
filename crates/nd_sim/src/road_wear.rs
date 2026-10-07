//! [events] Infrastructure decay: every street piece wears with age and the
//! traffic over it, bridges faster. The city's road crews resurface what is
//! worn out of a daily maintenance budget the player sets (a funding level
//! times what the network needs to stand still). The queue is not fair, and
//! is not meant to be: crews go where the complaints are loudest and the
//! council listens to, which is where land is dear, so with the budget cut
//! the poor streets wait longest and crumble first (`priority`). Worn roads
//! are driven slower and crash a little more often (`slow_factor`,
//! `crash_factor`, read by the traffic model through `Lane::rough`).
//!
//! The figures are kept per segment, by segment index, with each piece's
//! midpoint: when road works rebuild the network the wear follows the
//! pieces that are still there (`RoadCondition::remap`), and new road is
//! smooth. Nothing here draws a random number.

use crate::roads::RoadNetwork;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Wear a piece gains a day from age alone (a street left alone crumbles
/// in about 300 days).
pub const AGE_WEAR: f32 = 0.0033;
/// Wear a day per vehicle-sample per lane-metre-normalised unit: a sample is
/// one vehicle seen on the piece at a traffic check (every
/// `SAMPLE_EVERY` ticks); lorries and buses count `HEAVY` cars.
pub const TRAFFIC_WEAR: f32 = 0.00012;
pub const HEAVY: f32 = 4.0;
pub const SAMPLE_EVERY: u64 = 10;
/// Bridges and viaducts wear this much faster (joints, salt, the deck).
pub const BRIDGE_WEAR: f32 = 1.35;
/// Crews look at pieces worn this far, and leave them like new.
pub const REPAIR_AT: f32 = 0.3;
/// What resurfacing costs per lane-metre per unit of wear taken off.
pub const REPAIR_COST: f32 = 6.0;
/// What the budget is sized from: a lane-metre's expected wear a day (age
/// plus ordinary traffic) at the repair cost. Funding 1.0 keeps an
/// ordinary network standing still.
pub const NEED_PER_LANE_M: f32 = (AGE_WEAR + 0.0022) * REPAIR_COST;
/// Above this wear a road is driven slower and crashes come easier.
pub const ROUGH_FROM: f32 = 0.35;
/// At full wear cars drive this much under the limit, and misjudge and
/// run off the road this many times as often.
pub const MAX_SLOW: f32 = 0.35;
pub const MAX_CRASH: f32 = 1.5;
/// The most the player can fund (150% of the need).
pub const MAX_FUNDING: f32 = 1.5;
/// Pieces worn this far show as worn; past `CRUMBLING` as crumbling.
pub const WORN: f32 = 0.5;
pub const CRUMBLING: f32 = 0.8;

/// How rough a piece is for traffic, `0..1` (0 below `ROUGH_FROM`).
pub fn roughness(wear: f32) -> f32 {
    ((wear - ROUGH_FROM) / (1.0 - ROUGH_FROM)).clamp(0.0, 1.0)
}

/// Share of the limit a driver keeps on a piece this rough.
pub fn slow_factor(rough: f32) -> f32 {
    1.0 - MAX_SLOW * rough.clamp(0.0, 1.0)
}

/// How many times the ordinary chance of a misjudged gap or running off
/// the road on a piece this rough.
pub fn crash_factor(rough: f32) -> f32 {
    1.0 + MAX_CRASH * rough.clamp(0.0, 1.0)
}

/// Where a worn piece stands in the crews' queue: the more worn, the
/// sooner, and the dearer the land, the sooner still (rich neighbourhoods
/// call the council, and the council answers). At the same wear a piece on
/// land worth 0.9 goes ahead of one on land worth 0.1 by about three to one.
pub fn priority(wear: f32, land_value: f32) -> f32 {
    wear * (0.35 + 1.3 * land_value.clamp(0.0, 1.0))
}

/// What resurfacing a piece costs.
pub fn repair_cost(lane_m: f32, wear: f32) -> f32 {
    lane_m * wear.max(0.0) * REPAIR_COST
}

/// A label for a wear figure.
pub fn label(wear: f32) -> &'static str {
    if wear >= CRUMBLING {
        "crumbling"
    } else if wear >= WORN {
        "worn"
    } else if wear >= REPAIR_AT {
        "fair"
    } else {
        "good"
    }
}

/// One day's road maintenance, for the record.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MaintenanceDay {
    pub day: u64,
    /// What the funding level allowed, and what the crews spent.
    pub allowance: f32,
    pub spent: f32,
    /// Pieces resurfaced, and their lane-metres.
    pub repaired: u32,
    pub repaired_lane_m: f32,
    /// Mean land value under the pieces worked on, whole or patched (who
    /// got served).
    pub repaired_land_value: f32,
    /// Pieces left waiting over `REPAIR_AT` at the end of the day.
    pub waiting: u32,
}

/// The state of the streets.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct RoadCondition {
    /// Wear per segment index `0..1` (0: new).
    pub wear: Vec<f32>,
    /// The midpoint of each piece when its wear was last set, to follow it
    /// through rebuilds (`remap`).
    pub mids: Vec<Vec2>,
    /// `RoadNetwork::version` the vectors were laid out for.
    pub roads_version: u64,
    /// Vehicle samples on each piece since midnight (`HEAVY` per lorry).
    #[serde(skip)]
    pub load: Vec<f32>,
    /// Yesterday's samples, for `World::age_roads`.
    #[serde(skip)]
    pub last_load: Vec<f32>,
    /// The player's funding level: 1.0 is what the network needs, 0 cuts
    /// the line, up to `MAX_FUNDING`.
    pub funding: f32,
    /// The last days' maintenance (newest last, 30 kept).
    pub days: Vec<MaintenanceDay>,
    pub repaired_total: u32,
    /// Bumped whenever the wear changes enough to redraw the cracks.
    #[serde(skip)]
    pub version: u64,
}

impl Default for RoadCondition {
    fn default() -> Self {
        Self { wear: Vec::new(), mids: Vec::new(), roads_version: u64::MAX, load: Vec::new(), last_load: Vec::new(), funding: 1.0, days: Vec::new(), repaired_total: 0, version: 0 }
    }
}

/// Cell size of the grid old midpoints are matched on.
const MATCH_CELL: f32 = 4.0;
/// A new piece takes the wear of an old one whose midpoint is this close.
const MATCH_REACH: f32 = 4.5;

fn cell(p: Vec2) -> (i32, i32) {
    ((p.x / MATCH_CELL).floor() as i32, (p.y / MATCH_CELL).floor() as i32)
}

/// Is this piece the city's to wear and mend: an alive street in town (not
/// a path, not track, not a bored tunnel, which is lined and dry; not a
/// highway carriageway or anything past the map edge, which the region
/// looks after).
pub fn wears(roads: &RoadNetwork, i: usize) -> bool {
    roads.segments.get(i).is_some_and(|s| {
        let edge = crate::landscape::MAP_EDGE;
        let inside = |n: crate::roads::NodeId| roads.node(n).pos.abs().max_element() <= edge;
        s.alive && s.is_street() && !s.tunnel && !s.is_highway() && inside(s.a) && inside(s.b)
    })
}

/// Lane-metres of a piece.
pub fn lane_metres(roads: &RoadNetwork, i: usize) -> f32 {
    let s = &roads.segments[i];
    let (a, b) = (roads.node(s.a).pos, roads.node(s.b).pos);
    a.distance(b) * (s.lanes_fwd + s.lanes_bwd).max(1) as f32
}

fn midpoint(roads: &RoadNetwork, i: usize) -> Vec2 {
    let s = &roads.segments[i];
    (roads.node(s.a).pos + roads.node(s.b).pos) * 0.5
}

impl RoadCondition {
    /// The wear of segment `i` (0 for one never seen).
    pub fn wear_of(&self, i: usize) -> f32 {
        self.wear.get(i).copied().unwrap_or(0.0)
    }

    /// Lay the figures out for the network as it is now. When the network
    /// changed, each piece takes the wear of the old piece whose midpoint
    /// was nearest its own (within `MATCH_REACH`): a street cut in two keeps
    /// its wear on both halves, new road starts smooth. True when anything
    /// moved.
    pub fn remap(&mut self, roads: &RoadNetwork) -> bool {
        let n = roads.segments.len();
        if self.roads_version == roads.version && self.wear.len() == n && self.mids.len() == n {
            if self.load.len() != n {
                self.load = vec![0.0; n];
            }
            return false;
        }
        // Old saves from before wear, or a fresh world: everything new.
        let mut old: HashMap<(i32, i32), Vec<(Vec2, f32)>> = HashMap::new();
        for (k, (&w, &m)) in self.wear.iter().zip(self.mids.iter()).enumerate() {
            let _ = k;
            if w > 0.0 {
                old.entry(cell(m)).or_default().push((m, w));
            }
        }
        let old_load: Vec<f32> = std::mem::take(&mut self.load);
        let mut wear = vec![0.0; n];
        let mut mids = vec![Vec2::ZERO; n];
        for i in 0..n {
            if !roads.segments[i].alive {
                continue;
            }
            let m = midpoint(roads, i);
            mids[i] = m;
            if !wears(roads, i) || old.is_empty() {
                continue;
            }
            // Unchanged pieces keep their index and midpoint: take it
            // straight, else the nearest old midpoint around.
            if self.mids.get(i).is_some_and(|&o| o.distance(m) < 0.01) {
                wear[i] = self.wear[i];
                continue;
            }
            let (cx, cy) = cell(m);
            let mut best: Option<(f32, f32)> = None;
            for dx in -1..=1 {
                for dy in -1..=1 {
                    for &(p, w) in old.get(&(cx + dx, cy + dy)).map(|v| v.as_slice()).unwrap_or(&[]) {
                        let d = p.distance(m);
                        if d <= MATCH_REACH && best.is_none_or(|b| d < b.0) {
                            best = Some((d, w));
                        }
                    }
                }
            }
            wear[i] = best.map_or(0.0, |b| b.1);
        }
        self.wear = wear;
        self.mids = mids;
        self.load = vec![0.0; n];
        // The day's traffic so far is kept where the index still fits.
        for (i, l) in old_load.into_iter().enumerate().take(n) {
            self.load[i] = l;
        }
        self.roads_version = roads.version;
        self.version += 1;
        true
    }

    /// Mean wear over the pieces that wear, weighted by lane-metres.
    pub fn mean_wear(&self, roads: &RoadNetwork) -> f32 {
        let (mut sum, mut len) = (0.0, 0.0);
        for i in 0..roads.segments.len() {
            if wears(roads, i) {
                let l = lane_metres(roads, i);
                sum += self.wear_of(i) * l;
                len += l;
            }
        }
        if len > 0.0 { sum / len } else { 0.0 }
    }

    /// What the network needs a day to stand still, and the allowance at
    /// the funding level.
    pub fn need_per_day(&self, roads: &RoadNetwork) -> f32 {
        (0..roads.segments.len()).filter(|&i| wears(roads, i)).map(|i| lane_metres(roads, i) * if roads.segments[i].bridge { BRIDGE_WEAR } else { 1.0 }).sum::<f32>() * NEED_PER_LANE_M
    }

    pub fn allowance(&self, roads: &RoadNetwork) -> f32 {
        self.need_per_day(roads) * self.funding.clamp(0.0, MAX_FUNDING)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rough_roads_are_slower_and_riskier_only_past_the_threshold() {
        assert_eq!(roughness(0.2), 0.0);
        assert_eq!(slow_factor(roughness(0.2)), 1.0);
        assert!(slow_factor(roughness(0.9)) < 0.8);
        assert!(crash_factor(roughness(1.0)) > 2.0);
    }

    #[test]
    fn dear_land_jumps_the_queue() {
        assert!(priority(0.5, 0.9) > 2.5 * priority(0.5, 0.1));
        // But a crumbling poor street still beats a barely worn rich one.
        assert!(priority(0.95, 0.1) > priority(0.31, 0.6) * 0.9);
    }
}
