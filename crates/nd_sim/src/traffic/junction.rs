//! [tools] The player's junction settings on the lane network
//! (`crate::junctions::JunctionRule`): applied after every rebuild of the
//! lanes, found again by the junction's position and the directions its
//! streets leave it in.
//!
//! - Signals: forced on (where two phases can run) or off; otherwise the
//!   junction's shape decides.
//! - Greens: each phase runs the longest green set for its approaches.
//! - The protected left arrow: as demand calls it, always, or never, and
//!   its length.
//! - No left turn from an approach: its left movement is taken out of what
//!   the junction offers there, so its lane arrows lose it and routes
//!   never use its left-turn connectors.

use super::*;
use crate::junctions::{JunctionRule, SignalMode};

/// How far a rule's anchor may be from its junction and still apply.
pub const RULE_REACH: f32 = 1.5;

impl LaneNetwork {
    /// The intersection at (or within `RULE_REACH` of) `at`.
    pub fn intersection_at(&self, roads: &RoadNetwork, at: Vec2) -> Option<usize> {
        self.intersections
            .iter()
            .enumerate()
            .filter_map(|(k, i)| roads.nodes.get(i.node.0 as usize).map(|n| (k, n.pos.distance(at))))
            .filter(|&(_, d)| d < RULE_REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(k, _)| k)
    }

    /// The unit direction each approach's street leaves its junction in,
    /// as the junction lists them (`Intersection::approaches` order: the
    /// node's alive streets).
    pub fn approach_dirs(&self, roads: &RoadNetwork, k: usize) -> Vec<(SegmentId, Vec2)> {
        let Some(inter) = self.intersections.get(k) else { return Vec::new() };
        roads.streets_at(inter.node).map(|s| (s, roads.away_dir(s, inter.node))).collect()
    }
}

impl Traffic {
    /// The player's rule for the junction at `at`, if any.
    pub fn junction_rule(&self, at: Vec2) -> Option<&JunctionRule> {
        self.junction_rules.iter().find(|r| r.at.distance(at) < RULE_REACH)
    }

    /// Set (or with a default rule, clear) the rule of a junction and apply
    /// it at once. The lanes keep their ids: only signals, greens and
    /// which connectors are permitted change.
    pub fn set_junction_rule(&mut self, roads: &RoadNetwork, rule: JunctionRule) {
        self.junction_rules.retain(|r| r.at.distance(rule.at) >= RULE_REACH);
        if !rule.is_default() {
            self.junction_rules.push(rule.clone());
        }
        if let Some(k) = self.net.intersection_at(roads, rule.at) {
            self.reset_junction(roads, k);
            self.apply_rule(roads, k, &rule);
            // The player's own arrows go back on where they still fit.
            self.resolve_lane_use(roads);
            self.net.apply_signals();
        }
    }

    /// Back to what the rebuild made: signals by shape, default greens and
    /// arrow, every movement offered.
    fn reset_junction(&mut self, roads: &RoadNetwork, k: usize) {
        let node = self.net.intersections[k].node;
        let (approaches, two_way, phases) = {
            let inter = &self.net.intersections[k];
            let streets: Vec<SegmentId> = roads.streets_at(node).collect();
            let two_way = streets.iter().filter(|&&s| roads.segment(s).lanes_fwd > 0 && roads.segment(s).lanes_bwd > 0).count();
            (inter.approaches, two_way, inter.phases.len())
        };
        let ring = self.net.rings.nodes.contains_key(&node);
        let inter = &mut self.net.intersections[k];
        inter.signalized = !ring && default_signalized(approaches, two_way, phases);
        inter.greens = vec![GREEN_SECONDS; phases];
        inter.arrow_mode = Default::default();
        inter.arrow_len = ARROW_SECONDS;
        inter.ruled = false;
        // Every movement back: exits from the connectors there are.
        for ai in 0..self.net.approaches.len() {
            if self.net.approaches[ai].node != node {
                continue;
            }
            let lanes = self.net.approaches[ai].lanes.clone();
            let exits = self.net.exits_of(&lanes);
            if exits != self.net.approaches[ai].exits {
                self.net.approaches[ai].exits = exits;
                self.net.approaches[ai].fixed = false;
                let a = default_arrows(lanes.len(), exits, None, None);
                // Force the connectors to follow even if the arrows match.
                self.net.approaches[ai].arrows.clear();
                self.net.set_arrows(ai, a);
            }
        }
    }

    /// Apply one rule to intersection `k`.
    fn apply_rule(&mut self, roads: &RoadNetwork, k: usize, rule: &JunctionRule) {
        let dirs = self.net.approach_dirs(roads, k);
        let node = self.net.intersections[k].node;
        {
            let inter = &mut self.net.intersections[k];
            match rule.signal {
                SignalMode::Auto => {}
                SignalMode::Signals => {
                    inter.signalized = inter.phases.len() >= 2;
                    inter.ruled = true;
                }
                SignalMode::Unsignalised => {
                    inter.signalized = false;
                    inter.ruled = true;
                }
            }
            let greens: Vec<f32> = inter
                .phases
                .iter()
                .map(|g| g.iter().filter_map(|&ap| dirs.get(ap).and_then(|(_, d)| rule.green_for(*d))).fold(0.0, f32::max))
                .map(|g| if g > 0.0 { g } else { GREEN_SECONDS })
                .collect();
            inter.greens = greens;
            inter.arrow_mode = rule.left_arrow;
            if rule.arrow_seconds > 0.0 {
                inter.arrow_len = rule.arrow_seconds.clamp(crate::junctions::ARROW_MIN_SECONDS, crate::junctions::ARROW_MAX_SECONDS);
            }
            if inter.arrow > 0.0 && rule.left_arrow == crate::junctions::LeftArrow::Never {
                inter.arrow = 0.0;
            }
        }
        // Banned lefts.
        for ai in 0..self.net.approaches.len() {
            let ap = &self.net.approaches[ai];
            if ap.node != node {
                continue;
            }
            let Some(dir) = dirs.iter().find(|(s, _)| *s == ap.segment).map(|(_, d)| *d) else { continue };
            if !rule.left_forbidden(dir) || ap.exits & LEFT == 0 {
                continue;
            }
            let exits = ap.exits & !LEFT;
            if exits == 0 {
                continue; // a left-only approach keeps its left
            }
            let n = ap.lanes.len();
            self.net.approaches[ai].exits = exits;
            self.net.approaches[ai].fixed = false;
            self.net.set_arrows(ai, default_arrows(n, exits, None, None));
        }
    }

    /// Every rule onto the freshly rebuilt network.
    pub(super) fn apply_junction_rules(&mut self, roads: &RoadNetwork) {
        if self.junction_rules.is_empty() {
            return;
        }
        let rules = self.junction_rules.clone();
        for r in &rules {
            if let Some(k) = self.net.intersection_at(roads, r.at) {
                self.apply_rule(roads, k, r);
            }
        }
    }
}
