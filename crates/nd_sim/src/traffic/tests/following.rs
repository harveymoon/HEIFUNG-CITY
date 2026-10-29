//! Following and passing: what drivers see ahead.

use super::*;

#[test]
fn the_lookahead_sees_a_bus_as_long_as_it_is() {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-200.0, 0.0), Vec2::new(0.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let west = roads.nearest_street(Vec2::new(-100.0, 0.0)).unwrap().0;
    let east = roads.nearest_street(Vec2::new(100.0, 0.0)).unwrap().0;
    // Both points on the eastbound (right-hand, +y) side.
    let (route, s0, s1) = t.net.find_route(west, Vec2::new(-30.0, 2.0), east, Vec2::new(150.0, 2.0)).expect("route");
    assert_eq!(route.len(), 3, "lane, bend connector, lane");
    // A bus standing just past the start of the far lane.
    let bus_route = vec![route[2]];
    let bus = t.spawn_car(CitizenId::default(), None, bus_route, 6.0, 150.0, 0);
    t.cars[bus].bus = Some(Bus { route: crate::transit::RouteId::default(), next_stop: 0, passengers: Vec::new(), to_depot: false, retiring: false, depot: BuildingId::default() });
    let car = t.spawn_car(CitizenId::default(), None, route.clone(), s0, s1, 0);
    let lane = &t.net.lanes[route[0] as usize];
    let (gap, lead_v, _, _) = t.lookahead(&t.cars[car], lane);
    let expected = (lane.length - s0) + t.net.lanes[route[1] as usize].length + 6.0 - BUS_LENGTH;
    assert!((gap - expected).abs() < 1e-3, "gap {gap} vs {expected}: the bus is {BUS_LENGTH} m, not a car");
    assert_eq!(lead_v, 0.0);
}

#[test]
fn a_car_passes_a_waiting_left_turner_on_the_right() {
    let a = pass_scene();
    assert_eq!(a, pass_scene(), "deterministic");
}
