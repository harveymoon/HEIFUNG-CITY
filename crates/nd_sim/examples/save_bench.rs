//! [saves] What a save costs: load a save (plain or compressed), then time
//! the world's copy, the RON write, the compressed write and the reads back.
//! Not part of the game.
//!
//! cargo run --release -p nd_sim --example save_bench -- city.ron [out_dir]
use nd_sim::World;
use std::time::Instant;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = &args[1];
    let out = std::path::PathBuf::from(args.get(2).cloned().unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned()));
    let t = Instant::now();
    let w = World::read_save(path).expect("load");
    println!("load {path}: {:.0} ms ({} people, {} buildings, tick {})", ms(t), w.citizens.len(), w.buildings.len(), w.clock.tick);
    for _ in 0..2 {
        let t = Instant::now();
        let c = w.clone();
        println!("clone: {:.0} ms", ms(t));
        let t = Instant::now();
        drop(c);
        println!("drop of the clone: {:.0} ms", ms(t));
    }
    let plain = out.join("bench_plain.ron");
    let t = Instant::now();
    w.write_ron(std::io::BufWriter::with_capacity(1 << 20, std::fs::File::create(&plain).unwrap())).unwrap();
    println!("write RON: {:.0} ms, {:.1} MB", ms(t), std::fs::metadata(&plain).unwrap().len() as f64 / 1e6);
    let gz = out.join("bench_gz.ron");
    let t = Instant::now();
    w.write_ron_gz(std::io::BufWriter::with_capacity(1 << 20, std::fs::File::create(&gz).unwrap())).unwrap();
    println!("write RON gz: {:.0} ms, {:.1} MB", ms(t), std::fs::metadata(&gz).unwrap().len() as f64 / 1e6);
    let t = Instant::now();
    let text = std::fs::read_to_string(&plain).unwrap();
    println!("read plain text: {:.0} ms", ms(t));
    let t = Instant::now();
    let bytes = std::fs::read(&gz).unwrap();
    let text2 = nd_sim::world::save_text(&bytes).unwrap();
    println!("read gz text: {:.0} ms (same: {})", ms(t), text == text2);
    drop(text2);
    let t = Instant::now();
    let back = World::from_ron(&text).unwrap();
    println!("parse + after_load: {:.0} ms", ms(t));
    drop(back);
    let _ = std::fs::remove_file(&plain);
    let _ = std::fs::remove_file(&gz);
}
