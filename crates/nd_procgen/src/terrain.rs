//! The ground: a flat basin for the city with procedural hills around it, and
//! a scatter pass that puts rocks, scrub and dead trees on the open ground.

use crate::buildings::parts::{mix, mul, ngon, rgba, rock_world, scrub_world, tree_kind, TreeKind};
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
    let t = ((r - cfg.basin_radius) / (cfg.half_size - cfg.basin_radius)).clamp(0.0, 1.0);
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
pub fn scatter_nature(cfg: &TerrainConfig, palette: &Palette, seed: u64, clear: &dyn Fn(Vec2) -> bool) -> Geometry {
    const STEP: f32 = 18.0;
    const CAP: usize = 3500;
    let mut g = Geometry::default();
    let heights = height_noise(cfg);
    let density = Fbm::<Perlin>::new(cfg.seed.wrapping_add(7)).set_octaves(2);
    let n = ((cfg.half_size * 2.0 - 40.0) / STEP) as i32;
    let dry = palette.foliage_dry;
    let rock = mix(palette.stone, palette.hills, 0.3);
    let mut count = 0;
    for j in 0..n {
        for i in 0..n {
            let mut rng = rng_for(seed, 41 + ((i as u64) << 20 | j as u64) * 0x9E37);
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
                8 => tree_kind(&mut g, p, y, TreeKind::Dead, 3.0 + size * 3.0, 0.0, mul(palette.bark, 0.8), dry),
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

// ── the landscape: ground, water and the works in it ───────────────────────

use nd_sim::landscape::{WaterKind, BANK_HEIGHT, LAND_HALF, WATER_LEVEL};
use nd_sim::ports::PortKind;
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
    let sand = mix(palette.ground, [0.62, 0.56, 0.42], 0.6);
    let mud = mul(mix(palette.ground, [0.25, 0.22, 0.16], 0.6), 0.7);
    let sulphur = [0.62, 0.55, 0.22];
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
    let deep = mul(palette.water, 0.22);
    let shallow = mix(mul(palette.water, 0.45), [0.2, 0.3, 0.28], 0.4);
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
            for (k, c) in corners.iter().enumerate() {
                let d = ds[k].unwrap_or(12.0);
                let t = ((-d) / 25.0).clamp(0.0, 1.0);
                let rgb = mix(shallow, deep, t);
                mesh.positions.push([c.x, WATER_LEVEL, c.y]);
                mesh.normals.push([0.0, 1.0, 0.0]);
                mesh.colors.push([rgb[0], rgb[1], rgb[2], 0.86]);
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
    let deep = mul(palette.water, 0.22);
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
        // Two halves meeting over the crossing: it may be the crest of a
        // way lifted over the other.
        for (p0, p1) in [(a3, m3), (m3, b3)] {
            g.part(Surface::Concrete).slab(p0, p1, hw * 2.0, 1.4, rgba(concrete));
            for sgn in [1.0f32, -1.0] {
                let off = Vec3::new(side.x * sgn, 0.0, side.y * sgn);
                g.part(Surface::Metal).beam(p0 + off + Vec3::Y * 1.0, p1 + off + Vec3::Y * 1.0, 0.15, rgba(steel));
                g.part(Surface::Concrete).slab(p0 + off + Vec3::Y * 0.45, p1 + off + Vec3::Y * 0.45, 0.35, 0.5, rgba(mul(concrete, 0.9)));
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
/// and its pavements under the street mesh, parapets and rails, and a
/// column on a footing every 24 m or so wherever the deck stands clear of
/// the ground (a ramp runs down onto it at the end).
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
        let motorway = seg.is_highway() || nd_sim::roads::is_ramp_spec(seg.kind, seg.lanes_fwd, seg.lanes_bwd);
        let hw = seg.half_width() + if seg.is_street() && !motorway { nd_sim::zoning::SIDEWALK } else { 0.6 };
        viaduct_span(g, a, b, za, zb, hw, &|p| world.ground_height(p), concrete, steel, true);
    }
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
                let side = Vec2::new(-out.y, out.x);
                let yaw = out.y.atan2(out.x);
                let top = z + 6.2;
                for sgn in [-1.0f32, 1.0] {
                    let q = p + side * sgn * (hw + 0.6);
                    g.part(Surface::Concrete).boxed(Vec3::new(q.x, (z + top) * 0.5, q.y), Vec3::new(1.2, top - z, 1.2), -yaw, wall);
                }
                // The lintel over the mouth, with a lit band.
                g.part(Surface::Concrete).boxed(Vec3::new(p.x, top - 0.6, p.y), Vec3::new(1.4, 1.2, hw * 2.0 + 2.4), -yaw, coping);
                let lit = p + out * 0.75;
                g.part(Surface::Glow).boxed(Vec3::new(lit.x, top - 0.9, lit.y), Vec3::new(0.08, 0.25, hw * 1.6), -yaw, rgba(mul(steel, 2.2)));
            }
            // Vent shafts on the surface along the bore.
            let vents = (len / 150.0).floor() as i32;
            for k in 1..=vents {
                let p = a.lerp(b, k as f32 / (vents + 1) as f32);
                let gz = world.ground_height(p);
                g.part(Surface::Concrete).boxed(Vec3::new(p.x, gz + 1.2, p.y), Vec3::new(3.0, 2.4, 3.0), -yaw, wall);
                g.part(Surface::Metal).boxed(Vec3::new(p.x, gz + 2.5, p.y), Vec3::new(2.6, 0.25, 2.6), -yaw, rgba(mul(steel, 0.6)));
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

/// One span of viaduct deck from `a` at height `za` to `b` at `zb`,
/// `hw` either side of the centreline, standing on `ground`. Shared with
/// the plan preview.
#[allow(clippy::too_many_arguments)]
pub fn viaduct_span(g: &mut Geometry, a: Vec2, b: Vec2, za: f32, zb: f32, hw: f32, ground: &dyn Fn(Vec2) -> f32, concrete: [f32; 3], steel: [f32; 3], parapets: bool) {
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
    // too unless the end is near the ground (a ramp foot).
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
        g.part(Surface::Concrete).boxed(Vec3::new(p.x, (top + foot) * 0.5, p.y), Vec3::new(1.6, top - foot, w), -yaw, rgba(mul(concrete, 0.8)));
        // A crosshead under the deck.
        g.part(Surface::Concrete).boxed(Vec3::new(p.x, top - 0.35, p.y), Vec3::new(1.9, 0.7, hw * 1.7), -yaw, rgba(mul(concrete, 0.85)));
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
        let hw = seg.half_width() + if seg.is_street() { nd_sim::zoning::SIDEWALK * 0.5 } else { 0.4 };
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
        // Piers down to the bed every 28 m or so.
        let len = a.distance(b);
        let spans = (len / 28.0).ceil().max(1.0) as i32;
        let yaw = dir.y.atan2(dir.x);
        for k in 1..spans {
            let t = k as f32 / spans as f32;
            let p = a.lerp(b, t);
            let top = za + (zb - za) * t - 1.4;
            let bed = land.height(p).min(WATER_LEVEL - 1.0);
            g.part(Surface::Concrete).boxed(Vec3::new(p.x, (top + bed) * 0.5, p.y), Vec3::new(2.2, top - bed, hw * 1.6), -yaw, rgba(mul(concrete, 0.8)));
        }
    }
    underpasses(world, &mut g, concrete, steel);
    // Dock piers.
    for port in world.ports.ports.values() {
        let (_, width) = port.kind.pier();
        let [root, tip] = port.pier;
        let deck = BANK_HEIGHT + 0.6;
        let r3 = Vec3::new(root.x, deck, root.y);
        let t3 = Vec3::new(tip.x, deck, tip.y);
        g.part(Surface::Concrete).slab(r3, t3, width, 0.8, rgba(mul(concrete, 0.95)));
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
                g.part(Surface::Metal).boxed(Vec3::new(p.x, WATER_LEVEL + 0.5, p.y), Vec3::new(8.0, 1.0, 5.0), -along.y.atan2(along.x), rgba(mul(palette.metal_paint(2), 0.8)));
                g.part(Surface::Metal).cylinder(p, 0.08, 0.08, WATER_LEVEL + 1.0, WATER_LEVEL + 5.0, 6, rgba([0.2, 0.2, 0.22]));
                g.part(Surface::Glow).boxed(Vec3::new(p.x, WATER_LEVEL + 5.1, p.y), Vec3::new(0.4, 0.25, 0.4), 0.0, rgba(palette.lamp));
            }
        }
    }
    // Plants that work the water.
    for b in world.buildings.values() {
        let Some(kind) = b.plant else { continue };
        let Some(lot) = world.lots.get(b.lot) else { continue };
        match kind {
            PowerPlantKind::Hydro => {
                // A weir across the river beside the plant.
                if let Some((ri, _, bank)) = land.river_beside(&lot.polygon, 40.0) {
                    let r = &land.rivers[ri];
                    let (seg, c, _) = r.nearest(bank);
                    let flow = r.flow(seg);
                    let across = Vec2::new(-flow.y, flow.x);
                    let half = r.width * 0.5 + 4.0;
                    let (p0, p1) = (c - across * half, c + across * half);
                    let top = BANK_HEIGHT + 1.2;
                    g.part(Surface::Concrete).slab(Vec3::new(p0.x, top, p0.y), Vec3::new(p1.x, top, p1.y), 4.0, top + 3.0, rgba(concrete));
                    // Spillway foam below it.
                    let foam = c + flow * 6.0;
                    g.part(Surface::Matte).ground_strip(&[foam - across * (half - 4.0), foam + across * (half - 4.0)], 7.0, WATER_LEVEL + 0.08, rgba([0.86, 0.9, 0.92]));
                    for k in 0..(r.width / 6.0) as i32 {
                        let p = p0 + across * (4.0 + k as f32 * 6.0);
                        g.part(Surface::Metal).boxed(Vec3::new(p.x, top + 1.6, p.y), Vec3::new(1.0, 2.0, 1.0), 0.0, rgba(steel));
                    }
                }
            }
            PowerPlantKind::Tidal => {
                if let Some(shore) = lot.polygon.iter().filter_map(|p| land.nearest_shore(*p, 50.0)).find(|s| s.water.kind == WaterKind::Sea) {
                    for k in 0..5 {
                        let p = shore.point + shore.seaward * (12.0 + k as f32 * 11.0);
                        g.part(Surface::Concrete).boxed(Vec3::new(p.x, WATER_LEVEL, p.y), Vec3::new(7.0, 4.0, 7.0), 0.0, rgba(concrete));
                        g.part(Surface::Metal).boxed(Vec3::new(p.x, WATER_LEVEL + 2.8, p.y), Vec3::new(2.0, 1.6, 2.0), 0.0, rgba(mul(palette.metal_paint(3), 0.9)));
                    }
                    let a = shore.point - shore.seaward * 2.0;
                    let b = shore.point + shore.seaward * 60.0;
                    g.part(Surface::Concrete).slab(Vec3::new(a.x, BANK_HEIGHT + 0.8, a.y), Vec3::new(b.x, BANK_HEIGHT + 0.8, b.y), 3.0, 0.6, rgba(mul(concrete, 0.9)));
                }
            }
            _ => {}
        }
    }
    // Steaming vents at the hot spots.
    for (k, s) in land.hot_spots.iter().enumerate() {
        let mut rng = rng_for(land.params.seed, 900 + k as u64);
        for _ in 0..7 {
            let p = s.pos + Vec2::from_angle(rng.random_range(0.0..6.28)) * rng.random_range(0.0..s.radius * 0.8);
            let y = world.ground_height(p);
            let size = 0.8 + rng.random::<f32>();
            rock_world(&mut g, &mut rng, p, y - 0.1, size, mul(palette.stone, 0.7));
            for q in 0..4 {
                let yy = y + 0.5 + q as f32 * 1.6;
                g.part(Surface::Matte).cylinder(p + Vec2::splat(q as f32 * 0.25), 0.4 + q as f32 * 0.35, 0.6 + q as f32 * 0.35, yy, yy + 1.4, 7, rgba([0.88, 0.88, 0.86]));
            }
        }
    }
    g
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
    m.cap(&top, Vec3::Y, rgba(mul(hull, 1.3)));
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
