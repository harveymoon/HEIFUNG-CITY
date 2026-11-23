//! [r15-lots] Lots judged by their shape without the booleans' crumbs,
//! backland with no street, and scraps folded into a neighbour.

use super::*;

/// A street along y = 0 and the line its lots front (the corridor's edge).
fn street() -> (RoadNetwork, f32) {
    let mut r = RoadNetwork::default();
    r.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    let (sid, _) = r.alive_segments().next().unwrap();
    let h = r.segment(sid).corridor_half_width();
    (r, h)
}

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

fn lot_of(poly: Vec<Vec2>, roads: &RoadNetwork) -> Lot {
    let c = inner_center(&poly);
    make_lot(ZoneId::default(), LandUse::Residential, poly, c, roads).expect("a lot")
}

/// (F1) A house plot with a half-metre step in its back fence and a
/// hairline tail where a cut slid past: both were "sharp" or a "sliver"
/// and the plot dead ground; judged without the crumbs it is a lot.
#[test]
fn a_lot_is_not_dead_for_its_outlines_crumbs() {
    let (roads, h) = street();
    let step = vec![v(0.0, h), v(20.0, h), v(20.0, h + 30.0), v(10.0, h + 30.0), v(10.0, h + 29.5), v(0.0, h + 29.5)];
    assert!(reflex_vertex(&step, 0.6).is_some(), "the raw step reads as a sharp corner");
    let l = lot_of(step.clone(), &roads);
    assert!(!l.landlocked, "{:?}", landlocked_why(LandUse::Residential, &step, &roads));
    let tail = vec![v(0.0, h), v(20.0, h), v(20.0, h + 30.0), v(10.0, h + 30.0), v(10.3, h + 55.0), v(9.7, h + 30.0), v(0.0, h + 30.0)];
    assert!(reflex_vertex(&tail, 0.6).is_some() || polygon_area(&tail) / perimeter(&tail).powi(2) < MIN_COMPACTNESS, "the raw tail reads as dead");
    let l = lot_of(tail.clone(), &roads);
    assert!(!l.landlocked, "{:?}", landlocked_why(LandUse::Residential, &tail, &roads));
    assert!(l.polygon.iter().all(|p| p.y <= h + 30.01), "the hairline is cut off the lot: {:?}", l.polygon);
    assert!((l.area() - 600.0).abs() < 1.0, "and nothing else: {}", l.area());
    assert!(l.car_access());
}

/// (F1) A real L and a real shard stay what they are.
#[test]
fn a_real_notch_or_shard_is_still_no_lot() {
    let (roads, h) = street();
    let ell = vec![v(0.0, h), v(30.0, h), v(30.0, h + 15.0), v(15.0, h + 15.0), v(15.0, h + 30.0), v(0.0, h + 30.0)];
    assert_eq!(landlocked_why(LandUse::Residential, &ell, &roads), Some("sharp_corner"));
    assert!(lot_of(ell, &roads).landlocked);
    let shard = vec![v(0.0, h), v(40.0, h), v(0.0, h + 3.0)];
    assert!(landlocked_why(LandUse::Residential, &shard, &roads).is_some());
}

/// (F1) Backland with no way at any edge has no car access: its card
/// read `car_access: true` off the nearest street it hangs on.
#[test]
fn backland_has_no_car_access() {
    let (roads, h) = street();
    let back = vec![v(0.0, h + 40.0), v(20.0, h + 40.0), v(20.0, h + 70.0), v(0.0, h + 70.0)];
    let l = lot_of(back.clone(), &roads);
    assert!(l.landlocked);
    assert_eq!(landlocked_why(LandUse::Residential, &back, &roads), Some("no_frontage"));
    assert!(l.car_segment.is_none() && !l.car_access(), "{:?}", l.car_segment);
}

/// (F1, F8) A thin wedge beside a plot and a small piece of backland
/// behind it are folded into it; a plot of its own is left alone.
#[test]
fn scraps_are_folded_into_the_lot_beside_them() {
    let (roads, h) = street();
    let plot = vec![v(0.0, h), v(16.0, h), v(16.0, h + 24.0), v(0.0, h + 24.0)];
    let wedge = vec![v(16.0, h), v(20.0, h), v(16.0, h + 24.0)];
    let other = vec![v(40.0, h), v(56.0, h), v(56.0, h + 24.0), v(40.0, h + 24.0)];
    assert!(landlocked_why(LandUse::Residential, &wedge, &roads).is_some(), "the wedge is no lot alone");
    let out = join_parcels(&[plot.clone(), wedge, other], LandUse::Residential, &roads);
    assert_eq!(out.len(), 2, "{out:?}");
    let joined = out.iter().find(|(m, _)| m.contains(&0)).unwrap();
    assert_eq!(joined.0, vec![0, 1]);
    assert!(landlocked_why(LandUse::Residential, &joined.1, &roads).is_none(), "{:?}", joined.1);
    // Backland behind the plot, under 60% of a lot.
    let back = vec![v(0.0, h + 24.0), v(16.0, h + 24.0), v(16.0, h + 34.0), v(0.0, h + 34.0)];
    let out = join_parcels(&[plot, back], LandUse::Residential, &roads);
    assert_eq!(out.len(), 1, "{out:?}");
}


