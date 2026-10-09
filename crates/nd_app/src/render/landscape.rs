//! The landscape's ground, water and works, and the ships on the water.
//!
//! [refactor/app] Split out of `render.rs`.

use super::*;

#[derive(Component)]
pub(super) struct WaterTag;
#[derive(Component, Clone)]
pub(super) struct WaterworksTag;
#[derive(Component)]
pub(super) struct ShipTag;

#[derive(Resource, Default)]
pub(super) struct LandscapeRender {
    /// What the ground, water and works were built for.
    pub(super) signature: Option<u64>,
    /// [refactor/app] The counters `signature` was hashed under.
    pub(super) gate: Gate<(u64, u64, u64, usize, usize, u64, bool)>,
    pub(super) water_mat: Option<Handle<StandardMaterial>>,
    /// [economy] One mesh per `ShipKind` (freighter, ferry, trawler, yacht, sailboat).
    pub(super) ship_meshes: Option<[Handle<Mesh>; 5]>,
    /// Each drawn ship and the kind its mesh shows.
    pub(super) ships: HashMap<nd_sim::ports::ShipId, (Entity, nd_sim::ports::ShipKind)>,
}

/// [fix/review-app] Bring `drawn` (sim id to entity, and what it was drawn
/// as) in line with the `live` sim collection. Returns the entities to
/// despawn (gone from the sim, drawn as something else, or everything on a
/// `full_rebuild`) and the ids to spawn, which the caller inserts. A loaded
/// city reuses slotmap ids: without the rebuild and kind checks a new
/// ferry kept the old city's freighter under the same id.
pub(super) fn reconcile<K: Copy + Eq + Hash, V: Copy + PartialEq>(drawn: &mut HashMap<K, (Entity, V)>, live: impl Iterator<Item = (K, V)>, full_rebuild: bool) -> (Vec<Entity>, Vec<(K, V)>) {
    let mut despawn: Vec<Entity> = Vec::new();
    if full_rebuild {
        despawn.extend(drawn.drain().map(|(_, (e, _))| e));
    }
    let mut keep: HashSet<K> = HashSet::new();
    let mut spawn = Vec::new();
    for (k, v) in live {
        keep.insert(k);
        match drawn.get(&k) {
            Some((_, was)) if *was == v => {}
            Some(&(e, _)) => {
                despawn.push(e);
                drawn.remove(&k);
                spawn.push((k, v));
            }
            None => spawn.push((k, v)),
        }
    }
    drawn.retain(|k, (e, _)| {
        let alive = keep.contains(k);
        if !alive {
            despawn.push(*e);
        }
        alive
    });
    (despawn, spawn)
}

pub(super) fn landscape_signature(w: &World) -> u64 {
    let mut h = DefaultHasher::new();
    w.landscape.params.seed.hash(&mut h);
    format!("{:?}", w.landscape.params.preset).hash(&mut h);
    w.landscape.rivers.len().hash(&mut h);
    w.roads.version.hash(&mut h);
    w.lots.len().hash(&mut h);
    w.ports.ports.len().hash(&mut h);
    w.buildings.values().filter(|b| b.plant.is_some()).count().hash(&mut h);
    w.landscape.is_flat().hash(&mut h);
    w.relief.hash(&mut h);
    h.finish()
}

/// The ground of a landscape (graded to the roads and lots), the water
/// sheet, and the bridges, piers, weirs and vents. Rebuilt when roads, lots,
/// docks or plants change, since the ground is graded to them.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_landscape(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut state: ResMut<LandscapeRender>,
    terrain: Query<Entity, With<TerrainTag>>,
    water: Query<Entity, With<WaterTag>>,
    works: Query<Entity, With<WaterworksTag>>,
) {
    let w = &sim.world;
    let fresh = !assets.palette_changed && !sim.full_rebuild;
    if state.gate.same((sim.generation, w.landscape.version(), w.roads.version, w.lots.len(), w.ports.ports.len(), w.buildings.version(), w.relief)) && state.signature.is_some() && fresh {
        return;
    }
    let sig = landscape_signature(w);
    if state.signature == Some(sig) && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    state.signature = Some(sig);
    despawn_all(&mut commands, &water);
    despawn_all(&mut commands, &works);
    if w.landscape.is_flat() {
        // No terrain or water to draw, but viaducts and flyovers the
        // player built stand on the flat ground.
        if w.relief {
            let geo = nd_procgen::terrain::waterworks(w, &assets.palette);
            spawn_geometry(&mut commands, &mut meshes, &mats, &geo, WaterworksTag);
        }
        return;
    }
    despawn_all(&mut commands, &terrain);
    let ground = nd_procgen::terrain::build_landscape_terrain(w, &assets.palette, 8.0);
    commands.spawn((Mesh3d(meshes.add(to_mesh(&ground, true))), MeshMaterial3d(mats.surface(Surface::Dirt)), Transform::IDENTITY, TerrainTag));
    let water_mat = state
        .water_mat
        .get_or_insert_with(|| {
            materials.add(StandardMaterial {
                base_color: Color::WHITE,
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.06,
                reflectance: 0.6,
                metallic: 0.1,
                ..default()
            })
        })
        .clone();
    let sheet = nd_procgen::terrain::build_water(w, &assets.palette);
    if !sheet.is_empty() {
        commands.spawn((Mesh3d(meshes.add(to_mesh(&sheet, false))), MeshMaterial3d(water_mat), Transform::IDENTITY, NotShadowCaster, WaterTag));
    }
    let geo = nd_procgen::terrain::waterworks(w, &assets.palette);
    spawn_geometry(&mut commands, &mut meshes, &mats, &geo, WaterworksTag);
}

