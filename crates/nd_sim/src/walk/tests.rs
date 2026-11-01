use super::*;
use crate::zoning::{generate_lots, Zone, ZoneId};

/// [review] At a sharp corner the crosswalk is set out past where the
/// two arms' kerb lines meet (else it ends on the asphalt between them,
/// and so does the pavement); a right-angled junction keeps the usual
/// set-back.
#[test]
fn sharp_corners_set_their_crosswalks_out_to_the_kerb() {
    let mut roads = RoadNetwork::default();
    let a = 150f32.to_radians();
    roads.add_polyline(&[Vec2::ZERO, Vec2::new(120.0, 0.0)], 1, 1);
    roads.add_polyline(&[Vec2::ZERO, Vec2::from_angle(a) * 120.0], 1, 1);
    roads.add_polyline(&[Vec2::ZERO, Vec2::from_angle(-a) * 120.0], 1, 1);
    let node = roads.nodes.iter().position(|n| n.pos.length() < 0.1).map(|i| NodeId(i as u32)).unwrap();
    let hw = roads.segment(roads.node(node).segments[0]).half_width();
    let usual = hw + CROSSWALK_BACK;
    // Where two kerb lines 60 degrees apart meet, along either arm.
    let meet = (hw + hw * 0.5) / 60f32.to_radians().sin();
    for &s in &roads.node(node).segments {
        let (p, q) = roads.segment_points(s);
        let far = if p.length() > q.length() { p } else { q };
        let set = crosswalk_setback(&roads, node, s);
        if far.x > 0.0 {
            assert_eq!(set, usual, "the straight-on arm keeps the usual set-back");
        } else {
            assert!(set - CROSSWALK_HALF > meet, "an arm of the sharp corner: zebra from {} m, the kerb lines meet at {meet} m", set - CROSSWALK_HALF);
        }
    }
    let mut plus = RoadNetwork::default();
    plus.add_polyline(&[Vec2::new(-120.0, 0.0), Vec2::new(120.0, 0.0)], 1, 1);
    plus.add_polyline(&[Vec2::new(0.0, -120.0), Vec2::new(0.0, 120.0)], 1, 1);
    let node = plus.nodes.iter().position(|n| n.pos.length() < 0.1).map(|i| NodeId(i as u32)).unwrap();
    for &s in &plus.node(node).segments {
        assert_eq!(crosswalk_setback(&plus, node, s), usual, "a crossroads is unchanged");
    }
}

/// A hand-built graph: `nodes` positions and `edges` as (a, b, interior, park).
fn graph(nodes: &[Vec2], edges: &[(u32, u32, bool, bool)]) -> WalkGraph {
    let mut g = WalkGraph::default();
    for p in nodes {
        g.add_node(*p, None);
    }
    for &(a, b, interior, park) in edges {
        g.link_kind(a, b, interior, park);
    }
    g.index = Grid::new(NODE_CELL, g.nodes.iter().map(|n| (n.pos, n.pos)));
    g
}

/// [under-construction] A park walkway and a path's gate into a lot
/// whose building is still a site are shut to walkers coming from
/// outside; whoever is inside walks out; once open, both are used.
#[test]
fn walkways_and_gates_of_a_site_are_shut_until_it_opens() {
    // 0-1 pavement; 1-2 a park path into the park (2, 3 on its lot);
    // 0-4 a player path, 4-3 its gate onto the park's walkway.
    let mut g = graph(
        &[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0), Vec2::new(-5.0, 5.0)],
        &[(0, 1, false, false), (1, 2, true, true), (2, 3, true, true), (0, 4, false, false), (4, 3, true, false)],
    );
    let mut lots: SlotMap<LotId, ()> = SlotMap::with_key();
    let park = lots.insert(());
    g.nodes[2].lot = Some(park);
    g.nodes[3].lot = Some(park);
    for (from, to) in [(4usize, 3u32), (3, 4)] {
        for e in g.adj[from].iter_mut().filter(|e| e.to == to) {
            e.gate = true;
        }
    }
    assert!(g.route_nodes(0, 3).is_some(), "open: in by the park path or the gate");
    g.closed = vec![park];
    assert!(g.node_closed(2) && !g.node_closed(1));
    assert_eq!(g.route_nodes(0, 3), None, "a site: neither the walkway nor the gate lets anyone in");
    assert_eq!(g.route_nodes(4, 2), None);
    assert!(g.route_nodes(3, 0).is_some(), "someone inside walks out");
    let d = g.distances_from(&[0]);
    assert!(d[2].is_infinite() && d[3].is_infinite(), "nor does the park count as within walking reach");
    g.closed.clear();
    assert!(g.route_nodes(4, 2).is_some(), "open again");
}

