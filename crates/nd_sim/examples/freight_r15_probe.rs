//! [r15-freight] Probe for the photographer's freight and traffic findings
//! (2026-11-20 F2, F12, F15) on a save: depot box stock against room,
//! lorries standing on one point, empty lorry runs, the worst roundabout
//! queues, bus and rail riders a day, population and treasury. Not part of
//! the game.
//!
//! `freight_r15_probe <save.ron> [days=2] [hours_first=0]`
use nd_minds::{HybridMind, MockBackend};
use nd_sim::World;
use std::collections::{BTreeMap, HashMap, HashSet};

fn depots(w: &World) {
    let mut over = 0;
    let mut rows = Vec::new();
    for (b, kind, p) in w.box_depots() {
        let stock = w.depot_stock(b);
        let room = w.box_room(b, kind);
        if stock > room {
            over += 1;
        }
        rows.push((stock as i64 - room as i64, format!("    {:?} {:24} {:?} at ({:.0},{:.0}) stock {stock} room {room} ground {} kept_away {}", b, w.buildings[b].name, kind, p.x, p.y, w.box_capacity(b, kind), w.kept_away(b))));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    println!("  depots {} over room {over}", rows.len());
    for r in rows.iter().take(8) {
        println!("{}", r.1);
    }
    // What the worst holds.
    if let Some((b, _, _)) = w.box_depots().into_iter().max_by_key(|d| w.depot_stock(d.0) as i64 - w.box_room(d.0, d.1) as i64) {
        let mut m: BTreeMap<String, usize> = BTreeMap::new();
        for r in w.containers.boxes.values().filter(|r| r.at.stack() == Some(b)) {
            let k = format!("{}{}{}{}", if r.goods.is_some() { "full" } else { "empty" }, match r.owner { nd_sim::containers::Owner::Line => "/line", _ => "/other" }, if r.export.is_empty() { "" } else { "/export" }, if r.bound_out { "/bound_out" } else { "" });
            *m.entry(k).or_default() += 1;
        }
        println!("    {} holds {m:?}", w.buildings[b].name);
    }
}

fn stacks(w: &World, detail: bool) -> (usize, usize, usize) {
    // Lorries (cargo) on the same point, to 0.2 m; those in bays are drawn in the bay.
    let mut at: HashMap<(i64, i64), Vec<nd_sim::CarId>> = HashMap::new();
    for (id, c) in w.traffic.cars.iter() {
        if c.cargo.is_none() || c.bay {
            continue;
        }
        at.entry(((c.pos.x * 5.0).round() as i64, (c.pos.y * 5.0).round() as i64)).or_default().push(id);
    }
    if detail {
        let mut gs: Vec<&Vec<nd_sim::CarId>> = at.values().filter(|v| v.len() >= 3).collect();
        gs.sort_by_key(|v| std::cmp::Reverse(v.len()));
        for v in gs.iter().take(4) {
            let c0 = &w.traffic.cars[v[0]];
            println!("    group of {} at ({:.1},{:.1}):", v.len(), c0.pos.x, c0.pos.y);
            for &id in v.iter().take(5) {
                let c = &w.traffic.cars[id];
                let lane = c.route[c.idx];
                let kerb = w.traffic.net.lanes[lane as usize].right.is_none();
                let l = &w.traffic.net.lanes[lane as usize];
                println!("        lane len {:.1} s_end {:.1} stalled {:.0} cooldown {:.1} lane cars {:?} job {:?} queued {:?} bay {:?}", l.length, c.s_end, c.stalled, c.cooldown, l.cars.iter().map(|o| (w.traffic.cars[*o].s, w.traffic.cars[*o].v)).collect::<Vec<_>>(), w.containers.lorries.get(&id).map(|j| j.depot), w.lorry_queued(id), w.lorry_bay(id));
                println!("      {:?} entering {} hold {:.0} wait {:?} s {:.1} lane {lane} kerb {kerb} idx {}/{} stranded {} why {:?} cargo {:?}", id, c.entering, c.hold, c.wait, c.s, c.idx, c.route.len(), c.stranded, w.traffic.stop_reason(id), c.cargo.map(|g| (g.resource, g.amount, w.buildings.get(g.to).map(|b| b.name.clone()))));
            }
        }
    }
    let groups = at.values().filter(|v| v.len() >= 2).count();
    let cars = at.values().filter(|v| v.len() >= 2).map(|v| v.len()).sum();
    let worst = at.values().map(|v| v.len()).max().unwrap_or(0);
    (groups, cars, worst)
}

/// Lorries (cars with cargo, as `truck_count` counts them): loaded on the
/// road, empty on the road, in a depot bay, standing at the kerb (unloading,
/// queued, stranded).
fn lorries(w: &World) -> [usize; 6] {
    let mut n = [0usize; 6];
    for (id, c) in w.traffic.cars.iter() {
        let Some(cg) = c.cargo else { continue };
        if w.containers.haulers.contains(&id) {
            continue;
        }
        let empty = cg.amount == 0.0 && cg.price == 0.0 && cg.resource == nd_sim::Resource::Fuel;
        let k = if c.bay { 2 } else if c.hold > 0.0 || c.entering { 3 } else if empty { 1 } else { 0 };
        n[k] += 1;
        n[4] += 1;
        if !empty && w.buildings.get(cg.to).is_some_and(|b| b.construction.is_some()) {
            n[5] += 1;
        }
    }
    n
}

/// The ring a car's queue is held at: its own lane is on the ring, or the
/// connector it waits to enter is at a ring node. With why ("entry": giving
/// way at the line; "ring": on the ring itself; "exit": leaving into a full arm).
fn ring_of(w: &World, id: nd_sim::CarId) -> Option<(usize, &'static str)> {
    let t = &w.traffic;
    let c = &t.cars[id];
    let here = *c.route.get(c.idx)?;
    let lane = &t.net.lanes[here as usize];
    if t.net.rings.circulating.contains(&here) {
        let rn = t.net.rings.nodes.get(&lane.end_node)?;
        return Some((rn.ring, "ring"));
    }
    if let nd_sim::traffic::LaneKind::Connector { node, .. } = lane.kind {
        if let Some(rn) = t.net.rings.nodes.get(&node) {
            return Some((rn.ring, if rn.exits.contains(&here) { "exit" } else { "entry" }));
        }
    }
    let next = *c.route.get(c.idx + 1)?;
    if let nd_sim::traffic::LaneKind::Connector { node, .. } = t.net.lanes[next as usize].kind {
        if let Some(rn) = t.net.rings.nodes.get(&node) {
            return Some((rn.ring, if rn.exits.contains(&next) { "exit" } else { "entry" }));
        }
    }
    None
}

/// Everything about ring `k` now: its nodes, each entry's queue and the
/// head's wait, and every car on the ring with why it stands.
fn ring_dump(w: &World, k: usize) {
    let t = &w.traffic;
    let mut nodes: Vec<(&nd_sim::roads::NodeId, &nd_sim::traffic::RingNode)> = t.net.rings.nodes.iter().filter(|(_, rn)| rn.ring == k).collect();
    nodes.sort_by_key(|(n, _)| n.0);
    for (n, rn) in nodes {
        let p = w.roads.node(*n).pos;
        if rn.entries.is_empty() && rn.exits.is_empty() {
            continue;
        }
        println!("    node {:?} at ({:.0},{:.0}): entries {:?} exits {:?} through {:?} ring_in {:?}", n, p.x, p.y, rn.entries, rn.exits, rn.through, rn.ring_in);
        for &e in &rn.entries {
            let el = &t.net.lanes[e as usize];
            let ins: Vec<u32> = el.prev.clone();
            for l in ins {
                let lane = &t.net.lanes[l as usize];
                let q = lane.cars.iter().filter(|&&c| t.cars[c].v < 0.5).count();
                if let Some(&head) = lane.cars.last() {
                    let h = &t.cars[head];
                    println!("      arm lane {l} -> entry {e}: {} cars ({q} standing), head {:?} at {:.1}/{:.1} v {:.1} stalled {:.0} wait {:?} why {:?}", lane.cars.len(), head, h.s, lane.length, h.v, h.stalled, h.wait, t.stop_reason(head));
                }
            }
            for &c in &el.cars {
                let x = &t.cars[c];
                println!("      in entry conn: {:?} s {:.1} v {:.1} wait {:?}", c, x.s, x.v, x.wait);
            }
        }
        for &x in rn.exits.iter().chain(rn.through.iter()) {
            for &c in &t.net.lanes[x as usize].cars {
                let y = &t.cars[c];
                println!("      on conn {x} ({}): {:?} s {:.1} v {:.1} wait {:?} why {:?}", if rn.exits.contains(&x) { "exit" } else { "through" }, c, y.s, y.v, y.wait, t.stop_reason(c));
            }
        }
        for &l in &rn.ring_in {
            let lane = &t.net.lanes[l as usize];
            let cs: Vec<String> = lane.cars.iter().map(|&c| { let y = &t.cars[c]; format!("{:.0}@{:.1}{}", y.s, y.v, if y.v < 0.5 { format!("({:?})", t.stop_reason(c)).chars().take(24).collect::<String>() } else { String::new() }) }).collect();
            println!("      ring lane {l} len {:.0}: {}", lane.length, cs.join(" "));
        }
    }
}

/// Per ring: cars standing in queues whose head is held at the ring, the
/// longest stall among them, and the heads' reasons.
fn ring_queues(w: &World) -> BTreeMap<usize, (usize, f32, BTreeMap<String, usize>)> {
    let t = &w.traffic;
    let mut out: BTreeMap<usize, (usize, f32, BTreeMap<String, usize>)> = BTreeMap::new();
    for (id, c) in t.cars.iter() {
        if c.v >= 0.5 || c.stalled < 30.0 || c.hold > 0.0 || c.entering {
            continue;
        }
        let chain = t.jam_chain(id);
        let head = *chain.cars.last().unwrap();
        let Some((ring, why)) = ring_of(w, head) else { continue };
        let e = out.entry(ring).or_default();
        e.0 += 1;
        e.1 = e.1.max(c.stalled);
        if head == id {
            let r = format!("{why}:{:?}", t.stop_reason(head)).split(|ch| ch == ' ' || ch == '{').next().unwrap_or("").to_string();
            *e.2.entry(r).or_default() += 1;
        }
    }
    out
}

/// The bus lines, and the commutes a bus is offered for: what each mode
/// costs them (minutes and weighed cost), on average.
fn transit(w: &World) {
    let secs = w.config.traffic_seconds_per_tick;
    println!("  config walk {:.1} bike {:.1} m/tick, bike from {:.0} m, drive from {:.0} m, {secs} s a tick", w.config.walk_speed, w.config.bike_speed, w.config.bike_threshold, w.config.drive_threshold);
    for (_, r) in &w.transit.routes {
        let len: f32 = (0..r.stops.len()).map(|i| r.stops[i].pos.distance(r.stops[(i + 1) % r.stops.len()].pos)).sum();
        println!("  line {:14} stops {:2} buses {:2} loop {:5.0} min headway {:4} min, crow loop {:5.0} m, fare {:.1}, riders today {}", r.name, r.stops.len(), r.buses, r.loop_minutes(secs), r.headway(secs), len, r.fare, r.riders_today);
    }
    let mut n = 0u32;
    let mut sums: BTreeMap<String, (u32, f32, f32)> = BTreeMap::new();
    let mut best: BTreeMap<String, u32> = BTreeMap::new();
    let mut owns_bike = 0u32;
    for (id, c) in w.citizens.iter().step_by(7) {
        let Some(ch) = w.commute_choice(id) else { continue };
        if !ch.options.iter().any(|o| o.mode == "bus") {
            continue;
        }
        n += 1;
        if c.owns_bike {
            owns_bike += 1;
        }
        *best.entry(ch.options[0].mode.clone()).or_default() += 1;
        for o in &ch.options {
            let e = sums.entry(o.mode.clone()).or_default();
            e.0 += 1;
            e.1 += o.minutes;
            e.2 += o.cost;
        }
    }
    println!("  commutes with a bus on offer (1 in 7 people): {n}, owning a bike {owns_bike}; cheapest {best:?}");
    for (m, (k, mins, cost)) in sums {
        println!("    {m:7} offered {k:4}: {:.1} min, cost {:.1}", mins / k as f32, cost / k as f32);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let days: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    let mut w = World::read_save(&args[1]).expect("load");
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    println!("load day {} {:02}:{:02} pop {} treasury {:.0}", w.clock.day(), w.clock.hour(), w.clock.minute_of_day() % 60, w.citizens.len(), w.economy.treasury);
    depots(&w);
    for p in w.ports.ports.values().filter(|p| p.kind == nd_sim::ports::PortKind::ShippingPort) {
        let (base, tip) = p.crane_boom();
        // Water along the seaward line from the base: where it starts and ends.
        let mut first = None;
        let mut last = None;
        for k in 0..600 {
            let q = base + p.seaward * k as f32;
            if w.landscape.is_water(q) {
                first.get_or_insert(k);
                last = Some(k);
            } else if first.is_some() {
                break;
            }
        }
        println!("  port {:?} centre ({:.0},{:.0}) seaward ({:.2},{:.2}) berth ({:.0},{:.0}) pier {:?} boom base ({:.0},{:.0}) tip ({:.0},{:.0}) outreach {:.0} m; water from {:?} to {:?} m out", p.building, p.center.x, p.center.y, p.seaward.x, p.seaward.y, p.berth.x, p.berth.y, p.pier, base.x, base.y, tip.x, tip.y, base.distance(tip), first, last);
    }
    let (g, c, worst) = stacks(&w, true);
    println!("  lorries on one point: {g} groups, {c} lorries, worst {worst}");
    if std::env::var("TRANSIT").is_ok() {
        transit(&w);
    }
    if std::env::var("RIDERSHIP").is_ok() {
        println!("  ridership {:#?}", w.ridership_report(1500));
    }
    let rings: Vec<(usize, glam::Vec2, f32, u8)> = w.roads.rings.iter().enumerate().map(|(k, r)| (k, r.center, r.radius, r.lanes)).collect();
    if std::env::var("RINGS").is_ok() {
        for (k, c, r, l) in &rings {
            println!("  ring {k} at ({:.0},{:.0}) r {r:.0} lanes {l}", c.x, c.y);
        }
    }
    if let Ok(k) = std::env::var("RING") {
        let k: usize = k.parse().unwrap();
        let at_least: usize = std::env::var("RING_Q").ok().and_then(|x| x.parse().ok()).unwrap_or(30);
        let mut dumps = 0;
        for _ in 0..days * 1440 {
            w.tick(&mut minds);
            if w.clock.minute_of_day() % 5 == 0 {
                let q = ring_queues(&w).get(&k).map_or(0, |x| x.0);
                if q >= at_least {
                    println!("== {:02}:{:02} ring {k} queue {q}", w.clock.hour(), w.clock.minute_of_day() % 60);
                    ring_dump(&w, k);
                    dumps += 1;
                    if dumps >= 3 {
                        return;
                    }
                }
            }
        }
        return;
    }
    if let Ok(k) = std::env::var("TRACE") {
        let k: u32 = k.parse().unwrap();
        let id = w.traffic.cars.keys().find(|id| slotmap::Key::data(id).as_ffi() as u32 == k).expect("car");
        for _ in 0..40 {
            w.tick(&mut minds);
            match w.traffic.cars.get(id) {
                Some(c) => println!("  t{} {:?} pos ({:.1},{:.1}) s {:.1}/{:.1} idx {}/{} entering {} hold {:.0} v {:.1} wait {:?} cargo {:?} job {:?} q {:?} bay {:?} lane {}", w.clock.tick, id, c.pos.x, c.pos.y, c.s, c.s_end, c.idx, c.route.len(), c.entering, c.hold, c.v, c.wait, c.cargo.map(|g| (w.buildings.get(g.from).map(|b| b.name.clone()), w.buildings.get(g.to).map(|b| b.name.clone()), g.amount)), w.containers.lorries.get(&id).map(|j| (j.depot, j.since, j.cargo.is_some(), j.take.len())), w.lorry_queued(id), w.lorry_bay(id), c.route[c.idx]),
                None => { println!("  gone"); break; }
            }
        }
        return;
    }
    // Per day.
    let mut empty_seen: HashSet<nd_sim::CarId> = HashSet::new();
    let mut empty_short = 0u32;
    let mut empty_runs = 0u32;
    let mut empty_m = 0.0f32;
    let mut stack_samples: Vec<(usize, usize)> = Vec::new();
    let mut short_pairs: BTreeMap<String, u32> = BTreeMap::new();
    let mut lorry_samples: Vec<[usize; 6]> = Vec::new();
    let mut ring_q: BTreeMap<usize, (usize, f32, BTreeMap<String, usize>)> = BTreeMap::new();
    let mut ring_sum: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
    let mut alert_samples = 0usize;
    let mut alert_rings: BTreeMap<usize, usize> = BTreeMap::new();
    for _ in 0..days * 1440 {
        w.tick(&mut minds);
        // Empty runs: a lorry seen first driving back empty, how far from its goal.
        for (id, car) in w.traffic.cars.iter() {
            let Some(cg) = car.cargo else { continue };
            let empty = cg.amount == 0.0 && cg.price == 0.0 && cg.resource == nd_sim::Resource::Fuel;
            if !empty {
                empty_seen.remove(&id);
                continue;
            }
            if empty_seen.insert(id) {
                empty_runs += 1;
                if let Some(q) = w.building_lot(cg.to).map(|l| l.center) {
                    let d = q.distance(car.pos);
                    empty_m += d;
                    if d < 150.0 && car.bus.is_none() {
                        empty_short += 1;
                        let name = |b: nd_sim::BuildingId| w.buildings.get(b).map_or("?".to_string(), |x| x.name.clone());
                        // Where it stands now (the place it delivered to) and where it goes.
                        let here = w.buildings.iter().filter_map(|(b, x)| w.lots.get(x.lot).map(|l| (b, l.center.distance(car.pos)))).min_by(|a, b| a.1.total_cmp(&b.1)).map(|x| x.0);
                        *short_pairs.entry(format!("{} -> {} (home {})", here.map(name).unwrap_or_default(), name(cg.to), name(cg.from))).or_insert(0u32) += 1;
                    }
                }
            }
        }
        let m = w.clock.minute_of_day();
        if m % 10 == 0 {
            let (_, c, worst) = stacks(&w, false);
            stack_samples.push((c, worst));
            if worst >= 4 && m % 60 == 0 {
                println!("  {:02}:{:02} lorries on one point, worst group {worst}:", w.clock.hour(), m % 60);
                let _ = stacks(&w, true);
            }
            let l = lorries(&w);
            lorry_samples.push(l);
            // Roundabout queues: cars standing in a queue whose head is held at the ring.
            for (k, (n, stall, heads)) in ring_queues(&w) {
                let e = ring_q.entry(k).or_default();
                if n > e.0 {
                    e.2 = heads;
                }
                e.0 = e.0.max(n);
                e.1 = e.1.max(stall);
                ring_sum.entry(k).or_insert((0, 0)).0 += n;
            }
            for (k, _, _, _) in &rings {
                ring_sum.entry(*k).or_insert((0, 0)).1 += 1;
            }
            // The roundabout_over_capacity alert's view (6+ cars behind heads 120 s+).
            let alert: Vec<(usize, usize)> = w.traffic.ring_queues(120.0).into_iter().filter(|x| x.1 .0 >= 6).map(|x| (x.0, x.1 .0)).collect();
            if !alert.is_empty() {
                alert_samples += 1;
                for (k, n) in alert {
                    let e = alert_rings.entry(k).or_insert(0usize);
                    *e = (*e).max(n);
                }
            }
        }
        if m == 1439 {
            let t = &w.economy.transit;
            let r = &w.rail.stats;
            println!("day {} end: pop {} treasury {:.0}; bus riders {} fares {:.0} upkeep {:.0}; rail riders {} fares {:.0}", w.clock.day(), w.citizens.len(), w.economy.treasury, t.riders_today, t.fares_today, t.upkeep_today, r.riders_today, r.fares_today);
            let modes: Vec<String> = nd_sim::citizens::TripMode::ALL.iter().map(|m| format!("{} {}", m.key(), w.vehicles.stats.trips_today.get(m.index()).copied().unwrap_or(0))).collect();
            println!("  trips today by mode: {}", modes.join(", "));
            depots(&w);
            let _ = stacks(&w, true);
            let worst = stack_samples.iter().map(|s| s.1).max().unwrap_or(0);
            let mean = stack_samples.iter().map(|s| s.0).sum::<usize>() as f32 / stack_samples.len().max(1) as f32;
            println!("  lorries on one point (10-min samples): mean {mean:.1} lorries in groups, worst group {worst}");
            println!("  empty runs started {empty_runs} (mean {:.0} m to go), {empty_short} under 150 m", empty_m / empty_runs.max(1) as f32);
            let mut worst_rings: Vec<(usize, usize, f32)> = ring_q.iter().map(|(k, v)| (*k, v.0, v.1)).collect();
            worst_rings.sort_by(|a, b| b.1.cmp(&a.1));
            let total: usize = ring_sum.values().map(|x| x.0).sum();
            println!("  rings: cars queued for a ring, mean over the day {:.1}", total as f32 / 144.0);
            for (k, n, s) in worst_rings.iter().take(8) {
                let (_, ctr, r, lanes) = rings[*k];
                let mean = ring_sum.get(k).map_or(0.0, |x| x.0 as f32 / x.1.max(1) as f32);
                println!("  ring {k} at ({:.0},{:.0}) r {:.0} lanes {lanes}: worst queue {n} (mean {mean:.1}), longest stall {:.0} s, heads then {:?}", ctr.x, ctr.y, r, s, ring_q[k].2);
            }
            ring_sum.clear();
            println!("  roundabout_over_capacity alert up in {alert_samples}/144 samples, rings (worst queue) {alert_rings:?}");
            alert_samples = 0;
            alert_rings.clear();
            let ns = lorry_samples.len().max(1) as f32;
            let mean = |k: usize| lorry_samples.iter().map(|x| x[k]).sum::<usize>() as f32 / ns;
            let full = lorry_samples.iter().filter(|x| x[4] - x[2] >= w.truck_cap()).count();
            println!("  site lorries (loaded, bound for a site): mean {:.1}", mean(5));
            println!("  lorries (mean of 10-min samples): total {:.1} (loaded driving {:.1}, empty driving {:.1}, in bays {:.1}, standing {:.1}); at/over the cap of {} in {full}/{} samples", mean(4), mean(0), mean(1), mean(2), mean(3), w.truck_cap(), lorry_samples.len());
            lorry_samples.clear();
            let mut sp: Vec<_> = short_pairs.iter().collect();
            sp.sort_by(|a, b| b.1.cmp(a.1));
            for (k, n) in sp.iter().take(6) {
                println!("    short empty run x{n}: {k}");
            }
            short_pairs.clear();
            empty_runs = 0;
            empty_short = 0;
            empty_m = 0.0;
            stack_samples.clear();
            ring_q.clear();
        }
    }
    if let Some(out) = args.get(3) {
        let f = std::io::BufWriter::new(std::fs::File::create(out).expect("save file"));
        w.write_ron_gz(f).expect("save");
        println!("saved {out}");
    }
}
