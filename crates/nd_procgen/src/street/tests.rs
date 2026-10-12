/// The tube's every face looks into it, so back-face culling cuts away
/// whichever half faces the camera; seen from straight above, nothing of
/// the vault is drawn over the carriageway, from the side the near
/// wall goes and the far one stays.
#[test]
fn bore_faces_look_inwards() {
    let walk = vec![Vec2::new(0.0, 0.0), Vec2::new(40.0, 0.0), Vec2::new(80.0, 20.0)];
    let hw = 4.0;
    let mut g = Geometry::default();
    bore_shell(&mut g, &walk, hw, [1.0; 4], [0.5, 0.5, 0.5, 1.0], [2.0; 4]);
    let m = g.get(Concrete);
    assert!(m.indices.len() > 100);
    let axis = |p: Vec3| {
        // The nearest point of the walk's axis at the spring line.
        let q = Vec2::new(p.x, p.z);
        let (_, c) = walk.windows(2).map(|w| nd_sim::roads::project_on_segment(q, w[0], w[1])).fold((f32::MAX, Vec2::ZERO), |best, (_, c)| if c.distance(q) < best.0 { (c.distance(q), c) } else { best });
        Vec3::new(c.x, BORE_SPRING.min(p.y), c.y)
    };
    let (mut up, mut down) = (0, 0);
    for t in m.indices.chunks(3) {
        let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from(m.positions[i as usize]));
        let n = (b - a).cross(c - a);
        if n.length() < 1e-6 {
            continue;
        }
        let mid = (a + b + c) / 3.0;
        if mid.y < 0.05 {
            continue; // the invert, facing up
        }
        assert!(n.dot(axis(mid) - mid) > -1e-4, "a face of the bore looks out at {mid}");
        // A camera straight above sees the faces whose front looks up.
        if n.y > 1e-3 {
            up += 1;
        } else if n.y < -1e-3 {
            down += 1;
        }
    }
    // The vault looks down into the tube: from above it is culled.
    assert!(down > 0 && up == 0, "vault faces up {up}, down {down}");
    // The profile meets the walls the portal arch is sized to.
    let p = bore_profile(hw);
    assert_eq!(p.first().unwrap().0, hw + TUNNEL_WALL);
    assert_eq!(p.last().unwrap().0, -(hw + TUNNEL_WALL));
    assert!(p.iter().map(|q| q.1).fold(0.0, f32::max) > 5.0);
}

use super::*;
use nd_sim::roads::RoadNetwork;

/// The street network the building sweep uses: straight, diagonal and
/// curved roads crossing, so there are bends, junctions and signals.
fn network() -> (RoadNetwork, LaneNetwork) {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-400.0, -40.0), Vec2::new(400.0, -40.0)], 2, 2);
    roads.add_polyline(&[Vec2::new(-120.0, -300.0), Vec2::new(60.0, 300.0)], 1, 1);
    roads.add_polyline(&[Vec2::new(-400.0, 200.0), Vec2::new(400.0, 200.0)], 1, 1);
    roads.add_polyline(&[Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0)], 2, 2);
    let curve: Vec<Vec2> = (0..=20).map(|i| {
        let t = i as f32 / 20.0;
        Vec2::new(-300.0 + 600.0 * t, 80.0 + 60.0 * (t * std::f32::consts::TAU).sin())
    }).collect();
    roads.add_polyline(&curve, 1, 1);
    roads.add_polyline_kind(&[Vec2::new(-350.0, 150.0), Vec2::new(350.0, 150.0)], 1, 1, RoadKind::Rail);
    roads.add_polyline_kind(&[Vec2::new(-200.0, -40.0), Vec2::new(-200.0, 200.0)], 1, 1, RoadKind::Path);
    let mut net = LaneNetwork::default();
    net.rebuild(&roads);
    (roads, net)
}

#[test]
fn one_way_roads_show_their_direction_and_ramps_get_a_gore() {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 3, 0);
    roads.add_polyline(&[Vec2::new(-100.0, -150.0), Vec2::new(-50.0, -12.0), Vec2::new(0.0, 0.0)], 1, 0);
    let mut net = LaneNetwork::default();
    net.rebuild(&roads);
    let lv = LandValue::new(400.0, 50.0);
    let pal = Palette::default();
    let g = generate_street(&StreetInput { roads: &roads, net: &net, land_value: &lv, palette: &pal, seed: 3, heights: &[], inner: &[], layer: StreetLayer::All, closed: &[] });
    for (_, m) in g.iter() {
        assert!(m.positions.iter().all(|p| p.iter().all(|v| v.is_finite())));
    }
    // Arrows in the highway's lanes: glow strips pointing +x somewhere
    // mid-carriageway, far from the join.
    let glow = g.get(Paint);
    assert!(glow.positions.iter().any(|p| p[0] > 100.0 && p[0] < 300.0 && p[2].abs() < 5.0 && (p[1] - MARK_Y - 0.003).abs() < 1e-3), "arrows on the highway");
    // A gore: hatching between the ramp and the carriageway near the join.
    assert!(glow.positions.iter().any(|p| p[0] < -2.0 && p[0] > -45.0 && p[2] < -5.0 && (p[1] - MARK_Y - 0.004).abs() < 1e-3), "chevrons in the gore");
    // No round junction pad at the join.
    let pad_y = ASPHALT_Y + 0.01;
    assert!(!g.get(Asphalt).positions.iter().any(|p| (p[1] - pad_y).abs() < 1e-4 && Vec2::new(p[0], p[2]).length() < 15.0), "no pad over the merge");
}