#[test]
fn the_heuristic_never_talks_a_route_out_of_a_cheaper_park_path() {
    // Start and goal 100 m apart on a pavement; a park path makes a
    // detour of 108 m that costs 99.36 with the park factor. With the
    // plain straight-line heuristic the goal popped first (f = 100
    // against 103.68 for the detour's corner) and the dearer pavement
    // was returned.
    let s = Vec2::ZERO;
    let goal = Vec2::new(100.0, 0.0);
    let corner = Vec2::new(50.0, (54.0f32 * 54.0 - 50.0 * 50.0).sqrt());
    let g = graph(&[s, goal, corner], &[(0, 1, false, false), (0, 2, true, true), (2, 1, true, true)]);
    let route = g.route_nodes(0, 1).expect("route");
    assert_eq!(route, vec![0, 2, 1], "the park detour is cheaper: 0.92 x 108 < 100");
    // And the pavement wins once the detour costs more than it saves.
    let far = Vec2::new(50.0, 40.0);
    let g = graph(&[s, goal, far], &[(0, 1, false, false), (0, 2, true, true), (2, 1, true, true)]);
    assert_eq!(g.route_nodes(0, 1).unwrap(), vec![0, 1]);
}

#[test]
fn nearest_node_matches_a_scan_and_respects_the_radius() {
    let pts: Vec<Vec2> = (0..15).flat_map(|y| (0..15).map(move |x| Vec2::new(x as f32 * 9.0, y as f32 * 9.0))).collect();
    let g = graph(&pts, &[]);
    for q in [Vec2::new(4.5, 4.5), Vec2::new(31.0, 70.2), Vec2::new(-3.0, 200.0), Vec2::new(63.0, 63.0)] {
        let lin = (0..pts.len()).filter(|&i| pts[i].distance(q) <= 60.0).min_by(|&a, &b| pts[a].distance(q).total_cmp(&pts[b].distance(q))).map(|i| i as u32);
        assert_eq!(g.nearest(q, 60.0), lin, "at {q:?}");
    }
    assert!(g.nearest(Vec2::new(500.0, 500.0), 60.0).is_none());
    assert!(WalkGraph::default().nearest(Vec2::ZERO, 60.0).is_none());
}

#[test]
fn walkers_cut_through_blocks_and_shops_see_them() {
    let mut roads = RoadNetwork::default();
    for c in [-70.0f32, 70.0] {
        roads.add_polyline(&[Vec2::new(-300.0, c), Vec2::new(300.0, c)], 1, 1);
        roads.add_polyline(&[Vec2::new(c, -300.0), Vec2::new(c, 300.0)], 1, 1);
    }
    let zone = Zone {
        polygon: vec![Vec2::new(-65.0, -65.0), Vec2::new(65.0, -65.0), Vec2::new(65.0, 65.0), Vec2::new(-65.0, 65.0)],
        land_use: LandUse::Commercial,
    };
    let mut lots = SlotMap::with_key();
    generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
    let lv = LandValue::new(400.0, 32.0);
    let g = WalkGraph::build(&roads, &lots, &lv, 1);
    assert!(g.nodes.len() > 50);
    assert!(g.adj.iter().any(|e| e.iter().any(|x| x.interior)), "lots have interior paths");
    // Crossing the block from the south pavement to the north one goes
    // through the lots instead of around the outside.
    let from = Vec2::new(0.0, -62.0);
    let to = Vec2::new(0.0, 62.0);
    let (pts, nodes) = g.route(from, to).expect("route exists");
    let walked: f32 = pts.windows(2).map(|w| w[0].distance(w[1])).sum();
    assert!(walked < 180.0, "cut through the block: {walked} (around would be about 244)");
    assert!(nodes.windows(2).any(|w| g.adj[w[0] as usize].iter().any(|e| e.to == w[1] && e.interior)), "uses a lot interior");
    let passed = g.lots_passed(&nodes);
    assert!(!passed.is_empty(), "the route passes lots");
}

/// Proper intersection of two segments (not just touching at an end).
fn crosses(p: Vec2, q: Vec2, a: Vec2, b: Vec2) -> bool {
    let d1 = (q - p).perp_dot(a - p);
    let d2 = (q - p).perp_dot(b - p);
    let d3 = (b - a).perp_dot(p - a);
    let d4 = (b - a).perp_dot(q - a);
    d1 * d2 < -1e-3 && d3 * d4 < -1e-3
}

