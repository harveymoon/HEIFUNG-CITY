//! Routes, replans, gridlocks and the kerb.

use super::*;

#[test]
fn a_stalled_vehicle_is_replanned_never_removed() {
    let roads = cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let west = roads.nearest_street(Vec2::new(-150.0, 0.0)).unwrap().0;
    let north = roads.nearest_street(Vec2::new(0.0, 150.0)).unwrap().0;
    let (route, s0, s1) = t.net.find_route(west, Vec2::new(-150.0, -2.0), north, Vec2::new(2.0, 150.0)).expect("route");
    let goal = *route.last().unwrap();
    let bus = t.spawn_car(CitizenId::default(), None, route.clone(), s0, s1, 0);
    t.cars[bus].bus = Some(Bus { route: crate::transit::RouteId::default(), next_stop: 1, passengers: Vec::new(), to_depot: false, retiring: false, depot: BuildingId::default() });
    let car = t.spawn_car(CitizenId::default(), None, route, s0 + 20.0, s1, 0);
    // Both have been out far too long; neither has moved.
    t.cars[bus].age = REPLAN_AFTER + 1.0;
    t.cars[car].age = REPLAN_AFTER + 1.0;
    let arrived = t.step(1.0, 1);
    assert!(!arrived.contains(&car) && t.cars.contains_key(car), "[carlife] a private car never gives up and vanishes");
    assert!(t.cars[car].age < 2.0, "it looked for another way");
    assert!(!arrived.contains(&bus), "nor does the bus");
    let b = &t.cars[bus];
    assert!(b.age < 2.0, "its clock restarted");
    assert_eq!(b.idx, 0);
    assert_eq!(*b.route.last().unwrap(), goal, "still bound for the same stop");
    assert!((b.s_end - s1).abs() < 1e-4);
    assert!(t.net.lanes[b.route[0] as usize].cars.contains(&bus), "and still on its lane");
    // A bus whose stop is behind it on its own lane cannot be replanned
    // (lanes are one way): it is already past it, nothing more.
    let east = roads.nearest_street(Vec2::new(150.0, 0.0)).unwrap().0;
    let (lane, s, _) = t.net.nearest_lane_on(east, Vec2::new(150.0, 2.0)).unwrap();
    let stuck = t.spawn_car(CitizenId::default(), None, vec![lane], s, s - 20.0, 0);
    t.cars[stuck].bus = Some(Bus { route: crate::transit::RouteId::default(), next_stop: 1, passengers: Vec::new(), to_depot: false, retiring: false, depot: BuildingId::default() });
    t.cars[stuck].age = REPLAN_AFTER + 1.0;
    t.step(1.0, 1);
    assert!(t.cars.contains_key(stuck), "past its stop it arrives where it stands; nothing is deleted");
}

