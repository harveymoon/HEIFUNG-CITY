//! [r2-traffic] Headless survey of turnarounds and trip times on a save:
//! how often vehicles drive round a dead end (out and back), U-turn at a
//! signalled junction or turn round illegally in the street, and how long
//! timed trips take against their expected time. Runs
//! `deadend_probe <save.ron> [ticks]`; `WATCH=<car ffi id>` also follows one
//! car. Not part of the game.
use nd_minds::{HybridMind, MockBackend};
use nd_sim::traffic::LaneKind;
use nd_sim::World;
use slotmap::Key;
use std::collections::HashMap;

#[derive(Default)]
struct Trip {
    start: u64,
    expected: f32,
    last_seen: u64,
    dead_ends: u32,
    uturns: u32,
}

/// Vehicles standing behind one that is not waiting in traffic itself
/// (unloading, crashed, at work): who holds up whom.
fn blockers(w: &World) {
    use nd_sim::traffic::StopReason;
    let t = &w.traffic;
    let mut by: HashMap<nd_sim::CarId, u32> = HashMap::new();
    for (id, c) in t.cars.iter() {
        if c.v < 0.5 && c.stalled > 30.0 {
            if let StopReason::Queued { behind } = t.stop_reason(id) {
                let r = t.stop_reason(behind);
                if matches!(r, StopReason::Unloading | StopReason::Crashed { .. } | StopReason::AtWork) {
                    *by.entry(behind).or_default() += 1;
                }
            }
        }
    }
    for (b, n) in by {
        let c = &t.cars[b];
        let lane = c.route[c.idx];
        let l = &t.net.lanes[lane as usize];
        println!("   BLOCKER {} {:?} behind it {n}: {} body {:?} entering {} hold {:.0} lateral {:.1} kerb_lane {} conn {} pos ({:.0},{:.0}) errand {:?}", w.car_label(b), b, t.stop_reason(b).key(), c.model.body, c.entering, c.hold, c.lateral, l.right.is_none(), l.is_connector(), c.pos.x, c.pos.y, c.errand);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).cloned().expect("save path");
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(240);
    let watch: Option<u64> = std::env::var("WATCH").ok().and_then(|s| s.parse().ok());
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    let t0 = std::time::Instant::now();
    let mut w = World::from_ron(&std::fs::read_to_string(&src).unwrap()).expect("load");
    eprintln!("loaded in {:.1}s", t0.elapsed().as_secs_f32());
    let rage0 = w.driving.rage_u_turns_total;
    let (jobs0, towed0) = (w.enforcement.jobs.len(), w.enforcement.stats.tows_total);
    for (b, bld) in w.buildings.iter().filter(|(_, b)| b.business.as_ref().is_some_and(|z| z.kind == nd_sim::BusinessKind::TowYard)) {
        println!("tow yard {:?} {} workers {} jobs {} trucks {} operating {} closed {:?}", b, bld.name, bld.workers.len(), bld.jobs, w.tow_trucks(b), bld.operating(), bld.business.as_ref().map(|z| z.closed));
    }
    let mut by: std::collections::BTreeMap<String, usize> = Default::default();
    for j in &w.enforcement.jobs {
        *by.entry(format!("{:?} local {} truck {}", j.reason, j.company.is_some(), j.truck.is_some())).or_default() += 1;
    }
    println!("tow jobs by kind: {by:?}");
    if let Some(p) = w.pressure.readings.iter().find(|p| p.kind == nd_sim::pressure::PressureKind::TowCompany) {
        println!("tow pressure: {p:?}");
    }
    // Per car: the lane it was on at the last look, and its trip.
    let mut last_lane: HashMap<u64, u32> = HashMap::new();
    let mut trips: HashMap<u64, Trip> = HashMap::new();
    let mut done: Vec<Trip> = Vec::new();
    let (mut dead_end_total, mut uturn_total) = (0u64, 0u64);
    let mut by_node: HashMap<u32, u32> = HashMap::new();
    // Per car, how often it drove onto each road lane: a car going round
    // in circles drives onto the same lanes again and again.
    let mut visits: HashMap<u64, HashMap<u32, u16>> = HashMap::new();
    let mut labels: HashMap<u64, String> = HashMap::new();
    for i in 0..ticks {
        w.tick(&mut minds);
        let tick = w.clock.tick;
        let net = &w.traffic.net;
        let mut alive = std::collections::HashSet::new();
        for (id, c) in w.traffic.cars.iter() {
            let key = id.data().as_ffi();
            alive.insert(key);
            // A new trip on the same car closes the old one.
            let t = trips.entry(key).or_default();
            if c.drive.expected_min > 0.0 && t.start != c.drive.trip_start {
                if t.expected > 0.0 {
                    done.push(std::mem::take(t));
                }
                *t = Trip { start: c.drive.trip_start, expected: c.drive.expected_min, last_seen: tick, ..Default::default() };
            }
            t.last_seen = tick;
            let Some(&lane) = c.route.get(c.idx) else { continue };
            if last_lane.insert(key, lane) == Some(lane) {
                continue;
            }
            let l = &net.lanes[lane as usize];
            if !l.is_connector() {
                let n = visits.entry(key).or_default().entry(lane).or_default();
                *n += 1;
                if *n >= 3 {
                    labels.entry(key).or_insert_with(|| format!("{} {:?} errand {:?}", w.car_label(id), c.model.body, c.errand));
                }
            }
            if let LaneKind::Connector { node, .. } = l.kind {
                let inter = net.node_intersection.get(node.0 as usize).and_then(|&k| net.intersections.get(k));
                if inter.is_some_and(|x| x.approaches == 1) {
                    dead_end_total += 1;
                    t.dead_ends += 1;
                    *by_node.entry(node.0).or_default() += 1;
                } else if l.uturn {
                    uturn_total += 1;
                    t.uturns += 1;
                }
            }
            if watch == Some(key) {
                println!("  watch tick {tick} lane {lane} conn {} pos ({:.0},{:.0}) v {:.1} stalled {:.0} anger {:.2} route {}", l.is_connector(), c.pos.x, c.pos.y, c.v, c.stalled, c.drive.anger, c.route.len() - c.idx);
            }
        }
        // Cars gone: their trip is over.
        let gone: Vec<u64> = trips.keys().copied().filter(|k| !alive.contains(k)).collect();
        for k in gone {
            let t = trips.remove(&k).unwrap();
            last_lane.remove(&k);
            if t.expected > 0.0 {
                if watch == Some(k) {
                    println!("  watch: arrived tick {} after {} min (expected {:.0})", t.last_seen, t.last_seen - t.start, t.expected);
                }
                done.push(t);
            }
        }
        if std::env::var("BLOCKERS").is_ok() && (i + 1) % 30 == 0 {
            blockers(&w);
        }
        if (i + 1) % 60 == 0 {
            eprintln!("+{} day {} {:02}:{:02} vehicles {} dead-end turns {dead_end_total} junction U-turns {uturn_total}", i + 1, w.clock.day(), w.clock.hour(), w.clock.minute_of_day() % 60, w.traffic.cars.len());
        }
    }
    // Trips that ended (arrived, or a new trip began) within the run and began in it.
    let t_end = w.clock.tick;
    let start_tick = t_end - ticks;
    let ended: Vec<&Trip> = done.iter().filter(|t| t.start >= start_tick).collect();
    let n = ended.len().max(1) as f64;
    let mean_min = ended.iter().map(|t| (t.last_seen - t.start) as f64).sum::<f64>() / n;
    let mean_exp = ended.iter().map(|t| t.expected as f64).sum::<f64>() / n;
    let mut ratios: Vec<f64> = ended.iter().map(|t| (t.last_seen - t.start) as f64 / (t.expected as f64).max(1.0)).collect();
    ratios.sort_by(|a, b| a.total_cmp(b));
    let pct = |p: f64| ratios.get(((ratios.len() as f64 - 1.0) * p).round() as usize).copied().unwrap_or(0.0);
    let looping = ended.iter().filter(|t| t.dead_ends >= 2).count();
    // Still on the road at the end: how long so far.
    let open: Vec<&Trip> = trips.values().filter(|t| t.expected > 0.0).collect();
    let over = open.iter().filter(|t| (t_end - t.start) as f32 > t.expected * 3.0 && t_end - t.start > 60).count();
    println!("tow jobs at start {jobs0} at end {} towed {} (waiting now {})", w.enforcement.jobs.len(), w.enforcement.stats.tows_total - towed0, w.enforcement.jobs.iter().filter(|j| matches!(j.state, nd_sim::enforcement::TowState::Waiting { .. })).count());
    let mut nodes: Vec<_> = by_node.into_iter().collect();
    nodes.sort_by(|a, b| b.1.cmp(&a.1));
    println!("worst dead ends (node, turnarounds): {:?}", &nodes[..nodes.len().min(10)]);
    // Cars that drove onto one lane three times or more: going in circles.
    let mut circling: Vec<(u16, u64)> = visits.iter().map(|(k, v)| (*v.values().max().unwrap_or(&0), *k)).filter(|x| x.0 >= 3).collect();
    circling.sort_by(|a, b| b.cmp(a));
    for (n, k) in circling.iter().take(12) {
        println!("   CIRCLING {n}x one lane: {} ({k})", labels.get(k).map_or("", |s| s.as_str()));
    }
    let circling_n = circling.len();
    println!(
        "SUMMARY ticks {ticks} dead_end_turnarounds {dead_end_total} junction_uturns {uturn_total} rage_uturns {} trips_done {} mean_trip_min {mean_min:.1} mean_expected_min {mean_exp:.1} ratio_p50 {:.2} p90 {:.2} p99 {:.2} trips_with_2plus_dead_ends {looping} circling_cars {circling_n} still_driving {} of_them_over_3x {over} wall {:.0}s",
        w.driving.rage_u_turns_total - rage0,
        ended.len(),
        pct(0.5),
        pct(0.9),
        pct(0.99),
        open.len(),
        t0.elapsed().as_secs_f32()
    );
}
