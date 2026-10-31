//! [r4-edge-bridges] Footbridges (and the other crossing upgrades: push
//! buttons, removed crosswalks) belong to their junction.
//!
//! Harvey's flag 143: he moved a junction a little to fix it and every
//! footbridge he had built there was gone. A crossing rule was kept only by
//! the crossing's middle, matched within 2 m on every rebuild, so once the
//! junction moved its crosswalks moved off the rules and the rules did
//! nothing.
//!
//! Now a rule at a junction carries a `CrossingAnchor`: the junction as it
//! was, the plan's roads meeting there and the road of the leg it crosses.
//! Every time the walk graph is laid (`build_walk_graph`), before the
//! upgrades go on, each anchored rule finds its junction again (the built
//! junction where its roads still meet, wherever it is now), then the
//! crossing over its leg there, and moves onto it: the bridge's stairs and
//! span are laid for the crossing as it is now. A leg that is gone takes
//! its rule with it (`SimEvent::CrossingRuleRemoved`); the junction's other
//! legs keep theirs. A rule from before anchors (an old save) is anchored
//! where it still matches a junction's crossing, or, left behind by a
//! junction moved before this, on the one crossing it clearly belongs to.

use super::*;
use crate::junctions::{CrossingAnchor, CrossingUpgrade};
use crate::plan::GestureId;
use crate::walk::CrossingKind;
use std::collections::BTreeMap;

/// A junction that kept only one of a rule's roads is still its own
/// within this distance of where it was (one road's ends redrawn).
const FOLLOW_NEAR: f32 = 25.0;
/// An old save's rule left off every crossing goes to the junction
/// crossing over a street running the same way within this distance...
pub const MIGRATE_REACH: f32 = 15.0;
/// ...when the next such crossing is this much farther (it clearly
/// belongs to the nearest).
const MIGRATE_CLEAR: f32 = 4.0;
/// A built piece whose middle is this close to a road's drawn line is
/// part of that road.
const ON_ROAD: f32 = 2.0;

/// A crosswalk over one leg of a junction, as now laid.
#[derive(Clone, Copy, Debug)]
struct LegCrossing {
    node: NodeId,
    segment: SegmentId,
    /// The junction's middle.
    at: Vec2,
    /// The crossing's middle.
    mid: Vec2,
    /// Unit direction of the street crossed.
    dir: Vec2,
    /// Unit direction the leg leaves the junction in.
    leg_dir: Vec2,
}

fn same_crossing(at: Vec2, dir: Vec2, c: &LegCrossing) -> bool {
    c.mid.distance(at) < 2.0 && c.dir.dot(dir).abs() > 0.9
}

impl World {
    /// The crosswalks over the legs of every junction now laid.
    fn leg_crossings(&self) -> Vec<LegCrossing> {
        let mut out = Vec::new();
        for c in &self.walk.crossings {
            let Some(n) = c.node else { continue };
            if c.kind == CrossingKind::Jay || !crate::walk::is_junction(&self.roads, n) {
                continue;
            }
            let Some(s) = self.roads.try_segment(c.segment) else { continue };
            let (pa, pb) = self.roads.segment_points(c.segment);
            let at = self.roads.node(n).pos;
            let far = if s.a == n { pb } else { pa };
            out.push(LegCrossing { node: n, segment: c.segment, at, mid: c.mid, dir: (pb - pa).normalize_or_zero(), leg_dir: (far - at).normalize_or_zero() });
        }
        out
    }

    /// The anchor of a rule on crossing `c`, as laid now.
    fn anchor_on(&self, c: &LegCrossing, roads_here: &[GestureId], lines: &mut BTreeMap<GestureId, Vec<Vec2>>) -> CrossingAnchor {
        let leg = roads_here.iter().copied().filter(|g| self.piece_on_road(c.segment, *g, lines)).min();
        let street = self.streets.of(c.segment).map(|s| s.name.clone()).unwrap_or_default();
        CrossingAnchor { junction: c.at, roads: roads_here.to_vec(), leg, leg_dir: c.leg_dir, street }
    }

    /// Is built piece `s` part of the plan's road `g`?
    fn piece_on_road(&self, s: SegmentId, g: GestureId, lines: &mut BTreeMap<GestureId, Vec<Vec2>>) -> bool {
        let Some(line) = self.road_line(g, lines) else { return false };
        let (a, b) = self.roads.segment_points(s);
        crate::geom::distance_to_polyline((a + b) * 0.5, line) <= ON_ROAD
    }

