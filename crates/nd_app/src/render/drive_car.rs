//! [r5-drive] The player's own car in Drive mode: a tuned coupé in the
//! city's vehicle style (body, lamps and, after dark, its underglow),
//! drawn where `drive::DriveCar` is, at the height of what it stands on.

use super::*;

/// The player's car's entities: body, lights and underglow.
#[derive(Resource, Default)]
pub(super) struct DriveCarRender {
    ents: Vec<Entity>,
    /// Spawned at least a frame ago (so the entities are in the query).
    spawned: bool,
}

/// The player's car's parts. Not a `CarTag`: the depth view leaves it be
/// (it shows in a tunnel and out of one), and so does the watchdog over
/// the traffic's bodies.
#[derive(Component)]
pub(super) struct DriveCarTag;

/// The look of the player's car.
fn model() -> nd_sim::VehicleModel {
    nd_sim::VehicleModel { body: nd_sim::BodyType::Coupe, class: nd_sim::vehicle::VehicleClass::Expensive, tuned: true, paint: 3, ..Default::default() }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_drive_car(
    mut commands: Commands,
    drive: Res<crate::drive::Drive>,
    mode: Res<crate::camera::CameraMode>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut vr: ResMut<VehicleRender>,
    mut dr: ResMut<DriveCarRender>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(&mut Transform, &mut Visibility), With<DriveCarTag>>,
) {
    let car = drive.car.filter(|_| *mode == crate::camera::CameraMode::Drive);
    let Some(car) = car else {
        for &e in &dr.ents {
            if let Ok((_, mut vis)) = q.get_mut(e) {
                *vis = Visibility::Hidden;
            }
        }
        return;
    };
    let pose = Transform::from_translation(Vec3::new(car.pos.x, car.floor + 0.05, car.pos.y)).with_rotation(Quat::from_rotation_y(-car.heading));
    // Drained by a full rebuild (a load): drawn again.
    if dr.ents.iter().any(|&e| q.get(e).is_err()) && dr.spawned {
        dr.ents.clear();
    }
    if dr.ents.is_empty() {
        dr.spawned = true;
        vr.ensure_materials(&mut materials);
        let m = model();
        let v = vr.variant(&mut meshes, &m, &assets.palette);
        let finish = vr.finish_of(m.class);
        let body = commands.spawn((Mesh3d(v.body), MeshMaterial3d(finish), pose, Visibility::Inherited, NotShadowCaster, SelfLifted, DriveCarTag)).id();
        let lights = commands.spawn((Mesh3d(v.lights), MeshMaterial3d(mats.glow()), pose, Visibility::Inherited, NotShadowCaster, NotShadowReceiver, SelfLifted, DriveCarTag)).id();
        dr.ents = vec![body, lights];
        if let (Some(fx), Some(fx_mat)) = (v.fx, vr.fx.clone()) {
            dr.ents.push(commands.spawn((Mesh3d(fx), MeshMaterial3d(fx_mat), pose, Visibility::Inherited, NotShadowCaster, NotShadowReceiver, SelfLifted, DriveCarTag)).id());
        }
        return;
    }
    for &e in &dr.ents {
        if let Ok((mut tf, mut vis)) = q.get_mut(e) {
            *tf = pose;
            *vis = Visibility::Inherited;
        }
    }
}
