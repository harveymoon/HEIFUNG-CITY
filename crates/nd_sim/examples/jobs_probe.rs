//! [r6-perfect-city] Probe: where a city's jobs and homes stand, by land use:
//! lots, built, vacant, sites, homes, jobs, workers, floors; the demand.
//!
//! cargo run --profile fast -p nd_sim --example jobs_probe -- <save.ron>

use nd_sim::{LandUse, World};

fn main() {
    let path = std::env::args().nth(1).expect("save.ron");
    let mut w = World::from_ron(&std::fs::read_to_string(&path).expect("read")).expect("load");
    println!("day {} pop {} demand {:?}", w.clock.day(), w.citizens.len(), w.economy.demand);
    println!("{:<12} {:>5} {:>5} {:>6} {:>5} {:>6} {:>6} {:>6} {:>6}", "use", "lots", "built", "vacant", "sites", "homes", "jobs", "workrs", "floors");
    for lu in LandUse::ALL {
        let lots: Vec<_> = w.lots.values().filter(|l| l.land_use == lu && !w.is_city_ground(l)).collect();
        let vacant = lots.iter().filter(|l| l.building.is_none() && l.usable()).count();
        let bs: Vec<_> = w.buildings.values().filter(|b| b.land_use == lu).collect();
        let sites = bs.iter().filter(|b| b.construction.is_some()).count();
        let homes: u32 = bs.iter().map(|b| b.homes as u32).sum();
        let jobs: u32 = bs.iter().map(|b| b.jobs as u32).sum();
        let workers: usize = bs.iter().map(|b| b.workers.len()).sum();
        let floors: u32 = bs.iter().map(|b| b.floors as u32).sum();
        println!("{:<12} {:>5} {:>5} {:>6} {:>5} {:>6} {:>6} {:>6} {:>6.1}", format!("{lu:?}"), lots.len(), bs.len(), vacant, sites, homes, jobs, workers, floors as f32 / bs.len().max(1) as f32);
    }
    for (id, b) in w.buildings.iter().filter(|(_, b)| b.plot.megablock.is_some()) {
        let r = w.megablock_report(id).unwrap();
        println!("megablock {} ({}): open {} progress {:.2} plots {} homes {} jobs {} started day {}", r.name, r.corp, r.open, r.progress, r.plots, r.homes, r.jobs, b.plot.megablock.unwrap().started_day);
    }
    let sites_jobs: u32 = w.buildings.values().filter(|b| b.construction.is_some() || b.demolition.is_some()).map(|b| b.jobs as u32).sum();
    println!("jobs in buildings being rebuilt (sites and demolitions): {sites_jobs}");
    if std::env::var_os("ND_SITES").is_some() {
        for b in w.buildings.values().filter(|b| b.construction.is_some() && matches!(b.land_use, LandUse::Entertainment | LandUse::Commercial)) {
            let owner = b.plot.owner.and_then(|o| w.estate.owners.get(o)).map(|o| format!("{:?}", o.kind)).unwrap_or_default();
            println!("  site {:?} {} floors {} jobs {} form {:?} mega {} owner {} business {:?}", b.land_use, b.name, b.floors, b.jobs, b.plot.form, b.plot.megablock.is_some(), owner.chars().take(40).collect::<String>(), b.business.as_ref().map(|z| z.kind));
        }
    }
    // An hour on, so the networks' areas are worked out.
    let mut minds = nd_minds::RuleBasedMind::default();
    for _ in 0..60 {
        w.tick(&mut minds);
    }
    let c = &w.connectivity;
    println!("stranded areas {} (lots cut: walk {} road {}) islands {}", c.areas.len(), c.walk_cut.len(), c.road_cut.len(), c.islands.len());
    for a in c.areas.iter().take(8) {
        println!("  {a:?}");
    }
    let adults = w.citizens.values().filter(|c| c.work.is_some()).count();
    println!("employed {adults} unemployed {} working_age {} jobs_total {} jobs_free {}", w.economy.unemployed, w.economy.working_age, w.economy.jobs_total, w.economy.jobs_free);
}
