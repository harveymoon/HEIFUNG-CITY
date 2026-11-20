//! Buildings, batched per chunk, and the cranes over building sites.
//!
//! [refactor/app] Split out of `render.rs`.

use super::*;
use super::chunk_jobs::{build_chunk, building_layer, spawn_layer, DrawKit, Job, LayerKey, ReadyLayer, MAX_JOBS};
use std::sync::Arc;
use std::time::Instant;

/// The field a solar or wind plant is drawn as part of, when its parcel
/// touches others of its kind (`World::plant_cluster`); `None` for a lone
/// plant or anything else.
pub(crate) fn plant_field(world: &World, id: BuildingId) -> Option<nd_procgen::PlantField> {
    let b = world.buildings.get(id)?;
    if !b.plant.is_some_and(|k| k.merges_with_neighbours()) {
        return None;
    }
    let cluster = world.plant_cluster(id);
    if cluster.len() < 2 {
        return None;
    }
    let lots: Vec<&nd_sim::Lot> = cluster.iter().filter_map(|l| world.lots.get(*l)).collect();
    let own = cluster.iter().position(|l| *l == b.lot)?;
    let anchor_seed = world.lots.get(cluster[0])?.building.and_then(|a| world.buildings.get(a)).map_or(0, |a| a.seed);
    nd_procgen::PlantField::new(&lots, own, anchor_seed)
}

/// How far a plant coming or going reaches when redrawing the fields round
/// it: further than any parcel of a field it could touch.
pub(super) const PLANT_FIELD_REACH: f32 = 300.0;

/// [fire-scene] The top of a building's mass as drawn (walls, glass and
/// roofs; not masts, signs or lights), above its pad.
pub(super) fn mass_top(g: &Geometry) -> f32 {
    use Surface as S;
    [S::Concrete, S::Plaster, S::Brick, S::Stone, S::Panel, S::Glass, S::WornConcrete, S::WornPlaster, S::WornBrick, S::WornPanel]
        .iter()
        .filter_map(|s| g.parts.get(s.index()))
        .flat_map(|m| m.positions.iter().map(|p| p[1]))
        .fold(0.0, f32::max)
}

/// [r4-style] Detail cells per chunk side: a chunk's detail layers are cut
/// into `DETAIL_SPLIT`² cells by lot centre, each its own mesh with a
/// distance range, so the detail of a building drops out about when that
/// building is far off, not when the middle of its 128 m chunk is.
pub(super) const DETAIL_SPLIT: usize = 2;

/// The detail cell of a chunk a building's detail goes in.
fn detail_cell(world: &World, id: BuildingId) -> usize {
    let Some(c) = world.buildings.get(id).and_then(|b| world.lots.get(b.lot)).map(|l| l.center) else { return 0 };
    let k = DETAIL_SPLIT as f32 / CHUNK;
    let (i, j) = (((c.x * k).floor() as i64).rem_euclid(DETAIL_SPLIT as i64), ((c.y * k).floor() as i64).rem_euclid(DETAIL_SPLIT as i64));
    (j as usize) * DETAIL_SPLIT + i as usize
}

/// [r10-chunk-hitch] What building generation reads besides the world
/// and the styles: the rules and the palette (from `GameAssets`, or a
/// chunk job's own `DrawKit`).
#[derive(Clone, Copy)]
pub(crate) struct Kit<'a> {
    pub rules: &'a nd_procgen::BuildingRules,
    pub palette: &'a nd_procgen::Palette,
}

impl<'a> Kit<'a> {
    pub(crate) fn of(assets: &'a GameAssets) -> Self {
        Kit { rules: &assets.rules, palette: &assets.palette }
    }
}

