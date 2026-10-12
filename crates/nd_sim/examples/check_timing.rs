//! Time the first project check of a road drawn on a loaded save (the
//! trial rebuild), the second, and the implement. Not part of the game.
//!
//! `check_timing <save.ron> [x y dx dy]...` (several roads: one each, on a
//! fresh copy of the loaded city)
use nd_sim::plan::Gesture;
use nd_sim::roads::RoadKind;
use nd_sim::{Vec2, World};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let t = Instant::now();
    let loaded = World::from_ron(&std::fs::read_to_string(&args[1]).unwrap()).expect("load");
    println!("load {:.2}s", t.elapsed().as_secs_f32());
    let v: Vec<f32> = args[2..].iter().map(|s| s.parse().unwrap()).collect();
    for r in v.chunks(4) {
        let mut w = loaded.clone();
        let (a, b) = (Vec2::new(r[0], r[1]), Vec2::new(r[0] + r[2], r[1] + r[3]));
        w.add_gesture(Gesture::Road { points: vec![a, b], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
        let t = Instant::now();
        let c1 = w.active_project_check();
        let first = t.elapsed().as_secs_f32();
        let t = Instant::now();
        let _ = w.active_project_check();
        let second = t.elapsed().as_secs_f32();
        let t = Instant::now();
        let ok = w.try_implement().is_ok();
        let imp = t.elapsed().as_secs_f32();
        println!("road ({:.0},{:.0})+({:.0},{:.0}): first check {:.3}s second {:.3}s implement {:.3}s ok {ok} refusal {:?}", r[0], r[1], r[2], r[3], first, second, imp, c1.refusal.map(|s| s.chars().take(60).collect::<String>()));
    }
}
