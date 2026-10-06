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
//! - Two-lane rings: from the outer lane cars may continue or leave, from
//!   the inner lane only continue (lane arrows), and passing an exit on the
//!   outer lane costs routing a little, so cars bound for a later exit take
//!   the inner lane and move out before their exit, as drivers are taught.
//!
//! Kept apart from the car-following code in `traffic.rs`: the rebuild calls
//! `apply_roundabouts`, routing reads `RingLanes::penalty`, and `lookahead`
//! asks `ring_wait` before a car enters a connector at a ring node.

use super::*;
use std::collections::HashSet;

/// Speed on the ring, m/s (about 25 km/h).
pub const RING_SPEED: f32 = 7.0;
/// Speed through an entry connector, m/s.
const RING_ENTRY_SPEED: f32 = 6.0;
/// A ring vehicle closer than this many seconds to an entry blocks it.
pub const RING_CRITICAL_GAP: f32 = 3.0;
/// How far back round the ring an entering driver looks, metres.
const RING_LOOKBACK: f32 = 45.0;
/// Routing cost of passing an exit in the outer lane of a two-lane ring
/// (a lane change costs `LANE_CHANGE_COST`): first and second exits stay
/// outside, later ones go round on the inside.
const RING_OUTER_THROUGH: f32 = 5.0;

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
}

impl RingLanes {
    /// Extra routing cost of driving onto `lane`.
    pub fn penalty(&self, lane: LaneId) -> f32 {
        self.penalty.get(lane as usize).copied().unwrap_or(0.0)
    }

