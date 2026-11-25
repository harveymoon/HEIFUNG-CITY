//! [r17-roundabouts] Probe for two-lane roundabouts on a save: per ring,
//! the cars queued behind it (mean and worst of 10-minute samples), the
//! longest stall, and the cars through it (entries) over a day; with
//! `WIDEN` the listed rings are widened first (keeping their outer edge)
//! and the plan implemented. Population and treasury at the start and the
//! end. Not part of the game.
//!
//! `roundabouts_r17_probe <save.ron> [hours=24] [out.ron]`
//! env `WIDEN="919,-59;700,-91"` (ring centres; nearest ring within 30 m),
//! `WIDEN_LANES=2`, `RINGS=1` (list every ring), `WATCH="919,-59;..."`
//! (rings to report whatever their rank).
use nd_minds::{HybridMind, MockBackend};
use nd_sim::World;
use std::collections::{BTreeMap, HashMap};

fn parse_points(s: &str) -> Vec<glam::Vec2> {
    s.split(';').filter_map(|p| {
        let mut it = p.split(',').map(|x| x.trim().parse::<f32>());
        Some(glam::Vec2::new(it.next()?.ok()?, it.next()?.ok()?))
    }).collect()
}

fn ring_near(w: &World, p: glam::Vec2) -> Option<usize> {
    w.roads.rings.iter().enumerate().filter(|(_, r)| r.center.distance(p) < 30.0).min_by(|a, b| a.1.center.distance(p).total_cmp(&b.1.center.distance(p))).map(|x| x.0)
}

