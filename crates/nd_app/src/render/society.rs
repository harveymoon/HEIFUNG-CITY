//! [society] Skid row and the trash, drawn from the simulation's state:
//! camps (tents, tarps, carts) on cheap pavements and vacant lots, the RVs
//! and vans people live in at the kerb, and bins overflowing onto the
//! pavement in front of buildings nobody collects from. Garbage trucks need
//! nothing here: they are cars whose model is the generator's garbage
//! truck, drawn by `sync_cars`.
//!
//! Merged geometry per map cell ([r7-sim-thread]), rebuilt when its signature changes:
//! a camp's size, a van arriving or leaving, a building's bins crossing an
//! overflow step, or the land under a camp crossing the skid row line (a
//! camp shows only where land value is low).
//!
//! [r11-adopt-frames] The cells that changed are put together and turned
//! into meshes on the async pool (`render::cell_jobs`) while the sim runs
//! on its thread: on a 38,000-person city most cells change with every
//! batch, 20-70 ms of the frame that took it in (over 200 ms when the
//! ground changed and every piece was built again).

use super::cell_jobs::{geometry_meshes, off_thread, spawn_meshes, CellJob, ReadyMesh};
use super::*;
use nd_procgen::society::{camp, trash_pile};
use nd_sim::homeless::SKID_ROW_LV;
use std::sync::Arc;

#[derive(Component, Clone)]
pub(super) struct SocietyTag;

#[derive(Resource, Default)]
pub(super) struct SocietyRender {
    key: Option<u64>,
    /// [r7-sim-thread] The drawing, by map cell ([`CELL`]): each cell's
    /// pieces hashed, and its meshes. A change rebuilds only its cell: one
    /// merged mesh for the whole city took ~100 ms on a 15,000-person city
    /// every time a camp grew or a bin crossed a step, several times a game
    /// hour, which the sim thread's batches turned into a hitch per batch.
    /// [r11-adopt-frames] No hash: drawn from pieces since rebuilt (a new
    /// ground or palette), kept on screen until it is drawn again.
    cells: HashMap<ChunkKey, (Option<u64>, Vec<Entity>)>,
    /// [r6-perf] Each camp, van and pile as last built and set on the
    /// ground, by everything it is built from, and the ground and
    /// landscape versions they were raised against (`pieces_on`). A
    /// rebuild makes again only what changed: raising every vertex of a
    /// 38,000-person city's piles took 1.7 s at each bin step.
    pieces: HashMap<u64, Arc<Geometry>>,
    pieces_on: Option<u64>,
    /// [r11-adopt-frames] Cells being drawn on the async pool.
    job: Option<CellJob<Vec<DrawnCell>>>,
}

/// [r7-sim-thread] Cells of four building chunks a side.
const CELL: f32 = CHUNK * 4.0;

fn cell_of(p: SVec2) -> ChunkKey {
    ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
}

/// What a piece is made from. [r11-adopt-frames] Owned, so a job can
/// carry it to the async pool.
#[derive(Clone, Copy, Debug)]
pub(super) enum Piece {
    /// A camp: where, the way its tents run, the way out, tents, on a
    /// pavement (or under a deck), seed; `deck`: raised onto a deck where
    /// there is one ([r5-layered] a camp under a deck stays on the ground
    /// under it).
    Camp { pos: SVec2, dir: SVec2, out: SVec2, tents: u8, sidewalk: bool, seed: u64, deck: bool },
    Rv { pos: SVec2, heading: f32, camper: bool, seed: u64, abandoned: bool },
    Pile { p: SVec2, along: SVec2, front: SVec2, step: u8, dumpster: bool, seed: u64 },
}

/// The key of a piece: what it is built from, hashed.
fn piece_key(parts: impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    parts.hash(&mut h);
    h.finish()
}

/// Overflow drawn in steps, so a pile is not rebuilt every hour.
pub(super) fn overflow_step(o: f32) -> u8 {
    if o <= 0.0 {
        0
    } else {
        (1.0 + o * 3.0).min(8.0) as u8
    }
}

