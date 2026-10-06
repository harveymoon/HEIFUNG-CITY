//! Roads drawn above and below the ground: junctions where ways meet at
//! grade, flyovers where they do not, and the heights and decks that
//! follow.

use super::*;
use crate::roads::{Crossing, GRADE_SEPARATION};
use crate::traffic::LaneKind;

fn flat_world() -> World {
    World::new(WorldConfig { landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::default() })
}

fn road(points: &[Vec2], lanes: (u8, u8), kind: RoadKind, elevation: &[f32]) -> Gesture {
    Gesture::Road { points: points.to_vec(), lanes_fwd: lanes.0, lanes_bwd: lanes.1, curved: false, kind, elevation: elevation.to_vec() }
}

fn build(w: &mut World, g: Gesture) -> GestureId {
    let had = w.project.take();
    let id = w.add_gesture(g);
    w.implement();
    w.project = had;
    id
}

/// Connector lanes at a node joining a road lane of segment `from` to a road
/// lane of segment `to`.
fn connects(w: &World, node: NodeId, from: SegmentId, to: SegmentId) -> bool {
    let lanes = &w.traffic.net.lanes;
    let seg_of = |l: u32| match lanes[l as usize].kind {
        LaneKind::Road { segment, .. } => Some(segment),
        _ => None,
    };
    lanes.iter().enumerate().any(|(i, l)| {
        matches!(l.kind, LaneKind::Connector { node: n, .. } if n == node)
            && l.prev.iter().any(|&p| seg_of(p) == Some(from))
            && l.next.iter().any(|&q| seg_of(q) == Some(to))
            && i < lanes.len()
    })
}

fn segments_at(w: &World, n: NodeId) -> Vec<SegmentId> {
    w.roads.node(n).segments.iter().copied().filter(|&s| w.roads.segment(s).alive).collect()
}

#[test]
fn crossing_at_grade_makes_a_junction_in_roads_and_lanes() {
    for (lanes, kind) in [((1, 1), RoadKind::Street), ((3, 3), RoadKind::Street), ((0, 0), RoadKind::Path)] {
        let mut w = flat_world();
        w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 2, 2);
        build(&mut w, road(&[Vec2::new(40.0, -200.0), Vec2::new(40.0, 200.0)], lanes, kind, &[]));
        let x = w.roads.node_near(Vec2::new(40.0, 0.0), 0.5).expect("a node at the crossing");
        let segs = segments_at(&w, x);
        assert_eq!(segs.len(), 4, "{kind:?} {lanes:?}");
        if kind == RoadKind::Street {
            // Cars can turn from the old road onto the new one and back.
            let old: Vec<SegmentId> = segs.iter().copied().filter(|&s| w.roads.segment(s).lanes_fwd == 2).collect();
            let new: Vec<SegmentId> = segs.iter().copied().filter(|&s| w.roads.segment(s).lanes_fwd != 2).collect();
            assert!(old.iter().any(|&a| new.iter().any(|&b| connects(&w, x, a, b))), "no connector old -> new");
            assert!(new.iter().any(|&a| old.iter().any(|&b| connects(&w, x, a, b))), "no connector new -> old");
        } else {
            // Walkers cross there, on the zebras round the junction (not
            // through its middle): one side of the street reaches the other.
            assert!(w.walk.route(Vec2::new(40.0, -60.0), Vec2::new(40.0, 60.0)).is_some(), "walkers cross the street at the path");
        }
    }
}

#[test]
fn crossing_at_an_existing_junction_or_vertex_joins_it() {
    let mut w = flat_world();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0)], 1, 1);
    // Through the existing four-way, diagonally.
    w.add_road(&[Vec2::new(-200.0, -200.0), Vec2::new(200.0, 200.0)], 1, 1);
    let o = w.roads.node_near(Vec2::ZERO, 0.5).unwrap();
    let segs = segments_at(&w, o);
    assert_eq!(segs.len(), 6);
    assert!(connects(&w, o, segs[0], segs[segs.len() - 1]) || connects(&w, o, segs[segs.len() - 1], segs[0]));
    // Over the bend of a polyline.
    w.add_road(&[Vec2::new(-250.0, 150.0), Vec2::new(-100.0, 100.0), Vec2::new(250.0, 150.0)], 1, 1);
    w.add_road(&[Vec2::new(-100.0, 20.0), Vec2::new(-100.0, 250.0)], 1, 1);
    let bend = w.roads.node_near(Vec2::new(-100.0, 100.0), 0.5).unwrap();
    assert_eq!(segments_at(&w, bend).len(), 4);
    assert_eq!(w.roads.nodes.iter().filter(|n| n.pos.distance(Vec2::new(-100.0, 100.0)) < 4.0).count(), 1);
}

