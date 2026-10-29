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
//! midpoint and ends: when road works rebuild the network the wear follows
//! the pieces that are still there, both halves of a street cut in two
//! included (`RoadCondition::remap`, `seg_match`), and new road is smooth.
//! Nothing here draws a random number.

use crate::roads::RoadNetwork;
use crate::seg_match::{Matcher, OldPiece};
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Wear a piece gains a day from age alone (a street left alone crumbles
/// in about 300 days).
pub const AGE_WEAR: f32 = 0.0033;
/// Wear a day per vehicle sample per lane per 100 m of the piece: a sample
/// is one vehicle seen on the piece at a traffic check (every
/// `SAMPLE_EVERY` ticks), so a lane holding one car per 100 m all day adds
/// about 0.003 a day, a jammed one three times that; lorries and buses
/// count `HEAVY` cars. Per metre, so a long piece is not worn more for
/// holding more cars at once.
pub const TRAFFIC_WEAR: f32 = 0.00002;
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

/// [dirty-industry] What one vehicle seen on a piece counts as: a car one,
/// a bus or an empty lorry `HEAVY`, a loaded lorry `HEAVY` times how hard
/// its load is on the road (`Resource::road_wear_factor`: ore, ingots,
/// crude and livestock for the dirty industries weigh most).
pub fn vehicle_load(cargo: Option<&crate::traffic::Cargo>, bus: bool) -> f32 {
    match cargo {
        Some(c) if c.amount > 0.0 => HEAVY * c.resource.road_wear_factor(),
        Some(_) => HEAVY,
        None if bus => HEAVY,
        None => 1.0,
    }
}

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
    /// The ends of each piece then (empty in saves from before they were
    /// kept: those match by midpoint).
    pub ends: Vec<(Vec2, Vec2)>,
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
        Self { wear: Vec::new(), mids: Vec::new(), ends: Vec::new(), roads_version: u64::MAX, load: Vec::new(), last_load: Vec::new(), funding: 1.0, days: Vec::new(), repaired_total: 0, version: 0 }
    }
}

/// Is this piece the city's to wear and mend: an alive street in town (not
/// a path, not track, not a bored tunnel, which is lined and dry; not a
/// highway carriageway or anything past the map edge, which the region
/// looks after).
pub fn wears(roads: &RoadNetwork, i: usize) -> bool {
    roads.segments.get(i).is_some_and(|s| {
        // [r4-tiles] Out to the edge of land that can be bought (the
        // roads in a bought tile are the city's).
        let edge = crate::landscape::GROWN_HALF - 40.0;
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

fn ends_of(roads: &RoadNetwork, i: usize) -> (Vec2, Vec2) {
    let s = &roads.segments[i];
    (roads.node(s.a).pos, roads.node(s.b).pos)
}

impl RoadCondition {
    /// The wear of segment `i` (0 for one never seen).
    pub fn wear_of(&self, i: usize) -> f32 {
        self.wear.get(i).copied().unwrap_or(0.0)
    }

    /// Lay the figures out for the network as it is now. When the network
    /// changed, each piece takes the wear (and the day's traffic so far) of
    /// the old piece it continues (`seg_match`: its midpoint on that old
    /// piece): a street cut in two keeps its wear on both halves, new road
    /// starts smooth. True when anything moved.
    pub fn remap(&mut self, roads: &RoadNetwork) -> bool {
        let n = roads.segments.len();
        if self.roads_version == roads.version && self.wear.len() == n && self.mids.len() == n {
            if self.load.len() != n {
                self.load = vec![0.0; n];
            }
            if self.ends.len() != n {
                self.ends = (0..n).map(|i| ends_of(roads, i)).collect();
            }
            return false;
        }
        let old_load: Vec<f32> = std::mem::take(&mut self.load);
        // Old saves from before wear, or a fresh world: everything new.
        // Pieces with wear or traffic are worth following.
        let worth = |k: usize| self.wear.get(k).is_some_and(|&w| w > 0.0) || old_load.get(k).is_some_and(|&l| l > 0.0);
        let old = Matcher::new((0..self.mids.len()).filter(|&k| worth(k)).map(|k| (k, OldPiece { mid: self.mids[k], ends: self.ends.get(k).copied() })));
        let old_len = |k: usize| self.ends.get(k).map(|&(a, b)| a.distance(b));
        let mut wear = vec![0.0; n];
        let mut load = vec![0.0; n];
        let mut mids = vec![Vec2::ZERO; n];
        let mut ends = vec![(Vec2::ZERO, Vec2::ZERO); n];
        for i in 0..n {
            if !roads.segments[i].alive {
                continue;
            }
            let (a, b) = ends_of(roads, i);
            let m = (a + b) * 0.5;
            mids[i] = m;
            ends[i] = (a, b);
            if old.is_empty() {
                continue;
            }
            // Unchanged pieces keep their index and ends: take it straight,
            // else the old piece it continues.
            let same = self.mids.get(i).is_some_and(|&o| o.distance(m) < 0.01) && self.ends.get(i).is_none_or(|&(oa, ob)| oa.distance(a) < 0.01 && ob.distance(b) < 0.01);
            let Some(k) = (if same { Some(i) } else { old.find(a, b) }) else { continue };
            if wears(roads, i) {
                wear[i] = self.wear.get(k).copied().unwrap_or(0.0);
            }
            // The traffic counted on the old piece, shared by length: half
            // a street cut in two carried about half of it.
            let share = if same { 1.0 } else { old_len(k).filter(|&l| l > 0.1).map_or(1.0, |l| (a.distance(b) / l).min(1.0)) };
            load[i] = old_load.get(k).copied().unwrap_or(0.0) * share;
        }
        self.wear = wear;
        self.mids = mids;
        self.ends = ends;
        self.load = load;
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

    /// A street cut in two by a new crossing keeps its wear on both halves
    /// (each 100 m long: far past the old midpoint-to-midpoint reach), the
    /// crossing street is new, and the wear follows the renumbered pieces.
    #[test]
    fn a_split_street_keeps_its_wear_on_both_halves() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0)], 1, 1);
        let mut c = RoadCondition::default();
        c.remap(&roads);
        assert_eq!(c.wear.len(), 1);
        c.wear[0] = 0.6;
        c.load[0] = 10.0;
        roads.add_polyline(&[Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0)], 1, 1);
        assert!(c.remap(&roads));
        let alive: Vec<usize> = (0..roads.segments.len()).filter(|&i| roads.segments[i].alive).collect();
        assert_eq!(alive.len(), 4);
        for i in alive {
            let (a, b) = ends_of(&roads, i);
            if (a.y - b.y).abs() < 0.1 {
                assert!((c.wear[i] - 0.6).abs() < 1e-6, "half {i} lost its wear: {}", c.wear[i]);
                assert!((c.load[i] - 5.0).abs() < 0.01, "half {i} takes half the day's traffic: {}", c.load[i]);
            } else {
                assert_eq!(c.wear[i], 0.0, "the new crossing street is smooth");
            }
        }
        // A save from before the ends were kept matches by midpoint and
        // still loads.
        c.ends.clear();
        c.roads_version = u64::MAX;
        c.remap(&roads);
        assert_eq!(c.ends.len(), roads.segments.len());
    }
}
