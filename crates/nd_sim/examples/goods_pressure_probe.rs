//! [r17-goods] Probe: why a new chain's pressure does or does not open its
//! business: loads a save, runs `days`, then prints each new chain's
//! reading, its days held, and how the vacant lots of its land uses fit.
//!
//! cargo run --profile fast -p nd_sim --example goods_pressure_probe -- saves/city.ron 2

use nd_sim::World;
use std::collections::BTreeMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).cloned().expect("save path");
    let days: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    let mut w = World::read_save(&src).expect("load");
    let mut minds = nd_minds::RuleBasedMind::default();
    for _ in 0..days * 1440 {
        w.tick(&mut minds);
    }
    let mut order: Vec<_> = w.pressure.readings.iter().filter(|r| r.wants_more()).map(|r| (r.gap, r.kind.key(), r.held.clone(), w.pressure.held_days.get(r.kind.index()).copied())).collect();
    order.sort_by(|a, b| b.0.total_cmp(&a.0));
    println!("wanting more (gap, key, held, held days):");
    for o in &order {
        println!("  {:.2} {} {:?} {:?}", o.0, o.1, o.2, o.3);
    }
    for r in &w.pressure.readings {
        if !["brewery", "drug_maker", "batching_plant", "saw_mill"].contains(&r.kind.key()) {
            continue;
        }
        let mut fits: BTreeMap<String, u32> = BTreeMap::new();
        for (_, f) in w.lot_need_lots(r.kind) {
            *fits.entry(format!("{f:?}")).or_default() += 1;
        }
        println!("{}: {}; lots {:?}", r.kind.key(), r.why, fits);
    }
}
