//! [r13-catalogue] Probe: load a save and run it on day by day, printing
//! the economy's headline numbers and every good's flow, so two builds
//! (before and after a change to the goods catalogue) can be diffed.
//!
//! cargo run --profile fast -p nd_sim --example catalogue_probe -- saves/city.ron 3

use nd_sim::market::Resource;
use nd_sim::World;
use std::collections::BTreeMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).cloned().expect("save path");
    let days: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let mut w = World::read_save(&src).expect("load");
    let mut minds = nd_minds::RuleBasedMind::default();
    println!("start: day {} pop {} treasury {:.0}", w.clock.day(), w.citizens.len(), w.economy.treasury);
    for _ in 0..days {
        // Sample the market just before midnight, when the day's counters
        // are full.
        let (mut deals, mut imports, mut exports) = (0u32, 0f32, 0f32);
        for i in 0..1440 {
            w.tick(&mut minds);
            if i == 1438 {
                let m = &w.economy.market;
                deals = m.deals_today;
                imports = m.imports_today;
                exports = m.exports_today;
            }
        }
        let e = &w.economy;
        let m = &e.market;
        let biz_money: f32 = w.buildings.values().filter_map(|b| b.business.as_ref()).map(|z| z.money).sum();
        let open = w.buildings.values().filter_map(|b| b.business.as_ref()).filter(|z| !z.closed).count();
        let mut boxes: BTreeMap<String, u32> = BTreeMap::new();
        let mut box_goods: BTreeMap<String, u32> = BTreeMap::new();
        for b in w.containers.boxes.values() {
            *boxes.entry(format!("{:?}", b.kind)).or_default() += 1;
            if let Some(g) = &b.goods {
                *box_goods.entry(format!("{:?}/{:?}", b.kind, g.resource)).or_default() += 1;
            }
        }
        println!(
            "day {}: pop {} treasury {:.0} business money {:.0} open {} | deals {} imports {:.0} exports {:.0} | materials {:.0} | trucks {}",
            w.clock.day(),
            w.citizens.len(),
            e.treasury,
            biz_money,
            open,
            deals,
            imports,
            exports,
            e.materials_today,
            w.traffic.cars.values().filter(|c| c.cargo.is_some()).count()
        );
        let goods: Vec<String> = Resource::ALL.iter().map(|r| format!("{} {:.0}@{:.1}", r.label(), m.stock.get(r).copied().unwrap_or(0.0), m.avg_price.get(r).copied().unwrap_or(0.0))).collect();
        println!("  goods: {}", goods.join(", "));
        println!("  boxes: {:?}", boxes);
        println!("  box goods: {:?}", box_goods);
    }
}
