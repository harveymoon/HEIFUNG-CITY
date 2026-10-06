//! Coal smoke: plumes of soft translucent puffs that leave the stacks, rise,
//! drift downwind, grow and thin out, and a low brown smog of wide flat
//! puffs lying downwind of the plant. Every puff is a camera-facing quad
//! with a soft round texture, so it reads as smoke from above and from the
//! street alike; none casts or receives shadows, and the alpha stays low
//! enough to see the city through it. How thick it is follows the plant's
//! output (its staffing and how much of its capacity the grid draws).

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use nd_procgen::coal_stacks;
use nd_sim::{BuildingId, Vec2 as SVec2};
use std::collections::HashMap;

use super::{smoothstep, Lighting};
use crate::sim::Sim;

/// Puffs in flight per stack.
const PLUME_PUFFS: usize = 18;
/// Wide puffs of ground-level smog per plant.
const SMOG_PUFFS: usize = 14;
/// Seconds a plume puff takes from the stack top to gone.
const PLUME_LIFE: f32 = 36.0;
/// Alpha steps the puff materials come in.
const ALPHA_STEPS: usize = 12;
/// Most opaque a plume puff gets (at full output, just out of the stack).
const PLUME_ALPHA: f32 = 0.42;
/// Most opaque a smog puff gets.
const SMOG_ALPHA: f32 = 0.16;

#[derive(Component)]
pub(super) struct Puff {
    plant: BuildingId,
    /// Plume puffs: the stack index; smog puffs: `None`.
    stack: Option<usize>,
    k: usize,
}

struct Plant {
    stacks: Vec<Vec3>,
    centre: Vec3,
    intensity: f32,
    entities: Vec<Entity>,
    key: u64,
}

#[derive(Default)]
pub(super) struct HazeState {
    plants: HashMap<BuildingId, Plant>,
    quad: Option<Handle<Mesh>>,
    /// Plume then smog materials, `ALPHA_STEPS` each, darkened at night.
    materials: Vec<Handle<StandardMaterial>>,
    light_key: Option<i32>,
}

