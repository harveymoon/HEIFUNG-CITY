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
fn walkers_and_bikes_never_route_through_a_tunnel() {
    let (mut w, _) = tunnel_town();
    // A street round the east end: the long way on the surface.
    w.add_road(&[Vec2::new(400.0, 0.0), Vec2::new(400.0, 400.0)], 1, 1);
    let a = w.roads.node_near(Vec2::new(-300.0, 0.0), 0.5).unwrap();
    let b = w.roads.node_near(Vec2::new(150.0, 400.0), 0.5).unwrap();
    let cars = w.roads.path(a, b).unwrap();
    let in_bore = |p: &[NodeId]| p.windows(2).any(|q| w.roads.neighbors(q[0]).any(|(n, s)| n == q[1] && w.roads.segment(s).fronts_nothing()));
    assert!(in_bore(&cars), "cars take the tunnel");
    let feet = w.roads.foot_path(a, b).expect("the surface way round");
    assert!(!in_bore(&feet), "walkers and bikes keep to the surface");
    // The rider's line stays on the surface streets.
    for mode in [TravelMode::Bike, TravelMode::Walk] {
        for p in w.route_path(&feet, mode) {
            let on_street = p.y.abs() < 15.0 || (p.y - 400.0).abs() < 15.0 || (p.x - 400.0).abs() < 15.0;
            assert!(on_street, "{mode:?} point {p} is off the surface streets (over the bore)");
        }
    }
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

#[test]
fn the_highway_follows_the_land_and_its_carriageways_stay_side_by_side() {
    let mut w = World::new(WorldConfig::default());
    // The demo's link: a street joined to the westbound carriageway by ramps.
    w.add_road(&[Vec2::new(0.0, 100.0), Vec2::new(0.0, crate::landscape::HIGHWAY_Y - 200.0)], 1, 1);
    w.link_to_highway(Vec2::new(0.0, crate::landscape::HIGHWAY_Y - 200.0));
    let hy = crate::landscape::HIGHWAY_Y;
    let half = crate::landscape::HIGHWAY_HALF_GAP;
    let at = |y: f32, x: f32| {
        let p = Vec2::new(x, y);
        let (sid, t, q) = w.roads.nearest_where(p, |s| s.is_highway()).unwrap();
        assert!(q.distance(p) < 1.0, "a carriageway at {p}");
        (w.road_height(sid, t), w.roads.segment(sid).bridge)
    };
    let mut worst: (f32, f32) = (0.0, 0.0);
    let edge = crate::landscape::LAND_HALF - 60.0;
    let mut x = -edge;
    while x < edge {
        let ((ze, be), (zw, bw)) = (at(hy + half, x), at(hy - half, x));
        let near_bridge = (-60..=60).step_by(20).any(|dx| { let (_, b1) = at(hy + half, x + dx as f32); let (_, b2) = at(hy - half, x + dx as f32); b1 || b2 });
        if !(be || bw || near_bridge) {
            let land = w.landscape.height(Vec2::new(x, hy)).max(crate::landscape::BANK_HEIGHT);
            let pair = (ze - zw).abs();
            let off = (ze - land).abs().max((zw - land).abs());
            // Flyovers over the ramps' street aside (within 120 m of x = 0).
            // Within the basin: the outer hills rise faster than any road.
            if x.abs() > 120.0 && x.abs() < crate::landscape::BASIN_RADIUS && off > worst.1 {
                worst = (x, off);
            }
            assert!(pair < 2.5 || x.abs() <= 120.0, "carriageways {ze:.1} / {zw:.1} at x = {x}");
        }
        x += 20.0;
    }
    assert!(worst.1 < 8.0, "the highway strays {:.1} m from the land at x = {}", worst.1, worst.0);
    let ((ze, _), (zw, _)) = (at(hy + half, -120.0), at(hy - half, -120.0));
    assert!((ze - zw).abs() < 3.0, "at x = -120: {ze} / {zw}");
}

/// A road on its way down to a tunnel that meets the railway part way down
/// would have made a level crossing at the road's height, dragging the
/// track down into the cutting. It is refused, saying what would work;
/// at grade, or deep enough to pass under, it is fine.
#[test]
fn a_portal_ramp_meeting_the_railway_off_grade_is_refused() {
    let mut w = flat_world();
    build(&mut w, road(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], (0, 0), RoadKind::Rail, &[]));
    let ramp = [Vec2::new(0.0, -150.0), Vec2::new(0.0, 150.0)];
    let why = w.road_refusal(&ramp, &[0.0, T], RoadKind::Street, 1, 1).expect("a ramp halfway down at the track is refused");
    assert!(why.contains("level crossing") && why.contains("under"), "{why}");
    // The other way round: track laid over a portal ramp that is there.
    build(&mut w, road(&[Vec2::new(200.0, -150.0), Vec2::new(200.0, 150.0)], (1, 1), RoadKind::Street, &[0.0, T]));
    assert!(w.road_refusal(&[Vec2::new(150.0, 40.0), Vec2::new(250.0, 40.0)], &[], RoadKind::Rail, 0, 0).is_some(), "track over the ramp at its middle");
    // Its warnings say so too, while drawing.
    assert!(w.road_grade_warnings(&ramp, &[0.0, T], RoadKind::Street, 1, 1).iter().any(|m| m.contains("level crossing")));
    // At grade: a level crossing. Deep: under the track. Both fine.
    assert!(w.road_refusal(&ramp, &[], RoadKind::Street, 1, 1).is_none());
    assert!(w.road_refusal(&ramp, &[T, T], RoadKind::Street, 1, 1).is_none());
    // A street that starts its ramp past the track is fine too.
    assert!(w.road_refusal(&[Vec2::new(-100.0, -150.0), Vec2::new(-100.0, 20.0), Vec2::new(-100.0, 150.0)], &[0.0, 0.0, T], RoadKind::Street, 1, 1).is_none());
}

