//! [r5-fixes] Generate a city the way the game's `generate_city` does (its
//! highway, its starting money, the world's own seed) and tick on, checking
//! after every tick that each lane's car list names only cars that exist,
//! each listed on the lane it is driving (`route[idx]`), once, and not while
//! it waits at the kerb or is off the lanes in a stunt. Counts each kind of
//! fault and prints the first few. Not part of the game.
//! `cargo run --profile fast -p nd_procgen --example lane_list_probe -- <preset> <seed> [ticks] [landscape] [climate]`
use nd_minds::{HybridMind, MockBackend};
use nd_sim::landscape::{Climate, LandscapePreset};
use nd_sim::{World, WorldConfig};
use std::collections::BTreeMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let preset = nd_procgen::citygen::CityPreset::parse(&args[1]).expect("preset");
    let seed: u64 = args[2].parse().expect("seed");
    let ticks: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(10_000);
    let mut landscape = match args.get(4).filter(|s| s.as_str() != "-") {
        Some(l) => nd_sim::landscape::LandscapeParams::preset(LandscapePreset::from_label(l).expect("landscape"), seed),
        None => preset.landscape(seed),
    };
    if let Some(c) = args.get(5) {
        landscape.climate = Climate::from_label(c).expect("climate");
    }
    // As the game's `Sim::fresh_world` + `generate_city`.
    landscape.highway = true;
    let mut w = World::new(WorldConfig { landscape, ..WorldConfig::default() });
    w.economy.treasury = 60_000.0;
    w.economy.ledger = Default::default();
    w.drain_events();
    let plan = nd_procgen::citygen::generate(preset, w.landscape.params.seed, &w.landscape);
    nd_procgen::citygen::lay_city(&mut w, &plan);
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    let mut counts: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut shown = 0;
    let mut max_cars = 0;
    for _ in 0..ticks {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| w.tick(&mut minds)));
        if result.is_err() {
            println!("PANIC at tick {} (day {} {:02}:{:02})", w.clock.tick, w.clock.day(), w.clock.hour(), w.clock.minute());
            *counts.entry("panic").or_default() += 1;
            break;
        }
        let t = &w.traffic;
        max_cars = max_cars.max(t.cars.len());
        let mut seen: BTreeMap<String, usize> = BTreeMap::new();
        for (i, l) in t.net.lanes.iter().enumerate() {
            for &cid in &l.cars {
                let key = format!("{cid:?}");
                let kind = match t.cars.get(cid) {
                    None => "ghost",
                    Some(c) if c.entering => "listed_while_entering",
                    Some(c) if c.in_stunt() => "listed_in_stunt",
                    Some(c) if c.route.get(c.idx).copied() != Some(i as _) => "listed_off_its_lane",
                    _ => "",
                };
                if seen.insert(key.clone(), i).is_some() {
                    *counts.entry("listed_twice").or_default() += 1;
                }
                if !kind.is_empty() {
                    *counts.entry(kind).or_default() += 1;
                    if shown < 12 {
                        shown += 1;
                        let what = t.cars.get(cid).map_or(String::new(), |c| format!("route lane {:?} idx {}/{} wait {:?} s {:.1} v {:.1} bus {} cargo {} siren {} crashed {}", c.route.get(c.idx), c.idx, c.route.len(), c.wait, c.s, c.v, c.bus.is_some(), c.cargo.is_some(), c.siren, c.crashed()));
                        println!("tick {} day {} {:02}:{:02}: {kind} {key} on lane {i}: {what}", w.clock.tick, w.clock.day(), w.clock.hour(), w.clock.minute());
                    }
                }
            }
        }
    }
    println!("{} seed {seed}: {} ticks to day {}, peak cars {max_cars}, population {}; faults {:?}", preset.key(), w.clock.tick, w.clock.day(), w.citizens.len(), counts);
}
