//! Air traffic: drone taxis, air ambulances and police helicopters flying
//! at altitude, taking off from and landing on the pads the buildings
//! actually drew (`Geometry::pads`), with nav lights, a blinking strobe,
//! a spinning main rotor on helicopters and, at night, the police
//! helicopter's searchlight.
//!
//! The sim reckons pad heights from floors; the drawn deck is wherever the
//! style put it. Low down near its pad an aircraft is eased onto the real
//! deck (slot by slot on a vertiport), so a landing touches the roof.

use super::*;
use nd_sim::air::{Craft, DroneId, Phase};

#[derive(Component)]
pub(super) struct AirTag;

/// Entities of one aircraft.
struct Craft3d {
    body: Entity,
    lights: Entity,
    fx: Option<Entity>,
    strobe: Entity,
    rotor: Option<Entity>,
    beam: Option<(Entity, Entity)>,
    spin: f32,
}

#[derive(Default)]
pub(super) struct AirRender {
    craft: HashMap<DroneId, Craft3d>,
    /// Drawn pads per pad building, keyed by what the drawing depends on.
    pads: HashMap<BuildingId, (u64, Vec<Vec3>)>,
    rotor: Option<Handle<Mesh>>,
    cone: Option<Handle<Mesh>>,
    strobe_mesh: Option<Handle<Mesh>>,
    strobe_mat: Option<Handle<StandardMaterial>>,
    beam_mat: Option<Handle<StandardMaterial>>,
}

#[derive(Resource, Default)]
pub(super) struct AirState(AirRender);

/// What a pad building's drawing depends on.
fn pad_key(w: &World, id: BuildingId) -> u64 {
    let mut h = DefaultHasher::new();
    if let Some(b) = w.buildings.get(id) {
        (b.seed, b.floors, b.tier as u8, b.construction.is_some(), b.lot, w.air.has_rooftop(id)).hash(&mut h);
        w.lot_pad(b.lot).to_bits().hash(&mut h);
    }
    h.finish()
}

fn model_of(craft: Craft, number: u16) -> nd_sim::VehicleModel {
    use nd_sim::{BodyType, VehicleClass, VehicleModel};
    match craft {
        Craft::Taxi => VehicleModel { body: BodyType::AirTaxi, class: VehicleClass::Expensive, paint: [0u8, 2, 4, 6][number as usize % 4], ..Default::default() },
        Craft::AirAmbulance => VehicleModel { body: BodyType::Helicopter, class: VehicleClass::Expensive, paint: 3, ..Default::default() },
        Craft::PoliceHeli => VehicleModel { body: BodyType::Helicopter, class: VehicleClass::Expensive, paint: 4, ..Default::default() },
    }
}

