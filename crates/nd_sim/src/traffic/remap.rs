//! Carrying vehicles and player lane arrows across a lane-network rebuild.
//!
//! Lane ids are indices into a network rebuilt from scratch whenever a road
//! changes, so after a rebuild an old id names some other lane (or none).
//! Taking every vehicle off the road at each edit (what the rebuild used to
//! do) made a single new street empty the whole city of traffic. Instead
//! each vehicle is matched by geometry: a lane whose points did not change
//! is the same lane, and the vehicle keeps its place and route on it; a lane
//! that was reshaped (a new junction split it, a widened street moved it)
//! hands the vehicle to the new lane under it, which plans a fresh route to
//! the same destination. Only a vehicle with no lane left under it (its
//! street was removed) or no way on from there is given back to the world
//! (`World::rebuild_from_master` parks or strands it).
//!
//! Player lane arrows (`LaneUseRule`) are anchored the same way: by the
//! junction's position and the direction the street leaves it in, not by
//! the segment and node ids that a rebuild renumbers.

use super::*;

/// How far (m) a vehicle may stand from a reshaped lane and still be on
/// it: a street widened from one lane each way to two moves its lanes by
/// half a lane width, a car mid lane change sits between two.
const CARRY_REACH: f32 = LANE_WIDTH;
/// How far (m) from the old destination a new lane may end the route: the
/// stop or kerb it was driving to moved with a reshaped street.
const GOAL_REACH: f32 = 25.0;
/// The direction of a lane under a vehicle must agree with the vehicle's
/// own (cosine): never the oncoming lane of the same street.
const SAME_WAY: f32 = 0.7;

/// What became of the vehicles when the lanes were rebuilt.
#[derive(Debug, Default)]
pub struct CarryReport {
    /// On the very same lane geometry, at the same place, same route.
    pub kept: usize,
    /// On a reshaped lane under them, or with a route planned afresh.
    pub rerouted: usize,
    /// Taken off the road: no lane left under them, or no way on from
    /// there. Removed from `Traffic::cars`; the world parks or strands them.
    pub lost: Vec<(CarId, Car)>,
}

/// A lane's geometry to the centimetre: its ends and length. Unchanged
/// lanes are rebuilt bit for bit from unchanged roads, so this finds them
/// whatever their new id.
type LaneKey = (i32, i32, i32, i32, i32);

fn lane_key(l: &Lane) -> LaneKey {
    let q = |v: f32| (v * 100.0).round() as i32;
    let (a, b) = (l.points[0], *l.points.last().unwrap());
    (q(a.x), q(a.y), q(b.x), q(b.y), q(l.length))
}

impl LaneNetwork {
    /// Can a vehicle go straight from `a` to `b` on this network: through
    /// a connector or by changing into the lane beside it?
    fn steps_to(&self, a: LaneId, b: LaneId) -> bool {
        let l = &self.lanes[a as usize];
        l.next.contains(&b) || l.is_beside(b)
    }

    /// Lanes running the way of `dir` within `reach` of `p`, nearest first,
    /// as `(lane, distance along, distance from)`.
    fn lanes_near(&self, p: Vec2, dir: Vec2, reach: f32) -> Vec<(LaneId, f32, f32)> {
        let mut out: Vec<(LaneId, f32, f32)> = Vec::new();
        for (i, l) in self.lanes.iter().enumerate() {
            if l.points.len() < 2 {
                continue;
            }
            // Cheap reject on the bounding box first.
            let (lo, hi) = l.points.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), q| (lo.min(*q), hi.max(*q)));
            if p.x < lo.x - reach || p.y < lo.y - reach || p.x > hi.x + reach || p.y > hi.y + reach {
                continue;
            }
            let (s, d) = l.project(p);
            if d > reach || l.point_at(s).1.dot(dir) < SAME_WAY {
                continue;
            }
            out.push((i as LaneId, s, d));
        }
        // Nearest first, ties by id, so the choice depends on nothing but
        // the geometry.
        out.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.0.cmp(&b.0)));
        out
    }
}