#[test]
fn a_gridlock_ring_is_found_and_broken() {
    // A two-by-two grid of blocks; fill the four lanes round the
    // north-east block with cars that each want the next lane round.
    let mut roads = RoadNetwork::default();
    for c in [-100.0, 0.0, 100.0] {
        roads.add_polyline(&[Vec2::new(-100.0, c), Vec2::new(100.0, c)], 1, 1);
        roads.add_polyline(&[Vec2::new(c, -100.0), Vec2::new(c, 100.0)], 1, 1);
    }
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let corners = [Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(100.0, 100.0), Vec2::new(0.0, 100.0)];
    let ring: Vec<LaneId> = (0..4)
        .map(|i| {
            let (a, b) = (corners[i], corners[(i + 1) % 4]);
            let seg = roads.nearest_street((a + b) * 0.5).unwrap().0;
            lane_of(&t, seg, (b - a).normalize(), 0)
        })
        .collect();
    let conn = |a: LaneId, b: LaneId| *t.net.lanes[a as usize].next.iter().find(|&&c| t.net.lanes[c as usize].next[0] == b).expect("connector");
    let mut placed: Vec<(Vec<LaneId>, f32, f32)> = Vec::new();
    for i in 0..4 {
        let (l0, l1, l2) = (ring[i], ring[(i + 1) % 4], ring[(i + 2) % 4]);
        let route = vec![l0, conn(l0, l1), l1, conn(l1, l2), l2];
        let len = t.net.lanes[l0 as usize].length;
        let n = ((len - 6.5) / 6.5).floor() as usize + 1;
        let step = (len - 6.5) / (n - 1) as f32;
        for k in 0..n {
            placed.push((route.clone(), 5.0 + k as f32 * step, t.net.lanes[l2 as usize].length * 0.5));
        }
    }
    let cars: Vec<CarId> = placed.into_iter().map(|(route, s, s_end)| t.spawn_car(CitizenId::default(), None, route, s, s_end, 0)).collect();
    assert!(cars.iter().all(|&c| !t.cars[c].entering), "all placed on the road");
    let locks = t.gridlocks();
    assert_eq!(locks.len(), 1, "one ring");
    assert_eq!(locks[0].junctions.len(), 4);
    assert_eq!(locks[0].vehicles, cars.len());
    let mut on_time = 0;
    for _ in 0..1200 {
        for id in t.step(2.0, 4) {
            on_time += (t.cars[id].age < REPLAN_AFTER) as usize;
            t.remove_car(id);
        }
        if t.cars.is_empty() {
            break;
        }
    }
    assert_eq!(on_time, cars.len(), "the ring was broken by drivers taking another way, nobody timed out");
}

/// A bus leaves a depot onto a lane saturated by a stream queuing at a
/// signal: the next driver lets it out with room for all 12 m of it,
/// it commits, and nothing ever waits on anything in a cycle. (The
/// courtesy gap used to be a car's length: the follower stopped 7.5 m
/// short of what the bus needed and both waited for ever.)
#[test]
fn a_bus_pulls_out_of_a_depot_beside_a_saturated_lane() {
    let roads = street_cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let w = LANE_WIDTH;
    let mut bus = None;
    let mut out_at = None;
    for tick in 0..900 {
        send(&mut t, &roads, Vec2::new(-380.0, 0.5 * w), Vec2::new(380.0, 0.5 * w));
        if tick == 120 {
            let id = send(&mut t, &roads, Vec2::new(-100.0, 0.5 * w), Vec2::new(300.0, 0.5 * w));
            t.cars[id].bus = Some(Bus { route: crate::transit::RouteId::default(), next_stop: 0, passengers: Vec::new(), to_depot: false, retiring: false, depot: BuildingId::default() });
            if !t.cars[id].entering {
                // Out of the depot gate, not yet on the road.
                let lane = t.cars[id].route[0];
                t.net.lanes[lane as usize].cars.retain(|&c| c != id);
                t.cars[id].entering = true;
            }
            bus = Some(id);
        }
        for id in t.step(2.0, 4) {
            t.remove_car(id);
        }
        if let Some(b) = bus {
            if out_at.is_none() && t.cars.get(b).is_none_or(|c| !c.entering) {
                out_at = Some(tick);
            }
        }
        let cycles = t.wait_cycles();
        assert!(cycles.is_empty(), "vehicles waiting on each other at {}s: {:?}", tick * 2, cycles.iter().map(|g| &g.points).collect::<Vec<_>>());
    }
    let west = roads.nearest_street(Vec2::new(-100.0, 0.0)).unwrap().0;
    let queued = t.net.lanes[lane_of(&t, west, Vec2::X, 0) as usize].cars.len();
    assert!(queued > 20, "the lane was saturated ({queued} vehicles)");
    let out = out_at.expect("the bus got out of the depot");
    assert!(out - 120 < 150, "within five minutes, took {}s", (out - 120) * 2);
    let worst = t.cars.values().filter(|c| !c.entering).map(|c| c.stalled).fold(0.0, f32::max);
    assert!(worst < 600.0, "the queue keeps moving (longest stand {worst:.0}s)");
}

