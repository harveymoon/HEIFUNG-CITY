//! [dirty-industry] What dirty industry looks like from the air, beyond its
//! buildings: smoke, fumes and dust over the dirty plants, and poisoned
//! ground stained in the terrain.
//!
//! - Smoke: soft camera-facing puffs like the coal smoke (`haze.rs`, whose
//!   texture and wind this shares), from the vents each style declares
//!   (`nd_procgen::dirty_vents`): thick soot off a smelter's stacks and a
//!   refinery's cracker, a thin yellow-green plume off a chemical works,
//!   tan dust over a strip mine and a smelter's ore heap, a low brown haze
//!   over a factory farm's pens. Thicker the harder the plant works; none
//!   from a shut one.
//! - Stain: the saved contamination (`World::contamination`) as a
//!   translucent, lit wash a little over the ground, sickly yellow-green
//!   where it is light and dead brown where it is bad; rebuilt when the
//!   contamination changes, at most every few seconds.

use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::prelude::*;
use nd_procgen::dirty::{dirty_vents, VentKind};
use nd_procgen::overlay::{ground_stain_color, HeatGrid};
use nd_sim::{BuildingId, Vec2 as SVec2};
use std::collections::HashMap;

use super::haze::{puff_image, wind_dir};
use super::{smoothstep, to_mesh, Lighting};
use crate::sim::Sim;

/// Alpha steps the puff materials come in.
const ALPHA_STEPS: usize = 10;
/// The most dirty plants that smoke at once.
const MAX_SOURCES: usize = 48;
/// Metres the stain floats over the graded ground.
const STAIN_Y: f32 = 0.28;
/// Side of the stain's grid, metres.
const STAIN_CELL: f32 = 12.0;
/// Seconds between stain rebuilds.
const STAIN_REBUILD_SECS: f32 = 4.0;

/// Look of each kind of vent: colour, most alpha, puffs per vent.
fn vent_look(k: VentKind) -> ([f32; 3], f32, usize) {
    match k {
        VentKind::Soot => ([0.17, 0.155, 0.14], 0.7, 14),
        VentKind::Fumes => ([0.66, 0.72, 0.26], 0.5, 10),
        VentKind::Dust => ([0.58, 0.49, 0.36], 0.32, 8),
        VentKind::Stench => ([0.36, 0.28, 0.15], 0.22, 6),
    }
}

const KINDS: [VentKind; 4] = [VentKind::Soot, VentKind::Fumes, VentKind::Dust, VentKind::Stench];

fn kind_index(k: VentKind) -> usize {
    KINDS.iter().position(|x| *x == k).unwrap_or(0)
}

#[derive(Component)]
pub(super) struct DirtyPuff {
    source: BuildingId,
    vent: usize,
    k: usize,
}

#[derive(Component)]
pub(super) struct StainTag;

struct Source {
    vents: Vec<(Vec3, VentKind)>,
    intensity: f32,
    entities: Vec<Entity>,
    key: u64,
}

#[derive(Default)]
pub(super) struct GrimeState {
    sources: HashMap<BuildingId, Source>,
    quad: Option<Handle<Mesh>>,
    /// `ALPHA_STEPS` materials per vent kind, in `KINDS` order.
    materials: Vec<Handle<StandardMaterial>>,
    light_key: Option<i32>,
    stain_material: Option<Handle<StandardMaterial>>,
    stain_version: Option<u64>,
    stain_built: f32,
}

