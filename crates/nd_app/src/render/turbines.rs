//! Spinning wind turbine rotors. The farm's generator draws the masts and
//! leaves the rotors out (`Extras::animated_rotors`); here every turbine
//! gets a helical rotor entity on top of its mast, turned about the vertical
//! at a speed set by its own farm's output ([grid-weather]: dispatched over
//! nameplate), so a farm on a sheltered site, one turned down because
//! nobody wants the power, or one braked in a storm (cut-out) turns slowly
//! or stands still while others run. They stop while the game is paused,
//! and spin up and down smoothly.

use bevy::prelude::*;
use nd_procgen::{wind_rotor, wind_turbines, Surface};
use nd_sim::power::PowerPlantKind;
use nd_sim::{BuildingId, SimEvent};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use super::{shared_mesh, Materials};
use crate::sim::Sim;

/// Below this share of nameplate the rotors stand still.
pub(super) const IDLE_SHARE: f32 = 0.03;

#[derive(Component)]
pub(super) struct TurbineRotor {
    phase: f32,
    farm: BuildingId,
}

#[derive(Default)]
pub(super) struct TurbineState {
    /// Rotor entities per wind farm, with a hash of what they were placed for.
    farms: HashMap<BuildingId, (u64, Vec<Entity>)>,
    /// The rotor's meshes, one per surface, shared by every turbine.
    meshes: Vec<(Surface, Handle<Mesh>)>,
    /// Rotor angle and angular speed per farm.
    spin: HashMap<BuildingId, (f32, f32)>,
    /// The farms standing when the rotors were last placed: a farm coming
    /// or going can rearrange the fields next to it.
    farms_seen: Vec<BuildingId>,
}

/// Angular speed of the rotors in radians per second for a farm running at
/// `share` of its nameplate: still when idle, about six turns a minute
/// just past cut-in, forty at full output.
pub(super) fn rotor_speed(share: f32) -> f32 {
    if share < IDLE_SHARE {
        0.0
    } else {
        0.6 + 3.6 * ((share - IDLE_SHARE) / (1.0 - IDLE_SHARE)).clamp(0.0, 1.0)
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_turbines(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    sim: Res<Sim>,
    mats: Res<Materials>,
    time: Res<Time>,
    mut state: Local<TurbineState>,
    mut q: Query<(&TurbineRotor, &mut Transform)>,
) {
    let w = &sim.world;
    if state.meshes.is_empty() {
        let g = wind_rotor(0.0);
        state.meshes = g.iter().map(|(s, m)| (s, meshes.add(shared_mesh(m, !s.is_glow())))).collect();
    }
    if sim.full_rebuild {
        for (_, (_, ents)) in state.farms.drain() {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    // Which farms stand, and where their rotors go. Touching farms are laid
    // out as one field, so the placement is only rechecked when buildings
    // came, changed or went (the field search walks every building).
    let seen: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.plant == Some(PowerPlantKind::Wind) && b.construction.is_none() && b.demolition.is_none() && w.lots.contains_key(b.lot)).map(|(id, _)| id).collect();
    let recheck = sim.full_rebuild || seen != state.farms_seen || sim.events.iter().any(|e| matches!(e, SimEvent::BuildingBuilt(_) | SimEvent::BuildingChanged(_)));
    for &id in seen.iter().filter(|_| recheck) {
        let b = &w.buildings[id];
        let lot = &w.lots[b.lot];
        let field = if b.demolition.is_none() { super::plant_field(w, id) } else { None };
        let pad = w.lot_pad(b.lot);
        let mut h = std::collections::hash_map::DefaultHasher::new();
        b.seed.hash(&mut h);
        pad.to_bits().hash(&mut h);
        for p in &lot.polygon {
            p.x.to_bits().hash(&mut h);
            p.y.to_bits().hash(&mut h);
        }
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
        let angle = state.spin.get(&id).map_or(0.0, |s| s.0);
        for t in wind_turbines(lot, b.seed, field.as_ref()) {
            let tf = Transform::from_xyz(t.pos.x, pad + t.hub, t.pos.y).with_rotation(Quat::from_rotation_y(-(angle + t.phase)));
            for (s, mesh) in &state.meshes {
                let mut e = commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mats.surface(*s)), tf, TurbineRotor { phase: t.phase, farm: id }));
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
        state.spin.remove(&id);
    }
    if state.farms.is_empty() {
        return;
    }
    // Spin: each farm at its own output.
    let dt = time.delta_secs().min(0.1);
    let ids: Vec<BuildingId> = state.farms.keys().copied().collect();
    for id in ids {
        let share = w.buildings.get(id).map_or(0.0, |b| w.plant_dispatched_mw(b) / w.plant_capacity_mw(b).max(1e-3));
        let target = if sim.paused { 0.0 } else { rotor_speed(share) };
        let s = state.spin.entry(id).or_insert((0.0, target));
        s.1 += (target - s.1) * (dt * 0.6).min(1.0);
        s.0 = (s.0 + s.1 * dt) % std::f32::consts::TAU;
    }
    for (r, mut tf) in &mut q {
        let angle = state.spin.get(&r.farm).map_or(0.0, |s| s.0);
        tf.rotation = Quat::from_rotation_y(-(angle + r.phase));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotors_follow_the_farms_output() {
        assert_eq!(rotor_speed(0.0), 0.0, "braked or becalmed");
        assert!(rotor_speed(0.2) > 0.0 && rotor_speed(0.2) < rotor_speed(0.9));
        assert!((rotor_speed(1.0) - 4.2).abs() < 1e-4);
    }
}