#[test]
fn vehicles_waiting_on_each_other_are_a_gridlock() {
    let roads = street_cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let w = LANE_WIDTH;
    let a = send(&mut t, &roads, Vec2::new(-200.0, 0.5 * w), Vec2::new(300.0, 0.5 * w));
    let b = send(&mut t, &roads, Vec2::new(-150.0, 0.5 * w), Vec2::new(300.0, 0.5 * w));
    for (x, y) in [(a, b), (b, a)] {
        let c = &mut t.cars[x];
        c.blocked_by = Some(y);
        c.stalled = WAIT_GRAPH_STALL + 1.0;
    }
    let locks = t.gridlocks();
    assert_eq!(locks.len(), 1);
    assert_eq!(locks[0].cycle.len(), 2);
    t.cars[b].stalled = 0.0;
    assert!(t.gridlocks().is_empty(), "a car that has just moved is not in a knot");
}

/// The 30-minute replan swaps the route and resets `idx`: the end-of-
/// route test must read the new index, not one taken before the swap.
/// A car on the connector, its old `idx` 1 on a 3-lane route, must not
/// be taken for one on the last lane of its new 2-lane route.
#[test]
fn a_replan_mid_route_does_not_arrive_the_car_early() {
    let roads = cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let west = roads.nearest_on_network(Vec2::new(-150.0, 0.0)).unwrap().0;
    let north = roads.nearest_on_network(Vec2::new(0.0, 150.0)).unwrap().0;
    let (route, s0, _) = t.net.find_route(west, Vec2::new(-150.0, -2.0), north, Vec2::new(2.0, 150.0)).expect("route");
    // Mid-route at `k`, where the replanned route (the rest of this
    // one) is `k + 1` lanes long: the old index reads as its last lane.
    assert!(route.len() >= 3 && route.len() % 2 == 1, "route of {} lanes", route.len());
    let k = (route.len() - 1) / 2;
    let car = t.spawn_car(CitizenId::default(), None, route.clone(), s0, 1.0, 0);
    // Put it near the end of lane `k`, and make the trip stale.
    t.net.lanes[route[0] as usize].cars.retain(|&c| c != car);
    let len = t.net.lanes[route[k] as usize].length;
    {
        let c = &mut t.cars[car];
        c.idx = k;
        c.s = len - 0.2;
        c.age = REPLAN_AFTER + 1.0;
    }
    t.insert_sorted(route[k], car);
    let arrived = t.step(0.05, 1);
    assert!(!arrived.contains(&car), "arrived on lane {k} of {}, short of its goal", route.len());
    let c = &t.cars[car];
    assert_eq!(c.route.len(), route.len() - k, "replanned from lane {k}");
    assert!(c.idx + 1 < c.route.len(), "not on its last lane yet");
}

/// A car waiting at the kerb is in no lane list; a reroute must not
/// put it in one, or `pull_out` lists it twice.
#[test]
fn rerouting_a_car_at_the_kerb_lists_it_once() {
    let roads = cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let west = roads.nearest_on_network(Vec2::new(-150.0, 0.0)).unwrap().0;
    let north = roads.nearest_on_network(Vec2::new(0.0, 150.0)).unwrap().0;
    let (route, s0, s1) = t.net.find_route(west, Vec2::new(-150.0, -2.0), north, Vec2::new(2.0, 150.0)).expect("route");
    let car = t.spawn_car(CitizenId::default(), None, route.clone(), s0, s1, 0);
    t.pull_in(car, 0.0);
    assert!(t.cars[car].entering);
    // A new route from another lane (from the south arm).
    let south = roads.nearest_on_network(Vec2::new(0.0, -150.0)).unwrap().0;
    let (other, o0, o1) = t.net.find_route(south, Vec2::new(-2.0, -150.0), north, Vec2::new(-2.0, 150.0)).expect("route");
    assert_ne!(other[0], route[0]);
    t.reroute(car, other, o0, o1);
    let listed = |t: &Traffic| t.net.lanes.iter().map(|l| l.cars.iter().filter(|&&c| c == car).count()).sum::<usize>();
    assert_eq!(listed(&t), 0, "still at the kerb");
    for _ in 0..20 {
        t.step(0.5, 1);
        assert!(listed(&t) <= 1, "listed {} times", listed(&t));
    }
    assert_eq!(listed(&t), 1, "pulled out");
}

