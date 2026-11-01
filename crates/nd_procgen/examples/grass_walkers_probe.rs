//! [r5-fixes] People on the grass (Harvey's flag 2, day 64 at (1026, 695)):
//! load a save, list the people standing or walking near a point, the
//! walk-graph nodes and edges there, and the roads and paths there; then
//! tick on and list them again. Not part of the game.
//! `cargo run --profile fast -p nd_procgen --example grass_walkers_probe -- <save> <x,y> [radius] [ticks]`
use nd_minds::{HybridMind, MockBackend};
use nd_sim::{Activity, Vec2, World};

fn report(w: &World, p: Vec2, r: f32) {
    println!("== day {} {:02}:{:02} tick {}", w.clock.day(), w.clock.hour(), w.clock.minute(), w.clock.tick);
    let mut n = 0;
    for (id, c) in w.citizens.iter().filter(|(_, c)| c.inside.is_none() && c.pos.distance(p) < r) {
        n += 1;
        if n > 25 {
            continue;
        }
        let act = match &c.activity {
            Activity::Traveling { path, idx, purpose, .. } => {
                let near: Vec<String> = path.iter().skip(idx.saturating_sub(2)).take(5).map(|q| format!("({:.1},{:.1})", q.x, q.y)).collect();
                let moved = c.pos.distance(c.prev_pos);
                format!("traveling {purpose:?} idx {idx}/{} moved {moved:.2} path {}", path.len(), near.join(" "))
            }
            a => format!("{}", a.label()),
        };
        println!("  {id:?} at ({:.1},{:.1}) mode {:?} {act}", c.pos.x, c.pos.y, c.mode);
    }
    println!("  {n} people outdoors within {r} m");
    // Riders anywhere farther than a road's width (and 4 m) from every road.
    let off = w.citizens.values().filter(|c| c.inside.is_none() && c.mode == nd_sim::citizens::TravelMode::Bike && matches!(c.activity, Activity::Traveling { .. })).filter(|c| {
        !w.roads.segments_near(c.pos, 40.0).into_iter().any(|s| {
            let (a, b) = w.roads.segment_points(s);
            let t = ((c.pos - a).dot(b - a) / (b - a).length_squared().max(1e-6)).clamp(0.0, 1.0);
            (a + (b - a) * t).distance(c.pos) < w.roads.segment(s).half_width() + 4.0
        })
    }).count();
    let riding = w.citizens.values().filter(|c| c.inside.is_none() && c.mode == nd_sim::citizens::TravelMode::Bike && matches!(c.activity, Activity::Traveling { .. })).count();
    println!("  riders off every road: {off} of {riding}");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (x, y) = args[2].split_once(',').unwrap();
    let p = Vec2::new(x.parse().unwrap(), y.parse().unwrap());
    let r: f32 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(15.0);
    let ticks: u64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
    let mut w = World::from_save_bytes(&std::fs::read(&args[1]).expect("read")).expect("load");
    let g = &w.walk;
    println!("walk nodes within {} m:", r * 2.0);
    for (i, nd) in g.nodes.iter().enumerate().filter(|(_, nd)| nd.pos.distance(p) < r * 2.0) {
        let edges: Vec<String> = g.adj[i].iter().map(|e| format!("->{} ({:.0},{:.0}) len {:.1}{}{}{}", e.to, g.nodes[e.to as usize].pos.x, g.nodes[e.to as usize].pos.y, e.len, if e.interior { " interior" } else { "" }, if e.park { " park" } else { "" }, if e.cross != u32::MAX { " cross" } else { "" })).collect();
        println!("  node {i} ({:.1},{:.1}) lot {:?} level {} deck {}: {}", nd.pos.x, nd.pos.y, nd.lot, nd.level, nd.deck, edges.join("; "));
    }
    println!("road pieces within {} m:", r * 3.0);
    for sid in w.roads.segments_near(p, r * 3.0) {
        let s = w.roads.segment(sid);
        let (a, b) = w.roads.segment_points(sid);
        println!("  seg {} {:?} ({:.0},{:.0})-({:.0},{:.0}) lanes {}+{} kind {:?} tunnel {} viaduct {} bridge {} street {:?}", sid.0, s.road_use(), a.x, a.y, b.x, b.y, s.lanes_fwd, s.lanes_bwd, s.kind, s.tunnel, s.viaduct, s.bridge, w.streets.of(sid).map(|s| s.name.clone()));
    }
    if let Some(q) = args.get(5).and_then(|s| s.split_once(',')).map(|(x, y)| Vec2::new(x.parse().unwrap(), y.parse().unwrap())) {
        for (i, nd) in w.roads.nodes.iter().enumerate().filter(|(_, n)| n.pos.distance(q) < 30.0) {
            let segs: Vec<String> = nd.segments.iter().map(|&sid| { let s = w.roads.segment(sid); let (a, b) = w.roads.segment_points(sid); format!("seg {} alive {} {:?} ({:.0},{:.0})-({:.0},{:.0}) {}+{}", sid.0, s.alive, s.kind, a.x, a.y, b.x, b.y, s.lanes_fwd, s.lanes_bwd) }).collect();
            println!("  road node {i} ({:.1},{:.1}) level {}: {}", nd.pos.x, nd.pos.y, nd.level, segs.join("; "));
        }
    }
    for f in w.walk.footbridges.iter().filter(|f| f.foot[0].distance(Vec2::new(1100.0, 690.0)) < 60.0) {
        let c = &w.walk.crossings[f.crossing as usize];
        let (a, b) = w.roads.segment_points(c.segment);
        println!("  footbridge crossing {} seg {} ({:.0},{:.0})-({:.0},{:.0}) mid ({:.1},{:.1}) foot {:?} top {:?} joined {:?} closed {}", f.crossing, c.segment.0, a.x, a.y, b.x, b.y, c.mid.x, c.mid.y, f.foot, f.top, f.joined, c.closed);
    }
    println!("lots within {} m:", r * 2.0);
    for (lid, l) in w.lots.iter().filter(|(_, l)| l.center.distance(p) < r * 3.0) {
        println!("  lot {lid:?} centre ({:.0},{:.0}) use {:?} building {:?}", l.center.x, l.center.y, l.land_use, l.building);
    }
    report(&w, p, r);
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    for t in 0..ticks {
        w.tick(&mut minds);
        if t % 60 == 59 {
            report(&w, p, r);
        }
    }
}
