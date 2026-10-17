//! [airlift] The airlift helicopter: a twin-rotor heavy-lift (Chinook-
//! style) machine flying in, hovering over a stuck vehicle, and flying it
//! away on a sling (`nd_sim::world::airlift`).

use super::*;

#[derive(Component)]
pub(super) struct AirliftTag;

/// Fuselage, front and rear rotor blades, front and rear rotor discs,
/// sling, the load.
type Parts = [Entity; 7];

pub(super) struct AirliftMeshes {
    body: Handle<Mesh>,
    rotor: Handle<Mesh>,
    disc: Handle<Mesh>,
    blade_mat: Handle<StandardMaterial>,
    disc_mat: Handle<StandardMaterial>,
    sling: Handle<Mesh>,
    load: Handle<Mesh>,
}

/// The slung vehicle's paint, by its model's paint index.
const LOAD_PAINT: [[f32; 3]; 6] = [[0.7, 0.1, 0.1], [0.1, 0.2, 0.55], [0.8, 0.8, 0.82], [0.08, 0.08, 0.09], [0.85, 0.6, 0.1], [0.15, 0.4, 0.25]];

/// Rotor hubs along the fuselage (front, back) and their height.
const HUBS: [f32; 2] = [6.4, -6.4];
const HUB_Y: f32 = 4.3;
/// [rotors] The rear rotor rides this much higher than the front (on its
/// tall pylon), as on a real tandem: the discs overlap in plan, never in
/// height.
const REAR_LIFT: f32 = 1.1;
/// Blade length, hub to tip, metres.
const BLADE: f32 = 9.0;
/// [rotors] Rotor speed, radians per scene second (about 2.2 turns).
const ROTOR_RATE: f32 = 14.0;
/// [rotors] Past this many radians a frame the eye sees a blur, not
/// blades: the rotor is drawn as a faint disc with soft streaks.
const BLUR_STEP: f32 = 0.11;
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

/// [rotors] Three thin blades tapering from a 0.62 m root to a 0.42 m tip
/// (a slight droop to the tip), and the hub: drawn while the rotor turns
/// slowly or stands (paused). White vertices: the dark blade material
/// gives the colour.
pub(super) fn rotor_mesh() -> MeshData {
    let blade = [1.0, 1.0, 1.0, 1.0];
    let mut m = MeshData::default();
    for k in 0..3 {
        let a = k as f32 / 3.0 * std::f32::consts::TAU;
        // Rotation about +Y turns +X toward -Z: blade k points at angle a
        // in the same sense as `Quat::from_rotation_y`.
        let (d, n) = (Vec3::new(a.cos(), 0.0, -a.sin()), Vec3::new(a.sin(), 0.0, a.cos()));
        let (r0, r1) = (0.5, BLADE);
        let (c0, c1) = (0.31, 0.21);
        let (y0, y1) = (0.0, -0.18);
        let t = 0.035;
        let at = |r: f32, c: f32, y: f32, dy: f32| d * r + n * c + Vec3::Y * (y + dy);
        let top = [at(r0, -c0, y0, t), at(r1, -c1, y1, t), at(r1, c1, y1, t), at(r0, c0, y0, t)];
        let bot = [at(r0, -c0, y0, -t), at(r1, -c1, y1, -t), at(r1, c1, y1, -t), at(r0, c0, y0, -t)];
        m.quad_facing(top, Vec3::Y, blade);
        m.quad_facing(bot, -Vec3::Y, blade);
        m.quad_facing([bot[0], bot[1], top[1], top[0]], -n, blade);
        m.quad_facing([bot[3], bot[2], top[2], top[3]], n, blade);
        m.quad_facing([bot[1], bot[2], top[2], top[1]], d, blade);
    }
    m.boxed(Vec3::ZERO, Vec3::new(0.8, 0.5, 0.8), 0.0, [0.6, 0.6, 0.6, 1.0]);
    m
}

