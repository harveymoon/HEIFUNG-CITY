//! Vehicle census probe: what the vehicles on the road are, by kind and
//! errand, and which are stuck or on endless trips. Loads a save
//! (`vehicle_census <save.ron> [ticks] [every]`). Not part of the game.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::traffic::Wait;
use nd_sim::vehicle::Errand;
use nd_sim::World;
use std::collections::BTreeMap;

fn errand_key(e: &Errand) -> &'static str {
    match e {
        Errand::Trip => "trip",
        Errand::Search { .. } => "search",
        Errand::ToLot(_) => "to_lot",
        Errand::ToKerb => "to_kerb",
        Errand::Sale(_) => "sale",
        Errand::Deliver => "deliver",
        Errand::Leave => "leave",
        Errand::Haul { .. } => "haul",
        Errand::Return(_) => "return",
        Errand::Patrol(_) => "patrol",
        Errand::Tow(_) => "tow",
        Errand::Recover(_) => "recover",
        Errand::Taxi(_) => "taxi",
        Errand::Through => "through",
        Errand::Out => "out",
    }
}

fn census(w: &World, label: &str) {
    let t = &w.traffic;
    let mut kinds: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    let mut long = 0;
    for (_, c) in t.cars.iter() {
        let kind = if c.bus.is_some() {
            "bus".to_string()
        } else if c.siren {
            "siren".to_string()
        } else if c.cargo.is_some() {
            format!("cargo/{}", errand_key(&c.errand))
        } else if c.owned.is_some() {
            format!("owned/{}", errand_key(&c.errand))
        } else if w.citizens.contains_key(c.citizen) {
            format!("unowned-driver/{}", errand_key(&c.errand))
        } else {
            format!("nobody/{}", errand_key(&c.errand))
        };
        let e = kinds.entry(kind).or_default();
        e.0 += 1;
        if c.entering {
            e.1 += 1;
        } else if c.stalled > 180.0 && c.wait != Wait::Signal {
            e.2 += 1;
        }
        if c.drive.expected_min > 0.0 && (w.clock.tick.saturating_sub(c.drive.trip_start)) as f32 > 5.0 * c.drive.expected_min + 60.0 {
            long += 1;
        }
    }
    let (parked, driving, searching) = w.vehicles.counts(t);
    println!("{label}: day {} {:02}:{:02} pop {} vehicles {} long_trips {long} | owned parked {parked} driving {driving} searching {searching} | fleet parked {}", w.clock.day(), w.clock.hour(), w.clock.minute_of_day() % 60, w.citizens.len(), t.cars.len(), w.vehicles.fleet_parked.values().map(|&n| n as u32).sum::<u32>());
    for (k, (n, ent, stuck)) in &kinds {
        println!("   {k:<28} {n:>6}  kerb {ent:>5}  stuck {stuck:>5}");
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1440);
    let every: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(360);
    let text = std::fs::read_to_string(path).unwrap();
    let mut w = World::from_ron(&text).expect("load");
    drop(text);
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    census(&w, "loaded");
    for i in 0..ticks {
        w.tick(&mut minds);
        if (i + 1) % every == 0 {
            census(&w, &format!("+{}", i + 1));
        }
    }
    if std::env::var("LONG").is_ok() {
        let t = &w.traffic;
        let mut v: Vec<_> = t.cars.iter().filter(|(_, c)| c.drive.expected_min > 0.0).map(|(id, c)| (id, w.clock.tick.saturating_sub(c.drive.trip_start) as f32 / c.drive.expected_min, c)).collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (id, r, c) in v.iter().take(15) {
            println!("LONG {id:?} x{r:.1} exp {:.0} errand {:?} owned {} cargo {} pos ({:.0},{:.0}) wait {:?} stalled {:.0} replans {} route {}/{} anger {:.2}", c.drive.expected_min, c.errand, c.owned.is_some(), c.cargo.is_some(), c.pos.x, c.pos.y, c.wait, c.stalled, c.replans, c.idx, c.route.len(), c.drive.anger);
        }
    }
}
