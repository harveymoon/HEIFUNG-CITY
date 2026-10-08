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

/// The ground beyond the drawn square, out to `outer` (past anything in
/// sight): a coarse height field from `height`, coloured like the hills,
/// mud under water. Cells wholly inside `inner` less a cell are left out;
/// the ring overlaps the main terrain by a cell and sits a little lower, so
/// there is never a gap at the seam and the main terrain wins over it.
pub fn build_far_ground(inner: f32, outer: f32, cell: f32, height: &dyn Fn(Vec2) -> f32, wet: &dyn Fn(Vec2) -> bool, palette: &Palette) -> MeshData {
    let n = ((outer * 2.0) / cell).ceil() as usize + 1;
    let at = |i: usize, j: usize| Vec2::new(-outer + i as f32 * cell, -outer + j as f32 * cell);
    // Grid points needed: everything but the middle, less a cell's margin
    // for the normals.
    let hole = inner - cell;
    let skip = |p: Vec2, margin: f32| p.x.abs() < hole - margin && p.y.abs() < hole - margin;
    let mut heights = vec![f32::NAN; n * n];
    for j in 0..n {
        for i in 0..n {
            let p = at(i, j);
            if !skip(p, cell * 1.5) {
                heights[j * n + i] = height(p) - 0.5;
            }
        }
    }
    let mut mesh = MeshData::default();
    let mut index = vec![u32::MAX; n * n];
    let detail = Fbm::<Perlin>::new(91).set_octaves(2);
    let mud = mul(mix(palette.ground, [0.25, 0.22, 0.16], 0.6), 0.7);
    let h = |i: usize, j: usize| {
        let v = heights[j.min(n - 1) * n + i.min(n - 1)];
        if v.is_nan() { 0.0 } else { v }
    };
    for j in 0..n - 1 {
        for i in 0..n - 1 {
            if skip(at(i, j), 0.0) && skip(at(i + 1, j + 1), 0.0) {
                continue;
            }
            let mut corner = |ii: usize, jj: usize, mesh: &mut MeshData| -> u32 {
                let k = jj * n + ii;
                if index[k] != u32::MAX {
                    return index[k];
                }
                let p = at(ii, jj);
                let y = h(ii, jj);
                let v = detail.get([p.x as f64 * 0.004, p.y as f64 * 0.004]) as f32 * 0.5 + 0.5;
                let hill_t = ((y - 12.0) / 120.0).clamp(0.0, 1.0);
                let mut rgb = [0.0f32; 3];
                for c in 0..3 {
                    let f = palette.ground[c] + palette.ground_variation[c] * v;
                    rgb[c] = f * (1.0 - hill_t) + palette.hills[c] * hill_t;
                }
                if wet(p) {
                    rgb = mud;
                }
                let dx = h(ii + 1, jj) - h(ii.saturating_sub(1), jj);
                let dz = h(ii, jj + 1) - h(ii, jj.saturating_sub(1));
                mesh.positions.push([p.x, y, p.y]);
                mesh.normals.push(glam::Vec3::new(-dx, 2.0 * cell, -dz).normalize().to_array());
                mesh.colors.push([rgb[0], rgb[1], rgb[2], 1.0]);
                mesh.uvs.push([p.x, -p.y]);
                index[k] = (mesh.positions.len() - 1) as u32;
                index[k]
            };
            let a = corner(i, j, &mut mesh);
            let b = corner(i + 1, j, &mut mesh);
            let c = corner(i, j + 1, &mut mesh);
            let d = corner(i + 1, j + 1, &mut mesh);
            mesh.indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    mesh
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

// ── the landscape: ground, water and the works in it ───────────────────────

use nd_sim::landscape::{WaterKind, BANK_HEIGHT, LAND_HALF, WATER_LEVEL};
use nd_sim::ports::PortKind;
use nd_sim::ShipKind;
use nd_sim::{PowerPlantKind, World};

/// Raise every vertex of `geo` by `h(x, z)`: street meshes, lot fills and
/// overlays are generated on a flat datum and set on the built ground here.
pub fn lift_geometry(geo: &mut Geometry, h: &dyn Fn(Vec2) -> f32) {
    for part in geo.parts.iter_mut() {
        lift_mesh(part, h);
    }
}

pub fn lift_mesh(mesh: &mut MeshData, h: &dyn Fn(Vec2) -> f32) {
    for p in mesh.positions.iter_mut() {
        p[1] += h(Vec2::new(p[0], p[2]));
    }
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
}

/// The ground mesh for a world with a landscape: the built ground (roads
/// cut and filled in, lots levelled) inside the city, natural terrain
/// beyond, sand along the water, mud under it, sulphur at the hot spots.
/// For the flat map use [`build_terrain`].
pub fn build_landscape_terrain(world: &World, palette: &Palette, cell: f32) -> MeshData {
    let land = &world.landscape;
    let half = LAND_HALF;
    let n = ((half * 2.0) / cell).ceil() as usize + 1;
    let detail = Fbm::<Perlin>::new(land.params.seed as u32 ^ 99).set_octaves(3);
    let mut mesh = MeshData::default();
    let mut heights = vec![0.0f32; n * n];
    // [climate] The shore and the hot ground from the climate's row.
    let look = climate::look(land.params.climate);
    let sand = look.shore.unwrap_or_else(|| mix(palette.ground, [0.62, 0.56, 0.42], 0.6));
    let mud = mul(mix(palette.ground, [0.25, 0.22, 0.16], 0.6), 0.7);
    let sulphur = look.hot_ground;
    for j in 0..n {
        for i in 0..n {
            let x = -half + i as f32 * cell;
            let z = -half + j as f32 * cell;
            let p = Vec2::new(x, z);
            // A hair under the graded ground so road and lot meshes win.
            let h = world.ground_height(p) - 0.08;
            heights[j * n + i] = h;
            let v = detail.get([x as f64 * 0.02, z as f64 * 0.02]) as f32 * 0.5 + 0.5;
            let hill_t = ((h - 12.0) / 120.0).clamp(0.0, 1.0);
            let mut flat = [0.0f32; 3];
            for k in 0..3 {
                let f = palette.ground[k] + palette.ground_variation[k] * v;
                flat[k] = f * (1.0 - hill_t) + palette.hills[k] * hill_t;
            }
            let d = land.water_distance(p);
            let rgb = if d < 0.0 {
                mud
            } else if d < 14.0 {
                mix(sand, flat, d / 14.0)
            } else {
                flat
            };
            let rgb = match land.hot_spots.iter().find(|s| s.pos.distance(p) < s.radius) {
                Some(s) => mix(rgb, sulphur, (1.0 - s.pos.distance(p) / s.radius) * 0.7),
                None => rgb,
            };
            mesh.positions.push([x, h, z]);
            mesh.normals.push([0.0, 1.0, 0.0]);
            mesh.colors.push([rgb[0], rgb[1], rgb[2], 1.0]);
            mesh.uvs.push([x, -z]);
        }
    }
    keep_under_streets(world, cell, n, &mut heights);
    for (p, h) in mesh.positions.iter_mut().zip(&heights) {
        p[1] = *h;
    }
    for j in 0..n {
        for i in 0..n {
            let h = |ii: isize, jj: isize| heights[(jj.clamp(0, n as isize - 1) as usize) * n + ii.clamp(0, n as isize - 1) as usize];
            let dx = h(i as isize + 1, j as isize) - h(i as isize - 1, j as isize);
            let dz = h(i as isize, j as isize + 1) - h(i as isize, j as isize - 1);
            mesh.normals[j * n + i] = glam::Vec3::new(-dx, 2.0 * cell, -dz).normalize().to_array();
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

/// Lower the terrain mesh's vertices round every street on the ground so no
/// triangle rises over its carriageway. The mesh is coarser than the built
/// ground (`cell` against `GROUND_CELL`) and runs straight from vertex to
/// vertex, so a vertex on a cutting's side slope, a lot's pad or the far
/// side of a junction a few metres off the kerb drew a sheet of earth over
/// the lanes beside it on any slope. Each vertex near enough for one of its
/// triangles to reach the carriageway is held to the height of the way
/// beside it (as if the road ran on sideways at its own height), a hair
/// under: a triangle whose corners are all under that surface is under it
/// everywhere between. Sunken roads between retaining walls, tunnels and
/// decks are left alone: their walls and portals meet the ground as built.
fn keep_under_streets(world: &World, cell: f32, n: usize, heights: &mut [f32]) {
    let half = LAND_HALF;
    let roads = &world.roads;
    // A vertex reaches a point at most a cell's diagonal from it.
    let reach = cell * std::f32::consts::SQRT_2;
    let index = |v: f32| (((v + half) / cell).floor().max(0.0) as usize).min(n - 1);
    for (sid, s) in roads.alive_segments() {
        if !s.is_street() || s.bridge || s.viaduct || s.tunnel || roads.node(s.a).level < 0.0 || roads.node(s.b).level < 0.0 {
            continue;
        }
        let (a, b) = roads.segment_points(sid);
        let r = s.half_width() + reach;
        let (lo, hi) = (a.min(b) - Vec2::splat(r), a.max(b) + Vec2::splat(r));
        for j in index(lo.y)..=(index(hi.y) + 1).min(n - 1) {
            for i in index(lo.x)..=(index(hi.x) + 1).min(n - 1) {
                let p = Vec2::new(-half + i as f32 * cell, -half + j as f32 * cell);
                let (t, q) = nd_sim::roads::project_on_segment(p, a, b);
                if q.distance(p) > r {
                    continue;
                }
                let k = j * n + i;
                heights[k] = heights[k].min(world.road_height(sid, t) - 0.08);
            }
        }
    }
}

/// The water surface: one flat sheet at the waterline over every cell near
/// water (the banks hide its edges), shaded deeper away from the shore.
/// Drawn with its own translucent material.
pub fn build_water(world: &World, palette: &Palette) -> MeshData {
    let land = &world.landscape;
    let mut mesh = MeshData::default();
    if land.is_flat() {
        return mesh;
    }
    let cell = 10.0;
    let half = LAND_HALF + 400.0;
    let n = ((half * 2.0) / cell) as i32;
    let look = climate::look(land.params.climate);
    let (shallow, deep) = look.water.unwrap_or((mix(mul(palette.water, 0.45), [0.2, 0.3, 0.28], 0.4), mul(palette.water, 0.22)));
    // [climate] Still water frozen over: lakes, and the sea out to
    // `sea_ice` from the shore. Rivers and streams keep running.
    let crack = Fbm::<Perlin>::new(land.params.seed as u32 ^ 0x1CE).set_octaves(3);
    let frozen = |p: Vec2, d: f32| -> bool {
        look.ice.is_some() && (land.lakes.iter().any(|l| l.center.distance(p) < 400.0 && nd_sim::geom::point_in_polygon(p, &l.outline)) || (d > -look.sea_ice * (0.5 + 0.9 * (crack.get([p.x as f64 * 0.004, p.y as f64 * 0.004]) as f32 * 0.5 + 0.5)) && p.x.abs() < LAND_HALF && p.y.abs() < LAND_HALF && land.sea.as_ref().is_some_and(|s| s.contains(p))))
    };
    let wet = |p: Vec2| -> Option<f32> {
        if p.x.abs() > LAND_HALF || p.y.abs() > LAND_HALF {
            return if land.is_water(p) { Some(-60.0) } else { None };
        }
        let d = land.water_distance(p);
        (d < 12.0).then_some(d)
    };
    for j in 0..n {
        for i in 0..n {
            let x0 = -half + i as f32 * cell;
            let z0 = -half + j as f32 * cell;
            let corners = [Vec2::new(x0, z0), Vec2::new(x0 + cell, z0), Vec2::new(x0 + cell, z0 + cell), Vec2::new(x0, z0 + cell)];
            let ds: Vec<Option<f32>> = corners.iter().map(|c| wet(*c)).collect();
            if ds.iter().all(|d| d.is_none()) {
                continue;
            }
            let base = mesh.positions.len() as u32;
            // Ice per corner, so its edge fades over a cell into open water
            // instead of stepping; corners on the bank follow the cell's water.
            let mut fr = [false; 4];
            if look.ice.is_some() {
                for (k, c) in corners.iter().enumerate() {
                    fr[k] = ds[k].is_some_and(|d| d < 0.0 && frozen(*c, d));
                }
                let any = fr.iter().any(|f| *f);
                for k in 0..4 {
                    if ds[k].is_none_or(|d| d >= 0.0) {
                        fr[k] = any;
                    }
                }
            }
            for (k, c) in corners.iter().enumerate() {
                let d = ds[k].unwrap_or(12.0);
                let t = ((-d) / 25.0).clamp(0.0, 1.0);
                let (rgb, a) = match look.ice.filter(|_| fr[k]) {
                    Some(ice) => {
                        let v = crack.get([c.x as f64 * 0.03, c.y as f64 * 0.03]) as f32;
                        (mul(ice, 0.85 + 0.3 * v.abs() + 0.15 * (1.0 - t)), 0.97)
                    }
                    None => (mix(shallow, deep, t), 0.86),
                };
                mesh.positions.push([c.x, WATER_LEVEL, c.y]);
                mesh.normals.push([0.0, 1.0, 0.0]);
                mesh.colors.push([rgb[0], rgb[1], rgb[2], a]);
                mesh.uvs.push([c.x, -c.y]);
            }
            mesh.indices.extend_from_slice(&[base, base + 3, base + 1, base + 1, base + 3, base + 2]);
        }
    }
    mesh
}

/// The water beyond the fine sheet of [`build_water`]: the sea and the
/// rivers running on out of sight, in coarse cells. It depends on the
/// landscape alone, so it is built once per map.
pub fn build_far_water(world: &World, palette: &Palette) -> MeshData {
    let land = &world.landscape;
    let mut mesh = MeshData::default();
    if land.is_flat() {
        return mesh;
    }
    let half = LAND_HALF + 400.0;
    let deep = climate::look(land.params.climate).water.map_or(mul(palette.water, 0.22), |w| w.1);
    let far_cell = 40.0;
    // On the same grid as the fine sheet's edge, so the two meet exactly.
    let outer = half + ((nd_sim::landscape::FAR_GROUND - half) / far_cell).ceil() * far_cell;
    let m = ((outer * 2.0) / far_cell) as i32;
    for j in 0..m {
        for i in 0..m {
            let x0 = -outer + i as f32 * far_cell;
            let z0 = -outer + j as f32 * far_cell;
            // Only outside the fine sheet.
            if x0 >= -half && x0 + far_cell <= half && z0 >= -half && z0 + far_cell <= half {
                continue;
            }
            let corners = [Vec2::new(x0, z0), Vec2::new(x0 + far_cell, z0), Vec2::new(x0 + far_cell, z0 + far_cell), Vec2::new(x0, z0 + far_cell)];
            if !corners.iter().any(|c| land.is_water(*c)) && !land.is_water(Vec2::new(x0 + far_cell * 0.5, z0 + far_cell * 0.5)) {
                continue;
            }
            let base = mesh.positions.len() as u32;
            for c in corners {
                mesh.positions.push([c.x, WATER_LEVEL, c.y]);
                mesh.normals.push([0.0, 1.0, 0.0]);
                mesh.colors.push([deep[0], deep[1], deep[2], 0.86]);
                mesh.uvs.push([c.x, -c.y]);
            }
            mesh.indices.extend_from_slice(&[base, base + 3, base + 1, base + 1, base + 3, base + 2]);
        }
    }
    mesh
}

/// Where a way crosses the highway well above or below it
/// (`Ground::underpasses`): the upper way's deck over the cut with parapets
/// and rails, and abutment walls either side of the lower way holding the
/// embankment back, stepped down to the ground beyond them. The walls run
/// from just outside the lower way's pavement to past the cut's edge, so the
/// drawn terrain's slope into the cut is inside them.
fn underpasses(world: &World, g: &mut Geometry, concrete: [f32; 3], steel: [f32; 3]) {
    use glam::Vec3;
    let all = &world.ground.underpasses;
    for (i, u) in all.iter().enumerate() {
        // Both carriageways of the highway cut the same street: its walls
        // are drawn once, by the first.
        let drawn = |p: Vec2| all[..i].iter().any(|o| o.under_dir.dot(u.under_dir).abs() > 0.999 && o.in_cut(p));
        let s = u.sin_angle();
        let opening = u.under_half_width + 0.6;
        let back = u.cut_half_width + 8.5;
        // The deck bears on the walls a few metres either side.
        let reach = (opening + 4.0) / s;
        let (a, b) = (u.at - u.over_dir * reach, u.at + u.over_dir * reach);
        let a3 = Vec3::new(a.x, u.over_z_at(-reach) - 0.05, a.y);
        let m3 = Vec3::new(u.at.x, u.over_z - 0.05, u.at.y);
        let b3 = Vec3::new(b.x, u.over_z_at(reach) - 0.05, b.y);
        let hw = u.over_half_width + 0.4;
        let side = Vec2::new(-u.over_dir.y, u.over_dir.x) * (hw - 0.2);
        // A highway or ramp carried over has its own concrete barrier along
        // the deck's edge (`street`): a parapet there stood in the same
        // place, the two boxes flickering through each other. It keeps the
        // rail on top.
        let motorway_over = world.roads.segments_in_box(u.at - Vec2::splat(2.0), u.at + Vec2::splat(2.0)).any(|o| {
            let s = world.roads.segment(o);
            let (oa, ob) = world.roads.segment_points(o);
            s.alive && (s.is_motorway() || s.ring) && (ob - oa).normalize_or_zero().dot(u.over_dir).abs() > 0.98 && nd_sim::roads::project_on_segment(u.at, oa, ob).1.distance(u.at) < 1.0
        });
        // Two halves meeting over the crossing: it may be the crest of a
        // way lifted over the other.
        for (p0, p1) in [(a3, m3), (m3, b3)] {
            g.part(Surface::Concrete).slab(p0, p1, hw * 2.0, 1.4, rgba(concrete));
            for sgn in [1.0f32, -1.0] {
                let off = Vec3::new(side.x * sgn, 0.0, side.y * sgn);
                // On the barrier's top (0.8 m) or the parapet's.
                let rail = if motorway_over { 0.88 } else { 1.0 };
                g.part(Surface::Metal).beam(p0 + off + Vec3::Y * rail, p1 + off + Vec3::Y * rail, 0.15, rgba(steel));
                if !motorway_over {
                    g.part(Surface::Concrete).slab(p0 + off + Vec3::Y * 0.45, p1 + off + Vec3::Y * 0.45, 0.35, 0.5, rgba(mul(concrete, 0.9)));
                }
            }
        }
        // Abutment and wing walls along the lower way, their tops following
        // the embankment beyond them down to its toe.
        let across = Vec2::new(-u.under_dir.y, u.under_dir.x);
        let step = 3.0;
        let steps = (u.cut_half_length / step).ceil() as i32;
        let wall = rgba(mul(concrete, 0.78));
        let coping = rgba(mul(concrete, 0.7));
        for sgn in [1.0f32, -1.0] {
            let at = |along: f32, lat: f32, y: f32| {
                let p = u.at + u.under_dir * along + across * sgn * lat;
                Vec3::new(p.x, y, p.y)
            };
            // Wall top at each station: the highest ground the wall holds
            // back there, never above the upper way's carriageway.
            let top = |along: f32| {
                let base = u.at + u.under_dir * along;
                let ground = [0.5f32, 0.75, 1.0]
                    .iter()
                    .map(|f| opening + (back - opening) * f)
                    .chain([back + 4.0])
                    .map(|o| world.ground_height(base + across * sgn * o))
                    .fold(f32::MIN, f32::max);
                let mid = base + across * sgn * (opening + back) * 0.5;
                ground.min(u.over_z_at((mid - u.at).dot(u.over_dir)) - 0.15).max(u.under_z_at(along))
            };
            for k in -steps..steps {
                let (a0, a1) = (k as f32 * step, (k + 1) as f32 * step);
                if drawn(u.at + u.under_dir * (a0 + a1) * 0.5) {
                    continue;
                }
                let (f0, f1) = (u.under_z_at(a0), u.under_z_at(a1));
                let (t0, t1) = (top(a0), top(a1));
                if (t0 - f0).max(t1 - f1) < 0.4 {
                    continue;
                }
                let (b0, b1) = (f0 - 0.6, f1 - 0.6);
                let out = Vec3::new(across.x * sgn, 0.0, across.y * sgn);
                let m = g.part(Surface::Concrete);
                // Face to the lower way, back into the fill, coping on top.
                m.quad_facing([at(a0, opening, b0), at(a1, opening, b1), at(a1, opening, t1), at(a0, opening, t0)], -out, wall);
                m.quad_facing([at(a0, back, b0), at(a1, back, b1), at(a1, back, t1), at(a0, back, t0)], out, wall);
                m.quad_facing([at(a0, opening, t0), at(a1, opening, t1), at(a1, back, t1), at(a0, back, t0)], Vec3::Y, coping);
            }
        }
    }
}

/// Roads the player raised off the ground: a deck the width of the road
/// and its pavements under the street mesh, parapets and rails, and the
/// concrete columns under it. A way the player drew raised stands on the
/// columns placed for it (`World::pillars`: shifted along it to clear the
/// roads and buildings underneath); a way hoisted by the grading alone (a
/// highway lifted over a street) gets one every 24 m or so wherever the
/// deck stands clear of the ground.
pub fn viaducts(world: &World, g: &mut Geometry, concrete: [f32; 3], steel: [f32; 3]) {
    let roads = &world.roads;
    for (sid, seg) in roads.alive_segments() {
        if !seg.viaduct {
            continue;
        }
        let (a, b) = roads.segment_points(sid);
        let za = world.road_z.get(seg.a.0 as usize).copied().unwrap_or(0.0);
        let zb = world.road_z.get(seg.b.0 as usize).copied().unwrap_or(0.0);
        // Highways and ramps have no pavements: the deck is the lanes and shoulders.
        let motorway = seg.is_motorway() || seg.ring;
        let hw = seg.half_width() + if seg.is_street() && !motorway { nd_sim::zoning::SIDEWALK } else { 0.6 };
        let planned = roads.node(seg.a).level.max(roads.node(seg.b).level) >= nd_sim::roads::VIADUCT_LEVEL && !world.pillars.is_empty();
        viaduct_span(g, a, b, za, zb, hw, &|p| world.ground_height(p), concrete, steel, !planned);
    }
    for p in &world.pillars {
        // The deck over it: the raised way it carries, nearest first.
        let deck = roads
            .segments_in_box(p.at - Vec2::splat(p.half_width + 2.0), p.at + Vec2::splat(p.half_width + 2.0))
            .filter(|&s| roads.segment(s).viaduct)
            .map(|s| {
                let (a, b) = roads.segment_points(s);
                let (t, q) = nd_sim::roads::project_on_segment(p.at, a, b);
                (s, t, q.distance(p.at))
            })
            .filter(|&(_, _, d)| d < p.half_width)
            .min_by(|x, y| x.2.total_cmp(&y.2));
        let Some((sid, t, _)) = deck else { continue };
        let top = world.road_height(sid, t) - 1.2;
        let foot = world.ground_height(p.at);
        pillar_column(g, p.at, p.dir, top, foot, p.half_width, concrete);
    }
}

/// One column on its footing from the ground at `foot` up to the deck's
/// underside at `top`, with a crosshead under the deck (`hw` its half
/// width) lying across the way (`dir` along it).
pub fn pillar_column(g: &mut Geometry, at: Vec2, dir: Vec2, top: f32, foot: f32, hw: f32, concrete: [f32; 3]) {
    use glam::Vec3;
    if top - foot < 1.0 {
        return;
    }
    let yaw = dir.y.atan2(dir.x);
    let w = (hw * 0.35).clamp(1.2, 3.0);
    g.part(Surface::Concrete).boxed(Vec3::new(at.x, (top + foot) * 0.5, at.y), Vec3::new(1.6, top - foot, w), yaw, rgba(mul(concrete, 0.8)));
    // The footing, just showing above the ground.
    g.part(Surface::Concrete).boxed(Vec3::new(at.x, foot + 0.15, at.y), Vec3::new(2.6, 0.3, w + 1.0), yaw, rgba(mul(concrete, 0.7)));
    // A crosshead under the deck.
    g.part(Surface::Concrete).boxed(Vec3::new(at.x, top - 0.35, at.y), Vec3::new(1.9, 0.7, hw * 1.7), yaw, rgba(mul(concrete, 0.85)));
}

/// What shows of the tunnels: a portal frame where each tunnel comes out
/// of the ground, retaining walls along the open cutting of its ramp, and
/// a ventilation shaft head on the surface every 150 m or so along it.
pub fn tunnel_works(world: &World, g: &mut Geometry, concrete: [f32; 3], steel: [f32; 3]) {
    use glam::Vec3;
    let roads = &world.roads;
    let z_of = |n: nd_sim::NodeId| world.road_z.get(n.0 as usize).copied().unwrap_or(0.0);
    let wall = rgba(mul(concrete, 0.75));
    let coping = rgba(mul(concrete, 0.6));
    for (sid, seg) in roads.alive_segments() {
        let (a, b) = roads.segment_points(sid);
        let (za, zb) = (z_of(seg.a), z_of(seg.b));
        let hw = seg.half_width() + if seg.is_street() { nd_sim::zoning::SIDEWALK } else { 0.6 };
        let len = a.distance(b);
        if len < 0.5 {
            continue;
        }
        let dir = (b - a) / len;
        let side = Vec2::new(-dir.y, dir.x);
        let yaw = dir.y.atan2(dir.x);
        if seg.tunnel {
            // Portals: where this tunnel piece meets a way that is not one.
            for (n, z) in [(seg.a, za), (seg.b, zb)] {
                let Some(o) = roads.node(n).segments.iter().copied().find(|&o| o != sid && roads.segment(o).alive && !roads.segment(o).tunnel) else { continue };
                let p = roads.node(n).pos;
                // Square to the ramp coming down to it (the bore may turn
                // at the portal).
                let os = roads.segment(o);
                let far = roads.node(if os.a == n { os.b } else { os.a }).pos;
                let out = (far - p).normalize_or_zero();
                // The bore has no pavements: the arch spans its walls.
                let behind = p - out * (PORTAL_HOLD * 0.5);
                let hill = world.landscape.height(behind).max(if world.landscape.is_flat() { 0.0 } else { BANK_HEIGHT });
                portal_arch(g, p, out, z, seg.half_width() + crate::street::TUNNEL_WALL, hill, wall, coping, rgba(mul(steel, 2.2)));
            }
            // Vent shafts on the surface along the bore.
            let vents = (len / 150.0).floor() as i32;
            for k in 1..=vents {
                let p = a.lerp(b, k as f32 / (vents + 1) as f32);
                // Not on a way crossing over the bore: the shaft head stood
                // in the lanes of the street above it.
                let on_way = roads.segments_in_box(p - Vec2::splat(30.0), p + Vec2::splat(30.0)).any(|o| {
                    let os = roads.segment(o);
                    if o == sid || os.tunnel || !os.alive {
                        return false;
                    }
                    let (oa, ob) = roads.segment_points(o);
                    let reach = os.half_width() + if os.is_street() && !os.fronts_nothing() { nd_sim::zoning::SIDEWALK } else { 0.5 } + 2.2;
                    nd_sim::roads::project_on_segment(p, oa, ob).1.distance(p) < reach
                });
                if on_way {
                    continue;
                }
                let gz = world.ground_height(p);
                g.part(Surface::Concrete).boxed(Vec3::new(p.x, gz + 1.2, p.y), Vec3::new(3.0, 2.4, 3.0), yaw, wall);
                g.part(Surface::Metal).boxed(Vec3::new(p.x, gz + 2.5, p.y), Vec3::new(2.6, 0.25, 2.6), yaw, rgba(mul(steel, 0.6)));
            }
            continue;
        }
        // Open cutting: retaining walls where the way runs well below the
        // ground beside it (a portal ramp, a sunken road).
        let steps = (len / 4.0).ceil().max(1.0) as usize;
        for k in 0..steps {
            let (t0, t1) = (k as f32 / steps as f32, (k + 1) as f32 / steps as f32);
            let (p0, p1) = (a.lerp(b, t0), a.lerp(b, t1));
            let (r0, r1) = (za + (zb - za) * t0, za + (zb - za) * t1);
            for sgn in [-1.0f32, 1.0] {
                let o = side * sgn * (hw + nd_sim::landscape::GROUND_CELL - 0.6);
                let probe = side * sgn * (hw + nd_sim::landscape::GROUND_CELL + 4.0);
                let (g0, g1) = (world.ground_height(p0 + probe), world.ground_height(p1 + probe));
                if g0 - r0 < 1.2 && g1 - r1 < 1.2 {
                    continue;
                }
                let (q0, q1) = (p0 + o, p1 + o);
                // [sweep-visual] Not across another way: at a junction on a
                // slope the wall along one arm ran on over the next arm's
                // lanes and pavements, a concrete band across the road.
                if [q0, q1, (q0 + q1) * 0.5].iter().any(|q| on_other_way(roads, sid, *q)) {
                    continue;
                }
                let (t0, t1) = (g0.max(r0) + 0.6, g1.max(r1) + 0.6);
                let out = Vec3::new(side.x * sgn, 0.0, side.y * sgn);
                let m = g.part(Surface::Concrete);
                m.quad_facing([Vec3::new(q0.x, r0 - 0.5, q0.y), Vec3::new(q1.x, r1 - 0.5, q1.y), Vec3::new(q1.x, t1, q1.y), Vec3::new(q0.x, t0, q0.y)], -out, wall);
                let (b0, b1) = (q0 + o.normalize_or_zero() * 0.4, q1 + o.normalize_or_zero() * 0.4);
                m.quad_facing([Vec3::new(q0.x, t0, q0.y), Vec3::new(q1.x, t1, q1.y), Vec3::new(b1.x, t1, b1.y), Vec3::new(b0.x, t0, b0.y)], Vec3::Y, coping);
            }
        }
    }
}

/// [sweep-visual] Does `p` stand on a way other than `sid` (its lanes, its
/// pavements, a little margin): no cutting wall goes there.
fn on_other_way(roads: &nd_sim::roads::RoadNetwork, sid: nd_sim::roads::SegmentId, p: Vec2) -> bool {
    roads.segments_in_box(p - Vec2::splat(25.0), p + Vec2::splat(25.0)).any(|o| {
        let os = roads.segment(o);
        if o == sid || !os.alive || os.tunnel {
            return false;
        }
        let (oa, ob) = roads.segment_points(o);
        let reach = os.half_width() + if os.is_street() && !os.fronts_nothing() { nd_sim::zoning::SIDEWALK } else { 0.6 } + 0.5;
        nd_sim::roads::project_on_segment(p, oa, ob).1.distance(p) < reach
    })
}

/// A tunnel mouth: a concrete headwall square across the way at `p`, facing
/// `out` (towards the open ramp), with an elliptical arch over the opening,
/// the arch's soffit run back through the wall's thickness, a coping along
/// the top and a lit band under the crown. The opening clears the
/// carriageway and its shoulders (`hw` either side); the bore's walls stand
/// behind the dark mouth.
/// Two posts and a lintel box stood here before and read as a blob.
#[allow(clippy::too_many_arguments)]
fn portal_arch(g: &mut Geometry, p: Vec2, out: Vec2, z: f32, hw: f32, hill: f32, wall: [f32; 4], coping: [f32; 4], lamp: [f32; 4]) {
    use glam::Vec3;
    if out == Vec2::ZERO {
        return;
    }
    let side = Vec2::new(-out.y, out.x);
    // Opening: half width, spring line and rise; the wall around it.
    let r = hw + 0.2;
    // Low enough that a tunnel drawn at the Tunnel preset's 8 m keeps its
    // headwall and lid at about the hill's surface.
    let spring = 4.4;
    let rise = (r * 0.3).clamp(1.0, 1.6);
    let wall_half = r + 1.6;
    let wall_top = spring + rise + 0.9;
    let foot = -0.4;
    let thick = 0.9;
    // A point of the headwall at `x` across (right of `out` positive) and
    // `y` up, on the face `t` along `out` (front +, back -).
    let at = |x: f32, y: f32, t: f32| {
        let q = p + side * x + out * t;
        Vec3::new(q.x, z + y, q.y)
    };
    // The opening's outline from the right foot round the arch to the left
    // foot, and for each point the matching point of the wall's outline
    // (straight out sideways below the spring line, up through the arch's
    // centre to the edge or the top above it).
    let n = 14;
    let mut opening: Vec<(f32, f32)> = vec![(r, foot), (r, spring)];
    for k in 1..n {
        let a = std::f32::consts::PI * k as f32 / n as f32;
        opening.push((r * a.cos(), spring + rise * a.sin()));
    }
    opening.push((-r, spring));
    opening.push((-r, foot));
    let outline: Vec<(f32, f32)> = opening
        .iter()
        .map(|&(x, y)| {
            if y <= spring + 1e-3 {
                return (x.signum() * wall_half, y);
            }
            // Along the ray from the arch's centre on the spring line.
            let (dx, dy) = (x, y - spring);
            let to_side = if dx.abs() > 1e-4 { wall_half / dx.abs() } else { f32::MAX };
            let to_top = (wall_top - spring) / dy;
            let k = to_side.min(to_top);
            (dx * k, spring + dy * k)
        })
        .collect();
    let o3 = Vec3::new(out.x, 0.0, out.y);
    let m = g.part(Surface::Concrete);
    for w in 0..opening.len() - 1 {
        let (a, b) = (opening[w], opening[w + 1]);
        let (c, d) = (outline[w], outline[w + 1]);
        for (t, face) in [(thick * 0.5, o3), (-thick * 0.5, -o3)] {
            m.quad_facing([at(a.0, a.1, t), at(b.0, b.1, t), at(d.0, d.1, t), at(c.0, c.1, t)], face, wall);
        }
        // The soffit (and the jambs), facing into the opening.
        let mid = Vec2::new((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
        let inward = Vec2::new(-mid.x, if mid.y > spring { spring - mid.y } else { 0.0 }).normalize_or_zero();
        let inward3 = Vec3::new(side.x * inward.x, inward.y, side.y * inward.x);
        m.quad_facing([at(a.0, a.1, thick * 0.5), at(b.0, b.1, thick * 0.5), at(b.0, b.1, -thick * 0.5), at(a.0, a.1, -thick * 0.5)], inward3, wall);
    }
    // The wall's ends and top.
    for sgn in [-1.0f32, 1.0] {
        let x = sgn * wall_half;
        let s3 = Vec3::new(side.x * sgn, 0.0, side.y * sgn);
        m.quad_facing([at(x, foot, thick * 0.5), at(x, wall_top, thick * 0.5), at(x, wall_top, -thick * 0.5), at(x, foot, -thick * 0.5)], s3, wall);
    }
    // A coping a little proud of the wall, along the top.
    let c = p;
    let yaw = out.y.atan2(out.x);
    m.boxed(Vec3::new(c.x, z + wall_top + 0.2, c.y), Vec3::new(thick + 0.4, 0.4, wall_half * 2.0 + 0.4), yaw, coping);
    // A lit band under the crown, just inside the mouth.
    let lit = p - out * (thick * 0.5 + 0.3);
    g.part(Surface::Glow).boxed(Vec3::new(lit.x, z + spring + rise - 0.35, lit.y), Vec3::new(0.08, 0.2, r * 1.2), yaw, lamp);
    // The dark of the bore, across the opening just behind the wall: the
    // built ground is a height field and cannot overhang, so through the
    // arch one saw the hillside the tunnel runs into. The ground is held
    // at the road's level for a few metres in (`Ground::build`), out of
    // sight behind this, and a lid over that stretch, at the wall's top,
    // closes it from above.
    let back = -thick * 0.5 - 0.05;
    let dark = crate::buildings::parts::rgba(crate::buildings::parts::DARK);
    g.part(Surface::Matte).quad_facing([at(-r - 0.1, foot, back), at(r + 0.1, foot, back), at(r + 0.1, spring + rise + 0.1, back), at(-r - 0.1, spring + rise + 0.1, back)], o3, dark);
    // The lid lies at the hill's surface (`hill`, the natural ground over
    // the bore), never lower than the arch nor higher than the wall, and
    // reaches a cell past the held ground, over the slope up to the hill.
    let reach = PORTAL_HOLD + nd_sim::landscape::GROUND_CELL;
    let lid = p - out * (thick * 0.5 + reach * 0.5);
    let top = crate::mesh::clamp_safe(hill - z, spring + rise + 0.4, wall_top);
    g.part(Surface::Concrete).boxed(Vec3::new(lid.x, z + top - 0.3, lid.y), Vec3::new(reach, 0.6, wall_half * 2.0 + 2.0), yaw, coping);
}

/// How far into the hill behind a tunnel portal the built ground is held at
/// the road's level (`nd_sim::landscape::PORTAL_HOLD`), lidded by the portal.
const PORTAL_HOLD: f32 = nd_sim::landscape::PORTAL_HOLD;

/// One span of viaduct deck from `a` at height `za` to `b` at `zb`,
/// `hw` either side of the centreline, standing on `ground`. Shared with
/// the plan preview.
#[allow(clippy::too_many_arguments)]
/// [interchange] A bridge's abutment on the bank at `p`, the deck running
/// out over the water along `out` at height `z` (`hw` its half width): a
/// bank seat under the deck's end and wing walls running back along the
/// approach, down into the ground at `ground`.
pub fn abutment(g: &mut Geometry, p: Vec2, out: Vec2, z: f32, hw: f32, ground: f32, concrete: [f32; 3]) {
    use glam::Vec3;
    let top = z - 1.4;
    let foot = ground - 1.0;
    if top - foot < 0.6 {
        return;
    }
    let yaw = out.y.atan2(out.x);
    // The seat: from just behind the deck's end to under it.
    let seat = p + out * 0.6;
    g.part(Surface::Concrete).boxed(Vec3::new(seat.x, (top + foot) * 0.5, seat.y), Vec3::new(2.4, top - foot, hw * 2.0 + 0.8), yaw, rgba(mul(concrete, 0.82)));
    // The wing walls, stepping down as they run back into the embankment.
    let side = Vec2::new(-out.y, out.x);
    for sgn in [-1.0f32, 1.0] {
        for k in 0..3 {
            let back = 1.5 + k as f32 * 2.5;
            let c = p - out * back + side * sgn * (hw + 0.25);
            let h = (z - 0.1 - foot) * (1.0 - k as f32 * 0.3);
            if h < 0.3 {
                continue;
            }
            g.part(Surface::Concrete).boxed(Vec3::new(c.x, foot + h * 0.5, c.y), Vec3::new(2.5, h, 0.5), yaw, rgba(mul(concrete, 0.86)));
        }
    }
}

pub fn viaduct_span(g: &mut Geometry, a: Vec2, b: Vec2, za: f32, zb: f32, hw: f32, ground: &dyn Fn(Vec2) -> f32, concrete: [f32; 3], steel: [f32; 3], columns: bool) {
    let parapets = true;
    use glam::Vec3;
    let len = a.distance(b);
    if len < 0.3 {
        return;
    }
    let dir = (b - a) / len;
    let a3 = Vec3::new(a.x, za - 0.02, a.y);
    let b3 = Vec3::new(b.x, zb - 0.02, b.y);
    g.part(Surface::Concrete).slab(a3, b3, hw * 2.0, 1.2, rgba(concrete));
    if parapets {
        let side = Vec2::new(-dir.y, dir.x) * (hw - 0.2);
        for s in [1.0f32, -1.0] {
            let off = Vec3::new(side.x * s, 0.0, side.y * s);
            g.part(Surface::Metal).beam(a3 + off + Vec3::Y * 1.05, b3 + off + Vec3::Y * 1.05, 0.12, rgba(steel));
            g.part(Surface::Concrete).slab(a3 + off + Vec3::Y * 0.5, b3 + off + Vec3::Y * 0.5, 0.3, 0.55, rgba(mul(concrete, 0.9)));
        }
    }
    // Columns where the deck stands clear of the ground, one at each end
    // too unless the end is near the ground (a ramp foot): only for a deck
    // whose columns were not placed for it (`World::pillars`).
    if !columns {
        return;
    }
    let spans = (len / 24.0).ceil().max(1.0) as i32;
    let yaw = dir.y.atan2(dir.x);
    for k in 0..=spans {
        let t = k as f32 / spans as f32;
        let p = a.lerp(b, t);
        let top = za + (zb - za) * t - 1.2;
        let foot = ground(p);
        if top - foot < 1.0 {
            continue;
        }
        let w = (hw * 0.35).clamp(1.2, 3.0);
        g.part(Surface::Concrete).boxed(Vec3::new(p.x, (top + foot) * 0.5, p.y), Vec3::new(1.6, top - foot, w), yaw, rgba(mul(concrete, 0.8)));
        // A crosshead under the deck.
        g.part(Surface::Concrete).boxed(Vec3::new(p.x, top - 0.35, p.y), Vec3::new(1.9, 0.7, hw * 1.7), yaw, rgba(mul(concrete, 0.85)));
    }
}

/// Everything built in or over the water, and the hot ground: bridge decks
/// with railings and piers, dock piers with piles, bollards and the crane
/// boom, weirs where hydro plants take their water, tidal turbine
/// caissons, and steaming vents at the hot spots.
pub fn waterworks(world: &World, palette: &Palette) -> Geometry {
    use glam::Vec3;
    let mut g = Geometry::default();
    let land = &world.landscape;
    let concrete = mul(palette.concrete(0), 0.85);
    let steel = mul(palette.steel, 0.8);
    if land.is_flat() {
        // The flat map has no water: only what the player built up.
        if world.relief {
            viaducts(world, &mut g, concrete, steel);
            underpasses(world, &mut g, concrete, steel);
            tunnel_works(world, &mut g, concrete, steel);
        }
        return g;
    }
    viaducts(world, &mut g, concrete, steel);
    tunnel_works(world, &mut g, concrete, steel);
    let roads = &world.roads;
    for (sid, seg) in roads.alive_segments() {
        if !seg.bridge || seg.viaduct {
            continue;
        }
        let (a, b) = roads.segment_points(sid);
        let za = world.road_z.get(seg.a.0 as usize).copied().unwrap_or(0.0);
        let zb = world.road_z.get(seg.b.0 as usize).copied().unwrap_or(0.0);
        // The deck carries the pavements (a highway's or ramp's, its
        // shoulders and barriers), with the parapet at its outer edge, as a
        // viaduct's: it was half a pavement narrower, so the parapet's rail
        // ran down the middle of the pavement.
        let motorway = seg.is_motorway() || seg.ring;
        let hw = seg.half_width() + if seg.is_street() && !motorway { nd_sim::zoning::SIDEWALK } else { 0.6 };
        let a3 = Vec3::new(a.x, za - 0.05, a.y);
        let b3 = Vec3::new(b.x, zb - 0.05, b.y);
        g.part(Surface::Concrete).slab(a3, b3, hw * 2.0, 1.4, rgba(concrete));
        let dir = (b - a).normalize_or_zero();
        let side = Vec2::new(-dir.y, dir.x) * (hw - 0.2);
        for s in [1.0f32, -1.0] {
            let off = Vec3::new(side.x * s, 0.0, side.y * s);
            g.part(Surface::Metal).beam(a3 + off + Vec3::Y * 1.0, b3 + off + Vec3::Y * 1.0, 0.15, rgba(steel));
            g.part(Surface::Concrete).slab(a3 + off + Vec3::Y * 0.45, b3 + off + Vec3::Y * 0.45, 0.35, 0.5, rgba(mul(concrete, 0.9)));
        }
        // [interchange] An abutment at each end on the bank (no other bridge
        // piece goes on from it): a bank seat under the deck's end and wing
        // walls back along the approach into the embankment. The deck used
        // to simply stop, a concrete block standing on the bank.
        for (n, p, out, z) in [(seg.a, a, dir, za), (seg.b, b, -dir, zb)] {
            let goes_on = roads.node(n).segments.iter().any(|&o| o != sid && roads.segment(o).alive && roads.segment(o).bridge && !roads.segment(o).viaduct);
            if !goes_on {
                abutment(&mut g, p, out, z, hw, land.height(p).max(WATER_LEVEL - 1.0), concrete);
            }
        }
        // [interchange] A ramp leaving (or joining) a highway on the bridge:
        // deck under the gore between the two, which used to be a thin
        // asphalt sheet over the water.
        let seg_len = a.distance(b);
        if seg.ramp {
            for (n, p, out) in [(seg.a, a, dir), (seg.b, b, -dir)] {
                let hw_seg = roads.node(n).segments.iter().copied().find(|&o| o != sid && roads.segment(o).alive && roads.segment(o).is_highway() && roads.segment(o).bridge);
                let Some(h) = hw_seg else { continue };
                let (h0, h1) = roads.segment_points(h);
                let hd = if h0.distance(p) < h1.distance(p) { (h1 - h0).normalize_or_zero() } else { (h0 - h1).normalize_or_zero() };
                let run = seg_len.min(40.0);
                let z0 = world.road_z.get(n.0 as usize).copied().unwrap_or(0.0);
                let z1 = if n == seg.a { za + (zb - za) * (run / seg_len.max(1e-3)) } else { zb + (za - zb) * (run / seg_len.max(1e-3)) };
                let mut s = 0.0f32;
                while s < run {
                    let r = p + out * (s + 2.0);
                    let q = p + hd * (r - p).dot(hd);
                    let gap = r.distance(q);
                    if gap > 0.5 {
                        let mid = (r + q) * 0.5;
                        let zz = z0 + (z1 - z0) * (s + 2.0) / run.max(1e-3) - 0.05;
                        let yaw = hd.y.atan2(hd.x);
                        g.part(Surface::Concrete).boxed(Vec3::new(mid.x, zz - 0.7, mid.y), Vec3::new(4.2, 1.4, gap), yaw, rgba(concrete));
                    }
                    s += 4.0;
                }
            }
        }
        // Piers down to the bed every 28 m or so.
        let len = a.distance(b);
        let spans = (len / 28.0).ceil().max(1.0) as i32;
        let yaw = dir.y.atan2(dir.x);
        for k in 1..spans {
            let t = k as f32 / spans as f32;
            let p = a.lerp(b, t);
            let top = za + (zb - za) * t - 1.4;
            let bed = land.height(p).min(WATER_LEVEL - 1.0);
            g.part(Surface::Concrete).boxed(Vec3::new(p.x, (top + bed) * 0.5, p.y), Vec3::new(2.2, top - bed, hw * 1.6), yaw, rgba(mul(concrete, 0.8)));
        }
    }
    underpasses(world, &mut g, concrete, steel);
    // Dock piers.
    let mut boats: Option<[MeshData; 3]> = None;
    for port in world.ports.ports.values() {
        let (_, width) = port.kind.pier();
        let [root, tip] = port.pier;
        // [economy] A marina's pontoons float just off the water.
        let floating = port.kind.is_marina();
        let deck = if floating { WATER_LEVEL + PONTOON_DECK } else { BANK_HEIGHT + 0.6 };
        let r3 = Vec3::new(root.x, deck, root.y);
        let t3 = Vec3::new(tip.x, deck, tip.y);
        if floating {
            g.part(Surface::Wood).slab(r3, t3, width, 0.6, rgba(mul(palette.wood, 0.85)));
        } else {
            g.part(Surface::Concrete).slab(r3, t3, width, 0.8, rgba(mul(concrete, 0.95)));
        }
        let along = (tip - root).normalize_or_zero();
        let across = Vec2::new(-along.y, along.x);
        let len = root.distance(tip);
        let piles = (len / 6.0) as i32;
        for k in 0..=piles {
            let p = root + along * (k as f32 * 6.0);
            for s in [-0.45f32, 0.45] {
                let q = p + across * width * s;
                g.part(Surface::Wood).cylinder(q, 0.35, 0.35, WATER_LEVEL - 3.0, deck - 0.6, 6, rgba(mul(palette.wood, 0.6)));
            }
            if k % 2 == 0 {
                for s in [-0.5f32, 0.5] {
                    let q = p + across * (width * s - s.signum() * 0.5);
                    g.part(Surface::Metal).cylinder(q, 0.25, 0.3, deck, deck + 0.6, 6, rgba(mul(palette.steel, 0.4)));
                }
            }
        }
        match port.kind {
            PortKind::ShippingPort => {
                // The crane's boom reaching out over the berth.
                let base = port.center + port.seaward * (port.kind.size().1 * 0.5 - 9.0) + port.along * (port.kind.size().0 * 0.12);
                let out = base + port.seaward * (len + 30.0);
                g.part(Surface::Metal).beam(Vec3::new(base.x, 31.0, base.y), Vec3::new(out.x, 31.0, out.y), 1.4, rgba(palette.metal_paint(0)));
                g.part(Surface::Glow).boxed(Vec3::new(out.x, 31.8, out.y), Vec3::splat(0.6), 0.0, rgba(palette.aviation_light));
            }
            PortKind::FerryTerminal => {
                // A floating pontoon at the tip with a lamp.
                let p = tip + along * 4.0;
                g.part(Surface::Metal).boxed(Vec3::new(p.x, WATER_LEVEL + 0.5, p.y), Vec3::new(8.0, 1.0, 5.0), along.y.atan2(along.x), rgba(mul(palette.metal_paint(2), 0.8)));
                g.part(Surface::Metal).cylinder(p, 0.08, 0.08, WATER_LEVEL + 1.0, WATER_LEVEL + 5.0, 6, rgba([0.2, 0.2, 0.22]));
                g.part(Surface::Glow).boxed(Vec3::new(p.x, WATER_LEVEL + 5.1, p.y), Vec3::new(0.4, 0.25, 0.4), 0.0, rgba(palette.lamp));
            }
            // [economy] Pontoons and quays with their boats moored.
            PortKind::MarinaSmall | PortKind::MarinaLarge | PortKind::FishingPortSmall | PortKind::FishingPortLarge => {
                let meshes = boats.get_or_insert_with(|| [boat_mesh(ShipKind::Sailboat, palette), boat_mesh(ShipKind::Yacht, palette), boat_mesh(ShipKind::FishingBoat, palette)]);
                let layout = dock_layout(port, land);
                let top = if floating { WATER_LEVEL + PONTOON_DECK } else { deck };
                for (a, b, w) in &layout.decks {
                    let (a3, b3) = (Vec3::new(a.x, top, a.y), Vec3::new(b.x, top, b.y));
                    if floating {
                        g.part(Surface::Wood).slab(a3, b3, *w, 0.6, rgba(mul(palette.wood, 0.9)));
                    } else {
                        g.part(Surface::Concrete).slab(a3, b3, *w, 0.8, rgba(mul(concrete, 0.95)));
                        // Piles under the quay.
                        let n = (a.distance(*b) / 6.0) as i32;
                        for k in 0..=n {
                            let q = a.lerp(*b, k as f32 / n.max(1) as f32);
                            g.part(Surface::Wood).cylinder(q, 0.35, 0.35, WATER_LEVEL - 3.0, deck - 0.6, 6, rgba(mul(palette.wood, 0.6)));
                        }
                    }
                }
                // A lamp at the pier head.
                let lamp = tip - along * 1.0 + across * (width * 0.5 - 0.4);
                g.part(Surface::Matte).cylinder(lamp, 0.07, 0.07, top, top + 4.0, 6, rgba([0.2, 0.2, 0.22]));
                g.part(Surface::Glow).boxed(Vec3::new(lamp.x, top + 4.1, lamp.y), Vec3::new(0.35, 0.22, 0.35), 0.0, rgba(palette.lamp));
                for m in &layout.boats {
                    // [fishing] Trawlers are drawn by the app where the
                    // sim has them: at their berth or out at sea.
                    if m.kind == ShipKind::FishingBoat {
                        continue;
                    }
                    let mesh = match m.kind {
                        ShipKind::Yacht => &meshes[1],
                        ShipKind::FishingBoat => &meshes[2],
                        _ => &meshes[0],
                    };
                    stamp_mesh(g.part(Surface::Panel), mesh, Vec3::new(m.pos.x, WATER_LEVEL, m.pos.y), m.yaw);
                }
            }
        }
    }
    // Plants that work the water.
    for b in world.buildings.values() {
        let Some(kind) = b.plant else { continue };
        let Some(lot) = world.lots.get(b.lot) else { continue };
        match kind {
            PowerPlantKind::Hydro => {
                // [plant-groups] A curved arch dam across the river beside the plant.
                if let Some((ri, _, bank)) = land.river_beside(&lot.polygon, 40.0) {
                    let r = &land.rivers[ri];
                    let (seg, c, _) = r.nearest(bank);
                    crate::dam::hydro_dam(&mut g, c, r.flow(seg), r.width, lot.center, world.plant_scale(b), &|p| world.ground_height(p), concrete, steel);
                }
            }
            PowerPlantKind::Tidal => {
                if let Some(shore) = lot.polygon.iter().filter_map(|p| land.nearest_shore(*p, 50.0)).find(|s| s.water.kind == WaterKind::Sea) {
                    // [freight] A turbine caisson per turbine bay and two
                    // more out at the head (five at the nameplate): a bigger
                    // plant runs further out to sea, the caissons the same size.
                    let caissons = PowerPlantKind::turbine_bays(world.plant_scale(b)) + 2;
                    for k in 0..caissons {
                        let p = shore.point + shore.seaward * (12.0 + k as f32 * 11.0);
                        g.part(Surface::Concrete).boxed(Vec3::new(p.x, WATER_LEVEL, p.y), Vec3::new(7.0, 4.0, 7.0), 0.0, rgba(concrete));
                        g.part(Surface::Metal).boxed(Vec3::new(p.x, WATER_LEVEL + 2.8, p.y), Vec3::new(2.0, 1.6, 2.0), 0.0, rgba(mul(palette.metal_paint(3), 0.9)));
                    }
                    let a = shore.point - shore.seaward * 2.0;
                    let b = shore.point + shore.seaward * (16.0 + (caissons - 1) as f32 * 11.0);
                    g.part(Surface::Concrete).slab(Vec3::new(a.x, BANK_HEIGHT + 0.8, a.y), Vec3::new(b.x, BANK_HEIGHT + 0.8, b.y), 3.0, 0.6, rgba(mul(concrete, 0.9)));
                }
            }
            _ => {}
        }
    }
    // Vents at the hot spots: crusted rocks round a pale sinter ring, on
    // open ground only (the city may have been laid over a hot spot). The
    // steam itself is animated by the renderer.
    for (k, s) in land.hot_spots.iter().enumerate() {
        let mut rng = rng_for(land.params.seed, 900 + k as u64);
        for _ in 0..7 {
            let p = s.pos + Vec2::from_angle(rng.random_range(0.0..6.28)) * rng.random_range(0.0..s.radius * 0.8);
            let size = 0.8 + rng.random::<f32>();
            if !climate::vent_clear(world, p) {
                continue;
            }
            let y = world.ground_height(p);
            rock_world(&mut g, &mut rng, p, y - 0.1, size, mul(palette.stone, 0.7));
            g.part(Surface::Stone).ground_polygon(&ngon(p.x, p.y, size * 2.2, 9, 0.0), y + 0.06, rgba([0.62, 0.58, 0.46]));
        }
    }
    g
}

// ── [economy] waterfront docks and boats ────────────────────────────────────

/// Height of a marina's floating pontoon decks above the water.
pub const PONTOON_DECK: f32 = 0.45;

/// A boat tied up at a dock: where (world x, z), which way its bow points
/// (radians, `atan2` of the bow direction, the convention of `Ship` poses)
/// and what kind of boat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mooring {
    pub pos: Vec2,
    pub yaw: f32,
    pub kind: ShipKind,
}

/// The water side of a marina or fishing port beyond its main pier: extra
/// deck pieces (finger pontoons, a T-head quay), each `(from, to, width)`,
/// and the boats moored at them.
#[derive(Clone, Debug, Default)]
pub struct DockLayout {
    pub decks: Vec<(Vec2, Vec2, f32)>,
    pub boats: Vec<Mooring>,
}

/// Length and beam of a boat kind, metres (matches [`boat_mesh`]).
pub fn boat_size(kind: ShipKind) -> (f32, f32) {
    match kind {
        ShipKind::Freighter => (70.0, 12.0),
        ShipKind::Ferry => (28.0, 8.0),
        ShipKind::FishingBoat => (16.0, 5.0),
        ShipKind::Yacht => (22.0, 6.0),
        ShipKind::Sailboat => (9.0, 3.0),
    }
}

/// Where the boats of a marina or fishing port lie at their berths, up to
/// `PortKind::berths()` of them (fewer where the water is too narrow or
/// the pier too short): small boats on finger pontoons off a marina's
/// pontoon (yachts too, on longer fingers, at a large marina), trawlers
/// alongside a fishing port's quay and its T-head. Deterministic from the
/// port. Other kinds of dock have none.
pub fn moorings(port: &nd_sim::ports::Port, land: &nd_sim::landscape::Landscape) -> Vec<Mooring> {
    dock_layout(port, land).boats
}

/// The decks and moorings of a waterfront dock (see [`moorings`]).
pub fn dock_layout(port: &nd_sim::ports::Port, land: &nd_sim::landscape::Landscape) -> DockLayout {
    let mut out = DockLayout::default();
    let kind = port.kind;
    if !(kind.is_marina() || kind.is_fishing()) {
        return out;
    }
    let berths = kind.berths() as usize;
    let (_, width) = kind.pier();
    let [root, tip] = port.pier;
    let len = root.distance(tip);
    if len < 2.0 {
        return out;
    }
    let along = (tip - root) / len;
    let across = Vec2::new(-along.y, along.x);
    // Where the pier leaves the bank.
    let mut s_water = 0.0;
    while s_water < len && !land.is_water(root + along * s_water) {
        s_water += 1.0;
    }
    // A boat fits where its middle, ends and sides are all on water.
    let afloat = |p: Vec2, dir: Vec2, k: ShipKind| -> bool {
        let (l, b) = boat_size(k);
        let n = Vec2::new(-dir.y, dir.x);
        [p, p + dir * l * 0.5, p - dir * l * 0.5, p + n * b * 0.5, p - n * b * 0.5].iter().all(|q| land.is_water(*q))
    };
    let yaw_of = |d: Vec2| d.y.atan2(d.x);
    if kind.is_marina() {
        // Finger pontoons both sides of the main pontoon, boats bow-in
        // between them; the large marina puts a yacht in every third pair.
        let mut s = s_water + 1.5;
        let mut pair = 0;
        while out.boats.len() < berths && pair < 40 {
            let k = if kind == nd_sim::ports::PortKind::MarinaLarge && pair % 3 == 0 { ShipKind::Yacht } else { ShipKind::Sailboat };
            let (l, b) = boat_size(k);
            let spacing = b + 1.6;
            let mid = s + spacing * 0.5;
            if mid + spacing * 0.5 > len {
                break;
            }
            for side in [1.0f32, -1.0] {
                if out.boats.len() >= berths {
                    break;
                }
                let base = root + along * mid;
                let p = base + across * side * (width * 0.5 + l * 0.5 + 0.4);
                if afloat(p, across, k) {
                    out.boats.push(Mooring { pos: p, yaw: yaw_of(-across * side), kind: k });
                    // The finger beside it.
                    let fs = root + along * (mid + spacing * 0.5);
                    let a = fs + across * side * width * 0.5;
                    let fb = a + across * side * (l * 0.8);
                    if land.is_water(fb) {
                        out.decks.push((a, fb, 1.0));
                    }
                }
            }
            s += spacing;
            pair += 1;
        }
        // Any berths left: a T-head across the tip, boats along its far side.
        if out.boats.len() < berths {
            let head = 24.0;
            out.decks.push((tip - across * head * 0.5, tip + across * head * 0.5, width));
            let (l, b) = boat_size(ShipKind::Sailboat);
            let mut x = -head * 0.5 + l * 0.5;
            while x + l * 0.5 <= head * 0.5 && out.boats.len() < berths {
                let p = tip + along * (width * 0.5 + b * 0.5 + 0.4) + across * x;
                if afloat(p, across, ShipKind::Sailboat) {
                    out.boats.push(Mooring { pos: p, yaw: yaw_of(across), kind: ShipKind::Sailboat });
                }
                x += l + 1.0;
            }
        }
    } else {
        // A quay with a T-head; the trawlers' berths are the sim's
        // (`nd_sim::fishing::quay_moorings`), so a boat out at sea leaves
        // its own berth empty.
        let head = nd_sim::fishing::quay_head(kind);
        out.decks.push((tip - across * head * 0.5, tip + across * head * 0.5, width));
        out.boats = nd_sim::fishing::quay_moorings(port, land).into_iter().map(|(pos, yaw)| Mooring { pos, yaw, kind: ShipKind::FishingBoat }).collect();
    }
    out
}

/// Copy `src` into `dst` turned by `yaw` about the vertical (its +x along
/// `(cos yaw, sin yaw)` in world x, z) and moved to `at`.
pub fn stamp_mesh(dst: &mut MeshData, src: &MeshData, at: glam::Vec3, yaw: f32) {
    let (s, c) = yaw.sin_cos();
    let rot = |p: [f32; 3]| -> [f32; 3] { [p[0] * c - p[2] * s, p[1], p[0] * s + p[2] * c] };
    let base = dst.positions.len() as u32;
    for (i, p) in src.positions.iter().enumerate() {
        let r = rot(*p);
        dst.positions.push([r[0] + at.x, r[1] + at.y, r[2] + at.z]);
        dst.normals.push(rot(src.normals[i]));
        dst.colors.push(src.colors.get(i).copied().unwrap_or([1.0; 4]));
        dst.uvs.push(src.uvs.get(i).copied().unwrap_or([0.0; 2]));
        let t = src.tangents.get(i).copied().unwrap_or([1.0, 0.0, 0.0, 1.0]);
        let tr = rot([t[0], t[1], t[2]]);
        dst.tangents.push([tr[0], tr[1], tr[2], t[3]]);
    }
    dst.indices.extend(src.indices.iter().map(|i| i + base));
}

/// A hull from keel to gunwale with a pointed bow, bow along +x, and a
/// paler deck: `len` long, `beam` wide, from `y0` to `y1`.
fn hull(m: &mut MeshData, len: f32, beam: f32, bow: f32, y0: f32, y1: f32, color: [f32; 3], deck: [f32; 3]) {
    use glam::Vec3;
    let half = len * 0.5;
    let ring = |hb: f32, back: f32, y: f32| vec![Vec3::new(-half + back, y, -hb), Vec3::new(half - bow, y, -hb), Vec3::new(half, y, 0.0), Vec3::new(half - bow, y, hb), Vec3::new(-half + back, y, hb)];
    let bottom = ring(beam * 0.32, len * 0.06, y0);
    let top = ring(beam * 0.5, 0.0, y1);
    m.frustum(&bottom, &top, rgba(color));
    let d: Vec<Vec3> = ring(beam * 0.46, 0.25, y1 + 0.05);
    m.cap(&d, Vec3::Y, rgba(deck));
}

/// [fishing] A trawler, bow along +x, waterline at 0: wheelhouse forward,
/// a gantry over the stern. Steaming or in port its outrigger booms stand
/// up; with `nets_out` they are lowered out over the water either side and
/// the warps run from the gantry down into the sea astern.
pub fn trawler_mesh(nets_out: bool, palette: &Palette) -> MeshData {
    use glam::Vec3;
    let mut m = MeshData::default();
    let white = [0.92, 0.92, 0.9];
    let glass = palette.glass_dark;
    let (len, beam) = boat_size(ShipKind::FishingBoat);
    let red = [0.55, 0.13, 0.1];
    hull(&mut m, len, beam, 3.5, -1.2, 1.4, red, [0.45, 0.42, 0.38]);
    // White sheer strake along the top of the hull.
    m.boxed(Vec3::new(-1.5, 1.25, 0.0), Vec3::new(len - 5.0, 0.3, beam * 1.005), 0.0, rgba(white));
    // Wheelhouse forward, windows round it.
    m.boxed(Vec3::new(2.4, 2.75, 0.0), Vec3::new(3.6, 2.7, 3.4), 0.0, rgba(white));
    m.boxed(Vec3::new(2.4, 3.55, 0.0), Vec3::new(3.65, 0.65, 3.45), 0.0, rgba(glass));
    m.boxed(Vec3::new(2.4, 4.2, 0.0), Vec3::new(3.9, 0.15, 3.7), 0.0, rgba([0.3, 0.3, 0.32]));
    // Mast with a crosstree and the outrigger booms.
    let spar = rgba([0.8, 0.8, 0.78]);
    m.boxed(Vec3::new(3.2, 6.5, 0.0), Vec3::new(0.2, 4.6, 0.2), 0.0, spar);
    m.boxed(Vec3::new(3.2, 7.6, 0.0), Vec3::new(0.15, 0.15, 2.0), 0.0, spar);
    for s in [-1.0f32, 1.0] {
        if nets_out {
            // Lowered nearly flat, a line from each boom end to the water.
            let end = Vec3::new(0.0, 3.4, s * 9.5);
            m.beam(Vec3::new(3.0, 4.5, s * 0.3), end, 0.16, rgba([0.75, 0.75, 0.72]));
            m.beam(end, Vec3::new(-6.0, -0.2, s * 11.0), 0.06, rgba([0.25, 0.25, 0.25]));
        } else {
            m.beam(Vec3::new(3.0, 4.5, 0.0), Vec3::new(1.0, 6.5, s * 5.5), 0.14, rgba([0.75, 0.75, 0.72]));
        }
    }
    // Stern gantry (A-frame) and the net drum.
    let yellow = [0.9, 0.62, 0.1];
    for s in [-1.0f32, 1.0] {
        m.beam(Vec3::new(-6.6, 1.4, s * 2.0), Vec3::new(-6.0, 6.2, s * 1.6), 0.25, rgba(yellow));
    }
    m.boxed(Vec3::new(-6.0, 6.2, 0.0), Vec3::new(0.35, 0.35, 3.6), 0.0, rgba(yellow));
    if nets_out {
        // The drum paid out: warps from the gantry into the water astern,
        // and the net's floats strung out behind.
        for s in [-1.0f32, 1.0] {
            m.beam(Vec3::new(-6.0, 6.0, s * 1.5), Vec3::new(-26.0, -0.3, s * 4.0), 0.07, rgba([0.2, 0.2, 0.2]));
        }
        for k in 0..6 {
            let x = -27.0 - k as f32 * 2.2;
            let z = (k as f32 - 2.5) * 1.3;
            m.boxed(Vec3::new(x, 0.12, z), Vec3::new(0.7, 0.35, 0.7), 0.0, rgba([0.95, 0.45, 0.08]));
        }
        m.boxed(Vec3::new(-4.0, 1.85, 0.0), Vec3::new(1.6, 0.9, 3.0), 0.0, rgba([0.15, 0.35, 0.2]));
    } else {
        m.boxed(Vec3::new(-4.0, 2.1, 0.0), Vec3::new(1.6, 1.4, 3.0), 0.0, rgba([0.15, 0.35, 0.2]));
    }
    m.boxed(Vec3::new(-1.0, 1.75, 0.0), Vec3::new(1.4, 0.7, 1.4), 0.0, rgba([0.2, 0.4, 0.75]));
    m
}

/// [economy] The mesh of a boat of any kind, bow along +x, waterline at 0:
/// a trawler with its wheelhouse forward and a gantry over the stern, a
/// white motor yacht with stepped decks, or a small sailing boat with a
/// mast and sails up. Freighters and ferries are [`ship_mesh`]'s.
pub fn boat_mesh(kind: ShipKind, palette: &Palette) -> MeshData {
    use glam::Vec3;
    let mut m = MeshData::default();
    let white = [0.92, 0.92, 0.9];
    let glass = palette.glass_dark;
    match kind {
        ShipKind::Freighter => return ship_mesh(true, palette),
        ShipKind::Ferry => return ship_mesh(false, palette),
        ShipKind::FishingBoat => return trawler_mesh(false, palette),
        ShipKind::Yacht => {
            let (len, beam) = boat_size(kind);
            hull(&mut m, len, beam, beam * 1.4, -1.2, 2.0, white, [0.72, 0.6, 0.45]);
            // A dark stripe along the topsides, a swim platform at the stern.
            m.boxed(Vec3::new(-1.5, 1.2, 0.0), Vec3::new(len - beam * 1.4 - 1.5, 0.35, beam * 1.005), 0.0, rgba([0.1, 0.14, 0.25]));
            m.boxed(Vec3::new(-len * 0.5 - 0.6, 0.35, 0.0), Vec3::new(1.4, 0.2, beam * 0.8), 0.0, rgba([0.72, 0.6, 0.45]));
            // Saloon deck and its window band.
            m.boxed(Vec3::new(-1.5, 3.0, 0.0), Vec3::new(len * 0.5, 2.0, beam * 0.78), 0.0, rgba(white));
            m.boxed(Vec3::new(-1.5, 3.2, 0.0), Vec3::new(len * 0.5 + 0.05, 0.8, beam * 0.785), 0.0, rgba(glass));
            // Upper deck, set back, then the flybridge and radar arch.
            m.boxed(Vec3::new(-2.0, 4.85, 0.0), Vec3::new(len * 0.32, 1.7, beam * 0.65), 0.0, rgba(white));
            m.boxed(Vec3::new(-0.9, 5.0, 0.0), Vec3::new(len * 0.1, 0.7, beam * 0.655), 0.0, rgba(glass));
            m.boxed(Vec3::new(-3.0, 5.85, 0.0), Vec3::new(len * 0.22, 0.3, beam * 0.7), 0.0, rgba([0.85, 0.85, 0.83]));
            for s in [-1.0f32, 1.0] {
                m.beam(Vec3::new(-3.5, 5.7, s * beam * 0.3), Vec3::new(-3.0, 7.4, s * beam * 0.15), 0.2, rgba(white));
            }
            m.boxed(Vec3::new(-3.0, 7.45, 0.0), Vec3::new(0.6, 0.25, beam * 0.35), 0.0, rgba(white));
        }
        ShipKind::Sailboat => {
            let (len, beam) = boat_size(kind);
            hull(&mut m, len, beam, 2.2, -0.8, 0.7, white, [0.7, 0.6, 0.45]);
            m.boxed(Vec3::new(-0.6, 1.05, 0.0), Vec3::new(3.2, 0.7, 1.8), 0.0, rgba(white));
            m.boxed(Vec3::new(-0.6, 1.1, 0.0), Vec3::new(3.25, 0.25, 1.85), 0.0, rgba(glass));
            // Mast, boom, mainsail and jib (both faces of each sail).
            let mast = 0.9;
            let top = 11.0;
            m.boxed(Vec3::new(mast, (0.7 + top) * 0.5, 0.0), Vec3::new(0.14, top - 0.7, 0.14), 0.0, rgba([0.8, 0.8, 0.8]));
            m.boxed(Vec3::new(mast - 1.9, 1.9, 0.0), Vec3::new(3.8, 0.12, 0.12), 0.0, rgba([0.8, 0.8, 0.8]));
            let sail = rgba([0.96, 0.95, 0.92]);
            let (a, b, c) = (Vec3::new(mast - 0.1, top - 0.3, 0.0), Vec3::new(mast - 0.1, 2.0, 0.0), Vec3::new(mast - 3.7, 2.0, 0.0));
            m.tri_facing(a, b, c, Vec3::Z, sail);
            m.tri_facing(a, b, c, -Vec3::Z, sail);
            let (a, b, c) = (Vec3::new(mast + 0.1, top - 1.5, 0.0), Vec3::new(len * 0.5 - 0.3, 0.8, 0.0), Vec3::new(mast + 0.2, 1.2, 0.0));
            m.tri_facing(a, b, c, Vec3::Z, sail);
            m.tri_facing(a, b, c, -Vec3::Z, sail);
        }
    }
    m
}

/// A ship's hull and deckhouse as one small mesh, bow along +x, waterline
/// at 0: a freighter with container stacks, or a ferry with a cabin.
pub fn ship_mesh(freighter: bool, palette: &Palette) -> MeshData {
    use glam::Vec3;
    let mut m = MeshData::default();
    let (len, beam) = if freighter { (70.0, 12.0) } else { (28.0, 8.0) };
    let hull = if freighter { [0.18, 0.2, 0.24] } else { [0.85, 0.86, 0.88] };
    let half = len * 0.5;
    let bottom = [Vec3::new(-half, -1.5, -beam * 0.5), Vec3::new(half - beam, -1.5, -beam * 0.5), Vec3::new(half, -1.5, 0.0), Vec3::new(half - beam, -1.5, beam * 0.5), Vec3::new(-half, -1.5, beam * 0.5)];
    let top: Vec<Vec3> = bottom.iter().map(|p| *p + Vec3::Y * 4.0).collect();
    m.frustum(&bottom, &top, rgba(hull));
    // The deck, paler than the hull. `frustum` already capped the top in
    // the hull colour; a second cap in the same plane z-fought with it, so
    // the deck stands a few centimetres proud of it (and hides it).
    let deck: Vec<Vec3> = top.iter().map(|p| *p + Vec3::Y * 0.06).collect();
    m.cap(&deck, Vec3::Y, rgba(mul(hull, 1.3)));
    if freighter {
        m.boxed(Vec3::new(-half + 6.0, 6.5, 0.0), Vec3::new(7.0, 7.0, beam * 0.8), 0.0, rgba([0.9, 0.9, 0.88]));
        let colors = &palette.container_colors;
        for k in 0..6 {
            let x = -half + 16.0 + k as f32 * 7.5;
            let c = if colors.is_empty() { [0.7, 0.3, 0.2] } else { colors[k % colors.len()] };
            let tiers = 1 + (k % 3) as i32;
            m.boxed(Vec3::new(x, 2.5 + tiers as f32 * 1.3, 0.0), Vec3::new(6.8, tiers as f32 * 2.6, beam * 0.85), 0.0, rgba(c));
        }
    } else {
        m.boxed(Vec3::new(-1.0, 4.0, 0.0), Vec3::new(len * 0.6, 3.0, beam * 0.8), 0.0, rgba([0.92, 0.92, 0.9]));
        m.boxed(Vec3::new(-2.0, 6.2, 0.0), Vec3::new(len * 0.35, 1.8, beam * 0.6), 0.0, rgba(palette.glass_dark));
        m.boxed(Vec3::new(-2.0, 4.0, 0.0), Vec3::new(len * 0.62, 0.6, beam * 0.82), 0.0, rgba([0.9, 0.25, 0.2]));
    }
    m
}

// ── retaining walls ─────────────────────────────────────────────────────────

/// [review] Where a corner lot levelled to its front street stands over a
/// lower side street (`nd_sim::landscape::Retaining`): a board-formed
/// concrete wall along the lot's edge, its outer face on the edge, from
/// below the side street's pavement up to a coping just over the pad; and
/// the lot's ground as one level cap at the pad (the coarse terrain mesh
/// dips toward the street behind the wall, `keep_under_streets`). Absolute
/// heights: drawn as is, not lifted. Returns (walls, caps).
pub fn retaining_walls(world: &World, palette: &Palette) -> (MeshData, MeshData) {
    const THICK: f32 = 0.35;
    const COPING: f32 = 0.12;
    const FOOTING: f32 = 0.3;
    let mut walls = MeshData::default();
    let mut caps = MeshData::default();
    let concrete = rgba(mul(palette.concrete(0), 0.95));
    let ground = rgba(mul(palette.ground, 0.9));
    let mut capped: Vec<nd_sim::LotId> = Vec::new();
    for r in &world.ground.retaining {
        let (a, b) = (r.a, r.b);
        let (ia, ib) = (a + r.inward * THICK, b + r.inward * THICK);
        let top = r.top + COPING;
        let (fa, fb) = (r.foot_a.min(r.top) - FOOTING, r.foot_b.min(r.top) - FOOTING);
        let v = |p: Vec2, y: f32| glam::Vec3::new(p.x, y, p.y);
        let out = glam::Vec3::new(-r.inward.x, 0.0, -r.inward.y);
        // The face to the street, the coping, the lip above the pad behind.
        walls.quad_facing([v(a, fa), v(b, fb), v(b, top), v(a, top)], out, concrete);
        walls.quad_facing([v(a, top), v(b, top), v(ib, top), v(ia, top)], glam::Vec3::Y, concrete);
        walls.quad_facing([v(ia, r.top - 0.05), v(ib, r.top - 0.05), v(ib, top), v(ia, top)], -out, concrete);
        // The ends, where the wall meets the next lot or the corner.
        let along = (b - a).normalize_or_zero();
        let along3 = glam::Vec3::new(along.x, 0.0, along.y);
        walls.quad_facing([v(a, fa), v(ia, fa), v(ia, top), v(a, top)], -along3, concrete);
        walls.quad_facing([v(b, fb), v(ib, fb), v(ib, top), v(b, top)], along3, concrete);
        if !capped.contains(&r.lot) {
            capped.push(r.lot);
            if let Some(lot) = world.lots.get(r.lot) {
                caps.ground_polygon(&lot.polygon, r.top + 0.01, ground);
            }
        }
    }
    (walls, caps)
}

#[cfg(test)]
mod sweep_visual_tests {
    use super::*;

    /// [sweep-visual] A cutting wall along one arm of a junction is not
    /// stood on the next arm: `on_other_way` sees that arm's lanes and
    /// pavements, and nothing past them.
    #[test]
    fn cutting_walls_keep_off_the_other_arms() {
        let mut roads = nd_sim::roads::RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0)], 1, 1);
        roads.add_polyline(&[Vec2::new(0.0, -100.0), Vec2::new(0.0, 100.0)], 1, 1);
        let along_x = roads.alive_segments().find(|(sid, _)| {
            let (a, b) = roads.segment_points(*sid);
            (a.y - b.y).abs() < 0.1 && a.x.max(b.x) > 50.0
        });
        let (sid, s) = along_x.expect("an east piece");
        let off = s.half_width() + nd_sim::zoning::SIDEWALK + 3.4;
        // Its wall line close to the junction crosses the north-south street.
        assert!(on_other_way(&roads, sid, Vec2::new(1.0, off)));
        // Further along it is clear.
        assert!(!on_other_way(&roads, sid, Vec2::new(40.0, off)));
    }
}

/// Test support for the orientation checks: the vertical faces of `m`
/// standing `height` tall (one kind of box in a mix), as (horizontal
/// normal, horizontal width). A box turned the wrong way round has its
/// long faces where its short ones should be.
#[cfg(test)]
pub(crate) fn upright_faces(m: &crate::mesh::MeshData, height: f32) -> Vec<(Vec2, f32)> {
    let mut out = Vec::new();
    for t in m.indices.chunks_exact(3) {
        let v = [t[0], t[1], t[2]].map(|i| glam::Vec3::from(m.positions[i as usize]));
        let n = (v[1] - v[0]).cross(v[2] - v[0]).normalize_or_zero();
        if n.y.abs() > 0.05 || n == glam::Vec3::ZERO {
            continue;
        }
        let (lo, hi) = v.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
        if ((hi - lo) - height).abs() > 0.01 {
            continue;
        }
        let along = Vec2::new(-n.z, n.x);
        let (a, b) = v.iter().fold((f32::MAX, f32::MIN), |(a, b), p| {
            let s = Vec2::new(p.x, p.z).dot(along);
            (a.min(s), b.max(s))
        });
        out.push((Vec2::new(n.x, n.z), b - a));
    }
    out
}

/// Every face of `faces` whose normal runs along `dir` is `want` wide
/// (within 5 cm): the box lies the right way round on the way.
#[cfg(test)]
pub(crate) fn faces_along_are(faces: &[(Vec2, f32)], dir: Vec2, want: f32) -> Result<(), String> {
    let hits: Vec<f32> = faces.iter().filter(|(n, _)| n.dot(dir).abs() > 0.95).map(|(_, w)| *w).collect();
    if hits.is_empty() {
        return Err("no faces along the way".into());
    }
    match hits.iter().find(|w| (**w - want).abs() > 0.05) {
        Some(w) => Err(format!("a face square to the way is {w:.2} m wide, not {want:.2}")),
        None => Ok(()),
    }
}

#[cfg(test)]
mod orientation_tests {
    use super::*;

    /// Review #4: every box on a way at 45 degrees lies along or across it
    /// as drawn on an axis-aligned one. `boxed` turns local +x to (cos yaw,
    /// sin yaw); the terrain works passed -yaw and turned each by twice
    /// the way's angle, so a crosshead ran along the deck.
    #[test]
    fn works_on_a_45_degree_way_face_the_way() {
        let c = [0.6, 0.6, 0.6];
        for deg in [45.0f32, 30.0, 0.0, 135.0] {
            let dir = Vec2::new(deg.to_radians().cos(), deg.to_radians().sin());
            let hw = 6.0;
            // A pillar: its crosshead lies across the deck.
            let mut g = Geometry::default();
            pillar_column(&mut g, Vec2::new(40.0, -30.0), dir, 10.0, 0.0, hw, c);
            faces_along_are(&upright_faces(g.get(Surface::Concrete), 0.7), dir, hw * 1.7).map_err(|e| format!("pillar at {deg}: {e}")).unwrap();
            // A viaduct span's own columns.
            let mut g = Geometry::default();
            let a = Vec2::new(-20.0, 15.0);
            viaduct_span(&mut g, a, a + dir * 60.0, 12.0, 12.0, hw, &|_| 0.0, c, c, true);
            faces_along_are(&upright_faces(g.get(Surface::Concrete), 0.7), dir, hw * 1.7).map_err(|e| format!("viaduct at {deg}: {e}")).unwrap();
            // A bridge abutment's seat spans the deck's width.
            let mut g = Geometry::default();
            abutment(&mut g, Vec2::new(5.0, 5.0), dir, 8.0, hw, 0.0, c);
            faces_along_are(&upright_faces(g.get(Surface::Concrete), 7.6), dir, hw * 2.0 + 0.8).map_err(|e| format!("abutment at {deg}: {e}")).unwrap();
            // A tunnel portal's coping runs along the headwall's top.
            let mut g = Geometry::default();
            let col = [0.5, 0.5, 0.5, 1.0];
            portal_arch(&mut g, Vec2::new(-7.0, 3.0), dir, 0.0, 5.0, 12.0, col, col, col);
            faces_along_are(&upright_faces(g.get(Surface::Concrete), 0.4), dir, (5.0 + 0.2 + 1.6) * 2.0 + 0.4).map_err(|e| format!("portal at {deg}: {e}")).unwrap();
        }
    }

    /// Review §1.5: clearing ground (a road through it) drops the scatter
    /// there and leaves every other piece where it was; at the cap at most
    /// as many others come in as were cleared.
    #[test]
    fn clearing_a_cell_changes_only_that_cell() {
        // Wide enough for the cap to bite.
        let cfg = TerrainConfig { half_size: 3200.0, ..TerrainConfig::default() };
        let growth = 1.0;
        let all = scatter_cells(&cfg, 9, growth, &|_| true);
        assert!(all.len() >= (3500.0 * growth) as usize, "the cap is in play: {}", all.len());
        let hole = |p: Vec2| p.distance(Vec2::new(300.0, -200.0)) > 120.0;
        let some = scatter_cells(&cfg, 9, growth, &hole);
        let spot = |(i, j): (i32, i32)| scatter_spot(&cfg, &mut scatter_rng(9, i, j), i, j);
        let cleared = all.iter().filter(|c| !hole(spot(**c))).count();
        assert!(cleared > 0);
        for c in &all {
            assert!(!hole(spot(*c)) || some.contains(c), "cell {c:?} outside the cleared ground went");
        }
        let new = some.iter().filter(|c| !all.contains(c)).count();
        assert!(new <= cleared, "{new} cells came in for {cleared} cleared");
    }
}