/// Tunnels are ways for traffic only, like the highway's ramps: no kerb
/// parking (legal or not: a car parked at a tunnel's kerb stood on the
/// ground above it), no lots fronting them, no mid-block crossings.
#[test]
fn tunnels_front_nothing_and_take_no_parking() {
    let (w, _) = tunnel_town();
    let tunnels: Vec<SegmentId> = w.roads.alive_segments().filter(|(_, s)| s.tunnel).map(|(id, _)| id).collect();
    assert!(!tunnels.is_empty());
    for &sid in &tunnels {
        assert!(w.roads.segment(sid).fronts_nothing());
        assert!(!w.kerb_physical(sid) && !w.kerb_allowed(sid), "kerb parking in tunnel {sid:?}");
    }
    assert!(w.lots.values().all(|l| !tunnels.contains(&l.front_segment)), "a lot fronts a tunnel");
    assert!(crate::walk::midblock_crossings(&w.roads).iter().all(|(s, _)| !tunnels.contains(s)));
}

/// A long street drawn as one leg over the hills follows the land through
/// its inner height samples, within its grade, without a single new node
/// (so its parcels are cut exactly as before), and the built ground and
/// the lots along it are set on that profile.
#[test]
fn long_streets_follow_the_land_between_their_ends() {
    let mut w = World::new(WorldConfig::default());
    assert!(!w.landscape.is_flat());
    // Candidate legs across the middle of the map, on dry land all along;
    // the one whose land strays most from the straight line is built.
    let floor = crate::landscape::BANK_HEIGHT;
    let land = w.landscape.clone();
    let h = |p: Vec2| land.height(p).max(floor);
    let mut pick: Option<(f32, Vec2, Vec2)> = None;
    for cx in -4..=4 {
        for cy in -4..=4 {
            for k in 0..4 {
                let c = Vec2::new(cx as f32 * 120.0, cy as f32 * 120.0);
                let d = Vec2::from_angle(k as f32 * 0.785 + 0.1) * 210.0;
                let (a, b) = (c - d, c + d);
                if (0..=30).any(|i| land.is_water(a.lerp(b, i as f32 / 30.0)) || land.water_distance(a.lerp(b, i as f32 / 30.0)) < 30.0) {
                    continue;
                }
                let stray = (1..30).map(|i| i as f32 / 30.0).map(|t| (h(a.lerp(b, t)) - (h(a) + (h(b) - h(a)) * t)).abs()).fold(0.0, f32::max);
                if pick.is_none_or(|(s, _, _)| stray > s) {
                    pick = Some((stray, a, b));
                }
            }
        }
    }
    let (_, a, b) = pick.expect("a dry line");
    let before = w.roads.nodes.len();
    w.add_road(&[a, b], 1, 1);
    let segs: Vec<SegmentId> = w.roads.alive_segments().filter(|(id, _)| { let (p, q) = w.roads.segment_points(*id); p.distance(a) < 1.0 && q.distance(b) < 1.0 }).map(|(id, _)| id).collect();
    assert_eq!(segs.len(), 1, "one leg, one segment: no height vertices on a street");
    assert!(w.roads.nodes.len() <= before + 2);
    let sid = segs[0];
    let za = w.road_z[w.roads.segment(sid).a.0 as usize];
    let zb = w.road_z[w.roads.segment(sid).b.0 as usize];
    let stray = (1..30).map(|i| i as f32 / 30.0).map(|t| (h(a.lerp(b, t)) - (za + (zb - za) * t)).abs()).fold(0.0, f32::max);
    let best = Some((stray, sid));
    let (stray, sid) = best.expect("a dry line");
    assert!(stray > 1.0, "no hills to test on ({stray} m)");
    let inner = &w.road_inner[sid.0 as usize];
    assert!(!inner.is_empty(), "a street {stray:.1} m off the land got no inner heights");
    let (a, b) = w.roads.segment_points(sid);
    let len = a.distance(b);
    // Within the street's grade everywhere, nearer the land than the line.
    let (mut off_line, mut off_profile) = (0.0f32, 0.0f32);
    let mut prev = w.road_height(sid, 0.0);
    for i in 1..=60 {
        let t = i as f32 / 60.0;
        let z = w.road_height(sid, t);
        assert!((z - prev).abs() <= water::STREET_GRADE * len / 60.0 + 1e-3, "too steep at t = {t}");
        prev = z;
        let land = w.landscape.height(a.lerp(b, t)).max(crate::landscape::BANK_HEIGHT);
        let za = w.road_z[w.roads.segment(sid).a.0 as usize];
        let zb = w.road_z[w.roads.segment(sid).b.0 as usize];
        off_line += (land - (za + (zb - za) * t)).abs();
        off_profile += (land - z).abs();
        // The built ground is graded to the profile under the road.
        let g = w.ground_height(a.lerp(b, t));
        assert!((g - z).abs() < 0.6, "ground {g} under the road at {z} (t = {t})");
    }
    assert!(off_profile < off_line * 0.8, "profile {off_profile} vs line {off_line}");
    // Lots along it stand on its profile at their frontage.
    for (id, l) in w.lots.iter().filter(|(_, l)| l.front_segment == sid && !l.landlocked) {
        assert!((w.lot_pad(id) - w.road_height(sid, l.front_t)).abs() < 1e-3);
    }
}

