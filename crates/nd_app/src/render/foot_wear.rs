//! [r10-worn-paths] Paths worn by real feet: the walking-heat field
//! (`nd_sim::foot_wear`, laid by the walkers themselves) drawn on the
//! ground as a mask, not as triangles per step.
//!
//! - The field's 128 m chunks are drawn as tiles: per tile at most two
//!   translucent, lit sheets draped over the built ground, each with a
//!   128 x 128 texture (1 m texels) worked out on the CPU from the heat.
//! - **Soft ground** (park lawns, vacant lots, verges and open land, a
//!   run-down park's dirt) wears into earth: worn grass, then a bare
//!   track with broken, gritty edges (two octaves of value noise, fixed to
//!   the world so tiles meet). Its sheet lies just over a park's lawn and
//!   under its paths and the pavements. Dusty and pale in the desert,
//!   dark mud in the temperate and tropical climates, grey slush in the
//!   arctic, ash on a volcanic map.
//! - **Paving** (pavements, footpaths, a park's paved walks, built lots and
//!   plazas) only darkens a little, in blotches, where a great many feet
//!   go: outside busy shop doors, stops and station entrances, crossings.
//!   Its sheet lies just over the pavement's kerb height.
//! - Carriageways, rails and water get nothing.
//!
//! What each cell is (soft, paving, neither) is read from the city when a
//! tile is rebuilt, for the cells hot enough to show only. Tiles are
//! rebuilt a few per frame after the field changes (it fades on the hour,
//! so about once a game hour), round-robin, so a sweep never hitches a
//! frame. The `foot_wear` map layer shows the heat itself.

use super::*;
use nd_procgen::overlay::{heat_color, HeatGrid};
use nd_sim::foot_wear::{self, FootWear, CELL, CHUNK, CHUNK_M, WEAR_START};
use nd_sim::Climate;
use std::collections::VecDeque;

/// Texels per tile side (1 m each).
const TEX: usize = 128;
/// Texels per cell side.
const TPC: usize = TEX / CHUNK;
/// Height of the soft sheet over the built ground: over a park's lawn
/// (0.03) and a run-down park's dirt paths (0.05), under its paved paths
/// (0.06) and the pavements.
const SOFT_Y: f32 = 0.055;
/// Height of the paving sheet: just over the pavement (`KERB_H` 0.16).
const HARD_Y: f32 = 0.172;
/// Heat at which paving starts to darken (about 45 passes a day kept up)
/// and at which it is as dark as it gets (about 1,850 a day).
pub const PAVE_START: f32 = 200.0;
pub const PAVE_FULL: f32 = 8_000.0;
/// Vertex spacing of a draped sheet on a landscape, metres.
const DRAPE: f32 = 8.0;
/// Most tiles rebuilt in one frame, and the time a frame may spend.
const TILES_PER_FRAME: usize = 3;
const FRAME_BUDGET_MS: f32 = 3.0;
/// The map layer's cell, metres, and seconds between its rebuilds.
const LAYER_CELL: f32 = 4.0;
const LAYER_SECS: f32 = 3.0;

/// What the ground of a cell is, for the wear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ground {
    Unknown,
    /// Grass, earth, a vacant lot: wears into a track.
    Soft,
    /// Pavement, paths, plazas, built lots: darkens.
    Paving,
    /// A carriageway, rails, water: nothing drawn.
    None,
}

#[derive(Component)]
pub(super) struct FootWearTag;

#[derive(Component)]
pub(super) struct FootWearLayerTag;

struct Sheet {
    entity: Entity,
    image: Handle<Image>,
}

#[derive(Default)]
struct Tile {
    soft: Option<Sheet>,
    hard: Option<Sheet>,
}

