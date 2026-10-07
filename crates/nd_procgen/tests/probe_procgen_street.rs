//! Street generation: layers, elevation and the cost of setting runs on
//! their height profile. These began as the October review's probes
//! (`review/oct`, which asserted the bugs were present: M9, M10, M11 and
//! the "flat world with a node 110 m up" lead); they now assert the fixes.
use glam::Vec2;
use nd_procgen::street::{generate_street, profile_height, RunProfile, StreetInput, StreetLayer};
use nd_procgen::{Geometry, Palette};
use nd_sim::land_value::LandValue;
use nd_sim::roads::{RoadKind, RoadNetwork};
use nd_sim::traffic::LaneNetwork;
use nd_sim::{Gesture, World, WorldConfig};
use std::time::{Duration, Instant};

fn assert_sound(label: &str, g: &Geometry) {
    for (s, m) in g.iter() {
        assert!(m.positions.iter().all(|p| p.iter().all(|v| v.is_finite())), "{label} {s:?}: NaN position");
        assert!(m.normals.iter().all(|p| p.iter().all(|v| v.is_finite())), "{label} {s:?}: NaN normal");
        assert!(m.uvs.iter().all(|p| p.iter().all(|v| v.is_finite())), "{label} {s:?}: NaN uv");
        assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()), "{label} {s:?}: bad index");
        assert!(m.positions.len() == m.tangents.len() && m.positions.len() == m.uvs.len(), "{label} {s:?}: attribute lengths");
    }
}

/// M10: the underground layer draws only tunnels, the surface layer
/// nothing of them: no zebras, beacons, walk lamps or signal posts of
/// surface streets underground, and no zebras over a tunnel on top.
#[test]
fn street_layers_keep_to_their_own_roads() {
    let mut roads = RoadNetwork::default();
    // A long surface street between two four-way junctions.
    roads.add_polyline(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 2, 2);
    roads.add_polyline(&[Vec2::new(-300.0, -200.0), Vec2::new(-300.0, 200.0)], 2, 2);
    roads.add_polyline(&[Vec2::new(300.0, -200.0), Vec2::new(300.0, 200.0)], 2, 2);
    // A separate tunnel far away.
    let t = roads.add_polyline(&[Vec2::new(-400.0, 600.0), Vec2::new(400.0, 600.0)], 1, 1);
    for sid in &t {
        roads.segments[sid.0 as usize].tunnel = true;
    }
    let mut net = LaneNetwork::default();
    net.rebuild(&roads);
    // Nobody crosses mid-block in a bore.
    assert!(nd_sim::walk::midblock_crossings(&roads).iter().all(|(s, _)| !roads.segment(*s).tunnel), "a mid-block crossing in the tunnel");
    assert!(!nd_sim::walk::midblock_crossings(&roads).is_empty(), "the long surface block still gets its crossings");
    let lv = LandValue::new(1000.0, 50.0);
    let pal = Palette::default();
    let heights = vec![0.0; roads.nodes.len()];
    let under = generate_street(&StreetInput { roads: &roads, net: &net, land_value: &lv, palette: &pal, seed: 1, heights: &heights, inner: &[], layer: StreetLayer::Underground });
    assert_sound("underground", &under);
    assert!(under.triangle_count() > 20, "the tunnel is drawn underground");
    let leaked: Vec<(String, [f32; 3])> = under.iter().flat_map(|(s, m)| m.positions.iter().filter(|p| p[2].abs() < 250.0).map(move |p| (format!("{s:?}"), *p))).collect();
    assert!(leaked.is_empty(), "{} underground vertices on the surface streets, e.g. {:?}", leaked.len(), leaked.first());
    let surf = generate_street(&StreetInput { roads: &roads, net: &net, land_value: &lv, palette: &pal, seed: 1, heights: &heights, inner: &[], layer: StreetLayer::Surface });
    assert_sound("surface", &surf);
    let over_tunnel: usize = surf.iter().map(|(_, m)| m.positions.iter().filter(|p| (p[2] - 600.0).abs() < 30.0).count()).sum();
    assert_eq!(over_tunnel, 0, "surface vertices along the tunnel");
    // The two layers together are the whole street, nothing more or less.
    let all = generate_street(&StreetInput { roads: &roads, net: &net, land_value: &lv, palette: &pal, seed: 1, heights: &heights, inner: &[], layer: StreetLayer::All });
    assert_eq!(all.triangle_count(), surf.triangle_count() + under.triangle_count(), "All = Surface + Underground");
}