impl Traffic {
    /// Rebuild the lanes for a changed road network and carry every vehicle
    /// across (see the module notes). Cars keep their ids, so whoever holds
    /// one (a driver, a service unit, a taxi firm) still finds it.
    pub fn rebuild_carrying(&mut self, roads: &RoadNetwork) -> CarryReport {
        let old = std::mem::take(&mut self.net.lanes);
        self.rebuild(roads);
        let mut by_key: HashMap<LaneKey, LaneId> = HashMap::with_capacity(self.net.lanes.len());
        for (i, l) in self.net.lanes.iter().enumerate() {
            if l.points.len() >= 2 {
                by_key.entry(lane_key(l)).or_insert(i as LaneId);
            }
        }
        let same = |lane: LaneId| old.get(lane as usize).filter(|l| l.points.len() >= 2).and_then(|l| by_key.get(&lane_key(l)).copied());
        let mut report = CarryReport::default();
        let ids: Vec<CarId> = self.cars.keys().collect();
        for id in ids {
            match self.carry(&old, id, &same) {
                Some((route, s, s_end, rerouted)) => {
                    let car = &mut self.cars[id];
                    let lane = route[0];
                    car.route = route;
                    car.idx = 0;
                    car.s = s;
                    car.s_end = s_end;
                    car.blocked_by = None;
                    car.drive.running = None;
                    car.drive.lane = Some(lane);
                    if rerouted {
                        car.age = 0.0;
                        report.rerouted += 1;
                    } else {
                        report.kept += 1;
                    }
                }
                None => {
                    if let Some(car) = self.cars.remove(id) {
                        report.lost.push((id, car));
                    }
                }
            }
        }
        // Into the new lanes' lists, as after a load: not a car waiting at
        // the kerb, nor one in a stunt off the lanes.
        let ids: Vec<CarId> = self.cars.keys().filter(|&id| !self.cars[id].entering && !self.cars[id].in_stunt()).collect();
        for id in ids {
            let lane = self.cars[id].route[0];
            self.insert_sorted(lane, id);
        }
        report
    }

    /// Where car `id` goes on the new lanes: `(route, s, s_end, rerouted)`,
    /// or None when there is no lane left under it or no way on.
    fn carry(&self, old: &[Lane], id: CarId, same: &impl Fn(LaneId) -> Option<LaneId>) -> Option<(Vec<LaneId>, f32, f32, bool)> {
        let car = &self.cars[id];
        let &here = car.route.get(car.idx)?;
        // The lane under it: the same one, or the nearest running its way.
        let (lane, s, moved) = match same(here) {
            Some(l) => (l, car.s.min(self.net.lanes[l as usize].length), false),
            None => {
                let heading = Vec2::new(car.heading.cos(), car.heading.sin());
                let &(l, s, _) = self.net.lanes_near(car.pos, heading, CARRY_REACH).first()?;
                (l, s, true)
            }
        };
        // The rest of the route, if every lane of it is still there and
        // still joined up; otherwise a new route to where it was going.
        // (A car on its last lane that was reshaped measures its end anew.)
        let rest: Option<Vec<LaneId>> = car.route[car.idx + 1..].iter().map(|&l| same(l)).collect();
        let joined = rest.filter(|r| (!moved || !r.is_empty()) && std::iter::once(lane).chain(r.iter().copied()).zip(r.iter().copied()).all(|(a, b)| self.net.steps_to(a, b)));
        match joined {
            Some(r) => {
                let unchanged = !moved;
                let mut route = Vec::with_capacity(r.len() + 1);
                route.push(lane);
                route.extend(r);
                Some((route, s, car.s_end, !unchanged))
            }
            None => self.route_to_old_goal(old, car, lane, s, same).map(|(route, s_end)| (route, s, s_end, true)),
        }
    }