/// Generate one building's geometry into the chunk's merged geometry, its
/// detail layers into the chunk's detail cell for it.
/// Returns its drawn height and the triangles of ghosts on its walls.
pub(super) fn append_building(geo: &mut Geometry, cells: &mut [Geometry], world: &World, kit: Kit, styles: &StyleBook, id: BuildingId) -> Option<(f32, u32)> {
    let b = world.buildings.get(id)?;
    let mut g = building_geometry(world, kit, styles, id)?;
    // [r5-hillside] On a lot stepped down the hill, storeys under its back
    // part down to the lower pad (lit with the building's own windows).
    if let Some(step) = world.ground.steps.get(&b.lot) {
        let plinth = nd_procgen::hill_plinth::hill_plinth(&g, step, world.lot_pad(b.lot), b.seed, kit.palette, true);
        g.append(&plinth);
    }
    // [r5-hillside] A terraced lot's pad off its street's height: steps up
    // (or down) from the pavement to the door (heights from the pad).
    if let Some(lot) = world.lots.get(b.lot).filter(|l| world.relief && nd_sim::landscape::walled_terrace(&world.landscape, l, world.lot_pad(b.lot))) {
        if world.roads.try_segment(lot.front_segment).is_some_and(|s| !s.bridge) {
            let street = world.road_height(lot.front_segment, lot.front_t) - world.lot_pad(b.lot);
            g.append(&nd_procgen::hill_plinth::entrance_stair(lot.front_edge, lot.front_dir, street, 0.0, kit.palette));
        }
    }
    // [r11-lot-memory] The ghosts of the buildings that stood against its
    // walls (none on a site or a demolition: `ghost_traces`).
    let traces = nd_procgen::lot_memory::ghost_traces(world, id);
    let mut ghost_tris = 0;
    if !traces.is_empty() {
        let ghosts = nd_procgen::lot_memory::wall_ghosts(&g, &traces, world.clock.tick);
        ghost_tris = ghosts.triangle_count() as u32;
        g.append(&ghosts);
    }
    // [climate] Snow on the roofs in a cold climate.
    nd_procgen::climate::snow_cover(&mut g, kit.palette.snow);
    lights::tag_windows(&mut g, lights::slot_of(id));
    let top = mass_top(&g);
    // Every building stands on its lot's levelled pad.
    nd_procgen::terrain::raise_geometry(&mut g, world.lot_pad(b.lot));
    if g.has_detail() {
        let mut d = Geometry::default();
        d.detail = std::mem::take(&mut g.detail);
        cells[detail_cell(world, id).min(cells.len() - 1)].append(&d);
    }
    geo.append(&g);
    Some((top, ghost_tris))
}