/// [economy] The ship kinds in `ship_meshes` order.
pub(super) const SHIP_KINDS: [nd_sim::ShipKind; 5] = [nd_sim::ShipKind::Freighter, nd_sim::ShipKind::Ferry, nd_sim::ShipKind::FishingBoat, nd_sim::ShipKind::Yacht, nd_sim::ShipKind::Sailboat];

/// Freighters, ferries and (economy) boats along their routes, blended between ticks.
pub(super) fn sync_ships(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, sim: Res<Sim>, assets: Res<GameAssets>, mats: Res<Materials>, mut state: ResMut<LandscapeRender>, mut q: Query<&mut Transform, With<ShipTag>>) {
    let ships = &sim.world.ports.ships;
    if ships.is_empty() && state.ships.is_empty() {
        return;
    }
    let handles = state
        .ship_meshes
        .get_or_insert_with(|| SHIP_KINDS.map(|k| meshes.add(shared_mesh(&nd_procgen::terrain::boat_mesh(k, &assets.palette), true))))
        .clone();
    let (despawn, spawn) = reconcile(&mut state.ships, ships.iter().map(|(id, s)| (id, s.kind)), sim.full_rebuild);
    for e in despawn {
        commands.entity(e).despawn();
    }
    for (id, kind) in spawn {
        let mesh = handles[SHIP_KINDS.iter().position(|k| *k == kind).unwrap_or(1)].clone();
        let e = commands.spawn((Mesh3d(mesh), MeshMaterial3d(mats.surface(Surface::Panel)), Transform::IDENTITY, ShipTag)).id();
        state.ships.insert(id, (e, kind));
    }
    let f = sim.frac();
    for (id, (e, _)) in state.ships.iter() {
        let Some(ship) = ships.get(*id) else { continue };
        let s = ship.prev_s + (ship.s - ship.prev_s) * f;
        let (p, dir) = ship.pose_at(s);
        if let Ok(mut tf) = q.get_mut(*e) {
            tf.translation = Vec3::new(p.x, nd_sim::landscape::WATER_LEVEL, p.y);
            tf.rotation = Quat::from_rotation_y(-dir.y.atan2(dir.x));
        }
    }
}

#[cfg(test)]
mod reconcile_tests {
    use super::*;

    fn e(n: u32) -> Entity {
        Entity::from_raw_u32(n).unwrap()
    }

    /// [fix/review-app] A reloaded city reuses ids: the drawn entities must
    /// not survive a `full_rebuild`, and an id drawn as another kind is
    /// redrawn even without one (the ship that kept the old city's model).
    #[test]
    fn reloaded_ids_are_redrawn() {
        let mut drawn: HashMap<u32, (Entity, char)> = HashMap::new();
        let (gone, spawn) = reconcile(&mut drawn, [(1, 'f'), (2, 'y')].into_iter(), false);
        assert!(gone.is_empty());
        assert_eq!(spawn, vec![(1, 'f'), (2, 'y')]);
        drawn.insert(1, (e(10), 'f'));
        drawn.insert(2, (e(11), 'y'));
        // Steady state: nothing to do.
        let (gone, spawn) = reconcile(&mut drawn, [(1, 'f'), (2, 'y')].into_iter(), false);
        assert!(gone.is_empty() && spawn.is_empty());
        // Same id, other kind; 2 gone.
        let (gone, spawn) = reconcile(&mut drawn, [(1, 's')].into_iter(), false);
        assert_eq!(gone.len(), 2);
        assert_eq!(spawn, vec![(1, 's')]);
        assert!(drawn.is_empty());
        // A load with the same ids and kinds still redraws everything.
        drawn.insert(1, (e(12), 's'));
        let (gone, spawn) = reconcile(&mut drawn, [(1, 's')].into_iter(), true);
        assert_eq!(gone, vec![e(12)]);
        assert_eq!(spawn, vec![(1, 's')]);
    }
}