#[test]
fn a_car_waits_at_a_full_exit_rather_than_take_a_huge_detour() {
    // [rage] F18: rerouted round every full exit it came to, a driver went
    // in loops for a day on an hour's drive. The only other way here is a
    // loop kilometres long: it waits for the exit to clear.
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(0.0, 0.0)], 1, 1);
    roads.add_polyline(&[Vec2::new(0.0, 0.0), Vec2::new(600.0, 0.0)], 1, 1);
    roads.add_polyline(&[Vec2::new(600.0, 0.0), Vec2::new(900.0, 0.0)], 1, 1);
    roads.add_polyline(&[Vec2::new(0.0, 0.0), Vec2::new(0.0, 1000.0), Vec2::new(600.0, 1000.0), Vec2::new(600.0, 0.0)], 1, 1);
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let west = roads.nearest_street(Vec2::new(-150.0, 0.0)).unwrap().0;
    let mid = roads.nearest_street(Vec2::new(300.0, 0.0)).unwrap().0;
    let east = roads.nearest_street(Vec2::new(750.0, 0.0)).unwrap().0;
    let (w_lane, goal) = (lane_of(&t, west, Vec2::X, 0), lane_of(&t, east, Vec2::X, 0));
    let len = t.net.lanes[w_lane as usize].length;
    let s1 = t.net.lanes[goal as usize].length * 0.5;
    let (route, _, _) = t.net.route_lanes(w_lane, len - 2.0, goal, s1).expect("route");
    let exit = route[2];
    assert_eq!(exit, lane_of(&t, mid, Vec2::X, 0), "straight on along the street: {route:?}");
    // The exit is full of stopped cars from its start.
    for k in 0..6 {
        let id = t.spawn_car(CitizenId::default(), None, vec![exit], 5.0 + 7.0 * k as f32, t.net.lanes[exit as usize].length - 1.0, 0);
        if t.cars[id].entering {
            t.cars[id].entering = false;
            t.insert_sorted(exit, id);
        }
    }
    let car = t.spawn_car(CitizenId::default(), None, route.clone(), len - 2.0, s1, 0);
    if t.cars[car].entering {
        t.cars[car].entering = false;
        t.insert_sorted(w_lane, car);
    }
    t.cars[car].stalled = REROUTE_AFTER + 5.0;
    assert!(t.exit_blocked(&t.net.lanes[route[1] as usize], exit, true), "the exit is full");
    let other = t.net.route_avoiding(w_lane, len - 2.0, *route.last().unwrap(), s1, Some(exit), None);
    assert!(other.is_some(), "the long way round exists");
    t.reroute_stalled(1.0);
    assert_eq!(t.cars[car].route, route, "it waits its turn instead of a loop of kilometres");
}

/// [r4-flags-look] (flag day 13 at (-589,89), "stacked trucks") Vehicles
/// pulling in at one kerb spot line up along it instead of standing on top
/// of each other.
#[test]
fn vehicles_pulling_in_at_one_spot_line_up_along_the_kerb() {
    let roads = cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let west = roads.nearest_on_network(Vec2::new(-150.0, 0.0)).unwrap().0;
    let north = roads.nearest_on_network(Vec2::new(0.0, 150.0)).unwrap().0;
    let (route, s0, s1) = t.net.find_route(west, Vec2::new(-120.0, -2.0), north, Vec2::new(2.0, 150.0)).expect("route");
    let mut cars = Vec::new();
    for _ in 0..3 {
        let car = t.spawn_car(CitizenId::default(), None, route.clone(), s0, s1, 0);
        assert!(!t.cars[car].entering, "set off into the lane");
        t.pull_in(car, 60.0);
        cars.push(car);
    }
    for _ in 0..4 {
        t.step(0.5, 1);
    }
    for (i, &a) in cars.iter().enumerate() {
        for &b in &cars[i + 1..] {
            let (ca, cb) = (&t.cars[a], &t.cars[b]);
            let apart = ca.pos.distance(cb.pos);
            assert!(apart >= ca.length().min(cb.length()), "two vehicles at the kerb {apart:.2} m apart");
        }
    }
}
