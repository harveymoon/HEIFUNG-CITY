//! Cars, trucks and buses on the road, delivery bots, and their shared meshes and finishes.
//!
//! [refactor/app] Split out of `render.rs`.

use super::*;

// ── vehicles: generated per variant (`nd_procgen::vehicles`) ─────────────────

/// Marker for delivery robots on the pavement.
#[derive(Component)]
pub struct BotTag;

/// One variant's meshes, built once and shared by every vehicle that looks
/// the same.
#[derive(Clone)]
pub(super) struct VariantMeshes {
    pub(super) body: Handle<Mesh>,
    pub(super) lights: Handle<Mesh>,
    pub(super) fx: Option<Handle<Mesh>>,
}

/// Vehicle drawing state: the variant cache, the materials, and the
/// entities beyond the body and lamps of each car (underglow, trails) and
/// of each robot.
#[derive(Resource, Default)]
pub(super) struct VehicleRender {
    pub(super) variants: HashMap<nd_sim::VehicleModel, VariantMeshes>,
    /// Paint finish by class: cheap (rough), mid, expensive (glossy).
    pub(super) finish: Vec<Handle<StandardMaterial>>,
    /// Additive, unlit: underglow and light trails; scaled by night.
    pub(super) fx: Option<Handle<StandardMaterial>>,
    pub(super) night_key: Option<i32>,
    pub(super) car_fx: HashMap<CarId, Entity>,
    pub(super) trails: HashMap<CarId, (Entity, Handle<Mesh>, std::collections::VecDeque<Vec3>)>,
    /// Trail entities and meshes not in use, hidden, for the next Akira
    /// bike: trail meshes are never removed from `Assets<Mesh>`.
    pub(super) spare_trails: Vec<(Entity, Handle<Mesh>)>,
    pub(super) bots: HashMap<nd_sim::BotId, Vec<Entity>>,
    /// [camera] Each shop's robot fleet in its own paint (`bots::fleet_colour`).
    pub(super) fleet_paint: HashMap<nd_sim::BuildingId, Handle<StandardMaterial>>,
}

impl VehicleRender {
    /// [fix/review-app] The paint for a car class, without indexing past
    /// the list (a new `VehicleClass` would have panicked here).
    pub(super) fn finish_of(&self, class: nd_sim::vehicle::VehicleClass) -> Handle<StandardMaterial> {
        self.finish.get(class as usize).or(self.finish.first()).cloned().unwrap_or_default()
    }

    pub(super) fn ensure_materials(&mut self, materials: &mut Assets<StandardMaterial>) {
        if self.fx.is_some() {
            return;
        }
        // [look] Expensive paint carries a clear coat over the base coat:
        // the sky and the city's glow slide over the bodywork. Metalness is
        // kept low: under the sky's reflections a metal body shows the sky
        // rather than its paint.
        for (rough, metal, refl, coat, coat_rough) in [(0.85, 0.05, 0.3, 0.0, 0.5), (0.45, 0.2, 0.5, 0.0, 0.5), (0.3, 0.3, 0.6, 0.8, 0.1)] {
            self.finish.push(materials.add(StandardMaterial {
                base_color: Color::WHITE,
                perceptual_roughness: rough,
                metallic: metal,
                reflectance: refl,
                clearcoat: coat,
                clearcoat_perceptual_roughness: coat_rough,
                ..default()
            }));
        }
        self.fx = Some(materials.add(StandardMaterial { base_color: Color::BLACK, unlit: true, alpha_mode: AlphaMode::Add, ..default() }));
    }

    /// The meshes of a model's variant in the palette's paints, built once
    /// per variant.
    pub(super) fn variant(&mut self, meshes: &mut Assets<Mesh>, model: &nd_sim::VehicleModel, pal: &nd_procgen::Palette) -> VariantMeshes {
        let key = nd_procgen::vehicles::variant_key(model);
        self.variants
            .entry(key)
            .or_insert_with(|| {
                let v = nd_procgen::vehicles::vehicle_mesh_with(&key, pal);
                VariantMeshes { body: meshes.add(shared_mesh(&v.body, true)), lights: meshes.add(shared_mesh(&v.lights, false)), fx: (!v.fx.is_empty()).then(|| meshes.add(shared_mesh(&v.fx, false))) }
            })
            .clone()
    }
}