#[derive(Default)]
pub(super) struct WearState {
    tiles: HashMap<(i32, i32), Tile>,
    /// Tiles still to rebuild in this sweep.
    queue: VecDeque<(i32, i32)>,
    /// The field version the sweep under way is drawing.
    sweeping: Option<u64>,
    /// The field version last swept fully.
    drawn: Option<u64>,
    /// Lots by tile, for the sweep under way.
    lot_bins: HashMap<(i32, i32), Vec<LotId>>,
    flat_quad: Option<Handle<Mesh>>,
    climate: Option<Climate>,
    layer: Option<(f32, u64, Entity)>,
}

/// The colour of worn earth in a climate (linear), and how wet it reads
/// (a wet track is darker down its middle, a dry one paler).
fn earth(climate: Climate, pal_dirt: [f32; 3]) -> ([f32; 3], f32) {
    match climate {
        // It has to stand off the ground it is worn into (often the
        // climate's own dirt): pale dust in the desert, dark elsewhere.
        Climate::Desert => ([pal_dirt[0] * 1.45, pal_dirt[1] * 1.38, pal_dirt[2] * 1.28], -0.6),
        Climate::Temperate => ([0.062, 0.045, 0.028], 1.0),
        Climate::Tropical => ([0.11, 0.052, 0.028], 0.9),
        Climate::Arctic => ([0.11, 0.10, 0.092], 0.4),
        Climate::Volcanic => ([0.024, 0.021, 0.019], 0.3),
    }
}

/// Grime ground into busy paving (linear).
const GRIME: [f32; 3] = [0.02, 0.018, 0.016];

fn hash2(x: i32, y: i32, salt: u32) -> f32 {
    use nd_sim::hash::{fin32_m2, lattice32, low24_01};
    low24_01(fin32_m2(lattice32(x, y, salt)))
}

/// Smooth value noise in `0..1` with lattice spacing `scale` metres, fixed
/// to the world.
fn vnoise(p: SVec2, scale: f32, salt: u32) -> f32 {
    let q = p / scale;
    let (x0, y0) = (q.x.floor(), q.y.floor());
    let (tx, ty) = (q.x - x0, q.y - y0);
    let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
    let (xi, yi) = (x0 as i32, y0 as i32);
    let a = hash2(xi, yi, salt);
    let b = hash2(xi + 1, yi, salt);
    let c = hash2(xi, yi + 1, salt);
    let d = hash2(xi + 1, yi + 1, salt);
    let top = a + (b - a) * sx;
    let bot = c + (d - c) * sx;
    top + (bot - top) * sy
}

fn smooth01(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `0..1`: how dark busy paving under heat `v` is, on a log scale.
pub fn pave_wear(v: f32) -> f32 {
    if v <= PAVE_START {
        return 0.0;
    }
    ((v / PAVE_START).ln() / (PAVE_FULL / PAVE_START).ln()).clamp(0.0, 1.0)
}

fn to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u8
}

fn tile_key(p: SVec2) -> (i32, i32) {
    ((p.x / CHUNK_M).floor() as i32, (p.y / CHUNK_M).floor() as i32)
}

/// Every lot binned into the tiles its bounding box touches.
fn bin_lots(w: &World) -> HashMap<(i32, i32), Vec<LotId>> {
    let mut bins: HashMap<(i32, i32), Vec<LotId>> = HashMap::new();
    for (id, lot) in w.lots.iter() {
        let (mut lo, mut hi) = (SVec2::splat(f32::MAX), SVec2::splat(f32::MIN));
        for p in &lot.polygon {
            lo = lo.min(*p);
            hi = hi.max(*p);
        }
        if lo.x > hi.x {
            continue;
        }
        let (a, b) = (tile_key(lo), tile_key(hi));
        for ty in a.1..=b.1 {
            for tx in a.0..=b.0 {
                bins.entry((tx, ty)).or_default().push(id);
            }
        }
    }
    bins
}

