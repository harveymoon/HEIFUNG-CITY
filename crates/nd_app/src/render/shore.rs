//! [waterfront] The private jetties businesses on the water build off
//! their lots (`nd_sim::shore`): a fishing company's jetty with its
//! trawler, a works' quay with a crane and a coaster, a restaurant's deck
//! of tables over the water, a boat rental's pontoon. Beaches and the
//! waterfront trades are buildings (`nd_procgen::buildings::shore`) and
//! go with the buildings.
//!
//! One merged geometry at absolute heights (the decking at the lot's pad,
//! the boats on the water), rebuilt when the jetties change or night
//! falls.

use super::*;

#[derive(Component, Clone)]
pub(super) struct JettyTag;

#[derive(Resource, Default)]
pub(super) struct JettyRender {
    key: Option<u64>,
    ents: Vec<Entity>,
}

pub(super) fn sync_jetties(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, sim: Res<Sim>, mats: Res<Materials>, mut state: ResMut<JettyRender>) {
    let w = &sim.world;
    let night = !(7..20).contains(&w.clock.hour());
    let mut h = DefaultHasher::new();
    night.hash(&mut h);
    for (b, j) in &w.shore.jetties {
        b.hash(&mut h);
        j.kind.hash(&mut h);
        hash_f32s(&mut h, [j.root, j.tip, j.berth]);
        if let Some(x) = w.buildings.get(*b) {
            w.lot_pad(x.lot).to_bits().hash(&mut h);
        }
        alongside(w, *b, j).hash(&mut h);
    }
    let key = h.finish();
    if state.key == Some(key) && !sim.full_rebuild {
        return;
    }
    state.key = Some(key);
    for e in state.ents.drain(..) {
        commands.entity(e).despawn();
    }
    if w.shore.jetties.is_empty() {
        return;
    }
    let mut geo = Geometry::default();
    for (b, j) in &w.shore.jetties {
        let Some(x) = w.buildings.get(*b) else { continue };
        let deck = w.lot_pad(x.lot) + 0.2;
        geo.append(&nd_procgen::buildings::shore::jetty(j, deck, nd_sim::landscape::WATER_LEVEL, night, x.seed, alongside(w, *b, j)));
    }
    state.ents = spawn_geometry(&mut commands, &mut meshes, &mats, &geo, JettyTag);
}

/// [sweep-visual] Whether a jetty's boat is drawn with the jetty: not a
/// fishery's trawler that sails with the fishing fleets
/// (`render::fishing` draws it, at its berth or at sea), nor a cargo
/// quay's coaster (`sync_coasters`).
fn alongside(w: &nd_sim::World, b: BuildingId, j: &nd_sim::shore::Jetty) -> bool {
    match j.kind {
        nd_sim::shore::JettyKind::Fishing => !w.fishing.fleets.get(&b).is_some_and(|f| f.jetty),
        nd_sim::shore::JettyKind::Cargo => false,
        _ => true,
    }
}

#[derive(Component, Clone)]
pub(super) struct CoasterTag;

/// Minutes a coaster lies alongside its quay unloading.
const COASTER_UNLOAD_MIN: f64 = 75.0;
/// Its speed under way (m/s), and the most sim minutes it takes to come in
/// from the map's edge (a long way is sailed faster than it would be).
const COASTER_MPS: f32 = 5.0;
const COASTER_MAX_SAIL_MIN: f64 = 150.0;

#[derive(Default)]
pub(super) struct CoasterState {
    mesh: Option<Handle<Mesh>>,
    /// Per quay: its coaster, the berth its route was found for, and the
    /// route from the berth out to the map's edge (empty: none by water).
    boats: HashMap<BuildingId, (Entity, SVec2, Vec<SVec2>)>,
}

/// Where a quay's coaster is at sim time `now` (minutes): alongside while
/// it unloads the last load, else coming in for the next load due, else
/// sailing out after the last; none when no load is near. Position along
/// `route` (from the berth outward: 0 is alongside) and whether it is
/// heading in.
fn coaster_at(now: f64, due_next: Option<u64>, last: u64, sail: f64) -> Option<(f64, bool)> {
    let since = now - last as f64;
    if last > 0 && (0.0..COASTER_UNLOAD_MIN).contains(&since) {
        return Some((0.0, true));
    }
    if let Some(d) = due_next {
        let left = d as f64 - now;
        if (0.0..sail).contains(&left) {
            return Some((left / sail, true));
        }
    }
    if last > 0 && since >= COASTER_UNLOAD_MIN && since < COASTER_UNLOAD_MIN + sail {
        return Some(((since - COASTER_UNLOAD_MIN) / sail, false));
    }
    None
}

