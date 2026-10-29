//! Load a save (or generate a city: `gen:<preset>:<seed>`) and tick on,
//! checking after every tick that each lane's car list names only cars
//! that exist; reports the first stale entry (lane, car, what the car was)
//! and stops. Not part of the game.
//! `cargo run --profile fast -p nd_procgen --example stale_lane_probe -- <save | gen:grid_plaza:1> [ticks]`
use nd_minds::{HybridMind, MockBackend};
use nd_sim::World;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2880);
    let mut w = if let Some(spec) = args[1].strip_prefix("gen:") {
        let (preset, seed) = spec.split_once(':').expect("gen:<preset>:<seed>");
        let preset = nd_procgen::citygen::CityPreset::parse(preset).expect("preset");
        nd_procgen::citygen::generate_world(preset, seed.parse().expect("seed")).0
    } else {
        World::from_save_bytes(&std::fs::read(&args[1]).expect("read save")).expect("load")
    };
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    println!("loaded: day {} tick {} cars {}", w.clock.day(), w.clock.tick, w.traffic.cars.len());
    let mut last: std::collections::BTreeMap<String, String> = Default::default();
    for _ in 0..ticks {
        // What every car was before this tick, so a stale id can be named.
        last.clear();
        for (id, c) in w.traffic.cars.iter() {
            last.insert(format!("{id:?}"), format!("siren {} bus {} cargo {} lane {:?} s {:.1} route {}/{}", c.siren, c.bus.is_some(), c.cargo.is_some(), c.drive.lane, c.s, c.idx, c.route.len()));
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| w.tick(&mut minds)));
        let stale: Vec<(usize, String)> = w
            .traffic
            .net
            .lanes
            .iter()
            .enumerate()
            .flat_map(|(i, l)| l.cars.iter().filter(|&&c| !w.traffic.cars.contains_key(c)).map(move |c| (i, format!("{c:?}"))))
            .collect();
        if result.is_err() || !stale.is_empty() {
            println!("tick {} (day {} {:02}:{:02}): panicked {} stale {}", w.clock.tick, w.clock.day(), w.clock.hour(), w.clock.minute(), result.is_err(), stale.len());
            for (lane, c) in stale.iter().take(10) {
                println!("  lane {lane}: {c} was {}", last.get(c).map_or("(not there before the tick)", String::as_str));
            }
            return;
        }
    }
    println!("no stale lane entries in {ticks} ticks");
}
