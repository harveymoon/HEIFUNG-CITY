use super::*;

/// The reference every grid-backed query must match: a full scan that
/// keeps the first best.
fn linear_nearest(net: &RoadNetwork, p: Vec2, keep: impl Fn(&Segment) -> bool) -> Option<(SegmentId, f32, Vec2)> {
    let mut best: Option<(f32, SegmentId, f32, Vec2)> = None;
    for (sid, seg) in net.alive_segments() {
        if !keep(seg) {
            continue;
        }
        let (p0, p1) = net.segment_points(sid);
        let (t, q) = project_on_segment(p, p0, p1);
        let d = p.distance(q);
        if best.is_none_or(|(bd, ..)| d < bd) {
            best = Some((d, sid, t, q));
        }
    }
    best.map(|(_, sid, t, q)| (sid, t, q))
}

fn city_grid(half: i32, spacing: f32) -> RoadNetwork {
    let mut net = RoadNetwork::default();
    let ext = half as f32 * spacing;
    for i in -half..=half {
        let c = i as f32 * spacing;
        net.add_polyline(&[Vec2::new(-ext, c), Vec2::new(ext, c)], 1, 1);
        net.add_polyline(&[Vec2::new(c, -ext), Vec2::new(c, ext)], 1, 1);
    }
    net.add_polyline_kind(&[Vec2::new(-ext, -ext - 30.0), Vec2::new(ext, -ext - 30.0)], 0, 0, RoadKind::Rail);
    net.add_polyline_kind(&[Vec2::new(-ext - 20.0, 0.0), Vec2::new(-ext - 20.0, ext)], 0, 0, RoadKind::Path);
    net
}

#[test]
fn grid_nearest_queries_match_the_linear_scan() {
    let net = city_grid(6, 90.0);
    assert!(net.alive_segments().count() > 150);
    let mut seed = 11u32;
    for k in 0..600 {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let mut p = Vec2::new((seed >> 8) as f32 / 16_777_216.0 * 1400.0 - 700.0, (seed >> 20) as f32 / 4096.0 * 1400.0 - 700.0);
        // Every third query sits on a junction or a lane centreline,
        // where several segments tie exactly.
        if k % 3 == 0 {
            p = Vec2::new((p.x / 90.0).round() * 90.0, if k % 2 == 0 { (p.y / 90.0).round() * 90.0 } else { p.y });
        }
        assert_eq!(net.nearest_street(p), linear_nearest(&net, p, Segment::is_street), "street at {p:?}");
        assert_eq!(net.nearest_on_network(p), linear_nearest(&net, p, |s| !s.is_rail()), "network at {p:?}");
        assert_eq!(net.nearest_of_kind(p, RoadKind::Rail), linear_nearest(&net, p, |s| s.is_rail()), "rail at {p:?}");
        assert_eq!(net.nearest_of_kind(p, RoadKind::Path), linear_nearest(&net, p, |s| !s.is_rail()), "path at {p:?}");
    }
    // Far outside the city the answer is still the right edge segment.
    let far = Vec2::new(5000.0, -3000.0);
    assert_eq!(net.nearest_street(far), linear_nearest(&net, far, Segment::is_street));
    // An empty network has no nearest.
    assert!(RoadNetwork::default().nearest_street(Vec2::ZERO).is_none());
    // The index follows edits: a new road becomes the nearest at once.
    let mut net = net;
    net.add_polyline(&[Vec2::new(2000.0, 2000.0), Vec2::new(2100.0, 2000.0)], 1, 1);
    let p = Vec2::new(2050.0, 2010.0);
    assert_eq!(net.nearest_street(p), linear_nearest(&net, p, Segment::is_street));
    assert!(net.nearest_street(p).unwrap().2.distance(Vec2::new(2050.0, 2000.0)) < 1e-3);
}

/// Rebuild cost of a large city: `cargo test -p nd_sim --release -- --ignored nearest_bench`.
#[test]
#[ignore]
fn nearest_bench() {
    let net = city_grid(40, 50.0);
    let n = net.alive_segments().count();
    let t0 = std::time::Instant::now();
    let mut acc = 0.0;
    for i in 0..20_000 {
        let p = Vec2::new((i % 300) as f32 * 13.7 - 2000.0, (i / 300) as f32 * 31.1 - 2000.0);
        acc += net.nearest_street(p).map_or(0.0, |(_, t, _)| t);
    }
    let grid = t0.elapsed();
    let t0 = std::time::Instant::now();
    for i in 0..20_000 {
        let p = Vec2::new((i % 300) as f32 * 13.7 - 2000.0, (i / 300) as f32 * 31.1 - 2000.0);
        acc += linear_nearest(&net, p, Segment::is_street).map_or(0.0, |(_, t, _)| t);
    }
    let linear = t0.elapsed();
    eprintln!("{n} segments, 20k queries: grid {grid:?}, linear {linear:?} ({acc})");
    assert!(grid < linear);
}

