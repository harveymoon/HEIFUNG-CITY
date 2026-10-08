//! [memory] Where a big generated city's memory goes, day by day: the live
//! heap (a counting allocator), the peak, the process working set, and the
//! heap size of every world field (`World::heap_probe`), plus the save size.
//!
//! cargo run --profile fast -p nd_procgen --example memory_probe -- grid_plaza 3 20 [landscape|-] [save.ron|-] [population cap]
//!
//! Prints fields that are big or grew since the first day.
//!
//! `memory_probe -- load save.ron` weighs loading a save instead.

use nd_minds::{HybridMind, MockBackend};
use nd_procgen::citygen::{self, CityPreset};
use nd_sim::{LandscapeParams, LandscapePreset, World, WorldConfig};
use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
static TOTAL: AtomicUsize = AtomicUsize::new(0);
static BIGGEST: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        note(l.size());
        unsafe { System.alloc(l) }
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        note(l.size());
        unsafe { System.alloc_zeroed(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        LIVE.fetch_sub(l.size(), Relaxed);
        note(new);
        unsafe { System.realloc(p, l, new) }
    }
}

fn note(n: usize) {
    let live = LIVE.fetch_add(n, Relaxed) + n;
    PEAK.fetch_max(live, Relaxed);
    TOTAL.fetch_add(n, Relaxed);
    BIGGEST.fetch_max(n, Relaxed);
}

#[global_allocator]
static A: Counting = Counting;

#[cfg(windows)]
fn working_set() -> usize {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_faults: u32,
        peak_ws: usize,
        ws: usize,
        a: usize,
        b: usize,
        c: usize,
        d: usize,
        pagefile: usize,
        peak_pagefile: usize,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(p: isize, c: *mut Counters, cb: u32) -> i32;
    }
    let mut c = Counters { cb: std::mem::size_of::<Counters>() as u32, ..Default::default() };
    unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    c.ws
}
#[cfg(not(windows))]
fn working_set() -> usize {
    0
}

/// Counts the bytes written to it.
struct Sink(usize);

impl std::io::Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0 += b.len();
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn mb(n: usize) -> f64 {
    n as f64 / 1_048_576.0
}

fn fields(w: &World) -> BTreeMap<&'static str, usize> {
    let mut out = BTreeMap::new();
    w.heap_probe(&mut |name, clone| {
        let before = TOTAL.load(Relaxed);
        clone();
        out.insert(name, TOTAL.load(Relaxed) - before);
    });
    out
}

