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
use nd_procgen::{coal_vents, CoalVent};
use nd_sim::{BuildingId, Vec2 as SVec2};
use std::collections::HashMap;

use super::{smoothstep, Lighting};
use crate::sim::Sim;

/// Puffs in flight per stack: [look] enough that they overlap into one
/// column rather than a string of beads (seen from the street most of all).
const PLUME_PUFFS: usize = 30;
/// Wide puffs of ground-level smog per plant.
const SMOG_PUFFS: usize = 14;
/// Seconds a plume puff takes from the stack top to gone.
const PLUME_LIFE: f32 = 36.0;
/// Alpha steps the puff materials come in.
const ALPHA_STEPS: usize = 12;
/// Most opaque a plume puff gets (at full output, just out of the stack).
const PLUME_ALPHA: f32 = 0.92;
/// Plume colour: [plant-scale] a dark sooty grey, heavy against the sky
/// (still lighter than the black plant under it).
const PLUME_RGB: [f32; 3] = [0.19, 0.18, 0.17];
/// Steam from a coal station's cooling towers: white.
const STEAM_RGB: [f32; 3] = [0.86, 0.87, 0.9];
const STEAM_ALPHA: f32 = 0.7;
/// Most opaque a smog puff gets.
const SMOG_ALPHA: f32 = 0.38;
/// Smog colour: a dirty brown.
const SMOG_RGB: [f32; 3] = [0.36, 0.27, 0.17];

#[derive(Component)]
pub(super) struct Puff {
    plant: BuildingId,
    /// Plume puffs: the stack index; smog puffs: `None`.
    stack: Option<usize>,
    k: usize,
}

struct Plant {
    stacks: Vec<CoalVent>,
    centre: Vec3,
    intensity: f32,
    entities: Vec<Entity>,
    key: u64,
}

#[derive(Default)]
pub(super) struct HazeState {
    plants: HashMap<BuildingId, Plant>,
    quad: Option<Handle<Mesh>>,
    /// Plume, smog then steam materials, `ALPHA_STEPS` each, darkened at night.
    materials: Vec<Handle<StandardMaterial>>,
    light_key: Option<i32>,
}

