//! Load a save and report on the bus lines, depots and transit trips:
//! why lines get no buses, how riders choose. Not part of the game.
//!
//! cargo run --profile fast -p nd_sim --example transit_probe -- <save.ron> [ticks]
use nd_minds::RuleBasedMind;
use nd_sim::World;

fn lines(w: &World, label: &str) {
    println!("== {label}: day {} {:02}:{:02} tick {}", w.clock.day(), w.clock.minute_of_day() / 60, w.clock.minute_of_day() % 60, w.clock.tick);
    for (id, b) in w.buildings.iter().filter(|(_, b)| b.depot) {
        let lot = w.building_lot(id);
        let out = w.traffic.cars.values().filter(|c| c.bus.as_ref().is_some_and(|x| x.depot == id)).count();
        println!("  depot {:?} {} working {} seg {:?} at {:?} buses {}", id, b.name, b.working_depot(), lot.and_then(|l| l.car_segment), lot.map(|l| l.car_point), out);
    }
    for (rid, r) in &w.transit.routes {
        let out = w.traffic.cars.values().filter(|c| c.bus.as_ref().is_some_and(|x| x.route == rid)).count();
        println!("  line {:?} {:12} stops {} buses {} out {out} runnable {} broken {:?} riders {} yday {}", rid, r.name, r.stops.len(), r.buses, r.runnable(), r.broken, r.riders_today, r.boardings_yesterday());
        for p in w.line_problems(rid) {
            println!("      problem: {p}");
        }
    }
    let e = &w.economy.transit;
    println!("  transit: buses {} waiting {} riding {} riders today {} | metro riders {} | rail riders {}", e.buses, e.waiting, e.riding, e.riders_today, w.subway.stats.riders_today, w.rail.stats.riders_today);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&args[1]).unwrap();
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
    let mut w = World::from_ron(&text).expect("load");
    lines(&w, "loaded");
    let mut minds = RuleBasedMind::default();
    let mut done = 0;
    while done < ticks {
        w.tick(&mut minds);
        done += 1;
        if done % 120 == 0 {
            lines(&w, "running");
        }
    }
    if args.iter().any(|a| a == "-modes") {
        println!("{}", ron::ser::to_string_pretty(&w.ridership_report(4000), ron::ser::PrettyConfig::default()).unwrap());
    }
}