/// `load <save.ron>`: what loading a save costs (the file as one string,
/// then the parse), its peak and biggest block.
fn load(path: &str) {
    let base = LIVE.load(Relaxed);
    PEAK.store(base, Relaxed);
    BIGGEST.store(0, Relaxed);
    let text = std::fs::read_to_string(path).expect("read");
    println!("read {:.1} MB: heap +{:.0} MB, biggest block {:.1} MB", mb(text.len()), mb(LIVE.load(Relaxed) - base), mb(BIGGEST.swap(0, Relaxed)));
    let w = World::from_ron(&text).expect("load");
    println!("parsed: heap +{:.0} MB, peak +{:.0} MB, biggest block {:.1} MB", mb(LIVE.load(Relaxed) - base), mb(PEAK.load(Relaxed) - base), mb(BIGGEST.swap(0, Relaxed)));
    drop(text);
    let world_total: usize = fields(&w).values().sum();
    println!("world {:.0} MB; heap after dropping the text +{:.0} MB; pop {}", mb(world_total), mb(LIVE.load(Relaxed) - base), w.citizens.len());
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).is_some_and(|a| a == "load") {
        return load(&args[2]);
    }
    let preset = CityPreset::parse(args.get(1).map_or("grid_plaza", |s| s)).expect("preset");
    let seed: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let days: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(15);
    let mut params = match args.get(4).filter(|s| *s != "-") {
        Some(s) => LandscapeParams::preset(LandscapePreset::from_label(s).expect("landscape"), seed),
        None => preset.landscape(seed),
    };
    let save_to = args.get(5).cloned().filter(|s| s != "-");
    // Optional: stop immigration at this population, to see what grows
    // when the city does not.
    let cap: Option<usize> = args.get(6).and_then(|s| s.parse().ok());
    // As the game's `generate_city`: a new game with its highway.
    params.highway = true;
    let mut w = World::new(WorldConfig { landscape: params, ..WorldConfig::default() });
    w.economy.treasury = 60_000.0;
    w.economy.ledger = Default::default();
    w.drain_events();
    let plan = citygen::generate(preset, seed, &w.landscape);
    let laid = citygen::lay_city(&mut w, &plan);
    println!("{preset:?} seed {seed}: {} lots, heap {:.0} MB", laid.lots, mb(LIVE.load(Relaxed)));
    let mut minds = HybridMind::new(Box::new(MockBackend::default()));
    let t0 = std::time::Instant::now();
    let first = fields(&w);
    let mut prev = first.clone();
    for _ in 0..days {
        let d = w.clock.day();
        PEAK.store(LIVE.load(Relaxed), Relaxed);
        while w.clock.day() == d {
            w.tick(&mut minds);
            if cap.is_some_and(|c| w.citizens.len() >= c) {
                w.config.immigration_interval = u64::MAX / 4;
            }
            // The app drains the events every frame.
            w.drain_events();
        }
        let peak = PEAK.load(Relaxed);
        let biggest_tick = BIGGEST.swap(0, Relaxed);
        let now = fields(&w);
        let world_total: usize = now.values().sum();
        // The save streamed (as the game now writes it) into a counting sink,
        // then the old way, as one string, for its biggest block.
        let mut sink = Sink(0);
        w.write_ron(&mut sink).expect("save");
        let streamed_biggest = BIGGEST.swap(0, Relaxed);
        let save_len = w.to_ron().map(|s| s.len()).unwrap_or(0);
        let string_biggest = BIGGEST.swap(0, Relaxed);
        assert_eq!(sink.0, save_len, "the streamed save is the string save");
        println!(
            "day {:>2}: pop {:>5} bldg {:>4} cars {:>4} | heap {:>6.0} MB (peak in day {:>6.0}) ws {:>6.0} MB | world {:>6.1} MB save {:>6.1} MB | biggest block: ticks {:.1} MB, save as string {:.1} MB, streamed {:.1} MB | {:.0}s",
            w.clock.day(),
            w.citizens.len(),
            w.buildings.len(),
            w.traffic.cars.len(),
            mb(LIVE.load(Relaxed)),
            mb(peak),
            mb(working_set()),
            mb(world_total),
            mb(save_len),
            mb(biggest_tick),
            mb(string_biggest),
            mb(streamed_biggest),
            t0.elapsed().as_secs_f32()
        );
        let mut rows: Vec<_> = now.iter().map(|(k, &v)| (*k, v, v as i64 - prev[k] as i64, v as i64 - first[k] as i64)).collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.1));
        for (k, v, dd, d0) in rows.iter().take(14) {
            println!("    {k:<18} {:>8.2} MB  day {:>+8.3} MB  since start {:>+8.2} MB", mb(*v), *dd as f64 / 1_048_576.0, *d0 as f64 / 1_048_576.0);
        }
        prev = now;
        // Per citizen, the parts that are lists.
        let weigh = |f: &dyn Fn(&nd_sim::Citizen)| {
            let before = TOTAL.load(Relaxed);
            for c in w.citizens.values() {
                f(c);
            }
            (TOTAL.load(Relaxed) - before) as f64 / w.citizens.len().max(1) as f64
        };
        let memories = weigh(&|c| drop(std::hint::black_box(c.memories.clone())));
        let places = weigh(&|c| drop(std::hint::black_box(c.places.clone())));
        let trail = weigh(&|c| drop(std::hint::black_box(c.trail.clone())));
        let footfall = weigh(&|c| drop(std::hint::black_box(c.pending_footfall.clone())));
        let text = weigh(&|c| drop(std::hint::black_box((c.name.clone(), c.thought.clone()))));
        let whole = weigh(&|c| drop(std::hint::black_box(c.clone())));
        println!("    per citizen: {whole:.0} B heap: memories {memories:.0}, places {places:.0}, trail {trail:.0}, footfall {footfall:.0}, name+thought {text:.0}; save {:.0} B", save_len as f64 / w.citizens.len().max(1) as f64);
    }
    if let Some(p) = save_to {
        std::fs::write(&p, w.to_ron().expect("save")).expect("write");
    }
}
