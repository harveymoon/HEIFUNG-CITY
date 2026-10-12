use super::*;

fn rich() -> LandValue {
    let mut lv = LandValue::new(600.0, 50.0);
    for v in &mut lv.values {
        *v = 0.9;
    }
    lv
}

fn draw(roads: &RoadNetwork, net: &LaneNetwork, heights: &[f32]) -> Geometry {
    let lv = rich();
    let pal = Palette::default();
    generate_street(&StreetInput { roads, net, land_value: &lv, palette: &pal, seed: 4, heights, inner: &[], layer: StreetLayer::All, closed: &[] })
}

/// Junctions of every shape: a crossroads of a wide and a narrow street,
/// a T, a Y with an acute and two obtuse corners, and a long block with
/// mid-block zebras and a footpath crossing.
fn junctions() -> (RoadNetwork, LaneNetwork) {
    let mut roads = RoadNetwork::default();
    // Crossroads at the origin; its x street runs on through a T at
    // (300, 0).
    roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(480.0, 0.0)], 2, 2);
    roads.add_polyline(&[Vec2::new(0.0, -200.0), Vec2::new(0.0, 200.0)], 1, 1);
    roads.add_polyline(&[Vec2::new(300.0, -200.0), Vec2::new(300.0, 0.0)], 1, 1);
    // A Y at (-300, 0): two arms 60 degrees apart.
    let a = 150f32.to_radians();
    roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(-300.0, 0.0) + Vec2::from_angle(a) * 200.0], 1, 1);
    roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(-300.0, 0.0) + Vec2::from_angle(-a) * 200.0], 1, 1);
    // A footpath over the crossroads' long y street.
    roads.add_polyline_kind(&[Vec2::new(-80.0, 120.0), Vec2::new(80.0, 120.0)], 1, 1, RoadKind::Path);
    let mut net = LaneNetwork::default();
    net.rebuild(&roads);
    (roads, net)
}

/// Upward faces of the street's ground-level surfaces, as triangles in
/// the plane with a bounding box each.
fn ground_tris(g: &Geometry) -> Vec<([Vec2; 3], Vec2, Vec2)> {
    let mut out = Vec::new();
    for s in [Asphalt, Paving, Concrete, Tactile, WornAsphalt, WornPaving, WornConcrete] {
        let m = g.get(s);
        for t in m.indices.chunks(3) {
            let p = [t[0], t[1], t[2]].map(|i| Vec3::from(m.positions[i as usize]));
            let n = (p[1] - p[0]).cross(p[2] - p[0]);
            if n.length() < 1e-9 || n.normalize().y.abs() < 0.5 || p.iter().any(|q| q.y > 0.5) {
                continue;
            }
            let q = p.map(|v| Vec2::new(v.x, v.z));
            out.push((q, q[0].min(q[1]).min(q[2]), q[0].max(q[1]).max(q[2])));
        }
    }
    out
}

fn covered(tris: &[([Vec2; 3], Vec2, Vec2)], p: Vec2) -> bool {
    tris.iter().any(|(t, lo, hi)| {
        if p.x < lo.x - 1e-4 || p.y < lo.y - 1e-4 || p.x > hi.x + 1e-4 || p.y > hi.y + 1e-4 {
            return false;
        }
        let s = |a: Vec2, b: Vec2| (b - a).perp_dot(p - a);
        let (d0, d1, d2) = (s(t[0], t[1]), s(t[1], t[2]), s(t[2], t[0]));
        let e = 1e-4;
        (d0 >= -e && d1 >= -e && d2 >= -e) || (d0 <= e && d1 <= e && d2 <= e)
    })
}

