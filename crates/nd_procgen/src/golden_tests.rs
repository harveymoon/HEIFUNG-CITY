//! Golden mesh hashes for behaviour-preserving refactors (phase 2b).
//!
//! `cargo test --profile fast -p nd_procgen golden_hashes -- --ignored`
//! with `ND_GOLDEN_OUT=<file>` writes one line per case: its name, an exact
//! hash (every attribute and index, in order), an order-free hash of the
//! triangles (the same triangles drawn in another order or split across
//! meshes hash the same) and the triangle count. Diff two files to prove a
//! refactor left every mesh byte-identical. Not run by default: the cases
//! cover every style, three cities and every other generator, and the
//! hashes change whenever a look is meant to.

use crate::buildings::sweep_tests::{hq, odd_lots, tower_on};
use crate::buildings::*;
use crate::mesh::MeshData;
use crate::palette::Palette;
use crate::surface::Geometry;
use glam::Vec2;
use nd_sim::{LandUse, Lot, Tier, World};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Mutex;

/// FNV-1a over 64 bits: stable across runs, platforms and Rust versions.
#[derive(Clone, Copy)]
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Fnv(0xcbf2_9ce4_8422_2325)
    }
    fn u32(&mut self, v: u32) {
        for b in v.to_le_bytes() {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    fn f(&mut self, v: f32) {
        self.u32(v.to_bits());
    }
    fn u64(&mut self, v: u64) {
        self.u32(v as u32);
        self.u32((v >> 32) as u32);
    }
    fn bytes(&mut self, s: &[u8]) {
        for &b in s {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
}

fn vertex_hash(m: &MeshData, i: usize) -> u64 {
    let mut h = Fnv::new();
    for v in m.positions[i].iter().chain(m.normals.get(i).into_iter().flatten()).chain(m.colors.get(i).into_iter().flatten()).chain(m.uvs.get(i).into_iter().flatten()).chain(m.tangents.get(i).into_iter().flatten()) {
        h.f(*v);
    }
    h.0
}

/// (exact, order-free, triangles) of one mesh.
fn mesh_hashes(m: &MeshData) -> (u64, u64, usize) {
    let mut h = Fnv::new();
    for (tag, n) in [(1u32, m.positions.len()), (2, m.normals.len()), (3, m.colors.len()), (4, m.uvs.len()), (5, m.tangents.len()), (6, m.indices.len())] {
        h.u32(tag);
        h.u32(n as u32);
    }
    for p in &m.positions {
        p.iter().for_each(|v| h.f(*v));
    }
    for p in &m.normals {
        p.iter().for_each(|v| h.f(*v));
    }
    for p in &m.colors {
        p.iter().for_each(|v| h.f(*v));
    }
    for p in &m.uvs {
        p.iter().for_each(|v| h.f(*v));
    }
    for p in &m.tangents {
        p.iter().for_each(|v| h.f(*v));
    }
    for i in &m.indices {
        h.u32(*i);
    }
    (h.0, soup_sum(m), m.indices.len() / 3)
}

/// Order-free: the wrapping sum of a hash per triangle (its three vertices'
/// attributes, rotated to start at the smallest so a triangle's own vertex
/// order may rotate but not flip).
fn soup_sum(m: &MeshData) -> u64 {
    let vh: Vec<u64> = (0..m.positions.len()).map(|i| vertex_hash(m, i)).collect();
    let mut sum = 0u64;
    for t in m.indices.chunks_exact(3) {
        let v = [vh[t[0] as usize], vh[t[1] as usize], vh[t[2] as usize]];
        let k = (0..3).min_by_key(|&k| v[k]).unwrap();
        let mut h = Fnv::new();
        for j in 0..3 {
            h.u64(v[(k + j) % 3]);
        }
        sum = sum.wrapping_add(h.0.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ (h.0 >> 29));
    }
    sum
}

struct Out(Mutex<BTreeMap<String, String>>);

impl Out {
    fn line(&self, name: String, v: String) {
        let mut m = self.0.lock().unwrap();
        assert!(m.insert(name.clone(), v).is_none(), "golden case {name} twice");
    }
    fn mesh(&self, name: impl Into<String>, m: &MeshData) {
        let (e, s, t) = mesh_hashes(m);
        self.line(name.into(), format!("exact={e:016x} soup={s:016x} tris={t}"));
    }
    /// A geometry: the exact hash over every surface, the order-free sum
    /// over all of them (a triangle on another surface still counts as
    /// moved), and the extras the renderer reads (pads, cranes, lamps).
    fn geo(&self, name: impl Into<String>, g: &Geometry) {
        let name = name.into();
        // ND_GOLDEN_DUMP=<case name>: write that case's meshes out in full,
        // to find what moved when a hash changes.
        if std::env::var("ND_GOLDEN_DUMP").is_ok_and(|d| d == name) {
            let path = format!("{}.dump", std::env::var("ND_GOLDEN_OUT").unwrap_or_default());
            std::fs::write(&path, format!("{:#?}", g.parts)).unwrap();
        }
        let mut h = Fnv::new();
        let mut soup = 0u64;
        let mut tris = 0;
        for (i, m) in g.parts.iter().enumerate() {
            let (e, s, t) = mesh_hashes(m);
            h.u32(i as u32);
            h.u64(e);
            soup = soup.wrapping_add(s.wrapping_mul(2 * i as u64 + 1));
            tris += t;
        }
        let mut x = Fnv::new();
        x.bytes(format!("{:?}{:?}{:?}", g.pads, g.cranes, g.lamps).as_bytes());
        self.line(name, format!("exact={:016x} soup={soup:016x} tris={tris} extras={:016x}", h.0, x.0));
    }
    fn debug(&self, name: impl Into<String>, v: &impl std::fmt::Debug) {
        let mut h = Fnv::new();
        h.bytes(format!("{v:?}").as_bytes());
        self.line(name.into(), format!("debug={:016x}", h.0));
    }
}

fn par<T: Sync>(items: &[T], body: impl Fn(&T) + Sync) {
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 12);
    std::thread::scope(|s| {
        for t in 0..threads {
            let body = &body;
            s.spawn(move || {
                for it in items.iter().skip(t).step_by(threads) {
                    body(it);
                }
            });
        }
    });
}

/// The four building sweeps' cases (`sweep_tests`), hashed.
fn buildings(out: &Out) {
    let rules = BuildingRules::default();
    let pal = Palette::default();
    let lots = odd_lots();
    let styles: Vec<(usize, Style)> = Style::ALL.into_iter().enumerate().collect();
    par(&styles, |&(si, style)| {
        let gen = |what: String, lot: &Lot, lu: LandUse, floors: u8, tier: Tier, cond: f32, seed: u64, rules: &BuildingRules, pal: &Palette, extras: &Extras| {
            let g = generate_styled_extras(lot, lu, floors, tier, cond, seed, style, rules, pal, extras);
            out.geo(what, &g);
        };
        for (li, (name, lot)) in lots.iter().enumerate() {
            for (fi, floors) in [1u8, 2, 6, 12, 40, 250].into_iter().enumerate() {
                let k = si * 31 + li * 7 + fi * 3;
                let tier = Tier::ALL[k % 3];
                let cond = [0.0f32, 0.5, 1.0][(k / 3) % 3];
                let lu = LandUse::ALL[k % LandUse::ALL.len()];
                let look = if k % 4 == 0 { None } else { Some(nd_sim::districts::Look::ALL[k % 8]) };
                let brand = (k % 5 == 0).then(hq);
                let tower = (style == Style::Supertall).then(|| tower_on(lot, floors, k)).flatten();
                let extras = Extras { dynamic_lights: k % 2 == 0, look, brand, tower, ..Extras::default() };
                gen(format!("b1 {style:?} {name} {floors}"), lot, lu, floors, tier, cond, 1000 + k as u64, &rules, &pal, &extras);
            }
        }
        for (name, lot) in [&lots[3], &lots[6]] {
            for tier in Tier::ALL {
                for cond in [0.0f32, 0.5, 1.0] {
                    for dynamic_lights in [false, true] {
                        let lu = LandUse::ALL[si % LandUse::ALL.len()];
                        let extras = Extras { dynamic_lights, tower: (style == Style::Supertall).then(|| tower_on(lot, 80, si)).flatten(), ..Extras::default() };
                        gen(format!("b2 {style:?} {name} {tier:?} {cond} {dynamic_lights}"), lot, lu, 8, tier, cond, 7 + si as u64, &rules, &pal, &extras);
                    }
                }
            }
        }
        // The shore's looks with their water, and a beach's crowd.
        for (li, (name, lot)) in lots.iter().enumerate().filter(|(li, _)| [0usize, 3, 6].contains(li)) {
            let beach = shore::BeachLook::default();
            let look = shore::ShoreLook { seaward: -lot.front_dir.perp(), drop: 1.8, beach: Some(beach) };
            let extras = Extras { shore: Some(look), dynamic_lights: li % 2 == 0, ..Extras::default() };
            gen(format!("b5 {style:?} {name} shore"), lot, LandUse::ALL[si % 6], 6, Tier::ALL[li % 3], 0.6, 50 + si as u64, &rules, &pal, &extras);
        }
    });
    // Limits and emptied palettes (sweep 3).
    let mut empty = Palette::default();
    for list in [&mut empty.glow_colors, &mut empty.car_colors, &mut empty.sign_colors, &mut empty.container_colors, &mut empty.container_marks, &mut empty.tarp_colors, &mut empty.led_colors, &mut empty.concrete, &mut empty.glass, &mut empty.brick, &mut empty.plaster, &mut empty.panel, &mut empty.metal_paint, &mut empty.awning_colors] {
        list.clear();
    }
    let mut ruleset = Vec::new();
    for fh in [2.2f32, 12.0] {
        let mut rules = BuildingRules::default();
        for r in [&mut rules.residential, &mut rules.commercial, &mut rules.industrial, &mut rules.entertainment, &mut rules.civic, &mut rules.park] {
            r.floor_height = fh;
            r.window_size = [fh + 1.0, fh + 1.0];
            r.window_spacing = 0.8;
        }
        rules.validate();
        ruleset.push((format!("fh {fh}"), rules));
    }
    par(&styles, |&(si, style)| {
        for (li, (name, lot)) in lots.iter().enumerate() {
            let lu = LandUse::ALL[(si + li) % LandUse::ALL.len()];
            let tier = Tier::ALL[(si + li) % 3];
            for floors in [2u8, 12] {
                for (rn, rules) in &ruleset {
                    let extras = Extras { tower: (style == Style::Supertall).then(|| tower_on(lot, floors, si)).flatten(), ..Extras::default() };
                    out.geo(format!("b3 {style:?} {name} {floors} {rn}"), &generate_styled_extras(lot, lu, floors, tier, 0.3, 3 + li as u64, style, rules, &pal, &extras));
                }
                let extras = Extras { tower: (style == Style::Supertall).then(|| tower_on(lot, floors, si)).flatten(), ..Extras::default() };
                out.geo(format!("b3 {style:?} {name} {floors} empty"), &generate_styled_extras(lot, lu, floors, tier, 0.3, 3 + li as u64, style, &BuildingRules::default(), &empty, &extras));
            }
        }
    });
    // Plants at every scale, and the pieces the renderer draws apart.
    for (li, (name, lot)) in lots.iter().enumerate() {
        for kind in nd_sim::PowerPlantKind::ALL {
            for scale in [0.3f32, 1.0, 2.4, 3.0] {
                for rotors in [false, true] {
                    let extras = Extras { plant: Some(kind), plant_scale: scale, animated_rotors: rotors, ..Extras::default() };
                    out.geo(format!("p {kind:?} {scale} {name} {rotors}"), &generate_building(lot, LandUse::Industrial, 1, Tier::Mid, 0.6, 9 + li as u64, &rules, &pal, &extras));
                }
            }
        }
        {
            out.debug(format!("p coal_vents {name}"), &[0.3f32, 1.0, 3.0].map(|s| coal_vents(lot, 9, s)));
            out.debug(format!("p wind_turbines {name}"), &wind_turbines(lot, 9 + li as u64, None));
            out.debug(format!("p solar_trackers {name}"), &solar_trackers(lot, None));
            out.debug(format!("p parking bays {name}"), &surface_lot_bays_world(lot));
            for kind in nd_sim::BusinessKind::ALL {
                out.debug(format!("p dirty_vents {kind:?} {name}"), &dirty::dirty_vents(lot, kind));
            }
        }
        for (floors, tier) in [(2u8, Tier::Low), (12, Tier::Mid), (40, Tier::High)] {
            for k in 0..=10 {
                let p = k as f32 / 10.0;
                let lu = LandUse::ALL[(li + k) % LandUse::ALL.len()];
                let style = choose_style(lot, lu, floors, tier, 4, &rules, &Extras::default());
                for (site, extras) in [("site", Extras { construction: Some(p), ..Extras::default() }), ("site cranes", Extras { construction: Some(p), animated_cranes: true, ..Extras::default() }), ("demolition", Extras { demolition: Some(p), ..Extras::default() })] {
                    out.geo(format!("s {site} {p} {name} {floors}"), &generate_styled_extras(lot, lu, floors, tier, 0.5, 4 + li as u64, style, &rules, &pal, &extras));
                }
            }
        }
        // Forced styles: businesses, services, annexes, forms, batteries, ports, depots, metro.
        for (ki, kind) in nd_sim::BusinessKind::ALL.into_iter().enumerate() {
            for tier in Tier::ALL {
                let extras = Extras { business: Some(kind), ..Extras::default() };
                let lu = LandUse::ALL[ki % LandUse::ALL.len()];
                out.geo(format!("f business {kind:?} {tier:?} {name}"), &generate_building(lot, lu, 3, tier, 0.7, 21 + li as u64, &rules, &pal, &extras));
            }
        }
        for kind in nd_sim::services::ServiceKind::ALL {
            let extras = Extras { service: Some(kind), ..Extras::default() };
            out.geo(format!("f service {kind:?} {name}"), &generate_building(lot, LandUse::Civic, 3, Tier::Mid, 0.7, 22 + li as u64, &rules, &pal, &extras));
        }
        for a in nd_sim::corp::Annex::ALL {
            let extras = Extras { annex: Some(a), ..Extras::default() };
            out.geo(format!("f annex {a:?} {name}"), &generate_building(lot, LandUse::Commercial, 4, Tier::High, 0.9, 23 + li as u64, &rules, &pal, &extras));
        }
        for b in nd_sim::BatteryKind::ALL {
            let extras = Extras { battery: Some((b, 0.6, -3.0)), ..Extras::default() };
            out.geo(format!("f battery {b:?} {name}"), &generate_building(lot, LandUse::Industrial, 1, Tier::Mid, 0.7, 24 + li as u64, &rules, &pal, &extras));
        }
        for (k, extras) in [
            Extras { depot: true, ..Extras::default() },
            Extras { metro: true, ..Extras::default() },
            Extras { parking: Some(0.7), ..Extras::default() },
            Extras { port: Some(nd_sim::ports::PortKind::ShippingPort), ..Extras::default() },
            Extras { port: Some(nd_sim::ports::PortKind::FerryTerminal), ..Extras::default() },
            Extras { rooftop_pad: true, ..Extras::default() },
            Extras { powered: 0.2, ..Extras::default() },
        ]
        .iter()
        .enumerate()
        {
            for tier in Tier::ALL {
                out.geo(format!("f extras {k} {tier:?} {name}"), &generate_building(lot, LandUse::Commercial, 14, tier, 0.7, 25 + li as u64, &rules, &pal, extras));
            }
        }
    }
    out.debug("p solar_head", &0);
    out.geo("p solar_head geo", &solar_head());
    for ph in [0.0f32, 0.7, 2.0] {
        out.geo(format!("p wind_rotor {ph}"), &wind_rotor(ph));
    }
    out.debug("p battery_lights", &[(0.0f32, 0.0f32), (0.5, 3.0), (1.0, -5.0), (0.2, 0.0)].map(|(l, f)| battery_lights(l, f)));
    out.debug("p crane_slew", &(0..20u64).map(|k| crane_slew(k * 7, k * 131)).collect::<Vec<_>>());
    // Every per-style table.
    out.debug("t styles", &Style::ALL.map(|s| (s, s.label(), s.rail_kind(), s.pad_capable(), adjacency::traits(s))));
    out.debug("t affinity", &Style::ALL.iter().map(|a| Style::ALL.map(|b| adjacency::affinity(*a, b))).collect::<Vec<_>>());
    out.debug("t industry", &nd_sim::BusinessKind::ALL.iter().map(|k| (Tier::ALL.map(|t| industry_styles(*k, t)), business_look_wins(*k))).collect::<Vec<_>>());
}

/// The test scene of `examples/street_scene.rs` (`scene.py`): crosses, a
/// five-arm junction, a Y, a roundabout, a curve, an overpass, a tunnel, a
/// path, a rail line, a taper, ramps off the highway, an underpass, and a
/// lane-count change at a bend.
fn scene() -> World {
    use nd_sim::roads::RoadKind;
    use nd_sim::{Gesture, WorldConfig};
    fn road(w: &mut World, pts: &[[f32; 3]], f: u8, b: u8, kind: RoadKind, curved: bool) {
        let points: Vec<Vec2> = pts.iter().map(|p| Vec2::new(p[0], p[1])).collect();
        let elevation: Vec<f32> = pts.iter().map(|p| p[2]).collect();
        let elevation = if elevation.iter().all(|e| *e == 0.0) { Vec::new() } else { elevation };
        w.add_gesture(Gesture::Road { points, lanes_fwd: f, lanes_bwd: b, curved, kind, elevation });
    }
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
    // A lane-count change at a 20 degree bend, and a straight one.
    st(&mut w, &[[-700.0, 200.0], [-600.0, 200.0]], 2, 2);
    let b = Vec2::new(-600.0, 200.0) + Vec2::from_angle(20f32.to_radians()) * 100.0;
    st(&mut w, &[[-600.0, 200.0], [b.x, b.y]], 1, 1);
    st(&mut w, &[[-700.0, 300.0], [-600.0, 300.0]], 2, 2);
    st(&mut w, &[[-600.0, 300.0], [-500.0, 300.0]], 1, 1);
    // A diagonal viaduct and a diagonal tunnel.
    road(&mut w, &[[300.0, 0.0, 0.0], [350.0, 50.0, 9.0], [450.0, 150.0, 9.0], [500.0, 200.0, 0.0]], 1, 1, s, false);
    road(&mut w, &[[600.0, 0.0, 0.0], [650.0, 50.0, -9.0], [750.0, 150.0, -9.0], [800.0, 200.0, 0.0]], 1, 1, s, false);
    // Zones so the scene has lots for the walk detail.
    for (x, y) in [(-560.0f32, -520.0f32), (-250.0, -520.0), (380.0, -230.0)] {
        let p = vec![Vec2::new(x, y), Vec2::new(x + 80.0, y), Vec2::new(x + 80.0, y + 40.0), Vec2::new(x, y + 40.0)];
        w.add_gesture(Gesture::Zone { polygon: p, land_use: LandUse::Commercial });
    }
    w.implement();
    w
}

/// Every world-level generator on one world.
fn world_meshes(out: &Out, tag: &str, w: &World, walk_at: &[Vec2], previews: &[Vec<Vec2>]) {
    use crate::street::{generate_street, lane_arrows, ped_signal_posts, signal_posts, StreetInput, StreetLayer};
    let pal = Palette::default();
    let heights: &[f32] = if w.relief { &w.road_z } else { &[] };
    let inner: &[Vec<(f32, f32)>] = if w.relief { &w.road_inner } else { &[] };
    let closed = [Vec2::new(-600.0, -550.0)];
    for (ln, layer) in [("surface", StreetLayer::Surface), ("underground", StreetLayer::Underground), ("all", StreetLayer::All)] {
        for (cn, closed) in [("", &[][..]), (" closed", &closed[..])] {
            let g = generate_street(&StreetInput { roads: &w.roads, net: &w.traffic.net, land_value: &w.land_value, palette: &pal, seed: w.config.seed, heights, inner, layer, closed });
            out.geo(format!("{tag} street {ln}{cn}"), &g);
        }
    }
    out.geo(format!("{tag} lane_arrows"), &lane_arrows(&w.roads, &w.traffic.net, heights, inner, &pal));
    out.debug(format!("{tag} signal_posts"), &signal_posts(&w.roads, &w.traffic.net));
    out.debug(format!("{tag} ped_signal_posts"), &ped_signal_posts(&w.roads, &w.traffic.net));
    out.geo(format!("{tag} waterworks"), &crate::terrain::waterworks(w, &pal));
    let (walls, caps) = crate::terrain::retaining_walls(w, &pal);
    out.mesh(format!("{tag} retaining walls"), &walls);
    out.mesh(format!("{tag} retaining caps"), &caps);
    if !w.landscape.is_flat() {
        out.mesh(format!("{tag} landscape"), &crate::terrain::build_landscape_terrain(w, &pal, 8.0));
        let tiles = crate::terrain::LandscapeTiles::new(w, &pal, 8.0);
        let mut union = MeshData::default();
        for t in 0..tiles.len() {
            union.append(tiles.mesh(t));
        }
        out.mesh(format!("{tag} landscape tiles"), &union);
        out.mesh(format!("{tag} water"), &crate::terrain::build_water(w, &pal));
        out.mesh(format!("{tag} far water"), &crate::terrain::build_far_water(w, &pal));
        out.mesh(format!("{tag} far ground"), &crate::terrain::build_far_ground(nd_sim::landscape::LAND_HALF, 3000.0, 25.0, &|p| w.ground_height(p), &|p| w.landscape.is_water(p), &pal));
    }
    out.geo(format!("{tag} footbridges"), &crate::footbridge::footbridges(w, &pal));
    for (k, c) in walk_at.iter().enumerate() {
        out.geo(format!("{tag} walk {k}"), &crate::walk::walk_detail(w, &pal, *c, crate::walk::WALK_RADIUS));
    }
    for c in w.level_crossings() {
        let rig = crate::crossing::rigs(c);
        let mut g = Geometry::default();
        crate::crossing::furniture(&mut g, &pal, &rig);
        out.geo(format!("{tag} crossing {:?}", c.pos), &g);
        out.debug(format!("{tag} crossing lamps {:?}", c.pos), &rig.iter().map(|r| r.lamps).collect::<Vec<_>>());
    }
    for (k, pts) in previews.iter().enumerate() {
        for (lk, levels) in [vec![0.0; pts.len()], pts.iter().enumerate().map(|(i, _)| if i == 0 || i + 1 == pts.len() { 0.0 } else { 9.0 }).collect(), pts.iter().enumerate().map(|(i, _)| if i == 0 || i + 1 == pts.len() { 0.0 } else { -9.0 }).collect()].into_iter().enumerate() {
            for kind in [nd_sim::roads::RoadKind::Street, nd_sim::roads::RoadKind::Path, nd_sim::roads::RoadKind::Rail] {
                let p = crate::preview::road_preview_with(w, pts, &levels, 2, 1, kind, &pal, &[1], [0.2, 0.8, 1.0], [1.0, 0.2, 0.1], 0.5, None);
                out.mesh(format!("{tag} preview {k} {lk} {kind:?}"), &p.mesh);
                out.debug(format!("{tag} preview {k} {lk} {kind:?} pts"), &(&p.junctions, &p.flyovers, &p.heights));
            }
        }
    }
    // Buildings and vacant lots, as the renderer draws them.
    let rules = BuildingRules::default();
    let mut book = StyleBook::default();
    book.refresh(w, &rules);
    let ids: Vec<_> = w.buildings.keys().collect();
    par(&ids, |&id| {
        let b = &w.buildings[id];
        let Some(fp) = w.footprint(id) else { return };
        let lot: &Lot = &fp;
        let extras = Extras { powered: b.powered, park: lot.park.as_ref(), construction: b.construction.as_ref().map(|c| c.progress), animated_rotors: true, animated_cranes: true, dynamic_lights: true, ..style_extras(w, id, book.neighbours(id)) };
        let g = generate_building(lot, b.land_use, b.floors, b.tier, b.condition, b.seed, &rules, &pal, &extras);
        out.geo(format!("{tag} building {:?} {:?}", id, lot.center), &g);
    });
    out.debug(format!("{tag} stylebook"), &ids.iter().map(|id| book.style(*id)).collect::<Vec<_>>());
    let lots: Vec<_> = w.lots.iter().filter(|(_, l)| l.building.is_none()).map(|(id, l)| (id, l.clone())).collect();
    par(&lots, |(id, lot)| {
        out.geo(format!("{tag} vacant {id:?}"), &crate::vacant::generate_vacant(lot, w.land_value.at(lot.center), 5, &pal));
    });
}

fn misc(out: &Out) {
    let pal = Palette::default();
    let cfg = crate::terrain::TerrainConfig::default();
    out.mesh("terrain build_terrain", &crate::terrain::build_terrain(&cfg, &pal));
    out.geo("terrain scatter", &crate::terrain::scatter_nature(&cfg, &pal, 5, &|p| p.x.abs() > 40.0));
    out.mesh("terrain far ground flat", &crate::terrain::build_far_ground(1200.0, 2000.0, 25.0, &crate::terrain::flat_height(&cfg), &|_| false, &pal));
    for k in [nd_sim::ShipKind::Freighter, nd_sim::ShipKind::Ferry, nd_sim::ShipKind::FishingBoat, nd_sim::ShipKind::Yacht, nd_sim::ShipKind::Sailboat] {
        out.mesh(format!("boat {k:?}"), &crate::terrain::boat_mesh(k, &pal));
        out.debug(format!("boat size {k:?}"), &crate::terrain::boat_size(k));
    }
    for nets in [false, true] {
        out.mesh(format!("trawler {nets}"), &crate::terrain::trawler_mesh(nets, &pal));
    }
    for f in [false, true] {
        out.mesh(format!("ship {f}"), &crate::terrain::ship_mesh(f, &pal));
    }
    for (i, m) in crate::vehicles::catalogue().iter().enumerate() {
        let v = crate::vehicles::vehicle_mesh(m);
        out.mesh(format!("vehicle {i} body"), &v.body);
        out.mesh(format!("vehicle {i} lights"), &v.lights);
        out.mesh(format!("vehicle {i} fx"), &v.fx);
        out.debug(format!("vehicle {i} key"), &crate::vehicles::variant_key(m));
    }
    out.mesh("rotor", &crate::vehicles::rotor_mesh());
    {
        use crate::vehicles::Parked;
        use rand::SeedableRng;
        for (k, kind) in [Parked::Car, Parked::Van, Parked::Truck, Parked::Wreck, Parked::Bus, Parked::GarbageTruck, Parked::Lorry].into_iter().enumerate() {
            let mut g = Geometry::default();
            let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(k as u64);
            crate::vehicles::draw_parked(&mut g, &pal, &mut rng, Vec2::new(10.0, 4.0), 0.2, 0.6, kind, [0.3, 0.5, 0.7]);
            out.geo(format!("parked {k}"), &g);
        }
    }
    for style in 0..7u8 {
        for outfit in 0..6u8 {
            for (bk, build) in [0u8, 1, 2].into_iter().enumerate() {
                let k = crate::people::PersonKey { style, outfit, build, child: (style + outfit) % 5 == 0, skin: (style * 3 + outfit) % 8, hair: (style + bk as u8 * 4) % 14, fav: [outfit, style], wear: bk as u8, ride: outfit == 2, tourist: style == 3 };
                out.mesh(format!("person {style} {outfit} {build}"), &crate::people::person_mesh(&k));
            }
        }
    }
    for skin in 0..4u8 {
        for pose in crate::people::FirePose::ALL {
            out.mesh(format!("firefighter {skin} {pose:?}"), &crate::people::firefighter_mesh(skin, pose));
        }
    }
    for c in 0..8u8 {
        out.geo(format!("metro car {c}"), &crate::metro::metro_car(c));
    }
    {
        let a: Vec<Vec2> = vec![Vec2::new(-300.0, -100.0), Vec2::new(0.0, -100.0), Vec2::new(200.0, 50.0), Vec2::new(400.0, 50.0)];
        let b: Vec<Vec2> = vec![Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0)];
        let lines = [crate::metro::TunnelLine { path: &a, colour: 1 }, crate::metro::TunnelLine { path: &b, colour: 3 }];
        let sites = [crate::metro::PlatformSite { at: Vec2::new(-150.0, -100.0), dir: Vec2::X, entrance: Vec2::new(-150.0, -80.0), colour: 1 }, crate::metro::PlatformSite { at: Vec2::new(0.0, 100.0), dir: Vec2::Y, entrance: Vec2::new(20.0, 100.0), colour: 7 }];
        out.geo("metro tunnels", &crate::metro::tunnels(&lines, &sites, &|p: Vec2| 4.0 + 0.02 * p.x + 3.0 * (p.y * 0.01).sin()));
    }
    for small in [false, true] {
        out.geo(format!("crossing arm {small}"), &crate::crossing::arm_geometry(&pal, 6.5, small));
    }
    for k in 0..6u64 {
        let p = Vec2::new(30.0 * k as f32, -20.0);
        let along = Vec2::from_angle(0.3 * k as f32);
        let outv = along.perp();
        out.geo(format!("camp {k}"), &crate::society::camp(p, along, outv, (k % 5) as u8 + 1, k % 2 == 0, 100 + k, &pal));
        out.geo(format!("rv {k}"), &crate::society::rv(p, 0.4 * k as f32, k % 2 == 1, 200 + k, &pal));
        out.geo(format!("trash {k}"), &crate::society::trash_pile(p, along, outv, k as f32 / 5.0, k % 3 == 0, 300 + k, &pal));
        out.geo(format!("garden {k}"), &crate::social::garden(p, along, 8.0 + 4.0 * k as f32, 400 + k));
        let (a, b) = crate::social::mural_wall(p, along, outv, 10.0 + k as f32, 4.0, k as f32 / 5.0, 500 + k, k, "NEON");
        out.geo(format!("mural wall {k} a"), &a);
        out.geo(format!("mural wall {k} b"), &b);
    }
    {
        let lots = odd_lots();
        let rules = BuildingRules::default();
        for (name, lot) in &lots {
            let g = generate_building(lot, LandUse::Residential, 6, Tier::Mid, 0.5, 31, &rules, &pal, &Extras::default());
            let spot = crate::social::facade_spot(&g, lot.center + lot.front_dir * 10.0, -lot.front_dir);
            out.debug(format!("facade spot {name}"), &spot.as_ref().map(|s| (s.clear_share(), s.at(0.5, 0.5, 0.1))));
            if let Some(s) = spot {
                let (a, b) = crate::social::paint_mural(&s, 0.7, 33, 2, "CREW");
                out.geo(format!("mural {name} a"), &a);
                out.geo(format!("mural {name} b"), &b);
            }
            out.geo(format!("vacant odd {name}"), &crate::vacant::generate_vacant(lot, 0.4, 7, &pal));
            out.debug(format!("dirty vents {name}"), &dirty::dirty_vents(lot, nd_sim::BusinessKind::ALL[0]));
        }
    }
    {
        let toll = |pos: Vec2, plaza: bool, id: u32| crate::toll::TollDraw { pos, dir: Vec2::from_angle(0.3), lanes: 4, half_width: 7.0, plaza, id, room: [12.0, 20.0] };
        out.geo("tolls", &crate::toll::toll_booths(&[toll(Vec2::new(0.0, 0.0), false, 1), toll(Vec2::new(100.0, 40.0), true, 2), toll(Vec2::new(-80.0, 9.0), true, 7)], &pal));
    }
    out.geo("bus stops", &crate::bus_stops(&[(Vec2::new(5.0, 5.0), Vec2::X, Vec2::Y), (Vec2::new(-40.0, 15.0), Vec2::from_angle(0.7), Vec2::from_angle(0.7).perp())], &pal));
    for (k, kind) in [nd_sim::shore::JettyKind::Fishing, nd_sim::shore::JettyKind::Cargo, nd_sim::shore::JettyKind::Pier, nd_sim::shore::JettyKind::Pontoon].into_iter().enumerate() {
        let sea = Vec2::from_angle(0.4 + k as f32);
        let root = Vec2::new(50.0 * k as f32, 10.0);
        let tip = root + sea * kind.size().0;
        let j = nd_sim::shore::Jetty { kind, root, tip, berth: tip - sea * 4.0 + sea.perp() * 3.0, seaward: sea, water: nd_sim::landscape::WaterKind::Sea, built: 0, loads: 0, last_load: 0 };
        for night in [false, true] {
            for boat in [false, true] {
                out.geo(format!("jetty {kind:?} {night} {boat}"), &shore::jetty(&j, 1.2, -0.4, night, 9 + k as u64, boat));
            }
        }
    }
    for (k, scale) in [0.5f32, 1.0, 3.0].into_iter().enumerate() {
        let mut g = Geometry::default();
        crate::dam::hydro_dam(&mut g, Vec2::new(10.0, 20.0), Vec2::from_angle(0.5), 30.0 + 20.0 * k as f32, Vec2::new(30.0, 40.0), scale, &|p| 2.0 + 0.05 * p.x, [0.5, 0.5, 0.5], [0.3, 0.3, 0.35]);
        out.geo(format!("dam {scale}"), &g);
    }
    for c in nd_sim::landscape::Climate::ALL {
        let p = crate::climate::palette_for(&pal, c);
        out.debug(format!("climate palette {c:?}"), &p);
        let mut g = Geometry::default();
        crate::climate::climate_tree(&mut g, &p, Vec2::new(3.0, 4.0), 0.5, 1.2, [0.2, 0.6, 0.2]);
        out.geo(format!("climate tree {c:?}"), &g);
        let mut b = generate_building(&odd_lots()[1].1, LandUse::Residential, 3, Tier::Low, 0.5, 3, &BuildingRules::default(), &p, &Extras::default());
        crate::climate::snow_cover(&mut b, 0.7);
        out.geo(format!("climate snow {c:?}"), &b);
    }
    {
        let mut h = crate::overlay::HeatGrid::covering(Vec2::new(-100.0, -100.0), Vec2::new(100.0, 100.0), 10.0, 10_000);
        for k in 0..30 {
            h.splat(Vec2::new(k as f32 * 6.0 - 90.0, (k as f32 * 1.3).sin() * 80.0), 1.0 + k as f32);
        }
        h.blur();
        out.mesh("heat mesh", &h.mesh(1.0, 0.3, crate::overlay::heat_color, |p| p.x * 0.01));
        out.mesh("heat mesh_by", &h.mesh_by(0.3, |i| [i as f32 * 0.01, 0.5, 0.2, 1.0], |i| i % 7 == 0, |p| p.y * 0.01));
    }
}

#[test]
#[ignore]
fn golden_hashes() {
    let path = std::env::var("ND_GOLDEN_OUT").expect("ND_GOLDEN_OUT=<file>");
    let only = std::env::var("ND_GOLDEN_ONLY").unwrap_or_default();
    let out = Out(Mutex::new(BTreeMap::new()));
    let t0 = std::time::Instant::now();
    if only.is_empty() || only.contains('b') {
        buildings(&out);
        eprintln!("buildings {:.1}s", t0.elapsed().as_secs_f32());
    }
    if only.is_empty() || only.contains('m') {
        misc(&out);
        eprintln!("misc {:.1}s", t0.elapsed().as_secs_f32());
    }
    if only.is_empty() || only.contains('w') {
        let w = scene();
        let walk = [Vec2::new(-600.0, -550.0), Vec2::new(-250.0, -520.0), Vec2::new(500.0, -550.0), Vec2::new(380.0, -230.0)];
        let previews = vec![vec![Vec2::new(-700.0, -480.0), Vec2::new(-500.0, -620.0)], vec![Vec2::new(-200.0, -620.0), Vec2::new(-100.0, -600.0), Vec2::new(0.0, -480.0), Vec2::new(60.0, -400.0)], vec![Vec2::new(420.0, -560.0), Vec2::new(560.0, -540.0)]];
        world_meshes(&out, "scene", &w, &walk, &previews);
        eprintln!("scene {:.1}s", t0.elapsed().as_secs_f32());
        for (preset, seed, days) in [(crate::citygen::CityPreset::CoastalRiver, 1u64, 2u32), (crate::citygen::CityPreset::OldTown, 2, 0)] {
            let (mut w, _, _) = crate::citygen::generate_world(preset, seed);
            let mut minds = nd_minds::RuleBasedMind::default();
            for _ in 0..days * 1440 {
                w.tick(&mut minds);
            }
            let c = w.lots.values().map(|l| l.center).fold(Vec2::ZERO, |a, p| a + p) / w.lots.len().max(1) as f32;
            let walk = [c, c + Vec2::new(300.0, 120.0), c - Vec2::new(250.0, 200.0)];
            let previews = vec![vec![c - Vec2::new(200.0, 30.0), c + Vec2::new(200.0, 30.0)], vec![c + Vec2::new(-50.0, -300.0), c + Vec2::new(20.0, -100.0), c + Vec2::new(120.0, 80.0)]];
            world_meshes(&out, &format!("{preset:?}{seed}"), &w, &walk, &previews);
            eprintln!("{preset:?} {:.1}s ({} buildings)", t0.elapsed().as_secs_f32(), w.buildings.len());
        }
    }
    let m = out.0.into_inner().unwrap();
    let mut s = String::new();
    for (k, v) in &m {
        let _ = writeln!(s, "{k} | {v}");
    }
    std::fs::write(&path, s).unwrap();
    eprintln!("{} cases in {:.1}s -> {path}", m.len(), t0.elapsed().as_secs_f32());
}
