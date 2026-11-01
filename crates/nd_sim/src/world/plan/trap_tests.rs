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