/// Camps fade from view where the land is no longer cheap.
fn camp_visible(w: &World, p: SVec2) -> bool {
    w.land_value.at(p) < SKID_ROW_LV + 0.06
}

/// The hash of everything drawn: unchanged, nothing is looked at again.
fn society_key(w: &World) -> u64 {
    let s = &w.society;
    let mut h = DefaultHasher::new();
    for c in &s.homeless.camps {
        (c.id, c.tents(), c.sidewalk, c.under, camp_visible(w, c.pos)).hash(&mut h);
        hash_f32s(&mut h, [c.pos]);
    }
    // [pt-growth] An RV shows once it has pulled up.
    let tick = w.clock.tick;
    for r in &s.homeless.rvs {
        (r.seed, r.camper, r.abandoned.is_some(), r.parked(tick)).hash(&mut h);
    }
    for (id, bin) in &s.trash.bins {
        let step = overflow_step(bin.overflow());
        if step > 0 {
            (id, step).hash(&mut h);
        }
    }
    w.landscape.is_flat().hash(&mut h);
    h.finish()
}

/// Every piece drawn now, with its cell and key, in drawing order (camps,
/// vans, piles).
pub(super) fn society_items(w: &World) -> Vec<(ChunkKey, u64, Piece)> {
    let s = &w.society;
    let mut items: Vec<(ChunkKey, u64, Piece)> = Vec::new();
    for c in &s.homeless.camps {
        if !c.under && !camp_visible(w, c.pos) {
            continue;
        }
        let out = if c.out != SVec2::ZERO { c.out } else { c.lot.and_then(|l| w.lots.get(l)).map_or(SVec2::Y, |l| l.front_dir) };
        let seed = w.config.seed ^ c.id as u64 * 0x9E37;
        let key = piece_key(("camp", c.under, [c.pos.x, c.pos.y, c.dir.x, c.dir.y, out.x, out.y].map(f32::to_bits), c.tents(), c.sidewalk || c.under, seed));
        // [r5-layered] Camps under a deck: on the ground under it (not up
        // on the deck), tents in a file along it, whatever the land is worth.
        items.push((cell_of(c.pos), key, Piece::Camp { pos: c.pos, dir: c.dir, out, tents: c.tents(), sidewalk: c.sidewalk || c.under, seed, deck: !c.under }));
    }
    let tick = w.clock.tick;
    for r in s.homeless.rvs.iter().filter(|r| r.parked(tick)) {
        let key = piece_key(("rv", [r.pos.x, r.pos.y, r.heading].map(f32::to_bits), r.camper, r.seed, r.abandoned.is_some()));
        items.push((cell_of(r.pos), key, Piece::Rv { pos: r.pos, heading: r.heading, camper: r.camper, seed: r.seed, abandoned: r.abandoned.is_some() }));
    }
    for (id, bin) in &s.trash.bins {
        let step = overflow_step(bin.overflow());
        if step == 0 {
            continue;
        }
        let Some(b) = w.buildings.get(*id) else { continue };
        let Some(lot) = w.lots.get(b.lot) else { continue };
        let along = (lot.front_edge[1] - lot.front_edge[0]).normalize_or_zero();
        let len = lot.front_edge[0].distance(lot.front_edge[1]);
        // On the pavement, off to one side of the door, the bins against
        // the lot's front line ([r14-photofix] they stood 0.65 m out, the
        // bags and litter to 2.4 m: across the footway, `trash_pile`).
        let p = lot.entrance() + along * (len * 0.28).min(5.0) + lot.front_dir.normalize_or_zero() * 0.95;
        let dumpster = b.homes == 0 || b.business.is_some();
        let seed = w.config.seed ^ (lot.center.x.to_bits() as u64).wrapping_mul(31) ^ lot.center.y.to_bits() as u64;
        let key = piece_key(("pile", [p.x, p.y, along.x, along.y, lot.front_dir.x, lot.front_dir.y].map(f32::to_bits), step, dumpster, seed));
        items.push((cell_of(p), key, Piece::Pile { p, along, front: lot.front_dir, step, dumpster, seed }));
    }
    items
}