/// [rotors] A turning rotor as the eye sees it: a faint disc, darker
/// toward the tips, with three soft streaks a third of a turn apart
/// (vertex alpha; the disc material blends).
pub(super) fn disc_mesh() -> MeshData {
    const SEGS: usize = 48;
    let mut m = MeshData::default();
    let rings = [(0.45, 0.04), (BLADE * 0.55, 0.1), (BLADE, 0.16)];
    let at = |i: usize, r: f32| {
        let a = i as f32 / SEGS as f32 * std::f32::consts::TAU;
        Vec3::new(a.cos() * r, -0.09 * r / BLADE, a.sin() * r)
    };
    let streak = |i: usize| {
        let a = (i as f32 / SEGS as f32 * 3.0).fract();
        1.0 + 1.4 * (1.0 - (a * 2.0 - 1.0).abs()).powi(3)
    };
    for w in rings.windows(2) {
        let ((ri, ai), (ro, ao)) = (w[0], w[1]);
        for i in 0..SEGS {
            let j = (i + 1) % SEGS;
            let s = streak(i).max(streak(j));
            let ca = [0.03, 0.03, 0.035, (ai * s).min(0.6)];
            let co = [0.03, 0.03, 0.035, (ao * s).min(0.6)];
            let base = m.positions.len() as u32;
            for (p, c) in [(at(i, ri), ca), (at(j, ri), ca), (at(j, ro), co), (at(i, ro), co)] {
                m.positions.push(p.to_array());
                m.normals.push([0.0, 1.0, 0.0]);
                m.colors.push(c);
                m.uvs.push([0.5, 0.5]);
                m.tangents.push([1.0, 0.0, 0.0, 1.0]);
            }
            m.indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
        }
    }
    m
}

/// [rotors] The two rotors' angles (front, back) about the fuselage's own
/// up axis at `spin` radians: counter-rotating, and phased so the rear's
/// blades are the front's mirror image across the gap between the hubs.
/// Mirrored blades of three-bladed rotors never meet where the discs
/// overlap (the meshing of a real tandem).
pub(super) fn rotor_angles(spin: f32) -> (f32, f32) {
    (spin, -spin)
}

