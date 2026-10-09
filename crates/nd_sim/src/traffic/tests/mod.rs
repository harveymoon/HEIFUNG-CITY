//! Traffic tests, by topic; the road layouts and drivers they share.

use super::*;

mod driving;
mod following;
mod lane_change;
mod network;
mod routing;
mod signals;

pub(super) fn cross() -> RoadNetwork {
    let mut net = RoadNetwork::default();
    net.add_polyline(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
    net.add_polyline(&[Vec2::new(0.0, -200.0), Vec2::new(0.0, 200.0)], 1, 1);
    net
}

/// A two-by-two lane avenue east-west crossing a one-lane street.
pub(super) fn avenue_cross() -> RoadNetwork {
    let mut net = RoadNetwork::default();
    net.add_polyline(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 2, 2);
    net.add_polyline(&[Vec2::new(0.0, -400.0), Vec2::new(0.0, 400.0)], 1, 1);
    net
}

/// The road lane of `segment` running along `dir` with this index.
pub(super) fn lane_of(t: &Traffic, segment: SegmentId, dir: Vec2, index: u8) -> LaneId {
    *t.net.segment_lanes[&segment]
        .iter()
        .find(|&&l| {
            let lane = &t.net.lanes[l as usize];
            matches!(lane.kind, LaneKind::Road { index: i, .. } if i == index) && (lane.end() - lane.start()).normalize().dot(dir) > 0.9
        })
        .expect("lane")
}

/// Drive one car on its own until it arrives; returns the lanes it used
/// and the largest lateral offset seen.
pub(super) fn drive(t: &mut Traffic, car: CarId) -> (Vec<LaneId>, f32, bool) {
    let mut used = vec![t.cars[car].route[0]];
    let mut lateral: f32 = 0.0;
    for _ in 0..600 {
        let arrived = t.step(1.0, 4);
        let c = &t.cars[car];
        lateral = lateral.max(c.lateral.abs());
        assert!(c.pos.distance(c.prev_pos) < ROAD_SPEED * 1.0 + 1.0, "positions move smoothly");
        if used.last() != Some(&c.route[c.idx]) {
            used.push(c.route[c.idx]);
        }
        if arrived.contains(&car) {
            return (used, lateral, c.age < REPLAN_AFTER);
        }
    }
    (used, lateral, false)
}

/// Load a four-way signal from every arm with mixed turns for ten
/// minutes. Returns (spawned, arrived in time, arrival order).
pub(super) fn busy_junction() -> (usize, usize, Vec<CarId>) {
    let roads = avenue_cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let w = LANE_WIDTH;
    // (start point on an inbound lane, end point on the outbound lane) per arm.
    let arms = [
        (Vec2::new(-350.0, 1.5 * w), Vec2::new(-300.0, -1.5 * w)),
        (Vec2::new(350.0, -1.5 * w), Vec2::new(300.0, 1.5 * w)),
        (Vec2::new(-0.5 * w, -350.0), Vec2::new(0.5 * w, -300.0)),
        (Vec2::new(0.5 * w, 350.0), Vec2::new(-0.5 * w, 300.0)),
    ];
    let inner = [Vec2::new(0.0, -w), Vec2::new(0.0, w), Vec2::ZERO, Vec2::ZERO];
    let mut spawned = 0;
    let mut on_time = 0;
    let mut order = Vec::new();
    let mut max_stall: f32 = 0.0;
    for tick in 0..2400 {
        if tick < 300 && tick % 4 == 0 {
            for (a, &(from, _)) in arms.iter().enumerate() {
                let b = (a + 1 + (tick / 4 + a) % 3) % 4;
                let from = if (tick / 4) % 2 == 0 { from } else { from + inner[a] };
                let to = arms[b].1;
                let (fs, _, _) = roads.nearest_street(from).unwrap();
                let (ts, _, _) = roads.nearest_street(to).unwrap();
                let (route, s0, s1) = t.net.find_route(fs, from, ts, to).expect("route");
                t.spawn_car(CitizenId::default(), None, route, s0, s1, 0);
                spawned += 1;
            }
        }
        for id in t.step(2.0, 4) {
            if t.cars[id].age < REPLAN_AFTER {
                on_time += 1;
            }
            order.push(id);
            t.remove_car(id);
        }
        assert!(t.gridlocks().is_empty(), "gridlock at {}s", tick * 2);
        max_stall = t.cars.values().filter(|c| !c.entering).map(|c| c.stalled).fold(max_stall, f32::max);
        if tick > 300 && t.cars.is_empty() {
            break;
        }
    }
    assert!(max_stall < 180.0, "nobody in traffic stands still for minutes ({max_stall:.0}s)");
    (spawned, on_time, order)
}

/// One-lane streets crossing at a signal, long enough for queues.
pub(super) fn street_cross() -> RoadNetwork {
    let mut net = RoadNetwork::default();
    net.add_polyline(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    net.add_polyline(&[Vec2::new(0.0, -400.0), Vec2::new(0.0, 400.0)], 1, 1);
    net
}

/// Spawn a car between two points (each on the lane it should use).
pub(super) fn send(t: &mut Traffic, roads: &RoadNetwork, from: Vec2, to: Vec2) -> CarId {
    let (fs, _, _) = roads.nearest_street(from).unwrap();
    let (ts, _, _) = roads.nearest_street(to).unwrap();
    let (route, s0, s1) = t.net.find_route(fs, from, ts, to).expect("route");
    t.spawn_car(CitizenId::default(), None, route, s0, s1, 0)
}

/// Forty seconds of the westbound stream (and cross traffic) first, so
/// the oncoming lane is busy when the left turner arrives.
pub(super) fn warm_up(t: &mut Traffic, roads: &RoadNetwork, cross: bool) {
    let w = LANE_WIDTH;
    for tick in 0..40 {
        if tick % 2 == 0 {
            send(t, roads, Vec2::new(300.0, -0.5 * w), Vec2::new(-300.0, -0.5 * w));
        }
        if cross && tick % 6 == 0 {
            send(t, roads, Vec2::new(-0.5 * w, -300.0), Vec2::new(-0.5 * w, 300.0));
        }
        for id in t.step(1.0, 4) {
            t.remove_car(id);
        }
    }
}

/// An eastbound left turner waits for a gap in a constant westbound
/// stream; the straight-on car behind it pulls onto the shoulder,
/// passes and merges back. Returns a trace for the determinism check.
pub(super) fn pass_scene() -> Vec<(f32, f32)> {
    let roads = street_cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let w = LANE_WIDTH;
    let east_in = roads.nearest_street(Vec2::new(-200.0, 0.0)).unwrap().0;
    let lane = lane_of(&t, east_in, Vec2::X, 0);
    let room = Traffic::pass_offset(&t.net.lanes[lane as usize]).expect("a shoulder to pass on");
    assert!(room + CAR_WIDTH * 0.5 <= LANE_WIDTH * 0.5 + SHOULDER, "stays off the kerb");
    warm_up(&mut t, &roads, false);
    let turner = send(&mut t, &roads, Vec2::new(-60.0, 0.5 * w), Vec2::new(0.5 * w, -150.0));
    let follower = send(&mut t, &roads, Vec2::new(-100.0, 0.5 * w), Vec2::new(150.0, 0.5 * w));
    let mut trace = Vec::new();
    let (mut passed_while_waiting, mut max_lateral, mut was_passing): (bool, f32, bool) = (false, 0.0, false);
    for tick in 0..300 {
        // The oncoming stream that keeps the turner waiting.
        if tick % 2 == 0 {
            send(&mut t, &roads, Vec2::new(300.0, -0.5 * w), Vec2::new(-300.0, -0.5 * w));
        }
        for id in t.step(1.0, 4) {
            if id != follower && id != turner {
                t.remove_car(id);
            }
        }
        let Some(f) = t.cars.get(follower) else { break };
        was_passing |= f.passing;
        max_lateral = max_lateral.max(f.lateral);
        let on_lane = f.route[f.idx] == lane;
        if on_lane && f.v < 0.5 && f.passing {
            assert!(f.s <= t.net.lanes[lane as usize].length - CROSSING_SETBACK + 0.5, "waits short of the crossing ({:.1})", f.s);
        }
        let tc = &t.cars[turner];
        if !on_lane && f.idx > 0 && tc.route[tc.idx] == lane {
            passed_while_waiting = true;
        }
        trace.push((f.pos.x, f.pos.y));
        if f.idx >= 2 && f.lateral.abs() < 0.05 {
            break;
        }
    }
    assert!(was_passing, "the follower pulled out to pass");
    assert!(max_lateral >= CAR_WIDTH && max_lateral <= room + 0.01, "onto the shoulder, not the kerb: {max_lateral:.2}");
    assert!(passed_while_waiting, "it got by while the turner was still waiting");
    let f = &t.cars[follower];
    assert!(f.idx >= 2 && f.lateral.abs() < 0.05, "and merged back into its lane");
    trace
}

/// Distance from `p` to the nearest lane of `route`.
pub(super) fn off_route(t: &Traffic, route: &[LaneId], p: Vec2) -> f32 {
    route.iter().map(|&l| t.net.lanes[l as usize].project(p).1).fold(f32::MAX, f32::min)
}

/// Drive one car through a turn at the game's tick (4 s in 8 substeps),
/// sampling where it is drawn 16 times a tick. Returns the largest
/// distance from the lane centre line of the drawn pose and of the old
/// straight-line blend, and whether it arrived.
pub(super) fn turn_drawn(t: &mut Traffic, car: CarId) -> (f32, f32, bool) {
    let route = t.cars[car].route.clone();
    let (mut drawn, mut blend): (f32, f32) = (0.0, 0.0);
    for _ in 0..200 {
        let arrived = t.step(4.0, 8);
        let c = &t.cars[car];
        for k in 0..=16 {
            let f = k as f32 / 16.0;
            drawn = drawn.max(off_route(t, &route, c.pose_at(f).0));
            blend = blend.max(off_route(t, &route, c.prev_pos.lerp(c.pos, f)));
        }
        if arrived.contains(&car) {
            return (drawn, blend, true);
        }
    }
    (drawn, blend, false)
}

/// A lone car's cruising speed and its gap behind a slow leader.
pub(super) fn cruise(profile: DriverProfile) -> (f32, f32) {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-1500.0, 0.0), Vec2::new(1500.0, 0.0)], 1, 1);
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let seg = roads.nearest_street(Vec2::ZERO).unwrap().0;
    let lane = lane_of(&t, seg, Vec2::X, 0);
    let car = t.spawn_car(CitizenId::default(), None, vec![lane], 10.0, 2900.0, 0);
    t.cars[car].drive.profile = profile;
    for _ in 0..40 {
        t.step(1.0, 4);
    }
    let speed = t.cars[car].v;
    // A leader at 8 m/s ahead: the steady gap behind it.
    let lead = t.spawn_car(CitizenId::default(), None, vec![lane], t.cars[car].s + 60.0, 2950.0, 0);
    t.cars[lead].v = 8.0;
    t.cars[lead].drive.profile = DriverProfile { speed: 8.0 / ROAD_SPEED, ..DriverProfile::default() };
    for _ in 0..90 {
        t.step(1.0, 4);
    }
    (speed, t.cars[lead].s - t.cars[lead].length() - t.cars[car].s)
}

