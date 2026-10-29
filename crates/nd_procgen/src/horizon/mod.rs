//! [horizon] The world past the map, drawn out to the horizon: the far
//! ground in rings of ever coarser detail, the far water, mountain ridges
//! as hazy silhouettes, the ways running on to the neighbouring towns, the
//! towns' skylines and their lights at night, and the shipping lanes.
//!
//! Everything here is a pure function of the saved map (its seed and
//! generated features, [`FarLand`]), the ways leading off it and world
//! coordinates. Nothing is simulated and nothing is written back.
//!
//! The pieces and their budgets (tested):
//! - the far ground: four quadrant meshes, about 140k triangles in all,
//!   out to [`GROUND_REACH`], opaque and fogged; its first row of vertices
//!   is the playable ground's last ([`Seam`]);
//! - the ridges: four quadrant meshes past that out to [`HORIZON`], drawn
//!   translucent in the haze's colour, only where mountains stand;
//! - the far water: four quadrant meshes in rings from the fine sheet's
//!   edge out to the horizon;
//! - the towns and ways ([`towns`], [`ways`]): a few meshes in all.

pub mod land;
pub mod rings;
pub mod towns;
pub mod ways;

pub use land::{FarLand, FAR_D, HORIZON, SEAM_BLEND};
pub use rings::{Ring, Sample};

use crate::climate;
use crate::color::{mix, mul};
use crate::mesh::MeshData;
use crate::palette::Palette;
use crate::terrain::{GroundSpot, LandColours};
use glam::Vec2;
use nd_sim::geom::smoothstep;
use nd_sim::landscape::{Climate, LAND_HALF, WATER_LEVEL};
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use std::collections::HashMap;

/// The opaque far ground's rings end here (each boundary rounded onto the
/// next ring's grid). Past it, out to [`HORIZON`], only the ridges and the
/// water are drawn: from anywhere the camera can be it lies well past the
/// solid fog, where the sky dome's horizon is the fog's colour.
pub const GROUND_REACH: [f32; 5] = [2112.0, 3200.0, 4608.0, 7168.0, 12288.0];
/// The ridge layer's cell.
const RIDGE_CELL: f32 = 256.0;
/// The fine water sheet (`terrain::build_water`) reaches this far.
pub const FINE_WATER: f32 = LAND_HALF + 400.0;

/// The far water's rings, from the fine sheet's edge.
pub fn water_rings() -> [Ring; 5] {
    [
        Ring { inner: FINE_WATER, outer: 3200.0, cell: 40.0 },
        Ring { inner: 3200.0, outer: 4800.0, cell: 80.0 },
        Ring { inner: 4800.0, outer: 8000.0, cell: 160.0 },
        Ring { inner: 8000.0, outer: 12800.0, cell: 320.0 },
        Ring { inner: 12800.0, outer: HORIZON, cell: 640.0 },
    ]
}

/// The playable ground's outermost vertices (on the square of half side
/// `half`), as its mesh has them: the far ground's first row is these.
pub struct Seam {
    half: f32,
    cell: f32,
    verts: HashMap<(i32, i32), Sample>,
}

impl Seam {
    /// From the playable ground's meshes (its tiles), whose grid has cell
    /// `cell` and reaches `half`.
    pub fn from_meshes<'m>(meshes: impl IntoIterator<Item = &'m MeshData>, half: f32, cell: f32) -> Self {
        let mut verts = HashMap::new();
        for m in meshes {
            for (i, p) in m.positions.iter().enumerate() {
                if (p[0].abs().max(p[2].abs()) - half).abs() > 0.01 {
                    continue;
                }
                let s = Sample { h: p[1], normal: m.normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]), rgba: m.colors.get(i).copied().unwrap_or([1.0; 4]) };
                verts.insert(seam_key(Vec2::new(p[0], p[2])), s);
            }
        }
        Self { half, cell, verts }
    }

    pub fn half(&self) -> f32 {
        self.half
    }

    pub fn cell(&self) -> f32 {
        self.cell
    }

    pub fn get(&self, p: Vec2) -> Option<Sample> {
        self.verts.get(&seam_key(p)).copied()
    }

    pub fn len(&self) -> usize {
        self.verts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.verts.is_empty()
    }
}

fn seam_key(p: Vec2) -> (i32, i32) {
    ((p.x * 4.0).round() as i32, (p.y * 4.0).round() as i32)
}

/// Where snow lies on the far mountains, by climate (metres).
fn snow_line(c: Climate) -> f32 {
    match c {
        Climate::Arctic => 80.0,
        Climate::Volcanic => 240.0,
        Climate::Temperate => 620.0,
        Climate::Tropical | Climate::Desert => f32::INFINITY,
    }
}

const SNOW: [f32; 3] = [0.56, 0.59, 0.64];

/// How snowy ground at height `h` is, 0..1, over `band` metres above the
/// snow line (none where the climate has no snow line).
fn snow_at(line: f32, band: f32, h: f32) -> f32 {
    if line.is_finite() { smoothstep(line, line + band, h) } else { 0.0 }
}

/// The far ground and the ridges past it.
pub struct FarGround {
    pub ground: [MeshData; 4],
    pub ridges: [MeshData; 4],
}

impl FarGround {
    pub fn triangles(&self) -> usize {
        self.ground.iter().chain(self.ridges.iter()).map(|m| m.indices.len() / 3).sum()
    }
}