#[test]
fn curved_roads_crossing_make_junctions() {
    let mut w = flat_world();
    build(&mut w, Gesture::Road { points: vec![Vec2::new(-200.0, -80.0), Vec2::new(0.0, 60.0), Vec2::new(200.0, -80.0)], lanes_fwd: 1, lanes_bwd: 1, curved: true, kind: RoadKind::Street, elevation: Vec::new() });
    build(&mut w, Gesture::Road { points: vec![Vec2::new(-120.0, 150.0), Vec2::new(0.0, -40.0), Vec2::new(120.0, 150.0)], lanes_fwd: 1, lanes_bwd: 1, curved: true, kind: RoadKind::Street, elevation: Vec::new() });
    let junctions: Vec<NodeId> = (0..w.roads.nodes.len() as u32).map(NodeId).filter(|&n| segments_at(&w, n).len() >= 3).collect();
    assert_eq!(junctions.len(), 2);
    for j in junctions {
        let segs = segments_at(&w, j);
        assert_eq!(segs.len(), 4);
        assert!(w.traffic.net.lanes.iter().any(|l| matches!(l.kind, LaneKind::Connector { node, .. } if node == j)));
    }
}

#[test]
fn a_raised_street_passes_over_a_street_on_a_deck_without_a_junction() {
    for mut w in [flat_world(), World::new(WorldConfig::default())] {
        w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        // Up from the ground well away, 8 m over the street, down again.
        let pts = [Vec2::new(60.0, -300.0), Vec2::new(60.0, -150.0), Vec2::new(60.0, 150.0), Vec2::new(60.0, 300.0)];
        build(&mut w, road(&pts, (1, 1), RoadKind::Street, &[0.0, 8.0, 8.0, 0.0]));
        let x: Vec<Crossing> = w.roads.grade_crossings();
        assert_eq!(x.len(), 1, "one flyover");
        let c = x[0];
        assert!(c.at.distance(Vec2::new(60.0, 0.0)) < 0.5);
        assert!(w.roads.segment(c.upper.0).viaduct && w.roads.segment(c.upper.0).bridge, "the raised street is a viaduct");
        assert!(!w.roads.segment(c.lower.0).bridge);
        // No node of the street there carries the viaduct.
        for n in 0..w.roads.nodes.len() as u32 {
            let segs = segments_at(&w, NodeId(n));
            let levels: Vec<bool> = segs.iter().map(|&s| w.roads.segment(s).viaduct).collect();
            assert!(levels.iter().all(|v| *v == levels[0]) || w.roads.node(NodeId(n)).pos.distance(Vec2::new(60.0, 0.0)) > 100.0, "a node joins the levels at {:?}", w.roads.node(NodeId(n)).pos);
        }
        // No lane connector joins them either.
        let (up, low) = (c.upper.0, c.lower.0);
        assert!(!w.traffic.net.lanes.iter().any(|l| matches!(l.kind, LaneKind::Connector { node, .. } if segments_at(&w, node).contains(&up) && segments_at(&w, node).contains(&low))));
        // Heights: the deck clears the street, cars on it stay up.
        let zu = w.road_height(up, c.upper.1);
        let zl = w.road_height(low, c.lower.1);
        assert!(zu - zl >= GRADE_SEPARATION, "{zu} over {zl}");
        assert!(w.ground.decks_at(c.at).iter().any(|z| (z - zu).abs() < 0.2), "a deck at the viaduct's height");
        assert!(w.relief);
        // Something on the street under it keeps to the street.
        assert!((w.surface_height_near(c.at, zl) - zl).abs() < 0.5);
        assert!((w.surface_height_near(c.at, zu) - zu).abs() < 0.2);
        // The ends came down to the ground.
        let end = w.roads.node_near(Vec2::new(60.0, 300.0), 0.5).unwrap();
        assert!((w.road_z[end.0 as usize] - w.ground_height(Vec2::new(60.0, 300.0))).abs() < 1.0, "end at {} on ground {}", w.road_z[end.0 as usize], w.ground_height(Vec2::new(60.0, 300.0)));
    }
}