/// One building's geometry as drawn, before it is raised onto its lot's
/// pad (`World::lot_pad`); [social] murals read their wall off it.
pub(crate) fn building_geometry(world: &World, kit: Kit, styles: &StyleBook, id: BuildingId) -> Option<Geometry> {
    let b = world.buildings.get(id)?;
    // [airport] The whole field is drawn by `render/airport.rs`.
    if b.airport.is_some() {
        return Some(Geometry::default());
    }
    // [land] A building over merged plots is drawn on their combined
    // footprint (`World::footprint`): one building, no seams.
    let footprint = world.footprint(id)?;
    let lot: &nd_sim::Lot = &footprint;
    let field = if b.construction.is_none() && b.demolition.is_none() { plant_field(world, id) } else { None };
    // [r5-layered] The decks over a park: no trees or tall features there.
    let overhead = if b.land_use == nd_sim::LandUse::Park { nd_procgen::undercroft::deck_quads(world, &lot.polygon) } else { Vec::new() };
    // [r11-lot-memory] Old signs painted over where the trade changed.
    let ghost_signs = nd_procgen::lot_memory::ghost_signs(world, id);
    // What decides the style comes from `style_extras`, the same inputs the
    // style book and the inspector's label use (a taxi company's yard is a
    // surface lot whose cabs `render/taxis.rs` stands in the bays); only
    // the look of the moment is added here.
    let extras = Extras {
        powered: b.powered,
        park: lot.park.as_ref(),
        construction: b.construction.as_ref().map(|c| c.progress),
        demolition: None,
        animated_rotors: true,
        field: field.as_ref(),
        animated_cranes: true,
        dynamic_lights: true,
        fleet: b.business.as_ref().filter(|z| z.kind == nd_sim::BusinessKind::FleetLot).map_or(0.5, |z| z.fleet_trucks as f32 / world.fleet_capacity(id).max(1) as f32),
        overhead: &overhead,
        ghost_signs: &ghost_signs,
        ..nd_procgen::style_extras(world, id, styles.neighbours(id))
    };
    let g = match &b.demolition {
        // Coming down: what stood there (which, during a redevelopment or a
        // civic take-over, is not what the building's own fields describe),
        // cut down to what is left, with the demolition site around it.
        Some(d) => {
            let extras = Extras { powered: b.powered, park: lot.park.as_ref(), demolition: Some(d.progress), neighbours: styles.neighbours(id), animated_cranes: true, look: d.look, ..Extras::default() };
            generate_building(lot, d.land_use, d.floors, d.tier, d.condition, d.seed, kit.rules, kit.palette, &extras)
        }
        None => generate_building(lot, b.land_use, b.floors, b.tier, b.condition, b.seed, kit.rules, kit.palette, &extras),
    };
    // [r5-layered] (flag line 147, Cinder Stacks through the raised ring)
    // Under a deck the building is held to the clearance in the sim; a
    // style whose roof, parapet or rooftop clutter would still reach the
    // deck is drawn squat enough to stand under it.
    let mut g = g;
    if b.land_use != nd_sim::LandUse::Park {
        if let Some(soffit) = world.deck_soffit_over(&lot.polygon) {
            fit_under(&mut g, soffit - world.lot_pad(b.lot) - 0.25);
        }
    }
    Some(g)
}

/// [r5-layered] Squash a building's geometry (heights over its ground) so
/// its top is at most `room` up.
pub(crate) fn fit_under(g: &mut Geometry, room: f32) {
    let top = g.parts.iter().chain(g.detail.iter().flatten()).flat_map(|p| p.positions.iter()).map(|p| p[1]).fold(0.0f32, f32::max);
    if room <= 0.5 || top <= room {
        return;
    }
    let k = room / top;
    for part in g.parts.iter_mut().chain(g.detail.iter_mut().flatten()) {
        for p in part.positions.iter_mut() {
            p[1] *= k;
        }
    }
    for p in g.pads.iter_mut() {
        p.y *= k;
    }
    for p in g.lamps.iter_mut() {
        p.y *= k;
    }
}

/// The chunk a building is drawn in: by its lot centre, so a building and
/// its lot's vacant/park neighbours share cells.
pub(super) fn building_chunk(world: &World, id: BuildingId) -> Option<ChunkKey> {
    // [tenants] A tenant is drawn as part of its building.
    let b = world.buildings.get(id).filter(|b| !b.is_tenant())?;
    Some(chunk_of(world.lots.get(b.lot)?.center))
}

/// [r10-chunk-hitch] The layers the sim's building events redraw, each
/// with its cause (counted once per event): the layer a built or changed
/// building is in now, and the one it was drawn in (`drawn`), when that is
/// another (it became or stopped being live, or the lot moved under it:
/// cause "left_layer").
pub(super) fn changed_layers(world: &World, drawn: &HashMap<BuildingId, LayerKey>, events: &[SimEvent]) -> Vec<(LayerKey, &'static str)> {
    let mut out = Vec::new();
    for e in events {
        if let SimEvent::BuildingBuilt(id) | SimEvent::BuildingChanged(id) = e {
            let cause = redraw_cause(world, e);
            let now = building_layer(world, *id);
            if let Some(k) = now {
                out.push((k, cause));
            }
            if let Some(k) = drawn.get(id).filter(|k| Some(**k) != now) {
                out.push((*k, "left_layer"));
            }
        }
    }
    out
}

