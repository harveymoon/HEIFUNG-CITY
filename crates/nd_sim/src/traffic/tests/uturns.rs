//! [r2-traffic] U-turns at signalled junctions of wide streets, and
//! re-plans that do not send a car out and back down a dead end.

use super::*;
use crate::junctions::{JunctionRule, SignalMode};

/// Two 2+2 avenues crossing (a signalled four-way), ending in dead ends.
fn avenues() -> RoadNetwork {
    let mut net = RoadNetwork::default();
    net.add_polyline(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 2, 2);
    net.add_polyline(&[Vec2::new(0.0, -400.0), Vec2::new(0.0, 400.0)], 2, 2);
    net
}

/// East along the west arm, to a point back west on the same arm.
fn back_west(t: &Traffic, roads: &RoadNetwork) -> (LaneId, f32, LaneId, f32) {
    let west = roads.nearest_street(Vec2::new(-200.0, 0.0)).unwrap().0;
    let from = lane_of(t, west, Vec2::X, 0);
    let to = lane_of(t, west, -Vec2::X, 1);
    let (s0, _) = t.net.lanes[from as usize].project(Vec2::new(-200.0, 0.0));
    let (s1, _) = t.net.lanes[to as usize].project(Vec2::new(-300.0, 0.0));
    (from, s0, to, s1)
}

#[test]
fn a_signalled_junction_of_wide_streets_has_u_turns_from_the_left_lane() {
    let roads = avenues();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let k = t.net.intersection_at(&roads, Vec2::ZERO).expect("junction");
    assert!(t.net.intersections[k].signalized);
    let uturns: Vec<LaneId> = (0..t.net.lanes.len() as LaneId).filter(|&l| t.net.lanes[l as usize].uturn).collect();
    assert_eq!(uturns.len(), 4, "one from each arm");
    for &u in &uturns {
        let c = &t.net.lanes[u as usize];
        assert!(c.permitted && c.turn == -1, "open, driven as a left turn");
        let from = c.prev[0];
        assert!(t.net.lanes[from as usize].left.is_none() && t.net.lanes[from as usize].arrows & LEFT != 0, "from the left-turn lane");
        let LaneKind::Road { segment: s_in, .. } = t.net.lanes[from as usize].kind else { panic!() };
        let LaneKind::Road { segment: s_out, forward, .. } = t.net.lanes[c.next[0] as usize].kind else { panic!() };
        assert_eq!(s_in, s_out, "back along the same street");
        assert!(t.net.lanes[c.next[0] as usize].right.is_none(), "swinging wide into the kerb lane ({forward})");
        assert!(c.points.iter().all(|p| p.length() < 14.0), "inside the junction");
    }
    // The lane arrows are what the junction offers without them.
    let ap = t.net.approaches.iter().find(|a| a.node == t.net.intersections[k].node).unwrap();
    assert_eq!(t.net.exits_of(&ap.lanes), LEFT | STRAIGHT | RIGHT);
}

#[test]
fn narrow_streets_and_unsignalled_junctions_have_no_u_turn() {
    let roads = street_cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    assert!(!t.net.lanes.iter().any(|l| l.uturn), "one lane each way: none");
    // Avenues with the signals switched off: the U-turns close.
    let roads = avenues();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    t.set_junction_rule(&roads, JunctionRule { signal: SignalMode::Unsignalised, ..JunctionRule::new(Vec2::ZERO) });
    assert!(t.net.lanes.iter().filter(|l| l.uturn).all(|l| !l.permitted), "closed without signals");
    let (from, s0, to, s1) = back_west(&t, &roads);
    let (route, _, _) = t.net.route_lanes(from, s0, to, s1).expect("the long way");
    assert!(!route.iter().any(|&l| t.net.lanes[l as usize].uturn));
    t.set_junction_rule(&roads, JunctionRule::new(Vec2::ZERO));
    assert!(t.net.lanes.iter().filter(|l| l.uturn).all(|l| l.permitted), "open again with the signals back");
}

#[test]
fn a_car_turns_back_at_the_lights_not_down_a_dead_end() {
    // K22/K49/K52: drivers went out and back down the nearest dead end to
    // turn round; at a signalled junction of wide streets they U-turn.
    let roads = avenues();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let (from, s0, to, s1) = back_west(&t, &roads);
    let (route, _, _) = t.net.route_lanes(from, s0, to, s1).expect("route");
    assert!(route.iter().any(|&l| t.net.lanes[l as usize].uturn), "a U-turn at the lights: {route:?}");
    assert!(!route.iter().any(|&l| t.net.lanes[l as usize].turnaround), "no dead end");
    let car = t.spawn_car(CitizenId::default(), None, route, s0, s1, 0);
    let (used, _, in_time) = drive(&mut t, car);
    assert!(in_time && used.iter().any(|&l| t.net.lanes[l as usize].uturn), "it drove the U-turn and arrived: {used:?}");
}

#[test]
fn a_replan_weighs_turning_back_and_remembers_what_it_gave_up() {
    let roads = avenues();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let (from, s0, to, s1) = back_west(&t, &roads);
    // Turning back costs in a re-plan: a dead end's turning circle is
    // never chosen over the U-turn, and an avoided lane stays avoided.
    let (route, _, _) = t.net.route_replan(from, s0, to, s1, &[], None).expect("route");
    let u = *route.iter().find(|&&l| t.net.lanes[l as usize].uturn).expect("the U-turn");
    let other = t.net.route_replan(from, s0, to, s1, &[u], None).expect("another way");
    assert!(!other.0.contains(&u), "not back into what it gave up on");
    let id = t.spawn_car(CitizenId::default(), None, vec![from], s0, s0 + 1.0, 0);
    let mut c = t.cars.remove(id).unwrap();
    for l in 0..10 {
        c.remember_avoided(l);
    }
    assert_eq!(c.avoided.len(), AVOID_MAX, "a bounded memory");
    assert_eq!(c.avoided.last().unwrap().0, 9);
}