#[test]
fn a_street_crossing_the_track_near_a_rail_node_becomes_a_level_crossing() {
    let mut net = RoadNetwork::default();
    net.add_polyline_kind(&[Vec2::new(-100.0, 0.0), Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0)], 0, 0, RoadKind::Rail);
    // The street crosses the track 3 m from its middle node: within the
    // snap radius, so the crossing is that node rather than a second one
    // a step away.
    net.add_polyline(&[Vec2::new(3.0, -100.0), Vec2::new(3.0, 100.0)], 1, 1);
    let crossing = net.node_near(Vec2::ZERO, SNAP_RADIUS).expect("crossing node");
    let kinds: Vec<RoadKind> = net.neighbors(crossing).map(|(_, s)| net.segment(s).kind).collect();
    assert_eq!(kinds.len(), 4, "two rail and two street pieces meet at the crossing: {kinds:?}");
    assert_eq!(kinds.iter().filter(|k| **k == RoadKind::Rail).count(), 2);
    assert_eq!(net.nodes.iter().filter(|n| n.pos.distance(Vec2::ZERO) < SNAP_RADIUS).count(), 1, "no second node beside the crossing");
    // Both ways still run through: trains along the track, cars along the street.
    let (ra, rb) = (net.node_near(Vec2::new(-100.0, 0.0), 1.0).unwrap(), net.node_near(Vec2::new(100.0, 0.0), 1.0).unwrap());
    assert_eq!(net.rail_path(ra, rb).map(|p| p.len()), Some(3));
    let (sa, sb) = (net.node_near(Vec2::new(3.0, -100.0), 1.0).unwrap(), net.node_near(Vec2::new(3.0, 100.0), 1.0).unwrap());
    assert_eq!(net.path(sa, sb).map(|p| p.len()), Some(3));
    // The rail ends are still the only rail leaves.
    assert_eq!(net.rail_leaf_nodes().len(), 2);
}

/// [road-polish] A street whose vertex lies exactly on the track (a
/// height sample or a bend falling on it) still crosses it at a level
/// crossing: the crossing test between pieces missed a crossing at a
/// piece's end, so the street ran under the track at grade, which was
/// drawn floating over its lanes on its bed (four places on the trunk
/// line in the grid-and-plaza example city).
#[test]
fn a_street_vertex_on_the_track_makes_a_level_crossing() {
    for x in [5.0f32, 0.0] {
        let mut net = RoadNetwork::default();
        net.add_polyline_kind(&[Vec2::new(-100.0, 0.0), Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0)], 0, 0, RoadKind::Rail);
        net.add_polyline(&[Vec2::new(x, -100.0), Vec2::new(x, 0.0), Vec2::new(x + 3.0, 100.0)], 1, 1);
        let crossing = net.node_near(Vec2::new(x, 0.0), 1.0).expect("a node on the track");
        let kinds: Vec<RoadKind> = net.neighbors(crossing).map(|(_, s)| net.segment(s).kind).collect();
        assert_eq!(kinds.iter().filter(|k| **k == RoadKind::Rail).count(), 2, "x {x}: the track runs through the crossing: {kinds:?}");
        assert_eq!(kinds.iter().filter(|k| **k == RoadKind::Street).count(), 2, "x {x}: {kinds:?}");
    }
}

