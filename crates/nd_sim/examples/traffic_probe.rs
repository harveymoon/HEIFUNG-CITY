//! Headless traffic probe: the demo district run to a given day, with
//! traffic metrics and a breakdown of slow vehicles. Not part of the game.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::traffic::LaneKind;
use nd_sim::{Gesture, LandUse, Vec2 as SVec2, World, WorldConfig};
use std::collections::{BTreeMap, HashMap};

fn seed(world: &mut World) {
    world.drain_events();
    world.start_project();
    for i in -2..=2 {
        let c = i as f32 * 140.0;
        world.add_gesture(Gesture::Road { points: vec![SVec2::new(-280.0, c), SVec2::new(280.0, c)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: nd_sim::roads::RoadKind::Street });
        world.add_gesture(Gesture::Road { points: vec![SVec2::new(c, -280.0), SVec2::new(c, 280.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: nd_sim::roads::RoadKind::Street });
    }
    let cell = |cx: f32, cy: f32| vec![SVec2::new(cx - 65.0, cy - 65.0), SVec2::new(cx + 65.0, cy - 65.0), SVec2::new(cx + 65.0, cy + 65.0), SVec2::new(cx - 65.0, cy + 65.0)];
    let plan = [
        (-210.0, -210.0, LandUse::Industrial), (-70.0, -210.0, LandUse::Industrial), (70.0, -210.0, LandUse::Residential), (210.0, -210.0, LandUse::Residential),
        (-210.0, -70.0, LandUse::Commercial), (-70.0, -70.0, LandUse::Entertainment), (70.0, -70.0, LandUse::Commercial), (210.0, -70.0, LandUse::Residential),
        (-210.0, 70.0, LandUse::Residential), (-70.0, 70.0, LandUse::Park), (70.0, 70.0, LandUse::Civic), (210.0, 70.0, LandUse::Commercial),
        (-210.0, 210.0, LandUse::Residential), (-70.0, 210.0, LandUse::Residential), (70.0, 210.0, LandUse::Entertainment), (210.0, 210.0, LandUse::Residential),
    ];
    for (x, y, lu) in plan {
        world.add_gesture(Gesture::Zone { polygon: cell(x, y), land_use: lu });
    }
    world.add_gesture(Gesture::Road { points: vec![SVec2::new(-420.0, nd_sim::rail::TRUNK_Y), SVec2::new(-330.0, -410.0), SVec2::new(-280.0, -348.0), SVec2::new(-110.0, -348.0), SVec2::new(-70.0, -324.0), SVec2::new(150.0, -324.0)], lanes_fwd: 0, lanes_bwd: 0, curved: false, kind: nd_sim::roads::RoadKind::Rail });
    world.implement();
    world.economy.treasury = world.economy.treasury.max(150_000.0);
    // The district's way in: a street north from the grid onto the highway.
    world.link_to_highway(SVec2::new(0.0, 280.0));
    use nd_sim::RailFacility;
    let _ = world.place_rail_facility(RailFacility::FreightYard, SVec2::new(-200.0, -330.0));
    let _ = world.place_rail_facility(RailFacility::Station, SVec2::new(80.0, -310.0));
    let lot = world.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial && l.building.is_none() && l.area() >= 600.0).max_by(|a, b| a.1.area().total_cmp(&b.1.area())).map(|(id, _)| id);
    if let Some(lot) = lot {
        let _ = world.build_power(lot, nd_sim::PowerPlantKind::Coal);
    }
    let lot = world.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial && l.building.is_none() && l.car_access() && l.area() >= nd_sim::transit::DEPOT_MIN_AREA).max_by(|a, b| a.1.area().total_cmp(&b.1.area())).map(|(id, _)| id);
    if let Some(lot) = lot {
        let _ = world.build_depot(lot);
    }
    let line = world.add_bus_route();
    for p in [(-70.0, -136.0), (70.0, -136.0), (136.0, -70.0), (136.0, 70.0), (70.0, 136.0), (-70.0, 136.0), (-136.0, 70.0), (-136.0, -70.0)] {
        let _ = world.add_bus_stop(line, SVec2::new(p.0, p.1));
    }
    world.set_route_buses(line, 2);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let to_day: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(6);
    let detail = args.iter().any(|a| a == "-v");
    let at_hour: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(12.0);
    let mut peak: BTreeMap<u64, (usize, u64)> = BTreeMap::new();
    let mut peak_stall: BTreeMap<u64, (usize, u64)> = BTreeMap::new();
    let wseed: Option<u64> = std::env::var("SEED").ok().and_then(|s| s.parse().ok());
    let mut world = World::new(match wseed { Some(sd) => WorldConfig { seed: sd, ..WorldConfig::default() }, None => WorldConfig::default() });
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    seed(&mut world);
    let t0 = std::time::Instant::now();
    let mut last_takeover: Option<(u32, &str)> = None;
    let mut last: HashMap<nd_sim::CarId, f32> = HashMap::new();
    let mut stalled: HashMap<nd_sim::CarId, f32> = HashMap::new();
    let mut lw: HashMap<nd_sim::CarId, f32> = HashMap::new();
    let mut yw: HashMap<nd_sim::CarId, f32> = HashMap::new();
    let mut kind_of: HashMap<nd_sim::CarId, &'static str> = HashMap::new();
    let mut by_kind: BTreeMap<String, (u32, u32, f32)> = BTreeMap::new();
    // Per day: (completed, timed out, speed sum, speed samples, stuck samples, samples)
    let mut per_day: BTreeMap<u64, [f64; 15]> = BTreeMap::new();
    while world.clock.day() < to_day || (world.clock.minute_of_day() as f32) < at_hour * 60.0 {
        world.tick(&mut minds);
        let day = world.clock.day();
        // [driving] Crashes, drink and takeovers, once a day and as they happen.
        let tk = world.takeover().map(|t| (t.id, t.stage.label(), t.participants(), t.crowd.len()));
        if tk.map(|t| (t.0, t.1)) != last_takeover {
            println!("day {day} {:02}:{:02} takeover {:?} (cars, crowd so far)", world.clock.hour(), world.clock.minute_of_day() % 60, tk);
            last_takeover = tk.map(|t| (t.0, t.1));
        }
        if world.clock.minute_of_day() == 1439 {
            let d = &world.driving;
            let styles = world.traffic.cars.values().fold(BTreeMap::<&str, u32>::new(), |mut m, c| { *m.entry(c.drive.profile.label()).or_default() += 1; m });
            let causes = d.recent.iter().filter(|r| r.tick + 1440 > world.clock.tick).fold(BTreeMap::<String, (u32, u32, u32)>::new(), |mut m, r| { let e = m.entry(r.cause.label().to_string()).or_default(); e.0 += 1; e.1 += r.drunk as u32; e.2 += (r.severity >= nd_sim::traffic::BAD_CRASH) as u32; m });
            println!("day {day} crash causes (n, drunk, bad): {causes:?}");
            println!("day {day} driving: crashes {} injuries {} drunk trips {} red runs {} towed {} takeovers {} last {:?} | styles now {:?}", d.crashes_today, d.injuries_today, d.drunk_trips_today, d.red_runs_today, d.towed_total, d.takeovers.total, d.takeovers.history.last().map(|r| (r.participants, r.crowd, r.arrests, r.police)), styles);
        }
        let e = per_day.entry(day).or_default();
        let nst = stalled.values().filter(|&&v| v > 180.0).count();
        let ps = peak_stall.entry(day).or_default();
        if nst > ps.0 { *ps = (nst, world.clock.minute_of_day()); }
        let lmax = world.traffic.cars.values().map(|c| c.turn_wait).fold(0.0f32, f32::max);
        if lmax > 600.0 && args.iter().any(|a| a == "-stop") { break; }
        let pk = peak.entry(day).or_default();
        if world.traffic.cars.len() > pk.0 { *pk = (world.traffic.cars.len(), world.clock.hour()); }
        for (id, age) in last.iter() {
            if !world.traffic.cars.contains_key(*id) {
                if *age + 4.0 > 1800.0 { e[1] += 1.0 } else { e[0] += 1.0 }
                let k = by_kind.entry(kind_of[id].to_string()).or_default();
                if *age + 4.0 > 1800.0 { k.1 += 1 } else { k.0 += 1; k.2 += *age }
            }
        }
        last.clear();
        stalled.retain(|id, _| world.traffic.cars.contains_key(*id));
        lw.retain(|id, _| world.traffic.cars.contains_key(*id));
        yw.retain(|id, _| world.traffic.cars.contains_key(*id));
        for (id, c) in world.traffic.cars.iter() {
            last.insert(id, c.age);
            kind_of.insert(id, if c.bus.is_some() { "bus" } else if let Some(cg) = c.cargo { if world.buildings.get(cg.from).is_none() && cg.price == 0.0 { "truck(site import)" } else if world.buildings.get(cg.from).is_none() { "truck(import)" } else { "truck" } } else { "car" });
            e[2] += c.v as f64;
            e[3] += 1.0;
            if c.v < 0.5 && c.age > 900.0 && c.hold <= 0.0 { e[4] += 1.0 }
            let st = stalled.entry(id).or_default();
            if c.v < 0.5 && c.hold <= 0.0 { *st += 4.0 } else { *st = 0.0 }
            if *st > 180.0 { e[6] += 1.0 }
            let in_lane = world.traffic.net.lanes[c.route[c.idx] as usize].cars.contains(&id);
            let red = c.route.get(c.idx + 1).is_some_and(|&n| { let l = &world.traffic.net.lanes[n as usize]; l.is_connector() && !l.green });
            if *st > 180.0 && in_lane && !red { e[9] += 1.0 }
            if c.v >= 0.5 { e[7] += c.v as f64; e[8] += 1.0 }
            let here = &world.traffic.net.lanes[c.route[c.idx] as usize];
            if here.is_connector() && here.turn < 0 {
                if let Some(wv) = lw.remove(&id) { e[10] += wv as f64; e[11] += 1.0; e[12] = e[12].max(wv as f64); }
                if let Some(wv) = yw.remove(&id) { e[13] += wv as f64; e[14] = e[14].max(wv as f64); }
            } else if !here.is_connector() && c.route.get(c.idx + 1).is_some_and(|&n| { let l = &world.traffic.net.lanes[n as usize]; l.is_connector() && l.turn < 0 }) {
                let x = lw.entry(id).or_default();
                if c.v < 0.5 { *x += 4.0 }
                let y = yw.entry(id).or_default();
                if c.v < 0.5 && c.wait == nd_sim::traffic::Wait::Yield { *y += 4.0 }
            }
        }
        e[5] += 1.0;
    }
    eprintln!("ran to day {} {:02}:{:02} in {:.1}s", world.clock.day(), world.clock.hour(), world.clock.tick % 60, t0.elapsed().as_secs_f32());
    println!("day  done/h  timeout/h  mean_v(km/h)  moving_v  vehicles  stuck_old  stalled>180s  stuck_rule  left_n  left_mean  left_max  yield_mean  yield_max");
    for (d, e) in &per_day {
        let hours = e[5] / 60.0;
        println!("{d:3}  {:6.1}  {:9.1}  {:12.1}  {:8.1}  {:8.1}  {:9.1}  {:9.1}  {:9.1}  {:6}  {:9.1}  {:8.0}  {:10.1}  {:9.0}", e[0] / hours, e[1] / hours, e[2] / e[3].max(1.0) * 3.6, e[7] / e[8].max(1.0) * 3.6, e[3] / e[5], e[4] / e[5], e[6] / e[5], e[9] / e[5], e[11], e[10] / e[11].max(1.0), e[12], e[13] / e[11].max(1.0), e[14]);
    }
    for (k, (d, t, a)) in &by_kind { println!("{k:14} done {d:6} timed out {t:5} mean trip {:.0}s", a / (*d).max(1) as f32); }
    println!("peak vehicles per day (count, hour): {:?}", peak);
    println!("peak stalled>180 per day (count, minute): {:?}", peak_stall);
    if args.iter().any(|a| a == "-l") {
        let t = &world.traffic;
        let mut rows: Vec<(f32, String)> = Vec::new();
        for (id, c) in t.cars.iter() {
            if c.turn_wait < 120.0 { continue; }
            let lane = &t.net.lanes[c.route[c.idx] as usize];
            let k = lane.cars.iter().position(|&x| x == id).map_or(-1, |k| k as i64);
            let head = lane.cars.last() == Some(&id);
            let node = lane.end_node;
            let inter = t.net.node_intersection.get(node.0 as usize).and_then(|&i| t.net.intersections.get(i));
            let p = world.roads.node(node).pos;
            let next = c.route.get(c.idx+1).map(|&n| &t.net.lanes[n as usize]);
            let exit = c.route.get(c.idx+2).map(|&n| &t.net.lanes[n as usize]);
            rows.push((c.turn_wait, format!("wait {:.0} at ({:.0},{:.0}) pos {k}/{} head {head} why {:?} arrows {} conn green {:?} exit cars {:?}/{:.0} inter sig {:?} arrow {:?} phase {:?} timer {:?}", c.turn_wait, p.x, p.y, lane.cars.len(), c.wait, nd_sim::traffic::arrows_label(lane.arrows), next.map(|n| n.green), exit.map(|e| e.cars.len()), exit.map_or(0.0, |e| e.length), inter.map(|i| i.signalized), inter.map(|i| i.arrow), inter.map(|i| i.phase), inter.map(|i| i.timer))));
        }
        rows.sort_by(|a, b| b.0.total_cmp(&a.0));
        for r in rows.iter().take(25) { println!("{}", r.1); }
    }
    // Snapshot breakdown of stuck vehicles now.
    let t = &world.traffic;
    let mut cats: BTreeMap<String, u32> = BTreeMap::new();
    let mut nodes: BTreeMap<String, u32> = BTreeMap::new();
    let mut n = 0;
    for (id, c) in t.cars.iter() {
        if !(stalled.get(&id).copied().unwrap_or(0.0) > 180.0) {
            continue;
        }
        n += 1;
        let kind = if c.bus.is_some() { "bus" } else if let Some(cg) = c.cargo { if world.buildings.get(cg.from).is_none() && cg.price == 0.0 { "truck(site import)" } else if world.buildings.get(cg.from).is_none() { "truck(import)" } else { "truck" } } else { "car" };
        let lid = c.route[c.idx];
        let lane = &t.net.lanes[lid as usize];
        let Some(k) = lane.cars.iter().position(|&x| x == id) else {
            *cats.entry(format!("{kind:14} entering")).or_default() += 1;
            if detail && n < 40 {
                let mut gaps: Vec<String> = lane.cars.iter().map(|&o| format!("{:.0}@{:.1}", t.cars[o].s, t.cars[o].v)).collect();
                gaps.truncate(30);
                println!("  entering lane {lid} (n{} len {:.0} conn {}) s {:.1} stalled {:.0} cars: {}", lane.end_node.0, lane.length, lane.is_connector(), c.s, stalled[&id], gaps.join(" "));
            }
            continue
        };
        let leader = lane.cars.get(k + 1).map(|&l| &t.cars[l]);
        let next = c.route.get(c.idx + 1).map(|&l| &t.net.lanes[l as usize]);
        let why = if lane.is_connector() {
            if leader.is_some() { "in junction, queued" } else { "in junction, exit blocked" }
        } else if let Some(l) = leader {
            if l.s - c.s - l.length() < 12.0 { "queued behind" } else { "gap ahead but stopped" }
        } else if c.idx + 1 == c.route.len() {
            if (c.s_end - c.s).abs() < 3.0 { "at destination" } else { "last lane, no leader" }
        } else if next.is_some_and(|n| n.is_connector() && !n.green) {
            "red light"
        } else if lane.length - c.s < 6.0 {
            "at junction, waits (exit full / yield)"
        } else {
            "other"
        };
        *cats.entry(format!("{kind:14} {why}")).or_default() += 1;
        let node = lane.end_node;
        let p = world.roads.node(node).pos;
        *nodes.entry(format!("node {:4} ({:6.0},{:6.0})", node.0, p.x, p.y)).or_default() += 1;
        if detail {
            println!("  {kind} age {:.0} lane {lid} conn {} s {:.1}/{:.1} idx {}/{} pos {:.0},{:.0}", c.age, lane.is_connector(), c.s, lane.length, c.idx, c.route.len(), c.pos.x, c.pos.y);
        }
    }
    println!("stuck now: {n} of {} vehicles", t.cars.len());
    for (k, v) in &cats { println!("  {v:4}  {k}"); }
    let mut nv: Vec<_> = nodes.into_iter().collect();
    nv.sort_by(|a, b| b.1.cmp(&a.1));
    for (k, v) in nv.iter().take(12) { println!("  {v:4}  {k}"); }
    if detail {
        let extra: Vec<u32> = args.iter().find(|a| a.starts_with("-n")).map(|a| a[2..].split(',').filter_map(|x| x.parse().ok()).collect()).unwrap_or_default();
        let mut list: Vec<u32> = nv.iter().take(4).map(|(k, _)| k[5..9].trim().parse().unwrap()).collect();
        list.extend(extra);
        for nid in list {
            println!("== node {nid}");
            for (li, lane) in t.net.lanes.iter().enumerate() {
                if lane.end_node.0 != nid || lane.is_connector() { continue; }
                let LaneKind::Road { segment, forward, index } = lane.kind else { continue };
                let seg = world.roads.segment(segment);
                let Some(&head) = lane.cars.last() else { println!("  in lane {li} ({}+{} f{forward} i{index}) empty", seg.lanes_fwd, seg.lanes_bwd); continue };
                let c = &t.cars[head];
                let next = c.route.get(c.idx + 1).copied();
                let exit = c.route.get(c.idx + 2).copied();
                let desc = |l: Option<u32>| l.map(|l| { let ln = &t.net.lanes[l as usize]; format!("{l}[n{} len{:.0} cars{} first_s{:.1} green{}]", ln.end_node.0, ln.length, ln.cars.len(), ln.cars.first().map_or(-1.0, |&f| t.cars[f].s), ln.green) }).unwrap_or("-".into());
                println!("  in lane {li} ({}+{} f{forward} i{index}) len {:.0} cars {} head s {:.1} v {:.1} age {:.0} -> {} -> {}", seg.lanes_fwd, seg.lanes_bwd, lane.length, lane.cars.len(), c.s, c.v, c.age, desc(next), desc(exit));
            }
            if let Some(inter) = t.net.intersections.iter().find(|i| i.node.0 == nid) {
                println!("  signalized {} approaches {} phases {:?} phase {} timer {:.1}", inter.signalized, inter.approaches, inter.phases, inter.phase, inter.timer);
                for &(cl, _) in &inter.connectors { if !t.net.lanes[cl as usize].cars.is_empty() { println!("    connector {cl} has {} cars", t.net.lanes[cl as usize].cars.len()); } }
            }
        }
    }
    if detail {
        // Follow wait chains from every full lane's head.
        let mut cycles = 0;
        let mut seen_cycle: std::collections::HashSet<u32> = Default::default();
        for start in 0..t.net.lanes.len() as u32 {
            let mut chain = vec![];
            let mut cur = start;
            loop {
                let lane = &t.net.lanes[cur as usize];
                let Some(&head) = lane.cars.last() else { break };
                let c = &t.cars[head];
                if c.v > 0.5 { break; }
                let Some(&exit) = c.route.get(c.idx + 2) else { break };
                if lane.is_connector() { break; }
                chain.push(cur);
                if let Some(pos) = chain[..chain.len()-1].iter().position(|&x| x == exit) { let _ = pos; }
                if chain.contains(&exit) {
                    let i = chain.iter().position(|&x| x == exit).unwrap();
                    let cyc: Vec<u32> = chain[i..].to_vec();
                    let mut key = cyc.clone(); key.sort();
                    if seen_cycle.insert(key[0]) {
                        cycles += 1;
                        println!("cycle: {:?}", cyc.iter().map(|&l| { let ln=&t.net.lanes[l as usize]; let p=world.roads.node(ln.end_node).pos; format!("{l}(n{} {:.0},{:.0} cars{})", ln.end_node.0, p.x,p.y, ln.cars.len()) }).collect::<Vec<_>>());
                    }
                    break;
                }
                if chain.len() > 50 { break; }
                cur = exit;
            }
        }
        println!("cycles: {cycles}");
    }
    let _ = LaneKind::Connector { node: nd_sim::NodeId(0), approach: 0 };
}
