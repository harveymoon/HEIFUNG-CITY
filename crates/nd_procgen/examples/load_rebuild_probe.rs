//! [r16-speed] What the renderer's first frame after a load costs in the
//! landscape: the ground tiles, the water sheet, the waterworks, on a saved
//! city (twice: the first pays the world's lazy caches). Not part of the game.
//!
//! cargo run --release -p nd_procgen --example load_rebuild_probe -- city.ron
use nd_sim::World;
use std::time::Instant;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn main() {
    let path = std::env::args().nth(1).expect("save path");
    let t = Instant::now();
    let w = World::read_save(&path).expect("load");
    println!("load {:.0} ms", ms(t));
    let pal = nd_procgen::palette::default_palette();
    for round in 0..2 {
        let t = Instant::now();
        let tiles = nd_procgen::terrain::LandscapeTiles::new(&w, pal, 8.0);
        let tiles_ms = ms(t);
        let tris: usize = (0..tiles.len()).map(|k| tiles.mesh(k).indices.len() / 3).sum();
        let mut h: u64 = 0xcbf29ce484222325;
        for k in 0..tiles.len() {
            let m = tiles.mesh(k);
            for v in m.positions.iter().chain(m.normals.iter()) {
                for f in v {
                    h = (h ^ f.to_bits() as u64).wrapping_mul(0x100000001b3);
                }
            }
            for c in &m.colors {
                for f in c {
                    h = (h ^ f.to_bits() as u64).wrapping_mul(0x100000001b3);
                }
            }
        }
        let t = Instant::now();
        let water = nd_procgen::terrain::build_water(&w, pal);
        let water_ms = ms(t);
        let t = Instant::now();
        let works = nd_procgen::terrain::waterworks(&w, pal);
        let works_ms = ms(t);
        println!("round {round}: tiles {tiles_ms:.0} ms ({} tiles, {tris} tris, hash {h:016x}) | water {water_ms:.0} ms ({} verts) | waterworks {works_ms:.0} ms ({} meshes)", tiles.len(), water.positions.len(), works.parts.len());
    }
}
