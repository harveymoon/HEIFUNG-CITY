//! [land] Probe: load a save and run it for some days, printing the
//! population, buyouts and the floors histogram each day.
//!
//! cargo run --profile fast -p nd_sim --example land_run -- saves/city.ron 4

fn main() {
    let path = std::env::args().nth(1).expect("a save path");
    let days: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(4);
    let mut w = nd_sim::World::from_ron(&std::fs::read_to_string(&path).expect("read")).expect("load");
    let mut minds = nd_minds::RuleBasedMind::default();
    // `off`: keep the pressure grid cold, so neither its caps nor the
    // developers' buyouts act (the city as main would run it).
    let off = std::env::args().nth(3).is_some_and(|a| a == "off");
    let start = w.clock.day();
    println!("day {start}: pop {} buildings {}", w.citizens.len(), w.buildings.len());
    let t0 = std::time::Instant::now();
    let mut left = w.economy.emigrated_by;
    while w.clock.day() < start + days {
        let d = w.clock.day();
        while w.clock.day() == d {
            w.tick(&mut minds);
            if off {
                w.estate.pressure.samples = 0;
            }
        }
        let s = &w.estate.stats;
        let sites = w.buildings.values().filter(|b| b.construction.is_some()).count();
        println!(
            "day {}: pop {} households {} | buyouts {} (today {}) merges {} | sites {} | homes {} free {} | unemployed {} | emigrated {} displaced {} | {:.0}s",
            w.clock.day(),
            w.citizens.len(),
            w.households.len(),
            s.buyouts_total,
            s.buyouts_today,
            s.merges_total,
            sites,
            w.economy.homes_total,
            w.economy.homes_free,
            w.economy.unemployed,
            w.economy.emigrated_total,
            w.economy.displaced_total,
            t0.elapsed().as_secs_f32()
        );
        // [review] Why they left today.
        let why: Vec<String> = nd_sim::economy::LeaveReason::ALL
            .iter()
            .map(|r| format!("{} {}", r.key(), w.economy.emigrated_by[r.index()] - left[r.index()]))
            .collect();
        left = w.economy.emigrated_by;
        println!("    left today: {} | power supply {:.0}%", why.join(", "), 100.0 * w.economy.power.served);
    }
}
