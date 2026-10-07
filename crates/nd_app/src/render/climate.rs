//! [climate] Weather you can see. When the sky is heavy (`world.weather.cloud`
//! over [`CLOUD_START`]) it rains on Temperate and Tropical maps and snows
//! on Arctic and Volcanic ones: a few blocks of streaks or flakes round the
//! camera, scrolled down (and downwind) every frame, one shared mesh and one
//! material, nothing per particle on the CPU. Off at the Low graphics tier.
//!
//! Steam rises off every hot spot in soft puffs (much more of it on a
//! volcanic map, and in the cold), billboards like the coal smoke's.

use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use nd_procgen::climate::{look, Precip};
use nd_sim::World;

use super::look::Quality;
use super::{haze, smoothstep, Lighting, RenderStats};
use crate::sim::Sim;

/// Cloud cover at which rain or snow starts; it is heaviest at full cover.
pub const CLOUD_START: f32 = 0.6;
/// Side and height of one block of falling particles, metres.
const TILE: f32 = 120.0;
/// Particles per block.
const RAIN_DROPS: usize = 3000;
const SNOW_FLAKES: usize = 4000;
/// Blocks: 2 x 2 round the camera, 3 high.
const LAYERS: usize = 3;

/// What is falling now and how hard (0..1), if anything.
pub fn precip_now(w: &World) -> Option<(&'static str, f32)> {
    let kind = match look(w.landscape.params.climate).precip {
        Precip::None => return None,
        Precip::Rain => "rain",
        Precip::Snow => "snow",
    };
    let i = ((w.weather.cloud - CLOUD_START) / (1.0 - CLOUD_START)).clamp(0.0, 1.0);
    (i > 0.0).then_some((kind, i))
}

#[derive(Component)]
pub(super) struct Precipitation {
    block: usize,
}

#[derive(Default)]
pub(super) struct PrecipState {
    rain: Option<Handle<Mesh>>,
    snow: Option<Handle<Mesh>>,
    material: Option<Handle<StandardMaterial>>,
    spawned: bool,
    shown: Option<Precip>,
}

fn hash(k: u32) -> f32 {
    let mut x = k.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A_2D39);
    x ^= x >> 15;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// One block of particles: `n` of them in a `TILE` cube (origin at a
/// corner), each two crossed vertical quads (rain: long thin streaks;
/// snow: small flakes with a flat one across).
fn particle_block(snow: bool) -> Mesh {
    let n = if snow { SNOW_FLAKES } else { RAIN_DROPS };
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    let salt = if snow { 7_777 } else { 0 };
    for i in 0..n as u32 {
        let k = i * 4 + salt;
        let c = Vec3::new(hash(k) * TILE, hash(k + 1) * TILE, hash(k + 2) * TILE);
        let a = hash(k + 3) * std::f32::consts::PI;
        let (w, h) = if snow { (0.09, 0.09) } else { (0.035, 2.2) };
        let mut quad = |u: Vec3, v: Vec3| {
            let b = pos.len() as u32;
            for p in [c - u * w - v * h, c + u * w - v * h, c + u * w + v * h, c - u * w + v * h] {
                pos.push(p.to_array());
            }
            idx.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
        };
        let u = Vec3::new(a.cos(), 0.0, a.sin());
        quad(u, Vec3::Y * 0.5);
        quad(Vec3::new(-u.z, 0.0, u.x), Vec3::Y * 0.5);
        if snow {
            quad(u, Vec3::new(-u.z, 0.0, u.x));
        }
    }
    let normals = vec![[0.0, 1.0, 0.0]; pos.len()];
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    m.insert_indices(Indices::U32(idx));
    m
}