/// [r10-chunk-hitch] Why a building event redraws its chunk, for
/// `status.render.chunks.causes`.
fn redraw_cause(world: &World, e: &SimEvent) -> &'static str {
    let SimEvent::BuildingChanged(id) = e else { return "built" };
    match world.buildings.get(*id) {
        None => "changed_gone",
        Some(b) if b.construction.is_some() => "construction",
        Some(b) if b.demolition.is_some() => "demolition",
        Some(b) if b.parking_visual().is_some() => "parking_fill",
        Some(b) if super::chunk_jobs::is_live(b) => "live_other",
        Some(_) => "changed_other",
    }
}

/// [r7-sim-thread] Main-thread time a frame may spend regenerating building
/// chunks before the rest wait for the next frame (at least one is done);
/// [r10-chunk-hitch] only when they are not built on the async pool.
const CHUNK_BUDGET: std::time::Duration = std::time::Duration::from_millis(6);

/// Buildings, batched per [`CHUNK`] cell: one mesh per non-empty surface per
/// cell and layer instead of one per surface per building. A layer is
/// thrown away and regenerated whenever a building in it is built, changed
/// or removed, so an edit costs one street block of procgen rather than the
/// whole city, and a few hundred buildings are a few dozen draw calls per
/// surface.
/// [r10-chunk-hitch] Two layers per cell (`chunk_jobs::Layer`): the settled
/// buildings, and the live ones (sites, demolitions, car parks) that change
/// every few hours. While the sim runs on its thread, layers are regenerated
/// on the async pool (`chunk_jobs::Job`) and swapped in when done; otherwise
/// (`ND_SIM_THREAD=0`, a window writing the city every frame,
/// `ND_CHUNK_JOBS=0`) on the main thread within [`CHUNK_BUDGET`] a frame.
/// A load or a new palette draws everything at once, on every core.
pub(super) fn sync_buildings(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut sim: ResMut<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    mut stats: ResMut<RenderStats>,
) {
    let _t = crate::frame_stats::span("sync_buildings");
    let frame_began = Instant::now();
    let rebuild_all = sim.full_rebuild || assets.rules_changed || assets.palette_changed;
    if sim.full_rebuild || assets.rules_changed {
        // Every building's resolved style and the older neighbours it was
        // picked against live in `Sim::styles` (`nd_procgen::buildings::
        // adjacency`), where the inspector reads them too.
        sim.styles.clear();
        // [fix/review-app] Drawn heights are keyed by building id too.
        sim.building_tops.0.clear();
    }
    if rebuild_all || state.draw_kit.is_none() {
        state.draw_kit = Some(Arc::new(DrawKit { rules: assets.rules.clone(), palette: assets.palette.clone() }));
    }
    let mut dirty: HashSet<LayerKey> = HashSet::new();
    if rebuild_all {
        // [r10-chunk-hitch] Jobs out were drawing the old city or look:
        // dropped (each stops at its next building).
        state.chunk_jobs.clear();
        state.pending_chunks.clear();
        state.chunk_dirty_since.clear();
        for (_, chunk) in state.chunks.drain() {
            for e in chunk.entities {
                commands.entity(e).despawn();
            }
        }
        state.building_chunks.clear();
        state.plant_sites.clear();
        if sim.full_rebuild {
            // Slotmap keys are reused by a loaded world, so an old car mesh
            // could stand in for a new bus; start the movers from scratch.
            for (_, e) in state.citizens.drain() {
                commands.entity(e).despawn();
            }
            for (_, ents) in state.cars.drain() {
                for e in ents {
                    commands.entity(e).despawn();
                }
            }
            for (_, ents) in state.trains.drain() {
                for e in ents {
                    commands.entity(e).despawn();
                }
            }
        }
        for id in sim.world.buildings.keys() {
            if let Some(k) = building_layer(&sim.world, id) {
                dirty.insert(k);
            }
        }
        // [r11-lot-memory] Everything is drawn with the history as it is.
        let w = &sim.world;
        state.memory_gate.same((sim.generation, w.lot_memory.version, w.clock.day()));
        state.memory_drawn = super::lot_memory::marks(w);
    } else {
        // [r11-lot-memory] A trace new, gone or a week further faded: the
        // layers it is drawn in.
        let w = &sim.world;
        if !state.memory_gate.same((sim.generation, w.lot_memory.version, w.clock.day())) {
            let now = super::lot_memory::marks(w);
            for k in super::lot_memory::changed_layers(w, &mut state.memory_drawn, now) {
                *stats.chunks.causes.entry("lot_memory").or_default() += 1;
                dirty.insert(k);
            }
        }
        // Built or changed: the layer the building is in now, and the one
        // it was drawn in (another when it became or stopped being live, or
        // the lot moved under it).
        for (k, cause) in changed_layers(&sim.world, &state.building_chunks, &sim.events) {
            *stats.chunks.causes.entry(cause).or_default() += 1;
            dirty.insert(k);
        }
        // Removed buildings leave no event in some paths (a rebuild of the
        // master plan drops them), so compare against what was drawn.
        let gone: Vec<BuildingId> = state.building_chunks.keys().copied().filter(|id| !sim.world.buildings.contains_key(*id)).collect();
        // Solar and wind plants merge with the ones they touch, so one
        // coming, changing or going redraws the plants round it too.
        let mut plant_touch: Vec<SVec2> = Vec::new();
        for e in &sim.events {
            if let SimEvent::BuildingBuilt(id) | SimEvent::BuildingChanged(id) = e {
                let w = &sim.world;
                if let Some(b) = w.buildings.get(*id).filter(|b| b.plant.is_some_and(|k| k.merges_with_neighbours())) {
                    if let Some(lot) = w.lots.get(b.lot) {
                        plant_touch.push(lot.center);
                    }
                } else if let Some(p) = state.plant_sites.get(id) {
                    // Was a plant, is something else now.
                    plant_touch.push(*p);
                }
            }
        }
        for id in gone {
            if let Some(k) = state.building_chunks.remove(&id) {
                *stats.chunks.causes.entry("removed").or_default() += 1;
                dirty.insert(k);
            }
            if let Some(p) = state.plant_sites.remove(&id) {
                plant_touch.push(p);
            }
        }
        if !plant_touch.is_empty() {
            let w = &sim.world;
            for (id, b) in w.buildings.iter() {
                if !b.plant.is_some_and(|k| k.merges_with_neighbours()) {
                    continue;
                }
                let Some(lot) = w.lots.get(b.lot) else { continue };
                if plant_touch.iter().any(|p| p.distance(lot.center) < PLANT_FIELD_REACH) {
                    if let Some(k) = building_layer(w, id) {
                        *stats.chunks.causes.entry("plant_field").or_default() += 1;
                        dirty.insert(k);
                    }
                }
            }
        }
        // Buildings that exist but were never drawn (missed events) are
        // caught the same way, cheaply, since the map lookup is O(1); not
        // while their chunk is waiting or being drawn already.
        for id in sim.world.buildings.keys() {
            if !state.building_chunks.contains_key(&id) {
                if let Some(k) = building_layer(&sim.world, id) {
                    if !state.pending_chunks.contains(&k) && !state.chunk_jobs.contains_key(&k.0) && dirty.insert(k) {
                        *stats.chunks.causes.entry("undrawn").or_default() += 1;
                    }
                }
            }
        }
    }
    let work = sim.world.clock.work_minutes();
    let rebuilt_before = stats.chunks.rebuilt;
    if rebuild_all {
        // A load or a palette change draws everything now, on every core.
        let groups = chunk_groups(&mut sim, &assets, &dirty);
        let kit = Kit::of(&assets);
        let (world, styles): (&World, &StyleBook) = (&sim.world, &sim.styles);
        let mut ready: Vec<ReadyLayer> = bevy::tasks::ComputeTaskPool::get().scope(|s| {
            for g in &groups {
                s.spawn(async move { build_chunk(world, kit, styles, std::slice::from_ref(g), &|| false).unwrap_or_default() });
            }
        }).into_iter().flatten().collect();
        for r in ready.iter_mut() {
            swap_in(&mut commands, &mut meshes, &mats, &mut sim, &mut state, &mut stats, r, work);
        }
        stats.chunks.full_ms = frame_began.elapsed().as_secs_f32() * 1000.0;
    } else {
        for k in &dirty {
            state.chunk_dirty_since.entry(*k).or_insert(frame_began);
        }
        state.pending_chunks.extend(dirty.drain());
        // Swap in what the jobs finished, every layer of a chunk at once.
        let done: Vec<ChunkKey> = state.chunk_jobs.iter().filter(|(_, j)| j.finished()).map(|(k, _)| *k).collect();
        for c in done {
            let Some(mut job) = state.chunk_jobs.remove(&c) else { continue };
            match job.take() {
                Some(mut layers) => {
                    for r in layers.iter_mut() {
                        swap_in(&mut commands, &mut meshes, &mats, &mut sim, &mut state, &mut stats, r, work);
                    }
                }
                None => {
                    // Recalled by an edit: drawn again from the edited city.
                    stats.chunks.abandoned += 1;
                    let layers = job.layers.clone();
                    state.pending_chunks.extend(layers.into_iter().map(|l| (c, l)));
                }
            }
        }
        // Start what waits, oldest first, a chunk at a time (a chunk with a
        // job out waits for it, so its layers always land together).
        let mut waiting: Vec<(Instant, ChunkKey)> = Vec::new();
        for k in state.pending_chunks.iter() {
            if !state.chunk_jobs.contains_key(&k.0) {
                waiting.push((state.chunk_dirty_since.get(k).copied().unwrap_or(frame_began), k.0));
            }
        }
        waiting.sort_unstable_by_key(|(t, k)| (*t, k.0, k.1));
        waiting.dedup_by_key(|(_, k)| *k);
        let asynchronous = chunk_jobs_on() && sim.threaded && sim.world.threaded() && sim.edit_streak < crate::sim::EDIT_STREAK;
        let room = if asynchronous { MAX_JOBS.saturating_sub(state.chunk_jobs.len()) } else { usize::MAX };
        let start: Vec<ChunkKey> = waiting.into_iter().map(|(_, k)| k).take(room).collect();
        if !start.is_empty() {
            let keys: HashSet<LayerKey> = state.pending_chunks.iter().copied().filter(|k| start.contains(&k.0)).collect();
            let mut groups = chunk_groups(&mut sim, &assets, &keys);
            if asynchronous {
                for chunk in &start {
                    let mine: Vec<(LayerKey, Vec<BuildingId>)> = groups.extract_if(.., |(k, _)| k.0 == *chunk).collect();
                    for (k, _) in &mine {
                        state.pending_chunks.remove(k);
                    }
                    let ids: Vec<BuildingId> = mine.iter().flat_map(|(_, ids)| ids.iter().copied()).collect();
                    let job = Job::start(sim.world.lend(), state.draw_kit.clone().expect("set above"), sim.styles.subset(&ids), mine);
                    state.chunk_jobs.insert(*chunk, job);
                }
            } else {
                // On the main thread, within the budget (at least one chunk).
                let kit = Kit::of(&assets);
                let began = Instant::now();
                for chunk in &start {
                    if began.elapsed() > CHUNK_BUDGET {
                        stats.chunks.deferred += 1;
                        continue;
                    }
                    let mine: Vec<(LayerKey, Vec<BuildingId>)> = groups.extract_if(.., |(k, _)| k.0 == *chunk).collect();
                    for (k, _) in &mine {
                        state.pending_chunks.remove(k);
                    }
                    let mut layers = build_chunk(&sim.world, kit, &sim.styles, &mine, &|| false).unwrap_or_default();
                    for r in layers.iter_mut() {
                        swap_in(&mut commands, &mut meshes, &mats, &mut sim, &mut state, &mut stats, r, work);
                    }
                }
            }
        }
        stats.chunks.pending = state.pending_chunks.len();
        stats.chunks.jobs = state.chunk_jobs.len();
        stats.chunks.frame_ms_max = stats.chunks.frame_ms_max.max(frame_began.elapsed().as_secs_f32() * 1000.0);
    }
    if rebuild_all || stats.chunks.rebuilt != rebuilt_before {
        stats.building_entities = state.chunks.values().map(|c| c.entities.len()).sum();
        stats.building_triangles = state.chunks.values().map(|c| c.triangles).sum();
    }
}