#[test]
fn motorway_barriers_never_stand_on_another_motorways_lanes() {
    let mut roads = RoadNetwork::default();
    roads.add_polyline(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 3, 0);
    // A slip road peeling off at a shallow angle, as the demo's do.
    roads.add_polyline(&[Vec2::new(0.0, 0.0), Vec2::new(-150.0, -30.0), Vec2::new(-200.0, -120.0)], 0, 1);
    let mut net = LaneNetwork::default();
    net.rebuild(&roads);
    let lv = LandValue::new(400.0, 50.0);
    let pal = Palette::default();
    let g = generate_street(&StreetInput { roads: &roads, net: &net, land_value: &lv, palette: &pal, seed: 3, heights: &[], inner: &[], layer: StreetLayer::All, closed: &[] });
    let lanes = |p: Vec2| {
        roads.alive_segments().any(|(sid, s)| {
            let (a, b) = roads.segment_points(sid);
            nd_sim::roads::project_on_segment(p, a, b).1.distance(p) < s.lanes() as f32 * LANE_WIDTH * 0.5 - 0.3
        })
    };
    // Barrier tops (0.8 m up) never stand over any lane.
    // (Cheap streets move their concrete to the worn material.)
    let tops: Vec<[f32; 3]> = [Concrete, WornConcrete].iter().flat_map(|s| g.get(*s).positions.iter().copied()).filter(|p| (p[1] - 0.8).abs() < 1e-3).collect();
    assert!(tops.len() > 20, "the barriers are drawn");
    let bad: Vec<_> = tops.iter().filter(|p| lanes(Vec2::new(p[0], p[2]))).collect();
    assert!(bad.is_empty(), "{} barrier vertices on the lanes, e.g. {:?}", bad.len(), bad.first());
}

#[test]
fn streets_are_finite_low_and_deterministic() {
    let (roads, net) = network();
    let lv = LandValue::new(400.0, 50.0);
    let pal = Palette::default();
    let inp = StreetInput { roads: &roads, net: &net, land_value: &lv, palette: &pal, seed: 5, heights: &[], inner: &[], layer: StreetLayer::All, closed: &[] };
    let a = generate_street(&inp);
    assert!(a.triangle_count() > 1000, "{} triangles is not a street network", a.triangle_count());
    for (s, m) in a.iter() {
        assert_eq!(m.positions.len(), m.uvs.len(), "{s:?} uv count");
        assert_eq!(m.positions.len(), m.tangents.len(), "{s:?} tangent count");
        for p in &m.positions {
            assert!(p.iter().all(|v| v.is_finite()), "{s:?} has NaN");
            assert!(p[1] >= -0.01 && p[1] < 12.0, "{s:?} at height {}", p[1]);
        }
        assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()), "{s:?} bad index");
    }
    let b = generate_street(&inp);
    assert!(a == b, "the same network drew two different streets");
    // Land value zero everywhere: the runs are drawn worn. Well-off: clean.
    assert!(!a.get(WornAsphalt).is_empty() && !a.get(WornPaving).is_empty(), "cheap streets use the worn materials");
    let mut rich = LandValue::new(400.0, 50.0);
    for v in &mut rich.values {
        *v = 0.9;
    }
    let r = generate_street(&StreetInput { land_value: &rich, ..inp });
    assert!(r.get(WornAsphalt).is_empty() && r.get(WornPaving).is_empty(), "smart streets stay clean");
    let posts = signal_posts(&roads, &net);
    assert!(!posts.is_empty(), "the four-way junctions should be signalised");
    assert!(posts.windows(2).all(|w| w[0].lane < w[1].lane), "signal posts come in lane order");
    assert!(posts.iter().all(|p| p.lamp.is_finite()));
}

#[test]
fn a_roundabout_leaves_its_middle_open_and_has_no_signals() {
    use nd_sim::plan::{Gesture, GestureId, Plan};
    let mut plan = Plan::default();
    plan.gestures.insert(GestureId(1), Gesture::Road { points: vec![Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
    plan.gestures.insert(GestureId(2), Gesture::Road { points: vec![Vec2::new(0.0, -200.0), Vec2::new(0.0, 200.0)], lanes_fwd: 2, lanes_bwd: 2, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
    let lv = LandValue::new(400.0, 50.0);
    let pal = Palette::default();
    let mut tris = Vec::new();
    for g in [None, Some(Gesture::Roundabout { center: Vec2::ZERO, radius: 30.0, lanes: 2, level: 0.0 })] {
        let mut plan = plan.clone();
        if let Some(g) = g {
            plan.gestures.insert(GestureId(3), g);
        }
        let (built, rings) = plan.built_roads();
        let mut roads = RoadNetwork::default();
        roads.rings = rings;
        for (_, pts, f, b, k) in built {
            roads.add_polyline_kind(&pts, f, b, k);
        }
        let mut net = LaneNetwork::default();
        net.rebuild(&roads);
        let geo = generate_street(&StreetInput { roads: &roads, net: &net, land_value: &lv, palette: &pal, seed: 9, heights: &[], inner: &[], layer: StreetLayer::All, closed: &[] });
        for (s, m) in geo.iter() {
            assert!(m.positions.iter().all(|p| p.iter().all(|v| v.is_finite()) && p[1] < 20.0), "{s:?}");
        }
        tris.push((geo.get(Foliage).indices.len(), geo.get(Stone).indices.len(), signal_posts(&roads, &net).len()));
    }
    let (plain, ring) = (tris[0], tris[1]);
    assert!(plain.2 > 0 && ring.2 == 0, "the signals went with the junction: {plain:?} {ring:?}");
    assert_eq!(ring.1, plain.1, "no sculpture: the middle is open land");
}
