//! [w1-roads] A road's use is set on the road, not read from its lanes:
//! a three-lane one-way street set to Mixed keeps its pavements and the
//! lots along it; a road from before keeps what its lanes made it.

use super::*;
use crate::roads::RoadUse;

fn flat() -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
    w.economy.treasury = 5_000_000.0;
    w
}

fn one_way(car_only: Option<bool>) -> Gesture {
    Gesture::Road { points: vec![Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], lanes_fwd: 3, lanes_bwd: 0, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only }
}

fn build(w: &mut World, g: Gesture) -> GestureId {
    let id = w.add_gesture(g);
    w.try_implement().expect("implemented");
    id
}

fn the_street(w: &World) -> &crate::roads::Segment {
    let (sid, _, _) = w.roads.nearest_street(Vec2::new(0.0, 0.0)).expect("a street");
    w.roads.segment(sid)
}

#[test]
fn a_three_lane_one_way_mixed_street_keeps_its_pavements_and_lots() {
    let mut w = flat();
    build(&mut w, one_way(Some(false)));
    let s = the_street(&w);
    assert_eq!(s.road_use(), RoadUse::Mixed);
    assert!(!s.is_highway() && !s.is_motorway() && !s.fronts_nothing(), "a street, not a highway");
    // Lots along it: zoning beside it takes.
    w.add_zone(vec![Vec2::new(-380.0, 8.0), Vec2::new(380.0, 8.0), Vec2::new(380.0, 60.0), Vec2::new(-380.0, 60.0)], LandUse::Residential);
    assert!(w.lots.values().any(|l| l.land_use == LandUse::Residential && l.car_access()), "lots front the mixed street");
    // A street crossing it meets it at a junction, as streets do.
    w.add_gesture(Gesture::Road { points: vec![Vec2::new(0.0, -200.0), Vec2::new(0.0, 200.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: Some(false) });
    w.try_implement().unwrap();
    let n = w.roads.node_near(Vec2::ZERO, 1.0).expect("a junction");
    assert_eq!(w.roads.node(n).segments.iter().filter(|&&s| w.roads.try_segment(s).is_some()).count(), 4);
}

#[test]
fn the_same_road_set_to_cars_only_is_a_highway_and_fronts_nothing() {
    let mut w = flat();
    build(&mut w, one_way(Some(true)));
    let s = the_street(&w);
    assert_eq!(s.road_use(), RoadUse::CarOnly);
    assert!(s.is_highway() && s.fronts_nothing());
    // Two lanes, cars only: still no pavements, though not a highway.
    let mut w2 = flat();
    build(&mut w2, Gesture::Road { points: vec![Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], lanes_fwd: 2, lanes_bwd: 0, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: Some(true) });
    let s = the_street(&w2);
    assert!(!s.is_highway() && s.is_motorway() && s.fronts_nothing());
}

#[test]
fn an_older_road_keeps_the_use_its_lanes_gave_it_and_loads_so() {
    let mut w = flat();
    let id = build(&mut w, one_way(None));
    assert!(the_street(&w).is_highway(), "a 3-lane one-way road from before is a highway");
    // A save from before the field: no car_only anywhere.
    let ron = w.to_ron().expect("saved");
    assert!(!ron.contains("car_only"), "unset uses are not written");
    let w2 = World::from_ron(&ron).expect("loaded");
    assert!(the_street(&w2).is_highway(), "still a highway after loading");
    // Switching it to Mixed (the Edit panel) brings its pavements back.
    let mut w = w;
    let Some(Gesture::Road { points, lanes_fwd, lanes_bwd, curved, kind, elevation, .. }) = w.gesture(id) else { panic!() };
    w.set_gesture(id, Gesture::Road { points, lanes_fwd, lanes_bwd, curved, kind, elevation, car_only: Some(false) }, true);
    w.try_implement().unwrap();
    assert!(!the_street(&w).fronts_nothing(), "pavements back");
    let ron = w.to_ron().expect("saved");
    let w3 = World::from_ron(&ron).expect("loaded");
    assert_eq!(the_street(&w3).road_use(), RoadUse::Mixed, "the use set is saved");
}

#[test]
fn road_use_is_read_as_before_when_unset() {
    assert_eq!(RoadUse::of(RoadKind::Street, 3, 0, None), RoadUse::CarOnly);
    assert_eq!(RoadUse::of(RoadKind::Street, 2, 0, None), RoadUse::Mixed);
    assert_eq!(RoadUse::of(RoadKind::Street, 3, 0, Some(false)), RoadUse::Mixed);
    assert_eq!(RoadUse::of(RoadKind::Street, 1, 1, Some(true)), RoadUse::CarOnly);
    assert_eq!(RoadUse::of(RoadKind::Path, 0, 0, None), RoadUse::Pedestrian);
    assert_eq!(RoadUse::parse("car_only"), Some(RoadUse::CarOnly));
    assert_eq!(RoadUse::parse("Mixed"), Some(RoadUse::Mixed));
    assert_eq!(RoadUse::parse("pedestrian"), Some(RoadUse::Pedestrian));
}