/// The ways near a tile, as (a, b, kind, carriageway half width).
fn ways_near(w: &World, lo: SVec2, hi: SVec2) -> Vec<(SVec2, SVec2, nd_sim::roads::RoadKind, f32)> {
    let pad = SVec2::splat(30.0);
    let mut out = Vec::new();
    for sid in w.roads.segments_near_box(lo - pad, hi + pad) {
        let Some(s) = w.roads.try_segment(sid) else { continue };
        if !s.alive || s.tunnel {
            continue;
        }
        let (a, b) = w.roads.segment_points(sid);
        out.push((a, b, s.kind, s.half_width()));
    }
    out
}

fn seg_distance(p: SVec2, a: SVec2, b: SVec2) -> f32 {
    let ab = b - a;
    let l2 = ab.length_squared();
    let t = if l2 < 1e-6 { 0.0 } else { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) };
    p.distance(a + ab * t)
}

/// What the ground at `p` is.
fn classify(w: &World, p: SVec2, ways: &[(SVec2, SVec2, nd_sim::roads::RoadKind, f32)], lots: &[LotId]) -> Ground {
    use nd_sim::roads::{RoadKind, PATH_HALF_WIDTH, RAIL_HALF_WIDTH};
    let mut pavement = false;
    // Beside a street but on no lot: the pavement runs on to the lot line.
    let mut by_street = false;
    for &(a, b, kind, hw) in ways {
        let d = seg_distance(p, a, b);
        match kind {
            RoadKind::Street => {
                if d <= hw {
                    return Ground::None;
                }
                if d <= hw + nd_sim::zoning::SIDEWALK {
                    pavement = true;
                }
                if d <= hw + nd_sim::zoning::SIDEWALK + 3.0 {
                    by_street = true;
                }
            }
            RoadKind::Path => {
                if d <= PATH_HALF_WIDTH + 0.2 {
                    pavement = true;
                }
            }
            RoadKind::Rail => {
                if d <= RAIL_HALF_WIDTH {
                    return Ground::None;
                }
            }
        }
    }
    if pavement {
        return Ground::Paving;
    }
    for &id in lots {
        let Some(lot) = w.lots.get(id) else { continue };
        if !nd_sim::zoning::point_in_polygon(p, &lot.polygon) {
            continue;
        }
        if let Some(park) = &lot.park {
            // A paved walk (a run-down park's are dirt: they wear too).
            if park.tier != nd_sim::Tier::Low {
                let half = if park.tier == nd_sim::Tier::High { 1.2 } else { 1.0 };
                let on_walk = park.paths.iter().any(|path| path.windows(2).any(|s| seg_distance(p, s[0], s[1]) <= half + 0.15));
                if on_walk {
                    return Ground::Paving;
                }
            }
            return Ground::Soft;
        }
        return if lot.building.is_some() { Ground::Paving } else { Ground::Soft };
    }
    if !w.landscape.is_flat() && w.landscape.is_water(p) {
        return Ground::None;
    }
    if by_street {
        return Ground::Paving;
    }
    // Open ground: verges, the land round the town.
    Ground::Soft
}

/// The heat of a tile's cells with a ring of the neighbours' round them,
/// `(CHUNK + 2)` a side, so texels at the edge blend into the next tile.
fn padded_heat(f: &FootWear, key: (i32, i32)) -> Vec<f32> {
    let n = CHUNK + 2;
    let mut out = vec![0.0; n * n];
    let (bx, by) = (key.0 * CHUNK as i32 - 1, key.1 * CHUNK as i32 - 1);
    let own = f.chunk(key);
    for j in 0..n {
        for i in 0..n {
            let inner = (1..=CHUNK).contains(&i) && (1..=CHUNK).contains(&j);
            out[j * n + i] = match (inner, own) {
                (true, Some(c)) => c[(j - 1) * CHUNK + (i - 1)],
                (true, None) => 0.0,
                _ => f.cell(bx + i as i32, by + j as i32),
            };
        }
    }
    out
}

