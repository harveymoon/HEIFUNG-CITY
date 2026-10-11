//! Headless delivery-lorry probe: lorries setting off per hour of the day,
//! by where they come from (`truck_probe <save.ron> [ticks]`). Not part of
//! the game.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::World;
use std::collections::{BTreeMap, HashSet};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut w = World::from_ron(&std::fs::read_to_string(&args[1]).unwrap()).expect("load");
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1440);
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    let mut seen: HashSet<nd_sim::CarId> = w.traffic.cars.iter().filter(|(_, c)| c.cargo.is_some()).map(|(id, _)| id).collect();
    let mut per_hour: BTreeMap<u64, (u32, u32, u32)> = BTreeMap::new();
    for _ in 0..ticks {
        w.tick(&mut minds);
        let h = w.clock.hour();
        for (id, c) in w.traffic.cars.iter() {
            let Some(k) = c.cargo else { continue };
            if seen.insert(id) {
                let e = per_hour.entry(h).or_default();
                let to_site = w.buildings.get(k.to).is_some_and(|b| b.construction.is_some());
                if to_site { e.0 += 1 } else if w.buildings.get(k.from).is_some() { e.1 += 1 } else { e.2 += 1 }
            }
        }
    }
    println!("hour: to building sites / from a business in town / from out of town; queued now {}", w.queued_trucks());
    for (h, (s, b, o)) in per_hour {
        println!("{h:02}: {s:4} {b:4} {o:4}");
    }
}