/// Cars standing (30 s+) in a queue whose head is held at a ring's entry,
/// per ring; and that head's stall.
fn queues(w: &World) -> BTreeMap<usize, (usize, f32)> {
    w.traffic.ring_queues(30.0)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let hours: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(24);
    let t0 = std::time::Instant::now();
    let mut w = World::read_save(&args[1]).expect("load");
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    println!("load day {} {:02}:{:02} pop {} treasury {:.0} ({:.0} s)", w.clock.day(), w.clock.hour(), w.clock.minute_of_day() % 60, w.citizens.len(), w.economy.treasury, t0.elapsed().as_secs_f32());
    if std::env::var("RINGS").is_ok() {
        for (k, r) in w.roads.rings.iter().enumerate() {
            println!("  ring {k} at ({:.0},{:.0}) r {:.1} lanes {} level {:.0}", r.center.x, r.center.y, r.radius, r.lanes, r.level);
        }
    }
    let lanes: u8 = std::env::var("WIDEN_LANES").ok().and_then(|s| s.parse().ok()).unwrap_or(2);
    let mut watch: Vec<glam::Vec2> = std::env::var("WATCH").map(|s| parse_points(&s)).unwrap_or_default();
    if let Ok(s) = std::env::var("WIDEN") {
        let pts = parse_points(&s);
        watch.extend(pts.iter().copied());
        let before = w.economy.treasury;
        for p in pts {
            let Some(k) = ring_near(&w, p) else {
                println!("  no ring near ({:.0},{:.0})", p.x, p.y);
                continue;
            };
            let r = w.roads.rings[k];
            // By hand (outer edge kept), so the probe also builds on main.
            let Some((id, pr)) = w.effective_plan().roundabouts().filter(|(_, x)| x.center.distance(r.center) < 1.0 && (x.radius - r.radius).abs() < 0.5).next() else { continue };
            let outer = pr.radius + pr.half_width();
            let radius = outer - nd_sim::roads::half_width_spec(nd_sim::roads::RoadKind::Street, lanes, 0);
            w.set_gesture(id, nd_sim::Gesture::Roundabout { center: pr.center, radius, lanes, level: pr.level }, true);
            println!("  widen ring {k} at ({:.0},{:.0}) r {:.1} lanes {} -> r {radius:.2} lanes {lanes}", r.center.x, r.center.y, r.radius, r.lanes);
        }
        let t = std::time::Instant::now();
        let r = w.try_implement();
        println!("  implement: {r:?} ({:.0} s); treasury {:.0} -> {:.0}; warnings {:?}", t.elapsed().as_secs_f32(), before, w.economy.treasury, w.plan_warnings);
    }
    let watch_rings: Vec<usize> = watch.iter().filter_map(|&p| ring_near(&w, p)).collect();
    // Marked crossings near the watched rings: how far their near edge is
    // from the ring's outer kerb line.
    if std::env::var("CROSS").is_ok() {
        for &k in &watch_rings {
            let r = w.roads.rings[k];
            let edge = r.radius + r.half_width();
            println!("  ring {k} at ({:.0},{:.0}) r {:.1} lanes {} outer edge {edge:.1}", r.center.x, r.center.y, r.radius, r.lanes);
            for (i, c) in w.walk.crossings.iter().enumerate() {
                let d = c.mid.distance(r.center);
                if d > edge + 40.0 || !c.marked() {
                    continue;
                }
                let at_ring = c.node.is_some_and(|n| w.roads.ring_of_node(n) == Some(k));
                println!("    crossing {i} {:?} at ({:.1},{:.1}) {:.1} m out from the ring edge, node {:?} (ring node {at_ring}) lanes {}", c.kind, c.mid.x, c.mid.y, d - edge, c.node, c.lanes);
            }
        }
    }
    let rings: Vec<(glam::Vec2, f32, u8)> = w.roads.rings.iter().map(|r| (r.center, r.radius, r.lanes)).collect();
    let pop0 = w.citizens.len();
    let tr0 = w.economy.treasury;
    // Entry connectors by ring, for the cars through each ring.
    let mut entry_ring: HashMap<u32, usize> = HashMap::new();
    let mut version = u64::MAX;
    let mut last_lane: HashMap<nd_sim::CarId, u32> = HashMap::new();
    let mut through: BTreeMap<usize, usize> = BTreeMap::new();
    let mut q_sum: BTreeMap<usize, usize> = BTreeMap::new();
    let mut q_max: BTreeMap<usize, (usize, f32)> = BTreeMap::new();
    let mut samples = 0usize;
    let mut gridlock_samples = 0usize;
    let mut alert_samples = 0usize;
    // City-wide: vehicles that left the road (trip done or given up) and
    // their age then; per watched ring, distinct cars that came near it,
    // distinct cars on its ring lanes, and car-minutes standing near it.
    let mut alive: HashMap<nd_sim::CarId, (f32, bool)> = HashMap::new();
    let (mut finished, mut age_sum, mut lorries_done) = (0usize, 0.0f64, 0usize);
    let mut near_cars: BTreeMap<usize, std::collections::HashSet<nd_sim::CarId>> = BTreeMap::new();
    let mut on_ring: BTreeMap<usize, std::collections::HashSet<nd_sim::CarId>> = BTreeMap::new();
    let mut stand_near: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
    let mut ring_of_lane: HashMap<u32, usize> = HashMap::new();
    let mut reasons: BTreeMap<(usize, String), usize> = BTreeMap::new();
    let t1 = std::time::Instant::now();
    for _ in 0..hours * 60 {
        w.tick(&mut minds);
        {
            let now: std::collections::HashSet<nd_sim::CarId> = w.traffic.cars.keys().collect();
            alive.retain(|id, (age, cargo)| {
                if now.contains(id) {
                    return true;
                }
                finished += 1;
                age_sum += *age as f64;
                if *cargo {
                    lorries_done += 1;
                }
                false
            });
            for (id, c) in w.traffic.cars.iter() {
                alive.insert(id, (c.age, c.cargo.is_some()));
            }
            if ring_of_lane.is_empty() || w.traffic.net.lanes.len() as u64 != version {
                ring_of_lane.clear();
                for &l in &w.traffic.net.rings.circulating {
                    if let nd_sim::traffic::LaneKind::Road { segment, .. } = w.traffic.net.lanes[l as usize].kind {
                        if let Some(k) = w.roads.ring_of_segment(segment) {
                            ring_of_lane.insert(l, k);
                        }
                    }
                }
            }
            for &k in &watch_rings {
                let r = w.roads.rings[k];
                let reach = r.radius + r.half_width() + 80.0;
                for (id, c) in w.traffic.cars.iter() {
                    if c.pos.distance(r.center) < reach {
                        near_cars.entry(k).or_default().insert(id);
                        if c.v < 0.5 && c.hold <= 0.0 && !c.entering {
                            let e = stand_near.entry(k).or_default();
                            if c.pos.distance(r.center) < r.radius + r.half_width() + 1.0 { e.0 += 1 } else { e.1 += 1 }
                            // Why: the head of its queue, and where that head is.
                            let head = w.traffic.jam_chain(id).cars.last().copied().unwrap_or(id);
                            let h = &w.traffic.cars[head];
                            let why = format!("{:?}", w.traffic.stop_reason(head));
                            let why = why.split(|ch: char| ch == ' ' || ch == '{').next().unwrap_or("").to_string();
                            let hd = h.pos.distance(r.center);
                            let at = if hd < r.radius + r.half_width() + 1.0 { "ring" } else if hd < r.radius + 40.0 { "near" } else if hd < r.radius + r.half_width() + 80.0 { "zone" } else { "beyond" };
                            *reasons.entry((k, format!("{why} head {at}"))).or_default() += 1;
                        }
                    }
                    if c.route.get(c.idx).and_then(|l| ring_of_lane.get(l)) == Some(&k) {
                        on_ring.entry(k).or_default().insert(id);
                    }
                }
            }
        }
        if w.traffic.net.lanes.len() as u64 != version {
            version = w.traffic.net.lanes.len() as u64;
            entry_ring.clear();
            for rn in w.traffic.net.rings.nodes.values() {
                for &e in &rn.entries {
                    entry_ring.insert(e, rn.ring);
                }
            }
        }
        for (id, c) in w.traffic.cars.iter() {
            let Some(&here) = c.route.get(c.idx) else { continue };
            if last_lane.insert(id, here) != Some(here) {
                if let Some(&k) = entry_ring.get(&here) {
                    *through.entry(k).or_default() += 1;
                }
            }
        }
        let m = w.clock.minute_of_day();
        if m == 1439 {
            let modes: Vec<String> = nd_sim::citizens::TripMode::ALL.iter().map(|md| format!("{} {}", md.key(), w.vehicles.stats.trips_today.get(md.index()).copied().unwrap_or(0))).collect();
            println!("  day {} trips by mode: {}", w.clock.day(), modes.join(", "));
        }
        if m % 10 == 0 && std::env::var("HEADS").is_ok() {
            // Heads held 300 s+ at a watched ring: where they stand.
            for (id, c) in w.traffic.cars.iter() {
                if c.stalled < 300.0 || c.v > 0.5 {
                    continue;
                }
                let Some(&here) = c.route.get(c.idx) else { continue };
                let Some(&next) = c.route.get(c.idx + 1) else { continue };
                let nl = &w.traffic.net.lanes[next as usize];
                let nd_sim::traffic::LaneKind::Connector { node, .. } = nl.kind else { continue };
                let Some(rn) = w.traffic.net.rings.nodes.get(&node) else { continue };
                let on_ring = w.traffic.net.rings.circulating.contains(&here);
                let what = if rn.exits.contains(&next) { "exit" } else if rn.entries.contains(&next) { "entry" } else { "through" };
                println!("    {:02}:{:02} ring {} car {:?} on_ring {on_ring} next {what} stalled {:.0} wait {:?} lane {:?} at ({:.0},{:.0})", w.clock.hour(), m % 60, rn.ring, id, c.stalled, c.wait, w.traffic.net.lanes[here as usize].kind, c.pos.x, c.pos.y);
            }
        }
        if m % 30 == 0 {
            if let Ok(spec) = std::env::var("JAMS") {
                // The biggest queues near a ring: their head, why, where.
                for p in parse_points(&spec) {
                    let Some(k) = ring_near(&w, p) else { continue };
                    let r = w.roads.rings[k];
                    let reach = r.radius + r.half_width() + 80.0;
                    let mut heads: HashMap<nd_sim::CarId, usize> = HashMap::new();
                    for (id, c) in w.traffic.cars.iter() {
                        if c.v < 0.5 && c.hold <= 0.0 && !c.entering && c.pos.distance(r.center) < reach {
                            let head = w.traffic.jam_chain(id).cars.last().copied().unwrap_or(id);
                            *heads.entry(head).or_default() += 1;
                        }
                    }
                    let mut v: Vec<(nd_sim::CarId, usize)> = heads.into_iter().collect();
                    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                    for (h, n) in v.into_iter().take(4).filter(|x| x.1 >= 5) {
                        let c = &w.traffic.cars[h];
                        let here = c.route[c.idx];
                        let next = c.route.get(c.idx + 1).copied();
                        let nk = next.map(|x| format!("{:?}", w.traffic.net.lanes[x as usize].kind));
                        let ring_node = next.and_then(|x| match w.traffic.net.lanes[x as usize].kind { nd_sim::traffic::LaneKind::Connector { node, .. } => w.traffic.net.rings.nodes.get(&node).map(|rn| (rn.ring, if rn.exits.contains(&x) { "exit" } else if rn.entries.contains(&x) { "entry" } else { "through" })), _ => None });
                        println!("    jam {:02}:{:02} ring {k}: {n} behind {:?} at ({:.0},{:.0}) v {:.1} stalled {:.0} why {:?} lane {:?} on_ring {} next {:?} {:?} wait {:?}", w.clock.hour(), m % 60, h, c.pos.x, c.pos.y, c.v, c.stalled, w.traffic.stop_reason(h), w.traffic.net.lanes[here as usize].kind, w.traffic.net.rings.circulating.contains(&here), ring_node, nk, c.wait);
                    }
                }
            }
        }
        if m % 10 == 0 {
            samples += 1;
            for (k, (n, stall)) in queues(&w) {
                *q_sum.entry(k).or_default() += n;
                let e = q_max.entry(k).or_default();
                e.0 = e.0.max(n);
                e.1 = e.1.max(stall);
            }
            if !w.traffic.gridlocks().is_empty() {
                gridlock_samples += 1;
            }
            if w.traffic.ring_queues(120.0).values().any(|x| x.0 >= 6) {
                alert_samples += 1;
            }
            if m % 120 == 0 {
                let total: usize = queues(&w).values().map(|x| x.0).sum();
                println!("  {:02}:00 cars {} queued at rings {total} ({:.0} s)", w.clock.hour(), w.traffic.cars.len(), t1.elapsed().as_secs_f32());
            }
        }
    }
    let ns = samples.max(1) as f32;
    println!("end day {} {:02}:{:02}: pop {pop0} -> {} treasury {tr0:.0} -> {:.0}; ran {:.0} s", w.clock.day(), w.clock.hour(), w.clock.minute_of_day() % 60, w.citizens.len(), w.economy.treasury, t1.elapsed().as_secs_f32());
    println!("  city: vehicles off the road {finished} (lorries {lorries_done}), mean age then {:.0} s", age_sum / finished.max(1) as f64);
    for &k in &watch_rings {
        let (on, arms) = stand_near.get(&k).copied().unwrap_or_default();
        println!("  ring {k:2}: distinct cars within 80 m {}, distinct cars on its lanes {}, car-minutes standing on the ring {on}, within 80 m off it {arms}", near_cars.get(&k).map_or(0, |s| s.len()), on_ring.get(&k).map_or(0, |s| s.len()));
    }
    for &k in &watch_rings {
        let mut v: Vec<(&String, &usize)> = reasons.iter().filter(|((kk, _), _)| *kk == k).map(|((_, s), n)| (s, n)).collect();
        v.sort_by(|a, b| b.1.cmp(a.1));
        println!("  ring {k:2} standing car-minutes by queue head: {}", v.iter().take(8).map(|(s, n)| format!("{s} {n}")).collect::<Vec<_>>().join(", "));
    }
    let total: usize = q_sum.values().sum();
    println!("  all rings: mean queued {:.1}, cars through {}; gridlock in {gridlock_samples}/{samples} samples; alert up in {alert_samples}/{samples}", total as f32 / ns, through.values().sum::<usize>());
    let mut order: Vec<usize> = q_sum.keys().copied().collect();
    order.sort_by_key(|k| std::cmp::Reverse(q_sum[k]));
    let mut shown: Vec<usize> = order.iter().take(8).copied().collect();
    for k in &watch_rings {
        if !shown.contains(k) {
            shown.push(*k);
        }
    }
    for k in shown {
        let (c, r, l) = rings[k];
        let (worst, stall) = q_max.get(&k).copied().unwrap_or_default();
        println!("  ring {k:2} at ({:5.0},{:5.0}) r {r:5.1} lanes {l}: mean queue {:5.1}, worst {worst:4}, longest stall {stall:5.0} s, cars through {}{}", c.x, c.y, q_sum.get(&k).copied().unwrap_or(0) as f32 / ns, through.get(&k).copied().unwrap_or(0), if watch_rings.contains(&k) { "  (watched)" } else { "" });
    }
    if let Some(out) = args.get(3) {
        let f = std::io::BufWriter::new(std::fs::File::create(out).expect("save file"));
        w.write_ron_gz(f).expect("save");
        println!("saved {out}");
    }
}
