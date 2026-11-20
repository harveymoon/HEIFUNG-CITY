//! [social] What crews and clubs made, drawn from the simulation's state:
//! street-art murals and community gardens on vacant lots
//! (`nd_procgen::social`). Merged geometry per map cell, rebuilt when a
//! mural or a garden in it comes, goes or changes, or the building a mural
//! is on changes.
//!
//! A mural is painted on its building's street face as drawn: the
//! building's geometry is generated again (`render::building_geometry`, the
//! call its chunk uses) and `facade_spot` finds the clear wall facing the
//! street. Only a mural with no building to paint (a vacant lot, or a wall
//! of glass) stands on a free-standing wall. The floodlit night copy of
//! the paint is its own set of meshes, shown only after dark.
//!
//! [r11-adopt-frames] By cell, from pieces kept as built: one merged mesh
//! for the whole city, with every garden and free-standing wall generated
//! and set on the ground again, took 65-90 ms of the frame each time a
//! mural changed (a building it is on changing, its paint wearing), which
//! on Harvey's day-81 city was nearly every batch the sim thread handed over.

use super::cell_jobs::{geometry_meshes, off_thread, spawn_meshes, CellJob, ReadyMesh};
use super::*;
use nd_procgen::social::{crew_tag, facade_spot, garden, mural_wall, paint_mural};
use std::sync::Arc;

#[derive(Component, Clone)]
pub(super) struct SocialTag;

/// The murals' floodlit paint: shown at night only.
#[derive(Component, Clone)]
pub(super) struct MuralNightTag;

/// A piece of the drawing: its day and night geometry, set where it stands.
pub(super) type Paint = Arc<(Geometry, Geometry)>;

#[derive(Resource, Default)]
pub(super) struct SocialRender {
    key: Option<u64>,
    /// [r11-adopt-frames] The drawing by map cell ([`CELL`]): its pieces'
    /// keys hashed (None: to draw again; the old drawing stays meanwhile),
    /// its day and its night meshes.
    cells: HashMap<ChunkKey, (Option<u64>, Vec<Entity>, Vec<Entity>)>,
    /// Buildings carrying a mural, so a change to one repaints.
    walls: HashSet<BuildingId>,
    /// [r6-perf] Each facade mural's paint (day, night), by everything it
    /// is painted from (`facade_key`): a repaint generates again only the
    /// murals whose inputs moved, not every mural's whole building (75 ms
    /// at every repaint of Harvey's day-65 city, several times a game hour).
    facades: HashMap<u64, Option<Paint>>,
    /// [r11-adopt-frames] Free-standing walls and gardens as built and set
    /// on the ground, by what they are built from, against the ground and
    /// landscape versions in `pieces_on`.
    pieces: HashMap<u64, Paint>,
    pieces_on: Option<u64>,
    /// [r11-adopt-frames] Cells being merged and made into meshes on the
    /// async pool.
    job: Option<CellJob<Vec<DrawnCell>>>,
}

/// [r11-adopt-frames] A cell merged: its hash, day and night meshes.
pub(super) struct DrawnCell {
    cell: ChunkKey,
    hash: u64,
    day: Vec<ReadyMesh>,
    night: Vec<ReadyMesh>,
}

/// [r11-adopt-frames] Merge each cell and make its meshes (none for an
/// empty side); None when `halt` said stop.
pub(super) fn draw_cells(cells: Vec<(ChunkKey, u64, Vec<(bool, Paint)>)>, halt: &dyn Fn() -> bool) -> Option<Vec<DrawnCell>> {
    let mut out = Vec::with_capacity(cells.len());
    for (cell, hash, list) in cells {
        if halt() {
            return None;
        }
        let (day, night) = merge_cell(&list);
        let mesh = |g: &Geometry| if g.triangle_count() > 0 { geometry_meshes(g) } else { Vec::new() };
        out.push(DrawnCell { cell, hash, day: mesh(&day), night: mesh(&night) });
    }
    Some(out)
}

/// Below this daylight the floodlit paint is on.
const NIGHT_BELOW: f32 = 0.35;

/// [r11-adopt-frames] Cells of four building chunks a side, as skid row's.
const CELL: f32 = CHUNK * 4.0;

fn cell_of(p: SVec2) -> ChunkKey {
    ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
}

/// [r6-perf] The paint reads the quality only against `paint_mural`'s
/// thresholds (-0.2, 0.1, 0.35): its band is the key.
fn quality_band(q: f32) -> (bool, bool, bool) {
    (q < -0.2, q > 0.1, q > 0.35)
}

