//! The lane network: lanes, connectors, ramps, junction room.

use super::*;

#[test]
fn lanes_and_connectors_are_generated() {
    let roads = cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let road_lanes = t.net.lanes.iter().filter(|l| !l.is_connector()).count();
    let connectors = t.net.lanes.iter().filter(|l| l.is_connector()).count();
    assert_eq!(road_lanes, 8);
    // Centre: 4 incoming lanes x 3 outgoing (no U-turn) = 12; dead ends: 4 U-turns.
    assert_eq!(connectors, 16);
    assert!(t.net.intersections.iter().any(|i| i.signalized));
}

#[test]
fn lanes_keep_right_and_turns_stay_inside_junctions() {
    let roads = cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    // Eastbound lane (a -> b with +x direction) must sit on the +y side in
    // sim coordinates, which is the driver's right once y maps to world z.
    for lane in t.net.lanes.iter().filter(|l| !l.is_connector()) {
        let d = (lane.end() - lane.start()).normalize_or_zero();
        let mid = (lane.start() + lane.end()) * 0.5;
        // Perpendicular offset of the lane from the segment centreline.
        let centre_offset = if d.x.abs() > 0.5 { mid.y } else { mid.x };
        let right = Vec2::new(-d.y, d.x);
        let side = if d.x.abs() > 0.5 { right.y } else { right.x };
        assert!(centre_offset * side > 0.0, "lane {:?} is on the left of its direction", lane.kind);
    }
    for lane in t.net.lanes.iter().filter(|l| l.is_connector()) {
        let node = roads.node(lane.end_node).pos;
        let r = t.net.node_radius[lane.end_node.0 as usize];
        for p in &lane.points {
            assert!(p.distance(node) <= r * 1.6 + 0.5, "connector swings {:.1} m out of a {:.1} m junction", p.distance(node), r);
        }
    }
}

#[test]
fn a_car_drives_across_the_intersection() {
    let roads = cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let west = roads.nearest_on_network(Vec2::new(-150.0, 0.0)).unwrap().0;
    let north = roads.nearest_on_network(Vec2::new(0.0, 150.0)).unwrap().0;
    let (route, s0, s1) = t.net.find_route(west, Vec2::new(-150.0, -2.0), north, Vec2::new(2.0, 150.0)).expect("route");
    assert!(route.len() >= 3);
    let citizen = CitizenId::default();
    let car = t.spawn_car(citizen, None, route, s0, s1, 0);
    let mut arrived = false;
    for _ in 0..240 {
        if t.step(1.0, 2).contains(&car) {
            arrived = true;
            break;
        }
    }
    assert!(arrived, "car should arrive within four minutes");
    assert!(t.cars[car].pos.distance(Vec2::new(2.0, 150.0)) < 6.0);
}

#[test]
fn ramps_merge_without_signals_and_bends_keep_speed() {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 2, 2);
    // A one-way ramp joining at a shallow angle.
    roads.add_polyline(&[Vec2::new(-150.0, 60.0), Vec2::new(-20.0, 2.0)], 1, 0);
    // A two-way side street.
    roads.add_polyline(&[Vec2::new(150.0, 200.0), Vec2::new(150.0, 3.0)], 1, 1);
    let mut net = LaneNetwork::default();
    net.rebuild(&roads);
    let merge = net.intersections.iter().find(|i| roads.node(i.node).pos.distance(Vec2::new(-20.0, 0.0)) < 1.0).expect("merge junction");
    assert_eq!(merge.approaches, 3);
    assert!(!merge.signalized, "a one-way ramp yields instead of getting a signal");
    let t = net.intersections.iter().find(|i| roads.node(i.node).pos.distance(Vec2::new(150.0, 0.0)) < 1.0).expect("T junction");
    assert!(t.signalized, "three two-way roads get signals");

    // A curved road is a chain of short straight pieces; its bends run at road speed.
    let curve = crate::plan::sample_curve(&[Vec2::new(0.0, -300.0), Vec2::new(60.0, -200.0), Vec2::new(0.0, -100.0)]);
    assert!(curve.len() > 10);
    let mut roads2 = RoadNetwork::default();
    roads2.add_polyline(&curve, 1, 1);
    let mut net2 = LaneNetwork::default();
    net2.rebuild(&roads2);
    // Only the bends: dead ends keep their slow U-turn connectors.
    let bend_connectors: Vec<&Lane> = net2.lanes.iter().filter(|l| matches!(l.kind, LaneKind::Connector { node, .. } if roads2.node(node).segments.len() == 2)).collect();
    assert!(!bend_connectors.is_empty());
    assert!(bend_connectors.iter().all(|l| l.speed_limit >= ROAD_SPEED - 0.01), "bends do not slow traffic to junction speed");
}

