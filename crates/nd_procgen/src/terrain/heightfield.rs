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
            mesh.normals[j * n + i] = grid_normal(&heights, n, cfg.cell, i, j);
        }
    }
    grid_indices(&mut mesh, 0, n, n);
    mesh
}

/// The ground mesh for a world with a landscape: the built ground (roads
/// cut and filled in, lots levelled) inside the city, natural terrain
/// beyond, sand along the water, mud under it, sulphur at the hot spots.
/// For the flat map use [`build_terrain`]. The renderer draws the same
/// ground in tiles it rebuilds one by one ([`LandscapeTiles`]).
pub fn build_landscape_terrain(world: &World, palette: &Palette, cell: f32) -> MeshData {
    let n = landscape_n(cell);
    let colours = LandColours::new(world, palette);
    let spots = ground_spots(world, cell, n);
    let (raw, kept) = landscape_heights(world, cell, n);
    let mut mesh = MeshData::default();
    for j in 0..n {
        for i in 0..n {
            let k = j * n + i;
            let (x, z) = grid_xz(cell, i, j);
            let rgb = colours.rgb(spots[k], raw[k]);
            mesh.positions.push([x, kept[k], z]);
            mesh.normals.push(grid_normal(&kept, n, cell, i, j));
            mesh.colors.push([rgb[0], rgb[1], rgb[2], 1.0]);
            mesh.uvs.push([x, -z]);
        }
    }
    grid_indices(&mut mesh, 0, n, n);
    // [p2-look] The strip mines' pits are dug below it.
    crate::holes::cut_holes(&mut mesh, &crate::holes::ground_holes(world));
    mesh
}

/// Vertices along a side of the landscape's ground at `cell`.
fn landscape_n(cell: f32) -> usize {
    ((LAND_HALF * 2.0) / cell).ceil() as usize + 1
}

/// World x, z of grid vertex (i, j).
fn grid_xz(cell: f32, i: usize, j: usize) -> (f32, f32) {
    (-LAND_HALF + i as f32 * cell, -LAND_HALF + j as f32 * cell)
}

/// What a vertex's colour takes from the map alone (fixed for a landscape):
/// the detail noise, the distance to water and how far into a hot spot.
#[derive(Clone, Copy)]
pub(crate) struct GroundSpot {
    pub(crate) v: f32,
    pub(crate) d: f32,
    pub(crate) hot: Option<f32>,
}

fn ground_spots(world: &World, cell: f32, n: usize) -> Vec<GroundSpot> {
    let land = &world.landscape;
    let detail = Fbm::<Perlin>::new(land.params.seed as u32 ^ 99).set_octaves(3);
    by_rows(n, |i, j| {
        let (x, z) = grid_xz(cell, i, j);
        let p = Vec2::new(x, z);
        let v = detail.get([x as f64 * 0.02, z as f64 * 0.02]) as f32 * 0.5 + 0.5;
        let d = land.water_distance(p);
        let hot = land.hot_spots.iter().find(|s| s.pos.distance(p) < s.radius).map(|s| (1.0 - s.pos.distance(p) / s.radius) * 0.7);
        GroundSpot { v, d, hot }
    })
}

/// `f` at every vertex of an `n` by `n` grid, row by row, the rows shared
/// out over a few threads (each vertex stands alone, so the result is the
/// same however they are shared).
fn by_rows<T: Send + Clone>(n: usize, f: impl Fn(usize, usize) -> T + Sync) -> Vec<T> {
    let threads = std::thread::available_parallelism().map_or(1, |t| t.get()).clamp(1, 8);
    let rows_per = n.div_ceil(threads);
    let mut parts: Vec<Vec<T>> = Vec::new();
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..threads)
            .map(|t| {
                let f = &f;
                s.spawn(move || {
                    let rows = (t * rows_per)..((t + 1) * rows_per).min(n);
                    let mut out = Vec::with_capacity(rows.len() * n);
                    for j in rows {
                        for i in 0..n {
                            out.push(f(i, j));
                        }
                    }
                    out
                })
            })
            .collect();
        parts = handles.into_iter().map(|h| h.join().expect("landscape rows")).collect();
    });
    parts.concat()
}

/// The landscape's colours: ground and hills from the palette, the shore
/// and the hot ground from the climate's row.
pub(crate) struct LandColours {
    ground: [f32; 3],
    variation: [f32; 3],
    hills: [f32; 3],
    sand: [f32; 3],
    mud: [f32; 3],
    sulphur: [f32; 3],
}

impl LandColours {
    fn new(world: &World, palette: &Palette) -> Self {
        Self::for_climate(world.landscape.params.climate, palette)
    }