    /// The ring node with this connector as an entry, if any.
    pub fn entry_node(&self, conn: LaneId) -> Option<NodeId> {
        self.nodes.iter().find(|(_, n)| n.entries.contains(&conn)).map(|(id, _)| *id)
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
                if roads.ring_of_segment(segment).is_some() {
                    lane.speed_limit = RING_SPEED;
                    ring_lane[i] = true;
                    self.rings.circulating.insert(i as LaneId);
                }
            }
        }
        if self.rings.circulating.is_empty() {
            return;
        }
        let mut penalty = vec![0.0f32; self.lanes.len()];
        for k in 0..self.intersections.len() {
            let nid = self.intersections[k].node;
            let Some(ring) = roads.ring_of_node(nid) else { continue };
            let lanes_on_ring = roads.rings[ring].lanes;
            let mut rn = RingNode { ring, ..Default::default() };
            let arms = self.intersections[k].approaches >= 3;
            for &(conn, _) in &self.intersections[k].connectors {
                let c = conn as usize;
                let (Some(&inc), Some(&out)) = (self.lanes[c].prev.first(), self.lanes[c].next.first()) else { continue };
                match (ring_lane[inc as usize], ring_lane[out as usize]) {
                    (true, true) => {
                        self.lanes[c].turn = 0;
                        self.lanes[c].speed_limit = RING_SPEED;
                        self.rings.circulating.insert(conn);
                        rn.through.push(conn);
                        let outer = matches!(self.lanes[inc as usize].kind, LaneKind::Road { index, .. } if lanes_on_ring > 1 && index + 1 == lanes_on_ring);
                        if outer && arms {
                            penalty[c] = RING_OUTER_THROUGH;
                        }
                    }
                    (false, true) => {
                        self.lanes[c].turn = 1;
                        self.lanes[c].speed_limit = RING_ENTRY_SPEED;
                        rn.entries.push(conn);
                    }
                    (true, false) => {
                        self.lanes[c].turn = 1;
                        self.lanes[c].speed_limit = RING_SPEED;
                        rn.exits.push(conn);
                    }
                    // Two arms meeting at one ring node do not cut across:
                    // marked as a left turn, which their arrows never allow.
                    (false, false) => self.lanes[c].turn = -1,
                }
            }
            rn.ring_in = self.intersections[k].incoming.iter().map(|&(l, _)| l).filter(|&l| ring_lane[l as usize]).collect();
            self.intersections[k].signalized = false;
            self.rings.nodes.insert(nid, rn);
        }
        // Arrows: arms only turn right onto the ring; ring lanes continue,
        // and leave from the outer lane.
        for ai in 0..self.approaches.len() {
            if !self.rings.nodes.contains_key(&self.approaches[ai].node) {
                continue;
            }
            let lanes = self.approaches[ai].lanes.clone();
            let on_ring = lanes.first().is_some_and(|&l| ring_lane[l as usize]);
            let exits = if on_ring {
                lanes.iter().flat_map(|&l| self.lanes[l as usize].next.iter()).fold(0u8, |acc, &c| acc | turn_bit(self.lanes[c as usize].turn))
            } else {
                RIGHT
            };
            self.approaches[ai].exits = exits;
            self.approaches[ai].arrows.clear();
            self.set_arrows(ai, default_arrows(lanes.len(), exits, Some(0.0), None));
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
            return match (exit, car.route.iter().position(|&l| l == conn)) {
                (Some(x), Some(_)) if self.exit_blocked(&lanes[conn as usize], x, false) => RingCall::Wait(dist - 1.0 + IDM_MIN_GAP, 0.0, Wait::ExitFull),
                _ => RingCall::Go,
            };
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

    /// Would a car reaching the give-way line in `reach` seconds meet ring
    /// traffic there: a vehicle in the junction when it arrives, or one
    /// that will pass the entry (not leave by it) arriving less than a
    /// critical gap after it, or not yet past when it gets there.
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
                let passes = car.route.iter().skip(car.idx).take(32).any(|x| rn.through.contains(x));
                if passes {
                    return true;
                }
            }
            let further = off + lane.length;
            if further > RING_LOOKBACK {
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
        let road = |a: Vec2, b: Vec2| Gesture::Road { points: vec![a, b], lanes_fwd: arm_lanes, lanes_bwd: arm_lanes, curved: false, kind: RoadKind::Street };
        plan.gestures.insert(GestureId(1), road(Vec2::new(-320.0, 0.0), Vec2::new(320.0, 0.0)));
        plan.gestures.insert(GestureId(2), road(Vec2::new(0.0, -320.0), Vec2::new(0.0, 320.0)));
        if let Some((radius, lanes)) = ring {
            plan.gestures.insert(GestureId(3), Gesture::Roundabout { center: Vec2::ZERO, radius, lanes });
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
            assert!(lane.speed_limit <= RING_SPEED + 1e-3);
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
        let third = route(Vec2::X, 0, Vec2::Y);
        assert_eq!(on_ring(&first, 0), 0, "first exit: outer lane only");
        assert!(on_ring(&first, 1) > 0);
        assert!(on_ring(&third, 0) > on_ring(&third, 1), "third exit: round on the inside ({} inner, {} outer)", on_ring(&third, 0), on_ring(&third, 1));
        // Only the outer lane may leave the ring.
        for n in t.net.rings.nodes.values() {
            for &e in &n.exits {
                let from = t.net.lanes[e as usize].prev[0];
                assert!(!t.net.lanes[e as usize].permitted || matches!(t.net.lanes[from as usize].kind, LaneKind::Road { index: 1, .. }), "exits from the outer lane only");
            }
        }
        // And a two-lane ring with two-lane arms carries a steady load.
        let (spawned, arrived, _, _) = load(&roads, 2, 4);
        assert_eq!(arrived, spawned);
    }

    #[test]
    fn rings_keep_their_limits_and_buses_and_lorries_go_round() {
        assert_eq!(Ring::new(Vec2::ZERO, 3.0, 0).radius, crate::roads::RING_MIN_RADIUS);
        assert_eq!(Ring::new(Vec2::ZERO, 300.0, 9).radius, crate::roads::RING_MAX_RADIUS);
        assert_eq!(Ring::new(Vec2::ZERO, 20.0, 9).lanes, 2);
        assert_eq!(Ring::new(Vec2::ZERO, 20.0, 0).lanes, 1);
        // The smallest ring still carries a bus and a lorry round.
        let roads = city(1, Some((1.0, 1)));
        assert_eq!(roads.rings[0].radius, crate::roads::RING_MIN_RADIUS);
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let (from, _) = arm_points(Vec2::X, 1, 0);
        let (_, to) = arm_points(Vec2::Y, 1, 0);
        let bus = send(&mut t, &roads, from, to);
        t.cars[bus].bus = Some(Bus { route: Default::default(), next_stop: 0, passengers: Vec::new(), to_depot: false, depot: Default::default() });
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
}
