//! Signals and lane arrows.

use super::*;

#[test]
fn a_one_way_exit_gets_no_green_phase() {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
    roads.add_polyline(&[Vec2::new(0.0, -200.0), Vec2::new(0.0, 0.0)], 1, 1);
    // The north arm leaves the junction one way: nothing ever arrives from it.
    roads.add_polyline(&[Vec2::new(0.0, 0.0), Vec2::new(0.0, 200.0)], 1, 0);
    let mut net = LaneNetwork::default();
    net.rebuild(&roads);
    let centre = net.intersections.iter().position(|i| roads.node(i.node).pos.distance(Vec2::ZERO) < 1.0).expect("centre");
    let inter = &net.intersections[centre];
    assert_eq!(inter.approaches, 4);
    assert!(inter.signalized);
    assert_eq!(inter.phases.len(), 2, "east and west share a phase, south has its own: {:?}", inter.phases);
    // Over a whole cycle every incoming approach gets a green, and at
    // no time is the junction red for everyone during a green window.
    let cycle = GREEN_SECONDS + CLEARANCE_SECONDS;
    let mut greens_seen = std::collections::HashSet::new();
    for step in 0..(2.0 * cycle) as usize {
        net.update_signals(1.0);
        let inter = &net.intersections[centre];
        if inter.timer < GREEN_SECONDS - 1.0 {
            let green: Vec<usize> = inter.connectors.iter().filter(|(l, _)| net.lanes[*l as usize].green).map(|(_, ap)| *ap).collect();
            assert!(!green.is_empty(), "nobody has the green at {step}s");
            greens_seen.extend(green);
        }
    }
    assert_eq!(greens_seen.len(), 3);
    // The cycle is two phases long, not four.
    let inter = &net.intersections[centre];
    assert_eq!(inter.phase, 0, "back at the first phase after two greens");
}

#[test]
fn a_busy_four_way_signal_never_deadlocks() {
    let (spawned, on_time, order) = busy_junction();
    assert_eq!(spawned, 300);
    assert_eq!(on_time, spawned, "every car arrives, none times out");
    // Deterministic: the same arrivals in the same order.
    let (_, _, again) = busy_junction();
    assert_eq!(order, again);
}

