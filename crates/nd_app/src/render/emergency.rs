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
    /// [fire-scene] White steam where the water hits a fire being fought.
    steam: Handle<Mesh>,
    rubble: Handle<Mesh>,
    /// [fire-scene] The fire engine's aerial ladder: turntable, the four
    /// nested sections (widest first), an outrigger beam and its jack.
    turntable: Handle<Mesh>,
    sections: [Handle<Mesh>; LADDER_SECTIONS],
    beam: Handle<Mesh>,
    jack: Handle<Mesh>,
    steel: Handle<StandardMaterial>,
}

use crate::fire_scene::{engine_scene, LadderPose, LADDER_PIVOT, LADDER_SECTION, LADDER_SECTIONS};

/// [fire-scene] A part of an engine's ladder rig.
#[derive(Component)]
pub(super) struct RigPart;

/// [fire-scene] An engine's ladder and outriggers: children of its body,
/// posed from `fire_scene::engine_scene` (stowed while it drives).
struct Rig {
    turntable: Entity,
    boom: Entity,
    sections: [Entity; LADDER_SECTIONS],
    /// Front left, front right, back left, back right.
    beams: [Entity; 4],
    jacks: [Entity; 4],
}

/// Where the outriggers sit along the truck, and how far each slides out.
const OUTRIGGER_X: [f32; 2] = [1.0, -2.5];
const OUTRIGGER_SIDES: [f32; 4] = [1.0, -1.0, 1.0, -1.0];
const OUTRIGGER_REACH: f32 = 1.3;
/// The roof the turntable sits on.
const ROOF: f32 = 2.8;

