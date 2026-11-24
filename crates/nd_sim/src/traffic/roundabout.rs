//! Roundabouts in the lane network: the ring is a one-way street whose
//! lanes run at about 25 km/h; at every junction on it the ring has
//! priority and the arms give way.
//!
//! - Entries yield with gap acceptance: a car at the give-way line goes only
//!   when nobody is in the ring's connector across its mouth and no ring
//!   vehicle that will pass the entry (one leaving here does not count) is
//!   less than `RING_CRITICAL_GAP` seconds away, looking back round the ring.
//! - Ring traffic never yields; it only follows a car that has merged ahead
//!   of it onto the same lane.
//! - No signals, ever: a junction stamped over becomes plain give-way.
//! - Multi-lane rings (two or three lanes): arms on the outer perimeter
//!   join and leave by the outer two lanes, arms joining from the open
//!   middle of the ring (roads inside it) by the inner two; every ring lane
//!   may continue (lane arrows). [r17-roundabouts] Lane choice by exit, as
//!   drivers are taught: passing an arm in a lane on that arm's side costs
//!   routing a little (more the further out), and entering or leaving by a
//!   lane further in costs `RING_INNER_LANE` each way, so a car bound for
//!   the first exit keeps to the outside and one bound for a later exit or
//!   going all the way round takes the inner lane in and out. A car
//!   leaving from the inner lane crosses the outer lane: it gives way to
//!   moving outer-lane traffic there (`ring_wait`); entering traffic gives
//!   way to every lane.
//!
//! Kept apart from the car-following code in `traffic.rs`: the rebuild calls
//! `apply_roundabouts`, routing reads `RingLanes::penalty`, and `lookahead`
//! asks `ring_wait` before a car enters a connector at a ring node.

use super::*;
use std::collections::HashSet;

/// Speed on the smallest rings, m/s (about 25 km/h); bigger rings are
/// faster (`Ring::speed`).
pub const RING_SPEED: f32 = 7.0;
/// Speed through an entry connector, m/s.
const RING_ENTRY_SPEED: f32 = 6.0;
/// A ring vehicle closer than this many seconds to an entry blocks it.
pub const RING_CRITICAL_GAP: f32 = 3.0;
/// How far back round the ring an entering driver looks at least, metres;
/// on a fast ring as far as a car covers in the time it judges.
const RING_LOOKBACK: f32 = 45.0;
/// Routing cost of passing an arm one lane in from the ring's side of it
/// (the outer lane of a two-lane ring past an outer arm; the middle lane
/// of a three-lane ring costs this, its outer lane twice this). A lane
/// change costs `LANE_CHANGE_COST`: first and second exits stay outside,
/// later ones go round further in.
const RING_OUTER_THROUGH: f32 = 5.0;
/// [r17-roundabouts] Routing cost of entering, and again of leaving, a
/// multi-lane ring by a lane one further in from the arm's side than the
/// outer lane (the inner lane of a two-lane ring). With the lane's shorter
/// way round and `RING_OUTER_THROUGH` per arm passed outside: first exits
/// from the outer lane, later exits and U-turns from the inner.
const RING_INNER_LANE: f32 = 4.0;
/// [r17-roundabouts] A car leaving by a lane further in waits for a
/// moving car in a lane it crosses less than this many seconds from the
/// exit.
const RING_EXIT_GAP: f32 = 2.0;
/// Seconds a ring car stands behind a ring that does not move before it
/// gets off at the arm it is at (`Traffic::bail_off_ring`).
const RING_BAIL_AFTER: f32 = 20.0;

/// The lanes of every roundabout, found when the lane network is rebuilt.
#[derive(Clone, Debug, Default)]
pub struct RingLanes {
    /// Every node on a ring (junctions and plain bends).
    pub nodes: HashMap<NodeId, RingNode>,
    /// Ring road lanes and the connectors between them.
    pub circulating: HashSet<LaneId>,
    /// Extra routing cost per lane (empty when there are no rings).
    penalty: Vec<f32>,
}

/// One node of a ring and its connectors by movement.
#[derive(Clone, Debug, Default)]
pub struct RingNode {
    /// Index into `RoadNetwork::rings`.
    pub ring: usize,
    /// Ring lane to ring lane, across the junction.
    pub through: Vec<LaneId>,
    /// Arm onto the ring.
    pub entries: Vec<LaneId>,
    /// Ring onto an arm.
    pub exits: Vec<LaneId>,
    /// Ring lanes arriving at the node.
    pub ring_in: Vec<LaneId>,
    /// How far back round the ring entering drivers look, metres.
    pub lookback: f32,
}