fn elevated_world() -> World {
    let mut w = World::new(WorldConfig { landscape: nd_sim::landscape::LandscapeParams::flat(), ..WorldConfig::default() });
    w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
    w.add_road(&[Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0)], 1, 1);
    w.add_gesture(Gesture::Road { points: vec![Vec2::new(60.0, -300.0), Vec2::new(60.0, -150.0), Vec2::new(60.0, 150.0), Vec2::new(60.0, 300.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: vec![0.0, 8.0, 8.0, 0.0] });
    w.add_gesture(Gesture::Road { points: vec![Vec2::new(-150.0, -300.0), Vec2::new(-150.0, -150.0), Vec2::new(-150.0, 150.0), Vec2::new(-150.0, 300.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: vec![0.0, -12.0, -12.0, 0.0] });
    w.add_gesture(Gesture::Road { points: vec![Vec2::new(-300.0, 200.0), Vec2::new(300.0, 200.0)], lanes_fwd: 3, lanes_bwd: 0, curved: false, kind: RoadKind::Street, elevation: vec![9.0, 9.0] });
    w.add_gesture(Gesture::Roundabout { center: Vec2::new(0.0, 0.0), radius: 30.0, lanes: 2 });
    w.add_gesture(Gesture::Roundabout { center: Vec2::new(200.0, -200.0), radius: 8.0, lanes: 1 });
    w.implement();
    w
}

/// Elevated, sunken and ring roads draw soundly and the same every time,
/// in every layer.
#[test]
fn elevated_streets_are_sound_and_deterministic() {
    let w = elevated_world();
    assert!(w.roads.alive_segments().any(|(_, s)| s.tunnel) && w.roads.alive_segments().any(|(_, s)| s.viaduct));
    let pal = Palette::default();
    for layer in [StreetLayer::All, StreetLayer::Surface, StreetLayer::Underground] {
        let inp = StreetInput { roads: &w.roads, net: &w.traffic.net, land_value: &w.land_value, palette: &pal, seed: 3, heights: &w.road_z, inner: &w.road_inner, layer };
        let a = generate_street(&inp);
        assert_sound(&format!("{layer:?}"), &a);
        assert!(a == generate_street(&inp), "{layer:?} is not deterministic");
    }
}

/// The October lead: a "flat" world had a node at (-5000, -480) with
/// `road_z` 109.9. Checked: not a bug. The Flat preset is the old flat map,
/// a level basin ringed by the outer hills (`OUTER_HILLS`, up to 160 m past
/// `BASIN_RADIUS`), and a way to elsewhere keeps the height of the land
/// where it leaves the map (`rebuild_ground` clamps to `LAND_HALF`), so the
/// highway's far node stands on the hills at the map's edge. In the basin
/// every way is at the level the player drew (here at most 12 m off grade,
/// plus underpass lifts).
#[test]
fn a_flat_world_keeps_its_roads_near_the_ground() {
    let w = elevated_world();
    let basin = nd_sim::landscape::BASIN_RADIUS;
    for (i, z) in w.road_z.iter().enumerate() {
        let p = w.roads.nodes[i].pos;
        if p.length() < basin {
            assert!((-12.5..=20.0).contains(z), "node {i} at {p:?} in the basin stands at {z}");
        } else {
            let edge = p.clamp(Vec2::splat(-nd_sim::landscape::LAND_HALF), Vec2::splat(nd_sim::landscape::LAND_HALF));
            assert!(*z <= w.landscape.height(edge) + 12.5 && *z <= nd_sim::landscape::OUTER_HILLS, "node {i} at {p:?} beyond the basin stands at {z}");
        }
    }
}

/// M11: the walk camera's street detail stays off tunnels (and the highway)
/// and stands on a viaduct's deck, not on the ground beneath it.
#[test]
fn walk_detail_follows_the_road_it_belongs_to() {
    let w = elevated_world();
    let pal = Palette::default();
    let wd = nd_procgen::walk_detail(&w, &pal, Vec2::new(-150.0, 0.0), 90.0);
    assert_sound("walk detail (tunnel)", &wd);
    // The tunnel street (x = -150) from -150 to 150 runs at -12 m; nothing
    // of the walk detail belongs in it (the street crossing over it at
    // z = 0 keeps its own).
    let in_bore: usize = wd.iter().map(|(_, m)| m.positions.iter().filter(|p| (p[0] + 150.0).abs() < 10.0 && p[2].abs() > 30.0 && p[2].abs() < 140.0).count()).sum();
    assert_eq!(in_bore, 0, "walk detail along the tunnel");
    let wd2 = nd_procgen::walk_detail(&w, &pal, Vec2::new(60.0, 0.0), 90.0);
    assert_sound("walk detail (viaduct)", &wd2);
    let vz = w.roads.node_near(Vec2::new(60.0, -150.0), 1.0).map(|n| w.road_z[n.0 as usize]).expect("viaduct node");
    assert!(vz > 7.0, "the viaduct stands {vz} m up");
    let on_via: Vec<f32> = wd2.iter().flat_map(|(_, m)| m.positions.iter().filter(|p| (p[0] - 60.0).abs() < 9.0 && p[2].abs() < 100.0 && p[2].abs() > 30.0).map(|p| p[1])).collect();
    assert!(!on_via.is_empty(), "the viaduct's pavements get detail");
    let lowest = on_via.iter().copied().fold(f32::MAX, f32::min);
    assert!(lowest > vz - 1.0, "walk detail at {lowest} m under a deck at {vz} m");
}

/// Times `f`, best of three (other work on the machine only adds).
fn best_of_three(mut f: impl FnMut()) -> Duration {
    (0..3).map(|_| {
        let t = Instant::now();
        f();
        t.elapsed()
    }).min().unwrap()
}

/// M9: setting runs on their height profile is near-linear. It used to
/// scan every leg of the run per vertex: a 16x16 grid with the 10 km outer
/// road took 101 ms flat and 1.02 s with heights. The grid of legs gives
/// the same heights as the scan (`profile_height`), so the street is
/// unchanged, only faster.
#[test]
fn heights_cost_little_and_change_nothing() {
    let pal = Palette::default();
    let n = 16usize;
    let mut w = World::new(WorldConfig { landscape: nd_sim::landscape::LandscapeParams::flat(), ..WorldConfig::default() });
    for i in 0..=n {
        let c = -(n as f32) * 30.0 + i as f32 * 60.0;
        let e = n as f32 * 30.0;
        w.add_road(&[Vec2::new(-e, c), Vec2::new(e, c)], 1, 1);
        w.add_road(&[Vec2::new(c, -e), Vec2::new(c, e)], 1, 1);
    }
    w.implement();
    let zeros = vec![0.0f32; w.roads.nodes.len()];
    // Heights that vary node to node, so every run is lifted.
    let hills: Vec<f32> = w.roads.nodes.iter().map(|nd| 1.0 + (nd.pos.x * 0.01).sin() * 3.0 + (nd.pos.y * 0.013).cos() * 2.0).collect();
    let gen = |hs: &[f32]| generate_street(&StreetInput { roads: &w.roads, net: &w.traffic.net, land_value: &w.land_value, palette: &pal, seed: 3, heights: hs, inner: &[], layer: StreetLayer::Surface });
    let flat = best_of_three(|| {
        gen(&zeros);
    });
    let lifted = best_of_three(|| {
        gen(&hills);
    });
    println!("grid {n}: {} segments; flat {flat:?}, with heights {lifted:?}", w.roads.alive_segments().count());
    assert!(lifted < flat * 3 + Duration::from_millis(60), "heights cost {lifted:?} against {flat:?} flat");

    // The indexed profile answers exactly as the scan does, on and off the
    // run (a polyline of many legs, as the highway's).
    let pts: Vec<Vec2> = (0..=90).map(|k| Vec2::new(-5400.0 + k as f32 * 120.0, (k as f32 * 0.3).sin() * 40.0)).collect();
    let z: Vec<f32> = (0..=90).map(|k| (k as f32 * 0.21).cos() * 6.0).collect();
    let prof = RunProfile::new(&pts, &z, 40.0);
    let mut k = 0u32;
    for i in 0..4000 {
        let x = -5600.0 + (i as f32) * 2.8;
        for off in [-60.0f32, -25.0, -3.0, 0.0, 7.5, 33.0, 150.0] {
            let p = Vec2::new(x, (x * 0.0025).sin() * 40.0 + off);
            assert_eq!(prof.height(p).to_bits(), profile_height(&pts, &z, p).to_bits(), "at {p:?}");
            k += 1;
        }
    }
    // And it is quicker per query by a wide margin on a long run.
    let probes: Vec<Vec2> = (0..20000).map(|i| Vec2::new(-5400.0 + i as f32 * 0.54, ((i % 17) as f32 - 8.0) * 2.0)).collect();
    let scan = best_of_three(|| {
        let s: f32 = probes.iter().map(|&p| profile_height(&pts, &z, p)).sum();
        std::hint::black_box(s);
    });
    let grid = best_of_three(|| {
        let s: f32 = probes.iter().map(|&p| prof.height(p)).sum();
        std::hint::black_box(s);
    });
    println!("{k} equal answers; 20000 lifts on a 90-leg run: scan {scan:?}, grid {grid:?}");
    assert!(grid * 2 < scan, "grid {grid:?} vs scan {scan:?}");
}
