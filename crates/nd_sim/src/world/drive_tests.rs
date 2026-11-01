//! [r5-drive] What the player's car drives on.

use super::*;

fn town() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0)], 1, 1);
    let mut minds = crate::mind::IdleMind;
    w.tick(&mut minds);
    w
}

#[test]
fn roads_junctions_and_the_ground_off_them() {
    let w = town();
    let h = w.surface_height(Vec2::new(100.0, 0.0));
    let on = w.drive_ground(Vec2::new(100.0, 1.0), h);
    assert!(matches!(on, Ok(DriveGround::Road { .. })), "{on:?}");
    assert!(matches!(w.drive_ground(Vec2::new(0.5, 0.5), h), Ok(DriveGround::Junction { .. })));
    // Well off the street: the ground, drivable.
    let off = w.drive_ground(Vec2::new(100.0, 40.0), h);
    assert!(matches!(off, Ok(DriveGround::Off { .. })), "{off:?}");
    // Past the land: the edge.
    let edge = w.drive_limit() + 10.0;
    assert_eq!(w.drive_ground(Vec2::new(edge, 0.0), h), Err(DriveBlock::Edge));
    // A floor far above (a deck that is not there): a drop, not a road.
    assert_eq!(w.drive_ground(Vec2::new(100.0, 40.0), h + 8.0), Err(DriveBlock::Wall));
}

#[test]
fn the_street_has_a_name_and_a_limit() {
    let w = town();
    let p = Vec2::new(120.0, 1.5);
    let g = w.drive_ground(p, 0.0).unwrap();
    assert!(w.drive_street_name(&g).is_some_and(|n| !n.is_empty()));
    let limit = w.drive_limit_at(&g, p, 0.0).expect("limit");
    assert!((5.0..40.0).contains(&limit), "{limit}");
    let (_, d, side) = w.drive_lane(&g, p, 0.0).expect("lane");
    assert!(d.x > 0.9 && side.abs() < 3.0, "{d:?} {side}");
}

#[test]
fn the_traffic_sees_the_player_on_its_road_and_junction() {
    let w = town();
    let on_street = w.player_car(Vec2::new(120.0, 1.5), 0.0, 0.0, 0.0, 0.0);
    assert!(!on_street.segments.is_empty() && on_street.nodes.is_empty(), "{on_street:?}");
    let in_junction = w.player_car(Vec2::new(0.0, 1.0), 0.0, 0.0, 0.0, 0.0);
    assert!(!in_junction.nodes.is_empty(), "{in_junction:?}");
    // Far above (a viaduct that is not there): no piece at its level.
    let above = w.player_car(Vec2::new(120.0, 1.5), 0.0, 0.0, 0.0, 9.0);
    assert!(above.segments.is_empty() && above.nodes.is_empty());
}