#[test]
fn streets_still_pass_under_the_highway_and_over_it_when_raised() {
    let mut w = World::new(WorldConfig::default());
    let hy = crate::landscape::HIGHWAY_Y;
    w.add_road(&[Vec2::new(-200.0, hy - 200.0), Vec2::new(-200.0, hy + 200.0)], 1, 1);
    let under: Vec<Crossing> = w.roads.grade_crossings().into_iter().filter(|c| (c.at.x + 200.0).abs() < 1.0).collect();
    assert_eq!(under.len(), 2, "both carriageways");
    for c in &under {
        assert!(w.roads.segment(c.upper.0).is_highway());
        let (zu, zl) = (w.road_height(c.upper.0, c.upper.1), w.road_height(c.lower.0, c.lower.1));
        assert!(zu - zl >= crate::landscape::UNDERPASS_CLEARANCE - 0.05, "the highway is lifted clear: {zu} over {zl}");
        assert!(w.ground.underpasses.iter().any(|u| u.at.distance(c.at) < 1.0), "an underpass with its deck and abutments");
    }
    // Raised, a street crosses over the highway instead: high enough to
    // clear it on its embankment.
    let pts = [Vec2::new(200.0, hy - 400.0), Vec2::new(200.0, hy - 60.0), Vec2::new(200.0, hy + 60.0), Vec2::new(200.0, hy + 400.0)];
    let (sid, t, _) = w.roads.nearest_where(Vec2::new(200.0, hy), |s| s.is_highway()).unwrap();
    let hz = w.road_height(sid, t);
    let up = (hz + 6.0 - w.landscape.height(pts[1]).min(w.landscape.height(pts[2])).max(crate::landscape::BANK_HEIGHT)).max(6.0).ceil();
    build(&mut w, road(&pts, (1, 1), RoadKind::Street, &[0.0, up, up, 0.0]));
    let over: Vec<Crossing> = w.roads.grade_crossings().into_iter().filter(|c| (c.at.x - 200.0).abs() < 1.0).collect();
    assert_eq!(over.len(), 2);
    for c in &over {
        assert!(!w.roads.segment(c.upper.0).is_highway(), "the street goes over");
        let (zu, zl) = (w.road_height(c.upper.0, c.upper.1), w.road_height(c.lower.0, c.lower.1));
        assert!(zu - zl >= crate::landscape::UNDERPASS_CLEARANCE - 0.05, "{zu} over {zl}: {:?}", w.road_grade_warnings(&pts, &[0.0, up, up, 0.0], RoadKind::Street, 1, 1));
    }
    // Traffic on the highway does not ride up onto the overpass: its lanes
    // are on the highway segment, so its height is the highway's.
    let hw_seg = over[0].lower.0;
    assert!(w.roads.segment(hw_seg).is_highway());
}

#[test]
fn elevation_survives_save_load_and_undo() {
    let mut w = flat_world();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    let g = road(&[Vec2::new(60.0, -300.0), Vec2::new(60.0, -150.0), Vec2::new(60.0, 150.0), Vec2::new(60.0, 300.0)], (1, 1), RoadKind::Street, &[0.0, 8.0, 8.0, 0.0]);
    let id = w.add_gesture(g.clone());
    // Pending: undo takes it away, redo brings it back with its levels.
    assert!(w.undo());
    assert!(w.gesture(id).is_none());
    assert!(w.redo());
    assert_eq!(w.gesture(id), Some(g.clone()));
    w.implement();
    let crossings = w.roads.grade_crossings().len();
    assert_eq!(crossings, 1);
    let text = w.to_ron().unwrap();
    let back = World::from_ron(&text).unwrap();
    assert_eq!(back.master.gestures.get(&id), Some(&g));
    assert_eq!(back.roads.grade_crossings().len(), 1);
    assert_eq!(back.road_z, w.road_z, "heights rebuilt on load");
    // A road from before elevation loads at grade.
    let old = "Road(points: [(0.0, 0.0), (10.0, 0.0)], lanes_fwd: 1, lanes_bwd: 1)";
    let g: Gesture = ron::from_str(old).unwrap();
    assert_eq!(g.road_levels(), vec![0.0, 0.0]);
    assert!(!ron::to_string(&g).unwrap().contains("elevation"), "roads at grade save as they did");
    // Moving, inserting and removing control points keeps the levels.
    let mut e = road(&[Vec2::ZERO, Vec2::new(100.0, 0.0), Vec2::new(200.0, 0.0)], (1, 1), RoadKind::Street, &[0.0, 6.0, 0.0]);
    e.insert_point(1, Vec2::new(50.0, 0.0));
    assert_eq!(e.road_levels(), vec![0.0, 3.0, 6.0, 0.0]);
    e.remove_point(2);
    assert_eq!(e.road_levels(), vec![0.0, 3.0, 0.0]);
}

