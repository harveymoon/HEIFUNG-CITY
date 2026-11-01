//! [r5-fixes] Vehicles stuck at the kerb: load a save, count the vehicles
//! standing pulled in (waiting at the kerb) whose trip has run far over
//! time, and those standing past the end of their route, then tick on and
//! count again; also counts the airlifts flown and their reasons. Not part
//! of the game.
//! `cargo run --profile fast -p nd_procgen --example kerb_stuck_probe -- <save> [ticks] [x,y]`
use nd_minds::{HybridMind, MockBackend};
use nd_sim::World;

fn census(w: &World, label: &str, watch: Option<nd_sim::Vec2>) {
    let (mut kerb, mut past_end, mut over_day, mut over_day_kerb, mut over_week_kerb, mut lorries, mut stranded) = (0, 0, 0, 0, 0, 0, 0);
    for (id, c) in w.traffic.cars.iter() {
        let last = c.idx + 1 >= c.route.len();
        let past = last && c.s > c.s_end + 1.0;
        let (elapsed, expected) = w.car_trip_minutes(id).unwrap_or((0.0, 0.0));
        let long = elapsed > 1440.0 && elapsed > 4.0 * expected.max(1.0);
        kerb += c.entering as u32;
        past_end += (past && c.entering) as u32;
        over_day += long as u32;
        over_day_kerb += (long && c.entering) as u32;
        over_week_kerb += (c.entering && elapsed > 7.0 * 1440.0) as u32;
        lorries += (c.entering && long && c.cargo.is_some()) as u32;
        stranded += (w.traffic.stop_reason(id).key() == "stranded") as u32;
    }
    println!(
        "{label}: day {} cars {} at_kerb {kerb} kerb_past_route_end {past_end} trip_over_a_day {over_day} (of them at the kerb {over_day_kerb}, lorries {lorries}; at the kerb over a week {over_week_kerb}; shown stranded {stranded})",
        w.clock.day(),
        w.traffic.cars.len()
    );
    if let Some(p) = watch {
        for (id, c) in w.traffic.cars.iter().filter(|(_, c)| c.pos.distance(p) < 6.0) {
            let (elapsed, expected) = w.car_trip_minutes(id).unwrap_or((0.0, 0.0));
            println!(
                "  watch {id:?}: entering {} hold {:.0} wait {:?} route {:?} idx {} s {:.2} s_end {:.2} v {:.1} cargo {:?} trip {elapsed:.0}/{expected:.0} min stunt {}",
                c.entering, c.hold, c.wait, c.route, c.idx, c.s, c.s_end, c.v, c.cargo.map(|g| (g.amount, g.from, g.to)), c.in_stunt()
            );
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1440);
    let watch = args.get(3).and_then(|s| s.split_once(',')).map(|(x, y)| nd_sim::Vec2::new(x.parse().unwrap(), y.parse().unwrap()));
    let mut w = World::from_save_bytes(&std::fs::read(&args[1]).expect("read save")).expect("load");
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    census(&w, "loaded", watch);
    let net = &w.traffic.net;
    println!("trapped lanes {} of {}", net.trap.iter().filter(|t| **t).count(), net.lanes.len());
    for t in w.roads.traps() {
        let names: std::collections::BTreeSet<String> = t.segments.iter().filter_map(|s| w.streets.of(*s)).map(|s| s.name.clone()).collect();
        let oneway: Vec<String> = t.segments.iter().map(|&s| w.roads.segment(s)).map(|s| format!("{}+{}", s.lanes_fwd, s.lanes_bwd)).collect();
        println!("  pocket at ({:.0}, {:.0}): {} pieces {:?} lanes {:?}", t.at.x, t.at.y, t.segments.len(), names, oneway);
    }
    if let Some(p) = watch {
        for (i, l) in net.lanes.iter().enumerate().filter(|(_, l)| l.points.iter().any(|q| q.distance(p) < 30.0)) {
            println!("  lane {i}: trap {} len {:.1} next {:?} prev {:?} kind {:?}", net.trap[i], l.length, l.next, l.prev, l.kind);
        }
    }
    let mut lifts: Vec<(u64, nd_sim::Vec2)> = Vec::new();
    for t in 0..ticks {
        w.tick(&mut minds);
        for e in w.drain_events().iter() {
            if let nd_sim::SimEvent::Airlift { text, .. } = e {
                println!("  tick {} event: {text}", w.clock.tick);
            }
        }
        if t % 1440 == 1439 {
            println!("  airlifts sent by tick +{}: {}", t + 1, lifts.len());
        }
        for a in w.airlifts.iter().filter(|a| a.started == w.clock.tick || a.started + 1 == w.clock.tick) {
            if !lifts.iter().any(|l| l.0 == a.started && l.1.distance(a.at) < 1.0) {
                lifts.push((a.started, a.at));
            }
        }
        if t < 5 || t % 360 == 359 {
            census(&w, &format!("tick +{}", t + 1), watch);
        }
    }
    println!("airlifts sent in {ticks} ticks: {}", lifts.len());
    for (t, at) in lifts.iter().take(20) {
        println!("  tick {t} at ({:.0}, {:.0})", at.x, at.y);
    }
}
