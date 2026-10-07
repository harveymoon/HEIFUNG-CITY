//! Sun-tracking solar panels. A solar farm's generator draws the pedestals
//! and leaves the panels out (`Extras::animated_rotors`); here every
//! tracker gets a hexagonal panel (`nd_procgen::solar_head`) on its pivot,
//! turned to face the sun: the same sun the city is lit by and solar output
//! follows (`nd_sim::power::sun_direction`). Tilt stops at
//! `SOLAR_MAX_TILT`, so at dawn and dusk the panels face the low sun as far
//! as they go; as the sun sets they lie back down flat for the night (the
//! stow position, which also keeps them out of the wind), and wake facing
//! east in the morning.
//!
//! Every panel faces the same way, so one rotation serves the whole city.
//! It is recomputed only when the facing moves by a noticeable step, which
//! at normal speed is a few times a minute.

use bevy::prelude::*;
use nd_procgen::{solar_head, solar_trackers, Surface, SOLAR_MAX_TILT};
use nd_sim::power::PowerPlantKind;
use nd_sim::{BuildingId, SimEvent};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use super::{shared_mesh, smoothstep, Lighting, Materials};
use crate::sim::Sim;

#[derive(Component)]
pub(super) struct SolarPanel;

#[derive(Default)]
pub(super) struct SolarState {
    /// Panel entities per farm, with a hash of what they were placed for.
    farms: HashMap<BuildingId, (u64, Vec<Entity>)>,
    /// The panel's meshes, one per surface, shared by every tracker.
    meshes: Vec<(Surface, Handle<Mesh>)>,
    /// The farms standing when the panels were last placed: a farm coming
    /// or going can rearrange the fields next to it.
    farms_seen: Vec<BuildingId>,
    /// The facing the panels were last turned to.
    facing: Option<Vec3>,
}

/// Which way the panels face at `day_fraction`: straight up at night; by
/// day towards the sun, tilted at most [`SOLAR_MAX_TILT`] from flat, rising
/// out of the stow as the sun clears the horizon.
pub(super) fn panel_facing(day_fraction: f32) -> Vec3 {
    let Some(d) = nd_sim::power::sun_direction(day_fraction) else { return Vec3::Y };
    let d = Vec3::from(d);
    let wake = smoothstep(0.0, 0.12, d.y);
    let tilt = d.y.clamp(-1.0, 1.0).acos().min(SOLAR_MAX_TILT) * wake;
    let across = Vec2::new(d.x, d.z).normalize_or_zero();
    Vec3::new(across.x * tilt.sin(), tilt.cos(), across.y * tilt.sin()).normalize()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_solar(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    mats: Res<Materials>,
    lighting: Res<Lighting>,
    mut state: Local<SolarState>,
    mut q: Query<&mut Transform, With<SolarPanel>>,
) {
    let w = &sim.world;
    if state.meshes.is_empty() {
        let g = solar_head();
        state.meshes = g.iter().map(|(s, m)| (s, meshes.add(shared_mesh(m, !s.is_glow())))).collect();
    }
    if sim.full_rebuild {
        for (_, (_, ents)) in state.farms.drain() {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    let t = if lighting.follow_clock { w.clock.day_fraction() } else { (lighting.hour / 24.0).rem_euclid(1.0) };
    let facing = panel_facing(t);
    let turn = Quat::from_rotation_arc(Vec3::Y, facing);
    // Which farms stand (built, not still clearing the old building), and
    // where their trackers go. Rechecked only when buildings came, changed
    // or went, since the field search walks every building.
    let seen: Vec<BuildingId> = w
        .buildings
        .iter()
        .filter(|(_, b)| b.plant == Some(PowerPlantKind::Solar) && b.construction.is_none() && b.demolition.is_none() && w.lots.contains_key(b.lot))
        .map(|(id, _)| id)
        .collect();
    let recheck = sim.full_rebuild || seen != state.farms_seen || sim.events.iter().any(|e| matches!(e, SimEvent::BuildingBuilt(_) | SimEvent::BuildingChanged(_)));
    for &id in seen.iter().filter(|_| recheck) {
        let b = &w.buildings[id];
        let lot = &w.lots[b.lot];
        let field = super::plant_field(w, id);
        let pad = w.lot_pad(b.lot);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        pad.to_bits().hash(&mut h);
        for l in w.plant_cluster(id) {
            for p in &w.lots[l].polygon {
                p.x.to_bits().hash(&mut h);
                p.y.to_bits().hash(&mut h);
            }
        }
        let key = h.finish();
        if state.farms.get(&id).is_some_and(|(k, _)| *k == key) {
            continue;
        }
        if let Some((_, ents)) = state.farms.remove(&id) {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
        let mut ents = Vec::new();
        for t in solar_trackers(lot, field.as_ref()) {
            let tf = Transform::from_xyz(t.pos.x, pad + t.hub, t.pos.y).with_rotation(turn);
            for (s, mesh) in &state.meshes {
                let mut e = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mats.surface(*s)), tf, SolarPanel));
                if s.is_glow() {
                    e.insert((bevy::light::NotShadowCaster, bevy::light::NotShadowReceiver));
                }
                ents.push(e.id());
            }
        }
        state.farms.insert(id, (key, ents));
    }
    let gone: Vec<BuildingId> = state.farms.keys().copied().filter(|id| !seen.contains(id)).collect();
    state.farms_seen = seen;
    for id in gone {
        if let Some((_, ents)) = state.farms.remove(&id) {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    // Turn: only when the facing moved by about a fifth of a degree, so a
    // city of panels is not rewritten every frame. New panels are spawned
    // already turned.
    if state.farms.is_empty() || state.facing.is_some_and(|f| f.dot(facing) > 0.999_994) {
        return;
    }
    state.facing = Some(facing);
    for mut tf in &mut q {
        tf.rotation = turn;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panels_lie_flat_at_night_follow_the_sun_and_stop_at_their_tilt() {
        assert_eq!(panel_facing(0.0), Vec3::Y);
        assert_eq!(panel_facing(0.95), Vec3::Y);
        let morning = panel_facing(0.32);
        assert!(morning.x > 0.3, "faces east in the morning: {morning}");
        assert!(morning.y >= SOLAR_MAX_TILT.cos() - 1e-4, "never tilts past its stop");
        let evening = panel_facing(0.68);
        assert!(evening.x < -0.3, "faces west in the evening: {evening}");
        assert!(panel_facing(0.5).y > 0.99, "flat under the noon sun");
        // Just after sunrise it is still waking from the stow.
        assert!(panel_facing(0.2505).y > 0.95);
    }
}
