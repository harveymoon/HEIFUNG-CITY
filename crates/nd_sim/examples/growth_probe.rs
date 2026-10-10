//! [pt-growth] Probe: a city's growth day by day: population, homes,
//! homelessness, the land value spread over built lots, tiers, rents,
//! densification (buyouts, lot assemblies) and the floors histogram.
//!
//! cargo run --profile fast -p nd_sim --example growth_probe -- <save.ron|demo> [days] [out.ron]

use nd_minds::RuleBasedMind;
use nd_sim::{LandUse, World, WorldConfig};

fn quant(v: &mut [f32], f: f32) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    v[((v.len() as f32 - 1.0) * f) as usize]
}

fn report(w: &World, t0: std::time::Instant) {
    let mut lv: Vec<f32> = Vec::new();
    for b in w.buildings.values() {
        if b.land_use == LandUse::Park {
            continue;
        }
        if w.lots.contains_key(b.lot) {
            lv.push(w.lot_value(b.lot));
        }
    }
    let mut raw: Vec<f32> = w.buildings.values().filter(|b| b.land_use != LandUse::Park).filter_map(|b| w.lots.get(b.lot)).map(|l| w.land_value.raw_at(l.center)).collect();
    let n = lv.len().max(1) as f32;
    let top = lv.iter().filter(|v| **v >= 0.95).count() as f32 / n * 100.0;
    let hi = lv.iter().filter(|v| **v >= 0.66).count() as f32 / n * 100.0;
    let (p10, p50, p90, max) = (quant(&mut lv, 0.1), quant(&mut lv, 0.5), quant(&mut lv, 0.9), quant(&mut lv, 1.0));
    let mut floors = [0u32; 8];
    let mut tallest = 0u8;
    for b in w.buildings.values().filter(|b| b.construction.is_none() && b.land_use != LandUse::Park) {
        floors[match b.floors {
            0..=1 => 0,
            2 => 1,
            3..=4 => 2,
            5..=8 => 3,
            9..=15 => 4,
            16..=30 => 5,
            31..=60 => 6,
            _ => 7,
        }] += 1;
        tallest = tallest.max(b.floors);
    }
    let (r50, r90, r99, rmax) = (quant(&mut raw, 0.5), quant(&mut raw, 0.9), quant(&mut raw, 0.99), quant(&mut raw, 1.0));
    let e = &w.economy;
    let s = &w.estate.stats;
    let mut dens: Vec<f32> = w.lots.values().filter(|l| l.building.is_some()).map(|l| w.land_density_at(l.center)).collect();
    let (d50, d90, dmax) = (quant(&mut dens, 0.5), quant(&mut dens, 0.9), quant(&mut dens, 1.0));
    let mut foot: Vec<f32> = w.lots.values().filter(|l| l.building.is_some()).map(|l| w.estate.pressure.foot_at(l.center)).collect();
    let (f50, f90, fmax) = (quant(&mut foot, 0.5), quant(&mut foot, 0.9), quant(&mut foot, 1.0));
    println!(
        "day {:>3}: pop {:>6} unemp {:.1}% jobs {} free {} homeless {:>4} unhoused {:>4} | homes {:>6} free {:>5} | priced_out {:>6} waiting {:>3} | lv p10 {:.2} p50 {:.2} p90 {:.2} max {:.2} >=.66 {:.0}% >=.95 {:.1}% raw p50 {:.2} p90 {:.2} p99 {:.2} max {:.2} | tiers {:?} rent {:.0}/{:.0}/{:.0} | dens p50 {:.1} p90 {:.1} max {:.1} foot p50 {:.1} p90 {:.1} max {:.1} | buyouts {} assemblies {} ({} plots) redens {} foreclosed {} merges {} | floors 1/2/3-4/5-8/9-15/16-30/31-60/61+ {:?} tallest {} | sites {} | {:.0}s",
        w.clock.day(),
        w.citizens.len(),
        100.0 * w.citizens.values().filter(|c| c.can_work() && c.work.is_none()).count() as f32 / w.citizens.values().filter(|c| c.can_work()).count().max(1) as f32,
        e.jobs_total,
        e.jobs_free,
        w.homeless_people(),
        e.unhoused,
        e.homes_total,
        e.homes_free,
        e.priced_out_total,
        e.waiting_immigrants,
        p10,
        p50,
        p90,
        max,
        hi,
        top,
        r50,
        r90,
        r99,
        rmax,
        e.tier_counts,
        e.avg_rent[0],
        e.avg_rent[1],
        e.avg_rent[2],
        d50,
        d90,
        dmax,
        f50,
        f90,
        fmax,
        s.buyouts_total,
        s.assemblies_total,
        s.assembled_lots_total,
        s.redensified_total,
        s.foreclosures_total,
        s.merges_total,
        floors,
        tallest,
        w.buildings.values().filter(|b| b.construction.is_some()).count(),
        t0.elapsed().as_secs_f32(),
    );
    if std::env::var("ND_JOBS").is_ok() {
        let mut by: std::collections::BTreeMap<String, (u32, u32, u32)> = Default::default();
        for b in w.buildings.values() {
            let state = if b.demolition.is_some() { "demo" } else if b.construction.is_some() { "site" } else { "open" };
            let e = by.entry(format!("{:?}/{state}", b.land_use)).or_default();
            e.0 += 1;
            e.1 += b.jobs as u32;
            e.2 += b.workers.len() as u32;
        }
        println!("    jobs by use/state (n, jobs, workers): {by:?}");
        let sites: Vec<&nd_sim::Building> = w.buildings.values().filter(|b| b.construction.is_some() && b.demolition.is_none()).collect();
        let waiting = sites.iter().filter(|b| b.construction.as_ref().unwrap().waiting_for_materials()).count();
        let ordered = sites.iter().filter(|b| b.construction.as_ref().unwrap().ordered).count();
        let age: Vec<u64> = sites.iter().map(|b| (w.clock.tick - b.built_tick) / 1440).collect();
        let prog: Vec<String> = sites.iter().take(12).map(|b| { let c = b.construction.as_ref().unwrap(); format!("{:.2}/{}d{}/{}", c.progress, c.days as u32, c.deliveries, (w.clock.tick - b.built_tick) / 1440) }).collect();
        println!("    sites {} waiting {waiting} ordered {ordered} ages {age:?} | {}", sites.len(), prog.join(" "));
    }
    let mut why: std::collections::BTreeMap<&str, u32> = Default::default();
    for b in w.buildings.values() {
        if let Some(d) = &b.demolition {
            *why.entry(d.reason.key()).or_default() += 1;
        }
    }
    if !why.is_empty() {
        println!("    coming down: {why:?}");
    }
}

