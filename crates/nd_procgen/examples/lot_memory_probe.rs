//! [r11-lot-memory] What the lot history (`nd_sim::lot_memory`) costs and
//! draws on a save: its size in the save, the walls that carry ghosts (and
//! the time to find and paint them), the painted-over signs each changed
//! building gets, and the slabs on vacant lots.
//!
//! cargo run --profile fast -p nd_procgen --example lot_memory_probe -- saves/city.ron [days]
//!
//! With `days`, the city runs that many days first (shops change hands on
//! their own) and the history is reported again.

use nd_procgen::buildings::{generate_building, style_extras, BuildingRules, Extras};
use nd_procgen::lot_memory::{ghost_signs, ghost_traces, slab, slab_trace, wall_ghosts};
use nd_procgen::surface::{Geometry, Surface};
use nd_procgen::Palette;
use std::time::Instant;

fn report(w: &nd_sim::World) {
    let (rules, pal) = (BuildingRules::default(), Palette::default());
    let m = &w.lot_memory;
    let ron = ron::to_string(m).unwrap();
    println!("traces {} ({} demolished, {} changed), {} bytes in the save ({:.0} a trace)", m.len(), m.traces().iter().filter(|t| t.kind == nd_sim::lot_memory::TraceKind::Demolished).count(), m.traces().iter().filter(|t| t.kind == nd_sim::lot_memory::TraceKind::Changed).count(), ron.len(), ron.len() as f32 / m.len().max(1) as f32);
    // Ghosts on walls: every standing building with a trace round it.
    let t0 = Instant::now();
    let (mut walls, mut tris, mut looked) = (0, 0, 0);
    let mut gen_ms = 0.0;
    for (id, b) in w.buildings.iter() {
        let traces = ghost_traces(w, id);
        if traces.is_empty() {
            continue;
        }
        looked += 1;
        let Some(lot) = w.footprint(id) else { continue };
        let tg = Instant::now();
        let g = generate_building(&lot, b.land_use, b.floors, b.tier, b.condition, b.seed, &rules, &pal, &style_extras(w, id, &[]));
        gen_ms += tg.elapsed().as_secs_f64() * 1000.0;
        let gh = wall_ghosts(&g, &traces, w.clock.tick);
        if gh.triangle_count() > 0 {
            walls += 1;
            tris += gh.triangle_count();
            println!("  ghost on {} ({:?}) at {:?}: {} triangles", b.name, id, w.lots[b.lot].center, gh.triangle_count());
        }
    }
    let all_ms = t0.elapsed().as_secs_f64() * 1000.0;
    println!("ghost walls: {walls} of {looked} buildings next to a trace, {tris} triangles; ghost finding and painting {:.1} ms in all (besides {:.1} ms generating those buildings)", all_ms - gen_ms, gen_ms);
    // Painted-over signs.
    for (id, b) in w.buildings.iter() {
        let signs = ghost_signs(w, id);
        if signs.is_empty() {
            continue;
        }
        let Some(lot) = w.footprint(id) else { continue };
        let ex = style_extras(w, id, &[]);
        let plain = generate_building(&lot, b.land_use, b.floors, b.tier, b.condition, b.seed, &rules, &pal, &ex);
        let with = generate_building(&lot, b.land_use, b.floors, b.tier, b.condition, b.seed, &rules, &pal, &Extras { ghost_signs: &signs, ..ex });
        let style = nd_procgen::buildings::building_style(w, id, &rules, None).map_or("?", |s| s.label());
        println!("  sign {:?} on {} ({style}) at {:?}: {} more triangles", signs.iter().map(|s| s.text.as_str()).collect::<Vec<_>>(), b.name, w.lots[b.lot].center, with.triangle_count() as i64 - plain.triangle_count() as i64);
    }
    // Slabs.
    let mut slabs = 0;
    for l in w.lots.values().filter(|l| l.building.is_none()) {
        if let Some((t, s)) = slab_trace(w, l) {
            let mut g = Geometry::default();
            slab(&mut g, &l.polygon, t, s, &pal);
            slabs += 1;
            println!("  slab of {} at {:?}, stage {s}: {} triangles ({} slab)", t.name, l.center, g.triangle_count(), g.get(Surface::WornConcrete).indices.len() / 3);
        }
    }
    println!("slabs: {slabs}");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("a save");
    let t = Instant::now();
    let mut w = nd_sim::World::read_save(path).expect("load");
    println!("loaded {path} in {:.1} s: day {}", t.elapsed().as_secs_f32(), w.clock.day());
    report(&w);
    if let Some(days) = args.get(2).and_then(|d| d.parse::<u64>().ok()) {
        let mut minds = nd_sim::mind::IdleMind;
        let t = Instant::now();
        for _ in 0..days * 1440 {
            w.tick(&mut minds);
        }
        println!("ran {days} days in {:.1} s", t.elapsed().as_secs_f32());
        report(&w);
    }
}
