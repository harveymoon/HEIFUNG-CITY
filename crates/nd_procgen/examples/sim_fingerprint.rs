//! [refactor/sim-world] Behaviour fingerprint of the simulation: runs the
//! demo district and generated cities for a few days and prints a hash of
//! the whole saved world after each day, plus the answers of the read-only
//! queries the refactor touches (height caps, facility footprints, parcels,
//! plant rows, project checks). Two builds that print the same lines
//! behave the same. Timings go to stderr.
//!
//! cargo run --profile fast -p nd_procgen --example sim_fingerprint -- <dump dir> [days]

#[path = "../../nd_sim/tests/common/mod.rs"]
mod common;

use nd_minds::RuleBasedMind;
use nd_procgen::citygen::{generate_world, CityPreset};
use nd_sim::roads::RoadKind;
use nd_sim::{Gesture, Vec2, World};
use std::time::Instant;

fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

fn queries(w: &World) -> String {
    let mut s = String::new();
    for (id, l) in w.lots.iter() {
        s += &format!("{id:?} cap {} under {}\n", w.height_cap(id), w.under_facility(l.center));
    }
    for i in 0..w.parcels.parcels.len() {
        s += &format!("parcel {i} {} {:?}\n", w.parcel_holds_civic(i), w.parcel_refusal(i, None));
    }
    for r in w.plant_rows() {
        s += &format!("{r:?}\n");
    }
    for (id, b) in w.buildings.iter() {
        if b.plant.is_some() {
            s += &format!("bonus {id:?} {}\n", w.plant_group_bonus(b));
        }
    }
    s
}

fn road(points: Vec<Vec2>, raised: bool) -> Gesture {
    let elevation = if raised { (0..points.len()).map(|i| if i == 0 || i + 1 == points.len() { 0.0 } else { 8.0 }).collect() } else { Vec::new() };
    Gesture::Road { points, lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation, car_only: None }
}

/// Project checks of a few test projects, and how long one takes.
fn checks(w: &World, projects: &[Vec<Gesture>]) -> (String, f64) {
    let mut s = String::new();
    let mut total = 0.0;
    let mut n = 0;
    for gs in projects {
        let mut t = w.clone();
        t.start_project();
        for g in gs {
            t.add_gesture(g.clone());
        }
        let t0 = Instant::now();
        let c = t.active_project_check();
        for _ in 0..2 {
            let again = t.active_project_check();
            assert_eq!(format!("{again:?}"), format!("{c:?}"));
        }
        total += t0.elapsed().as_secs_f64() / 3.0;
        n += 1;
        s += &format!("check {c:?}\n");
    }
    (s, total / n.max(1) as f64 * 1000.0)
}

fn run(name: &str, mut w: World, days: u64, dir: &str, projects: &[Vec<Gesture>]) {
    let mut m = RuleBasedMind::default();
    let (q, ms) = checks(&w, projects);
    println!("{name} day0 world {:016x} queries {:016x} checks {:016x}", fnv(&w.to_ron().unwrap()), fnv(&queries(&w)), fnv(&q));
    eprintln!("{name} day0 check {ms:.1} ms");
    for d in 1..=days {
        let t0 = Instant::now();
        for _ in 0..1440 {
            w.tick(&mut m);
        }
        let tick_ms = t0.elapsed().as_secs_f64() * 1000.0 / 1440.0;
        let ron = w.to_ron().unwrap();
        let qs = queries(&w);
        let (q, ms) = checks(&w, projects);
        std::fs::write(format!("{dir}/{name}_d{d}.ron"), &ron).unwrap();
        std::fs::write(format!("{dir}/{name}_d{d}.queries"), format!("{qs}{q}")).unwrap();
        println!("{name} day{d} world {:016x} queries {:016x} checks {:016x} citizens {} buildings {}", fnv(&ron), fnv(&qs), fnv(&q), w.citizens.len(), w.buildings.len());
        eprintln!("{name} day{d} tick {tick_ms:.3} ms check {ms:.1} ms citizens {}", w.citizens.len());
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = args.get(1).cloned().unwrap_or_else(|| ".".into());
    let days: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let only = args.get(3).cloned().unwrap_or_default();
    std::fs::create_dir_all(&dir).unwrap();
    let v = |p: &[(f32, f32)]| p.iter().map(|&(x, y)| Vec2::new(x, y)).collect::<Vec<_>>();
    if only.is_empty() || only == "demo" {
        let demo_projects = vec![
            vec![road(v(&[(-350.0, -200.0), (350.0, -200.0)]), false)],
            vec![road(v(&[(-350.0, 75.0), (0.0, 75.0), (350.0, 75.0)]), true)],
            vec![road(v(&[(-300.0, -300.0), (300.0, 300.0)]), false), road(v(&[(-300.0, 300.0), (300.0, -300.0)]), false)],
        ];
        run("demo", common::demo_world(), days, &dir, &demo_projects);
    }
    for (name, preset, seed) in [("grid", CityPreset::GridPlaza, 7u64), ("coast", CityPreset::CoastalRiver, 3)] {
        if !only.is_empty() && only != name {
            continue;
        }
        let t0 = Instant::now();
        let (w, _, _) = generate_world(preset, seed);
        eprintln!("{name} generated in {:.1} s, {} lots", t0.elapsed().as_secs_f64(), w.lots.len());
        let c = w.lots.values().map(|l| l.center).fold(Vec2::ZERO, |a, b| a + b) / w.lots.len().max(1) as f32;
        let projects = vec![
            vec![road(vec![c + Vec2::new(-400.0, 3.0), c + Vec2::new(400.0, 3.0)], false)],
            vec![road(vec![c + Vec2::new(5.0, -300.0), c + Vec2::new(5.0, 0.0), c + Vec2::new(5.0, 300.0)], true)],
        ];
        run(name, w, days, &dir, &projects);
    }
}