/// A clamped, mipmapped sRGB texture for a sheet.
fn sheet_image(rgba: &[u8]) -> Image {
    let mut img = Image::new(Extent3d { width: TEX as u32, height: TEX as u32, depth_or_array_layers: 1 }, TextureDimension::D2, rgba.to_vec(), TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
    let (data, levels) = mip_chain(TEX, rgba);
    img.data = Some(data);
    img.texture_descriptor.mip_level_count = levels;
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..ImageSamplerDescriptor::linear()
    });
    img
}

/// A sheet over a tile draped on the built ground, `lift` over it. UVs run
/// 0..1 over the tile with `v` along +y (the texture's rows).
fn draped_sheet(w: &World, key: (i32, i32), lift: f32) -> MeshData {
    let o = FootWear::chunk_origin(key);
    let steps = (CHUNK_M / DRAPE) as usize;
    let mut m = MeshData::default();
    for j in 0..=steps {
        for i in 0..=steps {
            let (u, v) = (i as f32 / steps as f32, j as f32 / steps as f32);
            let p = o + SVec2::new(u, v) * CHUNK_M;
            m.positions.push([p.x, w.ground_height(p) + lift, p.y]);
            m.normals.push([0.0, 1.0, 0.0]);
            m.colors.push([1.0, 1.0, 1.0, 1.0]);
            m.uvs.push([u, v]);
            m.tangents.push([1.0, 0.0, 0.0, 1.0]);
        }
    }
    let n = steps as u32 + 1;
    for j in 0..steps as u32 {
        for i in 0..steps as u32 {
            let c = [j * n + i, j * n + i + 1, (j + 1) * n + i + 1, (j + 1) * n + i];
            m.indices.extend_from_slice(&[c[0], c[2], c[1], c[0], c[3], c[2]]);
        }
    }
    m
}

/// A unit quad (0..1 in x and z, UVs to match) for flat maps, placed and
/// scaled per tile.
fn unit_quad() -> MeshData {
    let mut m = MeshData::default();
    for (x, z) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        m.positions.push([x, 0.0, z]);
        m.normals.push([0.0, 1.0, 0.0]);
        m.colors.push([1.0, 1.0, 1.0, 1.0]);
        m.uvs.push([x, z]);
        m.tangents.push([1.0, 0.0, 0.0, 1.0]);
    }
    m.indices.extend_from_slice(&[0, 2, 1, 0, 3, 2]);
    m
}

