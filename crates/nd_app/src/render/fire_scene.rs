//! [fire-scene] A fire engine's crew at work: firefighters in turnout gear
//! at their places (`crate::fire_scene` says where and doing what), the
//! hose line on the ground, and water arcing from the nozzles onto the
//! fire, with a cloud of spray where it lands. The ladder and outriggers
//! are the engine's own rig (`emergency`).
//!
//! The water is instanced droplets streaming along each arc (fewer at the
//! Medium tier, a thin few at Low), each stretched along its path so the
//! stream reads as one jet.

use super::*;
use crate::fire_scene::{engine_scenes, Stream};
use nd_procgen::people::{firefighter_mesh, FirePose};
use nd_sim::UnitId;

#[derive(Component)]
pub(super) struct FireSceneTag;

pub(super) struct SceneAssets {
    figures: HashMap<(u8, FirePose), Handle<Mesh>>,
    cube: Handle<Mesh>,
    drop: Handle<Mesh>,
    hose_mat: Handle<StandardMaterial>,
    /// Flames in the windows of a tower's burning floors.
    flame: Handle<Mesh>,
    flame_mat: Handle<StandardMaterial>,
    water_mat: Handle<StandardMaterial>,
    mist_mat: Handle<StandardMaterial>,
}

#[derive(Default)]
pub(super) struct SceneEnts {
    figures: Vec<Entity>,
    hose: Vec<Entity>,
    drops: Vec<Entity>,
    mist: Vec<Entity>,
    windows: Vec<Entity>,
}

#[derive(Default)]
pub(super) struct FireSceneState {
    scenes: HashMap<UnitId, SceneEnts>,
}

/// Droplets in one stream by graphics tier.
fn drops_per_stream(tier: look::Quality) -> usize {
    match tier {
        look::Quality::Low => 10,
        look::Quality::Medium => 22,
        look::Quality::High => 40,
    }
}

const MIST_PUFFS: usize = 4;

fn spawn(commands: &mut Commands, mesh: &Handle<Mesh>, mat: &Handle<StandardMaterial>) -> Entity {
    commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), super::offstage(), Visibility::Hidden, NotShadowCaster, NotShadowReceiver, FireSceneTag)).id()
}

/// Make `v` hold exactly `n` entities.
fn pool(commands: &mut Commands, v: &mut Vec<Entity>, n: usize, mesh: &Handle<Mesh>, mat: &Handle<StandardMaterial>) {
    while v.len() < n {
        v.push(spawn(commands, mesh, mat));
    }
    while v.len() > n {
        if let Some(e) = v.pop() {
            commands.entity(e).despawn();
        }
    }
}

