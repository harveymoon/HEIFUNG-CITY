//! [taxis] Cabs standing still: in the painted bays of their company's
//! yard (drawn as a surface lot), queued at ranks and waiting at the kerb,
//! with a lit TAXI sign at every yard's gate. Cabs on the road are cars of
//! the traffic model and drawn with the others; this draws the rest, so a
//! cab is on screen wherever the sim says it is.

use super::*;

#[derive(Component)]
pub(super) struct ParkedCabTag;

#[derive(Component)]
pub(super) struct TaxiSignTag;

#[derive(Resource, Default)]
pub(super) struct TaxiRender {
    /// Tick, cab count and companies drawn for.
    key: Option<(u64, usize, usize)>,
    /// Each drawn cab: where (decimetres) and its two entities.
    cabs: HashMap<nd_sim::taxi::CabId, ((i32, i32), [Entity; 2])>,
    /// Yard bays per company, from its lot.
    bays: HashMap<BuildingId, (nd_sim::LotId, Vec<(SVec2, f32)>)>,
    signs: HashMap<BuildingId, Entity>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_taxis(mut commands: Commands, sim: Res<Sim>, mats: Res<Materials>, mut tr: ResMut<TaxiRender>, mut vr: ResMut<VehicleRender>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let w = &sim.world;
    let companies = w.taxi_companies();
    let key = (w.clock.tick, w.taxis.cabs.len(), companies.len());
    if tr.key == Some(key) && !sim.full_rebuild {
        return;
    }
    tr.key = Some(key);
    vr.ensure_materials(&mut materials);
    // Where every standing cab is drawn.
    let mut want: Vec<(nd_sim::taxi::CabId, SVec2, f32, nd_sim::VehicleModel)> = w.parked_cabs().collect();
    for &c in &companies {
        let Some(lot_id) = w.buildings.get(c).map(|b| b.lot) else { continue };
        let fresh = tr.bays.get(&c).is_none_or(|(l, _)| *l != lot_id) || sim.full_rebuild;
        if fresh {
            let bays = w.lots.get(lot_id).map(nd_procgen::buildings::surface_lot_bays_world).unwrap_or_default();
            tr.bays.insert(c, (lot_id, bays));
        }
        let bays = &tr.bays[&c].1;
        let cabs = w.cabs_in_yard(c);
        for ((id, model), (p, h)) in cabs.iter().zip(bays.iter()) {
            want.push((*id, *p, *h, *model));
        }
        // A yard too narrow for painted bays (or more cabs than bays):
        // the rest stand side by side back from the gate.
        if let Some(lot) = w.lots.get(lot_id) {
            let back = -lot.front_dir;
            let heading = { let a = SVec2::new(-back.y, back.x); a.y.atan2(a.x) };
            for (k, (id, model)) in cabs.iter().skip(bays.len()).enumerate() {
                let p = lot.entrance() + back * (9.0 + 3.2 * k as f32);
                want.push((*id, p, heading, *model));
            }
        }
    }
    let mut seen: std::collections::HashSet<nd_sim::taxi::CabId> = std::collections::HashSet::new();
    for (id, p, heading, model) in want {
        seen.insert(id);
        let at = ((p.x * 10.0) as i32, (p.y * 10.0) as i32);
        if tr.cabs.get(&id).is_some_and(|(k, _)| *k == at) {
            continue;
        }
        if let Some((_, es)) = tr.cabs.remove(&id) {
            for e in es {
                commands.entity(e).despawn();
            }
        }
        let v = vr.variant(&mut meshes, &model);
        let finish = vr.finish[model.class as usize].clone();
        let pose = Transform::from_translation(Vec3::new(p.x, 0.05, p.y)).with_rotation(Quat::from_rotation_y(-heading));
        let body = commands.spawn((Mesh3d(v.body), MeshMaterial3d(finish), pose, NotShadowCaster, ParkedCabTag, Grounded)).id();
        // The roof sign and lamps: a cab waiting for a fare shows its light.
        let lights = commands.spawn((Mesh3d(v.lights), MeshMaterial3d(mats.glow()), pose, NotShadowCaster, NotShadowReceiver, ParkedCabTag, Grounded)).id();
        tr.cabs.insert(id, (at, [body, lights]));
    }
    let gone: Vec<nd_sim::taxi::CabId> = tr.cabs.keys().copied().filter(|id| !seen.contains(id)).collect();
    for id in gone {
        if let Some((_, es)) = tr.cabs.remove(&id) {
            for e in es {
                commands.entity(e).despawn();
            }
        }
    }
    // A lit TAXI sign on a post at each yard's gate.
    for &c in &companies {
        if tr.signs.contains_key(&c) && !sim.full_rebuild {
            continue;
        }
        let Some(lot) = w.building_lot(c) else { continue };
        let along = (lot.front_edge[1] - lot.front_edge[0]).normalize_or_zero();
        let gate = lot.front_edge[0] + along * 2.0 - lot.front_dir * 1.5;
        let yaw = along.y.atan2(along.x);
        let mut m = MeshData::default();
        m.boxed(Vec3::new(0.0, 2.4, 0.0), Vec3::new(0.18, 4.8, 0.18), 0.0, [0.25, 0.25, 0.28, 1.0]);
        // The sign face, lit, with a dark checker band across it.
        m.boxed(Vec3::new(0.0, 5.2, 0.0), Vec3::new(3.0, 1.0, 0.3), 0.0, [3.4, 2.6, 0.35, 1.0]);
        for i in 0..7 {
            let x = -1.35 + i as f32 * 0.45;
            m.boxed(Vec3::new(x, 5.2 + if i % 2 == 0 { 0.11 } else { -0.11 }, 0.0), Vec3::new(0.22, 0.22, 0.32), 0.0, [0.03, 0.03, 0.03, 1.0]);
        }
        // A lit canopy on top, so the yard reads as a taxi yard from the
        // top-down camera too.
        m.boxed(Vec3::new(0.0, 5.85, 0.0), Vec3::new(3.6, 0.12, 1.8), 0.0, [3.4, 2.6, 0.35, 1.0]);
        if let Some(old) = tr.signs.remove(&c) {
            commands.entity(old).despawn();
        }
        let e = commands.spawn((Mesh3d(meshes.add(to_mesh(&m, false))), MeshMaterial3d(mats.glow()), Transform::from_translation(Vec3::new(gate.x, 0.0, gate.y)).with_rotation(Quat::from_rotation_y(-yaw)), NotShadowCaster, NotShadowReceiver, TaxiSignTag, Grounded)).id();
        tr.signs.insert(c, e);
    }
    let closed: Vec<BuildingId> = tr.signs.keys().copied().filter(|c| !companies.contains(c)).collect();
    for c in closed {
        if let Some(e) = tr.signs.remove(&c) {
            commands.entity(e).despawn();
        }
        tr.bays.remove(&c);
    }
}