/// [r10-chunk-hitch] `ND_CHUNK_JOBS=0` regenerates building chunks on the
/// main thread, as before (within the budget).
pub(super) fn chunk_jobs_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("ND_CHUNK_JOBS").map_or(true, |v| v != "0"))
}

/// The buildings of each layer in `keys`, in the order the world lists
/// them (a layer left empty is listed too: its meshes go), with the styles
/// brought up to date first.
fn chunk_groups(sim: &mut Sim, assets: &GameAssets, keys: &HashSet<LayerKey>) -> Vec<(LayerKey, Vec<BuildingId>)> {
    {
        let s = &mut *sim;
        s.styles.refresh(&s.world, &assets.rules);
    }
    let mut members: HashMap<LayerKey, Vec<BuildingId>> = keys.iter().map(|k| (*k, Vec::new())).collect();
    for id in sim.world.buildings.keys() {
        if let Some(k) = building_layer(&sim.world, id) {
            if let Some(m) = members.get_mut(&k) {
                m.push(id);
            }
        }
    }
    let mut groups: Vec<(LayerKey, Vec<BuildingId>)> = members.into_iter().collect();
    groups.sort_unstable_by_key(|(k, _)| (k.0 .0, k.0 .1, k.1));
    groups
}

/// Put a finished layer on screen in place of the old one, and keep what
/// it drew: which layer each building is in, drawn heights, depot slots
/// and bays, plant sites.
#[allow(clippy::too_many_arguments)]
fn swap_in(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &Materials, sim: &mut Sim, state: &mut RenderState, stats: &mut RenderStats, r: &mut ReadyLayer, work: u64) {
    if let Some(chunk) = state.chunks.remove(&r.key) {
        for e in chunk.entities {
            commands.entity(e).despawn();
        }
    }
    for d in r.drawn.drain(..) {
        let id = d.id;
        if d.bays.is_empty() {
            sim.depot_bays.remove(&id);
        } else {
            sim.depot_bays.insert(id, d.bays);
        }
        if d.slots.is_empty() {
            if sim.depot_slots.remove(&id).is_some() {
                sim.depot_slots_version += 1;
            }
        } else {
            sim.depot_slots_version += 1;
            let v = sim.depot_slots_version;
            sim.depot_slots.insert(id, (v, d.slots));
        }
        // Tracked even when nothing could be drawn, so it is not tried
        // again every frame as never drawn.
        state.building_chunks.insert(id, r.key);
        if let Some(top) = d.top {
            sim.building_tops.0.insert(id, top);
        }
        if d.ghosts > 0 {
            sim.ghost_walls.insert(id, d.ghosts);
        } else {
            sim.ghost_walls.remove(&id);
        }
        match d.plant {
            Some(p) => state.plant_sites.insert(id, p),
            None => state.plant_sites.remove(&id),
        };
    }
    let chunk = spawn_layer(commands, meshes, mats, r, work);
    if !chunk.entities.is_empty() {
        state.chunks.insert(r.key, chunk);
    }
    stats.chunks.rebuilt += 1;
    stats.chunks.gen_ms += r.ms as f64;
    stats.chunks.chunk_ms_max = stats.chunks.chunk_ms_max.max(r.ms);
    if let Some(t) = state.chunk_dirty_since.remove(&r.key) {
        let lag = t.elapsed().as_secs_f32() * 1000.0;
        stats.chunks.lag_ms += lag as f64;
        stats.chunks.lag_ms_max = stats.chunks.lag_ms_max.max(lag);
    }
}

