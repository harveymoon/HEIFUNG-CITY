//! [r17-goods] Probe: load a save and run it on day by day, printing the
//! economy each midnight: population, treasury, unemployment, hungry
//! people, imports and exports, lorries on the road by body, businesses by
//! kind (new kinds flagged), and the town's stock of each good. Uses only
//! APIs main has too (but for the `[r17-goods]` lines at the end of each
//! day: build main's copy from main's tree), so a build of main and a build
//! of the branch can be diffed.
//!
//! cargo run --profile fast -p nd_sim --example goods_probe -- saves/city.ron 3

use nd_sim::World;
use std::collections::BTreeMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).cloned().expect("save path");
    let days: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let mut w = World::read_save(&src).expect("load");
    let mut minds = nd_minds::RuleBasedMind::default();
    let kinds0 = kinds(&w);
    println!("start: day {} pop {} treasury {:.0} unemployment {:.1}% businesses {}", w.clock.day(), w.citizens.len(), w.economy.treasury, w.economy.unemployment_share() * 100.0, kinds0.values().sum::<u32>());
    for d in 0..days {
        let (mut imports, mut exports) = (0f32, 0f32);
        let mut lorries: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        let mut samples = 0usize;
        let mut cargo_goods: BTreeMap<String, usize> = BTreeMap::new();
        let mut hungry_max = 0u32;
        for i in 0..1440 {
            w.tick(&mut minds);
            if i % 30 == 0 {
                samples += 1;
                let mut now: BTreeMap<String, usize> = BTreeMap::new();
                for c in w.traffic.cars.values() {
                    if let Some(k) = c.cargo {
                        *now.entry(c.model.body.label().to_string()).or_default() += 1;
                        *cargo_goods.entry(k.resource.label().to_string()).or_default() += 1;
                    }
                }
                for (b, n) in now {
                    let e = lorries.entry(b).or_default();
                    e.0 += n;
                    e.1 = e.1.max(n);
                }
                hungry_max = hungry_max.max(w.economy.market.hungry);
            }
            if i == 1438 {
                imports = w.economy.market.imports_today;
                exports = w.economy.market.exports_today;
            }
        }
        let k = kinds(&w);
        println!(
            "day {} (+{}): pop {} treasury {:.0} unemployment {:.1}% hungry(max) {} imports {:.0} exports {:.0} businesses {}",
            w.clock.day(),
            d + 1,
            w.citizens.len(),
            w.economy.treasury,
            w.economy.unemployment_share() * 100.0,
            hungry_max,
            imports,
            exports,
            k.values().sum::<u32>()
        );
        let total: usize = lorries.values().map(|v| v.0).sum();
        println!("  lorries (mean on the road {:.1}): {}", total as f32 / samples.max(1) as f32, lorries.iter().map(|(b, (s, m))| format!("{b} {:.1}/{m}", *s as f32 / samples.max(1) as f32)).collect::<Vec<_>>().join(", "));
        println!("  lorry loads seen by good (half-hour samples): {}", cargo_goods.iter().map(|(g, n)| format!("{g} {n}")).collect::<Vec<_>>().join(", "));
        let changed: Vec<String> = k.iter().filter(|(n, c)| kinds0.get(*n) != Some(c)).map(|(n, c)| format!("{n} {}->{c}", kinds0.get(n).copied().unwrap_or(0))).collect();
        println!("  kinds changed since start: {}", changed.join(", "));
        let stock: Vec<String> = w.economy.market.stock.iter().filter(|(_, s)| **s > 0.5).map(|(r, s)| format!("{} {:.0}", r.label(), s)).collect();
        println!("  stock: {}", stock.join(", "));
        // [r17-goods] The new lines: medicine, nuclear fuel, the chains' pressures.
        let g = &w.goods.yesterday;
        println!("  [r17-goods] medicine used {:.1}, bought in town {:.1}, imported {:.1}, hospitals out {}; convoys sent {} arrived {}; plants {:?}", g.medicine_used, g.medicine_local, g.medicine_imported, g.hospitals_short, g.convoys_sent, g.convoys_arrived, w.goods.nuclear.values().map(|f| (f.fuelled(w.clock.tick), f.loads)).collect::<Vec<_>>());
        let new = ["brewery", "drug_maker", "quarry", "cement_works", "batching_plant", "saw_mill", "textile_mill", "garment_workshop", "fashion_district"];
        println!("  [r17-goods] pressures: {}", w.pressure.readings.iter().filter(|r| new.contains(&r.kind.key())).map(|r| format!("{} want {:.1} have {}{}", r.kind.key(), r.wanted, r.have, r.held.as_ref().map(|h| format!(" (held: {h})")).unwrap_or_default())).collect::<Vec<_>>().join("; "));
        println!("  [r17-goods] imports a day: {}", [nd_sim::Resource::Drinks, nd_sim::Resource::Medicine, nd_sim::Resource::Cement, nd_sim::Resource::Concrete, nd_sim::Resource::Timber, nd_sim::Resource::Cloth, nd_sim::Resource::Clothing, nd_sim::Resource::Limestone].iter().map(|r| format!("{} {:.0}", r.label(), w.economy.taxes.imported_of(*r) * 0.5)).collect::<Vec<_>>().join(", "));
    }
    println!("end kinds: {}", kinds(&w).iter().map(|(n, c)| format!("{n} {c}")).collect::<Vec<_>>().join(", "));
}

fn kinds(w: &World) -> BTreeMap<String, u32> {
    let mut m = BTreeMap::new();
    for b in w.buildings.values() {
        if let Some(z) = &b.business {
            if !z.closed {
                *m.entry(z.kind.label().to_string()).or_default() += 1;
            }
        }
    }
    m
}