fn main() {
    let arg = std::env::args().nth(1).unwrap_or_else(|| "demo".into());
    let days: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(10);
    let out = std::env::args().nth(3);
    let mut w = if arg == "demo" {
        let mut w = World::new(WorldConfig::default());
        w.drain_events();
        nd_sim::scenarios::demo_district(&mut w);
        w
    } else {
        World::from_ron(&std::fs::read_to_string(&arg).expect("read")).expect("load")
    };
    let mut minds = RuleBasedMind::default();
    let t0 = std::time::Instant::now();
    // The grid as loaded (an old save's scale) and after one recompute.
    report(&w, t0);
    w.update_land_value();
    report(&w, t0);
    let start = w.clock.day();
    while w.clock.day() < start + days {
        let d = w.clock.day();
        while w.clock.day() == d {
            w.tick(&mut minds);
            w.drain_events();
        }
        report(&w, t0);
    }
    if let Ok(names) = std::env::var("ND_FIND") {
        for n in names.split(',') {
            for (id, b) in w.buildings.iter().filter(|(_, b)| b.name == n) {
                println!("{id:?} {}: {:?} {:?} floors {} built day {} construction {:?} demolition {:?} form {:?} service {:?} business {:?} height {:.0} m rank {:?}", b.name, b.land_use, b.tier, b.floors, b.built_tick / 1440 + 1, b.construction.as_ref().map(|c| c.progress), b.demolition.as_ref().map(|d| (d.floors, d.tier, d.progress)), b.plot.form, b.service.map(|s| s.kind), b.business.as_ref().map(|z| z.kind), w.building_height(id), w.skyline_rank(id));
            }
        }
    }
    if std::env::var("ND_LOTS").is_ok() {
        // Low buildings: plot sizes and what the land density asks for.
        let mut rows: Vec<(f32, u8, u8, u8)> = Vec::new();
        for b in w.buildings.values() {
            if !matches!(b.land_use, LandUse::Residential | LandUse::Commercial) || b.floors > 3 || b.construction.is_some() {
                continue;
            }
            let Some(l) = w.lots.get(b.lot) else { continue };
            rows.push((l.area(), b.floors, w.pressure_floors(b.lot), w.height_cap(b.lot)));
        }
        let mut areas: Vec<f32> = rows.iter().map(|r| r.0).collect();
        println!("low buildings {}: area p10 {:.0} p50 {:.0} p90 {:.0}; small (<1100) {}; small with pf>=4 {}; with cap>=4 too {}", rows.len(), quant(&mut areas, 0.1), quant(&mut areas, 0.5), quant(&mut areas, 0.9), rows.iter().filter(|r| r.0 < 1100.0).count(), rows.iter().filter(|r| r.0 < 1100.0 && r.2 >= 4).count(), rows.iter().filter(|r| r.0 < 1100.0 && r.2 >= 4 && r.3 >= 4).count());
    }
    if std::env::var("ND_BREAKDOWN").is_ok() {
        print!("{}", w.land_value_breakdown());
    }
    if let Some(out) = out {
        std::fs::write(&out, w.to_ron().expect("save")).expect("write");
        println!("saved {out}");
    }
}
