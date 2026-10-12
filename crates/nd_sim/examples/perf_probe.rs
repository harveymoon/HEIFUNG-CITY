//! Where a big city's ticks go. Grows the demo district's layout tiled
//! 2x2 (an 8x8 grid of blocks) to a target population, saves it, and
//! measures any saved city tick by tick with the `prof!` section timers.
//!
//! cargo run --release -p nd_sim --features profile --example perf_probe -- grow 4000 city.ron
//! cargo run --release -p nd_sim --features profile --example perf_probe -- measure city.ron 1440

use nd_minds::RuleBasedMind;
use nd_sim::roads::RoadKind;
use nd_sim::{Gesture, LandUse, PowerPlantKind, Vec2, World, WorldConfig};
use std::time::Instant;

fn big_city() -> World {
    let config = WorldConfig { max_builds_per_interval: 8, immigration_interval: 6, ..WorldConfig::default() };
    let mut world = World::new(config);
    world.drain_events();
    world.start_project();
    let span = 560.0;
    for i in -4..=4 {
        let c = i as f32 * 140.0;
        world.add_gesture(Gesture::Road { points: vec![Vec2::new(-span, c), Vec2::new(span, c)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
        world.add_gesture(Gesture::Road { points: vec![Vec2::new(c, -span), Vec2::new(c, span)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
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
    for gy in 0..8 {
        for gx in 0..8 {
            let lu = tile[gy % 4][gx % 4];
            // Mostly homes outside the demo's own industrial row.
            let lu = if lu == Industrial && gy >= 4 { Residential } else { lu };
            let (x, y) = (-490.0 + gx as f32 * 140.0, -490.0 + gy as f32 * 140.0);
            world.add_gesture(Gesture::Zone { polygon: cell(x, y), land_use: lu });
        }
    }
    world.implement();
    world.economy.treasury = 2_000_000.0;
    world.link_to_highway(Vec2::new(0.0, span));
    for _ in 0..3 {
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
            for day in 0..60 {
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
            }
            std::fs::write(&out, w.to_ron().expect("save")).expect("write");
            println!("saved {out}");
        }
        Some("measure") => {
            let path = args.get(2).expect("save path");
            let ticks: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1440);
            let mut w = World::read_save(path).expect("load");
            println!("pop {} buildings {} cars {} walk nodes {} tow jobs {}", w.citizens.len(), w.buildings.len(), w.traffic.cars.len(), w.walk.nodes.len(), w.enforcement.jobs.len());
            let _ = nd_sim::prof::take();
            let mut times: Vec<f64> = Vec::new();
            for _ in 0..ticks {
                let t = Instant::now();
                w.tick(&mut minds);
                let dt = t.elapsed().as_secs_f64() * 1000.0;
                // [saves] The ticks a player would feel as a hitch.
                if dt > 100.0 {
                    println!("slow tick {} (day {} {}): {dt:.0} ms", w.clock.tick, w.clock.day() + 1, w.clock.format_hhmm());
                }
                times.push(dt);
            }
            let total: f64 = times.iter().sum();
            let mut sorted = times.clone();
            sorted.sort_by(f64::total_cmp);
            let pct = |q: f64| sorted[((sorted.len() - 1) as f64 * q) as usize];
            println!("ticks {ticks}: mean {:.2} ms, p50 {:.2}, p99 {:.2}, max {:.2} ms; pop now {}", total / ticks as f64, pct(0.5), pct(0.99), pct(1.0), w.citizens.len());
            println!("parks found full {} | bench meetings {} | cyclists ran reds {}", w.seats.full_total, w.seats.met_total, w.peds.red_lights_run_total);
            let mut rows: Vec<_> = nd_sim::prof::take().into_iter().collect();
            rows.sort_by(|a, b| b.1.total.total_cmp(&a.1.total));
            println!("{:<26} {:>8} {:>10} {:>10} {:>9}", "section", "calls", "total ms", "ms/tick", "max ms");
            for (name, e) in rows.iter().take(40) {
                println!("{:<26} {:>8} {:>10.1} {:>10.3} {:>9.2}", name, e.calls, e.total * 1000.0, e.total * 1000.0 / ticks as f64, e.max * 1000.0);
            }
        }
        _ => eprintln!("perf_probe grow <population> <out.ron> | measure <city.ron> <ticks>"),
    }
}