/// Build a piece and set it on the ground: by `surface_height` (onto a
/// deck where there is one), or by the ground alone.
pub(super) fn build_piece(w: &World, piece: &Piece, pal: &nd_procgen::Palette) -> Geometry {
    let (mut g, deck) = match *piece {
        Piece::Camp { pos, dir, out, tents, sidewalk, seed, deck } => (camp(pos, dir, out, tents, sidewalk, seed, pal), deck),
        Piece::Rv { pos, heading, camper, seed, abandoned } => (nd_procgen::society::rv_kept(pos, heading, camper, seed, abandoned, pal), true),
        Piece::Pile { p, along, front, step, dumpster, seed } => (trash_pile(p, along, front, step as f32 / 3.0, dumpster, seed, pal), true),
    };
    if w.landscape.is_flat() {
    } else if deck {
        for part in g.parts.iter_mut() {
            for p in part.positions.iter_mut() {
                p[1] += w.surface_height(SVec2::new(p[0], p[2]));
            }
        }
    } else {
        for part in g.parts.iter_mut().chain(g.detail.iter_mut().flatten()) {
            for p in part.positions.iter_mut() {
                p[1] += w.ground_height(SVec2::new(p[0], p[2]));
            }
        }
    }
    g
}

/// [r11-adopt-frames] A cell to draw: its hash and its pieces in order,
/// each with its key and, when the cache has it, the piece as built.
pub(super) struct CellWork {
    pub cell: ChunkKey,
    pub hash: u64,
    pub pieces: Vec<(u64, Piece, Option<Arc<Geometry>>)>,
}

/// [r11-adopt-frames] A cell drawn: its meshes (none when it is empty)
/// and the pieces built for it, for the cache.
pub(super) struct DrawnCell {
    pub cell: ChunkKey,
    pub hash: u64,
    pub meshes: Vec<ReadyMesh>,
    pub built: Vec<(u64, Arc<Geometry>)>,
}

/// [r11-adopt-frames] Put each cell together from its pieces (building
/// those not cached) and make its meshes; None when `halt` said stop.
/// The same on the main thread or the async pool.
pub(super) fn draw_cells(w: &World, pal: &nd_procgen::Palette, work: &[CellWork], halt: &dyn Fn() -> bool) -> Option<Vec<DrawnCell>> {
    let mut out = Vec::with_capacity(work.len());
    for c in work {
        if halt() {
            return None;
        }
        let geo = cell_geometry(w, pal, c);
        let meshes = if geo.0.triangle_count() == 0 { Vec::new() } else { geometry_meshes(&geo.0) };
        out.push(DrawnCell { cell: c.cell, hash: c.hash, meshes, built: geo.1 });
    }
    Some(out)
}

/// A cell's merged geometry, and the pieces built for it.
pub(super) fn cell_geometry(w: &World, pal: &nd_procgen::Palette, c: &CellWork) -> (Geometry, Vec<(u64, Arc<Geometry>)>) {
    let mut geo = Geometry::default();
    let mut built: Vec<(u64, Arc<Geometry>)> = Vec::new();
    for (key, piece, cached) in &c.pieces {
        let g = match (cached, built.iter().find(|(k, _)| k == key)) {
            (Some(g), _) | (None, Some((_, g))) => g.clone(),
            (None, None) => {
                let g = Arc::new(build_piece(w, piece, pal));
                built.push((*key, g.clone()));
                g
            }
        };
        geo.append(&g);
    }
    (geo, built)
}

/// Each cell's hash: its pieces' keys in order.
pub(super) fn cell_hashes(items: &[(ChunkKey, u64, Piece)]) -> HashMap<ChunkKey, u64> {
    let mut cell_hash: HashMap<ChunkKey, DefaultHasher> = HashMap::new();
    for (cell, key, _) in items {
        key.hash(cell_hash.entry(*cell).or_default());
    }
    cell_hash.into_iter().map(|(c, h)| (c, h.finish())).collect()
}

