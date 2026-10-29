//! [r4-ramp] The Road tool drawing a ramp between roads at different
//! heights (Harvey 2026-10-28): a point clicked on a road takes that road's
//! height while it is the end, whatever the Level dial says; the dial sets
//! only the points on open ground; the preview, the grade check, the
//! columns and the gesture laid all see the same heights.

use super::*;
use crate::sim::Sim;
use nd_sim::roads::RoadKind;
use nd_sim::{Gesture, Vec2 as SVec2, World, WorldConfig};

pub(crate) const DECK: f32 = 12.0;
pub(crate) const LOW_DECK: f32 = 6.0;
const T: f32 = nd_sim::roads::TUNNEL_DEPTH;

fn raised(w: &mut World, pts: &[SVec2], levels: &[f32]) {
    w.project = None;
    w.add_gesture(Gesture::Road { points: pts.to_vec(), lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: levels.to_vec(), car_only: None });
    w.try_implement().expect("built");
}

/// A flat city: streets at grade along y = 0 and y = 120, a 12 m viaduct
/// along y = 200 (up from x = -500 to 12 m at -300, down again from 300),
/// a 6 m one along y = 400, a street along x = -100 under the viaduct and a
/// tunnel at -8 m along y = -200.
pub(crate) fn ramp_town() -> Sim {
    let mut sim = Sim::new();
    let mut w = World::new(WorldConfig { landscape: nd_sim::LandscapeParams::flat(), ..WorldConfig::default() });
    let v = |x: f32, y: f32| SVec2::new(x, y);
    w.add_road(&[v(-500.0, 0.0), v(500.0, 0.0)], 1, 1);
    w.add_road(&[v(-500.0, 120.0), v(500.0, 120.0)], 1, 1);
    raised(&mut w, &[v(-500.0, 200.0), v(-300.0, 200.0), v(300.0, 200.0), v(500.0, 200.0)], &[0.0, DECK, DECK, 0.0]);
    raised(&mut w, &[v(-500.0, 400.0), v(-380.0, 400.0), v(380.0, 400.0), v(500.0, 400.0)], &[0.0, LOW_DECK, LOW_DECK, 0.0]);
    w.add_road(&[v(-100.0, 120.0), v(-100.0, 300.0)], 1, 1);
    raised(&mut w, &[v(-500.0, -200.0), v(-300.0, -200.0), v(300.0, -200.0), v(500.0, -200.0)], &[0.0, T, T, 0.0]);
    sim.world = w;
    sim
}

fn tools_at(dial: f32) -> ToolState {
    let mut t = ToolState::default();
    t.tool = Tool::Road;
    t.kind = RoadKind::Street;
    (t.lanes_fwd, t.lanes_bwd) = (1, 1);
    t.elevation = dial;
    t
}

fn click(t: &mut ToolState, sim: &Sim, x: f32, y: f32) {
    let dial = t.elevation;
    t.click_road_point(sim, SVec2::new(x, y), dial);
}

fn steep(sim: &Sim, t: &ToolState) -> Vec<String> {
    sim.world.road_grade_warnings(&t.points, &t.levels(), t.kind, 1, 1).into_iter().filter(|m| m.starts_with("too steep")).collect()
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.5
}

/// Harvey's ramp: two points, from the viaduct down to a street. Its ends
/// sit at the two roads' heights with the dial at the ground, at the deck
/// or anywhere else; a long enough ramp has no warning, a short one says
/// it is too steep and how long it must be, the same at every dial.
#[test]
fn a_ramp_from_a_viaduct_to_a_street_takes_both_roads_heights_at_any_dial() {
    let sim = ramp_town();
    for dial in [0.0, 5.0, DECK, 18.0] {
        let mut t = tools_at(dial);
        click(&mut t, &sim, 0.0, 203.0);
        click(&mut t, &sim, 0.0, 2.0);
        let l = t.levels();
        assert!(near(l[0], DECK) && near(l[1], 0.0), "dial {dial}: {l:?}");
        assert!(near(t.points[0].y, 200.0) && near(t.points[1].y, 0.0), "dial {dial}: {:?}", t.points);
        assert!(steep(&sim, &t).is_empty(), "dial {dial}: 12 m over 200 m: {:?}", steep(&sim, &t));
        let mut t = tools_at(dial);
        click(&mut t, &sim, 0.0, 203.0);
        click(&mut t, &sim, 0.0, 121.0);
        let w = steep(&sim, &t);
        assert!(w.len() == 1 && w[0].contains("a ramp needs 171 m for 12 m at 7%"), "dial {dial}: {w:?}");
    }
}

