//! [r1-roads] Road and path tool flags (K1, K5, K12–K15, K47, K48 and the
//! section C bikes under footbridges): raised paths over other ways, paths
//! up a hillside, crosswalks by paths and dead ends.

use super::*;
use crate::walk::CrossingKind;

fn flat() -> World {
    World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() })
}

fn path(points: &[Vec2], levels: &[f32]) -> Gesture {
    Gesture::Road { points: points.to_vec(), lanes_fwd: 0, lanes_bwd: 0, curved: false, kind: RoadKind::Path, elevation: levels.to_vec(), car_only: None }
}

fn v(x: f32, y: f32) -> Vec2 {
    Vec2::new(x, y)
}

/// Every node's level, by position, for comparing networks.
fn levels(w: &World) -> Vec<(i32, i32, i32)> {
    let mut out: Vec<(i32, i32, i32)> = w.roads.nodes.iter().filter(|n| n.segments.iter().any(|&s| w.roads.segment(s).alive)).map(|n| ((n.pos.x * 10.0) as i32, (n.pos.y * 10.0) as i32, (n.level * 10.0) as i32)).collect();
    out.sort();
    out
}

/// K1, K12: a path drawn a few metres over a street and a path at grade is
/// refused (it used to meet them at junctions at its own height, lifting
/// the ways under it up to its deck); high enough to clear them it is built
/// and leaves them on the ground.
#[test]
fn a_raised_path_too_low_to_clear_the_ways_under_it_is_refused() {
    let mut w = flat();
    w.add_road(&[v(-200.0, 0.0), v(200.0, 0.0)], 1, 1);
    w.add_path(&[v(-200.0, 40.0), v(200.0, 40.0)]);
    let before = levels(&w);
    let pts = [v(-60.0, -80.0), v(-60.0, 120.0)];
    let why = w.road_refusal(&pts, &[4.0, 4.0], RoadKind::Path, 0, 0).expect("refused at 4 m");
    assert!(why.contains("too close to pass over"), "{why}");
    // Planned anyway (the API, a script): implementing refuses with the reason.
    w.project = None;
    w.add_gesture(path(&pts, &[4.0, 4.0]));
    let err = w.try_implement().expect_err("implementing refuses");
    assert!(err.contains("crosses"), "{err}");
    // Discarded, nothing under it moved.
    w.discard_project();
    assert_eq!(levels(&w), before, "the ways under it are as they were");
    // At 6 m it clears both and is built; they stay on the ground.
    assert_eq!(w.road_refusal(&pts, &[6.0, 6.0], RoadKind::Path, 0, 0), None);
    w.project = None;
    w.add_gesture(path(&pts, &[6.0, 6.0]));
    w.try_implement().expect("built at 6 m");
    for (sid, s) in w.roads.alive_segments() {
        let (a, b) = w.roads.segment_points(sid);
        if a.y.abs() < 1.0 && b.y.abs() < 1.0 || (a.y - 40.0).abs() < 1.0 && (b.y - 40.0).abs() < 1.0 {
            assert!(w.roads.node(s.a).level.abs() < 1e-3 && w.roads.node(s.b).level.abs() < 1e-3, "{:?} lifted to {} / {}", s.kind, w.roads.node(s.a).level, w.roads.node(s.b).level);
        }
    }
}

/// K12, K13: raising a path in the Edit tool is held to the same rules: a
/// path at grade over a three-lane street raised to 4 m is refused (it was
/// built, met the street at a junction at 4 m, lifting it, and a column
/// stood in its lanes); raised to 6 m it is built and no column stands in
/// the street.
#[test]
fn raising_a_path_in_the_edit_tool_keeps_the_crossing_rules_and_columns_off_the_street() {
    let mut w = flat();
    w.add_road(&[v(-200.0, 0.0), v(200.0, 0.0)], 1, 2);
    w.add_road(&[v(-200.0, 120.0), v(200.0, 120.0)], 1, 1);
    w.add_road(&[v(-200.0, -120.0), v(200.0, -120.0)], 1, 1);
    let id = w.add_path(&[v(30.0, -120.0), v(30.0, 120.0)]);
    let edited = |l: f32| path(&[v(30.0, -120.0), v(30.0, -60.0), v(30.0, 60.0), v(30.0, 120.0)], &[0.0, l, l, 0.0]);
    w.project = None;
    w.set_gesture(id, edited(4.0), true);
    let err = w.try_implement().expect_err("4 m over the street is refused");
    assert!(err.contains("crosses"), "{err}");
    w.discard_project();
    w.project = None;
    w.set_gesture(id, edited(6.0), true);
    w.try_implement().expect("6 m clears it");
    let street: Vec<SegmentId> = w.roads.alive_segments().filter(|(sid, s)| s.is_street() && s.lanes() == 3 && { let (a, b) = w.roads.segment_points(*sid); a.x.min(b.x) <= 30.5 && a.x.max(b.x) >= 29.5 }).map(|(sid, _)| sid).collect();
    assert!(!street.is_empty(), "the street under it");
    for &sid in &street {
        let s = w.roads.segment(sid);
        assert!(w.roads.node(s.a).level.abs() < 1e-3 && w.roads.node(s.b).level.abs() < 1e-3, "the street stays at grade");
    }
    let s = w.roads.segment(street[0]);
    assert!(!w.pillars.is_empty(), "the raised path stands on columns");
    for p in &w.pillars {
        let (_, _, q) = w.roads.nearest_where(p.at, |s| s.is_street() && s.lanes() == 3).unwrap();
        assert!(q.distance(p.at) > s.half_width(), "a column at ({:.1}, {:.1}) stands in the street", p.at.x, p.at.y);
    }
}

