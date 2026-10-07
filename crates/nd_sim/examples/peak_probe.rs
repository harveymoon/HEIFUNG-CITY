//! Headless peak-hour probe: how many vehicles stand more than five
//! minutes, how many gridlocks there are, and what the heads of the
//! biggest jams wait for, sampled over a run of a save (`peak_probe
//! <save.ron> [ticks] [every]`). Ends with the run's averages, so a
//! change to the traffic rules can be measured before and after on the
//! same snapshot. Not part of the game.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::traffic::{StopReason, Wait};
use nd_sim::{Errand, Gesture, LandUse, Vec2 as SVec2, World, WorldConfig};
use std::collections::{BTreeMap, HashMap};

/// Stood over five minutes with no reason of its own (not at a light,
/// a stop or the kerb): the ring probe's "stuck".
fn stuck(c: &nd_sim::traffic::Car) -> bool {
    c.stalled > 300.0 && c.hold <= 0.0 && !c.entering && c.wait != Wait::Signal
}

/// Every junction of ring `r`: the car at the end of each ring lane
/// arriving there, where it goes next and why it stands; what is on the
/// ring by errand.
fn ring_dump(w: &World, r: usize) {
    let t = &w.traffic;
    let rings = &t.net.rings;
    let on: Vec<_> = t.cars.iter().filter(|(_, c)| rings.circulating.contains(&c.route[c.idx]) && rings.nodes.get(&t.net.lanes[c.route[c.idx] as usize].end_node).is_some_and(|n| n.ring == r)).collect();
    let mut errands: BTreeMap<String, usize> = BTreeMap::new();
    for (_, c) in &on {
        let e = format!("{:?}", c.errand);
        *errands.entry(e.split([' ', '(']).next().unwrap_or("").to_string()).or_default() += 1;
    }
    let kerb = on.iter().filter(|(_, c)| c.entering).count();
    println!("   RING {r}: {} cars ({} waiting at its kerb) {:?}", on.len(), kerb, errands);
    if std::env::var("RINGLANES").is_ok() {
        let mut ls: Vec<u32> = rings.circulating.iter().copied().filter(|&l| rings.nodes.get(&t.net.lanes[l as usize].end_node).is_some_and(|n| n.ring == r)).collect();
        ls.sort();
        for l in ls {
            let lane = &t.net.lanes[l as usize];
            if lane.cars.is_empty() { continue; }
            println!("     lane {l} conn {} len {:.1} next {:?}: {:?}", lane.is_connector(), lane.length, lane.next, lane.cars.iter().map(|&c| ((t.cars[c].s * 10.0).round() / 10.0, t.cars[c].length())).collect::<Vec<_>>());
        }
    }
    let mut nodes: Vec<_> = rings.nodes.iter().filter(|(_, n)| n.ring == r && !n.entries.is_empty()).collect();
    nodes.sort_by_key(|(id, _)| id.0);
    for (nid, n) in nodes {
        let mut line = format!("     node {} entries {} exits {}:", nid.0, n.entries.len(), n.exits.len());
        for &l in &n.ring_in {
            let lane = &t.net.lanes[l as usize];
            if let Some(&c) = lane.cars.last() {
                let car = &t.cars[c];
                let next = car.route.get(car.idx + 1).copied();
                let kind = next.map(|x| if n.exits.contains(&x) { "EXIT" } else if n.through.contains(&x) { "thru" } else { "?" }).unwrap_or("end");
                let mut ex = String::new();
                if kind == "EXIT" {
                    if let Some(&e) = car.route.get(car.idx + 2) {
                        let el = &t.net.lanes[e as usize];
                        ex = format!(" exitlane {:.0}m {} cars first@{:.1}", el.length, el.cars.len(), el.cars.first().map_or(-1.0, |&f| t.cars[f].s));
                        if let Some(&front) = el.cars.last() {
                            let ch = t.jam_chain(front);
                            if let Some(h) = ch.head() {
                                let hc = &t.cars[h];
                                let hl = &t.net.lanes[hc.route[hc.idx] as usize];
                                ex += &format!(" <- chain {} cyc {} head {:?} at ({:.0},{:.0}) {} errand {:?} s {:.0}/{:.0} stalled {:.0}", ch.cars.len(), ch.cycle, h, hc.pos.x, hc.pos.y, t.stop_reason(h).key(), hc.errand, hc.s, hl.length, hc.stalled);
                            }
                        }
                    }
                }
                line += &format!(" [in {l}: s {:.1}/{:.1} v {:.1} {kind} {:?}{ex}]", car.s, lane.length, car.v, car.wait);
            }
        }
        for &e in &n.entries {
            let el = &t.net.lanes[e as usize];
            let feed = el.prev.first().map(|&p| t.net.lanes[p as usize].cars.len()).unwrap_or(0);
            line += &format!(" (entry {e}: {} in conn, {} queued on arm)", el.cars.len(), feed);
        }
        println!("{line}");
    }
}

