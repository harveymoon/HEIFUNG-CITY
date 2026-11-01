//! [r5-hillside] Lots stepped onto two pads. Not part of the game.
//!
//! `cargo run --profile fast -p nd_sim --example step_probe -- save <save.ron>`:
//! the stepped lots of a save, their step and building.
//! `... -- fresh [seed]`: on a new Islands map at relief 1 (as `new_game
//! {preset: islands, seed, relief: 1}`), streets laid across steep
//! hillsides and zoned for homes: the sites with stepped lots.
use nd_sim::{LandUse, Vec2, World};

fn list(w: &World) -> usize {
    let mut n = 0;
    let mut at_street = 0;
    let mut ids: Vec<_> = w.ground.steps.keys().copied().collect();
    ids.sort();
    for id in ids {
        let Some(l) = w.lots.get(id) else { continue };
        let st = w.ground.steps[&id];
        let front = w.ground.pads[&id];
        let street = w.road_height(l.front_segment, l.front_t);
        n += 1;
        if (front - street).abs() < 0.3 {
            at_street += 1;
        }
        let b = l.building.and_then(|b| w.buildings.get(b)).map(|b| format!("{} {:?} {}f", b.name, b.land_use, b.floors));
        println!("  lot at ({:.0},{:.0}) front {:.1} street {:.1} back {:.1} ({:+.1}) toward ({:.2},{:.2}) {}", l.center.x, l.center.y, front, street, st.back, st.back - front, st.back_dir.x, st.back_dir.y, b.unwrap_or_else(|| "-".into()));
    }
    println!("{n} stepped lots, {at_street} with the front pad at street level; {} lots, {} walls", w.lots.len(), w.ground.retaining.len());
    n
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("save") => {
            let w = World::read_save(&args[2]).expect("load");
            list(&w);
        }
        _ => {
            let seed: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(7);
            let mut land = nd_sim::LandscapeParams::preset(nd_sim::landscape::LandscapePreset::Islands, seed);
            land.relief = 1.0;
            // Or a save's land, with what is built on it.
            let base = match args.get(3) {
                Some(path) => World::read_save(path).expect("load"),
                None => World::new(nd_sim::WorldConfig { landscape: land, ..Default::default() }),
            };
            let l = &base.landscape;
            let mut y = -1600.0;
            while y <= 1600.0 {
                let mut x = -1600.0;
                while x <= 1600.0 {
                    let p = Vec2::new(x, y);
                    x += 80.0;
                    if l.height(p) < 20.0 || l.water_distance(p) < 60.0 || base.lots.values().any(|o| o.center.distance(p) < 90.0) || !base.roads.segments_near(p, 90.0).is_empty() {
                        continue;
                    }
                    let e = 30.0;
                    let g = Vec2::new(l.height(p + Vec2::X * e) - l.height(p - Vec2::X * e), l.height(p + Vec2::Y * e) - l.height(p - Vec2::Y * e)) / (2.0 * e);
                    if !(0.25..=1.4).contains(&g.length()) {
                        continue;
                    }
                    let across = g.normalize().perp();
                    let mut w = base.clone();
                    w.economy.treasury = 1e9;
                    let pts = [p - across * 70.0, p + across * 70.0];
                    if w.road_refusal(&pts, &[], nd_sim::roads::RoadKind::Street, 1, 1).is_some() {
                        continue;
                    }
                    w.add_road(&pts, 1, 1);
                    let ids: Vec<usize> = w.parcels.in_circle(p, 60.0).into_iter().filter(|&i| w.parcels.parcels[i].street_frontage && w.parcel_refusal(i, Some(LandUse::Residential)).is_none()).collect();
                    w.paint_parcels(&ids, Some(LandUse::Residential), None);
                    w.implement();
                    let n = w.ground.steps.len() - base.ground.steps.len();
                    eprintln!("tried ({:.0},{:.0}) {:.0}% -> {n}", p.x, p.y, g.length() * 100.0);
                    if n > 0 {
                        println!("site ({:.0},{:.0}) slope {:.0}% road ({:.0},{:.0})-({:.0},{:.0}): {n} stepped", p.x, p.y, g.length() * 100.0, pts[0].x, pts[0].y, pts[1].x, pts[1].y);
                        if n >= 3 {
                            list(&w);
                        }
                    }
                }
                y += 80.0;
            }
        }
    }
}
