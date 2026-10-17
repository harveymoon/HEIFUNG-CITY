//! [insight] What the city's memory costs: loads a save, plays it on for
//! a while, and reports how much the memory holds, what it adds to the
//! compressed save, and how long recording it takes per tick.
//!
//! cargo run --release -p nd_sim --example insight_probe -- city.ron 1440

use nd_minds::RuleBasedMind;
use nd_sim::insight::Ev;
use nd_sim::World;
use std::time::Instant;

fn gz_len(w: &World) -> usize {
    let mut out = Vec::new();
    w.write_ron_gz(&mut out).expect("save");
    out.len()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("insight_probe <save> [ticks]");
    let ticks: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1440);
    let t = Instant::now();
    let mut w = World::read_save(path).expect("load");
    println!("loaded in {:.1} s: pop {} buildings {} cars {}", t.elapsed().as_secs_f64(), w.citizens.len(), w.buildings.len(), w.traffic.cars.len());
    let mut minds = RuleBasedMind::default();
    let mut tick_ms = Vec::new();
    let mut step_ms = Vec::new();
    for k in 0..ticks {
        let t = Instant::now();
        w.tick(&mut minds);
        tick_ms.push(t.elapsed().as_secs_f64() * 1000.0);
        // The recording on its own, run once more on the same state: it
        // takes in nothing new the second time, so this is the look itself
        // (the diff over every car, citizen and building), not its records.
        if k % 60 == 59 {
            let mut probe = w.insight.clone();
            std::mem::swap(&mut probe, &mut w.insight);
            let t = Instant::now();
            w.step_insight();
            step_ms.push(t.elapsed().as_secs_f64() * 1000.0);
            std::mem::swap(&mut probe, &mut w.insight);
        }
        if (k + 1) % 360 == 0 {
            println!("  {} ticks: memory {} KB", k + 1, w.insight.bytes() / 1024);
        }
    }
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
    println!("ticks {ticks}: mean tick {:.2} ms; the memory's look {:.3} ms (mean of {} samples, max {:.3})", mean(&tick_ms), mean(&step_ms), step_ms.len(), step_ms.iter().cloned().fold(0.0, f64::max));
    let ins = &w.insight;
    let recs = |f: &dyn Fn(&Ev) -> bool| -> usize { ins.live.values().chain(ins.done.iter()).flat_map(|t| t.log.records()).filter(|r| f(&r.ev)).count() };
    println!(
        "live trips {} ({} KB), done {} | people {} ({} KB trips+life, {} last drives) | buildings {} ({} KB)",
        ins.live.len(),
        ins.live.values().map(|t| t.log.size()).sum::<usize>() / 1024,
        ins.done.len(),
        ins.people.len(),
        ins.people.values().map(|p| p.trips.size() + p.life.size()).sum::<usize>() / 1024,
        ins.people.values().filter(|p| p.last_drive.is_some()).count(),
        ins.buildings.len(),
        ins.buildings.values().map(|p| p.log.size()).sum::<usize>() / 1024,
    );
    println!(
        "vehicle records: streets {} replans {} u-turns {} stalls {} anger {} errands {}",
        recs(&|e| matches!(e, Ev::Enter { .. })),
        recs(&|e| matches!(e, Ev::Replan { .. })),
        recs(&|e| matches!(e, Ev::UTurn { .. } | Ev::RageUTurn)),
        recs(&|e| matches!(e, Ev::Stall { .. })),
        recs(&|e| matches!(e, Ev::Anger { .. })),
        recs(&|e| matches!(e, Ev::Errand { .. })),
    );
    let encoded = ins.encode().len();
    let t = Instant::now();
    let with = gz_len(&w);
    let save_s = t.elapsed().as_secs_f64();
    let mut bare = w.clone();
    bare.insight = Default::default();
    let without = gz_len(&bare);
    println!(
        "memory {} KB raw ({} KB encoded); compressed save {:.2} MB with it, {:.2} MB without: +{} KB ({:.1}%); save took {:.1} s",
        ins.bytes() / 1024,
        encoded / 1024,
        with as f64 / 1e6,
        without as f64 / 1e6,
        (with as i64 - without as i64) / 1024,
        (with as f64 / without as f64 - 1.0) * 100.0,
        save_s
    );
}
