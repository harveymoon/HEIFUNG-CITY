//! Road QA: draws the streets (and the waterworks: decks, pillars, portals)
//! of a test scene or a generated city and dumps the triangles round a few
//! spots as JSON, for `tools/road_qa/render.py` to draw top-down.
//!
//! cargo run --profile fast -p nd_procgen --example street_scene -- scene out.json
//! cargo run --profile fast -p nd_procgen --example street_scene -- old_town 3 out.json x,y x,y ...

use glam::Vec2;
use nd_procgen::citygen::{self, CityPreset};
use nd_procgen::palette::Palette;
use nd_procgen::street::{generate_street, StreetInput, StreetLayer};
use nd_procgen::surface::Geometry;
use nd_sim::roads::RoadKind;
use nd_sim::{Gesture, World, WorldConfig};
use std::fmt::Write;

fn road(w: &mut World, pts: &[[f32; 3]], f: u8, b: u8, kind: RoadKind, curved: bool) {
    let points: Vec<Vec2> = pts.iter().map(|p| Vec2::new(p[0], p[1])).collect();
    let elevation: Vec<f32> = pts.iter().map(|p| p[2]).collect();
    let elevation = if elevation.iter().all(|e| *e == 0.0) { Vec::new() } else { elevation };
    w.add_gesture(Gesture::Road { points, lanes_fwd: f, lanes_bwd: b, curved, kind, elevation, car_only: None });
}

/// The test scene the remote API builds in `scene.py`, on the flat map.
pub fn scene() -> (World, Vec<(String, Vec2)>) {
    let mut w = World::new(WorldConfig { seed: 7, landscape: nd_sim::landscape::LandscapeParams { highway: true, ..nd_sim::landscape::LandscapeParams::flat() }, ..WorldConfig::default() });
    w.economy.treasury = 1.0e9;
    w.start_project();
    let s = RoadKind::Street;
    let st = |w: &mut World, pts: &[[f32; 2]], f: u8, b: u8| road(w, &pts.iter().map(|p| [p[0], p[1], 0.0]).collect::<Vec<_>>(), f, b, RoadKind::Street, false);
    st(&mut w, &[[-750.0, -550.0], [-450.0, -550.0]], 2, 2);
    st(&mut w, &[[-600.0, -700.0], [-600.0, -400.0]], 1, 1);
    st(&mut w, &[[-400.0, -550.0], [-200.0, -550.0]], 1, 1);
    st(&mut w, &[[-300.0, -550.0], [-300.0, -700.0]], 1, 1);
    let c = Vec2::new(-50.0, -550.0);
    for k in 0..5 {
        let a = (90.0f32 + 72.0 * k as f32).to_radians();
        let e = c + Vec2::from_angle(a) * 120.0;
        st(&mut w, &[[c.x, c.y], [e.x, e.y]], 1, 1);
    }
    st(&mut w, &[[120.0, -550.0], [350.0, -550.0]], 1, 1);
    let e = Vec2::new(200.0, -550.0) + Vec2::from_angle(25f32.to_radians()) * 130.0;
    st(&mut w, &[[200.0, -550.0], [e.x, e.y]], 1, 1);
    st(&mut w, &[[400.0, -550.0], [600.0, -550.0]], 1, 1);
    st(&mut w, &[[500.0, -650.0], [500.0, -450.0]], 1, 1);
    w.add_gesture(Gesture::Roundabout { center: Vec2::new(500.0, -550.0), radius: 25.0, lanes: 1 });
    road(&mut w, &[[-750.0, -300.0, 0.0], [-650.0, -250.0, 0.0], [-600.0, -150.0, 0.0], [-650.0, -50.0, 0.0]], 1, 1, s, true);
    st(&mut w, &[[-450.0, -300.0], [-450.0, 100.0]], 1, 1);
    road(&mut w, &[[-560.0, -100.0, 0.0], [-500.0, -100.0, 8.0], [-400.0, -100.0, 8.0], [-340.0, -100.0, 0.0]], 1, 1, s, false);
    st(&mut w, &[[-150.0, -250.0], [-150.0, 50.0]], 1, 1);
    road(&mut w, &[[-300.0, -100.0, 0.0], [-230.0, -100.0, -8.0], [-70.0, -100.0, -8.0], [0.0, -100.0, 0.0]], 1, 1, s, false);
    st(&mut w, &[[100.0, -250.0], [100.0, 50.0]], 1, 1);
    road(&mut w, &[[30.0, -150.0, 0.0], [170.0, -150.0, 0.0]], 1, 1, RoadKind::Path, false);
    road(&mut w, &[[50.0, -50.0, 0.0], [150.0, -60.0, 0.0]], 1, 1, RoadKind::Rail, false);
    st(&mut w, &[[250.0, -250.0], [350.0, -250.0]], 2, 2);
    st(&mut w, &[[350.0, -250.0], [480.0, -250.0]], 1, 1);
    st(&mut w, &[[300.0, 411.0], [220.0, 385.0], [150.0, 330.0], [150.0, 250.0]], 1, 0);
    st(&mut w, &[[-50.0, 250.0], [-50.0, 330.0], [-120.0, 385.0], [-200.0, 411.0]], 1, 0);
    st(&mut w, &[[150.0, 250.0], [-50.0, 250.0]], 1, 1);
    st(&mut w, &[[-400.0, 200.0], [-400.0, 600.0]], 1, 1);
    st(&mut w, &[[600.0, -250.0], [800.0, -250.0]], 1, 1);
    st(&mut w, &[[620.0, -250.0], [800.0, -210.0]], 1, 1);
    w.implement();
    let spots = [
        ("s01_cross", -600.0, -550.0),
        ("s02_T", -300.0, -550.0),
        ("s03_5arm", -50.0, -550.0),
        ("s04_Y25", 200.0, -550.0),
        ("s05_roundabout", 500.0, -550.0),
        ("s07_overpass", -450.0, -100.0),
        ("s07b_ramp_foot", -540.0, -100.0),
        ("s08_tunnel_portal", -260.0, -100.0),
        ("s08b_tunnel_cross", -150.0, -100.0),
        ("s09_path", 100.0, -150.0),
        ("s09b_rail", 100.0, -55.0),
        ("s10_taper", 350.0, -250.0),
        ("s11_offramp", 280.0, 405.0),
        ("s11b_onramp", -170.0, 405.0),
        ("s11c_ramp_T", 150.0, 250.0),
        ("s12_underpass", -400.0, 420.0),
        ("s13_shallow", 640.0, -248.0),
    ];
    (w, spots.iter().map(|(n, x, y)| (n.to_string(), Vec2::new(*x, *y))).collect())
}