fn meshes_for(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> AirliftMeshes {
    let mut sling = MeshData::default();
    sling.boxed(Vec3::new(0.0, -0.5, 0.0), Vec3::new(0.12, 1.0, 0.12), 0.0, [0.1, 0.1, 0.1, 1.0]);
    let mut load = MeshData::default();
    load.boxed(Vec3::new(0.0, 0.75, 0.0), Vec3::new(4.4, 1.1, 1.9), 0.0, [1.0, 1.0, 1.0, 1.0]);
    load.boxed(Vec3::new(-0.3, 1.55, 0.0), Vec3::new(2.3, 0.55, 1.7), 0.0, [0.15, 0.17, 0.2, 1.0]);
    let blade_mat = materials.add(StandardMaterial { base_color: Color::linear_rgb(0.035, 0.036, 0.04), perceptual_roughness: 0.55, metallic: 0.2, ..default() });
    let disc_mat = materials.add(StandardMaterial { base_color: Color::WHITE, alpha_mode: AlphaMode::Blend, perceptual_roughness: 0.9, double_sided: true, cull_mode: None, ..default() });
    AirliftMeshes { body: meshes.add(shared_mesh(&chinook_mesh(), true)), rotor: meshes.add(shared_mesh(&rotor_mesh(), true)), disc: meshes.add(shared_mesh(&disc_mesh(), false)), blade_mat, disc_mat, sling: meshes.add(shared_mesh(&sling, true)), load: meshes.add(shared_mesh(&load, true)) }
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
    clock: Res<super::climate::SceneClock>,
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
    let a = assets.get_or_insert_with(|| meshes_for(&mut meshes, &mut materials));
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
    // [rotors] On the scene clock: still while paused, faster on
    // fast-forward; a blur once a frame turns them past BLUR_STEP.
    let spin = clock.t * ROTOR_RATE;
    let blur = time.delta_secs() * clock.rate * ROTOR_RATE > BLUR_STEP;
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
                commands.spawn((Mesh3d(a.rotor.clone()), MeshMaterial3d(a.blade_mat.clone()), super::offstage(), NotShadowCaster, AirliftTag)).id(),
                commands.spawn((Mesh3d(a.rotor.clone()), MeshMaterial3d(a.blade_mat.clone()), super::offstage(), NotShadowCaster, AirliftTag)).id(),
                commands.spawn((Mesh3d(a.disc.clone()), MeshMaterial3d(a.disc_mat.clone()), super::offstage(), NotShadowCaster, bevy::light::NotShadowReceiver, AirliftTag)).id(),
                commands.spawn((Mesh3d(a.disc.clone()), MeshMaterial3d(a.disc_mat.clone()), super::offstage(), NotShadowCaster, bevy::light::NotShadowReceiver, AirliftTag)).id(),
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
        let (front, back) = rotor_angles(spin);
        let (hub_f, hub_b) = (body + along * HUBS[0] + Vec3::Y * HUB_Y, body + along * HUBS[1] + Vec3::Y * (HUB_Y + REAR_LIFT));
        // The discs' streaks creep round slowly, each the way its rotor turns.
        let (disc_f, disc_b) = rotor_angles(spin * 0.07);
        let poses: [(Vec3, Quat, Vec3, bool); 7] = [
            (body, yaw, Vec3::ONE, true),
            (hub_f, yaw * Quat::from_rotation_y(front), Vec3::ONE, !blur),
            (hub_b, yaw * Quat::from_rotation_y(back), Vec3::ONE, !blur),
            (hub_f, yaw * Quat::from_rotation_y(disc_f), Vec3::ONE, blur),
            (hub_b, yaw * Quat::from_rotation_y(disc_b), Vec3::ONE, blur),
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

    /// [rotors] Counter-rotating, meshed tandem rotors: at every angle no
    /// front blade passes near a rear one where the discs overlap (in plan,
    /// before the rear's extra height), and the rear disc rides higher.
    #[test]
    fn tandem_rotor_blades_never_cross() {
        let gap = HUBS[0] - HUBS[1];
        assert!(gap < 2.0 * BLADE, "the discs overlap, as a Chinook's do");
        // Blade k at rotor angle a, in the rotor's own frame (from `rotor_mesh`),
        // turned by the rotor angle about +Y: (cos, -sin) in (x, z).
        let blades = |hub: f32, a: f32| (0..3).map(move |k| (hub, a + k as f32 / 3.0 * std::f32::consts::TAU));
        let mut closest = f32::MAX;
        for step in 0..360 {
            let (f, r) = rotor_angles(step as f32 / 360.0 * std::f32::consts::TAU);
            for (hf, bf) in blades(HUBS[0], f) {
                for (hr, br) in blades(HUBS[1], r) {
                    for i in 1..=18 {
                        for j in 1..=18 {
                            let (s, t) = (i as f32 / 18.0 * BLADE, j as f32 / 18.0 * BLADE);
                            let p = Vec2::new(hf + bf.cos() * s, -bf.sin() * s);
                            let q = Vec2::new(hr + br.cos() * t, -br.sin() * t);
                            closest = closest.min(p.distance(q));
                        }
                    }
                }
            }
        }
        assert!(closest > 1.0, "blades pass {closest} m apart");
        assert!(REAR_LIFT > 0.5);
    }

    /// [rotors] The blades are thin; the disc is see-through.
    #[test]
    fn rotor_blades_are_thin_and_the_disc_translucent() {
        let r = rotor_mesh();
        let ys: Vec<f32> = r.positions.iter().map(|p| p[1]).collect();
        assert!(ys.iter().cloned().fold(f32::MIN, f32::max) - ys.iter().cloned().fold(f32::MAX, f32::min) < 0.6);
        let d = disc_mesh();
        assert!(d.colors.iter().all(|c| c[3] > 0.0 && c[3] <= 0.6));
    }
}
