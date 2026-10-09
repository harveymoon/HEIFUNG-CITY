//! Railway trains.
//!
//! [refactor/app] Split out of `render.rs`.

use super::*;

/// Trains: a locomotive and its coaches or wagons, each a body in the
/// train's colour, placed along the route and blended between ticks.
pub(super) fn sync_trains(
    mut commands: Commands,
    sim: Res<Sim>,
    mats: Res<Materials>,
    mut state: ResMut<RenderState>,
    mut q: Query<&mut Transform, With<TrainTag>>,
) {
    let rail = &sim.world.rail;
    for (id, train) in rail.trains.iter() {
        if !state.trains.contains_key(&id) {
            let mut ents = Vec::new();
            for k in 0..=(train.cars as usize) {
                let mesh = match (k, train.kind) {
                    (0, _) => mats.loco_mesh.clone(),
                    (_, nd_sim::TrainKind::Passenger) => mats.coach_mesh.clone(),
                    (_, nd_sim::TrainKind::Freight) => mats.wagon_mesh.clone(),
                };
                let colour = if k == 0 || train.kind == nd_sim::TrainKind::Passenger { train.color as usize } else { (train.color as usize + k * 3) % mats.car_bodies.len().max(1) };
                let mat = mats.car_bodies[colour % mats.car_bodies.len().max(1)].clone();
                ents.push(commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), Transform::IDENTITY, TrainTag)).id());
            }
            state.trains.insert(id, ents);
        }
    }
    let gone: Vec<nd_sim::TrainId> = state.trains.keys().copied().filter(|id| !rail.trains.contains_key(*id)).collect();
    for id in gone {
        if let Some(ents) = state.trains.remove(&id) {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    let f = sim.frac();
    for (id, ents) in state.trains.iter() {
        let Some(train) = rail.trains.get(*id) else { continue };
        let s = train.prev_s + (train.s - train.prev_s) * f;
        for (k, e) in ents.iter().enumerate() {
            if let Ok(mut tf) = q.get_mut(*e) {
                let (p, heading) = train.vehicle_pose_at(k, s);
                tf.translation = Vec3::new(p.x, 0.3, p.y);
                tf.rotation = Quat::from_rotation_y(-heading);
            }
        }
    }
}