    /// A route from `lane` at `s` to where the car's old route ended: the
    /// same last lane if it survived, else the nearest lanes running the
    /// same way near the old end point.
    fn route_to_old_goal(&self, old: &[Lane], car: &Car, lane: LaneId, s: f32, same: &impl Fn(LaneId) -> Option<LaneId>) -> Option<(Vec<LaneId>, f32)> {
        let last = *car.route.last()?;
        let old_last = old.get(last as usize)?;
        let (goal_pt, goal_dir) = old_last.point_at(car.s_end);
        let mut goals: Vec<(LaneId, f32)> = Vec::new();
        if let Some(g) = same(last) {
            goals.push((g, car.s_end));
        }
        for (g, gs, _) in self.net.lanes_near(goal_pt, goal_dir, GOAL_REACH).into_iter().take(6) {
            if !goals.iter().any(|&(x, _)| x == g) {
                goals.push((g, gs));
            }
        }
        goals.into_iter().find_map(|(g, gs)| self.net.route_lanes(lane, s, g, gs).map(|(r, _, s1)| (r, s1)))
    }

    /// Before the roads are replaced: note where each player arrow rule
    /// stands (its junction's position and the way its street leaves it),
    /// so `rebuild` can find it again on the renumbered network.
    pub fn anchor_lane_use(&mut self, roads: &RoadNetwork) {
        for r in &mut self.lane_use {
            if let Some((at, away)) = approach_geometry(roads, r.segment, r.node) {
                r.at = Some(at);
                r.away = Some(away);
            }
        }
    }

    /// Put the player's arrow rules on the approaches they belong to after
    /// a rebuild: found by geometry when anchored (by id for a rule from a
    /// save that predates anchoring, which loads onto the same network it
    /// was made on). A rule is kept only while it still fits: the same
    /// number of lanes, every lane with a movement the junction offers,
    /// every exit reachable (the checks `set_lane_use` makes). A rule that
    /// fits nowhere is dropped and the approach goes back to defaults.
    pub(super) fn resolve_lane_use(&mut self, roads: &RoadNetwork) {
        let rules = std::mem::take(&mut self.lane_use);
        for mut r in rules {
            let found = match (r.at, r.away) {
                (Some(at), Some(away)) => self.net.approaches.iter().position(|a| {
                    approach_geometry(roads, a.segment, a.node).is_some_and(|(p, d)| p.distance(at) < 1.0 && d.dot(away) > 0.98)
                }),
                _ => self.net.approaches.iter().position(|a| a.segment == r.segment && a.node == r.node),
            };
            let Some(ai) = found else { continue };
            let ap = &self.net.approaches[ai];
            let fits = ap.lanes.len() == r.arrows.len() && r.arrows.iter().all(|&x| x & ap.exits != 0) && r.arrows.iter().fold(0, |acc, &x| acc | x) & ap.exits == ap.exits;
            if !fits || self.lane_use.iter().any(|o| o.segment == ap.segment && o.node == ap.node) {
                continue;
            }
            r.segment = ap.segment;
            r.node = ap.node;
            if let Some((at, away)) = approach_geometry(roads, ap.segment, ap.node) {
                r.at = Some(at);
                r.away = Some(away);
            }
            self.net.approaches[ai].fixed = true;
            self.net.set_arrows(ai, r.arrows.clone());
            self.lane_use.push(r);
        }
    }
}

/// Where an approach is: its junction's position, and the unit direction
/// its street leaves the junction in.
fn approach_geometry(roads: &RoadNetwork, segment: SegmentId, node: NodeId) -> Option<(Vec2, Vec2)> {
    let seg = roads.segments.get(segment.0 as usize).filter(|s| s.alive)?;
    let at = roads.nodes.get(node.0 as usize)?.pos;
    let far = if seg.a == node {
        seg.b
    } else if seg.b == node {
        seg.a
    } else {
        return None;
    };
    let away = (roads.nodes.get(far.0 as usize)?.pos - at).normalize_or_zero();
    (away != Vec2::ZERO).then_some((at, away))
}