/// The two textures of a tile (soft, paving), `None` where nothing shows.
fn tile_textures(w: &World, key: (i32, i32), lots: &[LotId], climate: Climate, pal_dirt: [f32; 3]) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let f = &w.foot_wear;
    let n = CHUNK + 2;
    let heat = padded_heat(f, key);
    // Cells worth knowing the ground of: warm enough to show, or next to one.
    let warm_min = WEAR_START * 0.6;
    let mut need = vec![false; n * n];
    for j in 0..n {
        for i in 0..n {
            if heat[j * n + i] < warm_min {
                continue;
            }
            for dj in j.saturating_sub(1)..=(j + 1).min(n - 1) {
                for di in i.saturating_sub(1)..=(i + 1).min(n - 1) {
                    need[dj * n + di] = true;
                }
            }
        }
    }
    if !need.iter().any(|&b| b) {
        return (None, None);
    }
    let base = FootWear::chunk_origin(key) - SVec2::splat(CELL);
    let ways = ways_near(w, FootWear::chunk_origin(key), FootWear::chunk_origin(key) + SVec2::splat(CHUNK_M));
    let mut ground = vec![Ground::Unknown; n * n];
    for k in 0..n * n {
        if need[k] {
            let p = base + SVec2::new((k % n) as f32 + 0.5, (k / n) as f32 + 0.5) * CELL;
            ground[k] = classify(w, p, &ways, lots);
        }
    }
    let (rgb, wet) = earth(climate, pal_dirt);
    // `ND_FOOT_WEAR_DEBUG=1`: soft ground solid magenta, paving solid cyan
    // wherever they show any wear, to check what the ground was read as.
    let debug = std::env::var("ND_FOOT_WEAR_DEBUG").is_ok_and(|v| v == "1");
    let mut soft = vec![0u8; TEX * TEX * 4];
    let mut hard = vec![0u8; TEX * TEX * 4];
    let (mut any_soft, mut any_hard) = (false, false);
    let origin = FootWear::chunk_origin(key);
    for ty in 0..TEX {
        for tx in 0..TEX {
            let p = origin + SVec2::new(tx as f32 + 0.5, ty as f32 + 0.5) * (CELL / TPC as f32);
            // Heat blended between cell middles, in padded-grid cells.
            let g = (p - base) / CELL - SVec2::splat(0.5);
            let (x0, y0) = (g.x.floor().max(0.0) as usize, g.y.floor().max(0.0) as usize);
            let (x0, y0) = (x0.min(n - 2), y0.min(n - 2));
            let (fx, fy) = ((g.x - x0 as f32).clamp(0.0, 1.0), (g.y - y0 as f32).clamp(0.0, 1.0));
            let at = |i: usize, j: usize| heat[j * n + i];
            let h = {
                let top = at(x0, y0) + (at(x0 + 1, y0) - at(x0, y0)) * fx;
                let bot = at(x0, y0 + 1) + (at(x0 + 1, y0 + 1) - at(x0, y0 + 1)) * fx;
                top + (bot - top) * fy
            };
            if h < warm_min * 0.5 {
                continue;
            }
            // The ground of the cell the texel lies in.
            let ci = (((p.x - base.x) / CELL) as usize).min(n - 1);
            let cj = (((p.y - base.y) / CELL) as usize).min(n - 1);
            let kind = ground[cj * n + ci];
            let i = (ty * TEX + tx) * 4;
            match kind {
                Ground::Soft => {
                    let wear = foot_wear::wear(h);
                    if wear <= 0.0 {
                        continue;
                    }
                    let n1 = 0.65 * vnoise(p, 2.6, 11) + 0.35 * vnoise(p, 0.9, 12);
                    let edge = smooth01(0.22, 0.62, wear + (n1 - 0.5) * 0.6);
                    if edge <= 0.004 {
                        continue;
                    }
                    // Patchy earth; the trodden middle of a busy track
                    // darker (mud) or paler (dust); a few stones.
                    let rut = smooth01(0.7, 1.0, wear) * wet;
                    let grit = if hash2(p.x.floor() as i32, p.y.floor() as i32, 13) > 0.93 { 1.25 } else { 1.0 };
                    let k = (0.82 + 0.36 * vnoise(p, 1.7, 14)) * (1.0 - 0.28 * rut.max(0.0) + 0.22 * (-rut).max(0.0)) * grit;
                    let c = if debug { [1.0, 0.0, 1.0] } else { [rgb[0] * k, rgb[1] * k, rgb[2] * k] };
                    soft[i] = to_srgb(c[0]);
                    soft[i + 1] = to_srgb(c[1]);
                    soft[i + 2] = to_srgb(c[2]);
                    soft[i + 3] = if debug { 255 } else { (edge * 0.92 * 255.0) as u8 };
                    any_soft = true;
                }
                Ground::Paving => {
                    let d = pave_wear(h);
                    if d <= 0.0 {
                        continue;
                    }
                    let blot = 0.55 + 0.7 * vnoise(p, 1.4, 21) * (0.6 + 0.4 * vnoise(p, 0.5, 22));
                    let a = (smooth01(0.0, 0.08, d) * (0.12 + 0.45 * d) * blot).clamp(0.0, 0.62);
                    if a < 0.01 {
                        continue;
                    }
                    let c = if debug { [0.0, 1.0, 1.0] } else { GRIME };
                    hard[i] = to_srgb(c[0]);
                    hard[i + 1] = to_srgb(c[1]);
                    hard[i + 2] = to_srgb(c[2]);
                    hard[i + 3] = if debug { 255 } else { (a * 255.0) as u8 };
                    any_hard = true;
                }
                _ => {}
            }
        }
    }
    (any_soft.then_some(soft), any_hard.then_some(hard))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_foot_wear(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mut state: Local<WearState>,
    tagged: Query<Entity, With<FootWearTag>>,
) {
    // `ND_FOOT_WEAR=0` draws none (to compare frame times).
    if std::env::var("ND_FOOT_WEAR").is_ok_and(|v| v == "0") {
        return;
    }
    let w = &sim.world;
    // A new city (or a reload) or another climate: start again.
    if sim.full_rebuild || state.climate != Some(assets.climate) {
        for e in &tagged {
            commands.entity(e).despawn();
        }
        state.tiles.clear();
        state.queue.clear();
        state.sweeping = None;
        state.drawn = None;
        state.climate = Some(assets.climate);
    }
    let version = w.foot_wear.version;
    if state.queue.is_empty() {
        if let Some(v) = state.sweeping.take() {
            state.drawn = Some(v);
        }
        if state.drawn != Some(version) {
            // A new sweep: every chunk the field holds and every tile drawn.
            let mut keys: Vec<(i32, i32)> = w.foot_wear.chunks().map(|(k, _)| k).collect();
            keys.extend(state.tiles.keys().copied());
            keys.sort_unstable();
            keys.dedup();
            state.queue = keys.into();
            state.sweeping = Some(version);
            state.lot_bins = bin_lots(w);
        }
    }
    if state.queue.is_empty() {
        return;
    }
    let flat = w.landscape.is_flat();
    if flat && state.flat_quad.is_none() {
        state.flat_quad = Some(meshes.add(shared_mesh(&unit_quad(), false)));
    }
    let started = std::time::Instant::now();
    let pal_dirt = assets.palette.dirt;
    let climate = assets.climate;
    let mut done = 0;
    while done < TILES_PER_FRAME && started.elapsed().as_secs_f32() * 1000.0 < FRAME_BUDGET_MS {
        let Some(key) = state.queue.pop_front() else { break };
        done += 1;
        let empty = Vec::new();
        let lots = state.lot_bins.get(&key).unwrap_or(&empty).clone();
        let (soft, hard) = tile_textures(w, key, &lots, climate, pal_dirt);
        let flat_quad = state.flat_quad.clone();
        let tile = state.tiles.entry(key).or_default();
        for (rgba, slot, lift) in [(soft, &mut tile.soft, SOFT_Y), (hard, &mut tile.hard, HARD_Y)] {
            match (rgba, slot.as_ref()) {
                (None, None) => {}
                (None, Some(s)) => {
                    commands.entity(s.entity).despawn();
                    *slot = None;
                }
                (Some(rgba), Some(s)) => {
                    if let Some(mut img) = images.get_mut(&s.image) {
                        *img = sheet_image(&rgba);
                    }
                }
                (Some(rgba), None) => {
                    let image = images.add(sheet_image(&rgba));
                    let mat = materials.add(StandardMaterial {
                        base_color: Color::WHITE,
                        base_color_texture: Some(image.clone()),
                        perceptual_roughness: 0.96,
                        reflectance: 0.04,
                        alpha_mode: AlphaMode::Blend,
                        ..default()
                    });
                    let o = FootWear::chunk_origin(key);
                    let entity = match (&flat_quad, flat) {
                        (Some(q), true) => commands.spawn((Mesh3d(q.clone()), MeshMaterial3d(mat), Transform::from_xyz(o.x, lift, o.y).with_scale(Vec3::new(CHUNK_M, 1.0, CHUNK_M)), NotShadowCaster, FootWearTag)).id(),
                        _ => commands.spawn((Mesh3d(meshes.add(to_mesh(&draped_sheet(w, key, lift), false))), MeshMaterial3d(mat), Transform::IDENTITY, NotShadowCaster, FootWearTag)).id(),
                    };
                    *slot = Some(Sheet { entity, image });
                }
            }
        }
        if tile.soft.is_none() && tile.hard.is_none() {
            state.tiles.remove(&key);
        }
    }
}

