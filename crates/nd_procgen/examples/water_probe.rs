//! [r15-water] Is the water the sim knows drawn as water? From a save: every
//! point the sim calls water (`Landscape::is_water`, sampled on a grid) whose
//! drawn ground (the 8 m terrain mesh as the renderer draws it, or a lot's
//! floor) stands over the water sheet, and why; then each river's drawn
//! width against its sim width along its course. Not part of the game.
//!
//! `cargo run --profile fast -p nd_procgen --example water_probe -- <save.ron> [step] [worst]`
//!
//! Causes of a covered point (first that fits):
//! - `lot`: inside a lot's outline (its floor is drawn at its pad);
//! - `natural`: the natural raster's bed is itself over the waterline there
//!   (10 m raster cells interpolated across the shore);
//! - `graded`: the built ground (cut and fill) is over it, the natural bed not;
//! - `mesh`: the built ground is under it but the 8 m mesh drawn over it.
use glam::Vec2;
use nd_procgen::lot_ground::TerrainProbe;
use nd_sim::landscape::WATER_LEVEL;
use nd_sim::World;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("save path");
    let step: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(2.0);
    let worst_n: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(15);
    if std::env::var("ND_DAY").is_ok() {
        // A day run from the save, with the water kept open and as before
        // r15-water: population and treasury.
        for keep in [false, true] {
            nd_sim::landscape::KEEP_WATER_OPEN.store(keep, std::sync::atomic::Ordering::Relaxed);
            let mut w = World::read_save(path).expect("load");
            let mut minds = nd_minds::RuleBasedMind::default();
            let day = w.clock.day();
            while w.clock.day() == day {
                w.tick(&mut minds);
            }
            for _ in 0..1440 {
                w.tick(&mut minds);
            }
            println!("keep_water_open {keep}: day {} {:02}:00 pop {} treasury {:.0} buildings {}", w.clock.day(), w.clock.hour(), w.citizens.len(), w.economy.treasury, w.buildings.len());
        }
        return;
    }
    let t0 = std::time::Instant::now();
    let w = World::read_save(path).expect("load");
    eprintln!("loaded in {:.1}s", t0.elapsed().as_secs_f32());
    let land = &w.landscape;
    let probe = TerrainProbe::new(&w);
    if let Ok(spot) = std::env::var("ND_SPOT") {
        // ND_SPOT="x,y,r,step": the ground round a point, row by row:
        // natural/built/drawn heights, `~` where the sim says water.
        let v: Vec<f32> = spot.split(',').map(|x| x.parse().unwrap()).collect();
        let (c, r, st) = (Vec2::new(v[0], v[1]), v[2], v[3]);
        let mut y = c.y - r;
        while y <= c.y + r {
            let mut row = format!("{y:>6.0} ");
            let mut x = c.x - r;
            while x <= c.x + r {
                let p = Vec2::new(x, y);
                let wet = if land.is_water(p) { '~' } else { ' ' };
                row.push_str(&format!("{wet}{:>5.1}/{:>4.1}/{:>4.1}", land.height(p), w.ground_height(p), probe.height(p)));
                x += st;
            }
            println!("{row}");
            y += st;
        }
        let rr = Vec2::splat(r + 30.0);
        for sid in w.roads.segments_in_box(c - rr, c + rr) {
            let s = w.roads.segment(sid);
            if !s.alive {
                continue;
            }
            let (a, b) = w.roads.segment_points(sid);
            let d = nd_sim::roads::project_on_segment(c, a, b).1.distance(c);
            println!("seg {} {:?} bridge {} viaduct {} ({:.0},{:.0})->({:.0},{:.0}) z {:.1}/{:.1} dist {:.1} corridor hw {:.1}", sid.0, s.kind, s.bridge, s.viaduct, a.x, a.y, b.x, b.y, w.road_height(sid, 0.0), w.road_height(sid, 1.0), d, s.corridor_half_width());
        }
        return;
    }
    // ND_OLD=1: the ground mesh as drawn before r15-water (no shore cut).
    let old = std::env::var("ND_OLD").is_ok();
    let tc = std::time::Instant::now();
    let fresh = TerrainProbe::new(&w);
    let holes = nd_procgen::water_edge::water_holes_with(&w, &fresh);
    eprintln!("water_holes: {} in {:.0} ms", holes.len(), tc.elapsed().as_secs_f32() * 1000.0);
    let tc = std::time::Instant::now();
    let lines = nd_procgen::water_edge::shore_lines(land);
    eprintln!("shore_lines: {} pieces in {:.0} ms", lines.iter().map(|l| l.len()).sum::<usize>(), tc.elapsed().as_secs_f32() * 1000.0);
    let drawn_ground = nd_procgen::water_edge::DrawnGround::new(&w, &probe);
    let drawn_at = |p: Vec2| if old { probe.height(p) } else { drawn_ground.height(p) };
    if !old {
        println!("shore pieces cut: {}", drawn_ground.cut_len());
    }
    let lots: Vec<(Vec2, Vec2, &Vec<Vec2>, nd_sim::LotId)> = w
        .lots
        .iter()
        .filter(|(id, l)| l.polygon.len() >= 3 && w.ground.pads.contains_key(id) && !(l.building.is_none() && l.park.is_some()))
        .map(|(id, l)| {
            let (lo, hi) = nd_sim::geom::bounds(&l.polygon);
            (lo, hi, &l.polygon, id)
        })
        .collect();
    let lot_at = |p: Vec2| lots.iter().find(|(lo, hi, poly, _)| p.x >= lo.x && p.y >= lo.y && p.x <= hi.x && p.y <= hi.y && nd_sim::geom::point_in_polygon(p, poly)).map(|l| l.3);
    let half = land.land_half();
    let n = (half * 2.0 / step) as i32;
    let (mut wet, mut covered) = (0usize, 0usize);
    let mut by: std::collections::BTreeMap<&str, usize> = Default::default();
    // Covered points by 40 m block, for the worst places.
    let mut blocks: std::collections::HashMap<(i32, i32), (usize, f32)> = Default::default();
    let mut lot_hits: std::collections::HashMap<nd_sim::LotId, usize> = Default::default();
    let mut depth_sum = 0.0f32;
    for j in 0..n {
        for i in 0..n {
            let p = Vec2::new(-half + (i as f32 + 0.5) * step, -half + (j as f32 + 0.5) * step);
            if !land.is_water(p) {
                continue;
            }
            // A bridge or deck over it is not ground.
            wet += 1;
            let lot = lot_at(p);
            let drawn = drawn_at(p);
            let cause = if let Some(id) = lot {
                let pad = w.ground.pads.get(&id).copied().unwrap_or(0.0);
                if pad <= WATER_LEVEL && drawn <= WATER_LEVEL {
                    continue;
                }
                *lot_hits.entry(id).or_default() += 1;
                "lot"
            } else if drawn <= WATER_LEVEL {
                continue;
            } else if land.height(p) > WATER_LEVEL {
                "natural"
            } else if w.ground_height(p) > WATER_LEVEL {
                "graded"
            } else if probe.height(p) > WATER_LEVEL {
                "mesh"
            } else {
                "edge"
            };
            covered += 1;
            depth_sum += drawn.max(0.0);
            *by.entry(cause).or_default() += 1;
            let b = blocks.entry(((p.x / 40.0).floor() as i32, (p.y / 40.0).floor() as i32)).or_default();
            b.0 += 1;
            b.1 = b.1.max(drawn);
        }
    }
    let area = step * step;
    println!("water samples (step {step} m): {wet} ({:.1} ha)", wet as f32 * area / 1e4);
    println!("covered by drawn ground: {covered} ({:.2} ha, {:.1}%), mean height over the sheet {:.2} m", covered as f32 * area / 1e4, 100.0 * covered as f32 / wet.max(1) as f32, depth_sum / covered.max(1) as f32);
    for (k, v) in &by {
        println!("  {k:8} {v:7} ({:.2} ha)", *v as f32 * area / 1e4);
    }
    let mut lh: Vec<_> = lot_hits.into_iter().collect();
    lh.sort_by(|a, b| b.1.cmp(&a.1));
    println!("lots over sim water: {}", lh.len());
    for (id, c) in lh.iter().take(worst_n) {
        let l = &w.lots[*id];
        let name = l.building.and_then(|b| w.buildings.get(b)).map(|b| b.name.clone()).unwrap_or_else(|| "empty".into());
        println!("  lot {:?} at ({:.0},{:.0}) {:?} {} pad {:.2}: {:.0} m2 over water", id, l.center.x, l.center.y, l.land_use, name, w.ground.pads.get(id).copied().unwrap_or(0.0), *c as f32 * area);
    }
    let mut bl: Vec<_> = blocks.into_iter().collect();
    bl.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    println!("worst 40 m blocks (covered m2, highest drawn ground):");
    for ((bx, by), (c, hmax)) in bl.iter().take(worst_n) {
        println!("  ({:.0},{:.0}): {:.0} m2, up to {:.2} m", *bx as f32 * 40.0 + 20.0, *by as f32 * 40.0 + 20.0, *c as f32 * area, hmax);
    }
    // River widths: cross-sections every 10 m.
    println!("river widths (drawn wet span across the sim's, cross-sections every 10 m):");
    let mut spots: Vec<(f32, f32, f32, Vec2, usize)> = Vec::new();
    let (mut sections, mut thin) = (0usize, 0usize);
    let mut ratio_sum = 0.0f32;
    for (ri, r) in land.rivers.iter().enumerate() {
        for wv in r.points.windows(2) {
            let (a, b) = (wv[0], wv[1]);
            let len = a.distance(b);
            let dir = (b - a).normalize_or_zero();
            let side = Vec2::new(-dir.y, dir.x);
            let mut t = 5.0;
            while t < len {
                let c = a + dir * t;
                t += 10.0;
                if c.x.abs() > half - 5.0 || c.y.abs() > half - 5.0 || !land.is_water(c) {
                    continue;
                }
                let hw = r.width * 0.5;
                let (mut sim, mut drawn) = (0usize, 0usize);
                let mut s = -hw - 2.0;
                while s <= hw + 2.0 {
                    let p = c + side * s;
                    s += 0.5;
                    if !land.is_water(p) {
                        continue;
                    }
                    sim += 1;
                    if drawn_at(p) <= WATER_LEVEL && lot_at(p).is_none_or(|id| w.ground.pads.get(&id).copied().unwrap_or(0.0) <= WATER_LEVEL) {
                        drawn += 1;
                    }
                }
                if sim == 0 {
                    continue;
                }
                sections += 1;
                let ratio = drawn as f32 / sim as f32;
                ratio_sum += ratio;
                if ratio < 0.8 {
                    thin += 1;
                }
                spots.push((ratio, sim as f32 * 0.5, drawn as f32 * 0.5, c, ri));
            }
        }
    }
    println!("  sections {sections}, mean drawn/sim {:.3}, under 80%: {thin}", ratio_sum / sections.max(1) as f32);
    spots.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (ratio, sim, drawn, c, ri) in spots.iter().take(worst_n) {
        println!("  river {ri} at ({:.0},{:.0}): sim {sim:.1} m, drawn {drawn:.1} m ({:.0}%)", c.x, c.y, ratio * 100.0);
    }
}
