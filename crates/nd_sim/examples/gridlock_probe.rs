//! [w1-roads] Headless gridlock survey of a save: where vehicles stand,
//! why, and what they are (away fans, freight, buses, residents). Runs
//! `gridlock_probe <save.ron> [ticks] [every]`; prints a summary every
//! `every` ticks and the worst 100 m cells. Not part of the game.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::World;
use std::collections::{BTreeMap, HashMap};

fn kind(w: &World, id: nd_sim::CarId) -> &'static str {
    let c = &w.traffic.cars[id];
    if w.games.driving_in.iter().any(|f| f.car == id) {
        return "fan_in";
    }
    if c.bus.is_some() {
        return "bus";
    }
    if c.cargo.is_some() {
        return "freight";
    }
    if c.citizen == nd_sim::CitizenId::default() && c.owned.is_none() {
        return "no_owner";
    }
    "resident"
}

struct Totals {
    samples: u32,
    standing: u64,
    vehicles: u64,
    moving_speed: f64,
    moving: u64,
}

fn report(w: &World, label: &str, cells: &mut HashMap<(i32, i32), (u32, BTreeMap<&'static str, u32>)>, tot: &mut Totals) {
    let t = &w.traffic;
    let stall: f32 = std::env::var("STALL").ok().and_then(|s| s.parse().ok()).unwrap_or(120.0);
    let mut why: BTreeMap<&str, usize> = BTreeMap::new();
    let mut by_kind: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    let mut standing = 0usize;
    for (id, c) in t.cars.iter() {
        let k = kind(w, id);
        let e = by_kind.entry(k).or_default();
        e.0 += 1;
        if c.v < 0.5 && c.stalled > stall {
            standing += 1;
            e.1 += 1;
            let r = t.stop_reason(id);
            *why.entry(r.key()).or_default() += 1;
            let cell = ((c.pos.x / 100.0).floor() as i32, (c.pos.y / 100.0).floor() as i32);
            let ce = cells.entry(cell).or_default();
            ce.0 += 1;
            *ce.1.entry(r.key()).or_default() += 1;
        } else if c.v >= 0.5 {
            tot.moving += 1;
            tot.moving_speed += c.v as f64;
        }
    }
    tot.samples += 1;
    tot.standing += standing as u64;
    tot.vehicles += t.cars.len() as u64;
    println!("{label}: day {} {:02}:{:02} vehicles {} standing>{stall:.0}s {standing} kinds(all,standing) {:?} why {:?}", w.clock.day(), w.clock.hour(), w.clock.minute_of_day() % 60, t.cars.len(), by_kind, why);
    // Vehicles waiting at the kerb to pull out, by spot.
    let mut kerb: BTreeMap<(i32, i32), Vec<nd_sim::CarId>> = BTreeMap::new();
    for (id, c) in t.cars.iter() {
        if c.v < 0.5 && c.stalled > stall && t.stop_reason(id).key() == "at_kerb" {
            kerb.entry(((c.pos.x / 10.0).round() as i32, (c.pos.y / 10.0).round() as i32)).or_default().push(id);
        }
    }
    for ((x, y), ids) in kerb.iter().filter(|(_, v)| v.len() >= 3) {
        let c = &t.cars[ids[0]];
        let lane = c.route[c.idx.min(c.route.len() - 1)];
        let l = &t.net.lanes[lane as usize];
        println!("   KERB ({}, {}) {} waiting [{}] lane {lane} s {:.1}/{:.1} lane cars {} entering {} stalled {:.0} -> {}", x * 10, y * 10, ids.len(), kind(w, ids[0]), c.s, l.length, l.cars.len(), c.entering, c.stalled, t.stop_reason(ids[0]).text(|id| format!("{id:?} [{}] v{:.1} s{:.1}", kind(w, id), t.cars.get(id).map_or(0.0, |o| o.v), t.cars.get(id).map_or(0.0, |o| o.s))));
    }
    // Heads of the longest queues.
    let mut heads: HashMap<nd_sim::CarId, (usize, bool)> = HashMap::new();
    for (id, c) in t.cars.iter() {
        if !(c.v < 0.5 && c.stalled > stall) {
            continue;
        }
        let chain = t.jam_chain(id);
        if let Some(h) = chain.head() {
            let e = heads.entry(h).or_default();
            e.0 = e.0.max(chain.cars.len());
            e.1 |= chain.cycle;
        }
    }
    let mut hs: Vec<_> = heads.into_iter().filter(|(_, (n, _))| *n >= 5).collect();
    hs.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    for (h, (n, cycle)) in hs.iter().take(10) {
        let c = &t.cars[*h];
        let lane = c.route[c.idx.min(c.route.len() - 1)];
        let l = &t.net.lanes[lane as usize];
        println!(
            "   head {:?} [{}] queue {n}{} at ({:.0},{:.0}) v{:.1} stalled {:.0}s lane {lane} conn {} s {:.1}/{:.1} idx {}/{} errand {:?} -> {}",
            h,
            kind(w, *h),
            if *cycle { " CYCLE" } else { "" },
            c.pos.x,
            c.pos.y,
            c.v,
            c.stalled,
            l.is_connector(),
            c.s,
            l.length,
            c.idx,
            c.route.len(),
            c.errand,
            t.stop_reason(*h).text(|id| format!("{id:?}"))
        );
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).cloned().expect("save path");
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(240);
    let every: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(30);
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    let t0 = std::time::Instant::now();
    let mut w = World::from_ron(&std::fs::read_to_string(&src).unwrap()).expect("load");
    // Optionally skip to a time of day first (minute of day), quietly.
    if let Some(m) = std::env::var("START_MIN").ok().and_then(|s| s.parse::<u64>().ok()) {
        while w.clock.minute_of_day() != m {
            w.tick(&mut minds);
        }
    }
    if std::env::var("GAME").is_ok() {
        // Put on a game at every stadium today (as `stadium_game` does).
        let st: Vec<_> = w.buildings.iter().filter(|(_, b)| b.service.is_some_and(|s| s.kind == nd_sim::ServiceKind::Stadium)).map(|(id, _)| id).collect();
        for b in st {
            if !w.services.events_today.contains(&b) {
                w.services.events_today.push(b);
            }
        }
    }
    eprintln!("loaded in {:.1}s", t0.elapsed().as_secs_f32());
    let mut cells: HashMap<(i32, i32), (u32, BTreeMap<&'static str, u32>)> = HashMap::new();
    let mut tot = Totals { samples: 0, standing: 0, vehicles: 0, moving_speed: 0.0, moving: 0 };
    report(&w, "start", &mut cells, &mut tot);
    for i in 0..ticks {
        w.tick(&mut minds);
        if (i + 1) % every == 0 {
            report(&w, &format!("+{}", i + 1), &mut cells, &mut tot);
        }
    }
    let mut cs: Vec<_> = cells.into_iter().collect();
    cs.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    println!("worst cells (100 m, standing vehicle-samples):");
    for ((x, y), (n, why)) in cs.iter().take(15) {
        println!("   ({}, {}) {n} {:?}", x * 100 + 50, y * 100 + 50, why);
    }
    println!(
        "SUMMARY samples {} mean_vehicles {:.0} mean_standing {:.1} standing_share {:.3} mean_moving_speed {:.2} m/s report_gridlocks {} wall {:.0}s",
        tot.samples,
        tot.vehicles as f64 / tot.samples as f64,
        tot.standing as f64 / tot.samples as f64,
        tot.standing as f64 / tot.vehicles.max(1) as f64,
        tot.moving_speed / tot.moving.max(1) as f64,
        w.traffic.cars.values().filter(|c| c.stalled > 600.0).count(),
        t0.elapsed().as_secs_f32()
    );
}
