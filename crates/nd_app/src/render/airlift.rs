//! [airlift] The airlift helicopter: a twin-rotor heavy-lift (Chinook-
//! style) machine flying in, hovering over a stuck vehicle, and flying it
//! away on a sling (`nd_sim::world::airlift`).

use super::*;

#[derive(Component)]
pub(super) struct AirliftTag;

/// Fuselage, front rotor, rear rotor, sling, the load.
type Parts = [Entity; 5];

pub(super) struct AirliftMeshes {
    body: Handle<Mesh>,
    rotor: Handle<Mesh>,
    sling: Handle<Mesh>,
    load: Handle<Mesh>,
}

/// The slung vehicle's paint, by its model's paint index.
const LOAD_PAINT: [[f32; 3]; 6] = [[0.7, 0.1, 0.1], [0.1, 0.2, 0.55], [0.8, 0.8, 0.82], [0.08, 0.08, 0.09], [0.85, 0.6, 0.1], [0.15, 0.4, 0.25]];

/// Rotor hubs along the fuselage (front, back) and their height.
const HUBS: [f32; 2] = [6.4, -6.4];
const HUB_Y: f32 = 4.3;
/// The sling's length under the belly.
const SLING: f32 = 9.0;

/// [airlift] The Chinook: a long boxy fuselage, a tall rear pylon and a
/// low front one, two tandem rotors, sponsons and wheels; olive drab with
/// a yellow band so it reads from afar.
pub(super) fn chinook_mesh() -> MeshData {
    let olive = [0.27, 0.31, 0.2, 1.0];
    let dark = [0.12, 0.13, 0.11, 1.0];
    let glass = [0.15, 0.2, 0.26, 1.0];
    let yellow = [0.95, 0.75, 0.1, 1.0];
    let mut m = MeshData::default();
    m.boxed(Vec3::new(0.0, 2.0, 0.0), Vec3::new(15.0, 3.2, 3.6), 0.0, olive);
    // Nose and cockpit glass, the tail ramp.
    m.boxed(Vec3::new(8.0, 1.8, 0.0), Vec3::new(1.6, 2.6, 3.2), 0.0, olive);
    m.boxed(Vec3::new(8.4, 2.6, 0.0), Vec3::new(1.2, 1.0, 3.25), 0.0, glass);
    m.boxed(Vec3::new(-7.9, 1.6, 0.0), Vec3::new(1.2, 2.2, 3.4), 0.0, dark);
    // Pylons: the low front one, the tall rear one.
    m.boxed(Vec3::new(HUBS[0], 3.75, 0.0), Vec3::new(2.0, 1.0, 1.6), 0.0, olive);
    m.boxed(Vec3::new(HUBS[1], 3.6, 0.0), Vec3::new(3.6, 1.6, 2.0), 0.0, olive);
    // Sponsons, engines on the rear pylon, wheels.
    for z in [-2.0f32, 2.0] {
        m.boxed(Vec3::new(1.0, 0.9, z), Vec3::new(8.0, 1.0, 0.6), 0.0, olive);
        m.boxed(Vec3::new(-5.2, 3.4, z * 0.85), Vec3::new(3.2, 0.9, 0.9), 0.0, dark);
        for x in [5.5f32, -3.0] {
            m.boxed(Vec3::new(x, 0.35, z), Vec3::new(0.7, 0.7, 0.3), 0.0, dark);
        }
    }
    // A yellow band and lit beacons.
    m.boxed(Vec3::new(2.0, 2.0, 0.0), Vec3::new(0.6, 3.25, 3.65), 0.0, yellow);
    m.boxed(Vec3::new(0.0, 0.35, 0.0), Vec3::new(0.4, 0.2, 0.4), 0.0, [3.0, 0.3, 0.2, 1.0]);
    m
}

fn rotor_mesh() -> MeshData {
    let blade = [0.1, 0.1, 0.1, 1.0];
    let mut m = MeshData::default();
    // Three blades, 9 m each, and the hub.
    for k in 0..3 {
        let a = k as f32 / 3.0 * std::f32::consts::TAU;
        let d = Vec3::new(a.cos(), 0.0, a.sin()) * 4.5;
        m.boxed(d, Vec3::new(9.0, 0.08, 0.55), -a, blade);
    }
    m.boxed(Vec3::ZERO, Vec3::new(0.8, 0.5, 0.8), 0.0, [0.2, 0.2, 0.2, 1.0]);
    m
}

fn meshes_for(meshes: &mut Assets<Mesh>) -> AirliftMeshes {
    let mut sling = MeshData::default();
    sling.boxed(Vec3::new(0.0, -0.5, 0.0), Vec3::new(0.12, 1.0, 0.12), 0.0, [0.1, 0.1, 0.1, 1.0]);
    let mut load = MeshData::default();
    load.boxed(Vec3::new(0.0, 0.75, 0.0), Vec3::new(4.4, 1.1, 1.9), 0.0, [1.0, 1.0, 1.0, 1.0]);
    load.boxed(Vec3::new(-0.3, 1.55, 0.0), Vec3::new(2.3, 0.55, 1.7), 0.0, [0.15, 0.17, 0.2, 1.0]);
    AirliftMeshes { body: meshes.add(shared_mesh(&chinook_mesh(), true)), rotor: meshes.add(shared_mesh(&rotor_mesh(), true)), sling: meshes.add(shared_mesh(&sling, true)), load: meshes.add(shared_mesh(&load, true)) }
}

