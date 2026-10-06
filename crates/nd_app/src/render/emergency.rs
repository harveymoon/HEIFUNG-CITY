//! Emergency visuals: fire engines, police cars and ambulances with
//! flashing light bars (driving, or parked on scene), flames and smoke over
//! burning buildings, and rubble on burned lots. Kept apart from the car
//! code: the generic car mesh drawn for a unit's car is hidden here and the
//! unit's own body drawn in its place.

use super::*;
use nd_sim::services::{UnitKind, UnitState};
use nd_sim::UnitId;

#[derive(Component)]
pub(super) struct EmergencyTag;

/// Meshes and materials made on first use.
pub(super) struct EmergencyAssets {
    bodies: [Handle<Mesh>; 3],
    bars: [[Handle<Mesh>; 2]; 3],
    paint: [Handle<StandardMaterial>; 3],
    light_a: [Handle<StandardMaterial>; 3],
    light_b: [Handle<StandardMaterial>; 3],
    flame: Handle<Mesh>,
    /// Unlit, so flames read in daylight too (the shared glow dims by day).
    flame_mat: Handle<StandardMaterial>,
    smoke: Handle<Mesh>,
    rubble: Handle<Mesh>,
}

#[derive(Default)]
pub(super) struct EmergencyState {
    units: HashMap<UnitId, [Entity; 3]>,
    fires: HashMap<BuildingId, [Entity; 2]>,
    rubble: HashMap<LotId, Entity>,
}

fn kind_index(k: UnitKind) -> usize {
    match k {
        UnitKind::FireEngine => 0,
        UnitKind::PoliceCar => 1,
        UnitKind::Ambulance => 2,
    }
}

fn wheels(m: &mut MeshData, xs: &[f32], z: f32, r: f32) {
    for &x in xs {
        for s in [-1.0f32, 1.0] {
            m.boxed(Vec3::new(x, r, s * z), Vec3::new(r * 2.0, r * 2.0, 0.26), 0.0, [0.08, 0.08, 0.08, 1.0]);
        }
    }
}

