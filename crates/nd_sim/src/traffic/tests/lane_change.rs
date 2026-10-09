//! Lane changes, for a turn and at the solid line.

use super::*;

#[test]
fn lane_changes_wait_for_a_safe_gap() {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 2, 2);
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let seg = roads.nearest_street(Vec2::ZERO).unwrap().0;
    let (left, right) = (lane_of(&t, seg, Vec2::X, 0), lane_of(&t, seg, Vec2::X, 1));
    assert_eq!(t.net.lanes[right as usize].left, Some(left));
    assert_eq!(t.net.lanes[left as usize].right, Some(right));
    let car = t.spawn_car(CitizenId::default(), None, vec![right, left], 100.0, 200.0, 0);
    t.cars[car].v = 10.0;
    let other = t.spawn_car(CitizenId::default(), None, vec![left], 100.0, 250.0, 0);
    let mut check = |s: f32, v: f32| {
        t.net.lanes[left as usize].cars.retain(|&c| c != other);
        t.cars[other].s = s;
        t.cars[other].v = v;
        t.insert_sorted(left, other);
        t.change_ok(car, left, LC_SAFE_BRAKE).is_some()
    };
    assert!(!check(100.0, 10.0), "alongside: no room");
    assert!(!check(88.0, 13.0), "a faster car 12 m behind would have to brake hard");
    assert!(check(30.0, 13.0), "70 m behind it has time");
    assert!(!check(108.0, 0.0), "a stopped car 3.5 m ahead: the changer would have to brake hard");
    assert!(check(160.0, 10.0), "a leader well ahead at the same speed is fine");
    // With a safe gap the change happens on the next step and the car
    // drifts across instead of jumping.
    check(250.0 - 1.0, 10.0);
    let p0 = t.cars[car].pos;
    t.step(0.5, 1);
    let c = &t.cars[car];
    assert_eq!(c.route[c.idx], left, "changed lanes");
    assert!(t.net.lanes[left as usize].cars.contains(&car) && !t.net.lanes[right as usize].cars.contains(&car));
    assert!(c.lateral > 2.0, "still drawn near the old lane: {}", c.lateral);
    assert!(c.pos.distance(p0) < 10.0 * 0.5 + 1.0, "no sideways jump");
}

#[test]
fn cars_move_to_the_turn_lane_their_turn_needs() {
    let roads = avenue_cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let w = LANE_WIDTH;
    let west = roads.nearest_street(Vec2::new(-200.0, 0.0)).unwrap().0;
    let (inner, outer) = (lane_of(&t, west, Vec2::X, 0), lane_of(&t, west, Vec2::X, 1));
    // Lane discipline at the junction: the outer (right) lane never
    // turns left, the inner (left) lane never turns right; both go on.
    let turns = |l: LaneId| t.net.lanes[l as usize].next.iter().filter(|&&c| t.net.lanes[c as usize].permitted).map(|&c| t.net.lanes[c as usize].turn).collect::<Vec<i8>>();
    // The default two-lane approach: a left-only lane and a straight-and-right lane.
    assert_eq!(t.net.lanes[inner as usize].arrows, LEFT);
    assert_eq!(t.net.lanes[outer as usize].arrows, STRAIGHT | RIGHT);
    assert!(!turns(outer).contains(&-1) && turns(outer).contains(&1) && turns(outer).contains(&0), "{:?}", turns(outer));
    assert_eq!(turns(inner), vec![-1], "the inner lane only turns left");

    // A left turn (east to south: -y is the driver's left here) from the outer lane.
    let south = roads.nearest_street(Vec2::new(0.0, -200.0)).unwrap().0;
    let (route, s0, s1) = t.net.find_route(west, Vec2::new(-300.0, 1.5 * w), south, Vec2::new(0.5 * w, -150.0)).expect("route");
    assert_eq!(route[0], outer);
    assert!(route.windows(2).any(|p| p == [outer, inner]), "the route moves to the inner lane: {route:?}");
    let car = t.spawn_car(CitizenId::default(), None, route, s0, s1, 0);
    let (used, lateral, arrived) = drive(&mut t, car);
    assert!(arrived, "arrived before timing out: {used:?}");
    let conn = used.iter().position(|&l| t.net.lanes[l as usize].is_connector() && t.net.lanes[l as usize].turn != 0).expect("turned");
    assert_eq!(t.net.lanes[used[conn] as usize].turn, -1, "a left turn");
    assert_eq!(used[conn - 1], inner, "taken from the inner lane");
    assert!(lateral > 2.0, "drifted across visibly ({lateral})");
    t.remove_car(car);

    // A right turn (east to north) starting in the inner lane.
    let north = roads.nearest_street(Vec2::new(0.0, 200.0)).unwrap().0;
    let (route, s0, s1) = t.net.find_route(west, Vec2::new(-300.0, 0.5 * w), north, Vec2::new(-0.5 * w, 150.0)).expect("route");
    assert_eq!(route[0], inner);
    let car = t.spawn_car(CitizenId::default(), None, route, s0, s1, 0);
    let (used, _, arrived) = drive(&mut t, car);
    assert!(arrived);
    let conn = used.iter().position(|&l| t.net.lanes[l as usize].is_connector() && t.net.lanes[l as usize].turn != 0).expect("turned");
    assert_eq!(t.net.lanes[used[conn] as usize].turn, 1, "a right turn");
    assert_eq!(used[conn - 1], outer, "taken from the outer lane");
    assert!(t.cars[car].lateral.abs() < 0.05, "settled in its lane");
}