#[test]
fn a_self_crossing_polyline_splits_itself_at_the_crossing() {
    let mut net = RoadNetwork::default();
    // A figure of eight: the second half crosses the first.
    let pts = [Vec2::new(-100.0, -50.0), Vec2::new(100.0, 50.0), Vec2::new(100.0, -50.0), Vec2::new(-100.0, 50.0)];
    let made = net.add_polyline(&pts, 1, 1);
    assert!(made.len() >= 4, "the crossing splits a leg: {} pieces", made.len());
    let x = net.node_near(Vec2::ZERO, 1.0).expect("a node at the crossing");
    assert_eq!(net.neighbors(x).count(), 4);
    assert!(net.nodes.iter().all(|n| n.pos.is_finite()));
    let (a, b) = (net.node_near(pts[0], 1.0).unwrap(), net.node_near(pts[3], 1.0).unwrap());
    let path = net.path(a, b).expect("the ends connect");
    // Via the crossing is shorter than along the whole figure.
    assert_eq!(path.len(), 3);
    assert_eq!(net.leaf_nodes().len(), 2);
}

/// [r4-flags-look] (flag day 8 at (279,493)) A street drawn round a loop
/// whose end comes back beside an earlier vertex of its own joins itself
/// there in a junction: it used to stop 1.9 m off it, two nodes side by
/// side with overlapping lanes and no junction.
#[test]
fn a_looping_road_ending_by_its_own_vertex_joins_it() {
    let mut net = RoadNetwork::default();
    let pts = [Vec2::new(-200.0, 0.0), Vec2::new(0.0, 0.0), Vec2::new(100.0, 80.0), Vec2::new(250.0, 20.0), Vec2::new(1.5, 1.1)];
    net.add_polyline(&pts, 2, 2);
    let at = net.node_near(Vec2::ZERO, 1.0).expect("the loop's vertex");
    assert_eq!(net.neighbors(at).count(), 3, "the end joins the vertex: a T junction");
    assert!(net.node_near(Vec2::new(1.5, 1.1), 0.5).is_none_or(|n| n == at), "no second node beside it");
    // Harvey's: a car-only road was laid first from beside the vertex; the
    // avenue's end lands on its start, 1.9 m from the avenue's own vertex
    // (which, a street's, may not join the car-only road's node). The
    // vertex is welded onto the end: one junction of all four pieces.
    let mut net = RoadNetwork::default();
    let p = Vec2::new(1.55, 1.06);
    net.add_polyline_use(&[p, Vec2::new(90.0, -60.0)], &[], 1, 0, RoadKind::Street, Some(true));
    net.add_polyline_use(&[Vec2::new(-200.0, 0.0), Vec2::new(0.0, 0.0), Vec2::new(100.0, 80.0), Vec2::new(250.0, 20.0), p], &[], 2, 2, RoadKind::Street, Some(false));
    let j = net.node_near(p, 0.5).expect("the end's node");
    let live = |n: NodeId| net.node(n).segments.iter().filter(|&&s| net.segment(s).alive).count();
    assert_eq!(live(j), 4, "the avenue's vertex, its end and the car-only road meet at one junction");
    assert!(net.nodes.iter().enumerate().all(|(i, n)| NodeId(i as u32) == j || n.pos.distance(p) > 3.0 || n.segments.iter().all(|&s| !net.segment(s).alive)), "no second live node beside it");
    // A curve's last samples, close together, still never snap back.
    let mut net = RoadNetwork::default();
    let arc: Vec<Vec2> = (0..=12).map(|k| { let a = k as f32 * 0.12; Vec2::new(60.0 * a.cos(), 60.0 * a.sin()) }).collect();
    let made = net.add_polyline(&arc, 1, 1);
    assert_eq!(made.len(), 12, "every sample its own piece");
    assert_eq!(net.leaf_nodes().len(), 2);
}

#[test]
fn a_road_drawn_through_a_junction_joins_it() {
    let mut net = RoadNetwork::default();
    net.add_polyline(&[Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0)], 1, 1);
    net.add_polyline(&[Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0)], 1, 1);
    let origin = net.node_near(Vec2::ZERO, 1.0).unwrap();
    assert_eq!(net.node(origin).segments.len(), 4);
    net.add_polyline(&[Vec2::new(-100.0, -100.0), Vec2::new(100.0, 100.0)], 1, 1);
    assert_eq!(net.node(origin).segments.len(), 6, "the diagonal passes through the junction");
    let from = net.node_near(Vec2::new(100.0, 100.0), 1.0).unwrap();
    let to = net.node_near(Vec2::new(100.0, 0.0), 1.0).unwrap();
    assert!(net.path(from, to).is_some());
    // Through the bend of a polyline too.
    let mut net = RoadNetwork::default();
    net.add_polyline(&[Vec2::new(-100.0, 0.0), Vec2::ZERO, Vec2::new(100.0, 40.0)], 1, 1);
    net.add_polyline(&[Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0)], 1, 1);
    let bend = net.node_near(Vec2::ZERO, 1.0).unwrap();
    assert_eq!(net.node(bend).segments.len(), 4);
}