#[test]
fn lane_arrows_default_from_demand_and_can_be_set() {
    let roads = avenue_cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let w = LANE_WIDTH;
    let west = roads.nearest_street(Vec2::new(-200.0, 0.0)).unwrap().0;
    let (inner, outer) = (lane_of(&t, west, Vec2::X, 0), lane_of(&t, west, Vec2::X, 1));
    let node = t.net.lanes[outer as usize].end_node;
    // The shared straight-and-right lane takes both movements without a change.
    for to in [Vec2::new(300.0, 1.5 * w), Vec2::new(-0.5 * w, 150.0)] {
        let (route, _, _) = t.net.find_route(west, Vec2::new(-300.0, 1.5 * w), roads.nearest_street(to).unwrap().0, to).unwrap();
        assert_eq!(route[0], outer);
        assert!(!t.net.lanes[outer as usize].is_beside(route[1]), "no lane change from the straight-and-right lane: {route:?}");
    }
    // Straight on from the left-only lane moves over first.
    let (route, _, _) = t.net.find_route(west, Vec2::new(-300.0, 0.5 * w), roads.nearest_street(Vec2::new(300.0, 0.0)).unwrap().0, Vec2::new(300.0, 1.5 * w)).unwrap();
    assert_eq!(&route[..2], &[inner, outer]);

    // The player makes it left-and-straight plus right only.
    assert!(t.set_lane_use(west, node, Some(vec![LEFT, LEFT])).is_err(), "nothing would go straight or right");
    assert!(t.set_lane_use(west, node, Some(vec![LEFT])).is_err(), "two lanes need two arrows");
    t.set_lane_use(west, node, Some(vec![LEFT | STRAIGHT, RIGHT])).unwrap();
    let permitted = |l: LaneId| t.net.lanes[l as usize].next.iter().filter(|&&c| t.net.lanes[c as usize].permitted).map(|&c| t.net.lanes[c as usize].turn).collect::<Vec<i8>>();
    assert_eq!(permitted(outer), vec![1]);
    assert!(permitted(inner).contains(&0) && permitted(inner).contains(&-1));
    // It survives a rebuild of the network and can be cleared.
    t.rebuild(&roads);
    assert_eq!(t.net.lanes[inner as usize].arrows, LEFT | STRAIGHT);
    t.set_lane_use(west, node, None).unwrap();
    assert_eq!(t.net.lanes[inner as usize].arrows, LEFT);

    // Demand: a quiet left shares its lane with straight traffic, a
    // busy one on a wide avenue gets two lanes.
    let all = LEFT | STRAIGHT | RIGHT;
    assert_eq!(default_arrows(2, all, Some(0.1), None), vec![LEFT | STRAIGHT, STRAIGHT | RIGHT]);
    assert_eq!(default_arrows(2, all, Some(0.5), None), vec![LEFT, STRAIGHT | RIGHT]);
    assert_eq!(default_arrows(4, all, Some(0.5), None), vec![LEFT, LEFT, STRAIGHT, STRAIGHT | RIGHT]);
    assert_eq!(default_arrows(4, all, Some(0.1), None), vec![LEFT, STRAIGHT, STRAIGHT, STRAIGHT | RIGHT]);
    assert_eq!(default_arrows(2, LEFT | RIGHT, None, None), vec![LEFT, RIGHT], "a T junction");
    // Counted traffic revises the default.
    let ai = t.net.lanes[outer as usize].approach as usize;
    t.net.approaches[ai].counts = [2.0, 40.0, 8.0];
    t.adapt_arrows();
    assert_eq!(t.net.lanes[inner as usize].arrows, LEFT | STRAIGHT, "few lefts: the left lane is shared");
}

#[test]
fn a_protected_left_arrow_ends_starvation() {
    let roads = street_cross();
    let mut t = Traffic::default();
    t.rebuild(&roads);
    let w = LANE_WIDTH;
    warm_up(&mut t, &roads, false);
    let turner = send(&mut t, &roads, Vec2::new(-40.0, 0.5 * w), Vec2::new(0.5 * w, -150.0));
    let node = t.net.lanes[t.cars[turner].route[0] as usize].end_node;
    let inter = t.net.node_intersection[node.0 as usize];
    let mut arrows_seen = 0;
    let mut turned_at = None;
    for tick in 0..600 {
        // Constant oncoming traffic and nobody on the cross street: the
        // green never has to end, the case that used to starve lefts.
        if tick % 2 == 0 {
            send(&mut t, &roads, Vec2::new(300.0, -0.5 * w), Vec2::new(-300.0, -0.5 * w));
        }
        for id in t.step(1.0, 4) {
            if id != turner {
                t.remove_car(id);
            }
        }
        let i = &t.net.intersections[inter];
        if i.arrow > 0.0 && i.timer < i.arrow {
            arrows_seen += 1;
        }
        let c = &t.cars[turner];
        if t.net.lanes[c.route[c.idx] as usize].is_connector() && turned_at.is_none() {
            turned_at = Some(tick);
            break;
        }
    }
    let turned_at = turned_at.expect("the left turner got through");
    assert!(arrows_seen > 0, "on a protected arrow");
    assert!(turned_at < 60, "soon after starving, not at the trip timeout ({turned_at}s)");
    let (mean, max, n) = t.left_wait_stats();
    assert_eq!(n, 1);
    assert!(mean > 0.0 && max == mean);
    assert!(t.net.intersections[inter].left_turns == 1 && t.net.intersections[inter].left_wait_max > 0.0);
}
