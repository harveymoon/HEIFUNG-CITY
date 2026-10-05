//! The ground: a flat basin for the city with procedural hills around it, and
//! a scatter pass that puts rocks, scrub and dead trees on the open ground.

use crate::buildings::parts::{dry_tree_world, mix, mul, ngon, rgba, rock_world, scrub_world};
use crate::mesh::MeshData;
use crate::palette::Palette;
use crate::surface::{Geometry, Surface};
use glam::Vec2;
use nd_sim::rng_for;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rand::Rng;

#[derive(Clone, Debug)]
pub struct TerrainConfig {
    pub seed: u32,
    /// Half-size of the terrain mesh in metres.
    pub half_size: f32,
    pub cell: f32,
    /// Radius of the flat basin the city sits in.
    pub basin_radius: f32,
    pub hill_height: f32,
}

impl Default for TerrainConfig {
    fn default() -> Self {
        Self { seed: 7, half_size: 1800.0, cell: 12.0, basin_radius: 1000.0, hill_height: 160.0 }
    }
}

pub fn terrain_height(cfg: &TerrainConfig, noise: &Fbm<Perlin>, x: f32, z: f32) -> f32 {
    let r = (x * x + z * z).sqrt();
    if r <= cfg.basin_radius {
        return 0.0;
    }
    let t = ((r - cfg.basin_radius) / (cfg.half_size - cfg.basin_radius)).clamp(0.0, 1.0);
    let ramp = t * t * (3.0 - 2.0 * t);
    let n = noise.get([x as f64 * 0.0025, z as f64 * 0.0025]) as f32 * 0.5 + 0.5;
    ramp * cfg.hill_height * (0.4 + 0.6 * n)
}

/// The noise the hills are built from, so other passes can sample the same height.
fn height_noise(cfg: &TerrainConfig) -> Fbm<Perlin> {
    Fbm::<Perlin>::new(cfg.seed).set_octaves(4)
}

/// Rocks, scrub, tufts of dead grass and the odd dry tree over the open
/// ground outside the city. A jittered grid is thinned by low-frequency
/// noise so the growth clusters in some washes and leaves other ground bare;
/// `clear(p)`, supplied by the caller, rejects anything near a road or on
/// zoned land. Capped at a few thousand small pieces.
pub fn scatter_nature(cfg: &TerrainConfig, palette: &Palette, seed: u64, clear: &dyn Fn(Vec2) -> bool) -> Geometry {
    const STEP: f32 = 18.0;
    const CAP: usize = 3500;
    let mut g = Geometry::default();
    let heights = height_noise(cfg);
    let density = Fbm::<Perlin>::new(cfg.seed.wrapping_add(7)).set_octaves(2);
    let mut rng = rng_for(seed, 41);
    let n = ((cfg.half_size * 2.0 - 40.0) / STEP) as i32;
    let dry = palette.foliage_dry;
    let rock = mix(palette.stone, palette.hills, 0.3);
    let mut count = 0;
    for j in 0..n {
        for i in 0..n {
            let x = -cfg.half_size + 20.0 + (i as f32 + 0.5 + rng.random_range(-0.45..0.45)) * STEP;
            let z = -cfg.half_size + 20.0 + (j as f32 + 0.5 + rng.random_range(-0.45..0.45)) * STEP;
            let d = density.get([x as f64 * 0.004, z as f64 * 0.004]) as f32 * 0.5 + 0.5;
            let chance = ((d - 0.35) * 0.45).clamp(0.0, 0.4);
            if !rng.random_bool(chance as f64) {
                continue;
            }
            let p = Vec2::new(x, z);
            if !clear(p) {
                continue;
            }
            // Sunk a little so nothing floats where the mesh cuts a corner.
            let y = terrain_height(cfg, &heights, x, z) - 0.1;
            let kind = rng.random_range(0..10);
            let size: f32 = rng.random();
            let tint: f32 = rng.random();
            match kind {
                0..=4 => scrub_world(&mut g, &mut rng, p, y, 0.7 + size * 0.9, mix(dry, palette.foliage, tint * 0.4)),
                5..=7 => rock_world(&mut g, &mut rng, p, y, 0.4 + size * 1.2, mul(rock, 0.85 + tint * 0.4)),
                8 => dry_tree_world(&mut g, &mut rng, p, y, 3.0 + size * 3.0, mul(palette.bark, 0.8)),
                _ => {
                    let rot = tint * std::f32::consts::TAU;
                    g.part(Surface::Foliage).ground_polygon(&ngon(x, z, 0.8 + size * 1.2, 6, rot), y + 0.12, rgba(mul(dry, 0.9)));
                }
            }
            count += 1;
            if count >= CAP {
                return g;
            }
        }
    }
    g
}

/// Height-field mesh with colours from the palette.
pub fn build_terrain(cfg: &TerrainConfig, palette: &Palette) -> MeshData {
    let noise = height_noise(cfg);
    let detail = Fbm::<Perlin>::new(cfg.seed.wrapping_add(99)).set_octaves(3);
    let n = ((cfg.half_size * 2.0) / cfg.cell).ceil() as usize + 1;
    let mut mesh = MeshData::default();
    let mut heights = vec![0.0f32; n * n];

    for j in 0..n {
        for i in 0..n {
            let x = -cfg.half_size + i as f32 * cfg.cell;
            let z = -cfg.half_size + j as f32 * cfg.cell;
            let h = terrain_height(cfg, &noise, x, z);
            heights[j * n + i] = h;
            let v = detail.get([x as f64 * 0.02, z as f64 * 0.02]) as f32 * 0.5 + 0.5;
            let hill_t = (h / cfg.hill_height).clamp(0.0, 1.0);
            let mut c = [0.0f32; 4];
            for k in 0..3 {
                let flat = palette.ground[k] + palette.ground_variation[k] * v;
                c[k] = flat * (1.0 - hill_t) + palette.hills[k] * hill_t;
            }
            c[3] = 1.0;
            mesh.positions.push([x, h, z]);
            mesh.normals.push([0.0, 1.0, 0.0]);
            mesh.colors.push(c);
            mesh.uvs.push([x, -z]);
        }
    }

    // Normals from finite differences.
    for j in 0..n {
        for i in 0..n {
            let h = |ii: isize, jj: isize| {
                let ii = ii.clamp(0, n as isize - 1) as usize;
                let jj = jj.clamp(0, n as isize - 1) as usize;
                heights[jj * n + ii]
            };
            let dx = h(i as isize + 1, j as isize) - h(i as isize - 1, j as isize);
            let dz = h(i as isize, j as isize + 1) - h(i as isize, j as isize - 1);
            let nrm = glam::Vec3::new(-dx, 2.0 * cfg.cell, -dz).normalize();
            mesh.normals[j * n + i] = nrm.to_array();
        }
    }

    for j in 0..n - 1 {
        for i in 0..n - 1 {
            let a = (j * n + i) as u32;
            let b = a + 1;
            let c = a + n as u32;
            let d = c + 1;
            mesh.indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    mesh
}