/// What a piece of a cell is made from.
#[derive(Clone)]
pub(super) enum Source {
    /// Paint on a building (or its scaffolding: nothing), made already.
    Facade(Paint),
    /// The free-standing wall of the mural at this index.
    Wall(usize),
    /// The garden at this index.
    Garden(usize),
}

impl Source {
    /// Free-standing walls and gardens are drawn first, the paint on
    /// buildings after.
    fn on_ground(&self) -> bool {
        !matches!(self, Source::Facade(_))
    }
}

/// A free-standing wall or a garden, built and set on the built ground
/// (as the whole drawing was, vertex by vertex).
pub(super) fn ground_piece(w: &World, pal: &nd_procgen::Palette, src: &Source) -> (Geometry, Geometry) {
    let s = &w.social;
    let (mut day, mut night) = match *src {
        Source::Wall(i) => {
            let m = &s.murals[i];
            let tag = s.group(m.group).map(|g| crew_tag(&g.name)).unwrap_or_default();
            mural_wall(m.pos, m.along, m.facing, m.width, m.height, m.quality, m.seed, m.group as u64, &tag, pal)
        }
        Source::Garden(i) => {
            let g = &s.gardens[i];
            let lot = &w.lots[g.lot];
            (garden(lot.center, lot.front_dir, lot.width().min(lot.depth()), g.seed), Geometry::default())
        }
        Source::Facade(ref p) => return (p.0.clone(), p.1.clone()),
    };
    if !w.landscape.is_flat() {
        for geo in [&mut day, &mut night] {
            for part in geo.parts.iter_mut() {
                for p in part.positions.iter_mut() {
                    p[1] += w.surface_height(SVec2::new(p[0], p[2]));
                }
            }
        }
    }
    (day, night)
}

/// [r11-adopt-frames] Put a cell together: its pieces' day and night
/// geometry merged, those on the ground first, each kind in the order given.
pub(super) fn merge_cell(pieces: &[(bool, Paint)]) -> (Geometry, Geometry) {
    let mut day = Geometry::default();
    let mut night = Geometry::default();
    for ground in [true, false] {
        for (g, p) in pieces {
            if *g == ground {
                day.append(&p.0);
                night.append(&p.1);
            }
        }
    }
    (day, night)
}

/// The hash of everything drawn: unchanged (and no building painted on
/// changed), nothing is looked at again.
fn social_key(w: &World) -> u64 {
    let s = &w.social;
    let mut h = DefaultHasher::new();
    for m in &s.murals {
        (m.seed, m.group, quality_band(m.quality), m.building).hash(&mut h);
        if let Some(b) = m.building.and_then(|b| w.buildings.get(b)) {
            (b.seed, b.floors, b.tier, b.land_use, b.construction.is_some(), b.demolition.is_some()).hash(&mut h);
        }
    }
    for g in &s.gardens {
        g.seed.hash(&mut h);
    }
    w.landscape.is_flat().hash(&mut h);
    h.finish()
}

