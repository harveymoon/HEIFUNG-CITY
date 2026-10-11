//! The ground: a flat basin for the city with procedural hills around it, and
//! a scatter pass that puts rocks, scrub and dead trees on the open ground.

use crate::buildings::parts::{mix, mul, ngon, rgba, rock_world, scrub_world, tree_kind, TreeKind};
use crate::climate::{self, Flora};
use crate::mesh::MeshData;
use crate::palette::Palette;
use crate::surface::{Geometry, Surface};
use glam::Vec2;
use nd_sim::rng_for;
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rand::Rng;
use nd_sim::landscape::{WaterKind, BANK_HEIGHT, LAND_HALF, WATER_LEVEL};
use nd_sim::ports::PortKind;
use nd_sim::ShipKind;
use nd_sim::{PowerPlantKind, World};

mod docks;
mod heightfield;
mod retaining;
mod water;
mod works;

pub use docks::*;
pub use heightfield::*;
pub use retaining::*;
pub use water::*;
pub use works::*;

#[derive(Clone, Debug)]
pub struct TerrainConfig {
    pub seed: u32,
    /// Half-size of the terrain mesh in metres.
    pub half_size: f32,
    pub cell: f32,
    /// Radius of the flat basin the city sits in.
    pub basin_radius: f32,
    pub hill_height: f32,
    /// Ways running out through the hills (the highway, the trunk line,
    /// roads off the map): ends and half width. The flat map's roads lie at
    /// height zero, so the hills are levelled along them.
    pub corridors: Vec<(Vec2, Vec2, f32)>,
}

impl Default for TerrainConfig {
    fn default() -> Self {
        Self { seed: 7, half_size: 1800.0, cell: 12.0, basin_radius: 1000.0, hill_height: 160.0, corridors: Vec::new() }
    }
}

pub fn terrain_height(cfg: &TerrainConfig, noise: &Fbm<Perlin>, x: f32, z: f32) -> f32 {
    let r = (x * x + z * z).sqrt();
    if r <= cfg.basin_radius {
        return 0.0;
    }
    let t = ((r - cfg.basin_radius) / (cfg.half_size - cfg.basin_radius).max(1e-3)).clamp(0.0, 1.0);
    let ramp = t * t * (3.0 - 2.0 * t);
    let n = noise.get([x as f64 * 0.0025, z as f64 * 0.0025]) as f32 * 0.5 + 0.5;
    let mut level = 1.0f32;
    let p = Vec2::new(x, z);
    for &(a, b, hw) in &cfg.corridors {
        let core = hw + 40.0;
        let reach = core + 160.0;
        if x < a.x.min(b.x) - reach || x > a.x.max(b.x) + reach || z < a.y.min(b.y) - reach || z > a.y.max(b.y) + reach {
            continue;
        }
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
        let d = (a + ab * t).distance(p);
        let k = ((d - core) / (reach - core)).clamp(0.0, 1.0);
        level = level.min(k * k * (3.0 - 2.0 * k));
    }
    ramp * cfg.hill_height * (0.4 + 0.6 * n) * level
}

/// The flat map's ground height anywhere, the hills beyond the drawn
/// square included (they keep their full height out there).
pub fn flat_height(cfg: &TerrainConfig) -> impl Fn(Vec2) -> f32 + '_ {
    let noise = height_noise(cfg);
    move |p: Vec2| terrain_height(cfg, &noise, p.x, p.y)
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
///
/// Every cell draws from its own RNG seeded by its grid position, so a new
/// road that clears one cell leaves every rock downstream exactly where it
/// was instead of re-rolling the whole field.
/// The scatter's grid cells that grow something, each with its own stream
/// already past the draws that placed it: every cell that passes its own
/// chance and `clear`, then the `cap` with the lowest hash of the cell. So
/// clearing a cell (a road through it) changes that cell and at most lets
/// one other in at the cap, instead of shifting where a running count ran
/// out (review §1.5).
fn scatter_cells(cfg: &TerrainConfig, seed: u64, growth: f32, clear: &dyn Fn(Vec2) -> bool) -> Vec<(i32, i32)> {
    const CAP: usize = 3500;
    let cap = (CAP as f32 * growth) as usize;
    let density = Fbm::<Perlin>::new(cfg.seed.wrapping_add(7)).set_octaves(2);
    let n = ((cfg.half_size * 2.0 - 40.0) / SCATTER_STEP) as i32;
    let mut cells: Vec<(u64, i32, i32)> = Vec::new();
    for j in 0..n {
        for i in 0..n {
            let mut rng = scatter_rng(seed, i, j);
            let p = scatter_spot(cfg, &mut rng, i, j);
            let d = density.get([p.x as f64 * 0.004, p.y as f64 * 0.004]) as f32 * 0.5 + 0.5;
            let chance = ((d - 0.35) * 0.45).clamp(0.0, 0.4) * growth;
            if !rng.random_bool(chance as f64) || !clear(p) {
                continue;
            }
            cells.push((rng_for(seed ^ 0x5CA7, ((i as u64) << 20) | j as u64).random::<u64>(), i, j));
        }
    }
    if cells.len() > cap {
        cells.sort_unstable();
        cells.truncate(cap);
        cells.sort_unstable_by_key(|c| (c.2, c.1));
    }
    cells.into_iter().map(|(_, i, j)| (i, j)).collect()
}