/// [sweep-visual] Cargo quays' coasters drawn coming and going: in from the
/// map's edge by water ahead of each load (`shore.inbound`), alongside
/// while it is landed, then out again; the quay is empty between.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_coasters(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut st: Local<CoasterState>,
    mut q: Query<(&mut Transform, &mut Visibility), With<CoasterTag>>,
) {
    let w = &sim.world;
    let quays: Vec<(BuildingId, &nd_sim::shore::Jetty)> = w.shore.jetties.iter().filter(|(_, j)| j.kind == nd_sim::shore::JettyKind::Cargo).map(|(b, j)| (*b, j)).collect();
    if quays.is_empty() && st.boats.is_empty() {
        return;
    }
    let mesh = st.mesh.get_or_insert_with(|| meshes.add(shared_mesh(&nd_procgen::terrain::boat_mesh(nd_sim::ports::ShipKind::Freighter, &assets.palette), true))).clone();
    let gone: Vec<BuildingId> = st.boats.keys().copied().filter(|b| sim.full_rebuild || !quays.iter().any(|(q, j)| q == b && st.boats.get(b).is_some_and(|x| x.1 == j.berth))).collect();
    for b in gone {
        if let Some((e, _, _)) = st.boats.remove(&b) {
            commands.entity(e).despawn();
        }
    }
    for (b, j) in &quays {
        if !st.boats.contains_key(b) {
            let route = w.landscape.water_route(j.berth, None).unwrap_or_default();
            let e = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mats.surface(Surface::Panel)), Transform::IDENTITY, Visibility::Hidden, CoasterTag)).id();
            st.boats.insert(*b, (e, j.berth, route));
        }
    }
    let now = w.clock.tick as f64 + sim.frac() as f64;
    let mpt = w.config.metres_per_tick(COASTER_MPS).max(0.1) as f64;
    for (b, j) in &quays {
        let Some((e, _, route)) = st.boats.get(b) else { continue };
        let Ok((mut tf, mut vis)) = q.get_mut(*e) else { continue };
        let len = nd_sim::geom::path_length(route) as f64;
        let sail = (len / mpt).min(COASTER_MAX_SAIL_MIN).max(1.0);
        let due_next = w.shore.inbound.iter().filter(|(x, _, t)| x == b && *t as f64 >= now).map(|(_, _, t)| *t).min();
        let at = coaster_at(now, due_next, j.last_load, if route.len() >= 2 { sail } else { 0.0 });
        let Some((k, inbound)) = at else {
            *vis = Visibility::Hidden;
            continue;
        };
        let (p, dir) = if k <= 0.0 || route.len() < 2 {
            // Alongside: bow out along the quay, its side off the quay's
            // edge and its stern clear of the bank.
            let along = (j.tip - j.root).normalize_or_zero();
            let foot = j.root + along * (j.berth - j.root).dot(along);
            let off = (j.berth - foot).normalize_or_zero();
            (j.berth + off * 1.5 + along * 4.0, along)
        } else {
            let (p, d) = nd_sim::geom::point_along(route, (k * len) as f32);
            (p, if inbound { -d } else { d })
        };
        *vis = Visibility::Inherited;
        tf.translation = Vec3::new(p.x, nd_sim::landscape::WATER_LEVEL, p.y);
        tf.rotation = Quat::from_rotation_y(-dir.y.atan2(dir.x));
        // A coaster: a short freighter.
        tf.scale = Vec3::new(30.0 / 70.0, 0.7, 7.0 / 12.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_coaster_comes_in_lies_alongside_and_sails() {
        // Due at 1000, a 100-minute passage.
        assert_eq!(coaster_at(850.0, Some(1000), 0, 100.0), None, "not yet");
        let (k, inbound) = coaster_at(950.0, Some(1000), 0, 100.0).unwrap();
        assert!(inbound && (k - 0.5).abs() < 1e-9, "half way in");
        // Landed at 1000: alongside, then out.
        assert_eq!(coaster_at(1030.0, None, 1000, 100.0), Some((0.0, true)));
        let (k, inbound) = coaster_at(1000.0 + COASTER_UNLOAD_MIN + 25.0, None, 1000, 100.0).unwrap();
        assert!(!inbound && (k - 0.25).abs() < 1e-9, "a quarter of the way out");
        assert_eq!(coaster_at(1000.0 + COASTER_UNLOAD_MIN + 120.0, None, 1000, 100.0), None, "gone");
        // No route by water: only alongside.
        assert_eq!(coaster_at(950.0, Some(1000), 0, 0.0), None);
    }
}
