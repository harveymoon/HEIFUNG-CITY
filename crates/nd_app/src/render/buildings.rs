//! Buildings, batched per chunk, and the cranes over building sites.
//!
//! [refactor/app] Split out of `render.rs`.

use super::*;

/// Generate one building's geometry into the chunk's merged geometry.
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

pub(super) fn append_building(geo: &mut Geometry, world: &World, assets: &GameAssets, styles: &StyleBook, id: BuildingId) -> Option<f32> {
    let b = world.buildings.get(id)?;
    let mut g = building_geometry(world, assets, styles, id)?;
    // [climate] Snow on the roofs in a cold climate.
    nd_procgen::climate::snow_cover(&mut g, assets.palette.snow);
    lights::tag_windows(&mut g, lights::slot_of(id));
    let top = mass_top(&g);
    // Every building stands on its lot's levelled pad.
    nd_procgen::terrain::raise_geometry(&mut g, world.lot_pad(b.lot));
    geo.append(&g);
    Some(top)
}

/// One building's geometry as drawn, before it is raised onto its lot's
/// pad (`World::lot_pad`); [social] murals read their wall off it.
pub(crate) fn building_geometry(world: &World, assets: &GameAssets, styles: &StyleBook, id: BuildingId) -> Option<Geometry> {
    let b = world.buildings.get(id)?;
    // [land] A building over merged plots is drawn on their combined
    // footprint (`World::footprint`): one building, no seams.
    let footprint = world.footprint(id)?;
    let lot: &nd_sim::Lot = &footprint;
    let field = if b.construction.is_none() && b.demolition.is_none() { plant_field(world, id) } else { None };
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
        ..nd_procgen::style_extras(world, id, styles.neighbours(id))
    };
    let g = match &b.demolition {
        // Coming down: what stood there (which, during a redevelopment or a
        // civic take-over, is not what the building's own fields describe),
        // cut down to what is left, with the demolition site around it.
        Some(d) => {
            let extras = Extras { powered: b.powered, park: lot.park.as_ref(), demolition: Some(d.progress), neighbours: styles.neighbours(id), animated_cranes: true, look: d.look, ..Extras::default() };
            generate_building(lot, d.land_use, d.floors, d.tier, d.condition, d.seed, &assets.rules, &assets.palette, &extras)
        }
        None => generate_building(lot, b.land_use, b.floors, b.tier, b.condition, b.seed, &assets.rules, &assets.palette, &extras),
    };
    Some(g)
}

/// The chunk a building is drawn in: by its lot centre, so a building and
/// its lot's vacant/park neighbours share cells.
pub(super) fn building_chunk(world: &World, id: BuildingId) -> Option<ChunkKey> {
    let b = world.buildings.get(id)?;
    Some(chunk_of(world.lots.get(b.lot)?.center))
}

/// Buildings, batched per [`CHUNK`] cell: one mesh per non-empty surface per
/// cell instead of one per surface per building. A cell is thrown away and
/// regenerated whenever a building in it is built, changed or removed, so
/// an edit costs one street block of procgen rather than the whole city, and
/// a few hundred buildings are a few dozen draw calls per surface.
pub(super) fn sync_buildings(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut sim: ResMut<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    mut stats: ResMut<RenderStats>,
) {
    let rebuild_all = sim.full_rebuild || assets.rules_changed || assets.palette_changed;
    if sim.full_rebuild || assets.rules_changed {
        // Every building's resolved style and the older neighbours it was
        // picked against live in `Sim::styles` (`nd_procgen::buildings::
        // adjacency`), where the inspector reads them too.
        sim.styles.clear();
        // [fix/review-app] Drawn heights are keyed by building id too.
        sim.building_tops.0.clear();
    }
    let mut dirty: HashSet<ChunkKey> = HashSet::new();
    if rebuild_all {
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
            if let Some(k) = building_chunk(&sim.world, id) {
                dirty.insert(k);
            }
        }
    } else {
        // Built or changed: the chunk the building lives in now, and the one
        // it was drawn in (the same unless the lot moved under it).
        for e in &sim.events {
            if let SimEvent::BuildingBuilt(id) | SimEvent::BuildingChanged(id) = e {
                if let Some(k) = building_chunk(&sim.world, *id) {
                    dirty.insert(k);
                }
                if let Some(k) = state.building_chunks.get(id) {
                    dirty.insert(*k);
                }
            }
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
                    if let Some(k) = building_chunk(w, id) {
                        dirty.insert(k);
                    }
                }
            }
        }
        // Buildings that exist but were never drawn (missed events) are
        // caught the same way, cheaply, since the map lookup is O(1).
        for id in sim.world.buildings.keys() {
            if !state.building_chunks.contains_key(&id) {
                if let Some(k) = building_chunk(&sim.world, id) {
                    dirty.insert(k);
                }
            }
        }
    }
    let touched = !dirty.is_empty();
    if touched {
        {
            let s = &mut *sim;
            s.styles.refresh(&s.world, &assets.rules);
        }
        // Bucket every building once; only the dirty cells are regenerated.
        let mut members: HashMap<ChunkKey, Vec<BuildingId>> = HashMap::new();
        for id in sim.world.buildings.keys() {
            if let Some(k) = building_chunk(&sim.world, id) {
                if dirty.contains(&k) {
                    members.entry(k).or_default().push(id);
                }
            }
        }
        for k in dirty {
            if let Some(chunk) = state.chunks.remove(&k) {
                for e in chunk.entities {
                    commands.entity(e).despawn();
                }
            }
            let Some(ids) = members.remove(&k) else { continue };
            let mut geo = Geometry::default();
            for &id in &ids {
                if let Some(top) = append_building(&mut geo, &sim.world, &assets, &sim.styles, id) {
                    sim.building_tops.0.insert(id, top);
                    state.building_chunks.insert(id, k);
                    let w = &sim.world;
                    match w.buildings[id].plant.filter(|k| k.merges_with_neighbours()).and_then(|_| w.lots.get(w.buildings[id].lot)) {
                        Some(lot) => state.plant_sites.insert(id, lot.center),
                        None => state.plant_sites.remove(&id),
                    };
                }
            }
            let mut entities = spawn_building_geometry(&mut commands, &mut meshes, &mats, &geo);
            let work = sim.world.clock.work_minutes();
            for rig in &geo.cranes {
                entities.extend(spawn_crane_head(&mut commands, &mut meshes, &mats, rig, work));
            }
            state.chunks.insert(k, ChunkMeshes { entities, triangles: geo.triangle_count() });
        }
    }
    if rebuild_all || touched {
        stats.building_entities = state.chunks.values().map(|c| c.entities.len()).sum();
        stats.building_triangles = state.chunks.values().map(|c| c.triangles).sum();
    }
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
