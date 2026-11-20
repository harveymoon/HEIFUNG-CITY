//! [r10-worn-paths] Probe: load a save, run it on day by day and watch the
//! walking-heat field wear in: its totals, the share of cells past the
//! grass and paving thresholds, the hottest spots, the tick time, and what
//! the field adds to the save (plain and gzip-compressed, as the game
//! writes it). `out` saves the city at the end (for screenshots).
//!
//! cargo run --profile fast -p nd_sim --example foot_wear_probe -- saves/city.ron 4 [out.ron]

use nd_sim::foot_wear::{self, FootWear};
use nd_sim::World;
use std::io::Write;
use std::time::Instant;

fn gz_len(text: &str) -> usize {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    e.write_all(text.as_bytes()).unwrap();
    e.finish().unwrap().len()
}

fn report(w: &World) {
    let f = &w.foot_wear;
    let s = f.stats();
    let text = ron::to_string(f).unwrap();
    println!(
        "  field: {} chunks, {} cells walked, {} worn (>= {}), {} bare (>= {}), max {:.0}; save {} KB plain, {} KB gz",
        s.chunks,
        s.cells,
        s.worn,
        foot_wear::WEAR_START,
        s.bare,
        foot_wear::WEAR_FULL,
        s.max,
        text.len() / 1024,
        gz_len(&text) / 1024
    );
    // How the heat is spread: cells per decade.
    let mut hist = [0usize; 6];
    for (_, cells) in f.chunks() {
        for &v in cells {
            if v > 0.0 {
                hist[(v.log10().floor().max(0.0) as usize).min(5)] += 1;
            }
        }
    }
    println!("  cells by heat: <10 {} | 10-100 {} | 100-1k {} | 1k-10k {} | 10k+ {}", hist[0], hist[1], hist[2], hist[3], hist[4] + hist[5]);
    for (v, p) in f.hottest(6) {
        println!("    hot {:.0} at ({:.0},{:.0})", v, p.x, p.y);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let src = args.get(1).cloned().expect("save path");
    let days: u32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let out = args.get(3).cloned();
    let t = Instant::now();
    let mut w = World::read_save(&src).expect("load");
    println!("loaded {src} in {:.1} s: day {} pop {}, field empty: {}", t.elapsed().as_secs_f32(), w.clock.day(), w.citizens.len(), w.foot_wear.is_empty());
    let mut minds = nd_minds::RuleBasedMind::default();
    for d in 0..days {
        let t = Instant::now();
        let mut ticks = 0u32;
        let mut field_ns = 0u128;
        loop {
            w.tick(&mut minds);
            w.drain_events();
            ticks += 1;
            // The field's own cost: this tick's steps again, timed on a copy
            // of the field (the world's field already took them).
            if ticks % 60 == 30 {
                let mut probe: FootWear = w.foot_wear.clone();
                let t = Instant::now();
                for c in w.citizens.values() {
                    if c.inside.is_none() && c.mode == nd_sim::citizens::TravelMode::Walk && c.activity.is_traveling() {
                        probe.walk(c.prev_pos, c.pos, 1.0);
                    }
                }
                field_ns += t.elapsed().as_nanos() * 60;
            }
            if w.clock.minute_of_day() == 1439 {
                break;
            }
        }
        let secs = t.elapsed().as_secs_f64();
        println!("day {} done: {} ticks, {:.1} ms/tick, field ~{:.3} ms/tick", d + 1, ticks, secs * 1000.0 / ticks as f64, field_ns as f64 / 1e6 / ticks as f64);
        report(&w);
    }
    if let Some(out) = out {
        w.write_ron_gz(std::io::BufWriter::new(std::fs::File::create(&out).unwrap())).unwrap();
        println!("saved {out}: {} MB", std::fs::metadata(&out).unwrap().len() as f64 / 1e6);
    }
}