const SCATTER_STEP: f32 = 18.0;

fn scatter_rng(seed: u64, i: i32, j: i32) -> nd_sim::SimRng {
    rng_for(seed, 41 + ((i as u64) << 20 | j as u64) * 0x9E37)
}

/// A cell's jittered spot, drawn first from its stream.
fn scatter_spot(cfg: &TerrainConfig, rng: &mut impl Rng, i: i32, j: i32) -> Vec2 {
    let x = -cfg.half_size + 20.0 + (i as f32 + 0.5 + rng.random_range(-0.45..0.45)) * SCATTER_STEP;
    let z = -cfg.half_size + 20.0 + (j as f32 + 0.5 + rng.random_range(-0.45..0.45)) * SCATTER_STEP;
    Vec2::new(x, z)
}

pub fn scatter_nature(cfg: &TerrainConfig, palette: &Palette, seed: u64, clear: &dyn Fn(Vec2) -> bool) -> Geometry {
    let mut g = Geometry::default();
    let heights = height_noise(cfg);
    let dry = palette.foliage_dry;
    let rock = mix(palette.stone, palette.hills, 0.3);
    // [climate] Lusher climates grow more, and may draw more pieces.
    let growth = palette.flora.growth();
    for (i, j) in scatter_cells(cfg, seed, growth, clear) {
        {
            let mut rng = scatter_rng(seed, i, j);
            let p = scatter_spot(cfg, &mut rng, i, j);
            // The chance draw `scatter_cells` made, so the rest follows.
            let _: u64 = rng.random();
            let (x, z) = (p.x, p.y);
            // Sunk a little so nothing floats where the mesh cuts a corner.
            let y = terrain_height(cfg, &heights, x, z) - 0.1;
            let kind = rng.random_range(0..10);
            let size: f32 = rng.random();
            let tint: f32 = rng.random();
            let leaf = palette.foliage;
            let tuft = |g: &mut Geometry, c: [f32; 3], r: f32| {
                let rot = tint * std::f32::consts::TAU;
                g.part(Surface::Foliage).ground_polygon(&ngon(x, z, r, 6, rot), y + 0.12, rgba(c));
            };
            match (palette.flora, kind) {
                (Flora::Dry, 0..=4) => scrub_world(&mut g, &mut rng, p, y, 0.7 + size * 0.9, mix(dry, palette.foliage, tint * 0.4)),
                (Flora::Dry, 5..=7) => rock_world(&mut g, &mut rng, p, y, 0.4 + size * 1.2, mul(rock, 0.85 + tint * 0.4)),
                (Flora::Dry, 8) => tree_kind(&mut g, p, y, TreeKind::Dead, 3.0 + size * 3.0, 0.0, mul(palette.bark, 0.8), dry),
                (Flora::Dry, _) => tuft(&mut g, mul(dry, 0.9), 0.8 + size * 1.2),
                // Meadow and woodland: leafy trees in clumps, bushes, grass.
                (Flora::Leafy, 0..=3) => scrub_world(&mut g, &mut rng, p, y, 0.8 + size * 1.0, mul(mix(leaf, dry, tint * 0.3), 0.9 + tint * 0.3)),
                (Flora::Leafy, 4) => rock_world(&mut g, &mut rng, p, y, 0.4 + size * 1.0, mul(rock, 0.85 + tint * 0.3)),
                (Flora::Leafy, 5..=7) => tree_kind(&mut g, p, y, TreeKind::Round, 3.5 + size * 4.0, 3.0 + size * 2.5, palette.bark, mul(leaf, 0.8 + tint * 0.45)),
                (Flora::Leafy, 8) => tree_kind(&mut g, p, y, TreeKind::Conifer, 6.0 + size * 5.0, 2.2 + size, palette.bark, mul(leaf, 0.7)),
                (Flora::Leafy, _) => tuft(&mut g, mul(leaf, 0.8 + tint * 0.3), 1.0 + size * 1.6),
                // Jungle edge: palms, broad trees and dense undergrowth.
                (Flora::Palms, 0..=2) => scrub_world(&mut g, &mut rng, p, y, 1.0 + size * 1.3, mul(leaf, 0.85 + tint * 0.4)),
                (Flora::Palms, 3..=5) => tree_kind(&mut g, p, y, TreeKind::Palm, 6.0 + size * 6.0, 0.0, mul(palette.bark, 1.4), mul(leaf, 0.9 + tint * 0.4)),
                (Flora::Palms, 6..=7) => tree_kind(&mut g, p, y, TreeKind::Round, 4.0 + size * 4.0, 3.5 + size * 3.0, palette.bark, mul(leaf, 0.75 + tint * 0.4)),
                (Flora::Palms, 8) => rock_world(&mut g, &mut rng, p, y, 0.4 + size * 0.8, mul(rock, 0.8)),
                (Flora::Palms, _) => tuft(&mut g, mul(leaf, 0.9 + tint * 0.3), 1.2 + size * 1.8),
                // Taiga: spruce, boulders, the odd bare birch, low dark shrubs.
                (Flora::Conifers, 0..=4) => tree_kind(&mut g, p, y, TreeKind::Conifer, 5.0 + size * 7.0, 1.6 + size * 1.4, palette.bark, mul(leaf, 0.8 + tint * 0.4)),
                (Flora::Conifers, 5..=6) => rock_world(&mut g, &mut rng, p, y, 0.5 + size * 1.4, mul(rock, 0.8 + tint * 0.3)),
                (Flora::Conifers, 7) => tree_kind(&mut g, p, y, TreeKind::Dead, 3.0 + size * 3.0, 0.0, [0.5, 0.48, 0.45], dry),
                (Flora::Conifers, _) => scrub_world(&mut g, &mut rng, p, y, 0.6 + size * 0.7, mul(leaf, 0.9)),
                // Lava field: moss cushions over black rock, a stunted tree or two.
                (Flora::Moss, 0..=4) => tuft(&mut g, mul(mix(leaf, dry, tint * 0.4), 0.8 + tint * 0.4), 1.4 + size * 2.4),
                (Flora::Moss, 5..=7) => rock_world(&mut g, &mut rng, p, y, 0.5 + size * 1.6, mul(palette.ground, 1.6 + tint)),
                (Flora::Moss, 8) => tree_kind(&mut g, p, y, TreeKind::Round, 2.0 + size * 1.5, 1.4 + size, [0.55, 0.53, 0.5], mul(leaf, 0.9)),
                (Flora::Moss, _) => scrub_world(&mut g, &mut rng, p, y, 0.5 + size * 0.6, mul(leaf, 0.7)),
            }
        }
    }
    g
}

/// Raise every vertex of `geo` by one height (a building on its pad).
pub fn raise_geometry(geo: &mut Geometry, dy: f32) {
    if dy == 0.0 {
        return;
    }
    for part in geo.parts.iter_mut() {
        for p in part.positions.iter_mut() {
            p[1] += dy;
        }
    }
    for p in geo.pads.iter_mut() {
        p.y += dy;
    }
    for rig in geo.cranes.iter_mut() {
        raise_geometry(&mut rig.head, dy);
    }
    for rig in geo.games.iter_mut() {
        raise_geometry(&mut rig.stands, dy);
        raise_geometry(&mut rig.pitch, dy);
        raise_geometry(&mut rig.outside, dy);
        for s in rig.slots.iter_mut() {
            s.0.y += dy;
        }
    }
}