/// The `foot_wear` map layer: the heat itself, cool to hot, over a dimmed
/// city (`focus::Focus::Density`), rebuilt every few seconds while on.
pub(super) fn sync_foot_wear_layer(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    mats: Res<Materials>,
    overlays: Res<Overlays>,
    lighting: Res<Lighting>,
    time: Res<Time>,
    mut state: Local<WearState>,
) {
    let on = overlays.foot_wear && !lighting.heat;
    let now = time.elapsed_secs();
    if !on {
        if let Some((_, _, e)) = state.layer.take() {
            commands.entity(e).despawn();
        }
        return;
    }
    let w = &sim.world;
    let version = w.foot_wear.version;
    if let Some((at, v, _)) = state.layer {
        if !sim.full_rebuild && (v == version || now - at < LAYER_SECS) {
            return;
        }
    }
    if let Some((_, _, e)) = state.layer.take() {
        commands.entity(e).despawn();
    }
    let f = &w.foot_wear;
    let (mut lo, mut hi) = (SVec2::splat(f32::MAX), SVec2::splat(f32::MIN));
    for (k, _) in f.chunks() {
        let o = FootWear::chunk_origin(k);
        lo = lo.min(o);
        hi = hi.max(o + SVec2::splat(CHUNK_M));
    }
    let mut m = MeshData::default();
    if lo.x < hi.x {
        let mut g = HeatGrid::covering(lo, hi, LAYER_CELL, 200_000);
        let n = g.cols + 1;
        // Each corner takes the hottest cell round it, so a 2 m path
        // shows at the layer's coarser cell.
        let r = (g.cell * 0.5 / CELL).ceil() as i32;
        for j in 0..=g.rows {
            for i in 0..=g.cols {
                let p = g.corner(i, j);
                let (cx, cy) = foot_wear::cell_of(p);
                let mut best = 0.0f32;
                for dy in -r..r {
                    for dx in -r..r {
                        best = best.max(f.cell(cx + dx, cy + dy));
                    }
                }
                g.values[j * n + i] = best;
            }
        }
        let t = |v: f32| ((1.0 + v).ln() / (1.0 + PAVE_FULL).ln()).clamp(0.0, 1.0);
        let flat = w.landscape.is_flat();
        let height = |p: SVec2| if flat { 0.0 } else { w.ground_height(p) };
        m = g.mesh_by(
            0.45,
            |k| {
                let v = g.values[k];
                let c = heat_color(t(v));
                [c[0] * 1.6, c[1] * 1.6, c[2] * 1.6, if v < 4.0 { 0.0 } else { 0.35 + 0.55 * t(v) }]
            },
            |k| g.values[k] < 4.0,
            height,
        );
    }
    let e = commands.spawn((Mesh3d(meshes.add(to_mesh(&m, false))), MeshMaterial3d(mats.overlay.clone()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, focus::FocusKeep, bevy::camera::visibility::NoFrustumCulling, FootWearLayerTag)).id();
    state.layer = Some((now, version, e));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_is_fixed_to_the_world_and_in_range() {
        let p = SVec2::new(130.25, -77.5);
        assert_eq!(vnoise(p, 2.6, 11), vnoise(p, 2.6, 11));
        for k in 0..200 {
            let v = vnoise(SVec2::new(k as f32 * 0.37, k as f32 * -1.3), 1.7, 3);
            assert!((0.0..=1.0).contains(&v));
        }
    }

    #[test]
    fn paving_darkens_only_where_many_walk() {
        assert_eq!(pave_wear(100.0), 0.0);
        assert_eq!(pave_wear(PAVE_START), 0.0);
        assert!(pave_wear(PAVE_START * 3.0) > 0.0);
        assert!((pave_wear(PAVE_FULL) - 1.0).abs() < 1e-5);
    }
}