    /// [horizon] The same colours for a climate (the far ground's).
    pub(crate) fn for_climate(climate: nd_sim::Climate, palette: &Palette) -> Self {
        let look = climate::look(climate);
        Self {
            ground: palette.ground,
            variation: palette.ground_variation,
            hills: palette.hills,
            sand: look.shore.unwrap_or_else(|| mix(palette.ground, [0.62, 0.56, 0.42], 0.6)),
            mud: mul(mix(palette.ground, [0.25, 0.22, 0.16], 0.6), 0.7),
            sulphur: look.hot_ground,
        }
    }

    /// A vertex's colour at height `h` (the graded ground's, before it is
    /// held under the streets): hills up high, sand along the water, mud
    /// under it, sulphur at the hot spots.
    pub(crate) fn rgb(&self, s: GroundSpot, h: f32) -> [f32; 3] {
        let hill_t = ((h - 12.0) / 120.0).clamp(0.0, 1.0);
        let mut flat = [0.0f32; 3];
        for k in 0..3 {
            let f = self.ground[k] + self.variation[k] * s.v;
            flat[k] = f * (1.0 - hill_t) + self.hills[k] * hill_t;
        }
        let rgb = if s.d < 0.0 {
            self.mud
        } else if s.d < 14.0 {
            mix(self.sand, flat, s.d / 14.0)
        } else {
            flat
        };
        match s.hot {
            Some(t) => mix(rgb, self.sulphur, t),
            None => rgb,
        }
    }
}

/// The graded ground's height at every vertex (a hair under it, so road
/// and lot meshes win), and the same held under the streets.
fn landscape_heights(world: &World, cell: f32, n: usize) -> (Vec<f32>, Vec<f32>) {
    let raw = by_rows(n, |i, j| {
        let (x, z) = grid_xz(cell, i, j);
        world.ground_height(Vec2::new(x, z)) - 0.08
    });
    let mut kept = raw.clone();
    keep_under_streets(world, cell, n, &mut kept);
    (raw, kept)
}

/// The normal at grid vertex (i, j) of an `n` by `n` height field, from
/// central differences (one-sided at the edges).
fn grid_normal(heights: &[f32], n: usize, cell: f32, i: usize, j: usize) -> [f32; 3] {
    let h = |ii: isize, jj: isize| heights[(jj.clamp(0, n as isize - 1) as usize) * n + ii.clamp(0, n as isize - 1) as usize];
    let dx = h(i as isize + 1, j as isize) - h(i as isize - 1, j as isize);
    let dz = h(i as isize, j as isize + 1) - h(i as isize, j as isize - 1);
    glam::Vec3::new(-dx, 2.0 * cell, -dz).normalize().to_array()
}

