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
            match w.widen_roundabout(r.center, Some(r.radius), lanes) {
                Ok(id) => println!("  widen ring {k} at ({:.0},{:.0}) r {:.1} lanes {} -> gesture {} {:?}", r.center.x, r.center.y, r.radius, r.lanes, id.0, w.gesture(id).and_then(|g| g.ring()).map(|x| (x.radius, x.lanes))),
                Err(e) => println!("  widen ring {k}: {e}"),
            }
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
    let t1 = std::time::Instant::now();
    for _ in 0..hours * 60 {
        w.tick(&mut minds);
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
