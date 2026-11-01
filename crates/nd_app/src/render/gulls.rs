//! [r5-trash] Gulls over the landfills: a flock wheels over every tip with
//! trash in it, more the fuller it is, each bird on its own circle, height
//! and speed, its wings beating (a flat bird tilting as it banks). Drawing
//! only: nothing here touches the simulation.

use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use nd_procgen::MeshData;
use nd_sim::BuildingId;
use std::collections::HashMap;

use super::mesh_util::to_mesh;
use crate::sim::Sim;

/// Gulls over an empty-ish tip, and more at full.
const GULLS_MIN: usize = 6;
const GULLS_FULL: usize = 34;

#[derive(Component)]
pub(super) struct Gull {
    site: BuildingId,
    k: usize,
}

#[derive(Default)]
pub(super) struct GullState {
    mesh: Option<Handle<Mesh>>,
    mat: Option<Handle<StandardMaterial>>,
    /// Each landfill's flock: its centre and top, how many, the entities.
    flocks: HashMap<BuildingId, (Vec3, f32, Vec<Entity>)>,
}

fn hash01(a: u64, b: u64) -> f32 {
    let mut x = a.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ b.wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    x ^= x >> 29;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 32;
    (x % 10_000) as f32 / 10_000.0
}

/// How many gulls a tip this full draws.
pub(super) fn gulls_for(fill: f32) -> usize {
    if fill <= 0.0 {
        0
    } else {
        GULLS_MIN + ((GULLS_FULL - GULLS_MIN) as f32 * fill.clamp(0.0, 1.0)).round() as usize
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_gulls(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>, sim: Res<Sim>, time: Res<Time>, mut st: Local<GullState>, mut birds: Query<(&Gull, &mut Transform)>) {
    let w = &sim.world;
    if st.mesh.is_none() {
        // A gull from above: a thin swept wing and a body.
        let mut m = MeshData::default();
        m.boxed(Vec3::ZERO, Vec3::new(1.3, 0.05, 0.28), 0.0, [1.0, 1.0, 1.0, 1.0]);
        m.boxed(Vec3::new(0.0, 0.0, 0.1), Vec3::new(0.2, 0.12, 0.6), 0.0, [1.0, 1.0, 1.0, 1.0]);
        st.mesh = Some(meshes.add(to_mesh(&m, false)));
        st.mat = Some(materials.add(StandardMaterial { base_color: Color::srgb(0.86, 0.86, 0.84), perceptual_roughness: 0.9, ..default() }));
    }
    // Which tips want how many gulls (checked every frame: a handful of sites).
    let mut want: Vec<(BuildingId, Vec3, f32, usize)> = Vec::new();
    for (id, b) in w.buildings.iter() {
        if !b.service.is_some_and(|s| s.kind == nd_sim::ServiceKind::Landfill) || !b.operating() {
            continue;
        }
        let Some(fill) = w.landfill_fill(id) else { continue };
        let n = gulls_for(fill);
        let Some(lot) = w.lots.get(b.lot) else { continue };
        let top = [7.0, 12.0, 17.0][b.tier.index()] * fill + 1.0;
        let r = (lot.area().sqrt() * 0.4).clamp(12.0, 60.0);
        want.push((id, Vec3::new(lot.center.x, w.surface_height(lot.center) + top, lot.center.y), r, n));
    }
    let gone: Vec<BuildingId> = st.flocks.keys().copied().filter(|id| !want.iter().any(|x| x.0 == *id && x.3 > 0)).collect();
    for id in gone {
        if let Some((_, _, ents)) = st.flocks.remove(&id) {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    for (id, centre, r, n) in &want {
        if *n == 0 {
            continue;
        }
        let have = st.flocks.get(id).map_or(0, |f| f.2.len());
        if have == *n && !sim.full_rebuild {
            if let Some(f) = st.flocks.get_mut(id) {
                f.0 = *centre;
                f.1 = *r;
            }
            continue;
        }
        if let Some((_, _, ents)) = st.flocks.remove(id) {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
        let (mesh, mat) = (st.mesh.clone().unwrap(), st.mat.clone().unwrap());
        let ents: Vec<Entity> = (0..*n).map(|k| commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), Transform::from_translation(*centre), NotShadowCaster, NotShadowReceiver, Gull { site: *id, k })).id()).collect();
        st.flocks.insert(*id, (*centre, *r, ents));
    }
    let t = time.elapsed_secs();
    for (g, mut tf) in birds.iter_mut() {
        let Some((centre, r, _)) = st.flocks.get(&g.site) else { continue };
        let seed = slotmap::Key::data(&g.site).as_ffi();
        let k = g.k as u64;
        let (a0, rr, hh, sp) = (hash01(seed, k) * std::f32::consts::TAU, r * (0.35 + 0.75 * hash01(seed, k + 99)), 4.0 + 18.0 * hash01(seed, k + 7), 0.25 + 0.35 * hash01(seed, k + 3));
        let dir = if hash01(seed, k + 11) < 0.5 { 1.0 } else { -1.0 };
        let a = a0 + dir * sp * t;
        let p = *centre + Vec3::new(a.cos() * rr, hh + (t * 0.7 + a0).sin() * 1.5, a.sin() * rr);
        // Facing along the circle, banked into it, the wings beating.
        let heading = -a - dir * std::f32::consts::FRAC_PI_2;
        let flap = (t * (5.0 + 3.0 * hash01(seed, k + 5)) + a0).sin() * 0.35;
        tf.translation = p;
        tf.rotation = Quat::from_rotation_y(heading) * Quat::from_rotation_z(dir * 0.35 + flap);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fuller_tip_draws_more_gulls() {
        assert_eq!(gulls_for(0.0), 0);
        assert!(gulls_for(0.1) >= GULLS_MIN && gulls_for(1.0) == GULLS_FULL);
        assert!(gulls_for(0.8) > gulls_for(0.2));
    }
}