#[test]
fn a_car_never_lands_on_the_tail_of_one_just_into_the_next_lane() {
    // A street with a bend: two segments joined by a short connector,
    // like the many short pieces of a roundabout ring.
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-100.0, 0.0), Vec2::new(0.0, 0.0), Vec2::new(100.0, 8.0)], 1, 1);
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let first = lane_of(&t, roads.nearest_street(Vec2::new(-50.0, 0.0)).unwrap().0, Vec2::X, 0);
    let conn = t.net.lanes[first as usize].next[0];
    let second = t.net.lanes[conn as usize].next[0];
    // Someone stands just inside the second lane; a profiled driver comes
    // up too fast to stop short (a real driver cannot brake infinitely).
    let lead = t.spawn_car(CitizenId::default(), None, vec![second], 1.5, 50.0, 0);
    let len = t.net.lanes[first as usize].length;
    let fast = t.spawn_car(CitizenId::default(), None, vec![first, conn, second], len - 3.0, 50.0, 0);
    for id in [lead, fast] {
        t.cars[id].drive.profile.assigned = true;
    }
    t.cars[fast].v = 12.0;
    t.cars[lead].hold = 30.0;
    for _ in 0..6 {
        t.step(0.5, 2);
        let lane = &t.net.lanes[second as usize];
        for w in lane.cars.windows(2) {
            let (a, b) = (&t.cars[w[0]], &t.cars[w[1]]);
            assert!(b.s - b.length() - a.s > -0.5, "no car standing on another: {:.2} behind {:.2}", a.s, b.s);
        }
        assert!(t.cars.values().all(|c| c.s > 0.01 || c.entering || c.idx == 0 && c.route[0] == first), "nobody shoved back onto the lane start");
    }
    assert!(t.cars[lead].s >= 1.5 - 1e-3, "the car already there keeps its place");
}

/// Found in a player's save: routes off the map end on one lane of the
/// last carriageway, and cars in the other lanes stood at the map's
/// edge waiting to squeeze into it, holding queues behind them.
#[test]
fn a_car_leaving_the_map_does_not_wait_to_change_lanes_at_the_edge() {
    let mut roads = RoadNetwork::default();
    // One way, two lanes, running off the map at x = 300: no U-turn.
    roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 2, 0);
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let seg = roads.nearest_street(Vec2::ZERO).unwrap().0;
    let (left, right) = (lane_of(&t, seg, Vec2::X, 0), lane_of(&t, seg, Vec2::X, 1));
    assert!(t.net.lanes[right as usize].next.is_empty(), "the lane leads nowhere");
    let len = t.net.lanes[right as usize].length;
    // The exit lane is a standing queue all the way back: no gap to
    // merge into anywhere on the way.
    for k in 0..16 {
        let s = len - 2.0 - k as f32 * (CAR_LENGTH + 1.5);
        let parked = t.spawn_car(CitizenId::default(), None, vec![left], s, len - 1.0, 0);
        t.cars[parked].hold = 1.0e6;
    }
    let car = t.spawn_car(CitizenId::default(), None, vec![right, left], len - 60.0, len - 1.0, 0);
    let follower = t.spawn_car(CitizenId::default(), None, vec![right], len - 90.0, len - 1.0, 0);
    let mut arrived = Vec::new();
    let mut waited = 0.0;
    for _ in 0..90 {
        arrived.extend(t.step(1.0, 4));
        if arrived.contains(&car) {
            break;
        }
        if t.stop_reason(car).key() == "lane_change" && t.cars[car].v < 0.5 {
            waited += 1.0;
        }
    }
    assert!(arrived.contains(&car), "it leaves the map in its own lane");
    assert!(waited <= DEAD_END_GIVE_UP + 3.0, "stood {waited} s at the edge of the map trying to change lanes");
    assert_eq!(t.cars[car].route, vec![right]);
    t.remove_car(car);
    // And the car behind it is not held up for long either.
    for _ in 0..30 {
        arrived.extend(t.step(1.0, 4));
    }
    assert!(arrived.contains(&follower), "the queue behind clears");
}