/// [r4-style] Count the building detail generated and the detail within
/// its distance range of the camera now, for `status.render`.
pub(super) fn detail_stats(state: Res<RenderState>, cams: Query<&GlobalTransform, With<Camera3d>>, mut stats: ResMut<RenderStats>) {
    let eye = cams.iter().next().map(|t| t.translation());
    let (mut all, mut near) = ([0usize; 2], [0usize; 2]);
    for c in state.chunks.values() {
        for d in &c.detail {
            let Some(k) = d.lod.layer() else { continue };
            let range = if d.lod == nd_procgen::Lod::Near { nd_procgen::Lod::NEAR_RANGE } else { nd_procgen::Lod::MID_RANGE };
            all[k] += d.triangles;
            if eye.is_some_and(|e| e.distance(d.at) < range) {
                near[k] += d.triangles;
            }
        }
    }
    stats.detail_triangles = all;
    stats.detail_in_range = near;
}

/// A tower crane's head, turned about its mast by [`animate_cranes`]. Each
/// surface of a head is its own entity, all carrying the same state.
#[derive(Component)]
pub(super) struct CraneHead {
    /// World heading of the jib as generated: the slew is measured from it.
    pub(super) yaw: f32,
    pub(super) seed: u64,
    /// Current slew from `yaw`, radians.
    pub(super) slew: f32,
}