/// [r11-adopt-frames] Every piece of the drawing with its cell, key and
/// source, in drawing order; the facade paint made (or taken from
/// `facades`, less those of `changed` buildings) on the way. Also the
/// buildings painted on and the cells holding a changed one.
#[allow(clippy::type_complexity)]
pub(super) fn social_items(w: &World, styles: &StyleBook, kit: super::buildings::Kit, changed: &HashSet<BuildingId>, facades: &mut HashMap<u64, Option<Paint>>) -> (Vec<(ChunkKey, u64, Source)>, HashSet<BuildingId>, HashSet<ChunkKey>) {
    let s = &w.social;
    let mut used: HashSet<u64> = HashSet::new();
    let mut walls = HashSet::new();
    let mut repaint = HashSet::new();
    let mut items = Vec::new();
    for (i, m) in s.murals.iter().enumerate() {
        let crew = m.group as u64;
        let tag = s.group(m.group).map(|g| crew_tag(&g.name)).unwrap_or_default();
        let cell = cell_of(m.pos);
        let on_facade = m.building.and_then(|bid| {
            let b = w.buildings.get(bid)?;
            walls.insert(bid);
            if changed.contains(&bid) {
                repaint.insert(cell);
            }
            // Building works: the wall is scaffolding for now; the mural
            // comes back with the finished face.
            if b.construction.is_some() {
                let mut h = DefaultHasher::new();
                ("works", bid).hash(&mut h);
                return Some((h.finish(), Arc::new((Geometry::default(), Geometry::default()))));
            }
            let pad = w.lot_pad(b.lot);
            let fkey = {
                let mut h = DefaultHasher::new();
                (m.seed, m.group, quality_band(m.quality), m.pos.x.to_bits(), m.pos.y.to_bits(), m.facing.x.to_bits(), m.facing.y.to_bits(), bid, &tag).hash(&mut h);
                (b.seed, b.floors, b.tier, b.land_use, b.demolition.is_some(), b.lot, format!("{pad:?}")).hash(&mut h);
                h.finish()
            };
            used.insert(fkey);
            if changed.contains(&bid) {
                facades.remove(&fkey);
            }
            let painted = facades.entry(fkey).or_insert_with(|| {
                let _p = crate::frame_stats::span("social_paint");
                let geo = building_geometry(w, kit, styles, bid)?;
                // The lot's street edge: the mural's record stands just inside it.
                let front = m.pos + m.facing.normalize_or_zero() * 0.9;
                let spot = facade_spot(&geo, front, m.facing)?;
                let (mut day, mut night) = paint_mural(&spot, m.quality, m.seed, crew, &tag, kit.palette);
                nd_procgen::terrain::raise_geometry(&mut day, pad);
                nd_procgen::terrain::raise_geometry(&mut night, pad);
                Some(Arc::new((day, night)))
            });
            painted.clone().map(|p| (fkey, p))
        });
        match on_facade {
            Some((fkey, p)) => items.push((cell, fkey, Source::Facade(p))),
            None => {
                let mut h = DefaultHasher::new();
                ("wall", [m.pos, m.along, m.facing].map(|v| [v.x.to_bits(), v.y.to_bits()]), m.width.to_bits(), m.height.to_bits(), quality_band(m.quality), m.seed, crew, &tag).hash(&mut h);
                items.push((cell, h.finish(), Source::Wall(i)));
            }
        }
    }
    for (i, g) in s.gardens.iter().enumerate() {
        let Some(lot) = w.lots.get(g.lot) else { continue };
        let size = lot.width().min(lot.depth());
        let mut h = DefaultHasher::new();
        ("garden", [lot.center, lot.front_dir].map(|v| [v.x.to_bits(), v.y.to_bits()]), size.to_bits(), g.seed).hash(&mut h);
        items.push((cell_of(lot.center), h.finish(), Source::Garden(i)));
    }
    facades.retain(|k, _| used.contains(k));
    (items, walls, repaint)
}

/// Each cell's hash: its pieces' keys in order.
pub(super) fn social_cell_hashes(items: &[(ChunkKey, u64, Source)]) -> HashMap<ChunkKey, u64> {
    let mut hashers: HashMap<ChunkKey, DefaultHasher> = HashMap::new();
    for (cell, k, src) in items {
        (src.on_ground(), k).hash(hashers.entry(*cell).or_default());
    }
    hashers.into_iter().map(|(c, h)| (c, h.finish())).collect()
}