/// Draw each airlift under way; despawn those done.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_airlifts(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
    mats: Res<Materials>,
    time: Res<Time>,
    mut assets: Local<Option<AirliftMeshes>>,
    mut drawn: Local<HashMap<u64, Parts>>,
    mut load_mats: Local<HashMap<u8, Handle<StandardMaterial>>>,
    mut q: Query<(&mut Transform, &mut Visibility), With<AirliftTag>>,
) {
    let w = &sim.world;
    let lifts = w.airlifts();
    if lifts.is_empty() && drawn.is_empty() {
        return;
    }
    let a = assets.get_or_insert_with(|| meshes_for(&mut meshes));
    let live: HashSet<u64> = lifts.iter().map(|l| l.started * 1_000_003 + (l.from.x.to_bits() as u64 & 0xFFFF)).collect();
    let gone: Vec<u64> = drawn.keys().copied().filter(|k| !live.contains(k) || sim.full_rebuild).collect();
    for k in gone {
        if let Some(ents) = drawn.remove(&k) {
            for e in ents {
                commands.entity(e).despawn();
            }
        }
    }
    let f = sim.frac();
    let spin = time.elapsed_secs() * 14.0;
    for l in lifts {
        let key = l.started * 1_000_003 + (l.from.x.to_bits() as u64 & 0xFFFF);
        let paint = l.model.paint;
        let ents = *drawn.entry(key).or_insert_with(|| {
            let panel = mats.surface(Surface::Panel);
            let lm = load_mats.entry(paint).or_insert_with(|| {
                let c = LOAD_PAINT[paint as usize % LOAD_PAINT.len()];
                materials.add(StandardMaterial { base_color: Color::linear_rgb(c[0], c[1], c[2]), perceptual_roughness: 0.4, metallic: 0.3, ..default() })
            });
            let lm = lm.clone();
            [
                commands.spawn((Mesh3d(a.body.clone()), MeshMaterial3d(panel.clone()), super::offstage(), AirliftTag)).id(),
                commands.spawn((Mesh3d(a.rotor.clone()), MeshMaterial3d(panel.clone()), super::offstage(), NotShadowCaster, AirliftTag)).id(),
                commands.spawn((Mesh3d(a.rotor.clone()), MeshMaterial3d(panel.clone()), super::offstage(), NotShadowCaster, AirliftTag)).id(),
                commands.spawn((Mesh3d(a.sling.clone()), MeshMaterial3d(panel), super::offstage(), NotShadowCaster, AirliftTag)).id(),
                commands.spawn((Mesh3d(a.load.clone()), MeshMaterial3d(lm), super::offstage(), AirliftTag)).id(),
            ]
        });
        let ground = w.surface_height_near(l.at, w.ground_height(l.at));
        let p = l.pose(w.clock.tick, f, ground);
        let yaw = Quat::from_rotation_y(-p.heading);
        let body = Vec3::new(p.pos.x, p.height, p.pos.y);
        let along = Vec3::new(p.heading.cos(), 0.0, p.heading.sin());
        // The load: on the road until hooked, then on the sling.
        let (load_at, load_yaw) = if p.carrying { (body - Vec3::Y * (SLING + 1.6), yaw) } else { (Vec3::new(l.at.x, ground, l.at.y), Quat::from_rotation_y(-l.heading)) };
        let show_load = p.carrying;
        let poses: [(Vec3, Quat, Vec3, bool); 5] = [
            (body, yaw, Vec3::ONE, true),
            (body + along * HUBS[0] + Vec3::Y * HUB_Y, Quat::from_rotation_y(spin), Vec3::ONE, true),
            (body + along * HUBS[1] + Vec3::Y * (HUB_Y + 0.4), Quat::from_rotation_y(-spin + 0.5), Vec3::ONE, true),
            (body, yaw, Vec3::new(1.0, SLING, 1.0), p.carrying),
            (load_at, load_yaw, Vec3::ONE, show_load),
        ];
        for (k, e) in ents.iter().enumerate() {
            if let Ok((mut tf, mut v)) = q.get_mut(*e) {
                tf.translation = poses[k].0;
                tf.rotation = poses[k].1;
                tf.scale = poses[k].2;
                *v = if poses[k].3 { Visibility::Inherited } else { Visibility::Hidden };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [airlift] The helicopter is a tandem-rotor one: a long fuselage, and
    /// the rotors far apart along it, front and back.
    #[test]
    fn the_airlift_helicopter_has_tandem_rotors() {
        let m = chinook_mesh();
        let xs: Vec<f32> = m.positions.iter().map(|p| p[0]).collect();
        let len = xs.iter().cloned().fold(f32::MIN, f32::max) - xs.iter().cloned().fold(f32::MAX, f32::min);
        assert!(len > 15.0, "a long fuselage: {len}");
        assert!(HUBS[0] - HUBS[1] > 10.0 && HUBS[0] > 0.0 && HUBS[1] < 0.0);
    }
}