/// The far ground round the playable ground: `near(p)` is the map's own
/// ground under it (the built ground, graded to the ways running out, or
/// the flat map's hills), and the far land adds its coast, rivers, islands
/// and ranges past the seam. `seam` holds the playable ground's edge.
pub fn far_ground(far: &FarLand, near: &(dyn Fn(Vec2) -> f32 + Sync), seam: &Seam, palette: &Palette) -> FarGround {
    let land = far.landscape();
    let clim = land.params.climate;
    let colours = LandColours::for_climate(clim, palette);
    let fine = Fbm::<Perlin>::new(land.params.seed as u32 ^ 99).set_octaves(3);
    let coarse = Fbm::<Perlin>::new(land.params.seed as u32 ^ 91).set_octaves(3);
    let line = snow_line(clim);
    let half = seam.half();
    // A hair under the map's ground at the seam (as its tiles are), half a
    // metre a cell out, so the ways out there are never covered.
    let sink = |p: Vec2| 0.08 + 0.42 * smoothstep(half, half + seam.cell(), p.x.abs().max(p.y.abs()));
    let height = |p: Vec2| near(p) + far.extra(p);
    let shaded = |p: Vec2| height(p) - sink(p);
    let sample = |p: Vec2, cell: f32| -> Sample {
        if let Some(s) = seam.get(p) {
            return s;
        }
        let h = height(p);
        let w = FarLand::blend(p);
        let v_fine = fine.get([p.x as f64 * 0.02, p.y as f64 * 0.02]) as f32 * 0.5 + 0.5;
        let v_far = coarse.get([p.x as f64 * 0.004, p.y as f64 * 0.004]) as f32 * 0.5 + 0.5;
        let spot = GroundSpot { v: v_fine + (v_far - v_fine) * w, d: far.colour_distance(p), hot: None };
        let mut rgb = colours.rgb(spot, h);
        let normal = rings::normal_of(&shaded, p, cell);
        let snow = snow_at(line, 260.0, h) * smoothstep(0.55, 0.85, normal[1]);
        rgb = mix(rgb, SNOW, snow * 0.9);
        Sample { h: h - sink(p), normal, rgba: [rgb[0], rgb[1], rgb[2], 1.0] }
    };
    let ground_rings = rings::layout(half, seam.cell(), &GROUND_REACH);
    let ground = rings::build(&ground_rings, &sample, &|_, _| true);
    let reach = ground_rings.last().map_or(half, |r| r.outer);
    FarGround { ground, ridges: ridges(far, reach) }
}

/// The ranges past the opaque ground as silhouettes: each vertex's colour
/// is a shade of the haze (the renderer's material is the fog's colour)
/// and its alpha how much mountain stands there, so flat land and sea
/// leave the sky dome's horizon, which is the fog's colour, showing.
pub fn ridges(far: &FarLand, from: f32) -> [MeshData; 4] {
    let ring = [Ring { inner: from, outer: HORIZON, cell: RIDGE_CELL }];
    let line = snow_line(far.landscape().params.climate);
    let mask = |p: Vec2| smoothstep(30.0, 320.0, far.mountains(p));
    let sample = |p: Vec2, cell: f32| -> Sample {
        let h = far.natural(p);
        let n = rings::normal_of(&|q| far.natural(q), p, cell * 0.5);
        let snow = snow_at(line, 300.0, h);
        let s = (0.58 + 0.75 * snow) * (0.8 + 0.2 * n[1]);
        Sample { h, normal: n, rgba: [s, s, s, mask(p)] }
    };
    let keep = |c: Vec2, cell: f32| {
        let k = cell * 0.5;
        [Vec2::ZERO, Vec2::new(-k, -k), Vec2::new(k, -k), Vec2::new(-k, k), Vec2::new(k, k)].iter().any(|o| mask(c + *o) > 0.0)
    };
    rings::build(&ring, &sample, &keep)
}

/// The far water: the sea and rivers past the fine sheet out to the
/// horizon, in rings, deep-coloured (sea ice along an Arctic shore).
pub fn far_water(far: &FarLand, palette: &Palette) -> [MeshData; 4] {
    let land = far.landscape();
    if land.is_flat() {
        return Default::default();
    }
    let look = climate::look(land.params.climate);
    let (shallow, deep) = look.water.unwrap_or((mix(mul(palette.water, 0.45), [0.2, 0.3, 0.28], 0.4), mul(palette.water, 0.22)));
    // The sea ice as the fine sheet draws it (`terrain::build_water`).
    let crack = Fbm::<Perlin>::new(land.params.seed as u32 ^ 0x1CE).set_octaves(3);
    let sample = |p: Vec2, _cell: f32| -> Sample {
        let d = far.water_distance(p);
        let t = ((-d) / 25.0).clamp(0.0, 1.0);
        let reach = look.sea_ice * (0.5 + 0.9 * (crack.get([p.x as f64 * 0.004, p.y as f64 * 0.004]) as f32 * 0.5 + 0.5));
        let (rgb, a) = match look.ice {
            Some(ice) if d < 0.0 && d > -reach => {
                let v = crack.get([p.x as f64 * 0.03, p.y as f64 * 0.03]) as f32;
                (mul(ice, 0.85 + 0.3 * v.abs() + 0.15 * (1.0 - t)), 0.97)
            }
            _ => (mix(shallow, deep, t), 0.86),
        };
        Sample { h: WATER_LEVEL, normal: [0.0, 1.0, 0.0], rgba: [rgb[0], rgb[1], rgb[2], a] }
    };
    // A cell with water at a corner, its middle or within half a cell of
    // them: the banks hide the sheet's edge under the land.
    let keep = |c: Vec2, cell: f32| {
        let k = cell * 0.5;
        [Vec2::ZERO, Vec2::new(-k, -k), Vec2::new(k, -k), Vec2::new(-k, k), Vec2::new(k, k)].iter().any(|o| far.water_distance(c + *o) < k)
    };
    rings::build(&water_rings(), &sample, &keep)
}

#[cfg(test)]
mod tests;
