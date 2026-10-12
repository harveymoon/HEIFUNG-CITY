//! [stable-parcels] Parcels cut road by road (each road works recutting
//! only near itself) against the same roads cut in one go: how many usable
//! parcels and lots each gives, and how big. Not part of the game.
//!
//! `cargo run --profile fast -p nd_sim --example recut_quality`
use nd_sim::plan::Gesture;
use nd_sim::roads::RoadKind;
use nd_sim::{LandUse, Vec2, World, WorldConfig};

fn stats(label: &str, w: &World) {
    let p = &w.parcels.parcels;
    let usable: Vec<f32> = p.iter().filter(|q| !q.landlocked()).map(|q| q.area).collect();
    let back = p.iter().filter(|q| q.landlocked()).count();
    let tiny = usable.iter().filter(|a| **a < 150.0).count();
    let big = p.iter().filter(|q| q.area > 2400.0).count();
    let lots: Vec<f32> = w.lots.values().filter(|l| l.usable()).map(|l| l.area()).collect();
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32;
    println!(
        "{label:<28} parcels {:>4} usable {:>4} (mean {:>5.0} m², <150 m²: {:>3}) backland {:>3} over 2400 m²: {:>2} | usable lots {:>4} (mean {:>5.0})",
        p.len(),
        usable.len(),
        mean(&usable),
        tiny,
        back,
        big,
        lots.len(),
        mean(&lots)
    );
}

fn town(roads: &[(Vec2, Vec2)], one_go: bool, zone: &[Vec2]) -> World {
    let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: nd_sim::landscape::LandscapeParams::flat(), ..WorldConfig::default() });
    if one_go {
        for &(a, b) in roads {
            w.add_gesture(Gesture::Road { points: vec![a, b], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
        }
        w.implement();
    } else {
        for &(a, b) in roads {
            w.add_road(&[a, b], 1, 1);
        }
    }
    w.add_zone(zone.to_vec(), LandUse::Residential);
    w
}

fn main() {
    let zone = vec![Vec2::new(-700.0, -700.0), Vec2::new(700.0, -700.0), Vec2::new(700.0, 700.0), Vec2::new(-700.0, 700.0)];
    // A grid, laid row by row.
    let mut grid = Vec::new();
    for i in -3..=3 {
        let c = i as f32 * 140.0;
        grid.push((Vec2::new(-420.0, c), Vec2::new(420.0, c)));
    }
    for i in -3..=3 {
        let c = i as f32 * 140.0;
        grid.push((Vec2::new(c, -420.0), Vec2::new(c, 420.0)));
    }
    // A long avenue with side streets added one at a time, and a diagonal.
    let mut avenue = vec![(Vec2::new(-600.0, 0.0), Vec2::new(600.0, 0.0))];
    for k in -5..=5 {
        let x = k as f32 * 110.0 + 13.0;
        avenue.push((Vec2::new(x, 0.0), Vec2::new(x + 20.0, if k % 2 == 0 { 160.0 } else { -160.0 })));
    }
    avenue.push((Vec2::new(-500.0, -300.0), Vec2::new(400.0, 350.0)));
    for (name, roads) in [("grid", &grid), ("avenue + side streets", &avenue)] {
        println!("== {name}");
        stats("  cut in one go", &town(roads, true, &zone));
        stats("  cut road by road", &town(roads, false, &zone));
    }
}