/// A ramp carried on from a highway's start keeps its own width, and
/// the joint narrows over a taper instead of stepping: asphalt reaches
/// out past the ramp's edge just beside the joint, not further on.
#[test]
fn a_lane_drop_at_a_joint_tapers() {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(0.0, 0.0), Vec2::new(200.0, 0.0), Vec2::new(400.0, 0.0)], 4, 0);
    roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(-150.0, 0.0), Vec2::new(0.0, 0.0)], 1, 0);
    let mut net = LaneNetwork::default();
    net.rebuild(&roads);
    let g = draw(&roads, &net, &[]);
    let m = g.get(Asphalt);
    let tris: Vec<([Vec2; 3], Vec2, Vec2)> = m
        .indices
        .chunks(3)
        .map(|t| {
            let q = [t[0], t[1], t[2]].map(|i| Vec2::new(m.positions[i as usize][0], m.positions[i as usize][2]));
            (q, q[0].min(q[1]).min(q[2]), q[0].max(q[1]).max(q[2]))
        })
        .collect();
    let ramp_hw = LANE_WIDTH * 0.5 + SHOULDER;
    let highway_hw = 2.0 * LANE_WIDTH + SHOULDER;
    let beside = (ramp_hw + highway_hw) * 0.5;
    assert!(covered(&tris, Vec2::new(-2.0, beside)) && covered(&tris, Vec2::new(-2.0, -beside)), "the joint widens out to the highway");
    assert!(!covered(&tris, Vec2::new(-100.0, beside)), "the ramp further on keeps its width");
    // The highway itself is drawn its full width right up to the joint.
    assert!(covered(&tris, Vec2::new(2.0, highway_hw - 0.5)));
}

/// Round every junction, everything within the arms' pavements out to
/// 25 m from the centre is road, kerb or paving: the terrain used to
/// show in each corner between one pavement's end, the pad and the
/// next pavement.
#[test]
fn junction_corners_leave_no_bare_ground() {
    let (roads, net) = junctions();
    let g = draw(&roads, &net, &[]);
    let tris = ground_tris(&g);
    let mut checked = 0;
    for (i, node) in roads.nodes.iter().enumerate() {
        let arms: Vec<(Vec2, f32)> = node
            .segments
            .iter()
            .filter(|&&s| roads.segment(s).alive && roads.segment(s).is_street())
            .map(|&s| {
                let (a, b) = roads.segment_points(s);
                let seg = roads.segment(s);
                ((if seg.a.0 as usize == i { b - a } else { a - b }).normalize(), seg.half_width())
            })
            .collect();
        if arms.len() < 3 {
            continue;
        }
        let mut bare = Vec::new();
        for gx in -60..=60 {
            for gy in -60..=60 {
                let off = Vec2::new(gx as f32, gy as f32) * 0.4;
                let p = node.pos + off;
                let inside = arms.iter().any(|&(d, hw)| {
                    let s = off.dot(d);
                    s >= 0.0 && s <= 22.0 && off.perp_dot(d).abs() <= hw + SIDEWALK - 0.05
                });
                if !inside {
                    continue;
                }
                checked += 1;
                if !covered(&tris, p) {
                    bare.push(p);
                }
            }
        }
        assert!(bare.is_empty(), "junction at {:?}: {} bare points, e.g. {:?}", node.pos, bare.len(), &bare[..bare.len().min(6)]);
    }
    assert!(checked > 3000, "{checked} points checked");
    // The corners are rounded: at the crossroads no kerb stands at the
    // sharp corner where the kerb lines meet, and the asphalt reaches it.
    let wide = 2.0 * LANE_WIDTH + SHOULDER;
    let narrow = LANE_WIDTH + SHOULDER;
    let tip = Vec2::new(narrow, wide);
    let kerb_near_tip = g.get(Concrete).positions.iter().any(|p| Vec2::new(p[0], p[2]).distance(tip) < 0.6);
    assert!(!kerb_near_tip, "the crossroads' corner is rounded off");
    assert!(g.get(Asphalt).positions.iter().any(|p| Vec2::new(p[0], p[2]).distance(tip) < 1e-3), "asphalt out to the old corner");
}

