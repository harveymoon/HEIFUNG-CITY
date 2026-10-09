//! Headless jam probe: why every standing vehicle stands, and the heads
//! of the queues (`Traffic::stop_reason`, `Traffic::jam_chain`). Runs a
//! save (`jam_probe <save.ron> [ticks] [every]`) or the demo district
//! (`jam_probe demo [ticks] [every]`). Not part of the game.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::{World, WorldConfig};
use std::collections::{BTreeMap, HashMap};

fn seed(world: &mut World) {
    world.drain_events();
    nd_sim::scenarios::demo_district(world);
}


fn report(w: &World, label: &str) {
    let t = &w.traffic;
    let mut why: BTreeMap<&str, usize> = BTreeMap::new();
    let mut heads: HashMap<nd_sim::CarId, (usize, bool)> = HashMap::new();
    let standing: Vec<nd_sim::CarId> = t.cars.iter().filter(|(_, c)| c.v < 0.5 && c.stalled > std::env::var("STALL").ok().and_then(|s| s.parse().ok()).unwrap_or(60.0)).map(|(id, _)| id).collect();
    for &id in &standing {
        let r = t.stop_reason(id);
        *why.entry(r.key()).or_default() += 1;
        let chain = t.jam_chain(id);
        if let Some(h) = chain.head() {
            let e = heads.entry(h).or_default();
            e.0 = e.0.max(chain.cars.len());
            e.1 |= chain.cycle;
        }
    }
    println!("   angry now {} road rage today {} crashes today {} timed {} over2x {}", w.angry_drivers_now(), w.driving.road_rage_today, w.driving.crashes_today, t.cars.values().filter(|c| c.drive.expected_min > 0.0).count(), t.cars.values().filter(|c| c.drive.expected_min > 0.0 && (w.clock.tick - c.drive.trip_start) as f32 > 2.0 * c.drive.expected_min).count());
    println!("{label}: tick {} day {} {:02}:{:02} vehicles {} standing>60s {} {:?}", w.clock.tick, w.clock.day(), w.clock.hour(), w.clock.minute_of_day() % 60, t.cars.len(), standing.len(), why);
    let mut hs: Vec<_> = heads.into_iter().filter(|(_, (n, _))| *n >= 3).collect();
    hs.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    for (h, (n, cycle)) in hs.iter().take(12) {
        let c = &t.cars[*h];
        let lane = c.route[c.idx];
        let l = &t.net.lanes[lane as usize];
        let name = |id: nd_sim::CarId| format!("{id:?}");
        println!(
            "   head {:?} queue {n}{} at ({:.0},{:.0}) v{:.1} stalled {:.0}s wait {:?} entering {} hold {:.0} lane {lane} conn {} s {:.1}/{:.1} idx {}/{} errand {:?} bus {} cargo {} -> {}",
            h, if *cycle { " (cycle)" } else { "" }, c.pos.x, c.pos.y, c.v, c.stalled, c.wait, c.entering, c.hold, l.is_connector(), c.s, l.length, c.idx, c.route.len(), c.errand, c.bus.is_some(), c.cargo.is_some(), t.stop_reason(*h).text(name)
        );
    }
    let mut lc: Vec<_> = standing.iter().copied().filter(|&id| matches!(t.stop_reason(id), nd_sim::traffic::StopReason::LaneChange { .. })).collect();
    lc.sort_by(|a, b| t.cars[*b].stalled.total_cmp(&t.cars[*a].stalled));
    for id in lc.into_iter().take(6) {
        let c = &t.cars[id];
        let lane = c.route[c.idx];
        let l = &t.net.lanes[lane as usize];
        let tgt = c.route[c.idx + 1];
        let tl = &t.net.lanes[tgt as usize];
        let deg = w.roads.neighbors(l.end_node).count();
        let around: Vec<String> = tl.cars.iter().filter(|&&o| (t.cars[o].s - c.s).abs() < 30.0).map(|&o| { let x = &t.cars[o]; format!("{:?}@{:.1} v{:.1} {:?} st{:.0}", o, x.s, x.v, x.wait, x.stalled) }).collect();
        println!("   LANECHANGE {:?} at ({:.0},{:.0}) stalled {:.0} lane {lane} kind {:?} s {:.1}/{:.1} end node deg {} next {:?} -> target {tgt} ({} cars) next after {:?} replans {} cooldown {:.0} around {:?}", id, c.pos.x, c.pos.y, c.stalled, l.kind, c.s, l.length, deg, l.next, tl.cars.len(), c.route.get(c.idx + 2), c.replans, c.cooldown, around);
    }
    for &id in standing.iter().filter(|&&id| t.stop_reason(id) == nd_sim::traffic::StopReason::Unexplained).take(10) {
        let c = &t.cars[id];
        let lane = c.route[c.idx];
        let l = &t.net.lanes[lane as usize];
        println!("   UNEXPLAINED {:?} at ({:.0},{:.0}) wait {:?} by {:?} stalled {:.0} lane {lane} conn {} s {:.1}/{:.1} idx {}/{} lane cars {} pos_in_lane {:?}", id, c.pos.x, c.pos.y, c.wait, c.blocked_by, c.stalled, l.is_connector(), c.s, l.length, c.idx, c.route.len(), l.cars.len(), l.cars.iter().position(|&o| o == id));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).cloned().unwrap_or_else(|| "demo".into());
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1440);
    let every: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(120);
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    let mut w = if src == "demo" {
        let mut w = World::new(WorldConfig::default());
        seed(&mut w);
        w
    } else {
        World::from_ron(&std::fs::read_to_string(&src).unwrap()).expect("load")
    };
    report(&w, "start");
    for i in 0..ticks {
        w.tick(&mut minds);
        if (i + 1) % every == 0 {
            report(&w, &format!("+{}", i + 1));
        }
    }

}
