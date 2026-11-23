//! [r14-materials] Probe: load a save and run it on day by day, printing
//! what building sites do: how many open each day, how long they took
//! (from first seen as a site), sites waiting for materials, site lorries
//! on the road, the money spent on imported materials, the treasury,
//! imports and trucks. Uses only APIs main has too, so a build of main and
//! a build of the branch can be diffed.
//!
//! cargo run --profile fast -p nd_sim --example materials_probe -- saves/city.ron 5

use nd_sim::World;
use std::collections::HashMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).cloned().expect("save path");
    let days: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);
    let mut w = World::read_save(&src).expect("load");
    let mut minds = nd_minds::RuleBasedMind::default();
    let t0 = w.clock.tick;
    println!("start: day {} pop {} treasury {:.0} buildings {} sites {}", w.clock.day(), w.citizens.len(), w.economy.treasury, w.buildings.len(), w.buildings.values().filter(|b| b.construction.is_some()).count());
    // When each site was first seen (sites already standing count from the start).
    let mut seen: HashMap<nd_sim::BuildingId, u64> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| (id, t0)).collect();
    let mut fresh_done: Vec<f32> = Vec::new();
    let (mut tot_done, mut tot_mat, mut tot_imp) = (0u32, 0f64, 0f64);
    for _ in 0..days {
        let (mut done, mut site_lorries_max, mut lorries_sum, mut samples, mut waiting_sum) = (0u32, 0usize, 0usize, 0usize, 0usize);
        let (mut materials, mut imports, mut exports, mut deals) = (0f32, 0f32, 0f32, 0u32);
        let mut durations: Vec<f32> = Vec::new();
        for i in 0..1440 {
            let before: Vec<nd_sim::BuildingId> = if i % 10 == 0 { seen.keys().copied().collect() } else { Vec::new() };
            w.tick(&mut minds);
            let now = w.clock.tick;
            if i % 10 == 0 {
                for (id, b) in w.buildings.iter() {
                    if b.construction.is_some() {
                        seen.entry(id).or_insert(now);
                    }
                }
                for id in before {
                    let open = w.buildings.get(id).is_some_and(|b| b.construction.is_none());
                    let gone = !w.buildings.contains_key(id);
                    if open || gone {
                        let since = seen.remove(&id).unwrap_or(now);
                        if open {
                            done += 1;
                            let d = (now - since) as f32 / 1440.0;
                            durations.push(d);
                            if since > t0 {
                                fresh_done.push(d);
                            }
                        }
                    }
                }
            }
            if i % 30 == 0 {
                let sites: Vec<nd_sim::BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
                let n = w.traffic.cars.values().filter(|c| c.cargo.is_some_and(|k| sites.contains(&k.to))).count();
                site_lorries_max = site_lorries_max.max(n);
                lorries_sum += n;
                waiting_sum += w.buildings.values().filter(|b| b.construction.as_ref().is_some_and(|c| c.waiting_for_materials())).count();
                samples += 1;
            }
            if i == 1438 {
                let m = &w.economy.market;
                deals = m.deals_today;
                imports = m.imports_today;
                exports = m.exports_today;
                materials = w.economy.materials_today;
            }
        }
        tot_done += done;
        tot_mat += materials as f64;
        tot_imp += imports as f64;
        let mean = if durations.is_empty() { 0.0 } else { durations.iter().sum::<f32>() / durations.len() as f32 };
        let sites = w.buildings.values().filter(|b| b.construction.is_some()).count();
        let trucks = w.traffic.cars.values().filter(|c| c.cargo.is_some()).count();
        println!(
            "day {}: opened {done} (mean {mean:.1} d on site) | sites {sites} waiting {:.1} | site lorries mean {:.1} max {site_lorries_max} | trucks {trucks} cars {} | materials {materials:.0} imports {imports:.0} exports {exports:.0} deals {deals} | treasury {:.0} pop {}",
            w.clock.day(),
            waiting_sum as f32 / samples.max(1) as f32,
            lorries_sum as f32 / samples.max(1) as f32,
            w.traffic.cars.len(),
            w.economy.treasury,
            w.citizens.len()
        );
    }
    let fresh = if fresh_done.is_empty() { 0.0 } else { fresh_done.iter().sum::<f32>() / fresh_done.len() as f32 };
    println!("total: opened {tot_done} in {days} days ({:.1}/day), sites begun and finished in the run: {} taking {fresh:.2} d on average | materials {tot_mat:.0} imports {tot_imp:.0}", tot_done as f32 / days as f32, fresh_done.len());
}
