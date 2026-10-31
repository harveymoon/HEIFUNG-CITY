//! [r4-leisure-places] Probe (builds on main and on the branch): load a
//! copy of a save, run some days, and report each day: businesses by kind,
//! arrivals by kind and hour (people starting a visit), jobs,
//! unemployment, nightlife tax, sim time per day.
//!
//! cargo run --profile fast -p nd_sim --example eatout_probe -- <save.ron> [days]

use nd_minds::RuleBasedMind;
use nd_sim::{Activity, BusinessKind, LandUse, World};
use std::collections::BTreeMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).expect("a save");
    let days: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    let mut w = World::read_save(src).expect("load");
    let mut minds = RuleBasedMind::default();
    println!("loaded day {} hour {} pop {}", w.clock.day(), w.clock.hour(), w.citizens.len());
    probe_viable(&w);
    let mut prev: std::collections::HashMap<nd_sim::CitizenId, Option<nd_sim::BuildingId>> = Default::default();
    for d in 0..days {
        let t0 = std::time::Instant::now();
        let mut arrivals: BTreeMap<&'static str, [u32; 24]> = BTreeMap::new();
        let mut started = false;
        loop {
            w.tick(&mut minds);
            w.drain_events();
            let hour = w.clock.hour() as usize;
            for (id, c) in w.citizens.iter() {
                let now = match c.activity {
                    Activity::Visiting(b) => Some(b),
                    _ => None,
                };
                let was = prev.insert(id, now).flatten();
                if let Some(b) = now {
                    if was != Some(b) {
                        if let Some(z) = w.buildings.get(b).and_then(|x| x.business.as_ref()) {
                            arrivals.entry(z.kind.label()).or_insert([0; 24])[hour] += 1;
                        }
                    }
                }
            }
            if w.clock.minute_of_day() == 0 && started {
                break;
            }
            started = true;
        }
        let secs = t0.elapsed().as_secs_f64();
        probe_viable(&w);
        let mut open: BTreeMap<&'static str, (u32, usize)> = BTreeMap::new();
        for b in w.buildings.values() {
            if let Some(z) = b.business.as_ref().filter(|z| !z.closed && b.operating()) {
                let e = open.entry(z.kind.label()).or_insert((0, 0));
                e.0 += 1;
                e.1 += b.workers.len();
            }
        }
        let jobs: u32 = w.buildings.values().filter(|b| b.operating()).map(|b| b.jobs as u32).sum();
        let filled: usize = w.buildings.values().map(|b| b.workers.len()).sum();
        let nightlife: f32 = nd_sim::tax::TaxCategory::group(LandUse::Entertainment).iter().map(|c| w.economy.taxes.yesterday_of(*c)).sum();
        println!(
            "DAY {} (+{}) pop {} jobs {} filled {} unemployment {:.1}% nightlife_tax {:.0} tick_s {:.1} treasury {:.0}",
            w.clock.day(),
            d + 1,
            w.citizens.len(),
            jobs,
            filled,
            w.economy.unemployment_share() * 100.0,
            nightlife,
            secs,
            w.economy.treasury
        );
        let interesting = |k: &str| {
            let kind = BusinessKind::ALL.into_iter().find(|x| x.label() == k);
            kind.is_some_and(|x| x.is_outing() || matches!(x, BusinessKind::Grocery | BusinessKind::Bakery | BusinessKind::Cafe | BusinessKind::GadgetShop | BusinessKind::Office | BusinessKind::Bookshop))
        };
        for (k, (n, staff)) in &open {
            if interesting(k) {
                let a = arrivals.get(k).copied().unwrap_or([0; 24]);
                let total: u32 = a.iter().sum();
                println!("  {k:<26} open {n:>4} staff {staff:>5} arrivals {total:>6} by hour {:?}", a);
            }
        }
    }
}

#[allow(unused)]
fn probe_viable(w: &World) {
    let all: Vec<BusinessKind> = BusinessKind::ALL.to_vec();
    println!("viable now (10 jobs): {:?}", w.probe_kinds_viable(&all, Some(10)).iter().map(|k| k.label()).collect::<Vec<_>>());
    println!("pop stat {} sites: {:?}", w.economy.population, w.buildings.values().filter(|b| b.construction.is_some()).filter_map(|b| b.business.as_ref().map(|z| z.kind.label())).collect::<Vec<_>>());
}