/// Daylight on unlit particles: full by day, dim at night (the city's
/// glow still catches them a little).
fn daylight(w: &World, lighting: &Lighting) -> f32 {
    let t = if lighting.follow_clock { w.clock.day_fraction() } else { (lighting.hour / 24.0).rem_euclid(1.0) };
    let elev = ((t - 0.25) * std::f32::consts::TAU).sin();
    0.22 + 0.78 * smoothstep(-0.12, 0.3, elev)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_precipitation(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
    lighting: Res<Lighting>,
    stats: Res<RenderStats>,
    time: Res<Time>,
    mut state: Local<PrecipState>,
    cams: Query<&GlobalTransform, With<Camera3d>>,
    mut blocks: Query<(&Precipitation, &mut Transform, &mut Visibility, &mut Mesh3d)>,
) {
    let w = &sim.world;
    let now = precip_now(w).filter(|_| stats.graphics.tier != Quality::Low && !lighting.heat);
    let Some((kind, intensity)) = now else {
        if state.shown.take().is_some() {
            for (_, _, mut vis, _) in &mut blocks {
                *vis = Visibility::Hidden;
            }
        }
        return;
    };
    let snow = kind == "snow";
    if !state.spawned {
        state.spawned = true;
        state.rain = Some(meshes.add(particle_block(false)));
        state.snow = Some(meshes.add(particle_block(true)));
        let mat = materials.add(StandardMaterial { base_color: Color::NONE, unlit: true, alpha_mode: AlphaMode::Blend, double_sided: true, cull_mode: None, fog_enabled: true, ..default() });
        state.material = Some(mat.clone());
        let mesh = state.rain.clone().unwrap_or_default();
        for block in 0..4 * LAYERS {
            commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), Transform::IDENTITY, Visibility::Hidden, NotShadowCaster, NotShadowReceiver, Precipitation { block }));
        }
        return;
    }
    let mesh = if snow { state.snow.clone() } else { state.rain.clone() }.unwrap_or_default();
    state.shown = Some(if snow { Precip::Snow } else { Precip::Rain });
    if let Some(mut m) = state.material.as_ref().and_then(|h| materials.get_mut(h)) {
        let light = daylight(w, &lighting);
        let (rgb, a) = if snow { ([0.86, 0.89, 0.95], 0.85) } else { ([0.62, 0.68, 0.78], 0.32) };
        let a = a * (0.35 + 0.65 * intensity);
        let want = Color::linear_rgba(rgb[0] * light, rgb[1] * light, rgb[2] * light, a);
        if m.base_color != want {
            m.base_color = want;
        }
    }
    let Some(cam) = cams.iter().next() else { return };
    let eye = cam.translation();
    let t = time.elapsed_secs();
    // Rain falls fast and straight; snow slowly, drifting downwind.
    let (fall, drift) = if snow { (1.6, 1.2 + 3.0 * w.weather.wind) } else { (18.0, 1.0 + 4.0 * w.weather.wind) };
    let dir = haze::wind_dir(w.clock.tick);
    let shift = Vec3::new(dir.x * drift * t, 0.0, dir.y * drift * t);
    let shift = Vec3::new(shift.x.rem_euclid(TILE), 0.0, shift.z.rem_euclid(TILE));
    let drop = (t * fall).rem_euclid(TILE);
    let base = Vec3::new(((eye.x - shift.x) / TILE - 0.5).floor() * TILE + shift.x, (eye.y / TILE - 1.0).floor() * TILE - drop, ((eye.z - shift.z) / TILE - 0.5).floor() * TILE + shift.z);
    for (p, mut tf, mut vis, mut m) in &mut blocks {
        let (ix, iz, layer) = (p.block % 2, (p.block / 2) % 2, p.block / 4);
        if m.0 != mesh {
            m.0 = mesh.clone();
        }
        let corner = base + Vec3::new(ix as f32 * TILE, layer as f32 * TILE, iz as f32 * TILE);
        // Each block turned a quarter differently so the repeat does not show.
        let quarter = (p.block % 4) as f32 * std::f32::consts::FRAC_PI_2;
        let centre = corner + Vec3::splat(TILE * 0.5);
        *tf = Transform::from_translation(centre).with_rotation(Quat::from_rotation_y(quarter)) * Transform::from_translation(Vec3::splat(-TILE * 0.5));
        *vis = Visibility::Inherited;
    }
}

// ── steam over hot spots ────────────────────────────────────────────────────

/// Puffs per vent at the base steam scale.
const STEAM_PUFFS: usize = 7;
/// Vents per hot spot.
const VENTS: usize = 3;
/// Seconds a puff takes from the vent to gone.
const STEAM_LIFE: f32 = 9.0;
const STEAM_STEPS: usize = 8;
const STEAM_RGB: [f32; 3] = [0.84, 0.86, 0.9];

#[derive(Component)]
pub(super) struct Steam {
    vent: usize,
    /// This puff and how many share the vent.
    k: usize,
    per: usize,
}