#[derive(Default)]
pub(super) struct EmergencyState {
    units: HashMap<UnitId, [Entity; 3]>,
    fires: HashMap<BuildingId, [Entity; 3]>,
    rigs: HashMap<UnitId, Rig>,
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

/// [police-look] The black-and-white's body (see `make_assets`).
fn police_body(meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
    meshes.add(super::shared_mesh(&police_mesh(), true))
}

/// [police-look] The black-and-white's mesh, kept apart for its test.
pub(super) fn police_mesh() -> MeshData {
    let white = [1.0, 1.0, 1.0, 1.0];
    let black = [0.03, 0.03, 0.035, 1.0];
    let glass = [0.12, 0.14, 0.18, 1.0];
    let mut m = MeshData::default();
    // Black hood, boot and sills.
    m.boxed(Vec3::new(0.0, 0.72, 0.0), Vec3::new(4.4, 0.74, 1.9), 0.0, black);
    // White front and rear doors, proud of the body.
    m.boxed(Vec3::new(-0.05, 0.82, 0.0), Vec3::new(2.0, 0.58, 1.93), 0.0, white);
    // The cabin's glass and a white roof.
    m.boxed(Vec3::new(-0.3, 1.38, 0.0), Vec3::new(2.2, 0.46, 1.62), 0.0, glass);
    m.boxed(Vec3::new(-0.3, 1.64, 0.0), Vec3::new(2.0, 0.08, 1.56), 0.0, white);
    // The light bar's housing and its unlit lenses, red and blue.
    m.boxed(Vec3::new(-0.3, 1.76, 0.0), Vec3::new(0.42, 0.16, 1.5), 0.0, [0.1, 0.1, 0.11, 1.0]);
    m.boxed(Vec3::new(-0.3, 1.78, -0.42), Vec3::new(0.46, 0.14, 0.6), 0.0, [0.45, 0.04, 0.04, 1.0]);
    m.boxed(Vec3::new(-0.3, 1.78, 0.42), Vec3::new(0.46, 0.14, 0.6), 0.0, [0.04, 0.1, 0.5, 1.0]);
    // A push bar on the nose and a gold shield on each front door.
    m.boxed(Vec3::new(2.27, 0.62, 0.0), Vec3::new(0.12, 0.42, 1.5), 0.0, black);
    for z in [-0.975f32, 0.975] {
        m.boxed(Vec3::new(0.45, 0.9, z), Vec3::new(0.32, 0.32, 0.02), 0.0, [0.9, 0.7, 0.2, 1.0]);
    }
    wheels(&mut m, &[-1.4, 1.4], 0.88, 0.34);
    m
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
        // (The ladder is a rig of its own, `Rig`; a low roof rack under it.)
        m.boxed(Vec3::new(0.2, 2.86, 0.0), Vec3::new(4.6, 0.12, 1.3), 0.0, [0.3, 0.3, 0.32, 1.0]);
        // Lockers along the sides and the pump panel's chrome.
        for z in [-1.21f32, 1.21] {
            m.boxed(Vec3::new(-0.4, 1.1, z), Vec3::new(1.2, 0.9, 0.04), 0.0, [0.8, 0.82, 0.85, 1.0]);
            for x in [-2.6f32, 1.4] {
                m.boxed(Vec3::new(x, 1.9, z), Vec3::new(1.4, 1.2, 0.03), 0.0, [0.85, 0.85, 0.87, 1.0]);
            }
        }
        m.boxed(Vec3::new(-0.6, 1.0, 0.0), Vec3::new(6.05, 0.15, 2.45), 0.0, [1.4, 1.2, 0.6, 1.0]);
        wheels(&mut m, &[-2.4, -1.2, 2.6], 1.05, 0.5);
        meshes.add(super::shared_mesh(&m, true))
    };
    // [police-look] Police car: a black-and-white, a black sedan with white
    // doors and roof, a push bar, and its light bar on the roof always
    // (dark lenses; the flashing halves light over it, `bars`), so it reads
    // as the police with the lights off too.
    let police = police_body(meshes);
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
    let paint = [paint_of(materials, [0.55, 0.03, 0.02], 0.4), paint_of(materials, [0.95, 0.95, 0.97], 0.3), paint_of(materials, [0.9, 0.9, 0.86], 0.2)];
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
            // [fire-scene] Saturated enough to read as flame, not peach, by day.
            let col = if k % 2 == 0 { [1.8, 0.48, 0.04, 1.0] } else { [1.9, 0.95, 0.1, 1.0] };
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
    // Steam: the same column in white, for a fire under the hoses.
    let steam = {
        let mut m = MeshData::default();
        for k in 0..7 {
            let t = k as f32;
            let r = 0.35 + t * 0.16;
            m.boxed(Vec3::new(-0.1 * t, 0.3 + t * 0.5, 0.12 * t), Vec3::new(r * 2.0, r * 1.3, r * 2.0), t * 0.8 + 0.3, [0.85, 0.87, 0.9, (0.38 - t * 0.045).max(0.06)]);
        }
        meshes.add(super::shared_mesh(&m, false))
    };
    // [fire-scene] The aerial ladder: a turntable with its control
    // pedestal, then four nested sections along +x from the pivot (rails,
    // rungs and a red stripe; the last one carries the monitor nozzle).
    let steel_c = [0.78, 0.79, 0.82, 1.0];
    let turntable = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(0.0, 0.12, 0.0), Vec3::new(1.7, 0.24, 1.7), 0.0, [0.22, 0.22, 0.24, 1.0]);
        m.boxed(Vec3::new(0.0, 0.35, 0.0), Vec3::new(1.1, 0.3, 1.1), 0.0, [0.75, 0.08, 0.06, 1.0]);
        m.boxed(Vec3::new(-0.55, 0.75, 0.6), Vec3::new(0.3, 0.6, 0.3), 0.0, [0.3, 0.3, 0.32, 1.0]);
        // The pivot cheeks the ladder hinges between.
        for z in [-0.5f32, 0.5] {
            m.boxed(Vec3::new(0.0, 0.5, z), Vec3::new(0.5, 0.5, 0.1), 0.0, steel_c);
        }
        meshes.add(super::shared_mesh(&m, false))
    };
    let section = |meshes: &mut Assets<Mesh>, k: usize| {
        let mut m = MeshData::default();
        let half = 0.48 - 0.06 * k as f32;
        let len = LADDER_SECTION;
        for z in [-half, half] {
            m.boxed(Vec3::new(len * 0.5, 0.0, z), Vec3::new(len, 0.3, 0.07), 0.0, steel_c);
            m.boxed(Vec3::new(len * 0.5, 0.17, z), Vec3::new(len, 0.05, 0.075), 0.0, [0.8, 0.06, 0.05, 1.0]);
        }
        let rungs = (len / 0.55) as usize;
        for r in 0..rungs {
            m.boxed(Vec3::new(0.3 + r as f32 * 0.55, -0.08, 0.0), Vec3::new(0.06, 0.06, half * 2.0), 0.0, steel_c);
        }
        if k == LADDER_SECTIONS - 1 {
            // The monitor at the tip and a little platform to stand on.
            m.boxed(Vec3::new(len - 0.5, -0.12, 0.0), Vec3::new(1.0, 0.06, half * 2.0 + 0.1), 0.0, [0.3, 0.3, 0.32, 1.0]);
            m.boxed(Vec3::new(len + 0.1, 0.2, 0.0), Vec3::new(0.5, 0.18, 0.18), 0.0, [0.9, 0.75, 0.2, 1.0]);
        }
        meshes.add(super::shared_mesh(&m, false))
    };
    let sections = std::array::from_fn(|k| section(meshes, k));
    // An outrigger beam along +z (scaled to its reach) and a jack hanging
    // down from y = 0 (scaled to the ground), striped red and white.
    let beam = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(0.0, 0.0, 0.5), Vec3::new(0.28, 0.22, 1.0), 0.0, [0.85, 0.85, 0.85, 1.0]);
        meshes.add(super::shared_mesh(&m, false))
    };
    let jack = {
        let mut m = MeshData::default();
        m.boxed(Vec3::new(0.0, -0.5, 0.0), Vec3::new(0.2, 1.0, 0.2), 0.0, [0.8, 0.07, 0.05, 1.0]);
        m.boxed(Vec3::new(0.0, -0.97, 0.0), Vec3::new(0.55, 0.06, 0.55), 0.0, [0.2, 0.2, 0.2, 1.0]);
        meshes.add(super::shared_mesh(&m, false))
    };
    let steel = materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.4, metallic: 0.55, ..default() });
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
    EmergencyAssets { bodies: [engine, police, ambulance], bars, paint, light_a, light_b, flame, flame_mat, smoke, steam, rubble, turntable, sections, beam, jack, steel }
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
    selection: Res<crate::tools::Selection>,
    mut q: Query<(&mut Transform, &mut Visibility, Option<&mut InTunnel>), With<EmergencyTag>>,
    mut cars: Query<&mut Visibility, (With<CarTag>, Without<EmergencyTag>)>,
    mut parts: Query<&mut Transform, (With<RigPart>, Without<EmergencyTag>, Without<CarTag>)>,
) {
    let w = &sim.world;
    let s = &w.services;
    if sim.full_rebuild {
        for (_, ents) in es.units.drain() {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
        es.rigs.clear();
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
        let shown = u.car.is_some_and(|c| w.traffic.cars.contains_key(c)) || matches!(u.state, UnitState::OnScene | UnitState::Patrol);
        if shown && !es.units.contains_key(&uid) {
            let k = kind_index(u.kind);
            let body = commands.spawn((Mesh3d(a.bodies[k].clone()), MeshMaterial3d(a.paint[k].clone()), super::offstage(), NotShadowCaster, EmergencyTag, SelfLifted, InTunnel(false), super::focus::UnitTag)).id();
            let la = commands.spawn((Mesh3d(a.bars[k][0].clone()), MeshMaterial3d(a.light_a[k].clone()), super::offstage(), NotShadowCaster, NotShadowReceiver, EmergencyTag, SelfLifted, InTunnel(false), super::focus::UnitTag)).id();
            let lb = commands.spawn((Mesh3d(a.bars[k][1].clone()), MeshMaterial3d(a.light_b[k].clone()), super::offstage(), NotShadowCaster, NotShadowReceiver, EmergencyTag, SelfLifted, InTunnel(false), super::focus::UnitTag)).id();
            // [layers] The Crime layer lights the police up (`focus`).
            if u.kind == UnitKind::PoliceCar {
                for e in [body, la, lb] {
                    commands.entity(e).insert(super::focus::PoliceTag);
                }
            }
            es.units.insert(uid, [body, la, lb]);
            if u.kind == UnitKind::FireEngine {
                let rig = spawn_rig(&mut commands, a, body);
                es.rigs.insert(uid, rig);
            }
        }
        if !shown {
            es.rigs.remove(&uid);
            if let Some(ents) = es.units.remove(&uid) {
                for e in ents {
                    commands.entity(e).despawn();
                }
            }
        }
    }
    let gone: Vec<UnitId> = es.units.keys().copied().filter(|id| !s.units.contains_key(*id)).collect();
    for id in gone {
        es.rigs.remove(&id);
        if let Some(ents) = es.units.remove(&id) {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    let lifted = super::lifts(*selection);
    for (uid, ents) in es.units.iter() {
        let Some(u) = s.units.get(*uid) else { continue };
        // [fix/review-app] At the height of the road the unit drives on (a
        // viaduct, a tunnel), as `sync_cars` does, not lifted to the top
        // surface by `lift_movers`; and in the tunnel layer when it is in
        // one. Lifted when selected, like a selected car.
        let car = u.car.and_then(|c| w.traffic.cars.get(c));
        let (pos, heading) = match car {
            Some(car) => car.pose_at(f),
            None => (u.pos, u.heading),
        };
        let (ground, tunnel) = match car {
            Some(car) => (w.car_height_at(car, pos), w.car_in_tunnel(car)),
            None => (w.surface_height_near(pos, w.ground_height(pos)), false),
        };
        let y = ground + 0.05 + if lifted.unit == Some(*uid) { super::SELECTED_LIFT } else { 0.0 };
        // Lights and sirens: the two halves of the bar alternate.
        let phase = ((t * 5.0 + phase_of(*uid)) as i64) % 2 == 0;
        for (i, e) in ents.iter().enumerate() {
            if let Ok((mut tf, mut vis, depth)) = q.get_mut(*e) {
                tf.translation = Vec3::new(pos.x, y, pos.y);
                if let Some(mut d) = depth.filter(|d| d.0 != tunnel) {
                    d.0 = tunnel;
                }
                tf.rotation = Quat::from_rotation_y(-heading);
                *vis = match i {
                    0 => Visibility::Inherited,
                    1 if u.lights() && phase => Visibility::Inherited,
                    2 if u.lights() && !phase => Visibility::Inherited,
                    _ => Visibility::Hidden,
                };
            }
        }
        // [fire-scene] The ladder up and the legs down at a fire.
        if let Some(rig) = es.rigs.get(uid) {
            let pose = engine_scene(w, *uid, f, &game.rules, &sim.building_tops).map_or_else(LadderPose::stowed, |s| s.ladder);
            pose_rig(rig, &pose, &mut parts);
        }
    }

    // Fires: flames on the roof and a smoke column, flickering with intensity.
    let burning: HashMap<BuildingId, f32> = s.fires().collect();
    let fought: HashSet<BuildingId> = s.active().filter(|i| i.kind == nd_sim::services::IncidentKind::Fire && i.stage == nd_sim::services::Stage::OnScene).map(|i| i.building).collect();
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
            let fl = commands.spawn((Mesh3d(a.flame.clone()), MeshMaterial3d(a.flame_mat.clone()), super::offstage(), NotShadowCaster, NotShadowReceiver, EmergencyTag, Grounded)).id();
            let sm = commands.spawn((Mesh3d(a.smoke.clone()), MeshMaterial3d(mats.overlay.clone()), super::offstage(), NotShadowCaster, NotShadowReceiver, EmergencyTag, Grounded)).id();
            let st = commands.spawn((Mesh3d(a.steam.clone()), MeshMaterial3d(mats.overlay.clone()), super::offstage(), NotShadowCaster, NotShadowReceiver, EmergencyTag, Grounded)).id();
            [fl, sm, st]
        });
        // [fire-scene] Under the hoses: less black smoke, more white steam,
        // the more so as it weakens.
        let fought = fought.contains(&bid);
        // The roof as the building generator stacks it: floors at the land
        // use's storey height.
        // [fire-scene] As drawn (a style's tower can rise well above the
        // storeys' height), else floors at the land use's storey height.
        let height = sim.building_tops.height(w, bid, &game.rules);
        let size = (lot.area().sqrt() * 0.55).clamp(5.0, 30.0);
        let seed = phase_of(bid) * 10.0;
        let flicker = 0.8 + 0.25 * (t * 9.0 + seed).sin() + 0.12 * (t * 23.0 + seed * 2.0).sin();
        let k = intensity.clamp(0.1, 1.0);
        if let Ok((mut tf, _, _)) = q.get_mut(ents[0]) {
            tf.translation = Vec3::new(lot.center.x, height, lot.center.y);
            tf.scale = Vec3::new(size * (0.6 + 0.5 * k), size * (0.35 + 0.8 * k) * flicker, size * (0.6 + 0.5 * k));
            tf.rotation = Quat::from_rotation_y(t * 0.3 + seed);
        }
        if let Ok((mut tf, _, _)) = q.get_mut(ents[1]) {
            let s = if fought { 0.45 + 0.55 * k } else { 1.0 };
            tf.translation = Vec3::new(lot.center.x, height + size * 0.2, lot.center.y);
            tf.scale = Vec3::new(size * (0.5 + 0.6 * k), size * (0.5 + 0.8 * k), size * (0.5 + 0.6 * k)) * s;
        }
        if let Ok((mut tf, mut vis, _)) = q.get_mut(ents[2]) {
            *vis = if fought { Visibility::Inherited } else { Visibility::Hidden };
            let s = size * (0.45 + 0.7 * (1.0 - k)) * (0.9 + 0.1 * (t * 1.7 + seed).sin());
            tf.translation = Vec3::new(lot.center.x, height, lot.center.y);
            tf.scale = Vec3::new(s, s * 1.2, s);
            tf.rotation = Quat::from_rotation_y(-t * 0.2 + seed);
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

/// [fire-scene] An engine's ladder and outriggers, as children of its body.
fn spawn_rig(commands: &mut Commands, a: &EmergencyAssets, body: Entity) -> Rig {
    let part = |commands: &mut Commands, mesh: &Handle<Mesh>, parent: Entity, tf: Transform| commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(a.steel.clone()), tf, NotShadowCaster, RigPart, ChildOf(parent))).id();
    let turntable = part(commands, &a.turntable, body, Transform::from_xyz(LADDER_PIVOT[0], ROOF, LADDER_PIVOT[2]));
    let boom = commands.spawn((Transform::from_xyz(0.0, LADDER_PIVOT[1] - ROOF, 0.0), Visibility::Inherited, RigPart, ChildOf(turntable))).id();
    let sections = std::array::from_fn(|k| part(commands, &a.sections[k], boom, Transform::from_xyz(0.0, 0.07 * k as f32, 0.0)));
    let mut beams = [Entity::PLACEHOLDER; 4];
    let mut jacks = [Entity::PLACEHOLDER; 4];
    for i in 0..4 {
        let (x, side) = (OUTRIGGER_X[i / 2], OUTRIGGER_SIDES[i]);
        let rot = if side > 0.0 { Quat::IDENTITY } else { Quat::from_rotation_y(std::f32::consts::PI) };
        beams[i] = part(commands, &a.beam, body, Transform::from_xyz(x, 0.75, side * 0.9).with_rotation(rot));
        jacks[i] = part(commands, &a.jack, body, Transform::from_xyz(x, 0.75, side * 1.0));
    }
    Rig { turntable, boom, sections, beams, jacks }
}

/// Pose an engine's rig (all in its body's frame): the turntable turned,
/// the boom raised, each fly section slid out past the one below, the
/// outrigger beams out and their jacks down to the ground.
fn pose_rig(rig: &Rig, pose: &LadderPose, parts: &mut Query<&mut Transform, (With<RigPart>, Without<EmergencyTag>, Without<CarTag>)>) {
    if let Ok(mut tf) = parts.get_mut(rig.turntable) {
        tf.rotation = Quat::from_rotation_y(-pose.yaw_rel);
    }
    if let Ok(mut tf) = parts.get_mut(rig.boom) {
        tf.rotation = Quat::from_rotation_z(pose.pitch);
    }
    let off = pose.section_offset();
    for (k, e) in rig.sections.iter().enumerate() {
        if let Ok(mut tf) = parts.get_mut(*e) {
            tf.translation = Vec3::new(off * k as f32, 0.07 * k as f32, 0.0);
        }
    }
    let reach = 0.25 + OUTRIGGER_REACH * pose.outriggers;
    for i in 0..4 {
        let side = OUTRIGGER_SIDES[i];
        if let Ok(mut tf) = parts.get_mut(rig.beams[i]) {
            tf.scale = Vec3::new(1.0, 1.0, reach);
        }
        if let Ok(mut tf) = parts.get_mut(rig.jacks[i]) {
            tf.translation.z = side * (0.9 + reach - 0.12);
            tf.scale = Vec3::new(1.0, 0.05 + 0.75 * pose.outriggers, 1.0);
        }
    }
}

/// A stable phase in `0..1` per thing, so lights and flames do not beat in step.
fn phase_of<T: Hash>(id: T) -> f32 {
    let mut h = DefaultHasher::new();
    id.hash(&mut h);
    (h.finish() % 1000) as f32 / 1000.0
}

/// [services] A recovery truck on its way to a wreck flashes the amber
/// beacon on its cab roof: lights, but no siren, so nobody pulls over for
/// it (`DriveState::amber`). Drawn as a bright gizmo in the layer the truck
/// is seen in (in a tunnel only in the Underground view).
pub(super) fn amber_beacons(sim: Res<Sim>, time: Res<Time>, tools: Res<crate::tools::ToolState>, mut gizmos: Gizmos) {
    let w = &sim.world;
    let t = time.elapsed_secs();
    let f = sim.frac();
    let under = tools.depth == crate::tools::Depth::Underground;
    for (id, car) in w.traffic.cars.iter().filter(|(_, c)| c.drive.amber) {
        if w.car_in_tunnel(car) != under || ((t * 3.0 + phase_of(id)) as i64) % 2 != 0 {
            continue;
        }
        let (pos, heading) = car.pose_at(f);
        let p = pos + SVec2::new(heading.cos(), heading.sin()) * 0.5;
        let y = w.car_height_at(car, pos) + 1.95;
        gizmos.sphere(Isometry3d::from_translation(Vec3::new(p.x, y, p.y)), 0.22, Color::linear_rgb(6.0, 2.6, 0.15));
    }
}

#[cfg(test)]
mod police_look_tests {
    use super::*;

    /// [police-look] G8: a police car reads as one with its lights off: a
    /// black-and-white (black and white panels both) with a light bar's
    /// housing on the roof, red and blue lenses, part of the body itself.
    #[test]
    fn a_police_car_is_black_and_white_with_a_light_bar() {
        let m = police_mesh();
        let near = |c: &[f32; 4], k: [f32; 3]| (c[0] - k[0]).abs() < 0.05 && (c[1] - k[1]).abs() < 0.05 && (c[2] - k[2]).abs() < 0.05;
        let black = m.colors.iter().filter(|c| near(c, [0.03, 0.03, 0.035])).count();
        let white = m.colors.iter().filter(|c| near(c, [1.0, 1.0, 1.0])).count();
        assert!(black > 0 && white > 0, "black {black} white {white}");
        let roof_red = m.positions.iter().zip(&m.colors).any(|(p, c)| p[1] > 1.7 && c[0] > 0.3 && c[2] < 0.1);
        let roof_blue = m.positions.iter().zip(&m.colors).any(|(p, c)| p[1] > 1.7 && c[2] > 0.3 && c[0] < 0.1);
        assert!(roof_red && roof_blue, "the light bar's lenses sit on the roof");
    }
}

/// [walk-crossings] Someone lying hurt in the street, waiting for the
/// paramedics (`World::down_on_street`): a pulsing red cross over them, so
/// a casualty is told at a glance from someone asleep on the pavement
/// (Harvey's flag: "is this guy hurt? he should have some indicator").
pub(super) fn hurt_markers(sim: Res<Sim>, time: Res<Time>, tools: Res<crate::tools::ToolState>, mut gizmos: Gizmos) {
    if tools.depth == crate::tools::Depth::Underground {
        return;
    }
    let w = &sim.world;
    let down = w.down_on_street();
    if down.is_empty() {
        return;
    }
    let t = time.elapsed_secs();
    let pulse = 0.6 + 0.4 * (t * 4.0).sin().abs();
    let red = Color::linear_rgb(7.0 * pulse, 0.25, 0.3);
    for &id in &down {
        let Some(c) = w.citizens.get(id) else { continue };
        if c.inside.is_some() || c.activity.is_traveling() {
            continue;
        }
        // Where the figure lies: thrown clear beside the wreck.
        let p = c.pos + SVec2::new(-c.heading.sin(), c.heading.cos()) * 2.6;
        let y = w.surface_height(p) + 2.6 + 0.15 * (t * 2.0).sin();
        let centre = Vec3::new(p.x, y, p.y);
        let (h, v) = (Vec3::X * 0.55, Vec3::Y * 0.55);
        for k in [-0.08f32, 0.0, 0.08] {
            let o = Vec3::Z * k;
            gizmos.line(centre - h + o, centre + h + o, red);
            gizmos.line(centre - v + o, centre + v + o, red);
        }
        gizmos.circle(Isometry3d::new(centre, Quat::IDENTITY), 0.85, red);
        gizmos.line(centre - v * 1.6, Vec3::new(p.x, y - 2.3, p.y), Color::linear_rgb(2.0, 0.1, 0.1));
    }
}