/// What a car is drawn as. Cars from saves before vehicle models (and
/// test spawns) carry the default; trucks and buses among them are given
/// their kind's body, and the old colour becomes the paint.
pub(super) fn car_model(car: &nd_sim::Car) -> nd_sim::VehicleModel {
    use nd_sim::{BodyType, Livery, VehicleModel};
    let mut m = car.model;
    if m == VehicleModel::default() {
        m.paint = car.color % nd_sim::vehicle::PAINTS;
        if car.cargo.is_some() {
            m.body = BodyType::PanelTruck;
        }
    }
    if car.bus.is_some() && !m.body.is_bus() {
        m = VehicleModel { body: BodyType::BubbleBus, livery: Livery::Transit, paint: car.color % nd_sim::vehicle::PAINTS, ..Default::default() };
    }
    m
}

/// Points kept in an Akira bike's light trail.
pub(super) const TRAIL_POINTS: usize = 28;

/// A light-trail ribbon through `pts` (oldest first): two red streaks at
/// tail-lamp height fading out behind the bike. Always the same size
/// (`TRAIL_POINTS - 1` segments of two double-sided quads), so rewriting it
/// every frame never changes its allocation; segments not yet laid down
/// are zero-area and black.
pub(super) fn trail_mesh(pts: &std::collections::VecDeque<Vec3>) -> MeshData {
    let mut m = MeshData::default();
    let n = pts.len();
    let last = pts.back().copied().unwrap_or(Vec3::ZERO);
    for i in 1..TRAIL_POINTS {
        let (a, b) = if i < n { (pts[i - 1], pts[i]) } else { (last, last) };
        let d = b - a;
        let live = i < n && d.length_squared() >= 1e-4;
        let side = if live { Vec3::new(-d.z, 0.0, d.x).normalize_or_zero() * 0.12 } else { Vec3::ZERO };
        let fade = |k: usize| if live { (k as f32 / n as f32).powf(1.5) } else { 0.0 };
        let ca = [2.6 * fade(i - 1), 0.12 * fade(i - 1), 0.08 * fade(i - 1), 1.0];
        let cb = [2.6 * fade(i), 0.12 * fade(i), 0.08 * fade(i), 1.0];
        let (y0, y1) = if live { (0.62f32, 0.78f32) } else { (0.0, 0.0) };
        for sgn in [1.0f32, -1.0] {
            let o = side * sgn;
            let base = m.positions.len() as u32;
            for (p, c) in [(a + o + Vec3::Y * y0, ca), (b + o + Vec3::Y * y0, cb), (b + o + Vec3::Y * y1, cb), (a + o + Vec3::Y * y1, ca)] {
                m.positions.push(p.to_array());
                m.normals.push([0.0, 1.0, 0.0]);
                m.colors.push(c);
                m.uvs.push([0.0, 0.0]);
            }
            // Both faces: a ribbon is seen from either side.
            m.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3, base, base + 2, base + 1, base, base + 3, base + 2]);
        }
    }
    m
}

