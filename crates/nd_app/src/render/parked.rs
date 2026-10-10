//! Cars parked at the kerb, and wheel clamps.
//!
//! [refactor/app] Split out of `render.rs`.

use super::*;

/// [carlife] Parked cars drawn at the kerb: every private car standing on a
/// street (in a kerb slot, or squeezed in with no space) is a body mesh at
/// its spot, and so is a dealer's used stock on its forecourt. Cars in a
/// building's spaces are out of sight in its garage; a parking lot shows
/// its occupancy itself.
#[derive(Resource, Default)]
pub(super) struct ParkedRender {
    /// Tick and car count last drawn for.
    pub(super) key: Option<(u64, usize)>,
    /// Each car's spot (and whether it is clamped) as drawn, its body and
    /// [boot] the clamp on its wheel.
    pub(super) cars: HashMap<nd_sim::VehicleId, ((i32, i32, bool), Entity, Option<Entity>)>,
    /// [boot] The wheel clamp: one mesh and one bright orange material.
    pub(super) clamp: Option<(Handle<Mesh>, Handle<StandardMaterial>)>,
}

#[derive(Component)]
pub(super) struct ParkedTag;

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_parked_cars(mut commands: Commands, sim: Res<Sim>, assets: Res<GameAssets>, mut pr: ResMut<ParkedRender>, mut vr: ResMut<VehicleRender>, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let w = &sim.world;
    let key = (w.clock.tick, w.vehicles.cars.len());
    if pr.key == Some(key) && !sim.full_rebuild {
        return;
    }
    pr.key = Some(key);
    if sim.full_rebuild {
        // A loaded world reuses vehicle ids: a stale body could stand in
        // for a different model parked on the same spot.
        for (_, (_, e, clamp)) in pr.cars.drain() {
            commands.entity(e).despawn();
            if let Some(c) = clamp {
                commands.entity(c).despawn();
            }
        }
    }
    vr.ensure_materials(&mut materials);
    let mut seen: std::collections::HashSet<nd_sim::VehicleId> = std::collections::HashSet::new();
    for (vid, car) in w.vehicles.cars.iter() {
        // [carlife] And a dealer's used stock, in rows on its forecourt.
        let Some(spot) = car.spot().filter(|s| s.kind.on_street() || (s.kind == nd_sim::ParkKind::Dealer && car.dealer.is_some())) else { continue };
        seen.insert(vid);
        let at = ((spot.pos.x * 10.0) as i32, (spot.pos.y * 10.0) as i32, spot.booted);
        if pr.cars.get(&vid).is_some_and(|(k, _, _)| *k == at) {
            continue;
        }
        if let Some((_, e, clamp)) = pr.cars.remove(&vid) {
            commands.entity(e).despawn();
            if let Some(c) = clamp {
                commands.entity(c).despawn();
            }
        }
        let v = vr.variant(&mut meshes, &car.model, &assets.palette);
        let finish = vr.finish_of(car.model.class);
        let pose = Transform::from_translation(Vec3::new(spot.pos.x, 0.05, spot.pos.y)).with_rotation(Quat::from_rotation_y(-spot.heading));
        let e = commands.spawn((Mesh3d(v.body), MeshMaterial3d(finish), pose, NotShadowCaster, ParkedTag, Grounded)).id();
        // [boot] An orange clamp on the front wheel on the street side.
        let clamp = spot.booted.then(|| {
            let (mesh, mat) = pr.clamp.get_or_insert_with(|| (meshes.add(clamp_mesh()), materials.add(StandardMaterial { base_color: Color::srgb(1.0, 0.45, 0.02), emissive: LinearRgba::rgb(0.9, 0.3, 0.0), perceptual_roughness: 0.5, ..default() }))).clone();
            let len = nd_procgen::vehicles::body_length(car.model.body);
            let ax = len * 0.5 - 0.85;
            let (h, p) = (spot.heading, spot.pos);
            let left = SVec2::new(-h.sin(), h.cos());
            let middle = spot.segment.and_then(|s| w.roads.try_segment(s).map(|_| w.roads.segment_points(s))).map(|(a, b)| nd_sim::geom::project_on_segment(p, a, b).1);
            let side = if middle.is_none_or(|m| (m - p).dot(left) >= 0.0) { 1.0 } else { -1.0 };
            let half = if car.model.body.two_wheeler() { 0.25 } else { 0.98 };
            let local = Vec3::new(if car.model.body.two_wheeler() { len * 0.35 } else { ax }, 0.3, side * half);
            let t = pose * Transform::from_translation(local);
            commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat), t, NotShadowCaster, ParkedTag, Grounded)).id()
        });
        pr.cars.insert(vid, (at, e, clamp));
    }
    let gone: Vec<nd_sim::VehicleId> = pr.cars.keys().copied().filter(|id| !seen.contains(id)).collect();
    for id in gone {
        if let Some((_, e, clamp)) = pr.cars.remove(&id) {
            commands.entity(e).despawn();
            if let Some(c) = clamp {
                commands.entity(c).despawn();
            }
        }
    }
}

/// [boot] A wheel clamp: a yellow-orange plate over the wheel face with a
/// jaw round the tyre top and a padlock box, about a wheel across.
pub(super) fn clamp_mesh() -> Mesh {
    let parts: [(Vec3, Vec3); 4] = [
        // The plate over the wheel face.
        (Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.5, 0.5, 0.06)),
        // The arm over the tyre and the jaw behind it.
        (Vec3::new(0.0, 0.3, -0.1), Vec3::new(0.14, 0.08, 0.3)),
        (Vec3::new(0.0, 0.1, -0.24), Vec3::new(0.12, 0.34, 0.04)),
        // The lock box.
        (Vec3::new(0.0, -0.08, 0.07), Vec3::new(0.16, 0.2, 0.12)),
    ];
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut nrm: Vec<[f32; 3]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    for (c, size) in parts {
        let m = Mesh::from(Cuboid::new(size.x, size.y, size.z));
        let base = pos.len() as u32;
        if let Some(bevy::mesh::VertexAttributeValues::Float32x3(p)) = m.attribute(Mesh::ATTRIBUTE_POSITION) {
            pos.extend(p.iter().map(|v| [v[0] + c.x, v[1] + c.y, v[2] + c.z]));
        }
        if let Some(bevy::mesh::VertexAttributeValues::Float32x3(n)) = m.attribute(Mesh::ATTRIBUTE_NORMAL) {
            nrm.extend(n.iter().copied());
        }
        if let Some(i) = m.indices() {
            idx.extend(i.iter().map(|k| base + k as u32));
        }
    }
    let mut out = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    out.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    out.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    out.insert_indices(Indices::U32(idx));
    out
}