/// Fastest a jib turns on screen, radians per real second, whatever the
/// sim speed: at 1x a swing takes several seconds, at 256x the crane just
/// keeps working, never spinning.
pub(super) const CRANE_SLEW_RATE: f32 = 0.16;

/// Spawn a crane head (from `Geometry::cranes`) at the slew its site is at.
pub(super) fn spawn_crane_head(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &Materials, rig: &nd_procgen::CraneRig, work_minutes: u64) -> Vec<Entity> {
    let slew = nd_procgen::crane_slew(rig.seed, work_minutes);
    let tf = Transform::from_xyz(rig.pos.x, 0.0, rig.pos.y).with_rotation(Quat::from_rotation_y(-(rig.yaw + slew)));
    rig.head
        .iter()
        .map(|(s, m)| {
            let mut e = commands.spawn((Mesh3d(meshes.add(to_mesh(m, !s.is_glow()))), MeshMaterial3d(mats.surface(s)), tf, CraneHead { yaw: rig.yaw, seed: rig.seed, slew }, BuildingTag));
            if s.is_glow() {
                e.insert((NotShadowCaster, NotShadowReceiver));
            }
            e.id()
        })
        .collect()
}

/// Slew the tower cranes: each eases towards the heading of its current
/// lift (`nd_procgen::crane_slew`, from the sim's working minutes), so
/// cranes work through the day and stand still from knocking-off time to
/// the morning, and while the game is paused.
pub(super) fn animate_cranes(sim: Res<Sim>, time: Res<Time>, mut q: Query<(&mut CraneHead, &mut Transform)>) {
    if q.is_empty() || sim.paused {
        return;
    }
    let work = sim.world.clock.work_minutes();
    let dt = time.delta_secs().min(0.1);
    for (mut c, mut tf) in &mut q {
        let diff = nd_procgen::crane_slew(c.seed, work) - c.slew;
        if diff.abs() < 1e-4 {
            continue;
        }
        // Eases in to the new heading; slow and steady otherwise.
        let rate = (0.5 * diff.abs() + 0.02).min(CRANE_SLEW_RATE);
        c.slew += diff.signum() * (rate * dt).min(diff.abs());
        tf.rotation = Quat::from_rotation_y(-(c.yaw + c.slew));
    }
}
