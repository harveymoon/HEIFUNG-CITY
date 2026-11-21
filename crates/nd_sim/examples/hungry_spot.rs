//! [r12-hunger] Probe: the hungry living round a spot, and the grocers
//! there. Runs a save on `hours`, then lists what the hungry within 600 m
//! of (x, y) are doing, where they are headed, what they remember, and the
//! grocers within 600 m (stock, visitors, open now).
//!
//! cargo run --profile fast -p nd_sim --example hungry_spot -- saves/city.ron 42 440 824

use nd_sim::market::Resource;
use nd_sim::{Activity, BusinessKind, Vec2, World};
use std::collections::BTreeMap;

fn norm(s: &str) -> String {
    let s = s.split(" at ").next().unwrap_or(s);
    s.chars().map(|c| if c.is_ascii_digit() { '#' } else { c }).take(70).collect()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut w = World::read_save(&a[1]).expect("load");
    let hours: u64 = a[2].parse().unwrap();
    let spot = Vec2::new(a[3].parse().unwrap(), a[4].parse().unwrap());
    let mut minds = nd_minds::RuleBasedMind::default();
    for _ in 0..hours * 60 {
        w.tick(&mut minds);
        w.drain_events();
    }
    let hour = w.clock.hour() as u8;
    println!("day {} {:02}:00 at ({:.0},{:.0})", w.clock.day(), hour, spot.x, spot.y);
    let mut by: BTreeMap<String, u32> = BTreeMap::new();
    let mut mem: BTreeMap<String, u32> = BTreeMap::new();
    let mut n = 0;
    for c in w.citizens.values() {
        let Some(home) = c.home.and_then(|h| w.buildings.get(h)) else { continue };
        let Some(l) = w.lots.get(home.lot) else { continue };
        if l.center.distance(spot) > 600.0 || c.satiety >= 0.25 {
            continue;
        }
        n += 1;
        let mut k = |s: String| *by.entry(s).or_default() += 1;
        let pantry = c.household.and_then(|h| w.households.get(h)).map_or(c.groceries, |h| h.groceries);
        k(format!("pantry {}", if pantry < 1.0 { "empty" } else { "some" }));
        k(format!("stage {:?}", c.stage()));
        k(format!("at {:.0} m from home", (c.pos.distance(l.center) / 250.0).floor() * 250.0));
        match &c.activity {
            Activity::Traveling { to, purpose, .. } => {
                let d = to.and_then(|b| w.buildings.get(b)).and_then(|b| w.lots.get(b.lot)).map_or(-1.0, |l| l.center.distance(c.pos));
                k(format!("traveling {purpose:?} by {:?}, {} m to go", c.mode, (d / 250.0).floor() * 250.0));
            }
            Activity::Driving { purpose, .. } => k(format!("driving {purpose:?}")),
            act => k(format!("{:?}", act).split(|ch: char| !ch.is_alphanumeric()).next().unwrap_or("").to_string()),
        }
        let th: String = c.thought.chars().map(|ch| if ch.is_ascii_digit() { '#' } else { ch }).take(50).collect();
        k(format!("thought: {th}"));
        for m in c.memories.iter().rev().take(3) {
            *mem.entry(norm(&m.text)).or_default() += 1;
        }
    }
    println!("hungry residents within 600 m: {n}");
    for (k, v) in &by {
        if *v >= 3 {
            println!("  {v:5}  {k}");
        }
    }
    let mut mem: Vec<_> = mem.into_iter().collect();
    mem.sort_by(|a, b| b.1.cmp(&a.1));
    println!("  memories:");
    for (k, v) in mem.iter().take(25) {
        println!("    {v:5}  {k}");
    }
    println!("grocers within 600 m:");
    for (id, b) in w.buildings.iter() {
        let Some(z) = &b.business else { continue };
        if z.kind != BusinessKind::Grocery {
            continue;
        }
        let Some(l) = w.lots.get(b.lot) else { continue };
        if l.center.distance(spot) > 600.0 {
            continue;
        }
        let headed = w.citizens.values().filter(|c| matches!(&c.activity, Activity::Traveling { to: Some(t), .. } if *t == id)).count();
        let walk = w.building_entrance(id);
        println!(
            "  {:?} {} ({:.0},{:.0}) closed {} operating {} serves now {} hours {} stock {:.0} visitors {}/{} headed {headed} entrance {:?} floor0 lot car_access {}",
            id,
            b.name,
            l.center.x,
            l.center.y,
            z.closed,
            b.operating(),
            b.serves_at(hour),
            b.opening_hours().label(),
            z.stock_of(Resource::Groceries),
            b.visitors,
            b.customer_capacity(),
            walk,
            l.car_access()
        );
    }
}
