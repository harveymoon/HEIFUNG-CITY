//! Load a save, report traffic state, run on and time it. Not part of the game.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::traffic::Wait;
use nd_sim::World;
use std::collections::BTreeMap;

fn snapshot(w: &World, label: &str) {
    let t = &w.traffic;
    let mut stuck = 0;
    let mut kerb = 0;
    let mut why: BTreeMap<String, usize> = BTreeMap::new();
    for c in t.cars.values() {
        if c.stalled > 180.0 && c.hold <= 0.0 && !c.entering && c.wait != Wait::Signal {
            stuck += 1;
            *why.entry(format!("{:?}", c.wait)).or_default() += 1;
        } else if c.entering && c.stalled > 180.0 {
            kerb += 1;
        }
    }
    let locks = t.gridlocks();
    println!("{label}: tick {} vehicles {} stuck {stuck} kerb {kerb} gridlocks {} {:?}", w.clock.tick, t.cars.len(), locks.len(), why);
    for g in locks.iter().take(5) {
        println!("   lock {} vehicles at {:?}", g.vehicles, g.points.iter().take(4).map(|p| (p.x.round(), p.y.round())).collect::<Vec<_>>());
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1440);
    let text = std::fs::read_to_string(path).unwrap();
    let mut w = World::from_ron(&text).expect("load");
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    let _ = &mut minds;
    snapshot(&w, "loaded");
    if let Some(a) = args.iter().find(|a| a.starts_with("-lane")) {
        let t = &w.traffic;
        let mut lid: u32 = a[5..].parse().unwrap();
        for _ in 0..6 {
            let lane = &t.net.lanes[lid as usize];
            println!("LANE {lid} conn {} len {:.1} green {} end ({:.0},{:.0}) cars {}", lane.is_connector(), lane.length, lane.green, lane.end().x, lane.end().y, lane.cars.len());
            for &o in lane.cars.iter().rev().take(4) {
                let x = &t.cars[o];
                println!("     {:?}@{:.1} v{:.1} {:?} st{:.0} ent{} route_next {:?} drive {:?}", o, x.s, x.v, x.wait, x.stalled, x.entering, x.route.get(x.idx + 1..(x.idx + 3).min(x.route.len())), x.drive.gang.is_some());
            }
            let Some(&head) = lane.cars.last() else { break };
            let h = &t.cars[head];
            let Some(&n) = h.route.get(h.idx + 1) else { break };
            lid = n;
        }
    }
    if args.iter().any(|a| a == "-d") {
        let t = &w.traffic;
        let mut ent: Vec<_> = t.cars.iter().filter(|(_, c)| c.entering && c.stalled > 1000.0 && (std::env::var("UNK").is_err() || (c.bus.is_none() && c.cargo.is_none() && !w.citizens.contains_key(c.citizen)))).collect();
        ent.sort_by(|a, b| b.1.stalled.total_cmp(&a.1.stalled));
        for (id, c) in ent.iter().take(10) {
            let lid = c.route[c.idx];
            let lane = &t.net.lanes[lid as usize];
            let around: Vec<String> = lane.cars.iter().filter(|&&o| (t.cars[o].s - c.s).abs() < 40.0).map(|&o| { let x = &t.cars[o]; format!("{:?}@{:.1} len{:.0} v{:.1} {:?} st{:.0} ent{}", o, x.s, x.length(), x.v, x.wait, x.stalled, x.entering) }).collect();
            println!("ENT {:?} bus {} cargo {} owned {:?} errand {:?} siren {} citizen_null {} len {:.0} lane {lid} s {:.1}/{:.1} prev {:?} st {:.0} pos ({:.0},{:.0})", id, c.bus.is_some(), c.cargo.is_some(), c.owned.is_some(), c.errand, c.siren, c.citizen == Default::default(), c.length(), c.s, lane.length, lane.prev, c.stalled, c.pos.x, c.pos.y);
            for a in around { println!("      {a}"); }
            for &pl in &lane.prev { let l = &t.net.lanes[pl as usize]; if let Some(&last) = l.cars.last() { let x = &t.cars[last]; println!("      prev lane {pl} len {:.1} last {:?}@{:.1} v{:.1} {:?}", l.length, last, x.s, x.v, x.wait); } }
        }
    }
    let t0 = std::time::Instant::now();
    let mut chunk = std::time::Instant::now();
    for i in 0..ticks {
        w.tick(&mut minds);
        if (i + 1) % 360 == 0 {
            snapshot(&w, &format!("+{} ticks ({:.2}s/day pace)", i + 1, chunk.elapsed().as_secs_f32() * 4.0));
            chunk = std::time::Instant::now();
        }
    }
    println!("{} ticks in {:.2}s = {:.2}s per sim day", ticks, t0.elapsed().as_secs_f32(), t0.elapsed().as_secs_f32() * 1440.0 / ticks as f32);
}
