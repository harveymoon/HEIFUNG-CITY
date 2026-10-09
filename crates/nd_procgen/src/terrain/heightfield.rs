//! Height-field meshes: the flat map's ground, the far ground ring, and
//! the landscape's built ground (graded to the roads and lots).

use super::*;

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
