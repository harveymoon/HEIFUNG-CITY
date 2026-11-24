//! [r16-speed] What the sim thread's copy of the world costs on a saved
//! city, whole and field by field. Not part of the game.
//!
//! cargo run --release -p nd_sim --example clone_probe -- city.ron [rounds]
use nd_sim::World;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rounds: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(5);
    let t = Instant::now();
    let w = World::read_save(&args[1]).expect("load");
    println!("load {:.0} ms ({} people, {} buildings, tick {})", t.elapsed().as_secs_f64() * 1000.0, w.citizens.len(), w.buildings.len(), w.clock.tick);
    let mut whole = Vec::new();
    for _ in 0..rounds {
        let t = Instant::now();
        let c = nd_sim::versioned::exact_clone(|| w.clone());
        whole.push(t.elapsed().as_secs_f64() * 1000.0);
        drop(c);
    }
    whole.sort_by(f64::total_cmp);
    // [r16-speed] The sim thread's copy: the same clone on four threads.
    let mut par = Vec::new();
    for _ in 0..rounds {
        let t = Instant::now();
        let c = w.exact_copy();
        par.push(t.elapsed().as_secs_f64() * 1000.0);
        drop(c);
    }
    par.sort_by(f64::total_cmp);
    println!("parallel exact_copy ms: min {:.1} median {:.1} max {:.1}", par[0], par[par.len() / 2], par[par.len() - 1]);
    println!("whole clone ms: min {:.1} median {:.1} max {:.1}", whole[0], whole[whole.len() / 2], whole[whole.len() - 1]);
    let mut sum: std::collections::BTreeMap<&'static str, Vec<f64>> = Default::default();
    for _ in 0..rounds {
        for (n, ms) in w.clone_cost() {
            sum.entry(n).or_default().push(ms);
        }
    }
    let mut rows: Vec<(&str, f64)> = sum.into_iter().map(|(n, mut v)| { v.sort_by(f64::total_cmp); (n, v[v.len() / 2]) }).collect();
    rows.sort_by(|a, b| b.1.total_cmp(&a.1));
    let total: f64 = rows.iter().map(|r| r.1).sum();
    for (n, ms, k) in w.citizen_clone_cost() { println!("  citizen.{n:<18} {ms:>7.2} ms  {k} items"); }
    println!("fields summed (median each): {total:.1} ms");
    for (n, ms) in rows.iter().take(40) {
        println!("{n:<24} {ms:>8.2} ms");
    }
}