/// The crosswalk ends in `g` that meet no kerb; panics where a kerb is
/// met but not dropped, stands full height across the crosswalk, or has
/// no tactile pad behind it.
fn crosswalk_ends_without_kerb(roads: &RoadNetwork, g: &Geometry) -> Vec<(Vec2, f32)> {
    let cuts = crossing_cuts(roads);
    let all: Vec<(u32, Cut)> = cuts.iter().flat_map(|(s, v)| v.iter().map(move |c| (*s, *c))).collect();
    let kerb = g.get(Concrete);
    let pads = g.get(Tactile);
    let mut no_kerb = Vec::new();
    for (sid, c) in &all {
        let hw = roads.segment(SegmentId(*sid)).half_width();
        for side in [-1.0f32, 1.0] {
            // (u along the street, v out from its centre line on this side)
            let local = |p: &[f32; 3]| {
                let q = Vec2::new(p[0], p[2]) - c.mid;
                (q.dot(c.along), -q.perp_dot(c.along) * side)
            };
            if !kerb.positions.iter().map(local).any(|(u, v)| u.abs() <= c.half + 0.01 && v > hw - 0.1 && v < hw + 2.0) {
                no_kerb.push((c.mid, side));
                continue;
            }
            let low = kerb.positions.iter().filter(|p| (p[1] - KERB_LOW).abs() < 0.004).map(local).filter(|&(u, v)| u.abs() <= c.half + 0.01 && v > hw - 0.1 && v < hw + 2.0).count();
            assert!(low >= 2, "crosswalk at {:?} (side {side}): no dropped kerb", c.mid);
            let high = kerb.positions.iter().filter(|p| (p[1] - KERB_H).abs() < 0.004).map(local).find(|&(u, v)| u.abs() < c.half - 0.05 && v > hw - 0.1 && v < hw + 0.4);
            assert!(high.is_none(), "crosswalk at {:?} (side {side}): a full-height kerb across it at {high:?}", c.mid);
            let pad = pads.positions.iter().map(local).filter(|&(u, v)| u.abs() <= c.half + 0.05 && v > hw && v < hw + 3.0).count();
            assert!(pad >= 4, "crosswalk at {:?} (side {side}): no tactile pad", c.mid);
        }
    }
    no_kerb
}

/// Every crosswalk end meets a dropped kerb with a tactile pad behind
/// it, and nowhere across the crosswalk does the kerb stand full height.
#[test]
fn every_crosswalk_end_gets_a_kerb_drop_and_a_tactile_pad() {
    let (roads, net) = junctions();
    let g = draw(&roads, &net, &[]);
    let cuts = crossing_cuts(&roads);
    let all: Vec<(u32, Cut)> = cuts.iter().flat_map(|(s, v)| v.iter().map(move |c| (*s, *c))).collect();
    let kinds = (
        all.iter().filter(|(_, c)| c.half == nd_sim::walk::CROSSWALK_HALF).count(),
        all.iter().filter(|(_, c)| c.half == 1.3).count(),
    );
    assert!(kinds.0 >= 10 && kinds.1 >= 2, "junction, mid-block and path crossings: {kinds:?}");
    assert!(!nd_sim::walk::midblock_crossings(&roads).is_empty(), "the long block has a mid-block zebra");
    let no_kerb = crosswalk_ends_without_kerb(&roads, &g);
    assert!(no_kerb.is_empty(), "crosswalk ends that meet no kerb: {no_kerb:?}");
    // The pads lie low, on the landing behind the dropped kerb.
    assert!(g.get(Tactile).positions.iter().all(|p| p[1] < KERB_LOW + 0.01));
}

/// [review] A Y of three narrow streets, two of them 60 degrees apart:
/// the crosswalks either side of the sharp corner used to end on the
/// asphalt where the two arms run together, short of where their kerb
/// lines even meet, so the corner got no curb cut. They are set out to
/// the rounded kerb now (`nd_sim::walk::crosswalk_setback`).
#[test]
fn a_sharp_corner_gets_its_curb_cuts() {
    for deg in [60f32, 50.0, 70.0] {
        let mut roads = RoadNetwork::default();
        let a = (180.0 - deg * 0.5).to_radians();
        roads.add_polyline(&[Vec2::ZERO, Vec2::new(120.0, 0.0)], 1, 1);
        roads.add_polyline(&[Vec2::ZERO, Vec2::from_angle(a) * 120.0], 1, 1);
        roads.add_polyline(&[Vec2::ZERO, Vec2::from_angle(-a) * 120.0], 1, 1);
        let mut net = LaneNetwork::default();
        net.rebuild(&roads);
        let g = draw(&roads, &net, &[]);
        assert_eq!(crossing_cuts(&roads).values().map(Vec::len).sum::<usize>(), 3, "three crosswalks");
        let no_kerb = crosswalk_ends_without_kerb(&roads, &g);
        assert!(no_kerb.is_empty(), "{deg} degrees: crosswalk ends that meet no kerb: {no_kerb:?}");
    }
}