impl RingLanes {
    /// Extra routing cost of driving onto `lane`.
    pub fn penalty(&self, lane: LaneId) -> f32 {
        self.penalty.get(lane as usize).copied().unwrap_or(0.0)
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

impl LaneNetwork {
    /// Turn the lanes of every roundabout into one: ring speed, give-way
    /// entries, no signals, lane arrows for the ring and its arms.
    pub(super) fn apply_roundabouts(&mut self, roads: &RoadNetwork) {
        self.rings = RingLanes::default();
        if roads.rings.is_empty() {
            return;
        }
        let mut ring_lane = vec![false; self.lanes.len()];
        for (i, lane) in self.lanes.iter_mut().enumerate() {
            if let LaneKind::Road { segment, .. } = lane.kind {
                if let Some(r) = roads.ring_of_segment(segment) {
                    lane.set_limit(roads.rings[r].speed());
                    ring_lane[i] = true;
                    self.rings.circulating.insert(i as LaneId);
                }
            }
        }
        if self.rings.circulating.is_empty() {
            return;
        }
        let mut penalty = vec![0.0f32; self.lanes.len()];
        // Arms by the side of the ring they come from: an arm whose far
        // end lies inside the circle joins from the ring's open middle.
        let inside = |p: Vec2, r: &crate::roads::Ring| p.distance(r.center) < r.radius;
        for k in 0..self.intersections.len() {
            let nid = self.intersections[k].node;
            let Some(ring) = roads.ring_of_node(nid) else { continue };
            let rg = roads.rings[ring];
            let lanes_on_ring = rg.lanes;
            // How many lanes in from an arm's side a ring lane is (0 for
            // the outer lane and an arm outside; 0 for the inner lane and
            // an arm in the middle).
            let lanes_in = |l: &Lane, middle: bool| match l.kind {
                LaneKind::Road { index, .. } => {
                    let i = index.min(lanes_on_ring - 1);
                    (if middle { i } else { lanes_on_ring - 1 - i }) as f32
                }
                _ => 0.0,
            };
            let speed = rg.speed();
            let lookback = (speed * 3.0 * RING_CRITICAL_GAP + 10.0).max(RING_LOOKBACK);
            let mut rn = RingNode { ring, lookback, ..Default::default() };
            // Which sides of the ring have arms here.
            let (mut outer_arm, mut inner_arm) = (false, false);
            for &(conn, _) in &self.intersections[k].connectors {
                let c = conn as usize;
                let (Some(&inc), Some(&out)) = (self.lanes[c].prev.first(), self.lanes[c].next.first()) else { continue };
                for (l, on_ring) in [(inc, ring_lane[inc as usize]), (out, ring_lane[out as usize])] {
                    if !on_ring {
                        let lane = &self.lanes[l as usize];
                        let far = if lane.end_node == nid { lane.start() } else { lane.end() };
                        if inside(far, &rg) {
                            inner_arm = true;
                        } else {
                            outer_arm = true;
                        }
                    }
                }
            }
            for &(conn, _) in &self.intersections[k].connectors {
                let c = conn as usize;
                let (Some(&inc), Some(&out)) = (self.lanes[c].prev.first(), self.lanes[c].next.first()) else { continue };
                match (ring_lane[inc as usize], ring_lane[out as usize]) {
                    (true, true) => {
                        self.lanes[c].turn = 0;
                        self.lanes[c].set_limit(speed);
                        self.rings.circulating.insert(conn);
                        rn.through.push(conn);
                        // Lanes in from the outside (0 for the outer lane).
                        let index = match self.lanes[inc as usize].kind {
                            LaneKind::Road { index, .. } => index.min(lanes_on_ring - 1),
                            _ => 0,
                        };
                        let from_outside = (lanes_on_ring - 1 - index) as f32;
                        let from_inside = index as f32;
                        if outer_arm {
                            penalty[c] += RING_OUTER_THROUGH * from_inside;
                        }
                        if inner_arm {
                            penalty[c] += RING_OUTER_THROUGH * from_outside;
                        }
                    }
                    (false, true) => {
                        // Onto the ring: a right turn from outside, a left
                        // turn from the middle.
                        let from_middle = inside(self.lanes[inc as usize].start(), &rg);
                        self.lanes[c].turn = if from_middle { -1 } else { 1 };
                        self.lanes[c].set_limit(RING_ENTRY_SPEED);
                        penalty[c] += RING_INNER_LANE * lanes_in(&self.lanes[out as usize], from_middle);
                        rn.entries.push(conn);
                    }
                    (true, false) => {
                        let to_middle = inside(self.lanes[out as usize].end(), &rg);
                        self.lanes[c].turn = if to_middle { -1 } else { 1 };
                        self.lanes[c].set_limit(RING_SPEED);
                        penalty[c] += RING_INNER_LANE * lanes_in(&self.lanes[inc as usize], to_middle);
                        rn.exits.push(conn);
                    }
                    // Two arms meeting at one ring node do not cut across:
                    // marked straight on, which their arrows (right only
                    // outside, left only inside) never allow.
                    (false, false) => self.lanes[c].turn = 0,
                }
            }
            rn.ring_in = self.intersections[k].incoming.iter().map(|&(l, _)| l).filter(|&l| ring_lane[l as usize]).collect();
            self.intersections[k].signalized = false;
            self.rings.nodes.insert(nid, rn);
        }
        // Arrows: arms from outside only turn right onto the ring, arms
        // from the middle only left; every ring lane continues, the outer
        // two lanes may leave to the right (an outer arm), the inner two to
        // the left (an arm inside the ring). On a one-lane ring the lane
        // does all three. Fixed here: the junction arrows that adapt to the
        // counted turns (`Traffic::adapt_arrows`) leave ring nodes alone.
        for ai in 0..self.approaches.len() {
            if !self.rings.nodes.contains_key(&self.approaches[ai].node) {
                continue;
            }
            let mut lanes = self.approaches[ai].lanes.clone();
            lanes.sort_by_key(|&l| match self.lanes[l as usize].kind {
                LaneKind::Road { index, .. } => index,
                _ => 0,
            });
            let on_ring = lanes.first().is_some_and(|&l| ring_lane[l as usize]);
            let turns = self.exits_of(&lanes);
            let n = lanes.len();
            let arrows: Vec<u8> = if on_ring {
                (0..n)
                    .map(|i| {
                        let mut a = STRAIGHT & turns;
                        if i <= 1 {
                            a |= LEFT & turns;
                        }
                        if i + 2 >= n {
                            a |= RIGHT & turns;
                        }
                        if a == 0 { STRAIGHT } else { a }
                    })
                    .collect()
            } else {
                // An arm: its one way onto the ring (an arm both inside and
                // outside cannot be; right if in doubt).
                let way = if turns & LEFT != 0 && turns & RIGHT == 0 { LEFT } else { RIGHT };
                vec![way; n]
            };
            self.approaches[ai].exits = if on_ring { turns } else { arrows[0] };
            self.approaches[ai].arrows.clear();
            self.set_arrows(ai, arrows);
        }
        self.rings.penalty = penalty;
        self.arrows_version += 1;
    }
}

/// What a car about to enter a connector at a ring node must do.
pub(super) enum RingCall {
    /// Keep this gap to a stop or a leader.
    Wait(f32, f32, Wait),
    /// Go on: the generic junction checks (give way to anyone in the box,
    /// is the exit full) do not apply on a roundabout.
    Go,
}

impl Traffic {
    /// Before a car on a road lane enters connector `conn` at a ring node,
    /// `dist` metres ahead: give way, follow a car merging ahead, or go.
    pub(super) fn ring_wait(&self, car: &Car, conn: LaneId, dist: f32) -> RingCall {
        let lanes = &self.net.lanes;
        let LaneKind::Connector { node, .. } = lanes[conn as usize].kind else { return RingCall::Go };
        let Some(rn) = self.net.rings.nodes.get(&node) else { return RingCall::Go };
        let stop = RingCall::Wait(dist - 1.0 + IDM_MIN_GAP, 0.0, Wait::Yield);
        let exit = lanes[conn as usize].next.first().copied();
        if rn.entries.contains(&conn) {
            // The lane joined is stopped right at its start: entering would
            // leave this car across the ring.
            let jammed = exit.and_then(|x| lanes[x as usize].cars.first()).is_some_and(|&f| {
                let c = &self.cars[f];
                c.v < 0.5 && c.s < c.length() + IDM_MIN_GAP + 1.0
            });
            if jammed {
                return RingCall::Wait(dist - 1.0 + IDM_MIN_GAP, 0.0, Wait::ExitFull);
            }
            // Gap acceptance, judged from when this car reaches the line.
            let reach = dist / car.v.max(1.5);
            return if self.ring_blocks_entry(rn, reach) { stop } else { RingCall::Go };
        }
        if rn.exits.contains(&conn) {
            // Leaving: only into room on the arm, as at any junction.
            if let (Some(x), Some(_)) = (exit, car.route.iter().position(|&l| l == conn)) {
                if self.exit_blocked(&lanes[conn as usize], x, false) {
                    return RingCall::Wait(dist - 1.0 + IDM_MIN_GAP, 0.0, Wait::ExitFull);
                }
            }
            // [r17-roundabouts] From a lane further in: across the outer
            // lane(s), giving way to whoever is coming along them.
            if self.inner_exit_blocked(rn, conn, dist / car.v.max(1.5)) {
                return stop;
            }
            return RingCall::Go;
        }
        if rn.through.contains(&conn) && !rn.entries.is_empty() {
            // Follow a car that merged ahead onto the lane this one joins.
            let ours = dist + lanes[conn as usize].length;
            let mut best: Option<(f32, f32)> = None;
            for &e in &rn.entries {
                if lanes[e as usize].next.first().copied() != exit {
                    continue;
                }
                for &c in &lanes[e as usize].cars {
                    let ec = &self.cars[c];
                    let theirs = lanes[e as usize].length - ec.s;
                    if theirs < ours {
                        let gap = (ours - theirs - ec.length()).max(0.0);
                        if best.is_none_or(|(g, _)| gap < g) {
                            best = Some((gap, ec.v));
                        }
                    }
                }
            }
            if let Some((gap, v)) = best {
                // A car already in our own connector may be closer still.
                if let Some(&f) = lanes[conn as usize].cars.first() {
                    let lead = &self.cars[f];
                    let g = dist + lead.s - lead.length();
                    if g < gap {
                        return RingCall::Wait(g, lead.v, Wait::Leader);
                    }
                }
                return RingCall::Wait(gap, v, Wait::Leader);
            }
        }
        RingCall::Go
    }

    /// A ring car standing at the end of a ring lane behind a ring that
    /// does not move leaves by the arm here when that arm has room, and
    /// finds its way from there. A one-lane ring packed solid (every car
    /// at a junction going on round, the ones that want to leave stuck
    /// behind them) never moves again: nobody can go on until somebody
    /// gets off.
    pub(super) fn bail_off_ring(&mut self, id: CarId) -> bool {
        let c = &self.cars[id];
        if c.stalled < RING_BAIL_AFTER || c.cooldown > 0.0 || c.entering || c.hold > 0.0 || c.wait != Wait::Leader || c.idx + 1 >= c.route.len() {
            return false;
        }
        let here = c.route[c.idx];
        if !self.net.rings.circulating.contains(&here) {
            return false;
        }
        let lane = &self.net.lanes[here as usize];
        if lane.is_connector() || lane.cars.last() != Some(&id) || lane.length - c.s > 6.0 {
            return false;
        }
        let next = c.route[c.idx + 1];
        let Some(rn) = self.net.rings.nodes.get(&lane.end_node) else { return false };
        if !rn.through.contains(&next) {
            return false;
        }
        let (goal, s_end, s) = (*c.route.last().expect("route[idx + 1] is there"), c.s_end, c.s);
        let exits: Vec<LaneId> = rn.exits.iter().copied().filter(|&x| self.net.lanes[x as usize].prev.contains(&here)).collect();
        for x in exits {
            let Some(&arm) = self.net.lanes[x as usize].next.first() else { continue };
            if self.exit_blocked(&self.net.lanes[x as usize], arm, false) {
                continue;
            }
            let Some((rest, _, _)) = self.net.route_lanes(arm, 0.0, goal, s_end) else { continue };
            let mut route = vec![here, x];
            route.extend(rest);
            let c = &mut self.cars[id];
            c.route = route;
            c.idx = 0;
            c.s = s;
            c.cooldown = REROUTE_LANE_CHANGE_AFTER;
            return true;
        }
        false
    }

    /// Would a car reaching the give-way line in `reach` seconds meet ring
    /// traffic there: a vehicle in the junction when it arrives, or one
    /// that will pass the entry (not leave by it) arriving less than a
    /// critical gap after it, or not yet past when it gets there.
    /// [r15-freight] (photo 2026-11-20 F12) Rings holding up traffic: per
    /// ring (`RoadNetwork::rings` index), the vehicles standing in queues
    /// whose head has given way at one of its entries for `min_stall`
    /// seconds or more, and that head's stall (the longest). For the
    /// `roundabout_over_capacity` alert and probes.
    pub fn ring_queues(&self, min_stall: f32) -> std::collections::BTreeMap<usize, (usize, f32)> {
        let mut out: std::collections::BTreeMap<usize, (usize, f32)> = std::collections::BTreeMap::new();
        if self.net.rings.nodes.is_empty() {
            return out;
        }
        for (id, c) in self.cars.iter() {
            if c.v >= 0.5 || c.hold > 0.0 || c.entering {
                continue;
            }
            let Some(&head) = self.jam_chain(id).cars.last() else { continue };
            let h = &self.cars[head];
            if h.stalled < min_stall || self.stop_reason(head) != StopReason::Roundabout {
                continue;
            }
            let Some(&next) = h.route.get(h.idx + 1) else { continue };
            let LaneKind::Connector { node, .. } = self.net.lanes[next as usize].kind else { continue };
            let Some(rn) = self.net.rings.nodes.get(&node) else { continue };
            let e = out.entry(rn.ring).or_default();
            e.0 += 1;
            e.1 = e.1.max(h.stalled);
        }
        out
    }

    /// [r17-roundabouts] Must a car leaving the ring by exit `conn`, from
    /// a lane further in than the arm's side, wait for traffic in the
    /// lanes it crosses: a moving car in the junction from one of them, or
    /// a moving one there arriving within `RING_EXIT_GAP` of this car (one
    /// standing still lets it out).
    fn inner_exit_blocked(&self, rn: &RingNode, conn: LaneId, reach: f32) -> bool {
        let lanes = &self.net.lanes;
        let c = &lanes[conn as usize];
        let Some(&from) = c.prev.first() else { return false };
        let LaneKind::Road { index: mine, .. } = lanes[from as usize].kind else { return false };
        // Right (an outer arm): the lanes outside this one; left: inside.
        let crosses = |l: LaneId| match lanes[l as usize].kind {
            LaneKind::Road { index, .. } => {
                if c.turn >= 0 {
                    index > mine
                } else {
                    index < mine
                }
            }
            _ => false,
        };
        if reach > 2.0 * RING_CRITICAL_GAP || !rn.ring_in.iter().any(|&l| crosses(l)) {
            return false;
        }
        if reach < RING_CRITICAL_GAP {
            for &k in rn.through.iter().chain(rn.exits.iter()) {
                let kl = &lanes[k as usize];
                if k != conn && kl.prev.first().is_some_and(|&p| crosses(p)) && kl.cars.iter().any(|&o| self.cars[o].v >= 1.0) {
                    return true;
                }
            }
        }
        // Back along the lanes crossed (and the connectors between their
        // pieces), as far as a car covers in the time judged.
        let mut stack: Vec<(LaneId, f32)> = rn.ring_in.iter().filter(|&&l| crosses(l)).map(|&l| (l, 0.0)).collect();
        let mut seen: HashSet<LaneId> = HashSet::new();
        while let Some((l, off)) = stack.pop() {
            if !seen.insert(l) {
                continue;
            }
            let lane = &lanes[l as usize];
            for &o in &lane.cars {
                let car = &self.cars[o];
                if car.v < 1.0 {
                    continue;
                }
                // Coming up to the exit before this car is out, or within
                // the gap after it.
                if (off + lane.length - car.s) / car.v < reach + RING_EXIT_GAP {
                    return true;
                }
            }
            let further = off + lane.length;
            if further > RING_LOOKBACK * 0.5 {
                continue;
            }
            for &p in &lane.prev {
                let pl = &lanes[p as usize];
                if self.net.rings.circulating.contains(&p) && (pl.is_connector() || crosses(p)) {
                    stack.push((p, further));
                }
            }
        }
        false
    }

    fn ring_blocks_entry(&self, rn: &RingNode, reach: f32) -> bool {
        let lanes = &self.net.lanes;
        // Too far off to judge: decide nearer the line.
        if reach > 2.0 * RING_CRITICAL_GAP {
            return false;
        }
        if reach < RING_CRITICAL_GAP && rn.through.iter().any(|&t| !lanes[t as usize].cars.is_empty()) {
            return true;
        }
        let mut stack: Vec<(LaneId, f32)> = rn.ring_in.iter().map(|&l| (l, 0.0)).collect();
        let mut seen: HashSet<LaneId> = HashSet::new();
        while let Some((l, off)) = stack.pop() {
            if !seen.insert(l) {
                continue;
            }
            let lane = &lanes[l as usize];
            for &c in &lane.cars {
                let car = &self.cars[c];
                let d = off + lane.length - car.s;
                let arrive = d / car.v.max(2.0);
                // Past the line before this car gets there, or far enough
                // behind to leave a gap: no conflict.
                if arrive + 1.0 < reach || arrive > reach + RING_CRITICAL_GAP {
                    continue;
                }
                let passes = car.route.iter().skip(car.idx).take(64).any(|x| rn.through.contains(x));
                if passes {
                    return true;
                }
            }
            let further = off + lane.length;
            if further > rn.lookback {
                continue;
            }
            for &p in &lane.prev {
                if self.net.rings.circulating.contains(&p) {
                    stack.push((p, further));
                }
            }
        }
        false
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::citizens::CitizenId;
    use crate::plan::{Gesture, GestureId, Plan};
    use crate::roads::{RoadKind, Ring};

    /// Two streets crossing at the origin, `arm_lanes` each way, with a
    /// roundabout stamped on the junction when `ring` is given.
    fn city(arm_lanes: u8, ring: Option<(f32, u8)>) -> RoadNetwork {
        let mut plan = Plan::default();
        let road = |a: Vec2, b: Vec2| Gesture::Road { points: vec![a, b], lanes_fwd: arm_lanes, lanes_bwd: arm_lanes, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None };
        plan.gestures.insert(GestureId(1), road(Vec2::new(-320.0, 0.0), Vec2::new(320.0, 0.0)));
        plan.gestures.insert(GestureId(2), road(Vec2::new(0.0, -320.0), Vec2::new(0.0, 320.0)));
        if let Some((radius, lanes)) = ring {
            plan.gestures.insert(GestureId(3), Gesture::Roundabout { center: Vec2::ZERO, radius, lanes, level: 0.0 });
        }
        let (built, rings) = plan.built_roads();
        let mut net = RoadNetwork::default();
        net.rings = rings;
        for (_, pts, f, b, k) in built {
            net.add_polyline_kind(&pts, f, b, k);
        }
        net
    }

    const ARMS: [Vec2; 4] = [Vec2::X, Vec2::Y, Vec2::NEG_X, Vec2::NEG_Y];

    /// Start of inbound lane `k` (0 leftmost) of an arm, 280 m out, and the
    /// end of its outbound kerb-side lane, 260 m out.
    fn arm_points(u: Vec2, lanes: u8, k: u8) -> (Vec2, Vec2) {
        // Right of travel is (-d.y, d.x); inbound travel is -u.
        let inbound_right = Vec2::new(u.y, -u.x);
        let from_centre = (k as f32 + 0.5) * LANE_WIDTH;
        (u * 280.0 + inbound_right * from_centre, u * 260.0 - inbound_right * (lanes as f32 - 0.5) * LANE_WIDTH)
    }

    fn send(t: &mut Traffic, roads: &RoadNetwork, from: Vec2, to: Vec2) -> CarId {
        let (fs, _, _) = roads.nearest_street(from).unwrap();
        let (ts, _, _) = roads.nearest_street(to).unwrap();
        let (route, s0, s1) = t.net.find_route(fs, from, ts, to).expect("route");
        t.spawn_car(CitizenId::default(), None, route, s0, s1, 0)
    }

    #[test]
    fn a_stamped_four_way_becomes_a_give_way_ring_with_four_entries() {
        let roads = city(1, Some((18.0, 1)));
        assert_eq!(roads.rings.len(), 1);
        assert!(roads.node_near(Vec2::ZERO, 10.0).is_none(), "the old junction is gone");
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let entries: usize = t.net.rings.nodes.values().map(|n| n.entries.len()).sum();
        let exits: usize = t.net.rings.nodes.values().map(|n| n.exits.len()).sum();
        assert_eq!(entries, 4, "one entry per arm");
        assert_eq!(exits, 4, "one exit per arm");
        assert_eq!(t.net.rings.nodes.values().filter(|n| !n.entries.is_empty()).count(), 4, "four junctions on the ring");
        assert!(t.net.intersections.iter().all(|i| !i.signalized), "no signals anywhere");
        // Ring pieces are one-way, at ring speed, with the island on the left.
        let ring = roads.rings[0];
        for &l in &t.net.rings.circulating {
            let lane = &t.net.lanes[l as usize];
            assert!(lane.speed_limit <= ring.speed() + 1e-3);
            if let LaneKind::Road { .. } = lane.kind {
                let mid = (lane.start() + lane.end()) * 0.5;
                let d = (lane.end() - lane.start()).normalize();
                assert!(d.dot(ring.travel_dir(mid)) > 0.8, "runs round the ring the right way");
                let right = Vec2::new(-d.y, d.x);
                assert!(right.dot(mid - ring.center) > 0.0, "the island is on the left of the driver");
            }
        }
        // Arms only turn onto the ring; ring lanes continue or leave.
        for ap in t.net.approaches.iter().filter(|a| t.net.rings.nodes.contains_key(&a.node)) {
            let ring_lane = t.net.rings.circulating.contains(&ap.lanes[0]);
            assert_eq!(ap.arrows, if ring_lane { vec![STRAIGHT | RIGHT] } else { vec![RIGHT] }, "{ap:?}");
        }
        // Every arm reaches every other arm, round the ring.
        for a in ARMS {
            for b in ARMS {
                if a != b {
                    let (from, _) = arm_points(a, 1, 0);
                    let (_, to) = arm_points(b, 1, 0);
                    let (fs, _, _) = roads.nearest_street(from).unwrap();
                    let (ts, _, _) = roads.nearest_street(to).unwrap();
                    let (route, ..) = t.net.find_route(fs, from, ts, to).expect("route round the ring");
                    assert!(route.iter().any(|l| t.net.rings.circulating.contains(l)));
                }
            }
        }
    }

    #[test]
    fn an_entering_car_gives_way_to_the_ring() {
        let roads = city(1, Some((18.0, 1)));
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let east_node = t.net.rings.nodes.values().find(|n| n.entries.iter().any(|&e| t.net.lanes[e as usize].start().x > 10.0)).unwrap().clone();
        let (east, _) = arm_points(Vec2::X, 1, 0);
        let (_, west) = arm_points(Vec2::NEG_X, 1, 0);
        let (_, south_end) = arm_points(Vec2::NEG_Y, 1, 0);
        // A ring car from the north passes the east entry on its way south;
        // cars from the east start at a range of distances. Whenever one
        // meets the ring car at the entry, the ring car goes first.
        let mut met = 0;
        for k in 0..24 {
            let start = 30.0 + 5.0 * k as f32;
            let mut t = t.clone();
            let (north, _) = arm_points(Vec2::Y, 1, 0);
            let ring_car = send(&mut t, &roads, north * (40.0 / 280.0), south_end);
            let entering = send(&mut t, &roads, east * (start / 280.0), west);
            let mut yielded = false;
            let mut ring_passed_at = None;
            let mut entered_at = None;
            for step in 0..400 {
                t.step(0.5, 2);
                let c = &t.cars[entering];
                yielded |= c.wait == Wait::Yield;
                if entered_at.is_none() && east_node.entries.contains(&c.route[c.idx]) {
                    entered_at = Some(step);
                }
                let r = &t.cars[ring_car];
                if ring_passed_at.is_none() && east_node.through.iter().any(|x| r.route[..=r.idx].contains(x)) {
                    ring_passed_at = Some(step);
                }
                if entered_at.is_some() && ring_passed_at.is_some() {
                    break;
                }
            }
            let (p, e) = (ring_passed_at.expect("the ring car went past"), entered_at.expect("the entering car went"));
            if yielded {
                met += 1;
                assert!(p <= e, "start {start}: the ring car passed the entry first ({p} vs {e})");
            }
        }
        assert!(met > 0, "at least one meeting at the entry made the entering car give way");
    }

    /// Cars from every arm to every other arm, two of every `every` ticks
    /// of 2 s, for ten minutes. Returns (spawned, arrived, mean trip
    /// seconds, the final positions to check runs are identical).
    fn load(roads: &RoadNetwork, arm_lanes: u8, every: u32) -> (usize, usize, f32, Vec<(Vec2, f32)>) {
        let mut t = Traffic::default();
        t.rebuild(roads);
        let mut spawned = 0;
        let mut arrived = 0;
        let mut trip = 0.0;
        let mut seed = 7u32;
        for tick in 0..1500u32 {
            if tick < 300 && tick % every.max(1) == 0 {
                for (a, &u) in ARMS.iter().enumerate() {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let b = (a + 1 + (seed >> 24) as usize % 3) % 4;
                    let k = ((seed >> 16) % arm_lanes as u32) as u8;
                    let (from, _) = arm_points(u, arm_lanes, k);
                    let (_, to) = arm_points(ARMS[b], arm_lanes, 0);
                    send(&mut t, roads, from, to);
                    spawned += 1;
                }
            }
            for id in t.step(2.0, 4) {
                if t.cars[id].age < super::REPLAN_AFTER {
                    arrived += 1;
                    trip += t.cars[id].age;
                }
                t.remove_car(id);
            }
            if tick > 300 && t.cars.is_empty() {
                break;
            }
        }
        let rest = t.cars.values().map(|c| (c.pos, c.v)).collect();
        (spawned, arrived, trip / arrived.max(1) as f32, rest)
    }

    #[test]
    fn moderate_load_flows_through_without_deadlock_and_runs_the_same_twice() {
        let roads = city(1, Some((18.0, 1)));
        // 4 arms, a car every 8 s each: 1800 vehicles an hour into the ring.
        let (spawned, arrived, ring_trip, rest) = load(&roads, 1, 4);
        assert_eq!(arrived, spawned, "everyone gets through, nobody times out");
        let again = load(&roads, 1, 4);
        assert_eq!((again.0, again.1), (spawned, arrived));
        assert_eq!(again.3, rest, "deterministic");
        assert!((again.2 - ring_trip).abs() < 1e-3);
        // The same demand through the signalled crossroads, for the record.
        let signal = city(1, None);
        let (s2, a2, signal_trip, _) = load(&signal, 1, 4);
        assert_eq!(a2, s2);
        eprintln!("1800 veh/h, one-lane arms: roundabout mean trip {ring_trip:.1}s, signal {signal_trip:.1}s");
    }

    /// Throughput against a signal: `cargo test -p nd_sim --release -- --ignored --nocapture ring_throughput`.
    #[test]
    #[ignore]
    fn ring_throughput() {
        for every in [4u32, 3, 2] {
            for (label, lanes, ring) in [("ring r18 1-lane", 1u8, Some((18.0, 1u8))), ("signal 1+1", 1, None), ("ring r26 2-lane", 2, Some((26.0, 2))), ("signal 2+2", 2, None)] {
                let roads = city(lanes, ring);
                let t0 = std::time::Instant::now();
                let (spawned, arrived, trip, _) = load(&roads, lanes, every);
                eprintln!("{} veh/h offered, {label}: {arrived}/{spawned} arrived, mean trip {trip:.1}s ({:?})", 4 * 1800 / every, t0.elapsed());
            }
        }
    }

    #[test]
    fn on_two_lane_rings_later_exits_go_round_on_the_inside() {
        let roads = city(2, Some((26.0, 2)));
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let on_ring = |route: &[LaneId], index: u8| route.iter().filter(|&&l| t.net.rings.circulating.contains(&l) && matches!(t.net.lanes[l as usize].kind, LaneKind::Road { index: i, .. } if i == index)).count();
        let route = |from_arm: Vec2, k: u8, to_arm: Vec2| {
            let (from, _) = arm_points(from_arm, 2, k);
            let (_, to) = arm_points(to_arm, 2, 0);
            let (fs, _, _) = roads.nearest_street(from).unwrap();
            let (ts, _, _) = roads.nearest_street(to).unwrap();
            t.net.find_route(fs, from, ts, to).expect("route").0
        };
        // Travel runs clockwise in the sim plane: from the east arm the
        // first exit is the south (-y) arm, the third the north (+y).
        let first = route(Vec2::X, 1, Vec2::NEG_Y);
        // Later exits bound for the arm's left (inner) lane, which the
        // inner ring lane leads to (the kerb lane would cost a change).
        let to_left = |from_arm: Vec2, to_arm: Vec2| {
            let (from, _) = arm_points(from_arm, 2, 0);
            let to = to_arm * 260.0 - Vec2::new(to_arm.y, -to_arm.x) * 0.5 * LANE_WIDTH;
            let (fs, _, _) = roads.nearest_street(from).unwrap();
            let (ts, _, _) = roads.nearest_street(to).unwrap();
            t.net.find_route(fs, from, ts, to).expect("route").0
        };
        let second = to_left(Vec2::X, Vec2::NEG_X);
        let third = to_left(Vec2::X, Vec2::Y);
        assert_eq!(on_ring(&first, 0), 0, "first exit: outer lane only");
        assert!(on_ring(&first, 1) > 0);
        // [r17-roundabouts] Later exits: in by the inner lane, round on it
        // and out from it, no lane change on the ring.
        for (label, r) in [("second", &second), ("third", &third)] {
            assert!(on_ring(r, 0) > 0 && on_ring(r, 1) == 0, "{label} exit: round on the inside only ({} inner, {} outer)", on_ring(r, 0), on_ring(r, 1));
        }
        // Both lanes may leave the ring to an outer arm, and arms enter both.
        for n in t.net.rings.nodes.values() {
            let from: Vec<u8> = n.exits.iter().filter(|&&e| t.net.lanes[e as usize].permitted).filter_map(|&e| match t.net.lanes[t.net.lanes[e as usize].prev[0] as usize].kind {
                LaneKind::Road { index, .. } => Some(index),
                _ => None,
            }).collect();
            if !n.exits.is_empty() {
                assert!(from.contains(&0) && from.contains(&1), "exits from both lanes: {from:?}");
            }
        }
        // And a two-lane ring with two-lane arms carries a steady load.
        let (spawned, arrived, _, _) = load(&roads, 2, 4);
        assert_eq!(arrived, spawned);
    }

    /// [r17-roundabouts] A one-lane arm onto a two-lane ring: the first
    /// exit by the outer lane, later ones and the U-turn by the inner, in
    /// and out with no lane change on the ring.
    #[test]
    fn one_lane_arms_choose_their_ring_lane_by_exit() {
        let roads = city(1, Some((26.0, 2)));
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let ring_lanes = |route: &[LaneId]| -> Vec<u8> {
            route.iter().filter(|&&l| t.net.rings.circulating.contains(&l)).filter_map(|&l| match t.net.lanes[l as usize].kind {
                LaneKind::Road { index, .. } => Some(index),
                _ => None,
            }).collect()
        };
        let route = |from_arm: Vec2, to_arm: Vec2| {
            let (from, _) = arm_points(from_arm, 1, 0);
            let (_, to) = arm_points(to_arm, 1, 0);
            let (fs, _, _) = roads.nearest_street(from).unwrap();
            let (ts, _, _) = roads.nearest_street(to).unwrap();
            t.net.find_route(fs, from, ts, to).expect("route").0
        };
        for a in ARMS {
            // Clockwise in the sim plane: the first exit after arm `a`.
            let first = Vec2::new(-a.y, a.x) * -1.0;
            let first = ARMS.into_iter().find(|b| b.distance(Vec2::new(a.y, -a.x)) < 0.1).unwrap_or(first);
            let lanes = ring_lanes(&route(a, first));
            assert!(!lanes.is_empty() && lanes.iter().all(|&i| i == 1), "first exit from {a}: outer lane only, got {lanes:?}");
            for b in ARMS.into_iter().filter(|b| *b != a && *b != first) {
                let lanes = ring_lanes(&route(a, b));
                assert!(!lanes.is_empty() && lanes.iter().all(|&i| i == 0), "{a} to {b}: inner lane only, got {lanes:?}");
            }
        }
        let (spawned, arrived, _, rest) = load(&roads, 1, 4);
        assert_eq!(arrived, spawned, "left on the ring: {rest:?}");
    }

    /// [r17-roundabouts] A car leaving from the inner lane gives way to a
    /// moving car coming along the outer lane at about the same time; not
    /// to one standing, nor one far off; an outer-lane car never waits.
    #[test]
    fn an_inner_lane_car_leaving_gives_way_to_the_outer_lane() {
        let roads = city(1, Some((26.0, 2)));
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let index = |t: &Traffic, l: LaneId| match t.net.lanes[l as usize].kind {
            LaneKind::Road { index, .. } => index,
            _ => 9,
        };
        let rn = t.net.rings.nodes.values().find(|n| n.exits.len() >= 2).unwrap().clone();
        let x_in = *rn.exits.iter().find(|&&x| index(&t, t.net.lanes[x as usize].prev[0]) == 0).unwrap();
        let x_out = *rn.exits.iter().find(|&&x| index(&t, t.net.lanes[x as usize].prev[0]) == 1).unwrap();
        let l_in = t.net.lanes[x_in as usize].prev[0];
        let l_out = t.net.lanes[x_out as usize].prev[0];
        let arm = t.net.lanes[x_in as usize].next[0];
        let len_in = t.net.lanes[l_in as usize].length;
        let len_out = t.net.lanes[l_out as usize].length;
        let call = |outer_at: Option<(f32, f32)>| {
            let mut t = t.clone();
            let me = t.spawn_car(CitizenId::default(), None, vec![l_in, x_in, arm], len_in - 3.0, 10.0, 0);
            t.cars[me].v = 1.5;
            if let Some((back, v)) = outer_at {
                let o = t.spawn_car(CitizenId::default(), None, vec![l_out, x_out, arm], len_out - back, 10.0, 0);
                t.cars[o].v = v;
            }
            let car = t.cars[me].clone();
            matches!(t.ring_wait(&car, x_in, 3.0), RingCall::Wait(_, _, Wait::Yield))
        };
        assert!(!call(None), "nobody outside: out it goes");
        assert!(call(Some((len_out * 0.8, 6.0))), "an outer-lane car arriving with it: wait");
        assert!(!call(Some((len_out * 0.8, 0.0))), "an outer-lane car standing: go");
        assert!(!call(Some((len_out - 0.5, 0.5))), "one far back and slow: go");
        // From the outer lane nobody is crossed.
        let mut t2 = t.clone();
        let me = t2.spawn_car(CitizenId::default(), None, vec![l_out, x_out, arm], len_out - 3.0, 10.0, 0);
        t2.cars[me].v = 1.5;
        let o = t2.spawn_car(CitizenId::default(), None, vec![l_in, x_in, arm], len_in - 8.0, 10.0, 0);
        t2.cars[o].v = 6.0;
        let car = t2.cars[me].clone();
        assert!(!matches!(t2.ring_wait(&car, x_out, 3.0), RingCall::Wait(_, _, Wait::Yield)), "outer lane leaves freely");
    }

    #[test]
    fn rings_keep_their_limits_and_buses_and_lorries_go_round() {
        assert_eq!(Ring::new(Vec2::ZERO, 3.0, 0).radius, crate::roads::RING_MIN_RADIUS);
        assert_eq!(Ring::new(Vec2::ZERO, 300.0, 9).radius, crate::roads::RING_MAX_RADIUS);
        assert_eq!(Ring::new(Vec2::ZERO, 20.0, 9).lanes, crate::roads::RING_MAX_LANES);
        assert_eq!(Ring::new(Vec2::ZERO, 20.0, 0).lanes, 1);
        // The smallest ring still carries a bus and a lorry round.
        let roads = city(1, Some((1.0, 1)));
        assert_eq!(roads.rings[0].radius, crate::roads::RING_MIN_RADIUS);
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let (from, _) = arm_points(Vec2::X, 1, 0);
        let (_, to) = arm_points(Vec2::Y, 1, 0);
        let bus = send(&mut t, &roads, from, to);
        t.cars[bus].bus = Some(Bus { route: Default::default(), next_stop: 0, passengers: Vec::new(), to_depot: false, retiring: false, depot: Default::default() });
        let truck = send(&mut t, &roads, from * 0.8, to);
        t.cars[truck].cargo = Some(Cargo { from: Default::default(), to: Default::default(), resource: crate::market::Resource::Parts, amount: 1.0, price: 1.0 });
        let mut done = Vec::new();
        for _ in 0..400 {
            for id in t.step(1.0, 4) {
                t.remove_car(id);
                done.push(id);
            }
        }
        let state: Vec<String> = t.cars.iter().map(|(id, c)| format!("{id:?} {:?} v{:.1} s{:.1} idx{}/{} lane{} len{:.1} stalled{:.0} entering{}", c.wait, c.v, c.s, c.idx, c.route.len(), c.route[c.idx], t.net.lanes[c.route[c.idx] as usize].length, c.stalled, c.entering)).collect();
        assert!(done.contains(&bus) && done.contains(&truck), "bus and lorry made it round: {state:?}");
    }

    /// Two rings, one inside the other, joined by two short roads; the
    /// crossroads outside joins the outer ring.
    fn nested() -> RoadNetwork {
        let mut plan = Plan::default();
        let road = |a: Vec2, b: Vec2| Gesture::Road { points: vec![a, b], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None };
        plan.gestures.insert(GestureId(1), road(Vec2::new(-320.0, 0.0), Vec2::new(320.0, 0.0)));
        plan.gestures.insert(GestureId(2), road(Vec2::new(0.0, -320.0), Vec2::new(0.0, 320.0)));
        plan.gestures.insert(GestureId(3), Gesture::Roundabout { center: Vec2::ZERO, radius: 100.0, lanes: 1, level: 0.0 });
        plan.gestures.insert(GestureId(4), Gesture::Roundabout { center: Vec2::ZERO, radius: 30.0, lanes: 1, level: 0.0 });
        plan.gestures.insert(GestureId(5), road(Vec2::new(30.0, 0.0), Vec2::new(100.0, 0.0)));
        plan.gestures.insert(GestureId(6), road(Vec2::new(-30.0, 0.0), Vec2::new(-100.0, 0.0)));
        let (built, rings) = plan.built_roads();
        let mut net = RoadNetwork::default();
        net.rings = rings;
        for (_, pts, f, b, k) in built {
            net.add_polyline_kind(&pts, f, b, k);
        }
        net
    }

    #[test]
    fn traffic_flows_between_nested_rings_without_deadlock() {
        let roads = nested();
        assert_eq!(roads.rings.len(), 2);
        let mut t = Traffic::default();
        t.rebuild(&roads);
        assert!(t.net.intersections.iter().all(|i| !i.signalized));
        // Cars from the four arms to the links between the rings, and from
        // the links back out to the far arm.
        let links = [Vec2::new(65.0, 0.0), Vec2::new(-65.0, 0.0)];
        let mut spawned = 0;
        let mut arrived = 0;
        let mut rest = Vec::new();
        for run in 0..2 {
            let mut t = t.clone();
            let (mut s, mut a) = (0, 0);
            for tick in 0..900u32 {
                if tick < 240 && tick % 4 == 0 {
                    for (k, &u) in ARMS.iter().enumerate() {
                        let (from, _) = arm_points(u, 1, 0);
                        let link = links[(k + tick as usize / 4) % 2];
                        // Travelling inward on a link, toward the centre.
                        let inward = Vec2::new(-link.x.signum(), 0.0);
                        let right_in = Vec2::new(-inward.y, inward.x);
                        send(&mut t, &roads, from, link + right_in * 1.75);
                        send(&mut t, &roads, link - inward * 10.0 - right_in * 1.75, arm_points(ARMS[(k + 2) % 4], 1, 0).1);
                        s += 2;
                    }
                }
                for id in t.step(2.0, 4) {
                    if t.cars[id].age < super::REPLAN_AFTER {
                        a += 1;
                    }
                    t.remove_car(id);
                }
                assert!(t.gridlocks().is_empty(), "gridlock at {}s", tick * 2);
                if tick > 240 && t.cars.is_empty() {
                    break;
                }
            }
            let left: Vec<(Vec2, f32)> = t.cars.values().map(|c| (c.pos, c.v)).collect();
            if run == 0 {
                (spawned, arrived, rest) = (s, a, left);
            } else {
                assert_eq!((s, a), (spawned, arrived), "deterministic");
                assert_eq!(left, rest);
            }
        }
        assert_eq!(arrived, spawned, "everyone gets between the rings and out again");
    }

    #[test]
    fn a_240_m_ring_is_faster_and_flows() {
        let roads = city(1, Some((120.0, 1)));
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let lane = t.net.rings.circulating.iter().copied().find(|&l| !t.net.lanes[l as usize].is_connector()).unwrap();
        assert!(t.net.lanes[lane as usize].speed_limit > 12.0, "a big ring runs near street speed");
        assert!(t.net.rings.nodes.values().all(|n| n.lookback > 100.0), "and drivers look further back");
        let small = city(1, Some((12.0, 1)));
        let mut ts = Traffic::default();
        ts.rebuild(&small);
        assert!(ts.net.rings.circulating.iter().all(|&l| ts.net.lanes[l as usize].speed_limit <= RING_SPEED + 1e-3));
        let (spawned, arrived, _, _) = load(&roads, 1, 4);
        assert_eq!(arrived, spawned);
        let roads = city(2, Some((120.0, 2)));
        let (spawned, arrived, _, _) = load(&roads, 2, 4);
        assert_eq!(arrived, spawned);
    }

    #[test]
    fn a_three_lane_ring_is_a_ring_not_a_highway_and_carries_a_heavy_load() {
        let roads = city(2, Some((34.0, 3)));
        assert_eq!(roads.rings[0].lanes, 3);
        // One-way with three lanes, but a ring: it joins its arms and is
        // no highway (no grade separation, ring speed).
        let ring_pieces: Vec<_> = roads.alive_segments().filter(|(_, s)| s.ring).collect();
        assert!(!ring_pieces.is_empty());
        assert!(ring_pieces.iter().all(|(_, s)| !s.is_highway() && !s.ramp && s.lanes_fwd == 3));
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let entries: usize = t.net.rings.nodes.values().map(|n| n.entries.len()).sum();
        assert!(entries >= 4, "every arm enters the ring ({entries} entries)");
        assert!(t.net.intersections.iter().all(|i| !i.signalized));
        // Ring lanes: the outer two may continue or leave right, the inner
        // only continues; arms only turn right onto it.
        for ap in t.net.approaches.iter().filter(|a| t.net.rings.nodes.contains_key(&a.node)) {
            if t.net.rings.circulating.contains(&ap.lanes[0]) {
                assert_eq!(ap.arrows, vec![STRAIGHT, STRAIGHT | RIGHT, STRAIGHT | RIGHT], "{ap:?}");
            } else {
                assert!(ap.arrows.iter().all(|&a| a == RIGHT), "{ap:?}");
            }
        }
        // Later exits go further in.
        let on_ring = |route: &[LaneId], index: u8| route.iter().filter(|&&l| t.net.rings.circulating.contains(&l) && matches!(t.net.lanes[l as usize].kind, LaneKind::Road { index: i, .. } if i == index)).count();
        let route = |from_arm: Vec2, k: u8, to_arm: Vec2| {
            let (from, _) = arm_points(from_arm, 2, k);
            let (_, to) = arm_points(to_arm, 2, 0);
            let (fs, _, _) = roads.nearest_street(from).unwrap();
            let (ts, _, _) = roads.nearest_street(to).unwrap();
            t.net.find_route(fs, from, ts, to).expect("route").0
        };
        let first = route(Vec2::X, 1, Vec2::NEG_Y);
        let third = route(Vec2::X, 1, Vec2::Y);
        assert_eq!(on_ring(&first, 0) + on_ring(&first, 1), 0, "first exit: outer lane only");
        assert!(on_ring(&third, 0) + on_ring(&third, 1) > 0, "third exit: round further in");
        // Two lanes in from each of four arms, a car every 4 s per arm
        // (3600 vehicles an hour offered): nobody gridlocks, all arrive,
        // and the run is deterministic.
        let (spawned, arrived, trip, rest) = load(&roads, 2, 2);
        assert_eq!(arrived, spawned, "everyone gets through the three-lane ring");
        let again = load(&roads, 2, 2);
        assert_eq!((again.1, again.3), (arrived, rest), "deterministic");
        eprintln!("three-lane ring, 3600 veh/h: mean trip {trip:.1}s");
    }

    /// The crossroads with a three-lane ring of radius 60, and a street
    /// drawn afterwards from inside the ring's open middle out to it.
    fn ring_with_inner_arm() -> (RoadNetwork, Vec2, Vec2) {
        let mut plan = Plan::default();
        let road = |a: Vec2, b: Vec2| Gesture::Road { points: vec![a, b], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None };
        plan.gestures.insert(GestureId(1), road(Vec2::new(-320.0, 0.0), Vec2::new(320.0, 0.0)));
        plan.gestures.insert(GestureId(2), road(Vec2::new(0.0, -320.0), Vec2::new(0.0, 320.0)));
        plan.gestures.insert(GestureId(3), Gesture::Roundabout { center: Vec2::ZERO, radius: 60.0, lanes: 3, level: 0.0 });
        let dir = Vec2::new(-1.0, -1.0).normalize();
        let (inner_end, on_ring) = (dir * 15.0, dir * 60.0);
        plan.gestures.insert(GestureId(4), road(inner_end, on_ring));
        let (built, rings) = plan.built_roads();
        let mut net = RoadNetwork::default();
        net.rings = rings;
        for (_, pts, f, b, k) in built {
            net.add_polyline_kind(&pts, f, b, k);
        }
        (net, inner_end, on_ring)
    }

    #[test]
    fn arms_inside_a_ring_join_and_leave_by_the_inner_lane() {
        let (roads, inner_end, on_ring) = ring_with_inner_arm();
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let node = t.net.rings.nodes.iter().find(|(id, _)| roads.node(**id).pos.distance(on_ring) < 3.0).map(|(_, n)| n.clone()).expect("the inner street joins the ring");
        let ring_index = |l: LaneId| match t.net.lanes[l as usize].kind {
            LaneKind::Road { index, .. } => index,
            _ => 99,
        };
        // Entering from the middle: a left turn into the inner two lanes
        // ([r17-roundabouts] the one-lane arm reaches the next lane out too).
        let entries: Vec<LaneId> = node.entries.iter().copied().filter(|&e| t.net.lanes[e as usize].permitted).collect();
        assert!(!entries.is_empty());
        assert!(entries.iter().all(|&e| t.net.lanes[e as usize].turn == -1 && ring_index(t.net.lanes[e as usize].next[0]) <= 1), "into the inner lanes");
        // Leaving for the middle: only from the inner two lanes.
        let exits: Vec<LaneId> = node.exits.iter().copied().filter(|&e| t.net.lanes[e as usize].permitted).collect();
        assert!(!exits.is_empty());
        assert!(exits.iter().all(|&e| ring_index(t.net.lanes[e as usize].prev[0]) <= 1), "out of the inner lanes");
        // And cars get in and out: from the east arm to the inner street
        // and from it to the north arm, round the ring.
        let inward = (on_ring - inner_end).normalize();
        let right = Vec2::new(-inward.y, inward.x);
        let into = inner_end + inward * 20.0 - right * 1.75;
        let out_of = inner_end + inward * 10.0 + right * 1.75;
        let mut cars = Vec::new();
        for k in 0..6 {
            let (east, _) = arm_points(Vec2::X, 1, 0);
            cars.push(send(&mut t, &roads, east * (0.6 + 0.05 * k as f32), into));
            cars.push(send(&mut t, &roads, out_of - inward * k as f32, arm_points(Vec2::Y, 1, 0).1));
        }
        let mut done = 0;
        for _ in 0..600 {
            for id in t.step(1.0, 4) {
                t.remove_car(id);
                done += 1;
            }
            assert!(t.gridlocks().is_empty());
        }
        assert_eq!(done, cars.len(), "every car got in and out of the middle");
    }

    /// A ring car held up behind a ring that does not move gets off at the
    /// arm it is at when that arm has room, and finds its way from there.
    #[test]
    fn a_car_behind_a_ring_that_does_not_move_gets_off_at_the_arm() {
        let roads = city(1, Some((30.0, 1)));
        let mut t = Traffic::default();
        t.rebuild(&roads);
        // A ring junction with an exit, the ring lane arriving there, the
        // ring going on and the arm leaving.
        let (node, rn) = t.net.rings.nodes.iter().filter(|(_, n)| !n.exits.is_empty() && !n.through.is_empty()).min_by_key(|(id, _)| id.0).map(|(id, n)| (*id, n.clone())).unwrap();
        let here = rn.ring_in[0];
        let through = *t.net.lanes[here as usize].next.iter().find(|n| rn.through.contains(n)).unwrap();
        let exit = *t.net.lanes[here as usize].next.iter().find(|n| rn.exits.contains(n)).unwrap();
        let on = t.net.lanes[through as usize].next[0];
        // Bound for another arm, round the ring past this one.
        let goal = t.net.lanes.iter().enumerate().filter(|(i, l)| !l.is_connector() && l.next.is_empty() == false && !t.net.rings.circulating.contains(&(*i as LaneId)) && l.start().length() < 40.0 && l.end().length() > 200.0).map(|(i, _)| i as LaneId).find(|&g| {
            t.net.route_lanes(on, 0.0, g, 100.0).is_some_and(|(r, _, _)| !r.contains(&t.net.lanes[exit as usize].next[0]))
        }).expect("another arm out");
        let (rest, _, _) = t.net.route_lanes(on, 0.0, goal, 100.0).unwrap();
        let mut route = vec![here, through];
        route.extend(rest);
        let len = t.net.lanes[here as usize].length;
        let car = t.spawn_car(CitizenId::default(), None, route.clone(), len - 1.0, 100.0, 0);
        // The ring ahead stands (a car stopped right past the junction).
        let block = t.spawn_car(CitizenId::default(), None, route[2..].to_vec(), t.net.lanes[on as usize].length - 0.1, 100.0, 0);
        t.cars[block].hold = 10_000.0;
        // And one standing behind it across the junction.
        let tl = t.net.lanes[through as usize].length;
        let block2 = t.spawn_car(CitizenId::default(), None, route[1..].to_vec(), (tl - CAR_LENGTH - 0.5 - IDM_MIN_GAP).max(0.5), 100.0, 0);
        t.cars[block2].hold = 10_000.0;
        let _ = node;
        let mut left_by_exit = false;
        // Well before the 40 s after which a car at a full exit takes
        // another way (`REROUTE_AFTER`).
        for _ in 0..70 {
            for id in t.step(0.5, 1) {
                t.remove_car(id);
            }
            let Some(c) = t.cars.get(car) else { break };
            if c.route.contains(&exit) {
                left_by_exit = true;
                break;
            }
        }
        assert!(left_by_exit, "got off at the arm instead of standing behind the ring for good");
    }

    /// [r15-freight] The queue held at a ring's entry is counted against
    /// that ring, its longest stall with it; a queue held elsewhere is not.
    #[test]
    fn queues_held_at_a_ring_entry_are_counted_for_the_ring() {
        let roads = city(1, Some((18.0, 1)));
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let rn = t.net.rings.nodes.values().find(|n| n.entries.iter().any(|&e| t.net.lanes[e as usize].start().x > 10.0)).unwrap().clone();
        let (east, _) = arm_points(Vec2::X, 1, 0);
        let (_, west) = arm_points(Vec2::NEG_X, 1, 0);
        let head = send(&mut t, &roads, east * (60.0 / 280.0), west);
        let k = t.cars[head].route.iter().position(|l| rn.entries.contains(l)).unwrap() - 1;
        let old = t.cars[head].route[t.cars[head].idx];
        t.net.lanes[old as usize].cars.retain(|&c| c != head);
        let lane = t.cars[head].route[k];
        let len = t.net.lanes[lane as usize].length;
        {
            let c = &mut t.cars[head];
            c.idx = k;
            c.s = len - 1.0;
            c.v = 0.0;
            c.entering = false;
            c.wait = Wait::Yield;
            c.stalled = 300.0;
        }
        t.insert_sorted(lane, head);
        assert_eq!(t.stop_reason(head), StopReason::Roundabout);
        let q = t.ring_queues(120.0);
        assert_eq!(q.get(&rn.ring).map(|x| x.0), Some(1), "{q:?}");
        assert!((q[&rn.ring].1 - 300.0).abs() < 0.01);
        assert!(t.ring_queues(400.0).is_empty(), "not stood that long");
    }
}