/// [r11-adopt-frames] The pieces of each cell in `todo`, ground pieces from
/// `pieces` or built (and kept there).
pub(super) fn cell_pieces(w: &World, pal: &nd_procgen::Palette, items: &[(ChunkKey, u64, Source)], todo: &HashSet<ChunkKey>, pieces: &mut HashMap<u64, Paint>) -> HashMap<ChunkKey, Vec<(bool, Paint)>> {
    let mut cells: HashMap<ChunkKey, Vec<(bool, Paint)>> = HashMap::new();
    for (cell, k, src) in items {
        if !todo.contains(cell) {
            continue;
        }
        let p = match src {
            Source::Facade(p) => p.clone(),
            _ => pieces.entry(*k).or_insert_with(|| Arc::new(ground_piece(w, pal, src))).clone(),
        };
        cells.entry(*cell).or_default().push((src.on_ground(), p));
    }
    cells
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_social(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, sim: Res<Sim>, assets: Res<GameAssets>, mats: Res<Materials>, sky: Res<sky::SkyLook>, mut state: ResMut<SocialRender>, mut nights: Query<&mut Visibility, With<MuralNightTag>>) {
    let _t = crate::frame_stats::span("sync_social");
    // The night paint follows the light every frame (cheap: a handful of entities).
    let show = if sky.daylight < NIGHT_BELOW { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut nights {
        if *v != show {
            *v = show;
        }
    }
    let w = &sim.world;
    let renew = sim.full_rebuild || assets.rules_changed || assets.palette_changed;
    // [r11-adopt-frames] Cells merged on the pool: swapped in together. New
    // rules, palette or city drop what is being drawn.
    if renew {
        state.job = None;
    }
    if state.job.as_ref().is_some_and(|j| j.finished()) {
        let mut job = state.job.take().expect("checked");
        match job.take() {
            Some(drawn) => apply(&mut commands, &mut meshes, &mats, &mut state, drawn, show),
            None => state.key = None,
        }
    }
    if state.job.is_some() {
        return;
    }
    let key = social_key(w);
    let touched = sim.events.iter().any(|e| matches!(e, SimEvent::BuildingChanged(id) | SimEvent::BuildingBuilt(id) if state.walls.contains(id)));
    if state.key == Some(key) && !touched && !renew {
        return;
    }
    let _m = crate::frame_stats::span("social_rebuild");
    state.key = Some(key);
    // A new city: the old one's cells go now. New rules or palette:
    // everything paints again (the old drawing stays till then).
    if sim.full_rebuild {
        for (_, (_, day, night)) in state.cells.drain() {
            for e in day.into_iter().chain(night) {
                commands.entity(e).despawn();
            }
        }
    }
    if renew {
        state.facades.clear();
    }
    let on = {
        let mut h = DefaultHasher::new();
        (sim.generation, w.ground.version(), w.landscape.version(), w.relief, w.landscape.is_flat()).hash(&mut h);
        h.finish()
    };
    if state.pieces_on != Some(on) || renew {
        state.pieces.clear();
        state.pieces_on = Some(on);
        for c in state.cells.values_mut() {
            c.0 = None;
        }
    }
    // Buildings changed this frame paint again, and their cells are drawn again.
    let changed: HashSet<BuildingId> = sim.events.iter().filter_map(|e| match e {
        SimEvent::BuildingChanged(id) | SimEvent::BuildingBuilt(id) => Some(*id),
        _ => None,
    }).collect();
    let mut facades = std::mem::take(&mut state.facades);
    let (items, walls, repaint) = social_items(w, &sim.styles, super::buildings::Kit::of(&assets), &changed, &mut facades);
    state.facades = facades;
    state.walls = walls;
    let hashes = social_cell_hashes(&items);
    let gone: Vec<ChunkKey> = state.cells.keys().filter(|c| !hashes.contains_key(*c)).copied().collect();
    for c in gone {
        if let Some((_, day, night)) = state.cells.remove(&c) {
            for e in day.into_iter().chain(night) {
                commands.entity(e).despawn();
            }
        }
    }
    let todo: HashSet<ChunkKey> = hashes.iter().filter(|(c, h)| repaint.contains(*c) || state.cells.get(*c).and_then(|d| d.0) != Some(**h)).map(|(c, _)| *c).collect();
    let used: HashSet<u64> = items.iter().filter(|i| i.2.on_ground()).map(|i| i.1).collect();
    state.pieces.retain(|k, _| used.contains(k));
    let mut pieces = std::mem::take(&mut state.pieces);
    let cells = cell_pieces(w, &assets.palette, &items, &todo, &mut pieces);
    state.pieces = pieces;
    let mut cells: Vec<(ChunkKey, u64, Vec<(bool, Paint)>)> = cells.into_iter().map(|(c, list)| (c, hashes[&c], list)).collect();
    cells.sort_unstable_by_key(|c| c.0);
    if cells.is_empty() {
        return;
    }
    if off_thread(&sim) {
        state.job = Some(CellJob::detached(move |halt| draw_cells(cells, halt)));
    } else if let Some(drawn) = draw_cells(cells, &|| false) {
        apply(&mut commands, &mut meshes, &mats, &mut state, drawn, show);
    }
}

/// Swap drawn cells in for the old ones; the night paint shown or not by
/// the light now.
fn apply(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &Materials, state: &mut SocialRender, drawn: Vec<DrawnCell>, show: Visibility) {
    let _sp = crate::frame_stats::span("social_spawn");
    for d in drawn {
        if let Some((_, day, night)) = state.cells.remove(&d.cell) {
            for e in day.into_iter().chain(night) {
                commands.entity(e).despawn();
            }
        }
        let day = spawn_meshes(commands, meshes, mats, d.day, SocialTag);
        let night = spawn_meshes(commands, meshes, mats, d.night, MuralNightTag);
        for e in &night {
            commands.entity(*e).insert(show);
        }
        state.cells.insert(d.cell, (Some(d.hash), day, night));
    }
}

#[cfg(test)]
#[path = "social_tests.rs"]
mod tests;