/// K12: a path drawn at 2 m over the highway (which meets nothing, so it
/// was built at its height, the deck through the lorries) is refused.
#[test]
fn a_raised_path_too_low_over_the_highway_is_refused() {
    let mut w = flat();
    w.add_road(&[v(-400.0, 0.0), v(400.0, 0.0)], 3, 0);
    let why = w.road_refusal(&[v(0.0, -60.0), v(0.0, 60.0)], &[2.0, 2.0], RoadKind::Path, 0, 0).expect("refused");
    assert!(why.contains("passes only"), "{why}");
    assert_eq!(w.road_refusal(&[v(0.0, -60.0), v(0.0, 60.0)], &[6.0, 6.0], RoadKind::Path, 0, 0), None);
    // At grade it goes under (the highway is lifted over it), as before.
    assert_eq!(w.road_refusal(&[v(0.0, -60.0), v(0.0, 60.0)], &[], RoadKind::Path, 0, 0), None);
}

/// K47, K48: a dead end has a pavement round its end, not a crossing over
/// it; a junction's crosswalk over a short dead-end stub goes (the walk is
/// round the end).
#[test]
fn a_dead_end_is_walked_round_not_crossed() {
    let mut w = flat();
    w.add_road(&[v(-200.0, 0.0), v(200.0, 0.0)], 1, 1);
    w.add_road(&[v(0.0, 0.0), v(0.0, 120.0)], 1, 1);
    w.add_road(&[v(100.0, 0.0), v(100.0, -20.0)], 2, 2);
    let end = w.roads.node_near(v(0.0, 120.0), 1.0).unwrap();
    let crossing_at = |w: &World, p: Vec2| w.walk.crossings.iter().find(|c| !c.closed && c.mid.distance(p) < 3.0).map(|c| c.kind);
    assert_eq!(crossing_at(&w, w.roads.node(end).pos), None, "no crossing over the dead end");
    // Walk from one side of the dead end to the other: round the end, not
    // over a crossing.
    let (a, b) = (v(-6.0, 110.0), v(6.0, 110.0));
    let (pts, nodes) = w.walk.route(a, b).expect("a way round");
    assert!(pts.iter().any(|p| p.y > 120.0), "round the end: {pts:?}");
    assert!(nodes.windows(2).all(|n| w.walk.crossing_between(w.walk.nodes[n[0] as usize].pos, w.walk.nodes[n[1] as usize].pos).is_none()), "no crossing on the way round");
    // The 20 m stub's mouth has no crosswalk; the long arm's mouth keeps it.
    assert!(!w.walk.crossings.iter().any(|c| !c.closed && (c.mid.x - 100.0).abs() < 1.0 && c.mid.y < -1.0), "the stub's mouth is walked round");
    assert!(w.walk.crossings.iter().any(|c| !c.closed && c.kind == CrossingKind::Junction && c.mid.x.abs() < 1.0 && c.mid.y > 0.0), "the long arm keeps its crosswalk");
}