    fn road_line<'a>(&self, g: GestureId, lines: &'a mut BTreeMap<GestureId, Vec<Vec2>>) -> Option<&'a Vec<Vec2>> {
        if !lines.contains_key(&g) {
            let gesture = self.master.gestures.get(&g)?;
            lines.insert(g, gesture.road_polyline());
        }
        lines.get(&g)
    }

    /// Put every crossing rule at a junction on that junction's crossing
    /// over its leg as the roads are laid now (see the module notes). Run
    /// on the freshly built walk graph, before `apply_upgrades`.
    pub(super) fn fit_crossing_rules(&mut self) {
        if self.crosswalks.rules.is_empty() {
            return;
        }
        let legs = self.leg_crossings();
        let mut lines: BTreeMap<GestureId, Vec<Vec2>> = BTreeMap::new();
        let mut roads_at: BTreeMap<NodeId, Vec<GestureId>> = BTreeMap::new();
        let mut roads_of = |w: &World, n: NodeId| -> Vec<GestureId> { roads_at.entry(n).or_insert_with(|| World::joint_in(&w.roads, &w.master, n).gestures().into_iter().collect()).clone() };
        // Crossings already held: by a rule that matches one as it is (not
        // anchored, or anchored and staying), then as rules are fitted.
        let mut held: Vec<bool> = vec![false; legs.len()];
        let rules = std::mem::take(&mut self.crosswalks.rules);
        let mut out: Vec<Option<crate::junctions::CrossingRule>> = vec![None; rules.len()];
        let mut gone: Vec<(Vec2, CrossingUpgrade, String)> = Vec::new();
        // Crossings held as they are by rules not anchored yet.
        for r in rules.iter().filter(|r| r.anchor.is_none()) {
            if let Some(i) = legs.iter().position(|c| same_crossing(r.at, r.dir, c)) {
                held[i] = true;
            }
        }
        // Anchored rules first (they find their own crossings), then the
        // rest (a rule just made, or from an old save).
        for (idx, mut r) in rules.iter().cloned().enumerate().filter(|(_, r)| r.anchor.is_some()) {
            let anchor = r.anchor.clone().unwrap_or_default();
            // The junction: where the rule's roads still meet, nearest
            // where it was.
            let mut best: Option<(usize, f32, NodeId)> = None;
            let mut seen: Vec<NodeId> = Vec::new();
            for c in &legs {
                if seen.contains(&c.node) {
                    continue;
                }
                seen.push(c.node);
                let d = c.at.distance(anchor.junction);
                let overlap = if anchor.roads.is_empty() {
                    0
                } else {
                    // Only junctions on one of its roads can be it.
                    if !anchor.roads.iter().any(|g| self.road_line(*g, &mut lines).is_some_and(|l| crate::geom::distance_to_polyline(c.at, l) <= ON_ROAD)) {
                        continue;
                    }
                    let here = roads_of(self, c.node);
                    anchor.roads.iter().filter(|g| here.contains(g)).count()
                };
                let ok = overlap >= 2 || (overlap >= anchor.roads.len().min(1) && d <= FOLLOW_NEAR);
                if !ok {
                    continue;
                }
                let better = match best {
                    None => true,
                    Some((o, bd, bn)) => overlap > o || (overlap == o && (d < bd || (d == bd && c.node < bn))),
                };
                if better {
                    best = Some((overlap, d, c.node));
                }
            }
            let leg_alive = anchor.leg.is_none_or(|g| self.master.gestures.contains_key(&g));
            let Some((_, _, node)) = best else {
                if leg_alive {
                    // The junction is not there now (its roads dissolved
                    // it): the rule waits, as before, for it to come back.
                    out[idx] = Some(r);
                } else {
                    gone.push((anchor.junction, r.upgrade, anchor.street.clone()));
                }
                continue;
            };
            // Its leg there: a piece of the same road leaving the same way,
            // else (the road split by the bulldozer) any leg leaving the
            // way it did.
            let mut pick: Option<(usize, f32)> = None;
            for (i, c) in legs.iter().enumerate() {
                if c.node != node || held[i] {
                    continue;
                }
                let dot = c.leg_dir.dot(anchor.leg_dir);
                let on_leg = anchor.leg.is_some_and(|g| self.piece_on_road(c.segment, g, &mut lines));
                let score = if on_leg && dot > 0.0 { 1.0 + dot } else if dot > 0.9 { dot } else { continue };
                if pick.is_none_or(|(_, s)| score > s) {
                    pick = Some((i, score));
                }
            }
            match pick {
                Some((i, _)) => {
                    let c = legs[i];
                    held[i] = true;
                    let here_roads = roads_of(self, c.node);
                    let mut a = self.anchor_on(&c, &here_roads, &mut lines);
                    if a.leg.is_none() {
                        a.leg = anchor.leg.filter(|g| self.master.gestures.contains_key(g));
                    }
                    if a.street.is_empty() {
                        a.street = anchor.street.clone();
                    }
                    r.anchor = Some(a);
                    r.at = c.mid;
                    r.dir = c.dir;
                    out[idx] = Some(r);
                }
                // The leg it crossed is gone from its junction.
                None => gone.push((anchor.junction, r.upgrade, anchor.street.clone())),
            }
        }
        for (idx, mut r) in rules.into_iter().enumerate().filter(|(_, r)| r.anchor.is_none()) {
            let here = legs.iter().position(|c| same_crossing(r.at, r.dir, c));
            // Left off every crossing (a lone zebra's rule matches its own,
            // which is no junction's): an old save's footbridge whose
            // junction was moved before rules were anchored.
            let orphan = || !self.walk.crossings.iter().any(|c| c.kind != CrossingKind::Jay && self.roads.try_segment(c.segment).is_some() && c.mid.distance(r.at) < 2.0 && {
                let (s0, s1) = self.roads.segment_points(c.segment);
                (s1 - s0).normalize_or_zero().dot(r.dir).abs() > 0.9
            });
            let pick = here.or_else(|| if r.upgrade == CrossingUpgrade::Footbridge && orphan() { self.migrate_target(r.at, r.dir, &legs, &held) } else { None });
            if let Some(i) = pick {
                let c = legs[i];
                held[i] = true;
                let here_roads = roads_of(self, c.node);
                r.anchor = Some(self.anchor_on(&c, &here_roads, &mut lines));
                r.at = c.mid;
                r.dir = c.dir;
            }
            out[idx] = Some(r);
        }
        let kept: Vec<crate::junctions::CrossingRule> = out.into_iter().flatten().collect();
        self.crosswalks.rules = kept;
        for (at, upgrade, street) in gone {
            self.events.push(SimEvent::CrossingRuleRemoved { at, upgrade, street });
        }
    }

    /// [r4-edge-bridges] An old save's footbridge rule off every crossing
    /// (its junction moved before rules were anchored): the junction
    /// crossing over a street the same way within `MIGRATE_REACH`, free,
    /// when it is clearly the nearest.
    fn migrate_target(&self, at: Vec2, dir: Vec2, legs: &[LegCrossing], held: &[bool]) -> Option<usize> {
        let mut near: Vec<(f32, usize)> = legs.iter().enumerate().filter(|(i, c)| !held[*i] && c.dir.dot(dir).abs() > 0.9 && c.mid.distance(at) <= MIGRATE_REACH).map(|(i, c)| (c.mid.distance(at), i)).collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        match near.as_slice() {
            [] => None,
            [(_, i)] => Some(*i),
            [(d0, i), (d1, _), ..] => (*d1 - *d0 >= MIGRATE_CLEAR).then_some(*i),
        }
    }

    /// [r4-edge-bridges] The streets meeting at the junction at `at` (for
    /// saying which junction a footbridge belongs to), by name, sorted.
    pub fn junction_streets(&self, at: Vec2) -> Vec<String> {
        let Some(n) = self.roads.node_near(at, 1.0) else { return Vec::new() };
        let mut names: Vec<String> = self.roads.node(n).segments.iter().filter(|&&s| self.roads.try_segment(s).is_some()).filter_map(|&s| self.streets.of(s).map(|st| st.name.clone())).collect();
        names.sort();
        names.dedup();
        names
    }

    /// [r4-edge-bridges] The junction crossing rule `k`'s crosswalk belongs
    /// to: its middle and the name of the street crossed.
    pub fn crossing_junction(&self, k: u32) -> Option<(Vec2, String)> {
        let c = self.walk.crossings.get(k as usize)?;
        let (s0, s1) = self.roads.segment_points(c.segment);
        let dir = (s1 - s0).normalize_or_zero();
        let r = self.crosswalks.find(c.mid, dir)?;
        r.anchor.as_ref().map(|a| (a.junction, a.street.clone()))
    }
}