/// A soft round smoke texture: a gaussian falloff broken up by a few
/// lumps, white, so the material colour tints it.
pub(super) fn puff_image() -> Image {
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

/// [haze] How much of a billboard puff of `size` metres centred `d`
/// metres from the camera to draw: none when the camera is inside it, all
/// from about its own size away. A camera in the smog sees the smog round
/// it in the puffs further off; a puff right on the lens would lay its whole
/// colour over the view (the brown or white wash of a walk through smoke).
pub(super) fn near_fade(d: f32, size: f32) -> f32 {
    smoothstep(0.3 * size, 0.95 * size, d)
}

/// Prevailing wind direction (unit, sim plane): veers slowly over the days.
/// The sim's (`nd_sim::weather::wind_dir`), which ground contamination
/// drifts along too.
pub(super) fn wind_dir(tick: u64) -> SVec2 {
    nd_sim::weather::wind_dir(tick)
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
    cams: Query<&GlobalTransform, With<crate::camera::OrbitCamera>>,
    mut puffs: Query<(&Puff, &mut Transform, &mut MeshMaterial3d<StandardMaterial>, &mut Visibility, Option<&super::swap::SwapOriginal>)>,
) {
    let w = &sim.world;
    if state.quad.is_none() {
        state.quad = Some(meshes.add(Rectangle::new(1.0, 1.0)));
        let tex = images.add(puff_image());
        for (rgb, top) in [(PLUME_RGB, PLUME_ALPHA), (SMOG_RGB, SMOG_ALPHA), (STEAM_RGB, STEAM_ALPHA)] {
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
            let (rgb, top) = [(PLUME_RGB, PLUME_ALPHA), (SMOG_RGB, SMOG_ALPHA), (STEAM_RGB, STEAM_ALPHA)][(i / ALPHA_STEPS).min(2)];
            let a = top * ((i % ALPHA_STEPS) + 1) as f32 / ALPHA_STEPS as f32;
            // [plant-scale] At night the stack smoke picks up the furnace
            // and sodium glow from below: a dim rust-orange.
            let warm = if i < ALPHA_STEPS { (1.0 - light) * 0.07 } else { 0.0 };
            if let Some(mut m) = materials.get_mut(h) {
                m.base_color = Color::linear_rgba(rgb[0] * light + warm, rgb[1] * light * 0.97 + warm * 0.4, rgb[2] * light * 0.92 + warm * 0.1, a);
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
        // [plant-scale] The stacks come from the station's own layout (its
        // seed and size), so they follow a rebuild of the plant too.
        let scale = w.plant_scale(b);
        let key = ((lot.center.x.to_bits() as u64) << 32 | lot.center.y.to_bits() as u64 ^ pad.to_bits() as u64) ^ b.seed.rotate_left(17) ^ (scale.to_bits() as u64) << 7;
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
        let stacks: Vec<CoalVent> = coal_vents(lot, b.seed, scale).into_iter().map(|v| CoalVent { top: v.top + Vec3::Y * pad, ..v }).collect();
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
    let eye = cam.translation();
    let now = time.elapsed_secs();
    let dir = wind_dir(w.clock.tick);
    let wind = w.weather.wind.clamp(0.05, 1.0);
    let along = Vec3::new(dir.x, 0.0, dir.y);
    let side = Vec3::new(-dir.y, 0.0, dir.x);
    let reach = 70.0 + 190.0 * wind;
    for (puff, mut tf, mut mat, mut vis, orig) in &mut puffs {
        let Some(plant) = state.plants.get(&puff.plant) else { continue };
        let seed = puff.plant_seed();
        let (pos, size, alpha, smog) = match puff.stack {
            Some(s) => {
                let Some(vent) = plant.stacks.get(s) else { continue };
                let top = &vent.top;
                // A thick flue puts out a fatter plume; steam billows wider and
                // goes sooner.
                let girth = (vent.r / 1.6).clamp(0.6, 2.2);
                // Age 0..1, puffs evenly spaced in time, each stack offset.
                let age = (now / PLUME_LIFE + puff.k as f32 / PLUME_PUFFS as f32 + s as f32 * 0.37).fract();
                let rise = 50.0 * (1.0 - (1.0 - age).powi(2)) * (1.15 - 0.6 * wind);
                let wobble = (now * 0.21 + puff.k as f32 * 1.9).sin() * 9.0 * age;
                let pos = *top + Vec3::Y * rise + along * (age * reach) + side * wobble;
                let (size, alpha) = if vent.steam {
                    (vent.r * 1.6 + 40.0 * age.powf(0.7), smoothstep(0.0, 0.05, age) * (1.0 - age).powf(2.4))
                } else {
                    (9.0 * girth + 58.0 * girth.sqrt() * age.powf(0.75), smoothstep(0.0, 0.05, age) * (1.0 - age).powf(1.1))
                };
                (pos, size, alpha, if vent.steam { 2 } else { 0 })
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
                (pos, size, alpha, 1)
            }
        };
        let a = alpha * plant.intensity * near_fade(eye.distance(pos), size);
        if a < 0.5 / ALPHA_STEPS as f32 {
            vis.set_if_neq(Visibility::Hidden);
            continue;
        }
        vis.set_if_neq(Visibility::Inherited);
        let step = ((a * ALPHA_STEPS as f32).round() as usize).clamp(1, ALPHA_STEPS) - 1;
        let want = &state.materials[smog * ALPHA_STEPS + step];
        // [refactor/app] Against the original under heat or focus (no swap ping-pong).
        if orig.map_or(&mat.0, |o| &o.0) != want {
            mat.0 = want.clone();
        }
        tf.translation = pos;
        tf.rotation = face;
        // Smog lies flatter than it is tall from the street.
        tf.scale = if smog == 1 { Vec3::new(size, size * 0.45, 1.0) } else { Vec3::splat(size) };
    }
}

impl Puff {
    fn plant_seed(&self) -> u64 {
        use slotmap::Key;
        self.plant.data().as_ffi()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [haze] A puff the camera is inside is not drawn; one its own size
    /// away is drawn in full.
    #[test]
    fn puffs_fade_out_round_the_camera() {
        assert_eq!(near_fade(0.0, 50.0), 0.0);
        assert_eq!(near_fade(10.0, 50.0), 0.0);
        assert!(near_fade(30.0, 50.0) > 0.0 && near_fade(30.0, 50.0) < 1.0);
        assert_eq!(near_fade(60.0, 50.0), 1.0);
    }
}