#[test]
fn a_street_vertex_near_the_track_does_not_make_a_crossing() {
    let mut net = RoadNetwork::default();
    net.add_polyline_kind(&[Vec2::new(-100.0, 0.0), Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0)], 0, 0, RoadKind::Rail);
    // A street bending 4 m from the track's middle node, never crossing it.
    net.add_polyline(&[Vec2::new(-100.0, 40.0), Vec2::new(0.0, 4.0), Vec2::new(100.0, 40.0)], 1, 1);
    let rail_mid = net.node_near(Vec2::ZERO, 1.0).unwrap();
    assert!(net.node(rail_mid).segments.iter().all(|&s| net.segment(s).is_rail()), "no street joins the rail node");
    let street_mid = net.node_near(Vec2::new(0.0, 4.0), 1.0).unwrap();
    assert_ne!(rail_mid, street_mid);
}

#[test]
fn crossing_roads_make_an_intersection_node() {
    let mut net = RoadNetwork::default();
    net.add_polyline(&[Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0)], 1, 1);
    net.add_polyline(&[Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0)], 1, 1);
    assert_eq!(net.nodes.len(), 5);
    assert_eq!(net.alive_segments().count(), 4);
    let from = net.node_near(Vec2::new(-100.0, 0.0), 1.0).unwrap();
    let to = net.node_near(Vec2::new(0.0, 100.0), 1.0).unwrap();
    let path = net.path(from, to).unwrap();
    assert_eq!(path.len(), 3);
}

#[test]
fn grid_is_fully_connected() {
    let mut net = RoadNetwork::default();
    for i in -2..=2 {
        let c = i as f32 * 100.0;
        net.add_polyline(&[Vec2::new(-200.0, c), Vec2::new(200.0, c)], 1, 1);
        net.add_polyline(&[Vec2::new(c, -200.0), Vec2::new(c, 200.0)], 1, 1);
    }
    let a = net.node_near(Vec2::new(-200.0, -200.0), 1.0).unwrap();
    let b = net.node_near(Vec2::new(200.0, 200.0), 1.0).unwrap();
    assert!(net.path(a, b).is_some());
}

#[test]
fn road_end_dropped_near_a_road_snaps_and_makes_a_junction() {
    let mut net = RoadNetwork::default();
    net.add_polyline(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
    // The end lands 6 m off the centreline, well inside the snap radius.
    net.add_polyline(&[Vec2::new(50.0, 150.0), Vec2::new(50.0, 6.0)], 1, 1);
    let junction = net.nodes.iter().position(|n| n.pos.distance(Vec2::new(50.0, 0.0)) < 0.5).expect("node on the road");
    let alive = net.nodes[junction].segments.iter().filter(|&&s| net.segment(s).alive).count();
    assert_eq!(alive, 3, "the long road is split at the snapped end");
    assert_eq!(net.alive_segments().count(), 3);
    // Far away ends stay free.
    net.add_polyline(&[Vec2::new(-50.0, 150.0), Vec2::new(-50.0, 40.0)], 1, 1);
    assert!(net.nodes.iter().any(|n| n.pos.distance(Vec2::new(-50.0, 40.0)) < 0.5));
}

#[test]
fn snap_point_prefers_nodes_then_roads() {
    let mut net = RoadNetwork::default();
    net.add_polyline(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 2, 2);
    let (p, snapped) = net.snap_point(Vec2::new(30.0, 9.0), 8.0);
    assert!(snapped && p.distance(Vec2::new(30.0, 0.0)) < 0.01);
    let (p, snapped) = net.snap_point(Vec2::new(196.0, 5.0), 8.0);
    assert!(snapped && p == Vec2::new(200.0, 0.0));
    let (_, snapped) = net.snap_point(Vec2::new(30.0, 60.0), 8.0);
    assert!(!snapped);
}

fn degree(net: &RoadNetwork, n: NodeId) -> usize {
    net.neighbors(n).count()
}

#[test]
fn streets_paths_and_avenues_crossing_at_grade_make_junctions() {
    // (lanes forward, backward, kind) of the way crossed and the way drawn.
    let kinds = [(1, 1, RoadKind::Street), (0, 0, RoadKind::Path), (3, 3, RoadKind::Street), (2, 0, RoadKind::Street)];
    for &(f0, b0, k0) in &kinds {
        for &(f1, b1, k1) in &kinds {
            let mut net = RoadNetwork::default();
            net.add_polyline_kind(&[Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0)], f0, b0, k0);
            net.add_polyline_kind(&[Vec2::new(10.0, -100.0), Vec2::new(10.0, 100.0)], f1, b1, k1);
            let x = net.node_near(Vec2::new(10.0, 0.0), 0.5).unwrap_or_else(|| panic!("{k0:?}x{k1:?}: no node at the crossing"));
            assert_eq!(degree(&net, x), 4, "{k0:?}({f0},{b0}) x {k1:?}({f1},{b1})");
            assert!(net.grade_crossings().is_empty());
        }
    }
}