#[derive(Default)]
pub(super) struct SteamState {
    key: Option<(u64, usize, u64)>,
    vents: Vec<Vec3>,
    entities: Vec<Entity>,
    quad: Option<Handle<Mesh>>,
    materials: Vec<Handle<StandardMaterial>>,
    light_key: Option<i32>,
    scale: f32,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_steam(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
    lighting: Res<Lighting>,
    time: Res<Time>,
    mut state: Local<SteamState>,
    cams: Query<&GlobalTransform, With<Camera3d>>,
    mut puffs: Query<(&Steam, &mut Transform, &mut MeshMaterial3d<StandardMaterial>, &mut Visibility)>,
) {
    let w = &sim.world;
    if state.quad.is_none() {
        state.quad = Some(meshes.add(Rectangle::new(1.0, 1.0)));
        let tex = images.add(haze::puff_image());
        for k in 0..STEAM_STEPS {
            let a = 0.7 * (k + 1) as f32 / STEAM_STEPS as f32;
            state.materials.push(materials.add(StandardMaterial {
                base_color: Color::linear_rgba(STEAM_RGB[0], STEAM_RGB[1], STEAM_RGB[2], a),
                base_color_texture: Some(tex.clone()),
                unlit: true,
                alpha_mode: AlphaMode::Blend,
                double_sided: true,
                cull_mode: None,
                ..default()
            }));
        }
    }
    let land = &w.landscape;
    let key = (sim.generation, land.hot_spots.len(), land.params.seed ^ land.params.climate as u64 ^ w.roads.version << 8 ^ (w.buildings.len() as u64) << 40);
    if state.key != Some(key) || sim.full_rebuild {
        state.key = Some(key);
        for e in state.entities.drain(..) {
            commands.entity(e).despawn();
        }
        state.vents.clear();
        let scale = look(land.params.climate).steam;
        state.scale = scale;
        let per = ((STEAM_PUFFS as f32 * scale.sqrt()).round() as usize).max(3);
        let quad = state.quad.clone().unwrap_or_default();
        let mat = state.materials[0].clone();
        for (i, s) in land.hot_spots.iter().enumerate() {
            for v in 0..VENTS {
                let a = hash(i as u32 * 31 + v as u32 * 7 + 1) * std::f32::consts::TAU;
                let r = hash(i as u32 * 31 + v as u32 * 7 + 2) * s.radius * 0.6;
                let p = s.pos + nd_sim::Vec2::from_angle(a) * r;
                if !nd_procgen::climate::vent_clear(w, p) {
                    continue;
                }
                let y = w.ground_height(p);
                let vent = state.vents.len();
                state.vents.push(Vec3::new(p.x, y, p.y));
                for k in 0..per {
                    let e = commands.spawn((Mesh3d(quad.clone()), MeshMaterial3d(mat.clone()), Transform::from_scale(Vec3::ZERO), NotShadowCaster, NotShadowReceiver, Steam { vent, k, per })).id();
                    state.entities.push(e);
                }
            }
        }
    }
    if state.entities.is_empty() {
        return;
    }
    let light = daylight(w, &lighting);
    let light_key = (light * 48.0).round() as i32;
    if state.light_key != Some(light_key) {
        state.light_key = Some(light_key);
        for (k, h) in state.materials.iter().enumerate() {
            let a = 0.7 * (k + 1) as f32 / STEAM_STEPS as f32;
            if let Some(mut m) = materials.get_mut(h) {
                m.base_color = Color::linear_rgba(STEAM_RGB[0] * light, STEAM_RGB[1] * light, STEAM_RGB[2] * light, a);
            }
        }
    }
    let Some(cam) = cams.iter().next() else { return };
    let face = cam.compute_transform().rotation;
    let now = time.elapsed_secs();
    let dir = haze::wind_dir(w.clock.tick);
    let wind = w.weather.wind.clamp(0.05, 1.0);
    let along = Vec3::new(dir.x, 0.0, dir.y);
    let scale = state.scale;
    for (s, mut tf, mut mat, mut vis) in &mut puffs {
        let Some(vent) = state.vents.get(s.vent) else { continue };
        let (k, per) = (s.k, s.per.max(1));
        let age = (now / STEAM_LIFE + k as f32 / per as f32 + hash(s.vent as u32) * 3.0).fract();
        let rise = (6.0 + 16.0 * scale) * age.powf(0.7);
        let pos = *vent + Vec3::Y * (1.0 + rise) + along * (age * age * (4.0 + 14.0 * wind) * scale.sqrt());
        let size = 1.5 + (4.0 + 5.0 * scale) * age.powf(0.8);
        let a = smoothstep(0.0, 0.1, age) * (1.0 - age).powf(1.5) * (0.6 + 0.25 * scale).min(1.0);
        if a < 0.5 / STEAM_STEPS as f32 {
            *vis = Visibility::Hidden;
            continue;
        }
        *vis = Visibility::Inherited;
        let step = ((a * STEAM_STEPS as f32).round() as usize).clamp(1, STEAM_STEPS) - 1;
        if mat.0 != state.materials[step] {
            mat.0 = state.materials[step].clone();
        }
        tf.translation = pos;
        tf.rotation = face;
        tf.scale = Vec3::splat(size);
    }
}