/// Ten minutes of a stream of drivers (all drunk or all sober) through a
/// signal on one-lane streets, with cross traffic. Crashed cars are
/// cleared away at once so one crash does not cause the next. Returns
/// (crashes, cars, largest weave, red lights run).
pub(super) fn drink_trial(drunk: f32, seed: u64) -> (usize, usize, f32, u32) {
    let roads = street_cross();
    let mut t = Traffic::default();
    t.rng = seed;
    t.rebuild(&roads);
    let w = LANE_WIDTH;
    let (mut crashes, mut cars, mut weave) = (0, 0, 0.0f32);
    for tick in 0..300 {
        if tick % 2 == 0 {
            let c = send(&mut t, &roads, Vec2::new(-350.0, 0.5 * w), Vec2::new(350.0, 0.5 * w));
            let k = (tick as f32 * 0.618).fract();
            t.cars[c].drive.profile = DriverProfile::for_driver(30, 0.5, 0.5, drunk, k, k);
            cars += 1;
        }
        if tick % 5 == 0 {
            let c = send(&mut t, &roads, Vec2::new(-0.5 * w, -350.0), Vec2::new(-0.5 * w, 350.0));
            t.cars[c].drive.profile = DriverProfile::for_driver(45, 0.6, 0.4, 0.0, 0.6, 0.5);
        }
        for id in t.step(2.0, 4) {
            t.remove_car(id);
        }
        for c in t.cars.values() {
            if c.drive.profile.drunk == drunk {
                weave = weave.max(c.drive.weave.abs());
            }
        }
        for r in std::mem::take(&mut t.crashes) {
            crashes += 1;
            for c in r.cars {
                t.remove_car(c);
            }
        }
    }
    (crashes, cars, weave, t.red_runs)
}