#[test]
fn steep_ramps_and_half_height_crossings_are_warned_about() {
    let mut w = flat_world();
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    // 8 m up in 40 m: 20%.
    let warn = w.road_grade_warnings(&[Vec2::new(0.0, -300.0), Vec2::new(0.0, -260.0)], &[0.0, 8.0], RoadKind::Street, 1, 1);
    assert!(warn.iter().any(|m| m.contains("too steep")), "{warn:?}");
    // 8 m up over 150 m is fine.
    let ok = w.road_grade_warnings(&[Vec2::new(0.0, -300.0), Vec2::new(0.0, -150.0)], &[0.0, 8.0], RoadKind::Street, 1, 1);
    assert!(ok.is_empty(), "{ok:?}");
    // 3 m over a street: neither a junction nor a flyover.
    let half = w.road_grade_warnings(&[Vec2::new(30.0, -200.0), Vec2::new(30.0, 200.0)], &[3.0, 3.0], RoadKind::Street, 1, 1);
    assert!(half.iter().any(|m| m.contains("at grade")), "{half:?}");
    // Built anyway, a steep ramp is eased: its foot is lifted onto an
    // embankment so no piece is steeper than the grade.
    build(&mut w, road(&[Vec2::new(-200.0, -300.0), Vec2::new(-200.0, -260.0), Vec2::new(-200.0, -100.0)], (1, 1), RoadKind::Street, &[0.0, 8.0, 8.0]));
    for (sid, s) in w.roads.alive_segments().filter(|(_, s)| s.is_street()) {
        let (a, b) = w.roads.segment_points(sid);
        let fixed = w.roads.node(s.a).level != 0.0 && w.roads.node(s.b).level != 0.0;
        let dz = (w.road_z[s.a.0 as usize] - w.road_z[s.b.0 as usize]).abs();
        assert!(fixed || dz <= a.distance(b) * super::water::STREET_GRADE + 0.05, "{a} {b}: {dz}");
    }
}

#[test]
fn a_lowered_street_runs_in_a_cutting_under_a_street() {
    let mut w = World::new(WorldConfig::default());
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    let pts = [Vec2::new(60.0, -300.0), Vec2::new(60.0, -150.0), Vec2::new(60.0, 150.0), Vec2::new(60.0, 300.0)];
    // Just shallow enough to stay open (a cutting, not a tunnel).
    build(&mut w, road(&pts, (1, 1), RoadKind::Street, &[0.0, -4.5, -4.5, 0.0]));
    let x = w.roads.grade_crossings();
    assert_eq!(x.len(), 1);
    let c = x[0];
    assert!(w.roads.segment(c.lower.0).kind == RoadKind::Street && w.roads.node(w.roads.segment(c.lower.0).a).level < 0.0, "the cutting goes under");
    let u = w.ground.underpasses.iter().find(|u| u.at.distance(c.at) < 1.0).expect("the street over it gets a deck and abutments");
    assert!(u.over_z - u.under_z >= crate::landscape::UNDERPASS_CLEARANCE - 0.05);
    // The ground in the cut is the lowered street's.
    assert!((w.ground_height(c.at + u.under_dir * 10.0) - u.under_z_at(10.0)).abs() < 0.5);
}

// ── tunnels ─────────────────────────────────────────────────────────────────

const T: f32 = crate::roads::TUNNEL_DEPTH;