/// [r11-adopt-frames] The cells to draw again, in order: those whose hash
/// is not the one drawn (new, changed, or drawn from pieces since rebuilt).
pub(super) fn cells_to_draw(drawn: &HashMap<ChunkKey, (Option<u64>, Vec<Entity>)>, items: Vec<(ChunkKey, u64, Piece)>, cell_hash: &HashMap<ChunkKey, u64>, pieces: &HashMap<u64, Arc<Geometry>>) -> Vec<CellWork> {
    let mut work: HashMap<ChunkKey, CellWork> = HashMap::new();
    for (cell, key, piece) in items {
        let hash = cell_hash[&cell];
        if drawn.get(&cell).and_then(|d| d.0) == Some(hash) {
            continue;
        }
        let cached = pieces.get(&key).cloned();
        work.entry(cell).or_insert_with(|| CellWork { cell, hash, pieces: Vec::new() }).pieces.push((key, piece, cached));
    }
    let mut work: Vec<CellWork> = work.into_values().collect();
    work.sort_unstable_by_key(|c| c.cell);
    work
}

pub(super) fn sync_society(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, sim: Res<Sim>, assets: Res<GameAssets>, mats: Res<Materials>, mut state: ResMut<SocietyRender>) {
    let _t = crate::frame_stats::span("sync_society");
    let w = &sim.world;
    let renew = assets.palette_changed || sim.full_rebuild;
    // [r11-adopt-frames] Cells drawn on the pool: swapped in together. A
    // new city or palette drops what is being drawn.
    if renew {
        state.job = None;
    }
    if state.job.as_ref().is_some_and(|j| j.finished()) {
        let mut job = state.job.take().expect("checked");
        match job.take() {
            Some(drawn) => apply(&mut commands, &mut meshes, &mats, &mut state, drawn),
            // Recalled by an edit: looked at again in the city as it is now.
            None => state.key = None,
        }
    }
    if state.job.is_some() {
        return;
    }
    let key = society_key(w);
    if state.key == Some(key) && !renew {
        return;
    }
    let _r = crate::frame_stats::span("society_rebuild");
    state.key = Some(key);
    // A new city: the old one's cells go now.
    if sim.full_rebuild {
        for (_, (_, ents)) in state.cells.drain() {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    let on = piece_key((sim.generation, w.ground.version(), w.landscape.version(), w.relief, w.landscape.is_flat()));
    if state.pieces_on != Some(on) || renew {
        state.pieces.clear();
        state.pieces_on = Some(on);
        // Every cell is drawn again; until then the old drawing stays.
        for d in state.cells.values_mut() {
            d.0 = None;
        }
    }
    // Every piece with its key and cell; a cell's hash is its pieces' keys.
    let items = society_items(w);
    let cell_hash = cell_hashes(&items);
    // Cells gone lose their meshes now; cells changed keep theirs until
    // their new ones are made.
    let gone: Vec<ChunkKey> = state.cells.keys().filter(|c| !cell_hash.contains_key(*c)).copied().collect();
    for c in &gone {
        if let Some((_, ents)) = state.cells.remove(c) {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    // Pieces of cells kept stay in the cache unbuilt.
    let used: HashSet<u64> = items.iter().map(|i| i.1).collect();
    state.pieces.retain(|k, _| used.contains(k));
    let work = cells_to_draw(&state.cells, items, &cell_hash, &state.pieces);
    if work.is_empty() {
        return;
    }
    if off_thread(&sim) {
        let pal = assets.palette.clone();
        state.job = Some(CellJob::start(sim.world.lend(), move |w, halt| draw_cells(w, &pal, &work, halt)));
    } else if let Some(drawn) = draw_cells(w, &assets.palette, &work, &|| false) {
        apply(&mut commands, &mut meshes, &mats, &mut state, drawn);
    }
}

/// Swap drawn cells in for the old ones and keep the pieces they built.
fn apply(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &Materials, state: &mut SocietyRender, drawn: Vec<DrawnCell>) {
    let _sp = crate::frame_stats::span("society_spawn");
    for d in drawn {
        if let Some((_, ents)) = state.cells.remove(&d.cell) {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
        state.pieces.extend(d.built);
        let ents = spawn_meshes(commands, meshes, mats, d.meshes, SocietyTag);
        state.cells.insert(d.cell, (Some(d.hash), ents));
    }
}

#[cfg(test)]
#[path = "society_tests.rs"]
mod tests;