fn make_assets(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> EmergencyAssets {
    let white = [1.0, 1.0, 1.0, 1.0];
    let dark = [0.12, 0.14, 0.17, 1.0];
    // Fire engine: a long cab-over truck with a ladder on the roof.
    let engine = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(-0.6, 1.6, 0.0), Vec3::new(6.0, 2.4, 2.4), 0.0, white);
        m.boxed(Vec3::new(3.1, 1.5, 0.0), Vec3::new(1.6, 2.2, 2.35), 0.0, white);
        m.boxed(Vec3::new(3.7, 2.0, 0.0), Vec3::new(0.3, 0.9, 2.1), 0.0, dark);
        m.boxed(Vec3::new(-0.8, 2.95, 0.0), Vec3::new(5.4, 0.25, 0.9), 0.0, [0.75, 0.75, 0.78, 1.0]);
        m.boxed(Vec3::new(-0.6, 1.0, 0.0), Vec3::new(6.05, 0.15, 2.45), 0.0, [1.4, 1.2, 0.6, 1.0]);
        wheels(&mut m, &[-2.4, -1.2, 2.6], 1.05, 0.5);
        meshes.add(super::shared_mesh(&m, true))
    };
    // Police car: a white sedan with a dark door band.
    let police = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(0.0, 0.75, 0.0), Vec3::new(4.4, 0.8, 1.9), 0.0, white);
        m.boxed(Vec3::new(0.0, 0.78, 0.0), Vec3::new(1.8, 0.5, 1.92), 0.0, dark);
        m.boxed(Vec3::new(-0.3, 1.45, 0.0), Vec3::new(2.2, 0.6, 1.6), 0.0, [0.2, 0.22, 0.26, 1.0]);
        wheels(&mut m, &[-1.4, 1.4], 0.88, 0.34);
        meshes.add(super::shared_mesh(&m, true))
    };
    // Ambulance: a box van with a red stripe.
    let ambulance = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(-0.5, 1.45, 0.0), Vec3::new(4.0, 2.3, 2.2), 0.0, white);
        m.boxed(Vec3::new(2.1, 1.1, 0.0), Vec3::new(1.4, 1.6, 2.1), 0.0, white);
        m.boxed(Vec3::new(2.7, 1.5, 0.0), Vec3::new(0.25, 0.7, 1.9), 0.0, dark);
        m.boxed(Vec3::new(-0.5, 1.3, 0.0), Vec3::new(4.05, 0.3, 2.25), 0.0, [0.9, 0.1, 0.1, 1.0]);
        wheels(&mut m, &[-1.7, 1.9], 1.0, 0.4);
        meshes.add(super::shared_mesh(&m, true))
    };
    let bar = |meshes: &mut Assets<Mesh>, x: f32, y: f32, side: f32| {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(x, y, side * 0.5), Vec3::new(0.6, 0.35, 0.95), 0.0, white);
        meshes.add(super::shared_mesh(&m, false))
    };
    let bars = [
        [bar(meshes, 3.2, 2.75, -1.0), bar(meshes, 3.2, 2.75, 1.0)],
        [bar(meshes, -0.3, 1.9, -1.0), bar(meshes, -0.3, 1.9, 1.0)],
        [bar(meshes, 2.0, 2.05, -1.0), bar(meshes, 2.0, 2.05, 1.0)],
    ];
    let paint_of = |materials: &mut Assets<StandardMaterial>, c: [f32; 3], metal: f32| materials.add(StandardMaterial { base_color: Color::linear_rgb(c[0], c[1], c[2]), perceptual_roughness: 0.35, metallic: metal, ..default() });
    let paint = [paint_of(materials, [0.55, 0.03, 0.02], 0.4), paint_of(materials, [0.85, 0.87, 0.9], 0.3), paint_of(materials, [0.9, 0.9, 0.86], 0.2)];
    let light = |materials: &mut Assets<StandardMaterial>, c: [f32; 3]| materials.add(StandardMaterial { base_color: Color::linear_rgb(c[0], c[1], c[2]), emissive: LinearRgba::rgb(c[0] * 6.0, c[1] * 6.0, c[2] * 6.0), unlit: true, ..default() });
    let red = [3.0, 0.15, 0.1];
    let blue = [0.15, 0.4, 3.2];
    let amber = [3.0, 1.6, 0.2];
    let whitel = [2.6, 2.6, 2.6];
    let light_a = [light(materials, red), light(materials, red), light(materials, red)];
    let light_b = [light(materials, amber), light(materials, blue), light(materials, whitel)];
    // Flames: a ragged crown of glowing tongues on a unit footprint, scaled per fire.
    let flame = {
        let mut m = MeshData::default();
        let tongues = [(0.0f32, 0.0f32, 1.0f32), (0.35, 0.2, 0.7), (-0.3, 0.25, 0.8), (0.2, -0.35, 0.6), (-0.25, -0.3, 0.75), (0.42, -0.1, 0.5), (-0.45, 0.0, 0.55)];
        for (k, (x, z, h)) in tongues.iter().enumerate() {
            let col = if k % 2 == 0 { [3.2, 1.0, 0.15, 1.0] } else { [3.0, 1.8, 0.3, 1.0] };
            m.boxed(Vec3::new(*x, h * 0.5, *z), Vec3::new(0.22, *h, 0.22), k as f32 * 0.7, col);
        }
        meshes.add(super::shared_mesh(&m, false))
    };
    // Smoke: a leaning column of translucent puffs, widening as it rises.
    let smoke = {
        let mut m = MeshData::default();
        for k in 0..8 {
            let t = k as f32;
            let r = 0.3 + t * 0.12;
            m.boxed(Vec3::new(0.18 * t, 0.3 + t * 0.55, 0.08 * t), Vec3::new(r * 2.0, r * 1.4, r * 2.0), t * 0.6, [0.07, 0.07, 0.08, (0.42 - t * 0.04).max(0.08)]);
        }
        meshes.add(super::shared_mesh(&m, false))
    };
    let rubble = {
        let mut m = MeshData::default();
        let pieces = [(0.0f32, 0.0f32, 1.0f32, 0.0f32), (0.3, 0.25, 0.6, 0.4), (-0.3, 0.2, 0.7, 1.1), (0.25, -0.3, 0.5, 0.8), (-0.25, -0.25, 0.8, 2.0), (0.0, 0.38, 0.4, 0.3), (0.4, -0.05, 0.45, 1.6)];
        for (x, z, s, yaw) in pieces {
            m.boxed(Vec3::new(x, s * 0.15, z), Vec3::new(s * 0.45, s * 0.3, s * 0.3), yaw, [0.16, 0.15, 0.14, 1.0]);
        }
        m.boxed(Vec3::new(0.0, 0.02, 0.0), Vec3::new(1.4, 0.04, 1.4), 0.0, [0.07, 0.07, 0.07, 1.0]);
        meshes.add(super::shared_mesh(&m, true))
    };
    let flame_mat = materials.add(StandardMaterial { base_color: Color::WHITE, unlit: true, ..default() });
    EmergencyAssets { bodies: [engine, police, ambulance], bars, paint, light_a, light_b, flame, flame_mat, smoke, rubble }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_emergency(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
    mats: Res<Materials>,
    game: Res<GameAssets>,
    state: Res<RenderState>,
    time: Res<Time>,
    mut assets: Local<Option<EmergencyAssets>>,
    mut es: Local<EmergencyState>,
    mut q: Query<(&mut Transform, &mut Visibility), With<EmergencyTag>>,
    mut cars: Query<&mut Visibility, (With<CarTag>, Without<EmergencyTag>)>,
) {
    let w = &sim.world;
    let s = &w.services;
    if sim.full_rebuild {
        for (_, ents) in es.units.drain() {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
        for (_, ents) in es.fires.drain() {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
        for (_, e) in es.rubble.drain() {
            commands.entity(e).despawn();
        }
    }
    if s.units.is_empty() && es.units.is_empty() && es.fires.is_empty() && s.incidents.is_empty() && s.rubble.is_empty() && es.rubble.is_empty() {
        return;
    }
    let a = assets.get_or_insert_with(|| make_assets(&mut meshes, &mut materials));
    let t = time.elapsed_secs();
    let f = sim.frac();

    // Units out of the station; the generic car drawn for a unit's car is hidden.
    for (uid, u) in s.units.iter() {
        if let Some(car) = u.car {
            if let Some(ents) = state.cars.get(&car) {
                for e in ents {
                    if let Ok(mut v) = cars.get_mut(*e) {
                        *v = Visibility::Hidden;
                    }
                }
            }
        }
        let shown = u.car.is_some_and(|c| w.traffic.cars.contains_key(c)) || u.state == UnitState::OnScene;
        if shown && !es.units.contains_key(&uid) {
            let k = kind_index(u.kind);
            let body = commands.spawn((Mesh3d(a.bodies[k].clone()), MeshMaterial3d(a.paint[k].clone()), Transform::IDENTITY, NotShadowCaster, EmergencyTag, Grounded)).id();
            let la = commands.spawn((Mesh3d(a.bars[k][0].clone()), MeshMaterial3d(a.light_a[k].clone()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, EmergencyTag, Grounded)).id();
            let lb = commands.spawn((Mesh3d(a.bars[k][1].clone()), MeshMaterial3d(a.light_b[k].clone()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, EmergencyTag, Grounded)).id();
            es.units.insert(uid, [body, la, lb]);
        }
        if !shown {
            if let Some(ents) = es.units.remove(&uid) {
                for e in ents {
                    commands.entity(e).despawn();
                }
            }
        }
    }
    let gone: Vec<UnitId> = es.units.keys().copied().filter(|id| !s.units.contains_key(*id)).collect();
    for id in gone {
        if let Some(ents) = es.units.remove(&id) {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    for (uid, ents) in es.units.iter() {
        let Some(u) = s.units.get(*uid) else { continue };
        let (pos, heading) = match u.car.and_then(|c| w.traffic.cars.get(c)) {
            Some(car) => (car.prev_pos.lerp(car.pos, f), lerp_angle(car.prev_heading, car.heading, f)),
            None => (u.pos, u.heading),
        };
        // Lights and sirens: the two halves of the bar alternate.
        let phase = ((t * 5.0 + phase_of(*uid)) as i64) % 2 == 0;
        for (i, e) in ents.iter().enumerate() {
            if let Ok((mut tf, mut vis)) = q.get_mut(*e) {
                tf.translation = Vec3::new(pos.x, 0.05, pos.y);
                tf.rotation = Quat::from_rotation_y(-heading);
                *vis = match i {
                    0 => Visibility::Inherited,
                    1 if u.lights() && phase => Visibility::Inherited,
                    2 if u.lights() && !phase => Visibility::Inherited,
                    _ => Visibility::Hidden,
                };
            }
        }
    }

    // Fires: flames on the roof and a smoke column, flickering with intensity.
    let burning: HashMap<BuildingId, f32> = s.fires().collect();
    let out: Vec<BuildingId> = es.fires.keys().copied().filter(|b| !burning.contains_key(b)).collect();
    for b in out {
        if let Some(ents) = es.fires.remove(&b) {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    for (&bid, &intensity) in &burning {
        let Some(b) = w.buildings.get(bid) else { continue };
        let Some(lot) = w.lots.get(b.lot) else { continue };
        let ents = *es.fires.entry(bid).or_insert_with(|| {
            let fl = commands.spawn((Mesh3d(a.flame.clone()), MeshMaterial3d(a.flame_mat.clone()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, EmergencyTag, Grounded)).id();
            let sm = commands.spawn((Mesh3d(a.smoke.clone()), MeshMaterial3d(mats.overlay.clone()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, EmergencyTag, Grounded)).id();
            [fl, sm]
        });
        // The roof as the building generator stacks it: floors at the land
        // use's storey height.
        let height = (b.floors.max(1) as f32 * game.rules.for_land_use(b.land_use).floor_height).min(160.0);
        let size = (lot.area().sqrt() * 0.55).clamp(5.0, 30.0);
        let seed = phase_of(bid) * 10.0;
        let flicker = 0.8 + 0.25 * (t * 9.0 + seed).sin() + 0.12 * (t * 23.0 + seed * 2.0).sin();
        let k = intensity.clamp(0.1, 1.0);
        if let Ok((mut tf, _)) = q.get_mut(ents[0]) {
            tf.translation = Vec3::new(lot.center.x, height, lot.center.y);
            tf.scale = Vec3::new(size * (0.6 + 0.5 * k), size * (0.35 + 0.8 * k) * flicker, size * (0.6 + 0.5 * k));
            tf.rotation = Quat::from_rotation_y(t * 0.3 + seed);
        }
        if let Ok((mut tf, _)) = q.get_mut(ents[1]) {
            tf.translation = Vec3::new(lot.center.x, height + size * 0.2, lot.center.y);
            tf.scale = Vec3::new(size * (0.5 + 0.6 * k), size * (0.5 + 0.8 * k), size * (0.5 + 0.6 * k));
        }
    }

    // Rubble on burned lots that nothing has replaced yet.
    let piles: Vec<(LotId, SVec2)> = s.rubble.iter().filter(|r| w.lots.get(r.0).is_some_and(|l| l.building.is_none())).map(|r| (r.0, r.1)).collect();
    let keep: HashSet<LotId> = piles.iter().map(|p| p.0).collect();
    let stale: Vec<LotId> = es.rubble.keys().copied().filter(|l| !keep.contains(l)).collect();
    for l in stale {
        if let Some(e) = es.rubble.remove(&l) {
            commands.entity(e).despawn();
        }
    }
    for (lid, at) in piles {
        if es.rubble.contains_key(&lid) {
            continue;
        }
        let size = w.lots.get(lid).map_or(8.0, |l| (l.area().sqrt() * 0.7).clamp(4.0, 30.0));
        let e = commands.spawn((Mesh3d(a.rubble.clone()), MeshMaterial3d(mats.surface(Surface::Concrete)), Transform::from_translation(Vec3::new(at.x, 0.0, at.y)).with_scale(Vec3::new(size, size * 0.4, size)), EmergencyTag, Grounded)).id();
        es.rubble.insert(lid, e);
    }
}

/// A stable phase in `0..1` per thing, so lights and flames do not beat in step.
fn phase_of<T: Hash>(id: T) -> f32 {
    let mut h = DefaultHasher::new();
    id.hash(&mut h);
    (h.finish() % 1000) as f32 / 1000.0
}
