//! Adding a road onto an existing one never changes the existing road's
//! lanes: a 1-lane ramp joined to a highway's start, end or middle leaves
//! every piece of the highway as it was laid, and keeps its own lanes.

use nd_sim::landscape::LandscapeParams;
use nd_sim::plan::Gesture;
use nd_sim::roads::RoadKind;
use nd_sim::{Vec2, World, WorldConfig};

use nd_sim::geom::distance_to_polyline as dist_to_polyline;

/// Every alive piece lying along `pts` (midpoint on it, both ends on it).
fn lanes_along(w: &World, pts: &[Vec2]) -> Vec<(u8, u8)> {
    w.roads
        .alive_segments()
        .filter(|(sid, _)| {
            let (a, b) = w.roads.segment_points(*sid);
            [a, b, (a + b) * 0.5].iter().all(|&p| dist_to_polyline(p, pts) < 0.5)
        })
        .map(|(sid, s)| {
            // Oriented along the polyline: forward when `a` is nearer its start.
            let (a, b) = w.roads.segment_points(sid);
            let along = |p: Vec2| {
                let mut acc = 0.0;
                let mut best = (f32::INFINITY, 0.0);
                for win in pts.windows(2) {
                    let d = win[1] - win[0];
                    let t = ((p - win[0]).dot(d) / d.length_squared()).clamp(0.0, 1.0);
                    let e = p.distance(win[0] + d * t);
                    if e < best.0 {
                        best = (e, acc + d.length() * t);
                    }
                    acc += d.length();
                }
                best.1
            };
            if along(a) <= along(b) { (s.lanes_fwd, s.lanes_bwd) } else { (s.lanes_bwd, s.lanes_fwd) }
        })
        .collect()
}

fn highway() -> Vec<Vec2> {
    vec![Vec2::new(-400.0, 0.0), Vec2::new(-150.0, 10.0), Vec2::new(100.0, 0.0), Vec2::new(400.0, -20.0)]
}

fn lay(w: &mut World, pts: &[Vec2], lanes: (u8, u8), curved: bool) {
    w.add_gesture(Gesture::Road { points: pts.to_vec(), lanes_fwd: lanes.0, lanes_bwd: lanes.1, curved, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
    w.implement();
}

fn check(name: &str, main: (u8, u8), curved: bool, ramp_pts: &[Vec2], ramp: (u8, u8)) {
    let mut w = World::new(WorldConfig { landscape: LandscapeParams::flat(), ..WorldConfig::default() });
    let drawn = highway();
    lay(&mut w, &drawn, main, curved);
    let hw = if curved { nd_sim::plan::sample_curve(&drawn) } else { drawn };
    let before = lanes_along(&w, &hw);
    assert!(!before.is_empty() && before.iter().all(|&l| l == main), "{name}: highway laid with its lanes: {before:?}");
    lay(&mut w, ramp_pts, ramp, false);
    let after = lanes_along(&w, &hw);
    assert!(!after.is_empty(), "{name}: highway still there");
    assert!(after.iter().all(|&l| l == main), "{name}: highway keeps {main:?} on every piece, got {after:?}");
    // The new piece keeps its own lanes (its pieces away from the highway).
    let own: Vec<_> = w
        .roads
        .alive_segments()
        .filter(|(sid, _)| {
            let (a, b) = w.roads.segment_points(*sid);
            let m = (a + b) * 0.5;
            dist_to_polyline(m, &hw) > 1.0 && dist_to_polyline(m, ramp_pts) < 30.0
        })
        .map(|(_, s)| (s.lanes_fwd, s.lanes_bwd))
        .collect();
    assert!(!own.is_empty() && own.iter().all(|&(f, b)| (f, b) == ramp || (b, f) == ramp), "{name}: the new road keeps {ramp:?}: {own:?}");
}

fn variants(name: &str, main: (u8, u8), ramp: (u8, u8)) {
    for curved in [false, true] {
        variants_of(&format!("{name}{}", if curved { ", curved" } else { "" }), main, curved, ramp);
    }
}

fn variants_of(name: &str, main: (u8, u8), curved: bool, ramp: (u8, u8)) {
    let hw = highway();
    let (s, e) = (hw[0], *hw.last().unwrap());
    // Ending at the start, from behind and from the side.
    check(&format!("{name} ends at start (behind)"), main, curved, &[s + Vec2::new(-200.0, 30.0), s + Vec2::new(-80.0, 5.0), s], ramp);
    check(&format!("{name} ends at start (side)"), main, curved, &[s + Vec2::new(30.0, -200.0), s + Vec2::new(5.0, -80.0), s], ramp);
    // Starting at the start, leaving sideways.
    check(&format!("{name} starts at start"), main, curved, &[s, s + Vec2::new(-60.0, 120.0), s + Vec2::new(-80.0, 250.0)], ramp);
    // Starting at the end, running on.
    check(&format!("{name} starts at end"), main, curved, &[e, e + Vec2::new(100.0, -10.0), e + Vec2::new(250.0, 30.0)], ramp);
    check(&format!("{name} ends at end"), main, curved, &[e + Vec2::new(30.0, 250.0), e + Vec2::new(10.0, 100.0), e], ramp);
    // Joining mid-way.
    let mid = Vec2::new(-20.0, 5.0);
    check(&format!("{name} joins mid-way"), main, curved, &[mid + Vec2::new(-150.0, -150.0), mid + Vec2::new(-60.0, -60.0), mid], ramp);
}

#[test]
fn a_ramp_never_changes_a_one_way_highway() {
    variants("4-lane highway", (4, 0), (1, 0));
}

#[test]
fn a_ramp_never_changes_a_divided_highway_carriageway() {
    variants("3-lane carriageway", (3, 0), (1, 0));
    variants("3+3 avenue", (3, 3), (1, 0));
}

#[test]
fn a_ramp_never_changes_a_street() {
    variants("2+2 street", (2, 2), (1, 0));
    variants("2+2 street, 1+1 road", (2, 2), (1, 1));
}

/// Both carriageways of a divided highway keep three lanes when a slip
/// road is joined to the start of one and the end of the other.
#[test]
fn a_divided_highway_keeps_both_carriageways() {
    let mut w = World::new(WorldConfig { landscape: LandscapeParams::flat(), ..WorldConfig::default() });
    let east = highway();
    let west: Vec<Vec2> = east.iter().rev().map(|p| *p + Vec2::new(0.0, -30.0)).collect();
    lay(&mut w, &east, (3, 0), false);
    lay(&mut w, &west, (3, 0), false);
    // An on-ramp onto the eastbound start, an off-ramp from the westbound end.
    lay(&mut w, &[east[0] + Vec2::new(-60.0, 150.0), east[0] + Vec2::new(-20.0, 50.0), east[0]], (1, 0), false);
    let w_end = *west.last().unwrap();
    lay(&mut w, &[w_end, w_end + Vec2::new(-30.0, -60.0), w_end + Vec2::new(-80.0, -180.0)], (1, 0), false);
    for (name, c) in [("eastbound", &east), ("westbound", &west)] {
        let l = lanes_along(&w, c);
        assert!(!l.is_empty() && l.iter().all(|&x| x == (3, 0)), "{name} keeps three lanes: {l:?}");
    }
}
