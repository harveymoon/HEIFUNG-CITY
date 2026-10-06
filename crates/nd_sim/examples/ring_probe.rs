//! Load a save and explain why cars stand at a junction (yield starvation).
use nd_sim::traffic::{LaneKind, Wait};
use nd_sim::World;
use glam::Vec2;

/// A ring's own road lanes.
fn ring_lanes(w: &World, ring: usize) -> Vec<u32> {
    (0..w.traffic.net.lanes.len() as u32)
        .filter(|&l| matches!(w.traffic.net.lanes[l as usize].kind, LaneKind::Road { segment, .. } if w.roads.ring_of_segment(segment) == Some(ring)))
        .collect()
}

fn overlaps(w: &World, ring: usize) -> (usize, usize, usize) {
    let t = &w.traffic;
    let (mut over, mut cars, mut at0) = (0, 0, 0);
    for l in ring_lanes(w, ring) {
        let lane = &t.net.lanes[l as usize];
        cars += lane.cars.len();
        at0 += lane.cars.iter().filter(|&&c| t.cars[c].s < 0.05).count();
        for pair in lane.cars.windows(2) {
            let (a, b) = (&t.cars[pair[0]], &t.cars[pair[1]]);
            if b.s - b.length() - a.s < -0.5 { over += 1; }
        }
    }
    (cars, over, at0)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut w = World::from_ron(&std::fs::read_to_string(&args[1]).unwrap()).expect("load");
    let at = Vec2::new(args[2].parse().unwrap(), args[3].parse().unwrap());
    let t = &w.traffic;
    if let Some(lanes) = Some(ring_lanes(&w, 2)) {
        let t = &w.traffic;
        let mut over = 0; let mut cars = 0; let mut len = 0.0; let mut short = 0;
        for &l in &lanes {
            let lane = &t.net.lanes[l as usize];
            len += lane.length;
            if lane.length < 6.0 { short += 1; }
            cars += lane.cars.len();
            for pair in lane.cars.windows(2) {
                let (a, b) = (&t.cars[pair[0]], &t.cars[pair[1]]);
                let gap = b.s - b.length() - a.s;
                if gap < 0.0 { over += 1; }
            }
            if lane.cars.len() > 0 { println!("  ring2 lane {l} conn {} len {:.1} cars {} s: {:?}", lane.is_connector(), lane.length, lane.cars.len(), lane.cars.iter().map(|&c| (t.cars[c].s * 10.0).round() / 10.0).collect::<Vec<_>>()); }
        }
        println!("ring2: {} lanes ({} shorter than 6 m), total {:.0} m, {} cars, {} overlapping pairs", lanes.len(), short, len, cars, over);
    }
    println!("at load: r2 {:?} r3 {:?} r0 {:?} r1 {:?}", overlaps(&w, 2), overlaps(&w, 3), overlaps(&w, 0), overlaps(&w, 1));
    if let Ok(l) = std::env::var("LANE") {
        let t = &w.traffic;
        let mut lid: u32 = l.parse().unwrap();
        for _ in 0..4 {
            let lane = &t.net.lanes[lid as usize];
            println!("LANE {lid} conn {} len {:.1} next {:?} prev {:?}", lane.is_connector(), lane.length, lane.next, lane.prev);
            for &c in &lane.cars {
                let x = &t.cars[c];
                println!("   {:?} s {:.2} len {:.1} v {:.2} {:?} st {:.0} entering {} hold {:.0} idx {}/{} next {:?} crashed {} lateral {:.1} passing {}", c, x.s, x.length(), x.v, x.wait, x.stalled, x.entering, x.hold, x.idx, x.route.len(), x.route.get(x.idx + 1..(x.idx + 4).min(x.route.len())), x.crashed(), x.lateral, x.passing);
            }
            let Some(&n) = lane.next.first() else { break };
            lid = n;
        }
    }
    println!("rings in roads: {}", w.roads.rings.len());
    for r in &w.roads.rings { println!("  ring c=({:.0},{:.0}) r={:.0} lanes {}", r.center.x, r.center.y, r.radius, r.lanes); }
    let mut cars: Vec<_> = t.cars.iter().map(|(id, c)| (c.pos.distance(at), id, c)).filter(|x| x.0 < 30.0).collect();
    cars.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (d, id, c) in cars.iter().take(6) {
        let lane = c.route[c.idx];
        let l = &t.net.lanes[lane as usize];
        let seg = match l.kind { LaneKind::Road { segment, .. } => Some(segment), _ => None };
        println!("car {:?} d{:.1} wait {:?} st {:.0} v {:.1} lane {} conn {} s {:.1}/{:.1} seg {:?} ring_seg {:?} circ {} blocked_by {:?}", id, d, c.wait, c.stalled, c.v, lane, l.is_connector(), c.s, l.length, seg, seg.and_then(|s| w.roads.ring_of_segment(s)), t.net.rings.circulating.contains(&lane), c.blocked_by);
        if let Some(&next) = c.route.get(c.idx + 1) {
            let nl = &t.net.lanes[next as usize];
            if let LaneKind::Connector { node, approach } = nl.kind {
                let ringnode = t.net.rings.nodes.contains_key(&node);
                let ii = t.net.node_intersection.get(node.0 as usize).copied();
                println!("   next conn {next} node {:?} ring_node {} ring_of_node {:?} approach {approach} turn {}", node, ringnode, w.roads.ring_of_node(node), nl.turn);
                if let Some(inter) = ii.and_then(|i| t.net.intersections.get(i)) {
                    println!("   inter approaches {} signalized {} pos ({:.0},{:.0})", inter.approaches, inter.signalized, w.roads.node(node).pos.x, w.roads.node(node).pos.y);
                    for &(lid, ap) in &inter.connectors {
                        let cl = &t.net.lanes[lid as usize];
                        if !cl.cars.is_empty() {
                            let o = &t.cars[cl.cars[0]];
                            println!("     busy conn {lid} ap {ap} cars {} first wait {:?} v {:.1} st {:.0}", cl.cars.len(), o.wait, o.v, o.stalled);
                        }
                    }
                }
            }
        }
    }
    let steps: u64 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
    if steps > 0 {
        let mut minds = nd_sim::mind::IdleMind;
        let target = t.cars.iter().map(|(id, c)| (c.pos.distance(at), id)).min_by(|a, b| a.0.total_cmp(&b.0)).map(|x| x.1);
        let ring_lanes: std::collections::HashSet<u32> = ring_lanes(&w, 2).into_iter().collect();
        for k in 0..steps {
            let before: std::collections::HashMap<_, _> = w.traffic.cars.iter().map(|(id, c)| (id, (c.route.get(c.idx).copied(), c.s, c.v, c.wait, c.entering, c.idx, c.route.len()))).collect();
            w.tick(&mut minds);
            for (id, c) in w.traffic.cars.iter() {
                let lane = c.route[c.idx];
                if !ring_lanes.contains(&lane) || c.s > 0.05 || c.entering { continue; }
                let piled = w.traffic.net.lanes[lane as usize].cars.iter().filter(|&&o| o != id && w.traffic.cars[o].s < 4.5).count();
                if piled == 0 { continue; }
                match before.get(&id) {
                    Some(b) if b.0 == Some(lane) && b.1 < 0.05 => {}
                    b => println!("  NEW PILE t+{} {:?} on lane {lane} with {piled} others; before {:?}", k + 1, id, b),
                }
            }
            if k % 15 == 14 {
                let t = &w.traffic;
                let pos = target.and_then(|id| t.cars.get(id)).map(|c| (c.pos, c.wait, c.stalled));
                let stuck = t.cars.values().filter(|c| c.stalled > 300.0 && c.hold <= 0.0 && !c.entering && c.wait != Wait::Signal).count();
                println!("tick +{} target {:?} stuck>5min {} gridlocks {} (cars, overlaps, at s=0) r2 {:?} r3 {:?} r0 {:?}", k + 1, pos, stuck, t.gridlocks().len(), overlaps(&w, 2), overlaps(&w, 3), overlaps(&w, 0));
            }
        }
    }
    let t = &w.traffic;
    let yielders = t.cars.values().filter(|c| c.wait == Wait::Yield && c.stalled > 300.0).count();
    println!("cars yielding > 5 min: {yielders}");
    for g in t.gridlocks() {
        println!("gridlock {} vehicles at {:?}", g.vehicles, g.points.iter().take(6).map(|p| (p.x.round(), p.y.round())).collect::<Vec<_>>());
    }
    let mut groups: std::collections::BTreeMap<(i32, i32, String), (usize, f32)> = Default::default();
    for c in t.cars.values().filter(|c| c.stalled > 300.0 && c.hold <= 0.0 && !c.entering && c.wait != Wait::Signal) {
        let e = groups.entry(((c.pos.x / 80.0).round() as i32 * 80, (c.pos.y / 80.0).round() as i32 * 80, format!("{:?}", c.wait))).or_default();
        e.0 += 1;
        e.1 = e.1.max(c.stalled);
    }
    let mut v: Vec<_> = groups.into_iter().collect();
    v.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    for ((x, y, why), (n, worst)) in v.iter().take(15) {
        println!("stuck near ({x},{y}) {why}: {n} cars, worst {worst:.0}s");
    }
}
