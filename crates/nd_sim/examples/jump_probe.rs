//! Load a save, run it on and report cars that move further in a tick than
//! their speed allows (a "teleport"), with the lanes they jumped across.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::traffic::LaneKind;
use nd_sim::World;
use std::collections::{BTreeMap, HashMap};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut w = World::from_ron(&std::fs::read_to_string(&args[1]).unwrap()).expect("load");
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(300);
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    let dt = w.config.traffic_seconds_per_tick;
    let mut by_seg: BTreeMap<String, usize> = BTreeMap::new();
    let mut shown = 0;
    for _ in 0..ticks {
        let before: HashMap<_, _> = w.traffic.cars.iter().map(|(id, c)| (id, (c.pos, c.v, c.route.get(c.idx).copied(), c.s))).collect();
        w.tick(&mut minds);
        for (id, c) in w.traffic.cars.iter() {
            let Some(&(p0, v0, l0, s0)) = before.get(&id) else { continue };
            let d = p0.distance(c.pos);
            let allowed = v0.max(c.v) * dt * 1.2 + 6.0;
            if d > allowed && d > 15.0 {
                let lane = c.route.get(c.idx).copied().unwrap_or(0);
                let seg = |l: u32| match w.traffic.net.lanes[l as usize].kind { LaneKind::Road { segment, .. } => format!("{segment:?}"), _ => "conn".into() };
                *by_seg.entry(l0.map(seg).unwrap_or_default()).or_default() += 1;
                if shown < 25 {
                    shown += 1;
                    let ln = &w.traffic.net.lanes[lane as usize];
                    println!("t{} {:?} moved {:.0} m (v {:.1}->{:.1}) lane {:?}@{:.0} -> {}@{:.0} len {:.0} limit {:.0} from ({:.0},{:.0}) to ({:.0},{:.0}) seg {}", w.clock.tick, id, d, v0, c.v, l0, s0, lane, c.s, ln.length, ln.speed_limit, p0.x, p0.y, c.pos.x, c.pos.y, seg(lane));
                }
            }
        }
    }
    println!("jumps by starting segment: {by_seg:?}");
}