#[test]
fn a_crossing_at_a_vertex_or_curve_joins_without_a_second_node() {
    // Crossing a polyline exactly at its bend.
    let mut net = RoadNetwork::default();
    net.add_polyline(&[Vec2::new(-100.0, -20.0), Vec2::new(0.0, 0.0), Vec2::new(100.0, -20.0)], 1, 1);
    net.add_polyline(&[Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0)], 1, 1);
    let bend = net.node_near(Vec2::ZERO, 0.5).unwrap();
    assert_eq!(degree(&net, bend), 4);
    assert_eq!(net.nodes.iter().filter(|n| n.pos.distance(Vec2::ZERO) < SNAP_RADIUS).count(), 1);
    // Two sampled curves crossing: one node where they meet, every way
    // still through.
    let a = crate::plan::sample_curve(&[Vec2::new(-120.0, -60.0), Vec2::new(0.0, 30.0), Vec2::new(120.0, -60.0)]);
    let b = crate::plan::sample_curve(&[Vec2::new(-60.0, 80.0), Vec2::new(0.0, -10.0), Vec2::new(60.0, 80.0)]);
    let mut net = RoadNetwork::default();
    net.add_polyline(&a, 1, 1);
    net.add_polyline(&b, 1, 1);
    let junctions: Vec<NodeId> = (0..net.nodes.len() as u32).map(NodeId).filter(|&n| degree(&net, n) >= 3).collect();
    assert_eq!(junctions.len(), 2, "the curves cross twice");
    for &j in &junctions {
        assert_eq!(degree(&net, j), 4);
    }
    let (s, e) = (net.node_near(a[0], 0.5).unwrap(), net.node_near(*b.last().unwrap(), 0.5).unwrap());
    assert!(net.path(s, e).is_some());
}

#[test]
fn a_raised_street_passes_over_a_street_without_a_junction() {
    let mut net = RoadNetwork::default();
    net.add_polyline(&[Vec2::new(-100.0, 0.0), Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0)], 1, 1);
    // A viaduct 8 m up crossing the street a few metres from its
    // middle node, with a vertex of its own within snapping distance.
    net.add_polyline_levels(&[Vec2::new(3.0, -100.0), Vec2::new(3.0, -2.0), Vec2::new(3.0, 100.0)], &[8.0, 8.0, 8.0], 1, 1, RoadKind::Street);
    let mid = net.node_near(Vec2::ZERO, 0.5).unwrap();
    assert_eq!(degree(&net, mid), 2, "the street's node is not joined");
    assert!(net.alive_segments().all(|(_, s)| (net.node(s.a).level > 1.0) == (net.node(s.b).level > 1.0)), "no segment links the two levels");
    let x = net.grade_crossings();
    assert_eq!(x.len(), 1);
    assert!(net.segment_level(x[0].upper.0, x[0].upper.1) > 7.0 && net.segment_level(x[0].lower.0, x[0].lower.1).abs() < 0.01);
    assert!(x[0].at.distance(Vec2::new(3.0, 0.0)) < 0.01);
    // The ways are not connected at all.
    let (s, e) = (net.node_near(Vec2::new(-100.0, 0.0), 0.5).unwrap(), net.node_near(Vec2::new(3.0, 100.0), 0.5).unwrap());
    assert!(net.path(s, e).is_none());
    // Drawn first or second, the same.
    let mut net2 = RoadNetwork::default();
    net2.add_polyline_levels(&[Vec2::new(3.0, -100.0), Vec2::new(3.0, 100.0)], &[8.0, 8.0], 1, 1, RoadKind::Street);
    net2.add_polyline(&[Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0)], 1, 1);
    assert_eq!(net2.grade_crossings().len(), 1);
    assert!(net2.nodes.iter().enumerate().all(|(i, _)| degree(&net2, NodeId(i as u32)) <= 2));
    // A street only a little above the other still meets it at grade.
    let mut net3 = RoadNetwork::default();
    net3.add_polyline(&[Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0)], 1, 1);
    net3.add_polyline_levels(&[Vec2::new(3.0, -100.0), Vec2::new(3.0, 100.0)], &[2.0, 2.0], 1, 1, RoadKind::Street);
    assert!(net3.grade_crossings().is_empty());
    assert_eq!(net3.nodes.iter().enumerate().filter(|(i, _)| degree(&net3, NodeId(*i as u32)) == 4).count(), 1);
}