fn hash01(a: u64, b: u64) -> f32 {
    let h = (a ^ b.wrapping_mul(0x9E37_79B9_7F4A_7C15)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    ((h >> 40) as f32) / (1u64 << 24) as f32
}

fn seed_of(id: BuildingId) -> u64 {
    use slotmap::Key;
    id.data().as_ffi()
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_grime(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    sim: Res<Sim>,
    lighting: Res<Lighting>,
    time: Res<Time>,
    mut state: Local<GrimeState>,
    cams: Query<&GlobalTransform, With<Camera3d>>,
    mut puffs: Query<(&DirtyPuff, &mut Transform, &mut MeshMaterial3d<StandardMaterial>, &mut Visibility)>,
    stains: Query<Entity, With<StainTag>>,
) {
    let w = &sim.world;
    let now = time.elapsed_secs();
    if state.quad.is_none() {
        state.quad = Some(meshes.add(Rectangle::new(1.0, 1.0)));
        let tex = images.add(puff_image());
        for k in KINDS {
            let (rgb, top, _) = vent_look(k);
            for s in 0..ALPHA_STEPS {
                let a = top * (s + 1) as f32 / ALPHA_STEPS as f32;
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
        state.stain_material = Some(materials.add(StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 1.0, reflectance: 0.05, alpha_mode: AlphaMode::Blend, ..default() }));
    }

    // ── the stain ──
    let version = w.contamination.version;
    let stale = state.stain_version != Some(version) || sim.full_rebuild;
    if stale && (now - state.stain_built >= STAIN_REBUILD_SECS || state.stain_version.is_none() || sim.full_rebuild) {
        for e in &stains {
            commands.entity(e).despawn();
        }
        let c = &w.contamination;
        if !c.cells.is_empty() {
            let (mut lo, mut hi) = (SVec2::splat(f32::MAX), SVec2::splat(f32::MIN));
            for &i in c.cells.keys() {
                let p = c.cell_center(i as usize % c.n, i as usize / c.n);
                lo = lo.min(p);
                hi = hi.max(p);
            }
            let pad = SVec2::splat(c.cell);
            let mut g = HeatGrid::covering(lo - pad, hi + pad, STAIN_CELL, 60_000);
            let n = g.cols + 1;
            for j in 0..=g.rows {
                for i in 0..=g.cols {
                    g.values[j * n + i] = c.at(g.corner(i, j));
                }
            }
            let flat = w.landscape.is_flat();
            let height = |p: SVec2| if flat { 0.0 } else { w.ground_height(p) };
            let m = g.mesh_by(STAIN_Y, |k| ground_stain_color(g.values[k]), |k| g.values[k] < 0.03, height);
            if !m.is_empty() {
                let mat = state.stain_material.clone().unwrap_or_default();
                commands.spawn((Mesh3d(meshes.add(to_mesh(&m, false))), MeshMaterial3d(mat), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, StainTag));
            }
        }
        state.stain_version = Some(version);
        state.stain_built = now;
    }

    // ── the smoke ──
    // Unlit, so shaded here like the coal smoke: by day, and a dim umber
    // at night.
    let t = if lighting.follow_clock { w.clock.day_fraction() } else { (lighting.hour / 24.0).rem_euclid(1.0) };
    let elev = ((t - 0.25) * std::f32::consts::TAU).sin();
    let light = 0.16 + 0.84 * smoothstep(-0.12, 0.3, elev);
    let light_key = (light * 64.0).round() as i32;
    if state.light_key != Some(light_key) {
        state.light_key = Some(light_key);
        for (i, h) in state.materials.iter().enumerate() {
            let (rgb, top, _) = vent_look(KINDS[i / ALPHA_STEPS]);
            let a = top * ((i % ALPHA_STEPS) + 1) as f32 / ALPHA_STEPS as f32;
            if let Some(mut m) = materials.get_mut(h) {
                m.base_color = Color::linear_rgba(rgb[0] * light, rgb[1] * light * 0.97, rgb[2] * light * 0.92, a);
            }
        }
    }
    let mut seen = Vec::new();
    for (id, b) in w.buildings.iter() {
        if seen.len() >= MAX_SOURCES {
            break;
        }
        let Some(z) = &b.business else { continue };
        if b.construction.is_some() || b.is_derelict() || z.closed || z.kind.dirtiness() < nd_sim::industry::DIRTY {
            continue;
        }
        let Some(lot) = w.lots.get(b.lot) else { continue };
        let staffed = if b.jobs > 0 { (b.workers.len() as f32 / b.jobs as f32).clamp(0.0, 1.0) } else { 0.0 };
        let intensity = 0.35 + 0.65 * staffed;
        let pad = w.lot_pad(b.lot);
        let key = ((lot.center.x.to_bits() as u64) << 32 | lot.center.y.to_bits() as u64) ^ pad.to_bits() as u64 ^ (z.kind as u64).wrapping_mul(0x9E37);
        if let Some(s) = state.sources.get_mut(&id) {
            s.intensity = intensity;
            if s.key == key {
                seen.push(id);
                continue;
            }
        }
        let vents: Vec<(Vec3, VentKind)> = dirty_vents(lot, z.kind).into_iter().map(|v| (v.pos + Vec3::Y * pad, v.kind)).collect();
        if vents.is_empty() {
            continue;
        }
        seen.push(id);
        if let Some(old) = state.sources.remove(&id) {
            for e in old.entities {
                commands.entity(e).despawn();
            }
        }
        let quad = state.quad.clone().unwrap_or_default();
        let mat = state.materials[0].clone();
        let mut entities = Vec::new();
        for (vi, (_, kind)) in vents.iter().enumerate() {
            for k in 0..vent_look(*kind).2 {
                entities.push(commands.spawn((Mesh3d(quad.clone()), MeshMaterial3d(mat.clone()), Transform::from_scale(Vec3::ZERO), Visibility::Hidden, NotShadowCaster, NotShadowReceiver, DirtyPuff { source: id, vent: vi, k })).id());
            }
        }
        state.sources.insert(id, Source { vents, intensity, entities, key });
    }
    let gone: Vec<BuildingId> = state.sources.keys().copied().filter(|id| !seen.contains(id)).collect();
    for id in gone {
        if let Some(old) = state.sources.remove(&id) {
            for e in old.entities {
                commands.entity(e).despawn();
            }
        }
    }
    if state.sources.is_empty() {
        return;
    }
    let Some(cam) = cams.iter().next() else { return };
    let face = cam.compute_transform().rotation;
    let dir = wind_dir(w.clock.tick);
    let wind = w.weather.wind.clamp(0.05, 1.0);
    let along = Vec3::new(dir.x, 0.0, dir.y);
    let side = Vec3::new(-dir.y, 0.0, dir.x);
    for (puff, mut tf, mut mat, mut vis) in &mut puffs {
        let Some(src) = state.sources.get(&puff.source) else { continue };
        let Some(&(top, kind)) = src.vents.get(puff.vent) else { continue };
        let (_, _, count) = vent_look(kind);
        let seed = seed_of(puff.source) ^ (puff.vent as u64) << 20;
        let r1 = hash01(seed, puff.k as u64 * 2 + 1);
        let r2 = hash01(seed, puff.k as u64 * 2 + 2);
        let (pos, size, alpha, flat) = match kind {
            VentKind::Soot | VentKind::Fumes => {
                let life = if kind == VentKind::Soot { 30.0 } else { 24.0 };
                let age = (now / life + puff.k as f32 / count as f32 + puff.vent as f32 * 0.29).fract();
                let rise = (if kind == VentKind::Soot { 42.0 } else { 30.0 }) * (1.0 - (1.0 - age).powi(2)) * (1.15 - 0.6 * wind);
                let reach = 50.0 + 160.0 * wind;
                let wobble = (now * 0.23 + puff.k as f32 * 1.7).sin() * 7.0 * age;
                let pos = top + Vec3::Y * rise + along * (age * reach) + side * wobble;
                let size = if kind == VentKind::Soot { 5.0 + 38.0 * age.powf(0.8) } else { 3.0 + 22.0 * age.powf(0.8) };
                let alpha = smoothstep(0.0, 0.06, age) * (1.0 - age).powf(1.3);
                (pos, size, alpha, false)
            }
            VentKind::Dust => {
                // Drifting low clouds round the pit, rolling downwind.
                let age = (now / 40.0 + puff.k as f32 / count as f32 + r1 * 0.3).fract();
                let lateral = (r2 - 0.5) * 50.0;
                let pos = top + Vec3::Y * (2.0 + 10.0 * age) + along * (age * (30.0 + 80.0 * wind)) + side * lateral;
                let size = 12.0 + 22.0 * age;
                let alpha = smoothstep(0.0, 0.15, age) * (1.0 - age);
                (pos, size, alpha, true)
            }
            VentKind::Stench => {
                let d = 8.0 + (puff.k as f32 + r1) / count as f32 * 50.0;
                let drift = (now * 0.05 + puff.k as f32).sin() * 6.0;
                let pos = top + along * (d + drift) + side * ((r2 - 0.5) * 40.0) + Vec3::Y * (2.0 + 4.0 * r1);
                let size = 22.0 + 16.0 * r2;
                let alpha = 0.6 + 0.4 * (now * 0.07 + puff.k as f32 * 2.3).sin().abs();
                (pos, size, alpha, true)
            }
        };
        let a = alpha * src.intensity;
        if a < 0.5 / ALPHA_STEPS as f32 {
            *vis = Visibility::Hidden;
            continue;
        }
        *vis = Visibility::Inherited;
        let step = ((a * ALPHA_STEPS as f32).round() as usize).clamp(1, ALPHA_STEPS) - 1;
        let want = &state.materials[kind_index(kind) * ALPHA_STEPS + step];
        if mat.0 != *want {
            mat.0 = want.clone();
        }
        tf.translation = pos;
        tf.rotation = face;
        tf.scale = if flat { Vec3::new(size, size * 0.45, 1.0) } else { Vec3::splat(size) };
    }
}
