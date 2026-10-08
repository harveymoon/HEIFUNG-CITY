//! Dump street-mesh triangles touching a box, from a save: what is drawn
//! there and at what height. Not part of the game.
//!
//! cargo run --profile fast -p nd_procgen --example geo_box_probe -- save.ron x0 y0 x1 y1
use glam::Vec2;
use nd_procgen::palette::Palette;
use nd_procgen::street::{generate_street, StreetInput, StreetLayer};
use nd_sim::World;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let w = World::from_ron(&std::fs::read_to_string(&a[1]).unwrap()).expect("load");
    let f = |i: usize| a[i].parse::<f32>().unwrap();
    let (lo, hi) = (Vec2::new(f(2), f(3)), Vec2::new(f(4), f(5)));
    let pal = Palette::default();
    let heights: &[f32] = if w.relief { &w.road_z } else { &[] };
    let inner: &[Vec<(f32, f32)>] = if w.relief { &w.road_inner } else { &[] };
    let g = generate_street(&StreetInput { roads: &w.roads, net: &w.traffic.net, land_value: &w.land_value, palette: &pal, seed: w.config.seed, heights, inner, layer: StreetLayer::Surface, closed: &[] });
    let water = nd_procgen::terrain::waterworks(&w, &pal);
    let (walls, caps) = nd_procgen::terrain::retaining_walls(&w, &pal);
    let ground = nd_procgen::terrain::build_landscape_terrain(&w, &pal, 4.0);
    let mut via = nd_procgen::Geometry::default();
    nd_procgen::terrain::viaducts(&w, &mut via, [0.5; 3], [0.5; 3]);
    let extra: Vec<(&str, &nd_procgen::MeshData)> = water.parts.iter().map(|m| ("waterworks", m)).chain(via.parts.iter().map(|m| ("viaducts", m))).chain([("walls", &walls), ("caps", &caps)]).collect();
    for (name, m) in extra {
        for t in m.indices.chunks(3) {
            let v: Vec<[f32; 3]> = t.iter().map(|&i| m.positions[i as usize]).collect();
            let inside = v.iter().any(|p| p[0] >= lo.x && p[0] <= hi.x && p[2] >= lo.y && p[2] <= hi.y);
            if inside {
                println!("{name} {:?}", v.iter().map(|p| (p[0].round(), (p[1] * 10.0).round() / 10.0, p[2].round())).collect::<Vec<_>>());
            }
        }
    }
    let mut n = 0;
    for t in ground.indices.chunks(3) {
        let v: Vec<[f32; 3]> = t.iter().map(|&i| ground.positions[i as usize]).collect();
        if v.iter().any(|p| p[0] >= lo.x && p[0] <= hi.x && p[2] >= lo.y && p[2] <= hi.y) && n < 40 {
            n += 1;
            println!("ground {:?}", v.iter().map(|p| (p[0].round(), (p[1] * 10.0).round() / 10.0, p[2].round())).collect::<Vec<_>>());
        }
    }
    for (si, m) in g.parts.iter().enumerate() {
        for t in m.indices.chunks(3) {
            let v: Vec<[f32; 3]> = t.iter().map(|&i| m.positions[i as usize]).collect();
            let inside = v.iter().any(|p| p[0] >= lo.x && p[0] <= hi.x && p[2] >= lo.y && p[2] <= hi.y);
            let span = v.iter().map(|p| Vec2::new(p[0], p[2])).fold((Vec2::splat(1e9), Vec2::splat(-1e9)), |(a, b), p| (a.min(p), b.max(p)));
            if inside && (span.1 - span.0).max_element() > 4.0 {
                println!("part {si} {:?} span {:.1}", v.iter().map(|p| (p[0].round(), (p[1] * 10.0).round() / 10.0, p[2].round())).collect::<Vec<_>>(), (span.1 - span.0).max_element());
            }
        }
    }
}
