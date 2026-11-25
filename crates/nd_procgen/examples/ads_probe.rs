//! [r17-logos] What the ads and logos cost on a save, and where the screens
//! are: every building generated as the game draws it (its screens' ads
//! from the world, its shops' marks), counting the ad frames' triangles,
//! the logos' and the shop signs', and listing the buildings with screens
//! (for cameras).
//!
//! cargo run --profile fast -p nd_procgen --example ads_probe -- saves/city.ron [near_x near_y]

use nd_procgen::ads::{ad_code, ads_for, shop_marks, AdSource};
use nd_procgen::buildings::{generate_building, style_extras, BuildingRules, Extras};
use nd_procgen::surface::Surface;
use nd_procgen::Palette;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let path = args.get(1).expect("a save");
    let near = match (args.get(2).and_then(|a| a.parse::<f32>().ok()), args.get(3).and_then(|a| a.parse::<f32>().ok())) {
        (Some(x), Some(y)) => Some(glam::Vec2::new(x, y)),
        _ => None,
    };
    let t = Instant::now();
    let w = nd_sim::World::read_save(path).expect("load");
    println!("loaded {path} in {:.1} s: day {}, {} buildings", t.elapsed().as_secs_f32(), w.clock.day(), w.buildings.len());
    let (rules, pal) = (BuildingRules::default(), Palette::default());
    let (mut all_tris, mut glow_tris, mut ad_tris, mut ad_screens, mut base_ad_frames) = (0usize, 0usize, 0usize, 0usize, 0usize);
    let (mut shop_tris, mut near_detail_glow) = (0usize, 0usize);
    let mut gen_ms = 0.0;
    let mut screens: Vec<(f32, String, glam::Vec2, glam::Vec3, usize, usize)> = Vec::new();
    let mut shops: Vec<(f32, String, glam::Vec2)> = Vec::new();
    for (id, b) in w.buildings.iter() {
        if b.is_tenant() {
            continue;
        }
        let Some(lot) = w.footprint(id) else { continue };
        let src = |salt: u64| ads_for(&w, id, salt);
        let marks = shop_marks(&w, id);
        let ex = Extras { dynamic_lights: true, ads: Some(AdSource(&src)), shop_marks: &marks, ..style_extras(&w, id, &[]) };
        let tg = Instant::now();
        let g = generate_building(&lot, b.land_use, b.floors, b.tier, b.condition, b.seed, &rules, &pal, &ex);
        gen_ms += tg.elapsed().as_secs_f64() * 1000.0;
        all_tris += g.triangle_count();
        let glow = g.get(Surface::Glow);
        glow_tris += glow.indices.len() / 3;
        // Ad triangles: those whose first vertex carries an ad mark.
        let mut n_ad = 0;
        let mut frames = std::collections::HashSet::new();
        let mut sum = glam::Vec3::ZERO;
        let mut cnt = 0.0;
        let mut normal = glam::Vec3::ZERO;
        for tri in glow.indices.chunks(3) {
            if let Some(code) = ad_code(glow.colors[tri[0] as usize][3]) {
                n_ad += 1;
                frames.insert(code);
                sum += glam::Vec3::from(glow.positions[tri[0] as usize]);
                normal = glam::Vec3::from(glow.normals[tri[0] as usize]);
                cnt += 1.0;
            }
        }
        if n_ad > 0 {
            ad_tris += n_ad;
            base_ad_frames += frames.len();
            // Screens: frame 0 codes.
            let n_screens = frames.iter().filter(|c| *c & 3 == 0).count();
            ad_screens += n_screens;
            let c = w.lots[b.lot].center;
            let d = near.map_or(0.0, |n| n.distance(c));
            let style = nd_procgen::buildings::building_style(&w, id, &rules, None).map_or("?", |s| s.label());
            let at = sum / cnt;
            // An orbit camera 26 m out in front of it, at its height.
            let yaw = normal.x.atan2(normal.z);
            let pitch = (at.y / 26.0).clamp(0.1, 0.9).asin();
            screens.push((d, format!("{} {:?} ({style}, {} floors) cam look_at [{:.0},{:.0}] dist 26 pitch {pitch:.2} yaw {yaw:.2}", b.name, id, b.floors, at.x, at.z), c, at, n_screens, n_ad));
        }
        // Shop signs: the glow with the marks against without.
        if marks.iter().any(|m| m.is_some()) {
            let plain = generate_building(&lot, b.land_use, b.floors, b.tier, b.condition, b.seed, &rules, &pal, &Extras { shop_marks: &[], ..ex });
            let extra = glow.indices.len() / 3 - plain.get(Surface::Glow).indices.len() / 3;
            near_detail_glow += extra;
            let n = marks.iter().flatten().count();
            shop_tris += n;
            if let Some(nr) = near {
                let c = w.lots[b.lot].center;
                if nr.distance(c) < 400.0 {
                    let l = &w.lots[b.lot];
                    let f = (l.front_edge[0] + l.front_edge[1]) * 0.5;
                    let yaw = l.front_dir.x.atan2(l.front_dir.y);
                    shops.push((nr.distance(c), format!("{} {:?} {} marked shops: {:?} cam look_at [{:.0},{:.0}] yaw {yaw:.2}", b.name, id, n, marks.iter().flatten().map(|m| m.1.as_str()).collect::<Vec<_>>(), f.x, f.y), c));
                }
            }
        }
    }
    println!("generated every building in {:.0} ms", gen_ms);
    println!("triangles: all {all_tris}, glow {glow_tris}; ad frames {ad_tris} over {ad_screens} screens ({base_ad_frames} frames, {:.0} a screen)", ad_tris as f32 / ad_screens.max(1) as f32);
    println!("shop signs with a mark: {shop_tris}; their logos and names {near_detail_glow} glow triangles ({:.0} a sign)", near_detail_glow as f32 / shop_tris.max(1) as f32);
    shops.sort_by(|a, b| a.0.total_cmp(&b.0));
    for s in shops.iter().take(25) {
        println!("  shop {:>5.0} m {} lot {:?}", s.0, s.1, s.2);
    }
    // Corp lorries and vans (for a livery shot).
    for (cid, car) in w.traffic.cars.iter() {
        if let nd_sim::vehicle::Livery::Corp(..) = car.model.livery {
            println!("  corp vehicle {cid:?} {:?} at {:?} heading {:.2}", car.model.body, car.pos, car.heading);
        }
    }
    screens.sort_by(|a, b| a.0.total_cmp(&b.0));
    for s in screens.iter().take(if near.is_some() { 40 } else { 0 }) {
        println!("  {:>6.0} m  {}  lot {:?}  screens at {:?}: {} screens, {} tris", s.0, s.1, s.2, s.3, s.4, s.5);
    }
}