fn hash01(a: u64) -> f32 {
    let mut x = a.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x ^= x >> 31;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 29;
    (x % 10_000) as f32 / 10_000.0
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_fire_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut people: ResMut<people::PeopleRender>,
    sim: Res<Sim>,
    game: Res<GameAssets>,
    stats: Res<RenderStats>,
    time: Res<Time>,
    mut assets: Local<Option<SceneAssets>>,
    mut st: Local<FireSceneState>,
    mut q: Query<(&mut Transform, &mut Visibility, &mut Mesh3d), With<FireSceneTag>>,
) {
    let w = &sim.world;
    if sim.full_rebuild {
        for (_, s) in st.scenes.drain() {
            for e in s.figures.into_iter().chain(s.hose).chain(s.drops).chain(s.mist).chain(s.windows) {
                commands.entity(e).despawn();
            }
        }
    }
    let scenes = engine_scenes(w, sim.frac(), &game.rules, &sim.building_tops);
    if scenes.is_empty() && st.scenes.is_empty() {
        return;
    }
    let a = assets.get_or_insert_with(|| {
        let cube = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
        let drop = meshes.add(Sphere::new(0.5).mesh().ico(1).unwrap());
        let hose_mat = materials.add(StandardMaterial { base_color: Color::linear_rgb(0.85, 0.55, 0.08), perceptual_roughness: 0.7, ..default() });
        let water_mat = materials.add(StandardMaterial {
            base_color: Color::linear_rgba(0.45, 0.65, 0.95, 0.42),
            emissive: LinearRgba::rgb(0.05, 0.09, 0.16),
            perceptual_roughness: 0.1,
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        let mist_mat = materials.add(StandardMaterial { base_color: Color::linear_rgba(0.85, 0.9, 0.95, 0.16), perceptual_roughness: 1.0, alpha_mode: AlphaMode::Blend, ..default() });
        // A row of tongues of flame bursting from windows, along +x.
        let flame = {
            let mut m = MeshData::default();
            for k in 0..11 {
                let x = -2.5 + k as f32 * 0.5;
                let h = 0.5 + 0.9 * hash01(k as u64 + 5);
                let col = if k % 3 == 0 { [1.9, 1.05, 0.12, 1.0] } else if k % 3 == 1 { [1.7, 0.42, 0.04, 1.0] } else { [1.8, 0.7, 0.06, 1.0] };
                m.boxed(Vec3::new(x, h * 0.5, 0.2 + 0.15 * hash01(k as u64 + 40)), Vec3::new(0.24, h, 0.24), 0.3 * k as f32, col);
            }
            // The glow of the room behind, in the window.
            m.boxed(Vec3::new(0.0, 0.45, 0.02), Vec3::new(5.2, 0.9, 0.05), 0.0, [1.4, 0.35, 0.03, 1.0]);
            meshes.add(super::shared_mesh(&m, false))
        };
        let flame_mat = materials.add(StandardMaterial { base_color: Color::WHITE, unlit: true, ..default() });
        SceneAssets { figures: HashMap::new(), cube, drop, hose_mat, water_mat, mist_mat, flame, flame_mat }
    });
    let gone: Vec<UnitId> = st.scenes.keys().copied().filter(|u| !scenes.iter().any(|s| s.unit == *u)).collect();
    for u in gone {
        if let Some(s) = st.scenes.remove(&u) {
            for e in s.figures.into_iter().chain(s.hose).chain(s.drops).chain(s.mist).chain(s.windows) {
                commands.entity(e).despawn();
            }
        }
    }
    let people_mat = people.material(&mut materials, &mut images);
    let t = time.elapsed_secs();
    let n_drops = drops_per_stream(stats.graphics.tier);
    for scene in &scenes {
        let ents = st.scenes.entry(scene.unit).or_default();
        // ── the crew ──
        pool(&mut commands, &mut ents.figures, scene.figures.len(), &a.cube, &people_mat);
        for (i, fig) in scene.figures.iter().enumerate() {
            let mesh = a.figures.entry((fig.skin, fig.pose)).or_insert_with(|| meshes.add(super::shared_mesh(&firefighter_mesh(fig.skin, fig.pose), false))).clone();
            let Ok((mut tf, mut vis, mut m)) = q.get_mut(ents.figures[i]) else { continue };
            if m.0 != mesh {
                m.0 = mesh;
            }
            *vis = if fig.out { Visibility::Inherited } else { Visibility::Hidden };
            let bob = if fig.walking { (t * 9.0 + i as f32).sin().abs() * 0.06 } else { 0.0 };
            // Braced on the hose: leaning into the nozzle's kick.
            let lean = if fig.pose == FirePose::Nozzle { Quat::from_rotation_z(-0.12) } else { Quat::IDENTITY };
            tf.translation = fig.pos + Vec3::new(0.0, 0.04 + bob, 0.0);
            tf.rotation = Quat::from_rotation_y(-fig.yaw) * lean;
            tf.scale = Vec3::splat(fig.stature);
        }
        // ── hoses: a run of short tubes along each line ──
        let segs: Vec<(Vec3, Vec3)> = scene.hoses.iter().flat_map(|h| h.windows(2).map(|p| (p[0], p[1]))).collect();
        pool(&mut commands, &mut ents.hose, segs.len(), &a.cube, &a.hose_mat);
        for (k, (p0, p1)) in segs.iter().enumerate() {
            let Ok((mut tf, mut vis, _)) = q.get_mut(ents.hose[k]) else { continue };
            let d = *p1 - *p0;
            let len = d.length();
            *vis = if len > 0.01 { Visibility::Inherited } else { Visibility::Hidden };
            tf.translation = (*p0 + *p1) * 0.5;
            tf.rotation = Quat::from_rotation_arc(Vec3::X, d / len.max(1e-4));
            tf.scale = Vec3::new(len + 0.04, 0.11, 0.11);
        }
        // ── a tower's burning floors, where the ladder works ──
        let floors = if scene.tower && scene.intensity > 0.0 { 3 } else { 0 };
        pool(&mut commands, &mut ents.windows, floors, &a.flame, &a.flame_mat);
        for k in 0..floors {
            let Ok((mut tf, mut vis, _)) = q.get_mut(ents.windows[k]) else { continue };
            *vis = Visibility::Inherited;
            let along = SVec2::new(-scene.facing.y, scene.facing.x);
            let off = (k as f32 - 1.0) * 3.4 - 1.2;
            let p = scene.aim + Vec3::new(scene.facing.x, 0.0, scene.facing.y) * 0.2 + Vec3::new(along.x, 0.0, along.y) * (k as f32 - 1.0) * 1.5;
            let flick = 0.8 + 0.25 * (t * 8.0 + k as f32 * 1.7).sin() + 0.1 * (t * 21.0 + k as f32).sin();
            let s = 0.75 + 0.45 * scene.intensity;
            tf.translation = p + Vec3::Y * off;
            tf.rotation = Quat::from_rotation_y(scene.facing.x.atan2(scene.facing.y));
            tf.scale = Vec3::new(s, s * flick * 1.3, s);
        }
        // ── water ──
        pool(&mut commands, &mut ents.drops, scene.streams.len() * n_drops, &a.drop, &a.water_mat);
        pool(&mut commands, &mut ents.mist, scene.streams.len() * MIST_PUFFS, &a.drop, &a.mist_mat);
        for (si, s) in scene.streams.iter().enumerate() {
            stream(s, si, n_drops, t, &ents.drops[si * n_drops..(si + 1) * n_drops], &mut q);
            for k in 0..MIST_PUFFS {
                let Ok((mut tf, mut vis, _)) = q.get_mut(ents.mist[si * MIST_PUFFS + k]) else { continue };
                *vis = if s.flow > 0.4 { Visibility::Inherited } else { Visibility::Hidden };
                let ph = (t * 0.7 + k as f32 / MIST_PUFFS as f32).fract();
                let jit = Vec3::new(hash01(k as u64 * 3 + si as u64) - 0.5, 0.0, hash01(k as u64 * 7 + 1) - 0.5) * 3.0;
                tf.translation = s.to + jit + Vec3::Y * (ph * 2.0 - 0.5);
                tf.scale = Vec3::new(1.4, 0.8, 1.4) * (0.6 + ph * 1.8) * s.flow;
            }
        }
    }
}

/// Droplets along one stream, scrolling from the nozzle to where it lands:
/// each stretched along the arc so they join into a jet, widening and
/// scattering as the stream breaks up towards the end.
fn stream(s: &Stream, si: usize, n: usize, t: f32, ents: &[Entity], q: &mut Query<(&mut Transform, &mut Visibility, &mut Mesh3d), With<FireSceneTag>>) {
    let length = {
        let mut l = 0.0;
        let mut prev = s.at(0.0);
        for k in 1..=8 {
            let p = s.at(k as f32 / 8.0);
            l += p.distance(prev);
            prev = p;
        }
        l
    };
    // A jet moves at ~25 m/s; at least a couple of loops a second.
    let rate = (25.0 / length.max(1.0)).clamp(0.6, 3.0);
    let spacing = length / n as f32;
    for (i, e) in ents.iter().enumerate() {
        let Ok((mut tf, mut vis, _)) = q.get_mut(*e) else { continue };
        let u = (i as f32 / n as f32 + t * rate).fract();
        // Ramping up the jet reaches out from the nozzle; shutting off, the
        // last of it falls away.
        if s.flow <= 0.0 || u > s.flow * 1.02 {
            *vis = Visibility::Hidden;
            continue;
        }
        *vis = Visibility::Inherited;
        let p = s.at(u);
        let tangent = (s.at((u + 0.02).min(1.0)) - s.at((u - 0.02).max(0.0))).normalize_or(Vec3::X);
        let spread = u * u;
        let seed = (si * 1000 + i) as u64;
        let jit = Vec3::new(hash01(seed) - 0.5, hash01(seed + 77) - 0.5, hash01(seed + 151) - 0.5) * (spread * 0.9);
        tf.translation = p + jit;
        tf.rotation = Quat::from_rotation_arc(Vec3::X, tangent);
        let width = 0.18 + 0.4 * spread;
        tf.scale = Vec3::new(spacing * 1.9 + 0.3, width, width);
    }
}