#[test]
fn paths_carry_no_car_lanes_and_do_not_make_junctions() {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    let path = roads.add_polyline_kind(&[Vec2::new(0.0, 150.0), Vec2::new(0.0, 3.0)], 0, 0, crate::roads::RoadKind::Path);
    let mut net = LaneNetwork::default();
    net.rebuild(&roads);
    assert!(path.iter().all(|s| !net.segment_lanes.contains_key(s)), "paths have no lanes");
    let node = roads.nodes.iter().position(|n| n.pos.distance(Vec2::new(0.0, 0.0)) < 0.5).unwrap();
    let inter = net.intersections.iter().find(|i| i.node.0 as usize == node).unwrap();
    assert_eq!(inter.approaches, 2, "a path joining a street is not a junction for cars");
    assert!(!inter.signalized);
    assert!(roads.path(crate::roads::NodeId(node as u32), roads.segment(path[0]).a).is_some(), "walkers can use the path");
}

#[test]
fn turning_connectors_follow_an_arc_inside_the_junction() {
    let roads = avenue_cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let mut turns = 0;
    for lane in t.net.lanes.iter().filter(|l| l.is_connector() && l.turn != 0) {
        let (a, b) = (lane.start(), lane.end());
        let chord = a.distance(b);
        // The middle of the curve bows well away from the chord: no
        // more straight diagonals across the corner.
        let mid = lane.point_at(lane.length * 0.5).0;
        let (_, q) = project_on_segment(mid, a, b);
        assert!(mid.distance(q) > 0.15 * chord, "turn {:?} is nearly a straight chord ({:.2} m off a {:.1} m chord)", lane.kind, mid.distance(q), chord);
        // Tangent to both lanes: no kink where it joins them.
        let into = t.net.lanes[lane.prev[0] as usize].end_dir();
        let out = t.net.lanes[lane.next[0] as usize].start_dir();
        assert!(lane.start_dir().dot(into) > 0.99 && lane.end_dir().dot(out) > 0.99, "connector meets its lanes at an angle");
        turns += 1;
    }
    assert!(turns >= 8);
}

#[test]
fn turning_cars_stay_in_their_lane_through_a_junction() {
    let roads = avenue_cross();
    let w = LANE_WIDTH;
    // In-lane: the car's sides stay inside the lane's 3.5 m band.
    let in_lane = (LANE_WIDTH - CAR_WIDTH) * 0.5;
    // Right turn (west arm to north arm), left turn (east arm to north arm) and a bus.
    let cases = [
        (Vec2::new(-300.0, 1.5 * w), Vec2::new(-0.5 * w, 150.0), false),
        (Vec2::new(300.0, -0.5 * w), Vec2::new(-0.5 * w, 150.0), false),
        (Vec2::new(-300.0, 1.5 * w), Vec2::new(-0.5 * w, 150.0), true),
    ];
    let mut bus_swing = 0.0;
    for (from, to, bus) in cases {
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let car = send(&mut t, &roads, from, to);
        if bus {
            t.cars[car].bus = Some(Bus { route: crate::transit::RouteId::default(), next_stop: 0, passengers: Vec::new(), to_depot: false, retiring: false, depot: BuildingId::default() });
        }
        let (drawn, blend, arrived) = turn_drawn(&mut t, car);
        assert!(arrived, "the car got through");
        if bus {
            bus_swing = drawn;
            assert!(drawn > 0.5 && drawn < 1.3, "a bus swings wide through the turn, within reason: {drawn:.2} m");
            continue;
        }
        assert!(drawn < in_lane, "drawn {drawn:.2} m off the lane centre (limit {in_lane:.2})");
        // The old drawing (a straight blend over a 4 s tick) cut the corner by metres.
        assert!(blend > 1.5, "the straight blend should cut the corner: {blend:.2}");
    }
    assert!(bus_swing > 0.0);
}

/// A junction whose exit has room for one car behind a standing tail
/// takes one car, not every car that sees that room: the ones already
/// in the junction count.
#[test]
fn cars_in_the_junction_count_against_the_room_on_its_exit() {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    roads.add_polyline(&[Vec2::new(0.0, -400.0), Vec2::new(0.0, 400.0)], 1, 1);
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let west = roads.nearest_street(Vec2::new(-200.0, 0.0)).unwrap().0;
    let east = roads.nearest_street(Vec2::new(200.0, 0.0)).unwrap().0;
    let (into, out) = (lane_of(&t, west, Vec2::X, 0), lane_of(&t, east, Vec2::X, 0));
    let conn = *t.net.lanes[into as usize].next.iter().find(|&&c| t.net.lanes[c as usize].next[0] == out).expect("straight on");
    // A tail standing 10 m into the exit: room for one more car.
    let tail = t.spawn_car(CitizenId::default(), None, vec![out], 10.0, 300.0, 0);
    t.cars[tail].hold = 1000.0;
    assert!(!t.exit_blocked(&t.net.lanes[conn as usize], out, true), "room for one");
    let inside = t.spawn_car(CitizenId::default(), None, vec![conn, out], 4.0, 300.0, 0);
    assert_eq!(t.net.lanes[conn as usize].cars, vec![inside]);
    assert!(t.exit_blocked(&t.net.lanes[conn as usize], out, true), "that room is taken by the car already in the junction");
}