/// K15: a footpath meeting a street a few metres from a junction's
/// crosswalk ends on the pavement there: no zebra of its own. One meeting
/// it mid-block keeps its zebra.
#[test]
fn a_path_by_a_junction_crosswalk_gets_no_zebra() {
    let mut w = flat();
    w.add_road(&[v(-160.0, 0.0), v(160.0, 0.0)], 1, 1);
    w.add_road(&[v(0.0, -150.0), v(0.0, 150.0)], 1, 1);
    w.add_path(&[v(16.0, 6.0), v(16.0, 80.0)]);
    w.add_path(&[v(110.0, 6.0), v(110.0, 80.0)]);
    let open_zebra = |w: &World, x: f32| w.walk.crossings.iter().any(|c| c.kind == CrossingKind::Zebra && !c.closed && (c.mid.x - x).abs() < 3.0 && c.mid.y.abs() < 2.0);
    assert!(!open_zebra(&w, 16.0), "no zebra a few metres from the junction's crosswalk");
    assert!(open_zebra(&w, 110.0), "mid-block it keeps its zebra");
    assert!(w.closed_crosswalks().iter().any(|c| (c.x - 16.0).abs() < 3.0), "nothing painted there");
}

/// K14: a footbridge over the zebra where a raised path comes down to the
/// street joins the path: the path's deck runs on into the bridge, its
/// ramp down to the kerb is not walked, and only the far end has stairs.
#[test]
fn a_footbridge_where_a_raised_path_comes_down_joins_the_path() {
    let mut w = flat();
    w.add_road(&[v(-200.0, 0.0), v(200.0, 0.0)], 1, 1);
    w.add_road(&[v(-200.0, -160.0), v(200.0, -160.0)], 1, 1);
    w.project = None;
    w.add_gesture(path(&[v(0.0, 0.0), v(0.0, -24.0), v(0.0, -120.0), v(0.0, -160.0)], &[0.0, 6.0, 6.0, 0.0]));
    w.try_implement().expect("the raised path is built");
    let cr = w.walk.crossings.iter().position(|c| c.kind == CrossingKind::Zebra && c.mid.distance(v(0.0, 0.0)) < 3.0).expect("the path's zebra") as u32;
    w.economy.treasury = 1.0e6;
    w.set_crossing_upgrade(cr, crate::junctions::CrossingUpgrade::Footbridge).unwrap();
    let f = w.walk.footbridges.iter().find(|f| f.crossing == cr).expect("the footbridge");
    let (i, _, _) = f.path_join.expect("joined to the path");
    assert!(f.joined[i] && f.foot[i] == f.top[i], "no stairs on the path's side");
    assert!(!f.joined[1 - i] && f.foot[1 - i] != f.top[1 - i], "stairs down on the far side");
    // From up on the path over to the far pavement: by the bridge, never
    // down the old ramp.
    let up = w.walk.nearest(v(0.0, -24.0), 1.0).expect("the path's node up on the deck");
    let c = &w.walk.crossings[cr as usize];
    let far = if i == 0 { c.b } else { c.a };
    let nodes = w.walk.route_nodes(up, far).expect("a way over");
    assert!(nodes.iter().any(|&n| w.walk.nodes[n as usize].level > 0.0), "over the bridge");
    assert!(!nodes.iter().any(|&n| { let p = w.walk.nodes[n as usize].pos; p.x.abs() < 0.5 && p.y > -22.0 && p.y < -2.0 && w.walk.nodes[n as usize].level == 0.0 }), "not down the ramp");
}

/// K8: the land inside a small circus fronts it along a curve of short
/// edges (7 m pieces, shorter still on the inside); no single edge was long
/// enough, so it was all refused as having no road. The run of edges along
/// the curve counts.
#[test]
fn land_inside_a_circus_fronts_the_circus() {
    let mut w = flat();
    let c = v(0.0, 0.0);
    let r = 34.0;
    let mut pts: Vec<Vec2> = (0..8).map(|k| c + Vec2::from_angle(k as f32 * std::f32::consts::TAU / 8.0) * r).collect();
    pts.push(pts[0]);
    w.project = None;
    w.add_gesture(Gesture::Road { points: pts, lanes_fwd: 1, lanes_bwd: 1, curved: true, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
    w.try_implement().expect("the circus is built");
    w.add_road(&[v(r, 0.0), v(200.0, 0.0)], 1, 1);
    let inside: Vec<&crate::zoning::Parcel> = w.parcels.parcels.iter().filter(|p| p.center.distance(c) < r - 8.0).collect();
    assert!(!inside.is_empty(), "land inside the circus");
    let area: f32 = inside.iter().map(|p| p.area).sum();
    let fronting: f32 = inside.iter().filter(|p| p.street_frontage).map(|p| p.area).sum();
    assert!(fronting >= area * 0.8, "inside the circus {fronting:.0} of {area:.0} m² fronts it");
}