fn dump(out: &mut String, name: &str, g: &Geometry, at: Vec2, r: f32) {
    let _ = write!(out, "{{\"name\":\"{name}\",\"at\":[{},{}],\"r\":{r},\"tris\":[", at.x, at.y);
    let mut first = true;
    for (s, m) in g.iter() {
        for t in m.indices.chunks(3) {
            let p = [t[0], t[1], t[2]].map(|i| m.positions[i as usize]);
            let q = p.map(|v| Vec2::new(v[0], v[2]));
            let (lo, hi) = (q[0].min(q[1]).min(q[2]), q[0].max(q[1]).max(q[2]));
            if lo.x > at.x + r || lo.y > at.y + r || hi.x < at.x - r || hi.y < at.y - r {
                continue;
            }
            let col = m.colors[t[0] as usize];
            if !first {
                out.push(',');
            }
            first = false;
            let _ = write!(
                out,
                "[\"{s:?}\",{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.2},{:.2},{:.2}]",
                p[0][0], p[0][1], p[0][2], p[1][0], p[1][1], p[1][2], p[2][0], p[2][1], p[2][2], col[0], col[1], col[2]
            );
        }
    }
    out.push_str("]}");
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (w, spots, out_path) = if args.first().map(String::as_str) == Some("scene") {
        let (w, s) = scene();
        (w, s, args.get(1).cloned().unwrap_or_else(|| "scene.json".into()))
    } else {
        let p = CityPreset::parse(&args[0]).expect("preset");
        let seed: u64 = args[1].parse().unwrap();
        let (w, _, _) = citygen::generate_world(p, seed);
        let spots = args[3..]
            .iter()
            .map(|s| {
                let v: Vec<f32> = s.split(',').map(|x| x.parse().unwrap()).collect();
                (format!("{}_{}_{}", args[0], v[0], v[1]), Vec2::new(v[0], v[1]))
            })
            .collect();
        (w, spots, args[2].clone())
    };
    let pal = Palette::default();
    let heights: &[f32] = if w.relief { &w.road_z } else { &[] };
    let inner: &[Vec<(f32, f32)>] = if w.relief { &w.road_inner } else { &[] };
    let mut g = generate_street(&StreetInput { roads: &w.roads, net: &w.traffic.net, land_value: &w.land_value, palette: &pal, seed: w.config.seed, heights, inner, layer: StreetLayer::Surface, closed: &[] });
    let arrows = nd_procgen::street::lane_arrows(&w.roads, &w.traffic.net, heights, inner, &pal);
    for (s, m) in arrows.iter() {
        g.part(s).append(m);
    }
    let works = nd_procgen::terrain::waterworks(&w, &pal);
    for (s, m) in works.iter() {
        g.part(s).append(m);
    }
    let mut out = String::from("[");
    for (k, (name, at)) in spots.iter().enumerate() {
        if k > 0 {
            out.push(',');
        }
        dump(&mut out, name, &g, *at, std::env::var("ND_R").ok().and_then(|r| r.parse().ok()).unwrap_or(45.0));
    }
    out.push(']');
    std::fs::write(&out_path, out).unwrap();
    println!("wrote {out_path}: {} spots", spots.len());
}