/// A grid with a T, a dead end, a footpath over a street and one long
/// street, zoned so lots line the pavements.
fn crossing_town() -> (RoadNetwork, SlotMap<LotId, Lot>, WalkGraph) {
    let mut roads = RoadNetwork::default();
    for c in [-70.0f32, 70.0] {
        roads.add_polyline(&[Vec2::new(-300.0, c), Vec2::new(300.0, c)], 1, 1);
    }
    roads.add_polyline(&[Vec2::new(-70.0, -300.0), Vec2::new(-70.0, 300.0)], 1, 1);
    // A T onto the north street, and a dead end off it.
    roads.add_polyline(&[Vec2::new(70.0, 70.0), Vec2::new(70.0, 200.0)], 1, 1);
    // A footpath across the south street.
    roads.add_polyline_kind(&[Vec2::new(150.0, -150.0), Vec2::new(150.0, 10.0)], 0, 0, RoadKind::Path);
    let mut lots = SlotMap::with_key();
    for (x, y) in [(0.0f32, 0.0f32), (0.0, -140.0), (0.0, 140.0), (-160.0, 0.0), (200.0, 0.0)] {
        let zone = Zone { polygon: vec![Vec2::new(x - 60.0, y - 60.0), Vec2::new(x + 60.0, y - 60.0), Vec2::new(x + 60.0, y + 60.0), Vec2::new(x - 60.0, y + 60.0)], land_use: LandUse::Commercial };
        generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
    }
    let lv = LandValue::new(400.0, 32.0);
    let g = WalkGraph::build(&roads, &lots, &lv, 1);
    (roads, lots, g)
}

#[test]
fn walkers_cross_streets_only_at_crossings() {
    let (roads, _, g) = crossing_town();
    let streets: Vec<(Vec2, Vec2)> = roads.alive_segments().filter(|(_, s)| s.is_street()).map(|(sid, _)| roads.segment_points(sid)).collect();
    let mut kinds = HashMap::new();
    for c in &g.crossings {
        *kinds.entry(format!("{:?}", c.kind)).or_insert(0) += 1;
    }
    for (i, edges) in g.adj.iter().enumerate() {
        for e in edges.iter().filter(|e| e.cross == NO_CROSSING) {
            let (p, q) = (g.nodes[i].pos, g.nodes[e.to as usize].pos);
            for &(a, b) in &streets {
                assert!(!crosses(p, q, a, b), "a plain walk edge {p:?}-{q:?} crosses the street {a:?}-{b:?}");
            }
        }
    }
    // Nobody walks through the middle of a junction.
    for (ni, node) in roads.nodes.iter().enumerate() {
        if !is_junction(&roads, NodeId(ni as u32)) {
            continue;
        }
        let r = widest(&roads, NodeId(ni as u32));
        for (i, edges) in g.adj.iter().enumerate() {
            for e in edges {
                let (p, q) = (g.nodes[i].pos, g.nodes[e.to as usize].pos);
                let d = geom::project_on_segment(node.pos, p, q).1.distance(node.pos);
                assert!(d > r, "an edge passes {d} m from the middle of the junction at {:?}", node.pos);
            }
        }
    }
    for k in ["Junction", "Zebra", "Jay"] {
        assert!(kinds.get(k).copied().unwrap_or(0) > 0, "some {k} crossings: {kinds:?}");
    }
    // [r1-roads] (K47) The dead end is walked round, not crossed (it used
    // to be this town's unmarked crossing).
    let end = Vec2::new(70.0, 200.0);
    assert!(!g.crossings.iter().any(|c| c.mid.distance(end) < 4.0), "no crossing over the dead end");
    // The long streets get mid-block zebras, the 140 m blocks none.
    assert!(!midblock_crossings(&roads).is_empty());
    // Lawful routes never jaywalk; a jaywalker's may.
    let (from, to) = (Vec2::new(-20.0, -62.0), Vec2::new(-20.0, -78.0));
    let lawful = g.route(from, to).unwrap().1;
    assert!(lawful.windows(2).all(|w| g.crossing_between(g.nodes[w[0] as usize].pos, g.nodes[w[1] as usize].pos).is_none_or(|c| g.crossings[c as usize].lawful())));
    let bold = g.route_with(from, to, true).unwrap().1;
    assert!(bold.windows(2).any(|w| g.crossing_between(g.nodes[w[0] as usize].pos, g.nodes[w[1] as usize].pos).is_some_and(|c| !g.crossings[c as usize].lawful())), "straight over");
    assert!(bold.len() < lawful.len());
}