/// A street east-west at y = 0, and a tunnel that leaves it by a portal
/// ramp at x = -300, runs north under everything at -8 m and comes up by
/// a ramp onto a street at y = 400.
fn tunnel_town() -> (World, GestureId) {
    let mut w = flat_world();
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(-400.0, 400.0), Vec2::new(400.0, 400.0)], 1, 1);
    let pts = [Vec2::new(-300.0, 0.0), Vec2::new(-300.0, -130.0), Vec2::new(0.0, -130.0), Vec2::new(0.0, 530.0), Vec2::new(150.0, 530.0), Vec2::new(150.0, 400.0)];
    let id = build(&mut w, road(&pts, (1, 1), RoadKind::Street, &[0.0, T, T, T, T, 0.0]));
    (w, id)
}

#[test]
fn a_tunnel_passes_under_streets_and_joins_by_its_portals() {
    let (w, _) = tunnel_town();
    // Under both streets without a junction.
    let x: Vec<Crossing> = w.roads.grade_crossings().into_iter().filter(|c| c.at.x.abs() < 1.0).collect();
    assert_eq!(x.len(), 2, "under both streets");
    for c in &x {
        assert!(w.roads.segment(c.lower.0).tunnel, "the tunnel is the lower way");
        assert!(!w.ground.underpasses.iter().any(|u| u.at.distance(c.at) < 1.0), "bored, not cut");
        assert!(w.road_height(c.upper.0, c.upper.1) - w.road_height(c.lower.0, c.lower.1) >= 5.0);
    }
    // The portals join the ground streets: a way from one street to the
    // other through the tunnel.
    let a = w.roads.node_near(Vec2::new(-300.0, 0.0), 0.5).expect("a junction at the south portal");
    let b = w.roads.node_near(Vec2::new(150.0, 400.0), 0.5).expect("a junction at the north portal");
    assert_eq!(segments_at(&w, a).len(), 3);
    assert_eq!(segments_at(&w, b).len(), 3);
    let path = w.roads.path(a, b).expect("connected through the tunnel");
    assert!(path.windows(2).any(|p| w.roads.neighbors(p[0]).any(|(n, s)| n == p[1] && w.roads.segment(s).tunnel)));
    // Nobody walks in it; parcels above are whole.
    let mid = Vec2::new(0.0, 200.0);
    assert!(w.walk.nodes.iter().all(|n| n.pos.distance(mid) > 20.0), "no pavement in the tunnel");
    assert!(w.roads.alive_segments().filter(|(_, s)| s.tunnel).count() >= 3);
}

#[test]
fn tunnels_meet_tunnels_at_underground_junctions() {
    let (mut w, _) = tunnel_town();
    // A second tunnel crossing the first under the middle of the block.
    build(&mut w, road(&[Vec2::new(-200.0, 200.0), Vec2::new(200.0, 200.0)], (1, 1), RoadKind::Street, &[T, T]));
    let j = w.roads.node_near(Vec2::new(0.0, 200.0), 0.5).expect("a junction under ground");
    let segs = segments_at(&w, j);
    assert_eq!(segs.len(), 4);
    assert!(segs.iter().all(|&s| w.roads.segment(s).tunnel));
    assert!((w.road_z[j.0 as usize] - T).abs() < 0.5, "at tunnel depth");
    assert!(connects(&w, j, segs[0], segs[1]) || connects(&w, j, segs[0], segs[2]) || connects(&w, j, segs[0], segs[3]));
}

#[test]
fn cars_drive_through_a_tunnel() {
    let (mut w, _) = tunnel_town();
    let (s0, _, _) = w.roads.nearest_street(Vec2::new(-350.0, 0.0)).unwrap();
    let (s1, _, _) = w.roads.nearest_street(Vec2::new(300.0, 400.0)).unwrap();
    let (route, a, b) = w.traffic.net.find_route(s0, Vec2::new(-350.0, 0.0), s1, Vec2::new(300.0, 400.0)).expect("a route");
    let through = route.iter().any(|&l| matches!(w.traffic.net.lanes[l as usize].kind, LaneKind::Road { segment, .. } if w.roads.segment(segment).tunnel));
    assert!(through, "the short way is through the tunnel");
    let id = w.traffic.spawn_car(CitizenId::default(), None, route, a, b, 0);
    let mut deepest = 0.0f32;
    for _ in 0..4000 {
        w.traffic.step(1.0, 2);
        let Some(car) = w.traffic.cars.get(id) else { break };
        deepest = deepest.min(w.car_height(car));
    }
    assert!(deepest < -6.0, "the car went down into the tunnel: deepest {deepest}");
}

