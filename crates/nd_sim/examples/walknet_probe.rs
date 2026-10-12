//! [walk-networks] Load a save and report the walk graph's connected
//! components (nodes, built lots, residents, box), and around given points
//! the road nodes, their streets and the walk nodes with their component.
//! Not part of the game.
//! `walknet_probe <save> [x,y,r ...]`
use glam::Vec2;
use nd_sim::roads::SegmentId;
use nd_sim::World;
use std::collections::HashMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let t = std::time::Instant::now();
    let mut w = World::read_save(&args[1]).expect("load");
    eprintln!("loaded in {:.1}s", t.elapsed().as_secs_f32());
    w.refresh_connectivity();
    let g = w.walk.clone();
    let g = &g;
    // Component -> (nodes, lots, buildings, residents, lo, hi)
    let mut comps: HashMap<u32, (usize, usize, usize, Vec2, Vec2)> = HashMap::new();
    for (i, n) in g.nodes.iter().enumerate() {
        if g.adj[i].is_empty() {
            continue;
        }
        let e = comps.entry(g.comp[i]).or_insert((0, 0, 0, Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)));
        e.0 += 1;
        e.3 = e.3.min(n.pos);
        e.4 = e.4.max(n.pos);
    }
    // Lots by the component of their own loop nodes.
    let mut lot_comp: HashMap<nd_sim::zoning::LotId, u32> = HashMap::new();
    for (i, n) in g.nodes.iter().enumerate() {
        if let Some(l) = n.lot {
            if !g.adj[i].is_empty() {
                lot_comp.entry(l).or_insert(g.comp[i]);
            }
        }
    }
    for (l, c) in &lot_comp {
        let lot = &w.lots[*l];
        if let Some(b) = lot.building.and_then(|b| w.buildings.get(b)) {
            let e = comps.get_mut(c).unwrap();
            e.1 += 1;
            e.2 += b.residents.len();
        }
    }
    let mut v: Vec<_> = comps.into_iter().collect();
    v.sort_by(|a, b| b.1 .1.cmp(&a.1 .1).then(b.1 .0.cmp(&a.1 .0)));
    let with_b = v.iter().filter(|x| x.1 .1 > 0).count();
    let isolated = g.adj.iter().filter(|a| a.is_empty()).count();
    println!("walk nodes {} components {} (with buildings {}) isolated nodes {}", g.nodes.len(), v.len(), with_b, isolated);
    for (c, (n, b, r, lo, hi)) in v.iter().take(40) {
        println!("  comp {c:>7} nodes {n:>6} buildings {b:>4} residents {r:>5} box ({:.0},{:.0})..({:.0},{:.0})", lo.x, lo.y, hi.x, hi.y);
    }
    if std::env::var("SMALL").is_ok() {
        let main = v.first().map(|x| x.0).unwrap_or(0);
        for (c, (n, b, ..)) in v.iter().skip(1) {
            if *b == 0 { continue; }
            let lots: Vec<String> = lot_comp.iter().filter(|(_, x)| *x == c).map(|(l, _)| {
                let lot = &w.lots[*l];
                let bn = lot.building.and_then(|b| w.buildings.get(b)).map(|b| b.name.clone());
                let fs = w.roads.try_segment(lot.front_segment);
                format!("{l:?} {:?} {:?} landlocked {} beach {} front_seg {} ({:?} tunnel {:?} walkable {:?}) center ({:.0},{:.0})", bn, lot.land_use, lot.landlocked, lot.shore.beach, lot.front_segment.0, fs.map(|s| s.kind), fs.map(|s| s.tunnel), fs.map(|s| s.walkable()), lot.center.x, lot.center.y)
            }).collect();
            let mut best = (f32::INFINITY, Vec2::ZERO);
            for i in 0..g.nodes.len() {
                if g.comp[i] != *c || g.adj[i].is_empty() { continue; }
                let p = g.nodes[i].pos;
                for j in g.nodes_in_box(p - Vec2::splat(100.0), p + Vec2::splat(100.0)) {
                    if g.comp[j as usize] == main && g.nodes[j as usize].pos.distance(p) < best.0 { best = (g.nodes[j as usize].pos.distance(p), g.nodes[j as usize].pos); }
                }
            }
            println!("SMALL comp {c} nodes {n} nearest main {:.1} m at {:?}: {:?}", best.0, best.1, lots);
        }
    }
    // Usage stats of special edges.
    let raised = g.adj.iter().flatten().filter(|e| e.raised).count() / 2;
    let under = g.nodes.iter().filter(|n| n.level < -0.5).count();
    println!("raised edges {raised} underground nodes {under} footbridges {} crossings {} (closed {} shut {})", g.footbridges.len(), g.crossings.len(), g.crossings.iter().filter(|c| c.closed).count(), g.crossings.iter().filter(|c| c.shut).count());
    for a in args.iter().skip(2) {
        let p: Vec<f32> = a.split(',').filter_map(|x| x.parse().ok()).collect();
        if p.len() < 2 {
            continue;
        }
        let (c, r) = (Vec2::new(p[0], p[1]), p.get(2).copied().unwrap_or(40.0));
        println!("== around ({:.1},{:.1}) r {r}", c.x, c.y);
        for (ni, node) in w.roads.nodes.iter().enumerate() {
            if node.pos.distance(c) > r || node.segments.is_empty() {
                continue;
            }
            println!(" road node {ni} at ({:.1},{:.1}) level {:.2} street_degree {}", node.pos.x, node.pos.y, node.level, w.roads.street_degree(nd_sim::roads::NodeId(ni as u32)));
            for &s in &node.segments {
                let sg = w.roads.segment(s);
                if !sg.alive {
                    continue;
                }
                let (a, b) = w.roads.segment_points(s);
                println!("   seg {} {:?} lanes {}+{} bridge {} viaduct {} tunnel {} ramp {} ring {} car_only {:?} motorway {} highway {} fronts_nothing {} ({:.0},{:.0})-({:.0},{:.0})", s.0, sg.kind, sg.lanes_fwd, sg.lanes_bwd, sg.bridge, sg.viaduct, sg.tunnel, sg.ramp, sg.ring, sg.car_only, sg.is_motorway(), sg.is_highway(), sg.fronts_nothing(), a.x, a.y, b.x, b.y);
            }
        }
        for i in g.nodes_in_box(c - Vec2::splat(r), c + Vec2::splat(r)) {
            let n = &g.nodes[i as usize];
            if n.lot.is_some() || n.pos.distance(c) > r {
                continue;
            }
            let edges: Vec<String> = g.adj[i as usize].iter().map(|e| {
                let x = if e.cross != nd_sim::walk::NO_CROSSING { let k = &g.crossings[e.cross as usize]; format!("x{:?}{}{}seg{}", k.kind, if k.closed { "C" } else { "" }, if k.shut { "S" } else { "" }, k.segment.0) } else { String::new() };
                format!("{}{}", e.to, x)
            }).collect();
            println!("  wn {i} ({:.1},{:.1}) lvl {:.1} comp {} -> {:?}", n.pos.x, n.pos.y, n.level, g.comp[i as usize], edges);
        }
    }
    if let Ok(v) = std::env::var("ROUTE") {
        let (a, b) = v.split_once(',').unwrap();
        let (a, b) = (nd_sim::roads::NodeId(a.parse().unwrap()), nd_sim::roads::NodeId(b.parse().unwrap()));
        if let Some(path) = w.roads.path(a, b) {
            for p in path.windows(2) {
                let Some((_, sid)) = w.roads.neighbors(p[0]).find(|(n, _)| *n == p[1]) else { continue };
                let sg = w.roads.segment(sid);
                let at = w.roads.node(p[0]).pos;
                let mut cs: Vec<u32> = g.nodes_in_box(at - Vec2::splat(25.0), at + Vec2::splat(25.0)).into_iter().filter(|&i| g.nodes[i as usize].lot.is_none() && !g.adj[i as usize].is_empty()).map(|i| g.comp[i as usize]).collect();
                cs.sort();
                cs.dedup();
                println!("ROUTE node {} ({:.0},{:.0}) comps {:?} -> seg {} {:?} {}+{} bridge {} viaduct {} tunnel {} ramp {} car_only {:?} fronts_nothing {} deg {}", p[0].0, at.x, at.y, cs, sid.0, sg.kind, sg.lanes_fwd, sg.lanes_bwd, sg.bridge, sg.viaduct, sg.tunnel, sg.ramp, sg.car_only, sg.fronts_nothing(), w.roads.street_degree(p[0]));
            }
        }
    }
    if let Ok(v) = std::env::var("FOOT") {
        // BFS over road nodes by ways a walker may use (streets with
        // pavements and paths; tunnels too when FOOT_TUNNEL is set).
        let (a, b) = v.split_once(',').unwrap();
        let (a, b): (u32, u32) = (a.parse().unwrap(), b.parse().unwrap());
        let tun = std::env::var("FOOT_TUNNEL").is_ok();
        let mut prev: Vec<Option<(u32, SegmentId)>> = vec![None; w.roads.nodes.len()];
        let mut seen = vec![false; w.roads.nodes.len()];
        let mut q = std::collections::VecDeque::from([a]);
        seen[a as usize] = true;
        while let Some(u) = q.pop_front() {
            if u == b { break; }
            for &s in &w.roads.nodes[u as usize].segments {
                let sg = w.roads.segment(s);
                if !sg.alive || sg.is_rail() || sg.is_motorway() || (sg.tunnel && !tun) { continue; }
                let o = if sg.a.0 == u { sg.b.0 } else { sg.a.0 };
                if !seen[o as usize] { seen[o as usize] = true; prev[o as usize] = Some((u, s)); q.push_back(o); }
            }
        }
        println!("FOOT reachable {}", seen[b as usize]);
        let mut cur = b;
        let mut steps = Vec::new();
        while let Some((p, s)) = prev[cur as usize] { steps.push((p, s)); cur = p; }
        steps.reverse();
        let mut last: Vec<u32> = Vec::new();
        for (p, sid) in steps {
            let sg = w.roads.segment(sid);
            let at = w.roads.node(nd_sim::roads::NodeId(p)).pos;
            let mut cs: Vec<u32> = g.nodes_in_box(at - Vec2::splat(25.0), at + Vec2::splat(25.0)).into_iter().filter(|&i| g.nodes[i as usize].lot.is_none() && !g.adj[i as usize].is_empty()).map(|i| g.comp[i as usize]).collect();
            cs.sort();
            cs.dedup();
            if cs != last || std::env::var("FOOT_ALL").is_ok() {
                println!("FOOT node {} ({:.0},{:.0}) comps {:?} -> seg {} {:?} {}+{} bridge {} tunnel {} lv {:.1}/{:.1} car_only {:?} deg {} segs {}", p, at.x, at.y, cs, sid.0, sg.kind, sg.lanes_fwd, sg.lanes_bwd, sg.bridge, sg.tunnel, w.roads.node(sg.a).level, w.roads.node(sg.b).level, sg.car_only, w.roads.street_degree(nd_sim::roads::NodeId(p)), w.roads.nodes[p as usize].segments.len());
                last = cs;
            }
        }
    }
    if let Ok(v) = std::env::var("NEAR") {
        let p: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
        let c = Vec2::new(p[0], p[1]);
        for (sid, sg) in w.roads.alive_segments() {
            let (a, b) = w.roads.segment_points(sid);
            let (_, q) = nd_sim::geom::project_on_segment(c, a, b);
            if q.distance(c) < p[2] {
                println!("NEAR seg {} {:?} {}+{} tunnel {} bridge {} lv {:.1}/{:.1} ({:.0},{:.0})-({:.0},{:.0}) fronts_nothing {} car_only {:?}", sid.0, sg.kind, sg.lanes_fwd, sg.lanes_bwd, sg.tunnel, sg.bridge, w.roads.node(sg.a).level, w.roads.node(sg.b).level, a.x, a.y, b.x, b.y, sg.fronts_nothing(), sg.car_only);
            }
        }
    }
    if let Ok(v) = std::env::var("STUCK") {
        let p: Vec<f32> = v.split(',').filter_map(|x| x.parse().ok()).collect();
        let (c, r, ticks, every) = (Vec2::new(p[0], p[1]), p[2], p[3] as usize, p.get(4).copied().unwrap_or(120.0) as usize);
        let mut minds = nd_minds::HybridMind::new(Box::new(nd_minds::MockBackend::default()));
        for t in 0..=ticks {
            if t % every == 0 && std::env::var("COMPACT").is_ok() {
                let stalled = w.traffic.cars.iter().filter(|(_, car)| car.pos.distance(c) < r && car.v < 0.5 && car.stalled > 300.0).count();
                let sirens = w.traffic.cars.iter().filter(|(_, car)| car.pos.distance(c) < r && car.siren && car.stalled > 300.0).count();
                let mut spots: HashMap<(i32, i32), usize> = HashMap::new();
                for z in w.citizens.values() {
                    if z.pos.distance(c) < r && z.inside.is_none() && z.mode == nd_sim::citizens::TravelMode::Bike && z.prev_pos == z.pos {
                        *spots.entry(((z.pos.x * 10.0) as i32, (z.pos.y * 10.0) as i32)).or_default() += 1;
                    }
                }
                let still: usize = spots.values().sum();
                println!("COMPACT tick {t} stalled>300 {stalled} sirens {sirens} still bikes {still} spots {:?}", spots.iter().filter(|x| *x.1 > 1).collect::<Vec<_>>());
            } else if t % every == 0 {
                println!("STUCK tick {} clock {}", t, w.clock.tick);
                for (id, car) in w.traffic.cars.iter() {
                    if car.pos.distance(c) < r && car.v < 0.5 {
                        println!("   car {id:?} at ({:.1},{:.1}) stalled {:.0} siren {} reason {:?}", car.pos.x, car.pos.y, car.stalled, car.siren, w.traffic.stop_reason(id));
                    }
                }
                for (id, z) in w.citizens.iter() {
                    if z.pos.distance(c) < r && z.inside.is_none() {
                        let leg = if let nd_sim::Activity::Traveling { path, idx, .. } = &z.activity {
                            let a = if *idx > 0 { path.get(idx - 1).copied() } else { None };
                            format!("leg {:?}->{:?} idx {}/{} cross {:?} start {:?} end {:?}", a, path.get(*idx), idx, path.len(), a.zip(path.get(*idx)).and_then(|(a, b)| w.walk.crossing_between(a, *b)), path.first(), path.last())
                        } else { format!("{:?}", std::mem::discriminant(&z.activity)) };
                        println!("   cit {id:?} {:?} at ({:.1},{:.1}) {}", z.mode, z.pos.x, z.pos.y, leg);
                    }
                }
            }
            if t == ticks && std::env::var("XDUMP").is_ok() {
                for (k, x) in w.walk.crossings.iter().enumerate() {
                    if x.mid.distance(c) > r { continue; }
                    let (a, b) = (w.walk.nodes[x.a as usize].pos, w.walk.nodes[x.b as usize].pos);
                    let occ: Vec<String> = w.peds.occupants.iter().filter(|o| o.0 == k as u32).map(|o| { let z = &w.citizens[o.1]; format!("{:?}@({:.1},{:.1})", z.mode, z.pos.x, z.pos.y) }).collect();
                    let mut occ2 = occ.clone(); occ2.sort(); occ2.dedup();
                    println!("X {k} {:?} ring {} marked {} closed {} shut {} seg {} node {:?} ({:.1},{:.1})-({:.1},{:.1}) occ {} {:?}", x.kind, x.ring, x.marked(), x.closed, x.shut, x.segment.0, x.node.map(|n| n.0), a.x, a.y, b.x, b.y, occ.len(), occ2);
                }
                for (ni, node) in w.roads.nodes.iter().enumerate() {
                    if node.pos.distance(c) < r && !node.segments.is_empty() {
                        println!("RN {ni} ({:.1},{:.1}) segs {:?}", node.pos.x, node.pos.y, node.segments.iter().filter(|s| w.roads.segment(**s).alive).map(|s| s.0).collect::<Vec<_>>());
                    }
                }
                // A few stuck bikes' legs.
                let mut n = 0;
                for (id, z) in w.citizens.iter() {
                    if n < 12 && z.pos.distance(c) < r && z.mode == nd_sim::citizens::TravelMode::Bike && z.prev_pos == z.pos {
                        if let nd_sim::Activity::Traveling { path, idx, .. } = &z.activity {
                            n += 1;
                            let a = if *idx > 0 { path[idx - 1] } else { z.pos };
                            println!("BIKE {id:?} at ({:.2},{:.2}) leg ({:.1},{:.1})->({:.1},{:.1}) next {:?}", z.pos.x, z.pos.y, a.x, a.y, path[*idx].x, path[*idx].y, path.get(idx + 1));
                        }
                    }
                }
            }
            if t < ticks { w.tick(&mut minds); }
        }
    }
    if let Ok(v) = std::env::var("TUNNEL_USE") {
        // Ticks of people on foot or bike down in a tunnel, and who.
        let ticks: usize = v.parse().unwrap_or(1440);
        let mut minds = nd_minds::HybridMind::new(Box::new(nd_minds::MockBackend::default()));
        let (mut walk_ticks, mut bike_ticks) = (0u64, 0u64);
        let mut who: std::collections::HashSet<nd_sim::CitizenId> = Default::default();
        let mut by_seg: std::collections::BTreeMap<u32, u64> = Default::default();
        for _ in 0..ticks {
            w.tick(&mut minds);
            for (id, z) in w.citizens.iter() {
                let nd_sim::Activity::Traveling { path, idx, .. } = &z.activity else { continue };
                if *idx == 0 || *idx >= path.len() || z.inside.is_some() { continue; }
                if !matches!(z.mode, nd_sim::citizens::TravelMode::Walk | nd_sim::citizens::TravelMode::Bike) { continue; }
                if w.leg_in_tunnel(path[idx - 1], path[*idx], z.pos, z.mode == nd_sim::citizens::TravelMode::Bike) {
                    if z.mode == nd_sim::citizens::TravelMode::Walk { walk_ticks += 1 } else { bike_ticks += 1 }
                    who.insert(id);
                    if let Some(sid) = w.roads.segments_near(z.pos, 6.0).into_iter().find(|&s| w.roads.segment(s).tunnel) {
                        *by_seg.entry(sid.0).or_default() += 1;
                    }
                }
            }
        }
        println!("TUNNEL_USE over {ticks} ticks: walker-ticks {walk_ticks} bike-ticks {bike_ticks} distinct people {}", who.len());
        let mut v: Vec<_> = by_seg.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        for (sid, n) in v.iter().take(12) {
            let sg = w.roads.segment(SegmentId(*sid));
            println!("   seg {sid} {:?} {}+{}: {n} person-ticks", sg.kind, sg.lanes_fwd, sg.lanes_bwd);
        }
        let (fails, alerts) = (w.connectivity.failed.len(), w.connectivity.alerts.len());
        println!("   failed trips kept {fails} alerts {alerts}");
    }
    if let Ok(v) = std::env::var("SEG") {
        for id in v.split(',').filter_map(|x| x.parse::<u32>().ok()) {
            let sid = SegmentId(id);
            let Some(sg) = w.roads.try_segment(sid) else { continue };
            println!("SEG {id} {:?} alive {} links {:?}", sg.kind, sg.alive, w.segment_links(sid));
        }
    }
}