/// Two triangles per cell of a `w` by `h` grid of vertices starting at
/// `base`, row by row.
fn grid_indices(mesh: &mut MeshData, base: u32, w: usize, h: usize) {
    for j in 0..h - 1 {
        for i in 0..w - 1 {
            let a = base + (j * w + i) as u32;
            let b = a + 1;
            let c = a + w as u32;
            let d = c + 1;
            mesh.indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
}

/// The landscape's ground in square tiles of [`LandscapeTiles::TILE`]
/// cells, for the renderer: what depends on the map alone (each vertex's
/// noise, distance to water and hot ground) is worked out once, and after
/// a road or lot edit [`LandscapeTiles::update`] re-reads only the graded
/// heights and rebuilds the tiles whose vertices (or their neighbours, for
/// the normals) moved. Together the tiles hold exactly the triangles of
/// [`build_landscape_terrain`] at the same cell.
pub struct LandscapeTiles {
    cell: f32,
    n: usize,
    per_side: usize,
    colours: LandColours,
    spots: Vec<GroundSpot>,
    raw: Vec<f32>,
    kept: Vec<f32>,
    /// [p2-look] The holes cut through the ground (`holes::ground_holes`).
    holes: Vec<Vec<Vec2>>,
    meshes: Vec<MeshData>,
}

impl LandscapeTiles {
    /// Cells along a tile's side: 128 m at the renderer's 8 m cell.
    pub const TILE: usize = 16;

    pub fn new(world: &World, palette: &Palette, cell: f32) -> Self {
        let n = landscape_n(cell);
        let per_side = (n - 1).div_ceil(Self::TILE);
        let (raw, kept) = landscape_heights(world, cell, n);
        let holes = crate::holes::ground_holes(world);
        let mut t = Self { cell, n, per_side, colours: LandColours::new(world, palette), spots: ground_spots(world, cell, n), raw, kept, holes, meshes: Vec::new() };
        t.meshes = (0..per_side * per_side).map(|k| t.build_tile(k)).collect();
        t
    }

    /// Re-read the graded ground and rebuild the tiles it changed; returns
    /// their indices (ascending).
    pub fn update(&mut self, world: &World) -> Vec<usize> {
        let (raw, kept) = landscape_heights(world, self.cell, self.n);
        let mut dirty = vec![false; self.meshes.len()];
        let n = self.n;
        for k in 0..n * n {
            if raw[k].to_bits() == self.raw[k].to_bits() && kept[k].to_bits() == self.kept[k].to_bits() {
                continue;
            }
            // The tiles holding the vertex or one of its four neighbours
            // (whose normals read it): vertex u is in tiles
            // (u - 1) / TILE ..= u / TILE.
            let (i, j) = (k % n, k / n);
            let tiles = |v: usize| (v.saturating_sub(2) / Self::TILE).min(self.per_side - 1)..=((v + 1).min(n - 1) / Self::TILE).min(self.per_side - 1);
            for tj in tiles(j) {
                for ti in tiles(i) {
                    dirty[tj * self.per_side + ti] = true;
                }
            }
        }
        // [p2-look] A pit dug, grown or filled in: the tiles under it.
        let holes = crate::holes::ground_holes(world);
        if holes != self.holes {
            let tile_of = |v: f32| (((v + LAND_HALF) / self.cell).floor().max(0.0) as usize / Self::TILE).min(self.per_side - 1);
            for hole in self.holes.iter().chain(holes.iter()) {
                let (lo, hi) = hole.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
                for tj in tile_of(lo.y)..=tile_of(hi.y) {
                    for ti in tile_of(lo.x)..=tile_of(hi.x) {
                        dirty[tj * self.per_side + ti] = true;
                    }
                }
            }
            self.holes = holes;
        }
        self.raw = raw;
        self.kept = kept;
        let changed: Vec<usize> = (0..dirty.len()).filter(|&t| dirty[t]).collect();
        for &t in &changed {
            self.meshes[t] = self.build_tile(t);
        }
        changed
    }

    /// How many tiles there are (some may be empty).
    pub fn len(&self) -> usize {
        self.meshes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.meshes.is_empty()
    }

    pub fn mesh(&self, tile: usize) -> &MeshData {
        &self.meshes[tile]
    }

    /// The vertex range of a tile, inclusive: its cells and the shared edge.
    fn range(&self, tile: usize) -> ((usize, usize), (usize, usize)) {
        let (ti, tj) = (tile % self.per_side, tile / self.per_side);
        let i0 = ti * Self::TILE;
        let j0 = tj * Self::TILE;
        ((i0, (i0 + Self::TILE).min(self.n - 1)), (j0, (j0 + Self::TILE).min(self.n - 1)))
    }

    fn build_tile(&self, tile: usize) -> MeshData {
        let ((i0, i1), (j0, j1)) = self.range(tile);
        let (n, cell) = (self.n, self.cell);
        let mut mesh = MeshData::default();
        for j in j0..=j1 {
            for i in i0..=i1 {
                let k = j * n + i;
                let (x, z) = grid_xz(cell, i, j);
                let rgb = self.colours.rgb(self.spots[k], self.raw[k]);
                mesh.positions.push([x, self.kept[k], z]);
                mesh.normals.push(grid_normal(&self.kept, n, cell, i, j));
                mesh.colors.push([rgb[0], rgb[1], rgb[2], 1.0]);
                mesh.uvs.push([x, -z]);
            }
        }
        grid_indices(&mut mesh, 0, i1 - i0 + 1, j1 - j0 + 1);
        crate::holes::cut_holes(&mut mesh, &self.holes);
        mesh
    }
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
        // [pt-visual] A way held between retaining walls (`earthwork_hold`)
        // holds only its band: the vertices past the wall stay the land the
        // wall holds back (pulled down, they sank the grass beside it and
        // left the wall standing proud, F11).
        let held = nd_sim::landscape::earthwork_hold(&world.landscape, roads, &world.road_z, &world.road_inner, sid).is_some();
        let r = if held { nd_sim::landscape::earthwork_band(s).1 } else { s.half_width() + reach };
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

#[cfg(test)]
mod tile_tests {
    use super::*;
    use nd_sim::roads::RoadKind;
    use nd_sim::{Gesture, WorldConfig};

    fn world() -> World {
        let mut w = World::new(WorldConfig { landscape: nd_sim::landscape::LandscapeParams { relief: 0.8, ..nd_sim::landscape::LandscapeParams::default() }, ..WorldConfig::default() });
        w.economy.treasury = 1.0e9;
        w.add_gesture(Gesture::Road { points: vec![Vec2::new(-300.0, -200.0), Vec2::new(250.0, -150.0)], lanes_fwd: 2, lanes_bwd: 2, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
        w.implement();
        w
    }

    /// The tiles hold the whole mesh's triangles, and after an edit the
    /// tiles `update` rebuilt (and only those) differ, matching tiles built
    /// afresh.
    #[test]
    fn tiles_match_the_whole_mesh_and_follow_an_edit() {
        let pal = Palette::default();
        let mut w = world();
        let mut tiles = LandscapeTiles::new(&w, &pal, 8.0);
        let whole = build_landscape_terrain(&w, &pal, 8.0);
        let tris = |m: &MeshData| {
            let mut v: Vec<[u32; 36]> = m
                .indices
                .chunks_exact(3)
                .map(|t| {
                    let mut out = [0u32; 36];
                    for (c, &i) in t.iter().enumerate() {
                        let i = i as usize;
                        let a = [m.positions[i].as_slice(), m.normals[i].as_slice(), m.colors[i].as_slice(), m.uvs[i].as_slice()].concat();
                        for (k, x) in a.iter().enumerate() {
                            out[c * 12 + k] = x.to_bits();
                        }
                    }
                    out
                })
                .collect();
            v.sort();
            v
        };
        let mut union = MeshData::default();
        for t in 0..tiles.len() {
            union.append(tiles.mesh(t));
        }
        assert_eq!(tris(&union), tris(&whole));
        assert!(tiles.update(&w).is_empty(), "nothing changed");
        let before: Vec<MeshData> = (0..tiles.len()).map(|t| tiles.mesh(t).clone()).collect();
        w.add_gesture(Gesture::Road { points: vec![Vec2::new(100.0, 200.0), Vec2::new(180.0, 330.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
        w.implement();
        let changed = tiles.update(&w);
        let fresh = LandscapeTiles::new(&w, &pal, 8.0);
        assert!(!changed.is_empty() && changed.len() < tiles.len() / 4, "{} of {} tiles rebuilt", changed.len(), tiles.len());
        for t in 0..tiles.len() {
            assert!(*tiles.mesh(t) == *fresh.mesh(t), "tile {t} is stale");
            if !changed.contains(&t) {
                assert!(*tiles.mesh(t) == before[t]);
            }
        }
    }

    /// Timings for the review (§4.1, #19): the whole landscape and water
    /// sheet, against the tiles' update, after one road is added to a
    /// generated city.
    /// `cargo test --profile fast -p nd_procgen landscape_rebuild_timing -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn landscape_rebuild_timing() {
        use std::time::Instant;
        let pal = Palette::default();
        let (mut w, _, _) = crate::citygen::generate_world(crate::citygen::CityPreset::CoastalRiver, 1);
        let mut tiles = LandscapeTiles::new(&w, &pal, 8.0);
        let c = w.lots.values().map(|l| l.center).fold(Vec2::ZERO, |a, p| a + p) / w.lots.len().max(1) as f32;
        w.economy.treasury = 1.0e9;
        w.add_gesture(Gesture::Road { points: vec![c + Vec2::new(-60.0, 40.0), c + Vec2::new(70.0, 90.0)], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
        w.implement();
        let t0 = Instant::now();
        let whole = build_landscape_terrain(&w, &pal, 8.0);
        let t1 = Instant::now();
        let water = crate::terrain::build_water(&w, &pal);
        let t2 = Instant::now();
        let changed = tiles.update(&w);
        let t3 = Instant::now();
        let fresh = LandscapeTiles::new(&w, &pal, 8.0);
        let t4 = Instant::now();
        let rebuilt: usize = changed.iter().map(|&t| tiles.mesh(t).indices.len() / 3).sum();
        eprintln!(
            "whole landscape {:.1} ms ({} tris), water sheet {:.1} ms ({} tris); tiles update {:.1} ms ({} of {} tiles, {} tris rebuilt); tiles from scratch {:.1} ms",
            (t1 - t0).as_secs_f64() * 1e3,
            whole.indices.len() / 3,
            (t2 - t1).as_secs_f64() * 1e3,
            water.indices.len() / 3,
            (t3 - t2).as_secs_f64() * 1e3,
            changed.len(),
            tiles.len(),
            rebuilt,
            (t4 - t3).as_secs_f64() * 1e3
        );
        let t5 = Instant::now();
        let n = landscape_n(8.0);
        let mut raw = Vec::with_capacity(n * n);
        for j in 0..n {
            for i in 0..n {
                let (x, z) = grid_xz(8.0, i, j);
                raw.push(w.ground_height(Vec2::new(x, z)) - 0.08);
            }
        }
        let t6 = Instant::now();
        keep_under_streets(&w, 8.0, n, &mut raw);
        let t7 = Instant::now();
        eprintln!("of which ground heights {:.1} ms, held under the streets {:.1} ms", (t6 - t5).as_secs_f64() * 1e3, (t7 - t6).as_secs_f64() * 1e3);
        for t in 0..tiles.len() {
            assert!(*tiles.mesh(t) == *fresh.mesh(t));
        }
    }
}