/// Curb cuts and corners are cheap: a pad is a couple of triangles (the
/// dots are in the texture), a curb cut a few dozen, a corner a couple
/// of hundred, so a city with thousands of crossings stays in budget.
#[test]
fn kerbs_stay_within_the_triangle_budget() {
    let cols = KerbColours { kerb: [1.0; 4], paving: [1.0; 4], tactile: [1.0; 4] };
    let face = [Vec2::new(0.0, 5.0), Vec2::new(60.0, 5.0)];
    let mut plain = Geometry::default();
    kerb_ribbon(&mut plain, &face, None, -1.0, SIDEWALK, true, true, &[], &cols);
    let cut = Cut { mid: Vec2::new(30.0, 0.0), along: Vec2::X, half: nd_sim::walk::CROSSWALK_HALF, reach: 5.0 + SIDEWALK + 0.5 };
    let mut dropped = Geometry::default();
    kerb_ribbon(&mut dropped, &face, None, -1.0, SIDEWALK, true, true, &[cut], &cols);
    let extra = dropped.triangle_count() - plain.triangle_count();
    assert!(extra <= 60, "a curb cut costs {extra} triangles");
    assert!(dropped.get(Tactile).indices.len() / 3 <= 4, "a pad is a quad or two");
    assert!(plain.triangle_count() <= 12, "a plain kerb is a few quads: {}", plain.triangle_count());

    let (roads, net) = junctions();
    let runs = build_runs(&roads);
    let corners = junction_corners(&roads, &net, &runs);
    let n: usize = corners.by_node.values().map(|v| v.len()).sum();
    assert!(n >= 4 + 3 + 3, "{n} corners");
    let g = draw(&roads, &net, &[]);
    let crossings: usize = crossing_cuts(&roads).values().map(|v| v.len()).sum();
    let tactile = g.get(Tactile).indices.len() / 3;
    assert!(tactile <= crossings * 2 * 8, "{tactile} pad triangles for {crossings} crossings");
    for list in corners.by_node.values() {
        for c in list {
            let mut one = Geometry::default();
            kerb_ribbon(&mut one, &c.kerb, Some(&c.normals), -1.0, CORNER_RIBBON, false, false, &[], &cols);
            one.part(Paving).ground_polygon(&c.fill, KERB_H, [1.0; 4]);
            one.part(Asphalt).ground_polygon(&c.apron, ASPHALT_Y, [1.0; 4]);
            assert!(one.triangle_count() <= 220, "a corner costs {} triangles", one.triangle_count());
        }
    }
}

/// On a sloping street the corners stand at the road's height, not the
/// junction's: the kerb where a corner meets a run is at the run's
/// height, so there is no step or gap between them.
#[test]
fn corners_follow_the_slope_of_their_streets() {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0)], 1, 1);
    roads.add_polyline(&[Vec2::new(0.0, 0.0), Vec2::new(0.0, 100.0)], 1, 1);
    let mut net = LaneNetwork::default();
    net.rebuild(&roads);
    // Rising 6 m over 100 m to the east, flat elsewhere.
    let heights: Vec<f32> = roads.nodes.iter().map(|n| if n.pos.x > 50.0 { 6.0 } else { 0.0 }).collect();
    let g = draw(&roads, &net, &heights);
    for (s, m) in g.iter() {
        assert!(m.positions.iter().all(|p| p.iter().all(|v| v.is_finite())), "{s:?}");
    }
    // Kerb tops round the east corners stand at the street's height
    // there (6 m over 100 m), within a few centimetres.
    let kerb = g.get(Concrete);
    let mut seen = 0;
    for p in kerb.positions.iter().filter(|p| p[0] > 6.0 && p[0] < 14.0 && (p[2].abs() - 5.0).abs() < 0.4) {
        let road = p[0] / 100.0 * 6.0;
        assert!(p[1] >= road - 0.05 && p[1] <= road + KERB_H + 0.05, "kerb at x {} stands at {} (road {road})", p[0], p[1]);
        seen += 1;
    }
    assert!(seen > 4);
}