#[test]
fn streets_cross_highways_without_junctions_and_ramps_still_join() {
    let mut net = RoadNetwork::default();
    net.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 3, 0);
    net.add_polyline(&[Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0)], 2, 2);
    net.add_polyline_kind(&[Vec2::new(40.0, -80.0), Vec2::new(40.0, 80.0)], 0, 0, RoadKind::Path);
    let x = net.grade_crossings();
    assert_eq!(x.len(), 2, "the street and the path pass under");
    for c in &x {
        assert!(net.segment(c.upper.0).is_highway(), "the highway goes over");
        // The highway has a vertex over the crossing to lift.
        assert!(c.upper.1 < 0.01 || c.upper.1 > 0.99);
    }
    assert!(net.nodes.iter().enumerate().all(|(i, _)| degree(&net, NodeId(i as u32)) <= 2), "no junction on the highway");
    // A ramp dropped onto it joins.
    net.add_polyline(&[Vec2::new(150.0, 80.0), Vec2::new(150.0, 2.0)], 1, 0);
    let j = net.node_near(Vec2::new(150.0, 0.0), 0.5).unwrap();
    assert_eq!(degree(&net, j), 3);
    // A street drawn over the highway stays over it.
    net.add_polyline_levels(&[Vec2::new(-150.0, -100.0), Vec2::new(-150.0, 100.0)], &[7.0, 7.0], 1, 1, RoadKind::Street);
    let over = net.grade_crossings().into_iter().find(|c| c.at.x < -100.0).unwrap();
    assert!(!net.segment(over.upper.0).is_highway());
}

/// [fix/roads-tools] A footpath dropped by a street (G12) lands on the
/// pavement on its own side, clear of the junction at the street's end,
/// and the network then joins it to the street beside the junction, not
/// to the junction's node.
#[test]
fn a_path_dropped_by_a_street_lands_on_its_pavement() {
    let mut net = RoadNetwork::default();
    net.add_polyline(&[Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0)], 2, 2);
    net.add_polyline(&[Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0)], 2, 2);
    let hw = net.segment(net.nearest_where(Vec2::new(50.0, 0.0), |_| true).unwrap().0).half_width();
    // On the north side of the east arm.
    let (q, level) = net.pavement_snap(Vec2::new(30.0, 4.0), 8.0).expect("a pavement");
    assert_eq!(level, 0.0);
    assert!((q.y - (hw + crate::zoning::SIDEWALK * 0.55)).abs() < 1e-3, "on the pavement: {q}");
    assert!((q.x - 30.0).abs() < 1e-3);
    // Dropped right by the junction: still on a pavement, clear of it.
    let (c, _) = net.pavement_snap(Vec2::new(2.0, 1.0), 8.0).expect("a pavement");
    let off = hw + crate::zoning::SIDEWALK * 0.55;
    assert!(c.length() > SNAP_RADIUS + 1.0 && ((c.x.abs() - off).abs() < 1e-3 || (c.y.abs() - off).abs() < 1e-3), "on a pavement, clear of the junction: {c}");
    let junction = net.node_near(Vec2::ZERO, 0.5).unwrap();
    let before = net.node(junction).segments.len();
    net.add_polyline_levels(&[q, Vec2::new(q.x, 60.0)], &[0.0, 0.0], 0, 0, RoadKind::Path);
    assert_eq!(net.node(junction).segments.len(), before, "the junction takes no path");
    // A path nearer than the street keeps its own snapping.
    assert!(net.pavement_snap(Vec2::new(q.x + 0.2, 40.0), 8.0).is_none());
}
