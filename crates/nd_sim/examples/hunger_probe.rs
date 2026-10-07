//! [review] Probe: load a save, run some hours, and describe the hungry:
//! their pantry, purse, what they are doing and what they remember.
//!
//! cargo run --profile fast -p nd_sim --example hunger_probe -- saves/city.ron 24

use std::collections::BTreeMap;

fn norm(s: &str) -> String {
    // Drop place names: keep the text up to " at " / ":" and digits out.
    let s = s.split(" at ").next().unwrap_or(s);
    s.chars().map(|c| if c.is_ascii_digit() { '#' } else { c }).collect()
}

fn main() {
    let path = std::env::args().nth(1).expect("a save path");
    let hours: u64 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(24);
    let mut w = nd_sim::World::from_ron(&std::fs::read_to_string(&path).expect("read")).expect("load");
    let mut minds = nd_minds::RuleBasedMind::default();
    let end = w.clock.tick + hours * 60;
    while w.clock.tick < end {
        w.tick(&mut minds);
    }
    let hungry: Vec<_> = w.citizens.iter().filter(|(_, c)| c.hungry_days >= 1 || c.satiety < 0.25).collect();
    println!("day {} hour {}: {} citizens, {} hungry (hungry_days>=1 or satiety<0.25)", w.clock.day(), w.clock.hour(), w.citizens.len(), hungry.len());
    let mut by: BTreeMap<String, u32> = BTreeMap::new();
    let mut mem: BTreeMap<String, u32> = BTreeMap::new();
    for (_, c) in &hungry {
        let hh = c.household.and_then(|h| w.households.get(h));
        let pantry = hh.map_or(-1.0, |h| h.groceries);
        let money = hh.map_or(c.savings, |h| h.money);
        *by.entry(format!("household {}", hh.is_some())).or_default() += 1;
        *by.entry(format!("home {}", c.home.is_some())).or_default() += 1;
        *by.entry(format!("pantry {}", if pantry < 0.0 { "-" } else if pantry < 1.0 { "empty" } else { "some" })).or_default() += 1;
        *by.entry(format!("money {}", if money < -50.0 { "<-50" } else if money < 0.0 { "<0" } else if money < 50.0 { "<50" } else { ">=50" })).or_default() += 1;
        let act = format!("{:?}", c.activity);
        *by.entry(format!("activity {}", act.split(|ch: char| !ch.is_alphanumeric()).next().unwrap_or(""))).or_default() += 1;
        *by.entry(format!("stage {:?}", c.stage())).or_default() += 1;
        *by.entry(format!("works {}", c.work.is_some())).or_default() += 1;
        for m in c.memories.iter().rev().take(4) {
            *mem.entry(norm(&m.text)).or_default() += 1;
        }
    }
    // The hungry on their way: where to, how far along, are they moving?
    let travellers: Vec<(nd_sim::citizens::CitizenId, glam::Vec2)> = hungry.iter().filter(|(_, c)| matches!(c.activity, nd_sim::citizens::Activity::Traveling { .. })).map(|(id, c)| (*id, c.pos)).collect();
    for (id, c) in hungry.iter().filter(|(_, c)| matches!(c.activity, nd_sim::citizens::Activity::Traveling { .. })) {
        if let nd_sim::citizens::Activity::Traveling { to, purpose, path, idx } = &c.activity {
            *by.entry(format!("trip purpose {purpose:?}")).or_default() += 1;
            *by.entry(format!("trip to building {}", to.is_some())).or_default() += 1;
            *by.entry(format!("trip mode {:?}", c.mode)).or_default() += 1;
            let left: f32 = path.windows(2).skip(*idx).map(|p| p[0].distance(p[1])).sum();
            *by.entry(format!("trip left {}", if left < 200.0 { "<200m" } else if left < 1000.0 { "<1km" } else if left < 3000.0 { "<3km" } else { ">=3km" })).or_default() += 1;
            let _ = id;
        }
    }
    let snapshot: Vec<_> = travellers.clone();
    let all_bikes: Vec<(nd_sim::citizens::CitizenId, glam::Vec2)> = w.citizens.iter().filter(|(_, c)| matches!(c.activity, nd_sim::citizens::Activity::Traveling { .. }) && c.mode == nd_sim::citizens::TravelMode::Bike).map(|(id, c)| (id, c.pos)).collect();
    let t1 = w.clock.tick + 60;
    while w.clock.tick < t1 {
        w.tick(&mut minds);
    }
    let mut moved = BTreeMap::<String, u32>::new();
    for (id, p) in snapshot {
        let Some(c) = w.citizens.get(id) else { *moved.entry("gone".into()).or_default() += 1; continue };
        let d = c.pos.distance(p);
        let act = format!("{:?}", c.activity);
        let act = act.split(|ch: char| !ch.is_alphanumeric()).next().unwrap_or("").to_string();
        *moved.entry(format!("an hour later: {act}, moved {}", if d < 5.0 { "<5m" } else if d < 300.0 { "<300m" } else { ">=300m" })).or_default() += 1;
    }
    let mut spots = BTreeMap::<(i32, i32), u32>::new();
    let mut still = 0;
    for (id, p) in &all_bikes {
        if w.citizens.get(*id).is_some_and(|c| c.pos.distance(*p) < 5.0) {
            still += 1;
            *spots.entry(((p.x / 100.0).round() as i32 * 100, (p.y / 100.0).round() as i32 * 100)).or_default() += 1;
        }
    }
    let mut spots: Vec<_> = spots.into_iter().collect();
    spots.sort_by(|a, b| b.1.cmp(&a.1));
    println!("cyclists on the way: {}, still an hour later: {still}; top spots {:?}", all_bikes.len(), &spots[..spots.len().min(8)]);
    for (k, v) in &moved {
        println!("  {k}: {v}");
    }
    for (k, v) in &by {
        println!("  {k}: {v}");
    }
    let mut mem: Vec<_> = mem.into_iter().collect();
    mem.sort_by(|a, b| b.1.cmp(&a.1));
    println!("memories (last 4 each):");
    for (k, v) in mem.iter().take(25) {
        println!("  {v:5}  {k}");
    }
    // Shops.
    let r = nd_sim::market::Resource::Groceries;
    let mut sellers = 0;
    let mut stocked = 0;
    let mut closed = 0;
    let mut dark = 0;
    let mut stock = 0.0;
    for b in w.buildings.values() {
        let Some(bz) = &b.business else { continue };
        if !bz.sells(r) {
            continue;
        }
        sellers += 1;
        if bz.closed {
            closed += 1;
        }
        if bz.stock_of(r) >= 1.0 {
            stocked += 1;
        }
        if b.powered < 0.6 {
            dark += 1;
        }
        stock += bz.stock_of(r);
    }
    println!("grocery sellers {sellers}: stocked {stocked}, closed {closed}, dark {dark}, stock {stock:.0}; power served {:.0}%", w.economy.power.served * 100.0);
    println!("market: {:?}", w.economy.market);
}
