//! Parking probe: the demo district run headless; every few hours lists
//! street-parked cars standing closer than a car length.
//!
//! cargo run --release --example parking_probe -p nd_sim -- [days]

use nd_minds::RuleBasedMind;
use nd_sim::{World, WorldConfig};
use nd_sim::{CarPlace, ParkKind};

/// The same layout as `Sim::new` + `Sim::seed_demo_district` in nd_app.
pub fn demo_world() -> World {
    let mut world = World::new(WorldConfig::default());
    world.drain_events();
    nd_sim::scenarios::demo_district(&mut world);
    world
}

fn main() {
    let days: u64 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(4);
    let mut w = demo_world();
    let mut minds = RuleBasedMind::default();
    for t in 0..days * 1440 {
        w.tick(&mut minds);
        if t % 240 != 239 {
            continue;
        }
        let spots: Vec<_> = w.vehicles.cars.iter().filter_map(|(id, c)| match c.place { CarPlace::Parked(s) if s.kind.on_street() => Some((id, s)), _ => None }).collect();
        let mut pairs = std::collections::BTreeMap::<String, u32>::new();
        let mut example = None;
        for i in 0..spots.len() {
            for j in i + 1..spots.len() {
                let (a, b) = (&spots[i].1, &spots[j].1);
                if a.pos.distance(b.pos) < 4.5 {
                    let k = format!("{:?}/{:?} same_seg {} same_slot {}", a.kind, b.kind, a.segment == b.segment, a.segment == b.segment && a.side == b.side && a.slot == b.slot);
                    *pairs.entry(k).or_default() += 1;
                    example.get_or_insert((a.clone(), b.clone()));
                }
            }
        }
        let over = spots.iter().filter(|(_, s)| s.kind == ParkKind::Overflow).count();
        println!("day {} {:02}:00 street-parked {} overflow {} overlapping {:?}", w.clock.day() + 1, w.clock.hour(), spots.len(), over, pairs);
        if let Some((a, b)) = example {
            println!("   e.g. {:?}
        {:?}", a, b);
        }
    }
}