#[test]
fn a_tunnel_under_a_building_leaves_it_standing() {
    let mut w = flat_world();
    w.add_road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
    w.add_zone(vec![Vec2::new(-100.0, 5.0), Vec2::new(100.0, 5.0), Vec2::new(100.0, 80.0), Vec2::new(-100.0, 80.0)], LandUse::Residential);
    let lot = w.lots.iter().filter(|(_, l)| l.usable()).min_by(|a, b| a.1.center.length().total_cmp(&b.1.center.length())).map(|(id, _)| id).unwrap();
    w.build_on(lot);
    let building = w.lots[lot].building.unwrap();
    let at = w.lots[lot].center;
    let lots_before = w.lots.len();
    // A tunnel straight under the building (no portal here: it is deep all along).
    build(&mut w, road(&[Vec2::new(at.x, -300.0), Vec2::new(at.x, 300.0)], (1, 1), RoadKind::Street, &[T, T]));
    assert!(w.buildings.contains_key(building), "the building stands");
    assert_eq!(w.lots.len(), lots_before, "no parcel was cut");
    assert!(w.roads.alive_segments().any(|(_, s)| s.tunnel));
}

// ── ramps ───────────────────────────────────────────────────────────────────

#[test]
fn ramps_join_the_highway_nearly_tangent() {
    let mut w = flat_world();
    // An eastbound carriageway and a ramp drawn square onto it from the
    // south (a merge), and one leaving it square to the north (a diverge).
    w.add_road(&[Vec2::new(-500.0, 0.0), Vec2::new(500.0, 0.0)], 3, 0);
    w.add_road(&[Vec2::new(-100.0, -200.0), Vec2::new(-100.0, -5.0)], 1, 0);
    w.add_road(&[Vec2::new(200.0, 5.0), Vec2::new(200.0, 200.0)], 1, 0);
    for x in [-100.0f32, 200.0] {
        let j = w.roads.node_near(Vec2::new(x, 0.0), 0.5).expect("the ramp joins the highway");
        let segs = segments_at(&w, j);
        assert_eq!(segs.len(), 3);
        let ramp = segs.iter().copied().find(|&s| !w.roads.segment(s).is_highway()).unwrap();
        let (a, b) = w.roads.segment_points(ramp);
        let d = (b - a).normalize();
        assert!(d.x.abs() > 0.95, "the ramp's last piece runs alongside: {d:?}");
    }
    // The merge comes in from behind the join, the diverge leaves ahead.
    assert!(w.roads.nodes.iter().any(|n| (n.pos.x - (-100.0 - crate::roads::RAMP_LEAD)).abs() < 1.0 && n.pos.y < 0.0));
    assert!(w.roads.nodes.iter().any(|n| (n.pos.x - (200.0 + crate::roads::RAMP_LEAD)).abs() < 1.0 && n.pos.y > 0.0));
    // Still connected: a car can go ramp -> highway -> ramp.
    let (s0, _, _) = w.roads.nearest_street(Vec2::new(-100.0, -190.0)).unwrap();
    let (s1, _, _) = w.roads.nearest_street(Vec2::new(200.0, 190.0)).unwrap();
    assert!(w.traffic.net.find_route(s0, Vec2::new(-100.0, -190.0), s1, Vec2::new(200.0, 190.0)).is_some());
}

#[test]
fn a_ramp_climbing_to_a_raised_highway_stands_on_piers() {
    let mut w = flat_world();
    // A highway on a deck 8 m up, and a ramp from the ground onto it.
    build(&mut w, road(&[Vec2::new(-500.0, 0.0), Vec2::new(500.0, 0.0)], (3, 0), RoadKind::Street, &[8.0, 8.0]));
    w.add_road(&[Vec2::new(-100.0, -300.0), Vec2::new(-100.0, -5.0)], 1, 0);
    let raised: Vec<_> = w.roads.alive_segments().filter(|(_, s)| !s.is_highway() && s.viaduct).collect();
    assert!(!raised.is_empty(), "the high part of the ramp is a viaduct");
    assert!(w.roads.alive_segments().filter(|(_, s)| !s.is_highway()).any(|(_, s)| !s.viaduct), "its foot is on the ground");
}
