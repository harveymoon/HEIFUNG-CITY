//! [r14-builders] Probe: load a save and run it on day by day, printing
//! what the building trade does: sites finished a day and their mean build
//! time (from the site's start), sites under way and waiting, population,
//! treasury, unemployment and businesses' money; with the builders work in,
//! the builders' own books too (`support/site_probe_extra.rs`). Uses only what main
//! has, so the same file runs on main for a before/after.
//!
//! cargo run --profile fast -p nd_sim --example site_probe -- <save.ron> [days]

use nd_sim::World;
use std::collections::BTreeMap;

include!("support/site_probe_extra.rs");

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).cloned().expect("save path");
    let days: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);
    let t0 = std::time::Instant::now();
    let mut w = World::read_save(&src).expect("load");
    let mut minds = nd_minds::RuleBasedMind::default();
    println!("start: day {} pop {} treasury {:.0} buildings {} load {:.1}s", w.clock.day(), w.citizens.len(), w.economy.treasury, w.buildings.len(), t0.elapsed().as_secs_f64());
    // Each site seen under way: when it started (its `built_tick` while a
    // site is the day the work began).
    let mut started: BTreeMap<nd_sim::BuildingId, u64> = BTreeMap::new();
    let seed_sites = |w: &World, started: &mut BTreeMap<nd_sim::BuildingId, u64>| {
        for (id, b) in w.buildings.iter() {
            if b.construction.is_some() && b.demolition.is_none() {
                started.entry(id).or_insert(b.built_tick);
            }
        }
    };
    seed_sites(&w, &mut started);
    let (mut tot_done, mut tot_days) = (0u32, 0f64);
    for _ in 0..days {
        let t = std::time::Instant::now();
        let (mut done, mut build_days) = (0u32, 0f64);
        for i in 0..1440u32 {
            w.tick(&mut minds);
            if i % 30 == 0 {
                // Sites that opened since the last look.
                let gone: Vec<nd_sim::BuildingId> = started.keys().copied().filter(|id| w.buildings.get(*id).is_none_or(|b| b.construction.is_none())).collect();
                for id in gone {
                    let s = started.remove(&id).unwrap();
                    if w.buildings.get(id).is_some() {
                        done += 1;
                        build_days += w.clock.tick.saturating_sub(s) as f64 / 1440.0;
                    }
                }
                seed_sites(&w, &mut started);
            }
        }
        w.drain_events();
        tot_done += done;
        tot_days += build_days;
        let sites = w.buildings.values().filter(|b| b.construction.is_some() && b.demolition.is_none()).count();
        let waiting = w.buildings.values().filter(|b| b.demolition.is_none() && b.construction.as_ref().is_some_and(|c| c.waiting_for_materials())).count();
        let blocked = w.buildings.values().filter(|b| b.construction.as_ref().is_some_and(|c| c.blocked.is_some())).count();
        let adults = w.citizens.values().filter(|c| c.can_work()).count().max(1);
        let idle = w.citizens.values().filter(|c| c.can_work() && c.work.is_none()).count();
        let biz_money: f32 = w.buildings.values().filter_map(|b| b.business.as_ref()).filter(|z| !z.closed).map(|z| z.money).sum();
        let open = w.buildings.values().filter_map(|b| b.business.as_ref()).filter(|z| !z.closed).count();
        println!(
            "day {}: finished {} mean_build {:.2}d | sites {} waiting {} blocked {} | pop {} unemployment {:.1}% treasury {:.0} | businesses {} money {:.0} | materials_imported {:.0} | {:.0}s",
            w.clock.day(),
            done,
            if done > 0 { build_days / done as f64 } else { 0.0 },
            sites,
            waiting,
            blocked,
            w.citizens.len(),
            idle as f32 / adults as f32 * 100.0,
            w.economy.treasury,
            open,
            biz_money,
            w.economy.materials_today,
            t.elapsed().as_secs_f64()
        );
        let x = extra(&w);
        if !x.is_empty() {
            println!("{x}");
        }
    }
    println!("total: finished {} mean_build {:.2}d over {} days", tot_done, if tot_done > 0 { tot_days / tot_done as f64 } else { 0.0 }, days);
    if let Some(out) = args.get(3) {
        std::fs::write(out, w.to_ron().expect("save")).expect("write");
        println!("saved {out}");
    }
}
