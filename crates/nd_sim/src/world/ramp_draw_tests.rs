//! [r4-ramp] A ramp drawn between two roads at different heights: its ends
//! take the heights of the roads they land on, and the grade check judges
//! the ramp that will be built (Harvey 2026-10-28: a two-point road from a
//! raised road down to a street said "too steep", or said nothing once the
//! Level dial was raised, while the same ramp was built either way).

use super::*;
use super::water::STREET_GRADE;

const DECK: f32 = 12.0;
const LOW_DECK: f32 = 6.0;
const T: f32 = crate::roads::TUNNEL_DEPTH;

fn flat_world() -> World {
    World::new(WorldConfig { landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::default() })
}

fn road(points: &[Vec2], elevation: &[f32]) -> Gesture {
    Gesture::Road { points: points.to_vec(), lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: elevation.to_vec(), car_only: None }
}

fn build(w: &mut World, g: Gesture) {
    w.project = None;
    w.add_gesture(g);
    w.try_implement().expect("built");
}

/// Streets at grade along y = 0 and y = 120, a 12 m viaduct along y = 200
/// (ramps from x = -500 up to 12 m at x = -300 and down again from 300), a
/// 6 m one along y = 400, a street at grade along x = -100 passing under
/// the viaduct, and a tunnel at -8 m along y = -200.
fn ramp_town() -> World {
    let mut w = flat_world();
    w.add_road(&[Vec2::new(-500.0, 0.0), Vec2::new(500.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-500.0, 120.0), Vec2::new(500.0, 120.0)], 1, 1);
    build(&mut w, road(&[Vec2::new(-500.0, 200.0), Vec2::new(-300.0, 200.0), Vec2::new(300.0, 200.0), Vec2::new(500.0, 200.0)], &[0.0, DECK, DECK, 0.0]));
    build(&mut w, road(&[Vec2::new(-500.0, 400.0), Vec2::new(-380.0, 400.0), Vec2::new(380.0, 400.0), Vec2::new(500.0, 400.0)], &[0.0, LOW_DECK, LOW_DECK, 0.0]));
    w.add_road(&[Vec2::new(-100.0, 120.0), Vec2::new(-100.0, 300.0)], 1, 1);
    build(&mut w, road(&[Vec2::new(-500.0, -200.0), Vec2::new(-300.0, -200.0), Vec2::new(300.0, -200.0), Vec2::new(500.0, -200.0)], &[0.0, T, T, 0.0]));
    w
}

/// A point is snapped onto the road at its own level where two pass over
/// each other; with nothing at its level, only an end (`any`) comes up or
/// down to a road at another level, at that road's height there: the deck,
/// a deck's middle on its ramp (no node there), the tunnel.
#[test]
fn a_point_snaps_to_the_road_at_its_level_and_an_end_to_any() {
    let w = ramp_town();
    let r = &w.roads;
    let crossing = Vec2::new(-100.0, 201.0);
    let (q, l) = r.snap_point_level(crossing, 8.0, RoadKind::Street, 0.0, false).expect("the street under the deck");
    assert!(l.abs() < 0.5 && (q.x + 100.0).abs() < 0.5, "{q:?} {l}");
    let (_, l) = r.snap_point_level(crossing, 8.0, RoadKind::Street, DECK, false).expect("the deck over the street");
    assert!((l - DECK).abs() < 0.5, "{l}");
    // On the deck with nothing at grade in reach: a point between does not
    // snap (a road drawn under the viaduct passes under it), an end does.
    let deck = Vec2::new(40.0, 203.0);
    assert!(r.snap_point_level(deck, 8.0, RoadKind::Street, 0.0, false).is_none());
    let (q, l) = r.snap_point_level(deck, 8.0, RoadKind::Street, 0.0, true).expect("an end lands on the deck");
    assert!((l - DECK).abs() < 0.5 && (q.y - 200.0).abs() < 0.5, "{q:?} {l}");
    // Half way up the viaduct's ramp, between its nodes: half its height.
    let (q, l) = r.snap_point_level(Vec2::new(-400.0, 196.0), 8.0, RoadKind::Street, 0.0, true).expect("the ramp");
    assert!((l - DECK / 2.0).abs() < 0.6 && (q.y - 200.0).abs() < 0.5, "{q:?} {l}");
    // The tunnel, from the ground.
    let (_, l) = r.snap_point_level(Vec2::new(0.0, -197.0), 8.0, RoadKind::Street, 0.0, true).expect("the tunnel");
    assert!((l - T).abs() < 0.5, "{l}");
}

/// The warnings judge the ramp as built: an end drawn at the deck's level
/// over a street is laid on the street (`as_built`), so a short ramp from
/// the viaduct says it is too steep and how long it must be, whatever
/// level its end was drawn at; a long enough one says nothing.
#[test]
fn the_grade_check_judges_the_ramp_as_built_and_says_how_long_it_must_be() {
    let w = ramp_town();
    let (top, short, long) = (Vec2::new(0.0, 200.0), Vec2::new(0.0, 120.0), Vec2::new(0.0, 0.0));
    let need = DECK / STREET_GRADE;
    for end in [DECK, 0.0] {
        let warn = w.road_grade_warnings(&[top, short], &[DECK, end], RoadKind::Street, 1, 1);
        let steep = warn.iter().find(|m| m.starts_with("too steep")).unwrap_or_else(|| panic!("end at {end}: {warn:?}"));
        assert!(steep.contains(&format!("a ramp needs {need:.0} m for 12 m at 7%")), "{steep}");
        let ok = w.road_grade_warnings(&[top, long], &[DECK, end], RoadKind::Street, 1, 1);
        assert!(!ok.iter().any(|m| m.starts_with("too steep")), "end at {end}: {ok:?}");
    }
    // Both ends raised, 6 m apart over 200 m: fine.
    let ok = w.road_grade_warnings(&[Vec2::new(0.0, 200.0), Vec2::new(0.0, 400.0)], &[DECK, LOW_DECK], RoadKind::Street, 1, 1);
    assert!(!ok.iter().any(|m| m.starts_with("too steep")), "{ok:?}");
}

/// The ramp from the viaduct down to the street at grade is built joined
/// to both, at their heights, with its columns and no gap: the plan view's
/// check (`active_project_check`, its `gap_touches`) and implementing agree
/// with the preview's column plan.
#[test]
fn a_ramp_between_a_viaduct_and_a_street_is_built_at_both_heights() {
    let mut w = ramp_town();
    let pts = [Vec2::new(0.0, 200.0), Vec2::new(0.0, 0.0)];
    let levels = [DECK, 0.0];
    let plan = w.road_pillar_plan(&pts, &levels, 1, 1, RoadKind::Street);
    assert!(plan.ok() && !plan.pillars.is_empty(), "{:?}", plan.gaps);
    w.project = None;
    w.add_gesture(road(&pts, &levels));
    let check = w.active_project_check();
    assert!(check.pillar_gaps.is_empty() && check.pillar_refusal.is_none(), "{:?}", check.pillar_gaps);
    w.try_implement().expect("built");
    for (p, l) in pts.iter().zip(levels) {
        let n = w.roads.nodes.iter().find(|n| n.pos.distance(*p) < 0.5 && n.segments.len() >= 3).unwrap_or_else(|| panic!("a junction at {p:?}"));
        assert!((n.level - l).abs() < 0.5, "{p:?}: {} wanted {l}", n.level);
    }
    assert!(w.pillar_gaps.is_empty(), "{:?}", w.pillar_gaps);
}