/// Two cars side by side at the stop line, each needing the other's
/// lane for its turn (a left turner in the right lane, a right turner
/// in the left), are past the solid line: neither changes lanes there
/// nor stands waiting for the other. Each misses its turn, goes on from
/// its own lane and finds another way.
#[test]
fn cars_that_missed_their_lane_by_the_solid_line_go_on_and_reroute() {
    let roads = avenue_cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let w = LANE_WIDTH;
    let west = roads.nearest_street(Vec2::new(-200.0, 0.0)).unwrap().0;
    let south = roads.nearest_street(Vec2::new(0.0, -200.0)).unwrap().0;
    let north = roads.nearest_street(Vec2::new(0.0, 200.0)).unwrap().0;
    let (inner, outer) = (lane_of(&t, west, Vec2::X, 0), lane_of(&t, west, Vec2::X, 1));
    let (left_route, _, s1) = t.net.find_route(west, Vec2::new(-300.0, 1.5 * w), south, Vec2::new(0.5 * w, -150.0)).expect("route");
    let (right_route, _, s2) = t.net.find_route(west, Vec2::new(-300.0, 0.5 * w), north, Vec2::new(-0.5 * w, 150.0)).expect("route");
    assert_eq!(&left_route[..2], &[outer, inner]);
    assert_eq!(&right_route[..2], &[inner, outer]);
    let len = t.net.lanes[outer as usize].length;
    let a = t.spawn_car(CitizenId::default(), None, left_route, len - 1.0, s1, 0);
    let b = t.spawn_car(CitizenId::default(), None, right_route, len - 1.0, s2, 0);
    assert!(!t.cars[a].entering && !t.cars[b].entering);
    let mut used: HashMap<CarId, Vec<LaneId>> = HashMap::new();
    let mut done = Vec::new();
    let mut waited: f32 = 0.0;
    for _ in 0..1200 {
        for (id, c) in t.cars.iter() {
            let u = used.entry(id).or_default();
            if u.last() != Some(&c.route[c.idx]) {
                u.push(c.route[c.idx]);
            }
            if c.wait == Wait::LaneChange {
                waited = waited.max(c.stalled);
            }
        }
        for id in t.step(0.5, 1) {
            done.push(id);
            t.remove_car(id);
        }
        if done.len() == 2 {
            break;
        }
    }
    assert_eq!(done.len(), 2, "both got where they were going");
    assert!(waited < 1.0, "nobody stood at the line for a lane change ({waited} s)");
    // Out of the junction from the lane each was in.
    let out_of = |id: CarId| {
        let u = &used[&id];
        let k = u.iter().position(|&l| t.net.lanes[l as usize].is_connector()).expect("through the junction");
        u[k - 1]
    };
    assert_eq!(out_of(a), outer);
    assert_eq!(out_of(b), inner);
}

/// A car that is beside a free lane well before the junction still
/// moves over for its turn: the solid line is only the last stretch.
#[test]
fn a_car_changes_lanes_for_its_turn_before_the_solid_line() {
    let roads = avenue_cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let w = LANE_WIDTH;
    let west = roads.nearest_street(Vec2::new(-200.0, 0.0)).unwrap().0;
    let south = roads.nearest_street(Vec2::new(0.0, -200.0)).unwrap().0;
    let inner = lane_of(&t, west, Vec2::X, 0);
    let (route, s0, s1) = t.net.find_route(west, Vec2::new(-300.0, 1.5 * w), south, Vec2::new(0.5 * w, -150.0)).expect("route");
    let car = t.spawn_car(CitizenId::default(), None, route, s0, s1, 0);
    let mut glide: Vec<f32> = Vec::new();
    let mut changed = false;
    for _ in 0..1000 {
        t.step(0.2, 1);
        let Some(c) = t.cars.get(car) else { break };
        changed |= c.route[c.idx] == inner;
        if c.glide_from != 0.0 {
            glide.push(c.lateral);
        }
        if c.route[c.idx] != inner && changed {
            break;
        }
    }
    assert!(changed, "moved to the inner lane for the left turn");
    // A glide: the offset eases off, slowly at first and last.
    assert!(glide.len() >= 4, "{glide:?}");
    let steps: Vec<f32> = glide.windows(2).map(|p| (p[0] - p[1]).abs()).collect();
    let mid = steps[steps.len() / 2];
    assert!(steps[0] < mid && *steps.last().unwrap() < mid, "eased in and out: {steps:?}");
}