/// Where a ramp's corridor overlaps the carriageway it joins, the built
/// ground stays under the carriageway's surface: the higher way used to
/// hold the shared cells, and a ramp a few centimetres above the lanes
/// raised the ground through their asphalt (a bare wedge in the gore).
#[test]
fn the_ground_stays_under_the_lanes_where_ramps_join() {
    let mut w = World::new(WorldConfig::default());
    w.add_road(&[Vec2::new(0.0, 100.0), Vec2::new(0.0, crate::landscape::HIGHWAY_Y - 200.0)], 1, 1);
    w.link_to_highway(Vec2::new(0.0, crate::landscape::HIGHWAY_Y - 200.0));
    let mut worst = (0.0f32, Vec2::ZERO);
    for (sid, s) in w.roads.alive_segments().filter(|(_, s)| s.is_highway() && !s.bridge) {
        let (a, b) = w.roads.segment_points(sid);
        if a.x.abs().max(b.x.abs()) > 400.0 {
            continue;
        }
        let d = (b - a).normalize_or_zero();
        let side = Vec2::new(-d.y, d.x);
        let half = s.lanes() as f32 * crate::roads::LANE_WIDTH * 0.5 - 0.3;
        for i in 0..=40 {
            let t = i as f32 / 40.0;
            for k in [-1.0f32, -0.5, 0.0, 0.5, 1.0] {
                let p = a.lerp(b, t) + side * half * k;
                if w.ground.underpasses.iter().any(|u| u.at.distance(p) < 40.0) {
                    continue;
                }
                let over = w.ground_height(p) - w.road_height(sid, t);
                if over > worst.0 {
                    worst = (over, p);
                }
            }
        }
    }
    assert!(worst.0 < 0.05, "the ground stands {:.2} m over the lanes at {:?}", worst.0, worst.1);
}