/// The drawn pads of a pad building (generated once per change).
fn drawn_pads<'a>(st: &'a mut AirRender, w: &World, assets: &GameAssets, styles: &StyleBook, id: BuildingId) -> &'a [Vec3] {
    let key = pad_key(w, id);
    let fresh = st.pads.get(&id).is_none_or(|(k, _)| *k != key);
    if fresh {
        let mut geo = Geometry::default();
        let _ = super::append_building(&mut geo, &mut [Geometry::default()], w, assets, styles, id);
        st.pads.insert(id, (key, geo.pads));
    }
    &st.pads[&id].1
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_air(
    mut commands: Commands,
    sim: Res<Sim>,
    assets: Res<GameAssets>,
    mats: Res<Materials>,
    lighting: Res<Lighting>,
    time: Res<Time>,
    mut state: ResMut<AirState>,
    mut vr: ResMut<VehicleRender>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut q: Query<(&mut Transform, &mut Visibility), With<AirTag>>,
    mut spots: Query<&mut SpotLight, With<AirTag>>,
) {
    let w = &sim.world;
    let st = &mut state.0;
    if sim.full_rebuild {
        // A loaded world reuses drone and building ids: drop every craft and
        // every cached pad drawing.
        for (_, c) in st.craft.drain() {
            for e in [Some(c.body), Some(c.lights), c.fx, Some(c.strobe), c.rotor, c.beam.map(|b| b.0), c.beam.map(|b| b.1)].into_iter().flatten() {
                commands.entity(e).despawn();
            }
        }
        st.pads.clear();
    }
    if w.air.drones.is_empty() && st.craft.is_empty() {
        return;
    }
    vr.ensure_materials(&mut materials);
    let fx_mat = vr.fx.clone().expect("vehicle materials");
    if st.rotor.is_none() {
        st.rotor = Some(meshes.add(super::shared_mesh(&nd_procgen::vehicles::rotor_mesh(), true)));
        // A unit cone along +Y, wide at the bottom: the searchlight's beam.
        let mut cone = MeshData::default();
        cone.cylinder(Vec2::ZERO, 9.0, 0.35, 0.0, 1.0, 14, [0.55, 0.55, 0.48, 1.0]);
        st.cone = Some(meshes.add(super::shared_mesh(&cone, false)));
        let mut cube = MeshData::default();
        cube.boxed(Vec3::ZERO, Vec3::splat(0.22), 0.0, [1.0, 1.0, 1.0, 1.0]);
        st.strobe_mesh = Some(meshes.add(super::shared_mesh(&cube, false)));
        st.strobe_mat = Some(materials.add(StandardMaterial { base_color: Color::WHITE, emissive: LinearRgba::rgb(30.0, 30.0, 30.0), unlit: true, ..default() }));
        st.beam_mat = Some(materials.add(StandardMaterial { base_color: Color::linear_rgba(0.35, 0.35, 0.3, 1.0), unlit: true, alpha_mode: AlphaMode::Add, cull_mode: None, ..default() }));
    }
    // Spawn and despawn.
    for (id, d) in w.air.drones.iter() {
        if st.craft.contains_key(&id) {
            continue;
        }
        let model = model_of(d.craft, d.number);
        let v = vr.variant(&mut meshes, &model, &assets.palette);
        let finish = vr.finish_of(model.class);
        let body = commands.spawn((Mesh3d(v.body), MeshMaterial3d(finish), super::offstage(), Visibility::Visible, AirTag)).id();
        let lights = commands.spawn((Mesh3d(v.lights), MeshMaterial3d(mats.glow()), super::offstage(), Visibility::Visible, NotShadowCaster, NotShadowReceiver, AirTag)).id();
        let fx = v.fx.map(|fx| commands.spawn((Mesh3d(fx), MeshMaterial3d(fx_mat.clone()), super::offstage(), Visibility::Visible, NotShadowCaster, NotShadowReceiver, AirTag)).id());
        let strobe = commands.spawn((Mesh3d(st.strobe_mesh.clone().unwrap()), MeshMaterial3d(st.strobe_mat.clone().unwrap()), super::offstage(), Visibility::Hidden, NotShadowCaster, NotShadowReceiver, AirTag)).id();
        let rotor = (d.craft != Craft::Taxi).then(|| commands.spawn((Mesh3d(st.rotor.clone().unwrap()), MeshMaterial3d(vr.finish_of(nd_sim::vehicle::VehicleClass::Mid)), super::offstage(), Visibility::Visible, AirTag)).id());
        let beam = (d.craft == Craft::PoliceHeli).then(|| {
            let cone = commands.spawn((Mesh3d(st.cone.clone().unwrap()), MeshMaterial3d(st.beam_mat.clone().unwrap()), super::offstage(), Visibility::Hidden, NotShadowCaster, NotShadowReceiver, AirTag)).id();
            let light = commands
                .spawn((SpotLight { color: Color::linear_rgb(1.0, 0.97, 0.88), intensity: 0.0, range: 400.0, radius: 0.3, shadow_maps_enabled: false, inner_angle: 0.05, outer_angle: 0.11, ..default() }, super::offstage(), Visibility::Hidden, AirTag))
                .id();
            (cone, light)
        });
        st.craft.insert(id, Craft3d { body, lights, fx, strobe, rotor, beam, spin: 0.0 });
    }
    let gone: Vec<DroneId> = st.craft.keys().copied().filter(|id| !w.air.drones.contains_key(*id)).collect();
    for id in gone {
        if let Some(c) = st.craft.remove(&id) {
            for e in [Some(c.body), Some(c.lights), c.fx, Some(c.strobe), c.rotor, c.beam.map(|b| b.0), c.beam.map(|b| b.1)].into_iter().flatten() {
                commands.entity(e).despawn();
            }
        }
    }
    st.pads.retain(|b, _| w.air.pad(*b).is_some());
    let f = sim.frac();
    let night = super::night_strength(&sim, &lighting);
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    let paused = sim.paused;
    let ids: Vec<DroneId> = st.craft.keys().copied().collect();
    for id in ids {
        let Some(d) = w.air.drones.get(id) else { continue };
        let mut pos = d.prev_pos.lerp(d.pos, f);
        let alt = d.prev_alt + (d.alt - d.prev_alt) * f;
        let heading = super::lerp_angle(d.prev_heading, d.heading, f);
        let ground = w.surface_height(pos);
        let mut y = ground + alt;
        // Near its pad and low: ease onto the deck the building drew.
        if let Some(pb) = d.at.or(d.dest) {
            if let Some(pad) = w.air.pad(pb).copied() {
                let drawn = drawn_pads(st, w, &assets, &sim.styles, pb);
                if !drawn.is_empty() {
                    let real = drawn[d.slot as usize % drawn.len()];
                    let cruise = d.craft.cruise();
                    let near = pos.distance(pad.pos) < 120.0;
                    let k = if near { (1.0 - ((alt - pad.height) / (cruise - pad.height).max(1.0))).clamp(0.0, 1.0) } else { 0.0 };
                    let k = k * k * (3.0 - 2.0 * k);
                    let sim_deck = w.surface_height(pad.pos) + pad.height;
                    y += k * (real.y - sim_deck);
                    pos += (SVec2::new(real.x, real.z) - pad.pos) * k;
                }
            }
        }
        // Skids touch the deck; a little bank in the turn would be nice, but level reads better from afar.
        let rot = Quat::from_rotation_y(-heading);
        let at = Vec3::new(pos.x, y, pos.y);
        let Some(c) = st.craft.get_mut(&id) else { continue };
        for e in [Some(c.body), Some(c.lights), c.fx].into_iter().flatten() {
            if let Ok((mut tf, _)) = q.get_mut(e) {
                tf.translation = at;
                tf.rotation = rot;
            }
        }
        let engines = d.phase != Phase::Parked || d.job != nd_sim::air::Job::Idle;
        if let Some(r) = c.rotor {
            if !paused && engines {
                c.spin = (c.spin + dt * 28.0) % std::f32::consts::TAU;
            }
            if let Ok((mut tf, _)) = q.get_mut(r) {
                tf.translation = at + rot * Vec3::new(0.0, 3.0, 0.0);
                tf.rotation = rot * Quat::from_rotation_y(c.spin);
            }
        }
        // Strobe: a short white flash every second while flying, aft.
        let tail = if d.craft == Craft::Taxi { Vec3::new(-1.85, 1.3, 0.0) } else { Vec3::new(-5.15, 3.35, 0.0) };
        let flash = d.phase.airborne() && ((t + id_phase(id)) % 1.1) < 0.09;
        if let Ok((mut tf, mut vis)) = q.get_mut(c.strobe) {
            tf.translation = at + rot * tail;
            *vis = if flash { Visibility::Visible } else { Visibility::Hidden };
        }
        // Police searchlight over the scene after dark.
        if let Some((cone, light)) = c.beam {
            let on = d.searching() && night > 0.35;
            let target = Vec3::new(d.goal.x, w.surface_height(d.goal), d.goal.y);
            let from = at + Vec3::new(0.0, 0.3, 0.0);
            let v = from - target;
            if let Ok((mut tf, mut vis)) = q.get_mut(cone) {
                *vis = if on { Visibility::Visible } else { Visibility::Hidden };
                if on && v.length() > 1.0 {
                    tf.translation = target;
                    tf.rotation = Quat::from_rotation_arc(Vec3::Y, v.normalize());
                    tf.scale = Vec3::new(1.0, v.length(), 1.0);
                }
            }
            if let Ok((mut tf, mut vis)) = q.get_mut(light) {
                *vis = if on { Visibility::Visible } else { Visibility::Hidden };
                *tf = Transform::from_translation(from).looking_at(target, Vec3::Y);
            }
            if let Ok(mut s) = spots.get_mut(light) {
                s.intensity = if on { 6.0e7 * night } else { 0.0 };
            }
        }
    }
}

/// A per-aircraft offset so strobes do not blink in step.
fn id_phase(id: DroneId) -> f32 {
    use slotmap::Key;
    (id.data().as_ffi() % 97) as f32 * 0.0113
}