/// A roundabout's ring has a pavement round its outside that the arms'
/// pavements run into, and none on its inside.
#[test]
fn a_roundabout_ring_is_paved_round_the_outside() {
    use nd_sim::plan::{Gesture, GestureId, Plan};
    let mut plan = Plan::default();
    plan.gestures.insert(GestureId(1), Gesture::Road { points: vec![Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
    plan.gestures.insert(GestureId(2), Gesture::Road { points: vec![Vec2::new(0.0, -200.0), Vec2::new(0.0, 200.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
    plan.gestures.insert(GestureId(3), Gesture::Roundabout { center: Vec2::ZERO, radius: 30.0, lanes: 1, level: 0.0 });
    let (built, rings) = plan.built_roads();
    let mut roads = RoadNetwork::default();
    roads.rings = rings;
    for (_, pts, f, b, k) in built {
        roads.add_polyline_kind(&pts, f, b, k);
    }
    let mut net = LaneNetwork::default();
    net.rebuild(&roads);
    let ring = roads.rings[0];
    let g = draw(&roads, &net, &[]);
    let tris = ground_tris(&g);
    let out = ring.radius + ring.half_width() + 1.5;
    let bare: Vec<Vec2> = (0..72).map(|k| ring.center + Vec2::from_angle(k as f32 * 5f32.to_radians()) * out).filter(|&p| !covered(&tris, p)).collect();
    assert!(bare.is_empty(), "bare ground round the ring at {bare:?}");
    let kerb_inside = g.get(Concrete).positions.iter().any(|p| {
        let d = Vec2::new(p[0], p[2]).distance(ring.center);
        d < ring.radius - ring.half_width() + 0.5 && d > ring.radius - ring.half_width() - 3.0
    });
    assert!(!kerb_inside, "no pavement inside the ring");
}

/// [walk-crossings] G9: a circus big enough to have land in the middle
/// has a pavement round the inside of its ring too.
#[test]
fn a_big_circus_is_paved_round_the_inside_too() {
    use nd_sim::plan::{Gesture, GestureId, Plan};
    let mut plan = Plan::default();
    plan.gestures.insert(GestureId(1), Gesture::Road { points: vec![Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
    plan.gestures.insert(GestureId(3), Gesture::Roundabout { center: Vec2::ZERO, radius: 70.0, lanes: 2, level: 0.0 });
    let (built, rings) = plan.built_roads();
    let mut roads = RoadNetwork::default();
    roads.rings = rings;
    for (_, pts, f, b, k) in built {
        roads.add_polyline_kind(&pts, f, b, k);
    }
    let mut net = LaneNetwork::default();
    net.rebuild(&roads);
    let ring = roads.rings[0];
    assert!(ring.radius >= super::draw::BIG_RING_RADIUS);
    let g = draw(&roads, &net, &[]);
    let inner = ring.radius - ring.half_width();
    let kerb_inside = g.get(Concrete).positions.iter().filter(|p| {
        let d = Vec2::new(p[0], p[2]).distance(ring.center);
        d < inner + 0.5 && d > inner - 3.0
    }).count();
    assert!(kerb_inside > 50, "a pavement round the inside: {kerb_inside} vertices");
}

/// Prints the street triangles of a save, for comparing before and
/// after a change: `ND_STREET_SAVE=path cargo test --release -p
/// nd_procgen street_triangles_of_a_save -- --ignored --nocapture`.
#[test]
#[ignore]
fn street_triangles_of_a_save() {
    let Ok(path) = std::env::var("ND_STREET_SAVE") else { return };
    let w = nd_sim::world::World::from_ron(&std::fs::read_to_string(path).unwrap()).unwrap();
    let heights: &[f32] = if w.relief { &w.road_z } else { &[] };
    let inner: &[Vec<(f32, f32)>] = if w.relief { &w.road_inner } else { &[] };
    let pal = Palette::default();
    let t = std::time::Instant::now();
    let g = generate_street(&StreetInput { roads: &w.roads, net: &w.traffic.net, land_value: &w.land_value, palette: &pal, seed: w.config.seed, heights, inner, layer: StreetLayer::Surface, closed: &[] });
    let ms = t.elapsed().as_millis();
    let cuts: usize = crossing_cuts(&w.roads).values().map(|v| v.len()).sum();
    let corners: usize = junction_corners(&w.roads, &w.traffic.net, &build_runs(&w.roads)).by_node.values().map(|v| v.len()).sum();
    println!("street triangles {} (tactile {}), {cuts} crossings, {corners} corners, {ms} ms", g.triangle_count(), g.get(Tactile).indices.len() / 3);
    if let Ok(n) = std::env::var("ND_STREET_NODE") {
        let nid = NodeId(n.parse().unwrap());
        let runs = build_runs(&w.roads);
        for &s in &w.roads.node(nid).segments {
            let sg = w.roads.segment(s);
            println!("seg {} alive {} kind {:?} lanes {}+{} ramp {} ring {} tunnel {} hw {}", s.0, sg.alive, sg.kind, sg.lanes_fwd, sg.lanes_bwd, sg.ramp, sg.ring, sg.tunnel, sg.half_width());
        }
        let c = junction_corners(&w.roads, &w.traffic.net, &runs);
        for (ri, r) in runs.iter().enumerate().filter(|(_, r)| r.start_node == nid || r.end_node == nid) {
            println!("run {ri} len {} start {} trims {:?} {:?} {:?} {:?}", path_length(&r.points), r.start_node == nid, c.trims.get(&(ri, true, true)), c.trims.get(&(ri, true, false)), c.trims.get(&(ri, false, true)), c.trims.get(&(ri, false, false)));
        }
        println!("corners {}", c.by_node.get(&nid.0).map_or(0, |v| v.len()));
    }
    if std::env::var("ND_STREET_PLACES").is_ok() {
        // Where to look: signalised junctions, Ts, mid-block zebras and
        // crossings on a slope.
        for i in w.traffic.net.intersections.iter().filter(|i| i.signalized).take(3) {
            println!("signalised {:?}", w.roads.node(i.node).pos);
        }
        for (k, n) in w.roads.nodes.iter().enumerate().filter(|(k, _)| w.roads.street_degree(NodeId(*k as u32)) == 3).take(3) {
            println!("T {:?} z {}", n.pos, heights.get(k).copied().unwrap_or(0.0));
        }
        for (s, t) in nd_sim::walk::midblock_crossings(&w.roads).into_iter().take(3) {
            let (a, b) = w.roads.segment_points(s);
            let sg = w.roads.segment(s);
            let dz = (heights.get(sg.a.0 as usize).copied().unwrap_or(0.0) - heights.get(sg.b.0 as usize).copied().unwrap_or(0.0)).abs() / a.distance(b);
            println!("zebra {:?} slope {dz}", a.lerp(b, t));
        }
        let mut steep: Vec<(f32, Vec2)> = crossing_cuts(&w.roads)
            .iter()
            .map(|(s, v)| {
                let sg = w.roads.segment(SegmentId(*s));
                let (a, b) = w.roads.segment_points(SegmentId(*s));
                ((heights.get(sg.a.0 as usize).copied().unwrap_or(0.0) - heights.get(sg.b.0 as usize).copied().unwrap_or(0.0)).abs() / a.distance(b).max(1.0), v[0].mid)
            })
            .collect();
        steep.sort_by(|a, b| b.0.total_cmp(&a.0));
        for (dz, p) in steep.iter().take(3) {
            println!("sloped crossing {p:?} slope {dz}");
        }
    }
}

/// #9 (vi) Where a street carries on with fewer lanes round a bend, the
/// two carriageways are cut along the bend's bisector: no asphalt at the
/// carriageway's height lies twice anywhere round the joint (both used to
/// run on to the node with square ends, and on the inside of the bend the
/// two z-fought), and none is missing across the narrower street.
#[test]
fn a_lane_count_joint_at_a_bend_does_not_overlap() {
    for bend in [20.0f32, 45.0] {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-150.0, 0.0), Vec2::ZERO], 2, 2);
        let e = Vec2::from_angle(bend.to_radians());
        roads.add_polyline(&[Vec2::ZERO, e * 150.0], 1, 1);
        let mut net = LaneNetwork::default();
        net.rebuild(&roads);
        let g = draw(&roads, &net, &[]);
        let m = g.get(Asphalt);
        let tris: Vec<[Vec2; 3]> = m
            .indices
            .chunks(3)
            .filter(|t| t.iter().all(|&i| (m.positions[i as usize][1] - ASPHALT_Y).abs() < 1e-4))
            .map(|t| [t[0], t[1], t[2]].map(|i| Vec2::new(m.positions[i as usize][0], m.positions[i as usize][2])))
            .collect();
        // Strictly inside: points on a shared edge do not count twice.
        let inside = |t: &[Vec2; 3], p: Vec2| {
            let s = |a: Vec2, b: Vec2| (b - a).perp_dot(p - a);
            let (d0, d1, d2) = (s(t[0], t[1]), s(t[1], t[2]), s(t[2], t[0]));
            (d0 > 1e-3 && d1 > 1e-3 && d2 > 1e-3) || (d0 < -1e-3 && d1 < -1e-3 && d2 < -1e-3)
        };
        let mut twice = Vec::new();
        for i in -60..=60 {
            for j in -60..=60 {
                let p = Vec2::new(i as f32 * 0.2, j as f32 * 0.2);
                if tris.iter().filter(|t| inside(t, p)).count() > 1 {
                    twice.push(p);
                }
            }
        }
        assert!(twice.is_empty(), "{bend} deg: asphalt laid twice at {} points, e.g. {:?}", twice.len(), &twice[..twice.len().min(5)]);
        // Across the narrower street just past the joint, and across the
        // wider one just before it, the carriageway is whole.
        let narrow_hw = LANE_WIDTH + SHOULDER;
        let n = e.perp();
        for k in -4..=4 {
            let off = k as f32 / 4.0 * (narrow_hw - 0.3);
            for p in [e * 0.3 + n * off, Vec2::new(-0.3, off)] {
                assert!(tris.iter().any(|t| inside(t, p) || (t[0] - p).length() < 1e-3), "{bend} deg: no asphalt at {p:?}");
            }
        }
    }
}

/// [fix/buildings-ui] G15: no street trees, lamps or poles stand under a
/// viaduct crossing the street: they would poke up through its deck. Away
/// from the viaduct the dear street keeps its trees.
#[test]
fn no_trees_under_a_viaduct() {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
    let first_deck = roads.nodes.len();
    roads.add_polyline_levels(&[Vec2::new(0.0, -200.0), Vec2::new(0.0, -60.0), Vec2::new(0.0, 60.0), Vec2::new(0.0, 200.0)], &[8.0; 4], 2, 2, nd_sim::roads::RoadKind::Street);
    // The street at the ground, the highway 8 m over it.
    let heights: Vec<f32> = (0..roads.nodes.len()).map(|k| if k >= first_deck { 8.0 } else { 0.0 }).collect();
    let mut net = LaneNetwork::default();
    net.rebuild(&roads);
    let deck = roads.segments_in_box(Vec2::new(-1.0, -1.0), Vec2::new(1.0, 1.0)).find(|&s| roads.segment(s).lanes_fwd == 2).expect("the deck over the street");
    let reach = roads.segment(deck).half_width();
    assert!(junction::under_deck(&roads, &heights, &[], Vec2::new(reach - 1.0, 9.0), 0.0, 1.0, 12.0), "beside the street, under the deck");
    assert!(!junction::under_deck(&roads, &heights, &[], Vec2::new(60.0, 9.0), 0.0, 1.0, 12.0), "out in the open");
    assert!(!junction::under_deck(&roads, &heights, &[], Vec2::new(reach - 1.0, 9.0), 8.0, 1.0, 12.0), "on the deck itself");
    let g = draw(&roads, &net, &heights);
    let leaves = g.get(Foliage);
    let under: Vec<[f32; 3]> = leaves.positions.iter().copied().filter(|p| p[0].abs() < reach + 2.0 && p[1] < 8.0 && p[2].abs() < 30.0).collect();
    assert!(under.is_empty(), "foliage under the viaduct at {:?}", &under[..under.len().min(4)]);
    assert!(leaves.positions.iter().any(|p| p[0].abs() > 40.0 && p[2].abs() < 15.0), "the street keeps its trees away from the viaduct");
}
