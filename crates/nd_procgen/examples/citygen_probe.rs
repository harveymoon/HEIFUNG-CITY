//! Generate a city plan and dump it as text for a quick look, with timings.
//!
//! cargo run --release -p nd_procgen --example citygen_probe -- <preset> <seed> <out.txt> [lay]
//!
//! Lines: `W`/`I` water outlines and islands, `Z <use>` zone polygons,
//! `R <class>` roads; with `lay`, also `S` built road segments and
//! `L <use>` lots from the world it was laid out in.

use nd_procgen::citygen::{self, CityPreset};
use std::fmt::Write;

fn poly(s: &mut String, tag: &str, pts: &[nd_sim::Vec2]) {
    let _ = write!(s, "{tag}");
    for p in pts {
        let _ = write!(s, " {:.1},{:.1}", p.x, p.y);
    }
    let _ = writeln!(s);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let preset = CityPreset::parse(args.get(1).map_or("grid_plaza", |s| s)).expect("preset");
    let seed: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let out = args.get(3).cloned().unwrap_or_else(|| "city.txt".into());
    let t0 = std::time::Instant::now();
    let land = nd_sim::Landscape::new(&preset.landscape(seed));
    let t1 = std::time::Instant::now();
    let plan = citygen::generate(preset, seed, &land);
    let t2 = std::time::Instant::now();
    println!(
        "{preset:?} seed {seed}: landscape {:?}, plan {:?}: {} roads ({:.0} m arterial, {:.0} m collector, {:.0} m local), {} zones, links {:?}",
        t1 - t0,
        t2 - t1,
        plan.roads.len(),
        plan.road_length(citygen::RoadClass::Arterial),
        plan.road_length(citygen::RoadClass::Collector),
        plan.road_length(citygen::RoadClass::Local),
        plan.zones.len(),
        plan.links
    );
    let mut s = String::new();
    for (outline, holes) in land.water_polygons() {
        poly(&mut s, "W", &outline);
        for h in holes {
            poly(&mut s, "I", &h);
        }
    }
    for r in &land.rivers {
        poly(&mut s, &format!("V {:.0}", r.width), &r.points);
    }
    for z in &plan.zones {
        poly(&mut s, &format!("Z {:?}", z.land_use), &z.polygon);
    }
    for r in &plan.roads {
        poly(&mut s, &format!("R {:?}", r.class), &r.points);
    }
    if args.get(4).is_some_and(|a| a == "lay") {
        let t3 = std::time::Instant::now();
        let (world, _, laid) = citygen::generate_world(preset, seed);
        println!("laid in {:?}: {laid:?}; segments {} lots {} landlocked {}", t3.elapsed(), world.roads.alive_segments().count(), world.lots.len(), world.lots.values().filter(|l| l.landlocked).count());
        for (id, _) in world.roads.alive_segments() {
            let (a, b) = world.roads.segment_points(id);
            poly(&mut s, "S", &[a, b]);
        }
        for l in world.lots.values() {
            poly(&mut s, &format!("L {:?}", l.land_use), &l.polygon);
        }
    }
    std::fs::write(&out, s).unwrap();
}