#[test]
fn dump_walk_graph_for_inspection() {
    let Ok(path) = std::env::var("ND_WALK_DUMP") else { return };
    let mut roads = RoadNetwork::default();
    for c in [-70.0f32, 70.0] {
        roads.add_polyline(&[Vec2::new(-300.0, c), Vec2::new(300.0, c)], 1, 1);
        roads.add_polyline(&[Vec2::new(c, -300.0), Vec2::new(c, 300.0)], 1, 1);
    }
    let zone = Zone {
        polygon: vec![Vec2::new(-65.0, -65.0), Vec2::new(65.0, -65.0), Vec2::new(65.0, 65.0), Vec2::new(-65.0, 65.0)],
        land_use: LandUse::Commercial,
    };
    let mut lots = SlotMap::with_key();
    generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
    let lv = LandValue::new(400.0, 32.0);
    let g = WalkGraph::build(&roads, &lots, &lv, 1);
    let mut out = String::new();
    for l in lots.values() {
        let pts: Vec<String> = l.polygon.iter().map(|p| format!("{},{}", p.x, p.y)).collect();
        out.push_str(&format!("{}|{}
", if l.usable() { "lot" } else { "dead" }, pts.join(";")));
    }
    for (i, edges) in g.adj.iter().enumerate() {
        for e in edges {
            if (i as u32) < e.to {
                let a = g.nodes[i].pos;
                let b = g.nodes[e.to as usize].pos;
                out.push_str(&format!("{}|{},{};{},{}
", if e.interior { "walk_in" } else { "walk" }, a.x, a.y, b.x, b.y));
            }
        }
    }
    if let Some((pts, _)) = g.route(Vec2::new(0.0, -62.0), Vec2::new(0.0, 62.0)) {
        let s: Vec<String> = pts.iter().map(|p| format!("{},{}", p.x, p.y)).collect();
        out.push_str(&format!("route|{}
", s.join(";")));
    }
    let _ = std::fs::write(path, out);
}

#[test]
fn distances_from_the_nearest_of_several_sources_along_the_graph() {
    let mut g = WalkGraph::default();
    // a - b - c - d in a line 10 m apart, and e on its own.
    let ids: Vec<u32> = [0.0, 10.0, 20.0, 30.0, 100.0].iter().map(|&x| g.add_node(Vec2::new(x, 0.0), None)).collect();
    g.link(ids[0], ids[1], false);
    g.link(ids[1], ids[2], false);
    g.link(ids[2], ids[3], true);
    let d = g.distances_from(&[ids[0], ids[3]]);
    assert_eq!(d[0], 0.0);
    assert!((d[1] - 10.0).abs() < 1e-4 && (d[2] - 10.0).abs() < 1e-4, "{d:?}");
    assert_eq!(d[3], 0.0);
    assert!(d[4].is_infinite(), "unreachable stays infinite");
    assert!(g.distances_from(&[]).iter().all(|x| x.is_infinite()));
    // A nearest lookup that finds nothing near does not wander off.
    g.index = Grid::new(NODE_CELL, g.nodes.iter().map(|n| (n.pos, n.pos)));
    assert_eq!(g.nearest_within(Vec2::new(12.0, 3.0), 5.0), Some(ids[1]));
    assert_eq!(g.nearest_within(Vec2::new(60.0, 0.0), 20.0), None);
}

/// [pt-walk] Components: two pieces of graph get two labels (each its
/// lowest node), a route between them is refused without a search, and
/// one within a piece is found as before.
#[test]
fn components_label_the_pieces_and_refuse_routes_between_them() {
    let mut g = graph(
        &[Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(20.0, 0.0), Vec2::new(500.0, 0.0), Vec2::new(510.0, 0.0), Vec2::new(900.0, 0.0)],
        &[(0, 1, false, false), (1, 2, false, false), (4, 3, false, false)],
    );
    assert_eq!(g.component(0), None, "none before they are worked out");
    g.compute_components();
    assert_eq!(g.comp, vec![0, 0, 0, 3, 3, 5]);
    assert_eq!(g.component(4), Some(3));
    assert!(g.route_nodes(0, 2).is_some());
    assert_eq!(g.route_nodes(0, 4), None);
    assert_eq!(g.route_nodes(5, 0), None, "a lone node reaches nothing");
    let near: Vec<u32> = g.nodes_in_box(Vec2::new(-1.0, -1.0), Vec2::new(15.0, 1.0));
    assert_eq!(near.len(), 2);
}

/// [r4-flags-look] (flag day 3 at (974,417)) No mid-block zebra out on a
/// raised deck or a bridge (one was painted over the highway on an
/// overpass's span); the long run's zebras on the ground stay.
#[test]
fn no_midblock_zebra_on_a_deck_or_bridge() {
    let mut roads = RoadNetwork::default();
    let pts: Vec<Vec2> = (0..=6).map(|k| Vec2::new(-300.0 + 100.0 * k as f32, 0.0)).collect();
    roads.add_polyline(&pts, 1, 1);
    let before = midblock_crossings(&roads).len();
    assert!(before >= 3, "a 600 m run gets its zebras ({before})");
    let raised: Vec<SegmentId> = roads.alive_segments().filter(|(_, s)| { let m = (roads.node(s.a).pos.x + roads.node(s.b).pos.x) * 0.5; m.abs() < 150.0 }).map(|(id, _)| id).collect();
    for (k, &s) in raised.iter().enumerate() {
        if k % 2 == 0 { roads.segments[s.0 as usize].viaduct = true } else { roads.segments[s.0 as usize].bridge = true }
    }
    let after = midblock_crossings(&roads);
    assert!(after.iter().all(|(s, _)| !raised.contains(s)), "a zebra on the deck: {after:?}");
    assert!(!after.is_empty() && after.len() < before, "the ground's zebras stay ({} of {before})", after.len());
}

/// [r6-perf] A 30 x 30 grid of pavements 12 m apart, some of it park paths
/// and some interior: big enough for the landmarks.
fn big_grid() -> WalkGraph {
    let n = 30u32;
    let nodes: Vec<Vec2> = (0..n * n).map(|i| Vec2::new((i % n) as f32 * 12.0, (i / n) as f32 * 12.0)).collect();
    let mut edges = Vec::new();
    for y in 0..n {
        for x in 0..n {
            let i = y * n + x;
            let h = (x * 7 + y * 13) % 10;
            if x + 1 < n {
                edges.push((i, i + 1, h == 3, h == 1));
            }
            if y + 1 < n && h != 5 {
                edges.push((i, i + n, h == 7, h == 2));
            }
        }
    }
    let mut g = graph(&nodes, &edges);
    g.compute_components();
    g
}

/// [r6-perf] The landmark search finds routes that cost what the plain
/// A* finds (the same route but where two cost exactly the same), and
/// pops far fewer nodes doing it: the guard on the walking searches that
/// were a quarter of a big city's tick.
#[test]
fn landmark_routes_cost_the_same_and_search_less() {
    let g = big_grid();
    let n = g.nodes.len() as u32;
    let (mut fast, mut plain) = (0u64, 0u64);
    for k in 0..300u32 {
        let (a, b) = ((k * 7919) % n, (k * 104_729 + 17) % n);
        let p0 = route::POPS.with(|p| p.get());
        let got = g.route_nodes(a, b);
        let p1 = route::POPS.with(|p| p.get());
        let want = g.search_reference(a, b, false);
        let p2 = route::POPS.with(|p| p.get());
        fast += p1 - p0;
        plain += p2 - p1;
        assert_eq!(got.is_some(), want.is_some(), "{a} -> {b}");
        if let (Some(x), Some(y)) = (&got, &want) {
            let (cx, cy) = (g.path_cost(x, false), g.path_cost(y, false));
            assert!((cx - cy).abs() <= 1e-4 * cy.max(1.0), "{a} -> {b}: {cx} against {cy}");
            assert_eq!((x[0], *x.last().unwrap()), (a, b));
        }
    }
    assert!(fast * 3 < plain, "the landmarks pop {fast} nodes against {plain} for the plain search");
}

/// [r6-perf] A remembered route is never handed out once a crossing on it
/// closes or its lot shuts: the memo's stamp covers both.
#[test]
fn the_route_memo_forgets_closed_crossings() {
    let (_, _, mut g) = crossing_town();
    let (from, to) = (Vec2::new(-20.0, -62.0), Vec2::new(-20.0, -78.0));
    let first = g.route(from, to).unwrap().1;
    assert_eq!(g.route(from, to).unwrap().1, first, "asked again: the same way");
    let used: Vec<u32> = first.windows(2).filter_map(|w| g.crossing_between(g.nodes[w[0] as usize].pos, g.nodes[w[1] as usize].pos)).collect();
    assert!(!used.is_empty(), "the way crosses the street");
    for &c in &used {
        g.crossings[c as usize].closed = true;
    }
    let after = g.route(from, to).map(|r| r.1);
    assert_ne!(after.as_ref(), Some(&first), "not over the closed crossing");
    if let Some(r) = after {
        assert!(r.windows(2).all(|w| g.crossing_between(g.nodes[w[0] as usize].pos, g.nodes[w[1] as usize].pos).is_none_or(|c| !used.contains(&c))));
    }
}