/// The other way, up: from the street onto the deck, and onto the middle
/// of the viaduct's own ramp (between its nodes) at its height there.
#[test]
fn a_ramp_up_ends_on_the_deck_or_the_middle_of_its_ramp_at_its_height() {
    let sim = ramp_town();
    let mut t = tools_at(0.0);
    click(&mut t, &sim, 60.0, 1.0);
    click(&mut t, &sim, 60.0, 198.0);
    assert!(near(t.levels()[1], DECK), "{:?}", t.levels());
    let mut t = tools_at(0.0);
    click(&mut t, &sim, -400.0, 121.0);
    click(&mut t, &sim, -400.0, 197.0);
    let l = t.levels();
    assert!(near(l[0], 0.0) && (l[1] - DECK / 2.0).abs() < 0.6 && near(t.points[1].y, 200.0), "{l:?} {:?}", t.points);
}

/// A point between the ends is put at the dial's level on open ground and
/// snaps only onto a road at about that level: the last point clicked on
/// the deck was the end at 12 m, and is put again at grade, under the deck,
/// when the road goes on past it; at the crossing of the deck and the
/// street under it the street at grade wins.
#[test]
fn a_point_between_snaps_only_at_its_level() {
    let sim = ramp_town();
    let mut t = tools_at(0.0);
    click(&mut t, &sim, 40.0, 1.0);
    click(&mut t, &sim, 40.0, 202.0);
    assert!(near(t.levels()[1], DECK), "the end, for now: {:?}", t.levels());
    // The preview with the pointer further on shows it at grade already.
    let (pts, lv) = t.road_with_cursor(&sim, Some(SVec2::new(40.0, 300.0)));
    assert_eq!(pts.len(), 3);
    assert!(near(lv[1], 0.0) && near(pts[1].y, 202.0), "{lv:?} {pts:?}");
    click(&mut t, &sim, 40.0, 300.0);
    assert_eq!(t.levels(), lv, "clicking there lays what the preview showed");
    assert_eq!(t.points, pts);
    // Through the crossing under the deck: on the street at grade.
    let mut t = tools_at(0.0);
    click(&mut t, &sim, -200.0, 140.0);
    click(&mut t, &sim, -100.0, 202.0);
    click(&mut t, &sim, 0.0, 260.0);
    assert!(near(t.levels()[1], 0.0) && near(t.points[1].x, -100.0), "{:?} {:?}", t.levels(), t.points);
}

/// Both ends raised at different heights (the 12 m viaduct to the 6 m
/// one): each end at its deck's height, the point on open ground between
/// at the dial's level; a portal from the street down into the tunnel.
#[test]
fn both_ends_raised_and_a_tunnel_end_take_their_roads_heights() {
    let sim = ramp_town();
    let mut t = tools_at(9.0);
    click(&mut t, &sim, 0.0, 201.0);
    click(&mut t, &sim, 0.0, 300.0);
    click(&mut t, &sim, 0.0, 399.0);
    assert_eq!(t.levels(), vec![DECK, 9.0, LOW_DECK]);
    assert!(steep(&sim, &t).is_empty(), "{:?}", steep(&sim, &t));
    // Down into the tunnel from the street, the dial at the ground.
    let mut t = tools_at(0.0);
    click(&mut t, &sim, 100.0, 1.0);
    click(&mut t, &sim, 100.0, -198.0);
    assert!(near(t.levels()[0], 0.0) && near(t.levels()[1], T), "{:?}", t.levels());
    assert!(steep(&sim, &t).is_empty(), "8 m in 200 m");
}

/// Raising the whole road a step ([ / ]) lifts the points on open ground
/// and leaves those on a road at that road's height.
#[test]
fn raising_the_whole_road_leaves_the_points_on_roads_alone() {
    let sim = ramp_town();
    let mut t = tools_at(6.0);
    click(&mut t, &sim, 0.0, 201.0);
    click(&mut t, &sim, 0.0, 100.0);
    click(&mut t, &sim, 0.0, 1.0);
    t.step_whole_road(true);
    let l = t.levels();
    assert!(near(l[0], DECK) && l[1] > 6.0 && near(l[2], 0.0), "{l:?}");
}
