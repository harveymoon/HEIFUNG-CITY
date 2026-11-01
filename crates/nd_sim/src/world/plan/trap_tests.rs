//! [r5-fixes] The Road tool warns when one-way streets lead into streets
//! with no way out (Harvey's flag review, r4-flags-sim leftovers): while
//! the street is drawn, and when the project is implemented.

use super::*;

fn flat() -> World {
    World::new(WorldConfig { landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() })
}

#[test]
fn a_one_way_street_into_a_pocket_with_no_way_out_is_warned_of() {
    let mut w = flat();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-100.0, 100.0), Vec2::new(100.0, 100.0)], 1, 1);
    // Two-way into the side street: a way in and back out.
    w.add_road(&[Vec2::new(100.0, 0.0), Vec2::new(100.0, 100.0)], 1, 1);
    w.implement();
    assert!(w.plan_warnings.is_empty(), "{:?}", w.plan_warnings);
    assert!(w.roads.traps().is_empty());
    // Drawing a one-way street into it is fine: the two-way one leads out.
    let into = [Vec2::new(-100.0, 0.0), Vec2::new(-100.0, 100.0)];
    assert!(w.road_trap_warnings(&into, &[], RoadKind::Street, 1, 0).is_empty());
    // Built so, still no warning.
    let mut v = w.clone();
    v.add_road(&into, 1, 0);
    v.implement();
    assert!(v.plan_warnings.is_empty(), "{:?}", v.plan_warnings);
    let over = [Vec2::new(100.0, 0.0), Vec2::new(100.0, 100.0)];
    // A second street, one-way the other way out of it, is fine.
    assert!(v.road_trap_warnings(&[Vec2::new(0.0, 100.0), Vec2::new(0.0, 0.0)], &[], RoadKind::Street, 1, 0).is_empty());
    // The side street alone, cut off from its two-way link, with a new
    // one-way street drawn in: a pocket.
    let mut p = flat();
    p.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    p.add_road(&[Vec2::new(-100.0, 100.0), Vec2::new(100.0, 100.0)], 1, 1);
    p.implement();
    let warn = p.road_trap_warnings(&over, &[], RoadKind::Street, 1, 0);
    assert_eq!(warn.len(), 1, "{warn:?}");
    assert!(warn[0].starts_with("no way out at ("), "{}", warn[0]);
    // Two-way, or one-way out of it: no warning.
    assert!(p.road_trap_warnings(&over, &[], RoadKind::Street, 1, 1).is_empty());
    assert!(p.road_trap_warnings(&[over[1], over[0]], &[], RoadKind::Street, 1, 0).is_empty());
    // Built anyway: implementing says so too, and the lanes agree.
    p.add_road(&over, 1, 0);
    p.implement();
    assert_eq!(p.plan_warnings.len(), 1, "{:?}", p.plan_warnings);
    assert!(p.plan_warnings[0].contains("no way out"), "{:?}", p.plan_warnings);
    let side = p.roads.nearest_street(Vec2::new(0.0, 100.0)).unwrap().0;
    assert!(p.traffic.net.segment_lanes[&side].iter().all(|&l| p.traffic.net.trap[l as usize]));
    // An unrelated road later: the pocket is not warned of again.
    p.add_road(&[Vec2::new(-300.0, -200.0), Vec2::new(300.0, -200.0)], 1, 1);
    p.implement();
    assert!(p.plan_warnings.is_empty(), "{:?}", p.plan_warnings);
}

/// [r5-fixes] Harvey's day-63 city: 57 lorries stranded for weeks in a
/// pocket with no way out (from before routes kept out of pockets) were
/// lifted out by free airlifts as soon as it loaded, with no word of why.
/// The first lift now says why and how many more wait, the rest go
/// quietly two at a time; a vehicle stranded for hours with no road
/// anywhere (not in a pocket) is lifted out too.
#[test]
fn a_free_airlift_says_why_it_was_sent() {
    let mut w = flat();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-100.0, 100.0), Vec2::new(100.0, 100.0)], 1, 1);
    w.add_road(&[Vec2::new(100.0, 0.0), Vec2::new(100.0, 100.0)], 1, 0);
    w.implement();
    let side = w.roads.nearest_street(Vec2::new(0.0, 100.0)).unwrap().0;
    let pocket = w.traffic.net.segment_lanes[&side][0];
    assert!(w.traffic.net.trap[pocket as usize]);
    let cars: Vec<CarId> = (0..3).map(|k| w.traffic.spawn_car(CitizenId::default(), None, vec![pocket], 20.0 + 15.0 * k as f32, 150.0, 0)).collect();
    for &c in &cars {
        w.traffic.pull_in(c, 120.0);
    }
    w.drain_events();
    assert!(w.rescue_trapped(cars[0]));
    let said: Vec<String> = w.drain_events().into_iter().filter_map(|e| match e {
        SimEvent::Airlift { text, .. } => Some(text),
        _ => None,
    }).collect();
    assert_eq!(said.len(), 1, "{said:?}");
    assert!(said[0].starts_with("Airlift (free): a vehicle stuck in a dead-end pocket off "), "{}", said[0]);
    assert!(said[0].contains("one-way streets lead in, none lead out") && said[0].contains("2 more wait there"), "{}", said[0]);
    assert!(!w.airlifts[0].reason.is_empty());
    assert!(w.rescue_trapped(cars[1]));
    assert!(!w.drain_events().iter().any(|e| matches!(e, SimEvent::Airlift { .. })), "one line, not one per lift");
    assert!(!w.rescue_trapped(cars[2]), "two at a time: it waits its turn");
    // Stranded on the main street (no pocket), for a few hours only: not
    // yet; for a day, far over its trip's time: lifted out too.
    let main = w.roads.nearest_street(Vec2::new(-200.0, 0.0)).unwrap().0;
    let lane = w.traffic.net.segment_lanes[&main][0];
    let lost = w.traffic.spawn_car(CitizenId::default(), None, vec![lane], 30.0, 150.0, 0);
    w.airlifts.clear();
    w.clock.tick += 3000;
    w.traffic.cars[lost].drive.expected_min = 30.0;
    w.traffic.cars[lost].drive.trip_start = w.clock.tick - 120;
    assert!(!w.rescue_trapped(lost), "two hours late is traffic, not stranded");
    w.traffic.cars[lost].drive.trip_start = w.clock.tick - 1440;
    assert!(w.rescue_trapped(lost));
    assert!(w.airlifts[0].reason.contains("stranded on"), "{}", w.airlifts[0].reason);
}
