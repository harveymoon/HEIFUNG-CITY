//! Where a big city's ticks go. Grows the demo district's layout tiled
//! 2x2 (an 8x8 grid of blocks) to a target population, saves it, and
//! measures any saved city tick by tick with the `prof!` section timers.
//!
//! cargo run --release -p nd_sim --features profile --example perf_probe -- grow 4000 city.ron
//! cargo run --release -p nd_sim --features profile --example perf_probe -- measure city.ron 1440
//!
//! [r6-perf] `measure` also takes `ND_FINGERPRINT=1` (a hash of the world
//! after the run, its lines sorted: two builds that print the same one
//! simulated the same city; `ND_DUMP=<file>` writes the RON), `ND_SLOW_MS`
//! (ticks slower than this are listed with their top sections) and
//! `ND_GUARD=1` (fail when a section is over its budget, see the end).
//! `run <city.ron> <days> [out]` runs a city on and saves it (growing big
//! cities); `people <city.ron>` times the world queries the renderer makes
//! for every figure outdoors.

use nd_minds::RuleBasedMind;
use nd_sim::roads::RoadKind;
use nd_sim::{Gesture, LandUse, PowerPlantKind, Vec2, World, WorldConfig};
use std::time::Instant;

fn big_city() -> World {
    let config = WorldConfig { max_builds_per_interval: 8, immigration_interval: 6, ..WorldConfig::default() };
    let mut world = World::new(config);
    world.drain_events();
    world.start_project();
    // [r6-perf] `ND_GRID=<n>`: n x n blocks (default 8) for bigger cities.
    let n: i32 = std::env::var("ND_GRID").ok().and_then(|v| v.parse().ok()).unwrap_or(8).max(2) / 2 * 2;
    let half = n / 2;
    let span = 140.0 * half as f32;
    for i in -half..=half {
        let c = i as f32 * 140.0;
        world.add_gesture(Gesture::Road { points: vec![Vec2::new(-span, c), Vec2::new(span, c)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
        world.add_gesture(Gesture::Road { points: vec![Vec2::new(c, -span), Vec2::new(c, span)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
    }
    let cell = |cx: f32, cy: f32| vec![Vec2::new(cx - 65.0, cy - 65.0), Vec2::new(cx + 65.0, cy - 65.0), Vec2::new(cx + 65.0, cy + 65.0), Vec2::new(cx - 65.0, cy + 65.0)];
    // The demo district's 4x4 plan, tiled.
    use LandUse::*;
    let tile = [
        [Industrial, Industrial, Residential, Residential],
        [Commercial, Entertainment, Commercial, Residential],
        [Residential, Park, Civic, Commercial],
        [Residential, Residential, Entertainment, Residential],
    ];
    for gy in 0..n as usize {
        for gx in 0..n as usize {
            let lu = tile[gy % 4][gx % 4];
            // Mostly homes outside the demo's own industrial row.
            let lu = if lu == Industrial && gy >= 4 { Residential } else { lu };
            let (x, y) = (-span + 70.0 + gx as f32 * 140.0, -span + 70.0 + gy as f32 * 140.0);
            world.add_gesture(Gesture::Zone { polygon: cell(x, y), land_use: lu });
        }
    }
    world.implement();
    world.economy.treasury = 2_000_000.0;
    world.link_to_highway(Vec2::new(0.0, span));
    for _ in 0..(3 * (n * n) as usize / 64).max(3) {
        let lot = world.lots.iter().filter(|(_, l)| l.usable() && l.land_use == LandUse::Industrial && l.building.is_none() && l.area() >= 600.0).min_by(|a, b| (a.1.area() - nd_sim::power::COAL_REFERENCE_AREA).abs().total_cmp(&(b.1.area() - nd_sim::power::COAL_REFERENCE_AREA).abs())).map(|(id, _)| id);
        if let Some(lot) = lot {
            let _ = world.build_power(lot, PowerPlantKind::Coal);
        }
    }
    world
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut minds = RuleBasedMind::default();
    match args.get(1).map(String::as_str) {
        Some("grow") => {
            let target: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(4000);
            let out = args.get(3).cloned().unwrap_or_else(|| "perf_city.ron".into());
            let mut w = big_city();
            let t0 = Instant::now();
            let mut settle = None;
            let max_days: u64 = std::env::var("ND_DAYS").ok().and_then(|v| v.parse().ok()).unwrap_or(60);
            for day in 0..max_days {
                for _ in 0..1440 {
                    w.tick(&mut minds);
                    // At the target: no more newcomers, and a day to settle in.
                    if settle.is_none() && w.citizens.len() >= target {
                        w.config.immigration_interval = u64::MAX / 4;
                        settle = Some(day + 1);
                    }
                }
                w.economy.treasury = w.economy.treasury.max(1_000_000.0);
                let _ = nd_sim::prof::take();
                println!("day {:>2}: pop {:>5} buildings {:>4} cars {:>4} ({:.0} s)", day + 1, w.citizens.len(), w.buildings.len(), w.traffic.cars.len(), t0.elapsed().as_secs_f64());
                if settle.is_some_and(|d| day >= d) {
                    break;
                }
                if (day + 1) % 5 == 0 {
                    w.write_ron_gz(std::io::BufWriter::new(std::fs::File::create(&out).expect("create"))).expect("save");
                }
            }
            w.write_ron_gz(std::io::BufWriter::new(std::fs::File::create(&out).expect("create"))).expect("save");
            println!("saved {out}");
        }
        Some("measure") => {
            let path = args.get(2).expect("save path");
            let ticks: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1440);
            let mut w = World::read_save(path).expect("load");
            println!("pop {} buildings {} cars {} walk nodes {} tow jobs {}", w.citizens.len(), w.buildings.len(), w.traffic.cars.len(), w.walk.nodes.len(), w.enforcement.jobs.len());
            let _ = nd_sim::prof::take();
            let mut times: Vec<f64> = Vec::new();
            // [r6-perf] Each tick's sections, added up here, so a slow tick
            // can say where it went.
            let mut table: std::collections::BTreeMap<&'static str, nd_sim::prof::Entry> = Default::default();
            let mut events: std::collections::BTreeMap<String, u64> = Default::default();
            let slow_ms: f64 = std::env::var("ND_SLOW_MS").ok().and_then(|s| s.parse().ok()).unwrap_or(100.0);
            for _ in 0..ticks {
                let t = Instant::now();
                w.tick(&mut minds);
                let dt = t.elapsed().as_secs_f64() * 1000.0;
                let this = nd_sim::prof::take();
                // [r6-perf] What the renderer is told each tick.
                for e in w.drain_events() {
                    let name = format!("{e:?}");
                    *events.entry(name.split(['(', ' ', '{']).next().unwrap_or("").to_string()).or_insert(0u64) += 1;
                }
                // [saves] The ticks a player would feel as a hitch.
                if dt > slow_ms {
                    let mut top: Vec<_> = this.iter().map(|(n, e)| (*n, e.total * 1000.0)).collect();
                    top.sort_by(|a, b| b.1.total_cmp(&a.1));
                    let top: Vec<String> = top.iter().take(5).map(|(n, ms)| format!("{n} {ms:.0}")).collect();
                    println!("slow tick {} (day {} {}): {dt:.0} ms: {}", w.clock.tick, w.clock.day() + 1, w.clock.format_hhmm(), top.join(", "));
                }
                for (n, e) in this {
                    let t = table.entry(n).or_default();
                    t.calls += e.calls;
                    t.total += e.total;
                    t.max = t.max.max(e.max);
                }
                times.push(dt);
            }
            // [r6-perf] What the run left behind: two builds that print the
            // same fingerprint simulated the same city.
            if std::env::var_os("ND_FINGERPRINT").is_some() {
                let ron = w.to_ron().expect("save");
                // Over the sorted lines: a few maps (tourism's per-building
                // tallies) are `HashMap`s and save in a different order
                // every run, which says nothing about the simulation.
                let mut lines: Vec<&str> = ron.lines().collect();
                lines.sort_unstable();
                let mut h: u64 = 0xcbf29ce484222325;
                for l in lines {
                    for b in l.bytes().chain(std::iter::once(b'\n')) {
                        h ^= b as u64;
                        h = h.wrapping_mul(0x100000001b3);
                    }
                }
                if let Some(dump) = std::env::var_os("ND_DUMP") {
                    std::fs::write(dump, &ron).expect("dump");
                }
                println!("fingerprint {h:016x} ({} bytes) treasury {:.2} pop {} cars {}", ron.len(), w.economy.treasury, w.citizens.len(), w.traffic.cars.len());
            }
            let total: f64 = times.iter().sum();
            let mut sorted = times.clone();
            sorted.sort_by(f64::total_cmp);
            let pct = |q: f64| sorted[((sorted.len() - 1) as f64 * q) as usize];
            println!("ticks {ticks}: mean {:.2} ms, p50 {:.2}, p99 {:.2}, max {:.2} ms; pop now {}", total / ticks as f64, pct(0.5), pct(0.99), pct(1.0), w.citizens.len());
            println!("events: {events:?}");
            println!("parks found full {} | bench meetings {} | cyclists ran reds {}", w.seats.full_total, w.seats.met_total, w.peds.red_lights_run_total);
            let mut rows: Vec<_> = table.into_iter().collect();
            rows.sort_by(|a, b| b.1.total.total_cmp(&a.1.total));
            println!("{:<26} {:>8} {:>10} {:>10} {:>9}", "section", "calls", "total ms", "ms/tick", "max ms");
            for (name, e) in rows.iter().take(70) {
                println!("{:<26} {:>8} {:>10.1} {:>10.3} {:>9.2}", name, e.calls, e.total * 1000.0, e.total * 1000.0 / ticks as f64, e.max * 1000.0);
            }
            // [r6-perf] The regression guard: `ND_GUARD=1` checks the run
            // against budgets (ms a tick for a section, `tick` for the mean
            // tick, `max` for the slowest), the defaults below or
            // `ND_BUDGET=section=ms,...`, and fails when one blows up. The
            // defaults are about three times what Harvey's day-65 city
            // (15,000 people) takes after r6-perf, so only a real blow-up
            // (an O(n^2) scan, a search gone wide) trips them.
            if std::env::var_os("ND_GUARD").is_some() {
                let mut budget: Vec<(String, f64)> = [("tick", 100.0), ("max", 600.0), ("step_citizens", 60.0), ("step_traffic", 30.0), ("walk_astar", 6.0), ("tr_lane_changes", 12.0), ("hourly_society", 5.0), ("step_tow_jobs", 3.0), ("lane_search", 10.0)].iter().map(|(k, v)| (k.to_string(), *v)).collect();
                if let Ok(spec) = std::env::var("ND_BUDGET") {
                    for kv in spec.split(',').filter(|s| !s.is_empty()) {
                        let Some((k, v)) = kv.split_once('=') else { continue };
                        let Ok(v) = v.trim().parse::<f64>() else { continue };
                        budget.retain(|(n, _)| n != k.trim());
                        budget.push((k.trim().to_string(), v));
                    }
                }
                let mut over = Vec::new();
                for (name, limit) in &budget {
                    let got = match name.as_str() {
                        "tick" => total / ticks as f64,
                        "max" => pct(1.0),
                        n => rows.iter().find(|(r, _)| *r == n).map_or(0.0, |(_, e)| e.total * 1000.0 / ticks as f64),
                    };
                    if got > *limit {
                        over.push(format!("{name} {got:.2} ms over its {limit} ms"));
                    }
                }
                if over.is_empty() {
                    println!("guard: every section within budget");
                } else {
                    for o in &over {
                        println!("OVER BUDGET: {o}");
                    }
                    std::process::exit(2);
                }
            }
        }
        // [r6-perf] Run a saved city on for some days (a row a day) and
        // save it: big cities for `measure`. `ND_FAST_IMMIGRATION=1` lets
        // newcomers in every tick, `ND_TREASURY` keeps the city solvent.
        Some("run") => {
            let path = args.get(2).expect("save path");
            let days: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);
            let out = args.get(4).cloned();
            let mut w = World::read_save(path).expect("load");
            if std::env::var_os("ND_FAST_IMMIGRATION").is_some() {
                w.config.immigration_interval = 1;
                w.config.max_builds_per_interval = w.config.max_builds_per_interval.max(16);
            }
            let floor: Option<f64> = std::env::var("ND_TREASURY").ok().and_then(|s| s.parse().ok());
            let t0 = Instant::now();
            for day in 0..days {
                let t = Instant::now();
                for _ in 0..1440 {
                    w.tick(&mut minds);
                }
                if let Some(f) = floor {
                    w.economy.treasury = w.economy.treasury.max(f as _);
                }
                let _ = nd_sim::prof::take();
                println!("day {:>3}: pop {:>6} buildings {:>5} cars {:>5} walk nodes {:>6} | {:.1} s ({:.0} s in all)", day + 1, w.citizens.len(), w.buildings.len(), w.traffic.cars.len(), w.walk.nodes.len(), t.elapsed().as_secs_f64(), t0.elapsed().as_secs_f64());
                if let Some(out) = &out {
                    if (day + 1) % 5 == 0 || day + 1 == days {
                        let f = std::io::BufWriter::new(std::fs::File::create(out).expect("create"));
                        w.write_ron_gz(f).expect("save");
                    }
                }
            }
        }
        // [r6-perf] What drawing the people costs per frame: each world
        // query the renderer's `sync_citizens` makes, over everyone outdoors.
        Some("people") => {
            let path = args.get(2).expect("save path");
            let w = World::read_save(path).expect("load");
            let shown: Vec<_> = w.citizens.iter().filter(|(_, c)| c.is_visible()).map(|(id, _)| id).collect();
            println!("{} citizens shown of {}", shown.len(), w.citizens.len());
            let down = w.down_on_street();
            let time = |name: &str, f: &dyn Fn(nd_sim::CitizenId) -> f32| {
                let t = Instant::now();
                let mut acc = 0.0;
                for _ in 0..5 {
                    for &id in &shown {
                        acc += f(id);
                    }
                }
                println!("{name:<24} {:>8.2} ms a frame ({acc:.0})", t.elapsed().as_secs_f64() * 1000.0 / 5.0);
            };
            time("pose_with", &|id| w.pose_with(id, &down) as u8 as f32);
            time("citizen_in_tunnel", &|id| w.citizen_in_tunnel(&w.citizens[id]) as u8 as f32);
            time("walker_way_height", &|id| w.walker_way_height(&w.citizens[id], w.citizens[id].pos).unwrap_or(0.0));
            time("ground_height", &|id| w.ground_height(w.citizens[id].pos));
            time("is_water", &|id| w.landscape.is_water(w.citizens[id].pos) as u8 as f32);
            time("walker_level", &|id| w.walker_level(&w.citizens[id], w.citizens[id].pos));
            time("wheeling_bike", &|id| w.wheeling_bike(&w.citizens[id]) as u8 as f32);
            time("on_platform", &|id| w.citizens[id].on_platform() as u8 as f32);
            time("leg_is_plain", &|id| match &w.citizens[id].activity {
                nd_sim::Activity::Traveling { path, idx, .. } if *idx > 0 && *idx < path.len() => w.leg_is_plain(path[*idx - 1], path[*idx], 40.0) as u8 as f32,
                _ => 0.0,
            });
            let traveling = shown.iter().filter(|&&id| matches!(&w.citizens[id].activity, nd_sim::Activity::Traveling { path, idx, .. } if *idx > 0 && *idx < path.len())).count();
            println!("{traveling} of them on a leg");
        }
        _ => eprintln!("perf_probe grow <population> <out.ron> | measure <city.ron> <ticks> | run <city.ron> <days> [out.ron]"),
    }
}