/// Write a trail into its mesh in place: same attribute lengths each time.
pub(super) fn write_trail(mesh: &mut Mesh, data: &MeshData) {
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, data.positions.clone());
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, data.colors.clone());
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_cars(
    mut commands: Commands,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    selection: Res<Selection>,
    lighting: Res<Lighting>,
    mut state: ResMut<RenderState>,
    mut vr: ResMut<VehicleRender>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(&mut Transform, Option<&mut InTunnel>), With<CarTag>>,
) {
    let lifted = lifts(*selection);
    vr.ensure_materials(&mut materials);
    let fx_mat = vr.fx.clone().expect("vehicle materials");
    // Underglow and trails only show after dark.
    let night = night_strength(&sim, &lighting);
    let night_key = (night * 32.0).round() as i32;
    if vr.night_key != Some(night_key) {
        vr.night_key = Some(night_key);
        if let Some(mut m) = materials.get_mut(&fx_mat) {
            let k = night_key as f32 / 32.0;
            m.base_color = Color::linear_rgb(k, k, k);
        }
    }
    let traffic = &sim.world.traffic;
    let f = sim.frac();
    for (id, car) in traffic.cars.iter() {
        if !state.cars.contains_key(&id) {
            // Left over from entities a full rebuild drained.
            if let Some(e) = vr.car_fx.remove(&id) {
                commands.entity(e).despawn();
            }
            if let Some((e, h, _)) = vr.trails.remove(&id) {
                commands.entity(e).insert(Visibility::Hidden);
                vr.spare_trails.push((e, h));
            }
            let model = car_model(car);
            let v = vr.variant(&mut meshes, &model, &assets.palette);
            let finish = vr.finish_of(model.class);
            // [carlife] Spawned where the car is. Entities spawned through
            // `Commands` are not in this frame's query below, so a car
            // spawned at the identity transform was drawn for one frame at
            // the world origin: the centre of the main junction, flashing
            // each time a car joined the road.
            let pose = car_pose(car, f, &sim.world);
            let tunnel = sim.world.car_in_tunnel(car);
            let body = commands.spawn((Mesh3d(v.body), MeshMaterial3d(finish), pose, NotShadowCaster, CarTag, SelfLifted, InTunnel(tunnel))).id();
            let lights = commands.spawn((Mesh3d(v.lights), MeshMaterial3d(mats.glow()), pose, NotShadowCaster, NotShadowReceiver, CarTag, SelfLifted, InTunnel(tunnel))).id();
            state.cars.insert(id, [body, lights]);
            if let Some(fx) = v.fx {
                let e = commands.spawn((Mesh3d(fx), MeshMaterial3d(fx_mat.clone()), pose, NotShadowCaster, NotShadowReceiver, CarTag, SelfLifted, InTunnel(tunnel))).id();
                vr.car_fx.insert(id, e);
            }
            if model.body == nd_sim::BodyType::AkiraBike {
                let (e, h) = match vr.spare_trails.pop() {
                    Some((e, h)) => {
                        if let Some(mut m) = meshes.get_mut(&h) {
                            write_trail(&mut m, &trail_mesh(&std::collections::VecDeque::new()));
                        }
                        commands.entity(e).insert(Visibility::Inherited);
                        (e, h)
                    }
                    None => {
                        let h = meshes.add(shared_mesh(&trail_mesh(&std::collections::VecDeque::new()), false));
                        let e = commands.spawn((Mesh3d(h.clone()), MeshMaterial3d(fx_mat.clone()), Transform::IDENTITY, Visibility::Inherited, NotShadowCaster, NotShadowReceiver, bevy::camera::visibility::NoFrustumCulling)).id();
                        (e, h)
                    }
                };
                vr.trails.insert(id, (e, h, std::collections::VecDeque::new()));
            }
        }
    }
    let gone: Vec<CarId> = state.cars.keys().copied().filter(|id| !traffic.cars.contains_key(*id)).collect();
    for id in gone {
        if let Some(ents) = state.cars.remove(&id) {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    // Underglow and trails of cars no longer drawn: gone from the road, or
    // their entities drained by a full rebuild (a loaded world reuses car
    // ids, so a stale entry would leave a glow pad standing in the street).
    let gone_fx: Vec<CarId> = vr.car_fx.keys().chain(vr.trails.keys()).copied().filter(|id| !traffic.cars.contains_key(*id) || !state.cars.contains_key(id)).collect();
    for id in gone_fx {
        if let Some(e) = vr.car_fx.remove(&id) {
            commands.entity(e).despawn();
        }
        if let Some((e, h, _)) = vr.trails.remove(&id) {
            // Kept for the next bike: removing a mesh asset while an entity
            // still draws it is how the renderer ends up copying into freed
            // slab space.
            commands.entity(e).insert(Visibility::Hidden);
            vr.spare_trails.push((e, h));
        }
    }
    for (id, ents) in state.cars.iter() {
        let Some(car) = traffic.cars.get(*id) else { continue };
        // [driving] Along the substep poses, not a chord across the turn.
        let (pos, heading) = car.pose_at(f);
        // Lifted when selected, whether through its driver or, for a bus or
        // truck with nobody to inspect, as the vehicle itself.
        let selected = lifted.citizen == Some(car.citizen) || lifted.car == Some(*id);
        let fx = vr.car_fx.get(id).copied();
        // Lifted here from the road the car is on, not by `lift_movers`:
        // under the highway's decks position alone would put it up top.
        // At the height of the road where it is drawn, not where it ends
        // the tick.
        let lift = sim.world.car_height_at(car, pos);
        let tunnel = sim.world.car_in_tunnel(car);
        for e in ents.iter().copied().chain(fx) {
            if let Ok((mut tf, depth)) = q.get_mut(e) {
                tf.translation = Vec3::new(pos.x, lift + 0.05 + if selected { SELECTED_LIFT } else { 0.0 }, pos.y);
                tf.rotation = Quat::from_rotation_y(-heading);
                if let Some(mut d) = depth.filter(|d| d.0 != tunnel) {
                    d.0 = tunnel;
                }
            }
        }
    }
    // Akira light trails: the tail lamp's recent path, in world space.
    if !vr.trails.is_empty() {
        let w = &sim.world;
        let ids: Vec<CarId> = vr.trails.keys().copied().collect();
        for id in ids {
            let Some(car) = traffic.cars.get(id) else { continue };
            let (pos, heading) = car.pose_at(f);
            let tail = pos - SVec2::new(heading.cos(), heading.sin()) * 1.4;
            let p = Vec3::new(tail.x, 0.05 + w.surface_height(tail), tail.y);
            let Some((_, h, pts)) = vr.trails.get_mut(&id) else { continue };
            if pts.back().is_some_and(|b| b.distance(p) < 0.35) {
                continue;
            }
            if pts.back().is_some_and(|b| b.distance(p) > 25.0) {
                pts.clear();
            }
            pts.push_back(p);
            while pts.len() > TRAIL_POINTS {
                pts.pop_front();
            }
            let mesh = trail_mesh(pts);
            if let Some(mut m) = meshes.get_mut(&*h) {
                write_trail(&mut m, &mesh);
            }
        }
    }
}

/// A watchdog for the moving cars: every few seconds, every car on the road
/// must have a body entity, visible unless an emergency unit stands in for
/// it, holding a loaded, non-empty mesh, within a few metres of the car.
/// Anything else is logged with an example, since a car without a body is
/// otherwise only noticed by eye.
#[allow(clippy::type_complexity)]
pub(super) fn check_car_bodies(
    sim: Res<Sim>,
    state: Res<RenderState>,
    meshes: Res<Assets<Mesh>>,
    time: Res<Time>,
    mut next: Local<f32>,
    q: Query<(&Visibility, &Mesh3d, &Transform), With<CarTag>>,
) {
    let now = time.elapsed_secs();
    if now < *next {
        return;
    }
    *next = now + 5.0;
    let w = &sim.world;
    let units: std::collections::HashSet<CarId> = w.services.units.values().filter_map(|u| u.car).collect();
    let (mut missing, mut hidden, mut no_mesh, mut far) = (0, 0, 0, 0);
    let mut example = None;
    for (id, car) in w.traffic.cars.iter() {
        let Some(ents) = state.cars.get(&id) else { continue };
        let bad = match q.get(ents[0]) {
            Err(_) => {
                missing += 1;
                true
            }
            Ok((vis, mesh, tf)) => {
                let h = *vis == Visibility::Hidden && !units.contains(&id);
                let m = meshes.get(&mesh.0).is_none_or(|m| m.count_vertices() < 4);
                let (p, _) = car.pose_at(sim.frac());
                let f = SVec2::new(tf.translation.x, tf.translation.z).distance(p) > 20.0;
                hidden += h as usize;
                no_mesh += m as usize;
                far += f as usize;
                h || m || f
            }
        };
        if bad && example.is_none() {
            example = Some(id);
        }
    }
    if let Some(id) = example {
        warn!("car bodies: {missing} missing, {hidden} hidden, {no_mesh} without a mesh, {far} away from their car, of {} on the road (e.g. {id:?})", w.traffic.cars.len());
    }
}

/// [carlife] Where a car on the road is drawn this frame: the same pose
/// and height the per-frame update gives it (`sync_cars`).
pub(super) fn car_pose(car: &nd_sim::Car, f: f32, w: &nd_sim::World) -> Transform {
    let (pos, heading) = car.pose_at(f);
    let lift = w.car_height_at(car, pos);
    Transform::from_translation(Vec3::new(pos.x, lift + 0.05, pos.y)).with_rotation(Quat::from_rotation_y(-heading))
}

/// Pavement delivery robots: a small generated mesh per shop livery,
/// interpolated between ticks like walkers.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_bots(
    mut commands: Commands,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    mut vr: ResMut<VehicleRender>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut q: Query<&mut Transform, With<BotTag>>,
) {
    vr.ensure_materials(&mut materials);
    if sim.full_rebuild {
        // Bot ids are reused by a loaded world; the livery would be stale.
        vr.fleet_paint.clear();
        for (_, ents) in vr.bots.drain() {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    let w = &sim.world;
    for (id, bot) in w.bots.bots.iter() {
        if vr.bots.contains_key(&id) {
            continue;
        }
        // [camera] A plain light shell painted in the company's colour, so
        // each shop's fleet reads as its own (all grocers' robots used to
        // share the grocery livery).
        let paint = (slotmap::Key::data(&bot.shop).as_ffi() % nd_sim::vehicle::PAINTS as u64) as u8;
        let model = nd_sim::VehicleModel { body: nd_sim::BodyType::DeliveryBot, livery: nd_sim::Livery::None, paint, ..Default::default() };
        let v = vr.variant(&mut meshes, &model, &assets.palette);
        let finish = vr.fleet_paint.entry(bot.shop).or_insert_with(|| {
            let [r, g, b] = nd_sim::bots::fleet_colour(bot.shop);
            materials.add(StandardMaterial { base_color: Color::srgb(r, g, b), perceptual_roughness: 0.45, metallic: 0.2, reflectance: 0.5, ..default() })
        }).clone();
        // [carlife] Spawned where it is, not at the origin (see `sync_cars`).
        let pose = Transform::from_translation(Vec3::new(bot.pos.x, 0.16, bot.pos.y)).with_rotation(Quat::from_rotation_y(-bot.heading));
        let mut ents = vec![
            commands.spawn((Mesh3d(v.body), MeshMaterial3d(finish), pose, NotShadowCaster, BotTag, Grounded)).id(),
            commands.spawn((Mesh3d(v.lights), MeshMaterial3d(mats.glow()), pose, NotShadowCaster, NotShadowReceiver, BotTag, Grounded)).id(),
        ];
        if let (Some(fx), Some(mat)) = (v.fx, vr.fx.clone()) {
            ents.push(commands.spawn((Mesh3d(fx), MeshMaterial3d(mat), pose, NotShadowCaster, NotShadowReceiver, BotTag, Grounded)).id());
        }
        vr.bots.insert(id, ents);
    }
    let gone: Vec<nd_sim::BotId> = vr.bots.keys().copied().filter(|id| !w.bots.bots.contains_key(*id)).collect();
    for id in gone {
        for e in vr.bots.remove(&id).unwrap_or_default() {
            commands.entity(e).despawn();
        }
    }
    let f = sim.frac();
    for (id, ents) in vr.bots.iter() {
        let Some(bot) = w.bots.bots.get(*id) else { continue };
        let pos = if bot.prev_pos.distance(bot.pos) < 30.0 { bot.prev_pos.lerp(bot.pos, f) } else { bot.pos };
        let heading = lerp_angle(bot.prev_heading, bot.heading, f);
        for e in ents {
            if let Ok(mut tf) = q.get_mut(*e) {
                tf.translation = Vec3::new(pos.x, 0.16, pos.y);
                tf.rotation = Quat::from_rotation_y(-heading);
            }
        }
    }
}