/// A soft round smoke texture: a gaussian falloff broken up by a few
/// lumps, white, so the material colour tints it.
fn puff_image() -> Image {
    const N: usize = 64;
    let mut data = vec![0u8; N * N * 4];
    let lumps = [(0.32f32, 0.40f32, 0.22f32), (0.62, 0.35, 0.2), (0.5, 0.62, 0.26), (0.3, 0.64, 0.17), (0.7, 0.62, 0.17)];
    for y in 0..N {
        for x in 0..N {
            let (u, v) = ((x as f32 + 0.5) / N as f32, (y as f32 + 0.5) / N as f32);
            let mut a = 0.0f32;
            for (cx, cy, r) in lumps {
                let d2 = ((u - cx).powi(2) + (v - cy).powi(2)) / (r * r);
                a = a.max((-d2 * 2.2).exp());
            }
            // Fade to nothing well inside the quad's edge.
            let edge = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt();
            a *= 1.0 - smoothstep(0.36, 0.5, edge);
            let i = (y * N + x) * 4;
            data[i] = 255;
            data[i + 1] = 255;
            data[i + 2] = 255;
            data[i + 3] = (a * 255.0) as u8;
        }
    }
    let mut img = Image::new(Extent3d { width: N as u32, height: N as u32, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor { address_mode_u: ImageAddressMode::ClampToEdge, address_mode_v: ImageAddressMode::ClampToEdge, ..ImageSamplerDescriptor::linear() });
    img
}

/// Prevailing wind direction (unit, sim plane): veers slowly over the days.
pub(super) fn wind_dir(tick: u64) -> SVec2 {
    let a = 0.55 + 0.9 * ((tick as f32 / 1440.0) * 0.7).sin();
    SVec2::new(a.cos(), a.sin())
}

fn hash01(a: u64, b: u64) -> f32 {
    let h = (a ^ b.wrapping_mul(0x9E37_79B9_7F4A_7C15)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    ((h >> 40) as f32) / (1u64 << 24) as f32
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_haze(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
    lighting: Res<Lighting>,
    time: Res<Time>,
    mut state: Local<HazeState>,
    cams: Query<&GlobalTransform, With<Camera3d>>,
    mut puffs: Query<(&Puff, &mut Transform, &mut MeshMaterial3d<StandardMaterial>, &mut Visibility)>,
) {
    let w = &sim.world;
    if state.quad.is_none() {
        state.quad = Some(meshes.add(Rectangle::new(1.0, 1.0)));
        let tex = images.add(puff_image());
        for (rgb, top) in [([0.40f32, 0.36, 0.32], PLUME_ALPHA), ([0.46, 0.34, 0.21], SMOG_ALPHA)] {
            for k in 0..ALPHA_STEPS {
                let a = top * (k + 1) as f32 / ALPHA_STEPS as f32;
                state.materials.push(materials.add(StandardMaterial {
                    base_color: Color::linear_rgba(rgb[0], rgb[1], rgb[2], a),
                    base_color_texture: Some(tex.clone()),
                    unlit: true,
                    alpha_mode: AlphaMode::Blend,
                    double_sided: true,
                    cull_mode: None,
                    ..default()
                }));
            }
        }
    }
    // Smoke is unlit, so it is shaded here: grey-brown by day, a dim
    // umber at night (lit a little from below by the city).
    let t = if lighting.follow_clock { w.clock.day_fraction() } else { (lighting.hour / 24.0).rem_euclid(1.0) };
    let elev = ((t - 0.25) * std::f32::consts::TAU).sin();
    let light = 0.16 + 0.84 * smoothstep(-0.12, 0.3, elev);
    let light_key = (light * 64.0).round() as i32;
    if state.light_key != Some(light_key) {
        state.light_key = Some(light_key);
        for (i, h) in state.materials.iter().enumerate() {
            let (rgb, top) = if i < ALPHA_STEPS { ([0.40f32, 0.36, 0.32], PLUME_ALPHA) } else { ([0.46, 0.34, 0.21], SMOG_ALPHA) };
            let a = top * ((i % ALPHA_STEPS) + 1) as f32 / ALPHA_STEPS as f32;
            if let Some(mut m) = materials.get_mut(h) {
                m.base_color = Color::linear_rgba(rgb[0] * light, rgb[1] * light * 0.97, rgb[2] * light * 0.92, a);
            }
        }
    }

    // Plants and how hard each is burning.
    let p = &w.economy.power;
    let draw = if p.supply_mw > 0.0 { (p.demand_mw / p.supply_mw).clamp(0.25, 1.0) } else { 1.0 };
    let mut seen = Vec::new();
    for (id, b) in w.buildings.iter() {
        if !b.plant.is_some_and(|k| k.makes_haze()) || b.construction.is_some() {
            continue;
        }
        let Some(lot) = w.lots.get(b.lot) else { continue };
        seen.push(id);
        let staffed = if b.jobs > 0 { (b.workers.len() as f32 / b.jobs as f32).clamp(0.3, 1.0) } else { 1.0 };
        // A running plant always smokes; harder the more of it the grid draws.
        let intensity = if b.is_derelict() { 0.0 } else { 0.55 + 0.45 * (staffed * draw).clamp(0.0, 1.0) };
        let pad = w.lot_pad(b.lot);
        let key = (lot.center.x.to_bits() as u64) << 32 | lot.center.y.to_bits() as u64 ^ pad.to_bits() as u64;
        if let Some(plant) = state.plants.get_mut(&id) {
            plant.intensity = intensity;
            if plant.key == key {
                continue;
            }
        }
        if let Some(old) = state.plants.remove(&id) {
            for e in old.entities {
                commands.entity(e).despawn();
            }
        }
        let stacks: Vec<Vec3> = coal_stacks(lot).into_iter().map(|s| s + Vec3::Y * pad).collect();
        let quad = state.quad.clone().unwrap_or_default();
        let mat = state.materials[0].clone();
        let mut entities = Vec::new();
        let mut spawn = |stack: Option<usize>, k: usize| {
            commands.spawn((Mesh3d(quad.clone()), MeshMaterial3d(mat.clone()), Transform::from_scale(Vec3::ZERO), NotShadowCaster, NotShadowReceiver, Puff { plant: id, stack, k })).id()
        };
        for s in 0..stacks.len() {
            for k in 0..PLUME_PUFFS {
                entities.push(spawn(Some(s), k));
            }
        }
        for k in 0..SMOG_PUFFS {
            entities.push(spawn(None, k));
        }
        state.plants.insert(id, Plant { stacks, centre: Vec3::new(lot.center.x, pad, lot.center.y), intensity, entities, key });
    }
    let gone: Vec<BuildingId> = state.plants.keys().copied().filter(|id| !seen.contains(id)).collect();
    for id in gone {
        if let Some(old) = state.plants.remove(&id) {
            for e in old.entities {
                commands.entity(e).despawn();
            }
        }
    }
    if state.plants.is_empty() {
        return;
    }

    // Move every puff: billboards facing the camera.
    let Some(cam) = cams.iter().next() else { return };
    let face = cam.compute_transform().rotation;
    let now = time.elapsed_secs();
    let dir = wind_dir(w.clock.tick);
    let wind = w.weather.wind.clamp(0.05, 1.0);
    let along = Vec3::new(dir.x, 0.0, dir.y);
    let side = Vec3::new(-dir.y, 0.0, dir.x);
    let reach = 70.0 + 190.0 * wind;
    for (puff, mut tf, mut mat, mut vis) in &mut puffs {
        let Some(plant) = state.plants.get(&puff.plant) else { continue };
        let seed = puff.plant_seed();
        let (pos, size, alpha, smog) = match puff.stack {
            Some(s) => {
                let Some(top) = plant.stacks.get(s) else { continue };
                // Age 0..1, puffs evenly spaced in time, each stack offset.
                let age = (now / PLUME_LIFE + puff.k as f32 / PLUME_PUFFS as f32 + s as f32 * 0.37).fract();
                let rise = 50.0 * (1.0 - (1.0 - age).powi(2)) * (1.15 - 0.6 * wind);
                let wobble = (now * 0.21 + puff.k as f32 * 1.9).sin() * 9.0 * age;
                let pos = *top + Vec3::Y * rise + along * (age * reach) + side * wobble;
                let size = 5.0 + 44.0 * age.powf(0.8);
                let alpha = smoothstep(0.0, 0.06, age) * (1.0 - age).powf(1.4);
                (pos, size, alpha, false)
            }
            None => {
                // Smog: wide, flat-lying puffs downwind, breathing slowly.
                let r1 = hash01(seed, puff.k as u64 * 2 + 1);
                let r2 = hash01(seed, puff.k as u64 * 2 + 2);
                let d = 25.0 + (puff.k as f32 + r1) / SMOG_PUFFS as f32 * reach * 1.3;
                let lateral = (r2 - 0.5) * (40.0 + d * 0.6);
                let drift = (now * 0.05 + puff.k as f32).sin() * 10.0;
                let h = 7.0 + 18.0 * r1 + d * 0.05;
                let pos = plant.centre + along * (d + drift) + side * lateral + Vec3::Y * h;
                let size = 45.0 + 35.0 * r2 + d * 0.15;
                let alpha = 0.55 + 0.45 * (now * 0.08 + puff.k as f32 * 2.1).sin().abs();
                (pos, size, alpha, true)
            }
        };
        let a = alpha * plant.intensity;
        if a < 0.5 / ALPHA_STEPS as f32 {
            *vis = Visibility::Hidden;
            continue;
        }
        *vis = Visibility::Inherited;
        let step = ((a * ALPHA_STEPS as f32).round() as usize).clamp(1, ALPHA_STEPS) - 1;
        let want = &state.materials[if smog { ALPHA_STEPS + step } else { step }];
        if mat.0 != *want {
            mat.0 = want.clone();
        }
        tf.translation = pos;
        tf.rotation = face;
        // Smog lies flatter than it is tall from the street.
        tf.scale = if smog { Vec3::new(size, size * 0.45, 1.0) } else { Vec3::splat(size) };
    }
}

impl Puff {
    fn plant_seed(&self) -> u64 {
        use slotmap::Key;
        self.plant.data().as_ffi()
    }
}
