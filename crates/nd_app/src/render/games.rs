//! [games] Stadium game days drawn: the crowd in the stands while the
//! gates are open and the game is on, the two teams on a floodlit pitch
//! during the game, fans milling at the gates coming and going, and the
//! away fans' cars parked in rows on the plaza (`World::game_phase`,
//! `World::game_cars_parked`). The dressing comes with the stadium's
//! geometry (`nd_procgen::GameRig`), spawned hidden with its chunk.

use super::*;
use nd_sim::world::games::GamePhase;

/// Which part of a stadium's game-day dressing an entity is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GamePart {
    Stands,
    Pitch,
    Outside,
}

#[derive(Component)]
pub(super) struct GameDress {
    pub(super) building: BuildingId,
    pub(super) part: GamePart,
}

/// The bays a stadium's away fans park in (an entity with no mesh, kept
/// with the stadium's chunk).
#[derive(Component)]
pub(super) struct GameBays {
    pub(super) building: BuildingId,
    pub(super) slots: Vec<(Vec3, f32)>,
}

/// Spawn a stadium's game-day dressing, hidden: one entity per surface of
/// each part, and its car bays.
pub(super) fn spawn_game_rig(commands: &mut Commands, meshes: &mut Assets<Mesh>, mats: &Materials, rig: &nd_procgen::GameRig) -> Vec<Entity> {
    let Some(building) = rig.building else { return Vec::new() };
    let mut out = Vec::new();
    for (part, geo) in [(GamePart::Stands, &rig.stands), (GamePart::Pitch, &rig.pitch), (GamePart::Outside, &rig.outside)] {
        for (s, m) in geo.iter() {
            let mut e = commands.spawn((Mesh3d(meshes.add(to_mesh(m, !s.is_glow()))), MeshMaterial3d(mats.surface(s)), Transform::IDENTITY, Visibility::Hidden, NotShadowCaster, GameDress { building, part }, BuildingTag));
            if s.is_glow() {
                e.insert(NotShadowReceiver);
            }
            out.push(e.id());
        }
    }
    out.push(commands.spawn((Transform::IDENTITY, Visibility::Hidden, GameBays { building, slots: rig.slots.clone() }, BuildingTag)).id());
    out
}

/// Is `part` shown in `phase`?
pub(super) fn shown(part: GamePart, phase: Option<GamePhase>) -> bool {
    match (part, phase) {
        (_, None) => false,
        (GamePart::Stands, Some(p)) => p != GamePhase::Leaving,
        (GamePart::Pitch, Some(p)) => p == GamePhase::Live,
        (GamePart::Outside, Some(p)) => p != GamePhase::Live,
    }
}

/// Bays drawn per step of the parked count (the cars mesh is rebuilt when
/// it crosses one).
const BAY_STEP: u32 = 4;

/// Show the dressing by each stadium's game phase, and park the away
/// fans' cars in its bays.
#[allow(clippy::type_complexity)]
pub(super) fn sync_game_days(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    mats: Res<Materials>,
    mut dress: Query<(&GameDress, &mut Visibility), Without<GameBays>>,
    bays: Query<&GameBays>,
    mut cars: Local<HashMap<BuildingId, (u32, Entity)>>,
) {
    let w = &sim.world;
    if sim.full_rebuild {
        for (_, (_, e)) in cars.drain() {
            commands.entity(e).despawn();
        }
    }
    for (d, mut v) in dress.iter_mut() {
        let want = if shown(d.part, w.game_phase(d.building)) { Visibility::Inherited } else { Visibility::Hidden };
        if *v != want {
            *v = want;
        }
    }
    let mut seen = HashSet::new();
    for b in bays.iter() {
        seen.insert(b.building);
        let n = (w.game_cars_parked(b.building).div_ceil(BAY_STEP) * BAY_STEP).min(b.slots.len() as u32);
        let have = cars.get(&b.building).map(|c| c.0);
        if have == Some(n) {
            continue;
        }
        if let Some((_, e)) = cars.remove(&b.building) {
            commands.entity(e).despawn();
        }
        if n == 0 {
            continue;
        }
        let mut m = MeshData::default();
        for (k, (p, yaw)) in b.slots.iter().take(n as usize).enumerate() {
            let paint = CAR_PAINT[(nd_sim::hash::mix(k as u64, 0xCA75) % CAR_PAINT.len() as u64) as usize];
            m.boxed(*p + Vec3::Y * 0.55, Vec3::new(4.2, 0.8, 1.8), *yaw, paint);
            m.boxed(*p + Vec3::Y * 1.2, Vec3::new(2.2, 0.55, 1.6), *yaw, [0.12, 0.14, 0.17, 1.0]);
        }
        let e = commands.spawn((Mesh3d(meshes.add(shared_mesh(&m, true))), MeshMaterial3d(mats.surface(Surface::Panel)), Transform::IDENTITY, BuildingTag)).id();
        cars.insert(b.building, (n, e));
    }
    let gone: Vec<BuildingId> = cars.keys().copied().filter(|b| !seen.contains(b)).collect();
    for b in gone {
        if let Some((_, e)) = cars.remove(&b) {
            commands.entity(e).despawn();
        }
    }
}

const CAR_PAINT: [[f32; 4]; 7] = [[0.7, 0.1, 0.1, 1.0], [0.1, 0.2, 0.55, 1.0], [0.8, 0.8, 0.82, 1.0], [0.08, 0.08, 0.09, 1.0], [0.45, 0.47, 0.5, 1.0], [0.85, 0.6, 0.1, 1.0], [0.15, 0.4, 0.25, 1.0]];

#[cfg(test)]
mod tests {
    use super::*;

    /// G18: the stands fill from the gates opening, the teams are out only
    /// for the game, the crowd mills outside coming and going.
    #[test]
    fn the_dressing_follows_the_game() {
        use GamePart::*;
        use GamePhase::*;
        assert!(!shown(Stands, None) && !shown(Pitch, None) && !shown(Outside, None));
        assert!(shown(Stands, Some(Gates)) && !shown(Pitch, Some(Gates)) && shown(Outside, Some(Gates)));
        assert!(shown(Stands, Some(Live)) && shown(Pitch, Some(Live)) && !shown(Outside, Some(Live)));
        assert!(!shown(Stands, Some(Leaving)) && !shown(Pitch, Some(Leaving)) && shown(Outside, Some(Leaving)));
    }
}
