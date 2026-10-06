//! Load a save and list the ramps (one-way non-highway streets) with the
//! segments they join, their levels and heights, and the lanes on them.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::traffic::LaneKind;
use nd_sim::World;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut w = World::from_ron(&std::fs::read_to_string(&args[1]).unwrap()).expect("load");
    let r = &w.roads;
    for (sid, s) in r.alive_segments() {
        if !nd_sim::roads::is_ramp_spec(s.kind, s.lanes_fwd, s.lanes_bwd) {
            continue;
        }
        let (a, b) = r.segment_points(sid);
        let na = r.node(s.a);
        let nb = r.node(s.b);
        let fmt_nb = |n: &nd_sim::roads::Node| {
            n.segments.iter().filter(|&&o| o != sid).map(|&o| {
                let os = r.segment(o);
                format!("{}:{}/{}{}{}", o.0, os.lanes_fwd, os.lanes_bwd, if os.is_highway() { "H" } else { "" }, if os.viaduct { "V" } else { "" })
            }).collect::<Vec<_>>().join(",")
        };
        println!("ramp {} {}/{} ({:.0},{:.0})L{:.1}h{:.1} -> ({:.0},{:.0})L{:.1}h{:.1} len {:.0} via {} bridge {} | a: [{}] b: [{}]",
            sid.0, s.lanes_fwd, s.lanes_bwd, a.x, a.y, na.level, w.road_height(sid, 0.0), b.x, b.y, nb.level, w.road_height(sid, 1.0), a.distance(b), s.viaduct, s.bridge, fmt_nb(na), fmt_nb(nb));
        for (i, l) in w.traffic.net.lanes.iter().enumerate() {
            if let LaneKind::Road { segment, .. } = l.kind {
                if segment == sid {
                    println!("   lane {} len {:.1} limit {:.1} pts {} next {:?}", i, l.length, l.speed_limit, l.points.len(), l.next);
                }
            }
        }
    }
    // Optional: a segment id to watch: print its neighbourhood and the
    // poses of every car on it for a number of ticks.
    let Some(watch) = args.get(2).and_then(|a| a.parse::<u32>().ok()) else { return };
    let ticks: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(30);
    let sid = nd_sim::roads::SegmentId(watch);
    let s = w.roads.segment(sid).clone();
    for n in [s.a, s.b] {
        let node = w.roads.node(n);
        println!("node {} ({:.0},{:.0}) level {:.1}: {:?}", n.0, node.pos.x, node.pos.y, node.level, node.segments.iter().map(|&o| { let os = w.roads.segment(o); let (a, b) = w.roads.segment_points(o); format!("{}:{}/{} ({:.0},{:.0})->({:.0},{:.0}) h{:.1}..{:.1} V{} B{} T{}", o.0, os.lanes_fwd, os.lanes_bwd, a.x, a.y, b.x, b.y, w.road_height(o, 0.0), w.road_height(o, 1.0), os.viaduct, os.bridge, os.tunnel) }).collect::<Vec<_>>());
    }
    let lanes: Vec<u32> = w.traffic.net.lanes.iter().enumerate().filter(|(_, l)| matches!(l.kind, LaneKind::Road { segment, .. } if segment == sid)).map(|(i, _)| i as u32).collect();
    for &l in &lanes {
        let ln = &w.traffic.net.lanes[l as usize];
        println!("lane {l} len {:.1} limit {:.1} pts {:?} next {:?} prev {:?}", ln.length, ln.speed_limit, ln.points, ln.next, ln.prev);
    }
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    for _ in 0..ticks {
        w.tick(&mut minds);
        for (id, c) in w.traffic.cars.iter() {
            let on = c.route.get(c.idx).is_some_and(|l| lanes.contains(l)) || c.route.get(c.idx.saturating_sub(1)).is_some_and(|l| lanes.contains(l));
            if !on {
                continue;
            }
            let poses: Vec<String> = c.drive.poses.iter().map(|(p, _)| format!("({:.0},{:.0})", p.x, p.y)).collect();
            let draw: Vec<String> = (0..=8).map(|k| { let (p, _) = c.pose_at(k as f32 / 8.0); format!("({:.0},{:.0})", p.x, p.y) }).collect();
            println!("t{} {:?} lane {} s {:.1} v {:.1} wait {:?} h {:.1} poses {} draw {}", w.clock.tick, id, c.route[c.idx], c.s, c.v, c.wait, w.car_height(c), poses.join(" "), draw.join(" "));
        }
    }
}