/// Gap from a car's front to the rear of the next vehicle ahead on its
/// route (across short lanes), if one is within 30 m.
fn body_gap(t: &nd_sim::traffic::Traffic, id: nd_sim::CarId) -> Option<(f32, nd_sim::CarId)> {
    let c = &t.cars[id];
    let lane = &t.net.lanes[c.route[c.idx] as usize];
    if let Some(k) = lane.cars.iter().position(|&o| o == id) {
        if let Some(&l) = lane.cars.get(k + 1) {
            return Some((t.cars[l].s - t.cars[l].length() - c.s, l));
        }
    }
    let mut dist = lane.length - c.s;
    let mut prev = c.route[c.idx];
    for &n in c.route.iter().skip(c.idx + 1).take(6) {
        if t.net.lanes[prev as usize].is_beside(n) { return None; }
        let nl = &t.net.lanes[n as usize];
        if let Some(&f) = nl.cars.first() {
            return Some((dist + t.cars[f].s - t.cars[f].length(), f));
        }
        dist += nl.length;
        if dist > 30.0 { return None; }
        prev = n;
    }
    None
}

/// The demo district (as `jam_probe demo`).
fn seed(world: &mut World) {
    world.drain_events();
    world.start_project();
    for i in -2..=2 {
        let c = i as f32 * 140.0;
        world.add_gesture(Gesture::Road { points: vec![SVec2::new(-280.0, c), SVec2::new(280.0, c)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: nd_sim::roads::RoadKind::Street, elevation: Vec::new() });
        world.add_gesture(Gesture::Road { points: vec![SVec2::new(c, -280.0), SVec2::new(c, 280.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: nd_sim::roads::RoadKind::Street, elevation: Vec::new() });
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
    world.add_gesture(Gesture::Road { points: vec![SVec2::new(-420.0, nd_sim::rail::TRUNK_Y), SVec2::new(-330.0, -410.0), SVec2::new(-280.0, -348.0), SVec2::new(-110.0, -348.0), SVec2::new(-70.0, -324.0), SVec2::new(150.0, -324.0)], lanes_fwd: 0, lanes_bwd: 0, curved: false, kind: nd_sim::roads::RoadKind::Rail, elevation: Vec::new() });
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
    let src = &args[1];
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(900);
    let every: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(60);
    let verbose = std::env::var("VERBOSE").is_ok();
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    // `demo:<ticks>:<out.ron>` grows the demo district that long with the
    // rule-based mind and saves it, for a repeatable demo snapshot.
    if let Some(rest) = src.strip_prefix("demo:") {
        let (n, out) = rest.split_once(':').unwrap();
        let mut w = World::new(WorldConfig::default());
        seed(&mut w);
        let mut rb = nd_minds::RuleBasedMind::default();
        for _ in 0..n.parse::<u64>().unwrap() {
            w.tick(&mut rb);
        }
        std::fs::write(out, w.to_ron().unwrap()).unwrap();
        println!("saved {} at day {} {:02}:{:02}, {} on the road", out, w.clock.day(), w.clock.hour(), w.clock.minute_of_day() % 60, w.traffic.cars.len());
        return;
    }
    let mut w = World::from_ron(&std::fs::read_to_string(src).unwrap()).expect("load");
    if std::env::var("TOWS").is_ok() {
        let mut m: BTreeMap<String, usize> = BTreeMap::new();
        for j in &w.enforcement.jobs {
            let on_road = j.truck.is_some_and(|t| w.traffic.cars.contains_key(t));
            *m.entry(format!("company {} {:?} truck_on_road {}", j.company.is_some(), j.state, on_road)).or_default() += 1;
        }
        let ages: Vec<u64> = w.enforcement.jobs.iter().map(|j| w.clock.tick - j.since).collect();
        println!("TOW JOBS {} {:?} oldest {:?}", w.enforcement.jobs.len(), m, ages.iter().max());
        let tows: Vec<_> = w.traffic.cars.iter().filter(|(_, c)| matches!(c.errand, Errand::Tow(_))).map(|(id, c)| (id, c.errand)).collect();
        let orphan = tows.iter().filter(|(id, e)| match e { Errand::Tow(n) => !w.enforcement.jobs.iter().any(|j| j.id == *n && j.truck == Some(*id)), _ => false }).count();
        println!("tow trucks on road {} orphaned (no job pointing at them) {}", tows.len(), orphan);
        for (b, bld) in w.buildings.iter().filter(|(_, b)| b.business.as_ref().is_some_and(|z| z.kind == nd_sim::BusinessKind::TowYard)) {
            println!("  tow yard {:?} workers {} trucks {}", b, bld.workers.len(), w.tow_trucks(b));
        }
    }
    // DRAIN: the traffic model alone, nobody new on the road, every arrival
    // taken off: how much of the jam is stuck for good.
    if std::env::var("DRAIN").is_ok() {
        let start = w.traffic.cars.len();
        for i in 0..ticks {
            let before: HashMap<nd_sim::CarId, (u32, f32, bool)> = w.traffic.cars.iter().map(|(id, c)| (id, (c.route[c.idx], c.s, c.entering))).collect();
            let over_before: std::collections::HashSet<nd_sim::CarId> = w.traffic.cars.keys().filter(|&id| !w.traffic.cars[id].entering && body_gap(&w.traffic, id).is_some_and(|g| g.0 < -0.5)).collect();
            let arrived = w.traffic.step(4.0, 8);
            if std::env::var("OVERLAP").is_ok() {
                let t = &w.traffic;
                for id in t.cars.keys() {
                    if t.cars[id].entering || over_before.contains(&id) { continue; }
                    if let Some((g, o)) = body_gap(t, id).filter(|g| g.0 < -0.5) {
                        let c = &t.cars[id];
                        let oc = &t.cars[o];
                        println!("  NEW OVERLAP t{} {:?} lane {} s {:.1} (was {:?}) gap {:.1} vs {:?} lane {} s {:.1} (was {:?}) ring {}", i + 1, id, c.route[c.idx], c.s, before.get(&id), g, o, oc.route[oc.idx], oc.s, before.get(&o), t.net.rings.circulating.contains(&c.route[c.idx]));
                    }
                }
            }
            for id in arrived {
                w.traffic.remove_car(id);
            }
            if (i + 1) % every == 0 {
                let t = &w.traffic;
                let st = t.cars.values().filter(|c| stuck(c)).count();
                let mut why: BTreeMap<&str, usize> = BTreeMap::new();
                for (id, c) in t.cars.iter() {
                    if c.v < 0.5 && c.stalled > 60.0 { *why.entry(t.stop_reason(id).key()).or_default() += 1; }
                }
                let overl = t.cars.keys().filter(|&id| !t.cars[id].entering && body_gap(t, id).is_some_and(|g| g.0 < -0.5)).count();
                println!("drain +{} left {} of {} stuck>5m {} gridlocks {} overlaps {} | {:?}", i + 1, t.cars.len(), start, st, t.gridlocks().len(), overl, why);
                if let Some(r) = std::env::var("RING").ok().and_then(|r| r.parse::<usize>().ok()) {
                    ring_dump(&w, r);
                }
            }
        }
        if std::env::var("ERRANDS").is_ok() {
            let t = &w.traffic;
            let mut e: BTreeMap<String, (usize, usize)> = BTreeMap::new();
            for c in t.cars.values() {
                let k = format!("{:?}", c.errand);
                let k = k.split([' ', '(']).next().unwrap_or("").to_string();
                let k = if c.bus.is_some() { "Bus".to_string() } else if c.cargo.is_some() { format!("Lorry/{k}") } else { k };
                let x = e.entry(k).or_default();
                x.0 += 1;
                if stuck(c) { x.1 += 1; }
            }
            println!("     ERRANDS (on road, stuck>5m) {:?}", e);
        }
        if verbose {
            let t = &w.traffic;
            let mut heads: HashMap<nd_sim::CarId, (usize, bool)> = HashMap::new();
            for (id, c) in t.cars.iter() {
                if c.v >= 0.5 { continue; }
                let ch = t.jam_chain(id);
                if let Some(h) = ch.head() { let e = heads.entry(h).or_default(); e.0 = e.0.max(ch.cars.len()); e.1 |= ch.cycle; }
            }
            let mut hs: Vec<_> = heads.into_iter().collect();
            hs.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
            for (h, (len, cycle)) in hs.into_iter().take(15) {
                let c = &t.cars[h];
                let lane = c.route[c.idx];
                let l = &t.net.lanes[lane as usize];
                println!("   head {:?} chain {len}{} at ({:.0},{:.0}) {} ring {} lane {lane} s {:.0}/{:.0} next {:?} errand {:?} stalled {:.0}", h, if cycle { " CYCLE" } else { "" }, c.pos.x, c.pos.y, t.stop_reason(h).key(), t.net.rings.circulating.contains(&lane), c.s, l.length, c.route.get(c.idx + 1..(c.idx + 3).min(c.route.len())), c.errand, c.stalled);
            }
        }
        return;
    }
    let (mut sum_stuck, mut sum_grid, mut n, mut max_stuck) = (0usize, 0usize, 0usize, 0usize);
    let mut sum_arrived = 0usize;
    let mut head_total: BTreeMap<String, usize> = BTreeMap::new();
    let mut spawns: BTreeMap<String, usize> = BTreeMap::new();
    let mut trips: Vec<(f32, f32)> = Vec::new();
    for i in 0..ticks {
        let before: std::collections::HashSet<nd_sim::CarId> = w.traffic.cars.keys().collect();
        let timing: HashMap<nd_sim::CarId, (u64, f32)> = w.traffic.cars.iter().filter(|(_, c)| c.drive.expected_min > 0.0).map(|(id, c)| (id, (c.drive.trip_start, c.drive.expected_min))).collect();
        w.tick(&mut minds);
        sum_arrived += before.iter().filter(|id| !w.traffic.cars.contains_key(**id)).count();
        for (id, (start, exp)) in &timing {
            if !w.traffic.cars.contains_key(*id) {
                let took = w.clock.tick.saturating_sub(*start) as f32;
                trips.push((took, *exp));
            }
        }
        for (id, c) in w.traffic.cars.iter() {
            if before.contains(&id) { continue; }
            let k = format!("{:?}", c.errand);
            let k = k.split([' ', '(']).next().unwrap_or("").to_string();
            let k = if c.bus.is_some() { "Bus".to_string() } else if c.cargo.is_some() { "Lorry".to_string() } else if c.owned.is_none() && k == "Trip" { "Through".to_string() } else { k };
            *spawns.entry(k).or_default() += 1;
        }
        if let Ok(want) = std::env::var("TRACEID") {
            let t = &w.traffic;
            for (id, c) in t.cars.iter().filter(|(id, _)| format!("{:?}", id).contains(&want)) {
                let left: f32 = c.route.iter().skip(c.idx + 1).map(|&l| t.net.lanes[l as usize].length).sum::<f32>() + t.net.lanes[c.route[c.idx] as usize].length - c.s;
                let last = *c.route.last().unwrap();
                println!("  T{} {:?} {:?} at ({:.0},{:.0}) v {:.1} {:?} stalled {:.0} lane {} s {:.1} left {:.0} m in {} lanes goal {} s_end {:.1} replans {} age {:.0}", i + 1, id, c.errand, c.pos.x, c.pos.y, c.v, c.wait, c.stalled, c.route[c.idx], c.s, left, c.route.len() - c.idx, last, c.s_end, c.replans, c.age);
            }
        }
        if let Some(at) = std::env::var("SAVEAT").ok().and_then(|v| v.parse::<u64>().ok()) {
            if (i + 1) % at == 0 {
                let out = format!("{}.{}.ron", src, i + 1);
                std::fs::write(&out, w.to_ron().unwrap()).unwrap();
                println!("saved {out}");
            }
        }
        if (i + 1) % every != 0 {
            continue;
        }
        let t = &w.traffic;
        let st: Vec<_> = t.cars.iter().filter(|(_, c)| stuck(c)).map(|(id, _)| id).collect();
        let grid = t.gridlocks();
        let searching = t.cars.values().filter(|c| matches!(c.errand, Errand::Search { .. })).count();
        let trucks = t.cars.values().filter(|c| c.cargo.is_some()).count();
        let mut why: BTreeMap<&str, usize> = BTreeMap::new();
        let mut heads: HashMap<nd_sim::CarId, (usize, bool)> = HashMap::new();
        for &id in &st {
            *why.entry(t.stop_reason(id).key()).or_default() += 1;
            let chain = t.jam_chain(id);
            if let Some(h) = chain.head() {
                let e = heads.entry(h).or_default();
                e.0 = e.0.max(chain.cars.len());
                e.1 |= chain.cycle;
            }
        }
        // What the heads wait for, weighed by the stuck cars behind them.
        let mut by_head: BTreeMap<String, usize> = BTreeMap::new();
        for (&h, &(len, cycle)) in &heads {
            let c = &t.cars[h];
            let r = t.stop_reason(h);
            let mut key = if cycle { format!("cycle/{}", r.key()) } else { r.key().to_string() };
            if t.net.rings.circulating.contains(&c.route[c.idx]) {
                key.push_str("@ring");
            }
            *by_head.entry(key).or_default() += len;
        }
        for (k, v) in &by_head {
            *head_total.entry(k.clone()).or_default() += v;
        }
        sum_stuck += st.len();
        sum_grid += grid.len();
        max_stuck = max_stuck.max(st.len());
        n += 1;
        println!(
            "+{:4} {:02}:{:02} road {} trucks {} searching {} stuck>5m {} gridlocks {} ({} veh) angry {} | {:?} | heads {:?}",
            i + 1,
            w.clock.hour(),
            w.clock.minute_of_day() % 60,
            t.cars.len(),
            trucks,
            searching,
            st.len(),
            grid.len(),
            grid.iter().map(|g| g.vehicles).sum::<usize>(),
            w.angry_drivers_now(),
            why,
            by_head
        );
        if let Some(r) = std::env::var("RING").ok().and_then(|r| r.parse::<usize>().ok()) {
            ring_dump(&w, r);
        }
        if std::env::var("REDS").is_ok() {
            for (id, c) in t.cars.iter().filter(|(_, c)| c.wait == Wait::Signal && c.stalled > 300.0) {
                let lane = c.route[c.idx];
                let l = &t.net.lanes[lane as usize];
                if l.cars.last() != Some(&id) { continue; }
                // The red connector ahead.
                let mut info = String::new();
                for &n in c.route.iter().skip(c.idx + 1).take(4) {
                    let nl = &t.net.lanes[n as usize];
                    if nl.is_connector() && !nl.green {
                        if let nd_sim::traffic::LaneKind::Connector { node, approach } = nl.kind {
                            let ii = t.net.node_intersection.get(node.0 as usize).copied();
                            if let Some(inter) = ii.and_then(|i| t.net.intersections.get(i)) {
                                let busy: Vec<_> = inter.connectors.iter().filter(|(cl, _)| !t.net.lanes[*cl as usize].cars.is_empty()).map(|(cl, ap)| (*ap, t.net.lanes[*cl as usize].cars.len())).collect();
                                let inc: Vec<_> = inter.incoming.iter().map(|&(il, ap)| (ap, t.net.lanes[il as usize].cars.len(), t.net.lanes[il as usize].cars.last().map(|&h| (t.cars[h].wait, (t.net.lanes[il as usize].length - t.cars[h].s) as i32)))).collect();
                                info = format!("node {} ap {approach} conn {n} turn {} red_for {:.0} phases {:?} phase {} timer {:.1} arrow {:.0} approaches {} busy {:?} incoming {:?}", node.0, nl.turn, nl.red_for, inter.phases, inter.phase, inter.timer, inter.arrow, inter.approaches, busy, inc);
                            }
                        }
                        break;
                    }
                }
                let mut exit = String::new();
                if let (Some(&cn), Some(&ex)) = (c.route.get(c.idx + 1), c.route.get(c.idx + 2)) {
                    let el = &t.net.lanes[ex as usize];
                    let cl = &t.net.lanes[cn as usize];
                    exit = format!(" | conn {cn} len {:.0} cars {} -> exit {ex} len {:.0} cars {} tail {:?} front {:?}", cl.length, cl.cars.len(), el.length, el.cars.len(), el.cars.first().map(|&f| ((t.cars[f].s * 10.0).round() / 10.0, t.cars[f].v, t.stop_reason(f).key())), el.cars.last().map(|&f| ((t.cars[f].s * 10.0).round() / 10.0, t.cars[f].v, t.stop_reason(f).key(), t.cars[f].pos)));
                }
                println!("     RED {:?} at ({:.0},{:.0}) stalled {:.0} lane {lane} s {:.0}/{:.0} {info}{exit}", id, c.pos.x, c.pos.y, c.stalled, c.s, l.length);
            }
        }
        if std::env::var("HUNT").is_ok() {
            let mut tries: BTreeMap<String, usize> = BTreeMap::new();
            let mut mins: Vec<u64> = Vec::new();
            for c in t.cars.values() {
                let k = match c.errand { Errand::Search { tries } => format!("search{tries}"), Errand::ToKerb => "tokerb".into(), Errand::ToLot(_) => "tolot".into(), _ => continue };
                *tries.entry(k).or_default() += 1;
                if let Some(z) = w.citizens.get(c.citizen) {
                    if z.commute.search_since > 0 { mins.push(w.clock.tick - z.commute.search_since); }
                }
            }
            mins.sort();
            let q = |f: f32| mins.get(((mins.len() as f32 - 1.0) * f) as usize).copied().unwrap_or(0);
            println!("     HUNT {:?} searching-minutes median {} p90 {} max {}", tries, q(0.5), q(0.9), q(1.0));
        }
        if std::env::var("GRID").is_ok() {
            let mut g: BTreeMap<(i32, i32), (usize, usize, usize)> = BTreeMap::new();
            for c in t.cars.values() {
                let e = g.entry(((c.pos.y / 100.0).floor() as i32, (c.pos.x / 100.0).floor() as i32)).or_default();
                e.0 += 1;
                if stuck(c) { e.1 += 1; }
                if matches!(c.errand, Errand::Search { .. } | Errand::ToKerb | Errand::ToLot(_)) { e.2 += 1; }
            }
            let (y0, y1) = (g.keys().map(|k| k.0).min().unwrap_or(0), g.keys().map(|k| k.0).max().unwrap_or(0));
            let (x0, x1) = (g.keys().map(|k| k.1).min().unwrap_or(0), g.keys().map(|k| k.1).max().unwrap_or(0));
            println!("     GRID cars/stuck/hunting per 100 m, x from {} to {}", x0 * 100, x1 * 100 + 100);
            for y in (y0..=y1).rev() {
                let row: Vec<String> = (x0..=x1).map(|x| g.get(&(y, x)).map_or("      .     ".to_string(), |e| format!("{:>4}/{:<3}/{:<3}", e.0, e.1, e.2))).collect();
                println!("     y{:>5} {}", y * 100, row.join(""));
            }
        }
        if std::env::var("ERRANDS").is_ok() {
            let mut e: BTreeMap<String, (usize, usize)> = BTreeMap::new();
            for c in t.cars.values() {
                if c.pos.y > 350.0 { continue; }
                let k = format!("{:?}", c.errand);
                let k = k.split([' ', '(']).next().unwrap_or("").to_string();
                let k = if c.bus.is_some() { "Bus".to_string() } else if c.cargo.is_some() { format!("Lorry/{k}") } else if c.owned.is_none() && k == "Trip" { "Through".to_string() } else { k };
                let x = e.entry(k).or_default();
                x.0 += 1;
                if stuck(c) { x.1 += 1; }
            }
            println!("     CORE ERRANDS (n, stuck) {:?}", e);
        }
        if let Ok(kind) = std::env::var("TRACE") {
            let mut n = 0;
            for (id, c) in t.cars.iter() {
                let k = format!("{:?}", c.errand);
                if !k.starts_with(&kind) || c.pos.y > 350.0 { continue; }
                n += 1;
                if n > 6 { break; }
                let left: f32 = c.route.iter().skip(c.idx + 1).map(|&l| t.net.lanes[l as usize].length).sum::<f32>() + t.net.lanes[c.route[c.idx] as usize].length - c.s;
                println!("     TRACE {:?} {k} at ({:.0},{:.0}) v {:.1} {:?} stalled {:.0} age {:.0} route left {:.0} m in {} lanes replans {}", id, c.pos.x, c.pos.y, c.v, c.wait, c.stalled, c.age, left, c.route.len() - c.idx, c.replans);
            }
        }
        if let Some(jn) = std::env::var("JN").ok().and_then(|v| v.parse::<usize>().ok()) {
            if let Some(inter) = t.net.node_intersection.get(jn).and_then(|&i| t.net.intersections.get(i)) {
                println!("     JN {jn} signalized {} phases {:?} phase {} timer {:.1}", inter.signalized, inter.phases, inter.phase, inter.timer);
                for &(il, ap) in &inter.incoming {
                    let l = &t.net.lanes[il as usize];
                    let head = l.cars.last().map(|&h| {
                        let c = &t.cars[h];
                        let nx = c.route.get(c.idx + 1).copied();
                        let ex = c.route.get(c.idx + 2).copied();
                        let exs = ex.map(|e| { let el = &t.net.lanes[e as usize]; format!("exit {e} len {:.0} cars {} tail {:?}", el.length, el.cars.len(), el.cars.first().map(|&f| ((t.cars[f].s*10.0).round()/10.0, (t.cars[f].v*10.0).round()/10.0))) });
                        format!("head s {:.0}/{:.0} v {:.1} {:?} st {:.0} next {:?} beside {} {:?}", c.s, l.length, c.v, c.wait, c.stalled, nx, nx.is_some_and(|n| l.is_beside(n)), exs)
                    });
                    println!("       in {il} ap {ap} arrows {} cars {} {:?}", l.arrows, l.cars.len(), head);
                }
                for &(cl, ap) in &inter.connectors {
                    let l = &t.net.lanes[cl as usize];
                    if !l.cars.is_empty() { println!("       conn {cl} ap {ap} turn {} green {} cars {} -> {:?}", l.turn, l.green, l.cars.len(), l.next); }
                }
            }
        }
        if std::env::var("SPAWNS").is_ok() {
            println!("     SPAWNS since last {:?}", std::mem::take(&mut spawns));
        }
        if verbose {
            let mut hs: Vec<_> = heads.into_iter().collect();
            hs.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
            for (h, (len, cycle)) in hs.into_iter().take(8) {
                let c = &t.cars[h];
                let lane = c.route[c.idx];
                let l = &t.net.lanes[lane as usize];
                let r = t.stop_reason(h);
                let mut extra = String::new();
                if let StopReason::ExitFull { .. } = r {
                    if let Some(&exit) = c.route.get(c.idx + 2) {
                        let el = &t.net.lanes[exit as usize];
                        let last = el.cars.last().map(|&o| (t.stop_reason(o).key(), t.cars[o].v));
                        extra = format!(" exit lane {exit} len {:.0} cars {} front {:?}", el.length, el.cars.len(), last);
                    }
                }
                println!(
                    "     head {:?} chain {len}{} at ({:.0},{:.0}) {} ring {} lane {lane} s {:.0}/{:.0} errand {:?} truck {} stalled {:.0}{extra}",
                    h,
                    if cycle { " CYCLE" } else { "" },
                    c.pos.x,
                    c.pos.y,
                    r.key(),
                    t.net.rings.circulating.contains(&lane),
                    c.s,
                    l.length,
                    c.errand,
                    c.cargo.is_some(),
                    c.stalled
                );
            }
        }
    }
    let mut took: Vec<f32> = trips.iter().map(|t| t.0).collect();
    took.sort_by(|a, b| a.total_cmp(b));
    let ratio = trips.iter().map(|t| t.0 / t.1.max(1.0)).sum::<f32>() / trips.len().max(1) as f32;
    let q = |f: f32| took.get(((took.len() as f32 - 1.0) * f).max(0.0) as usize).copied().unwrap_or(0.0);
    println!("CRASHES total {} (crash reports so far)", w.driving.crashes_total);
    println!("TRIPS timed private drives finished {} minutes mean {:.1} median {:.0} p90 {:.0} max {:.0} | mean actual/expected {:.2}", took.len(), took.iter().sum::<f32>() / took.len().max(1) as f32, q(0.5), q(0.9), q(1.0), ratio);
    println!(
        "SUMMARY {} samples: mean stuck>5m {:.1} max {} mean gridlocks {:.2} trips done {} on road at end {} | stuck behind heads {:?}",
        n,
        sum_stuck as f32 / n.max(1) as f32,
        max_stuck,
        sum_grid as f32 / n.max(1) as f32,
        sum_arrived,
        w.traffic.cars.len(),
        head_total
    );
}
