//! [look] Grounded neon: the camera's realism effects, the graphics quality
//! tiers and the throttler that moves between them to hold the frame rate.
//!
//! - **Effects.** Screen-space ambient occlusion (Bevy's, with a radius
//!   that grows with distance so it reads at a city-builder's camera
//!   distance, `ssao_city.wgsl`), temporal anti-aliasing (lamp posts and
//!   lane markings stop shimmering), volumetric fog in a low layer of haze
//!   round the camera (light shafts between towers at a low sun, a moonlit
//!   murk at night), and reflections of the sky: the dome's colours are
//!   painted into small prefiltered cubemaps for an environment light, so
//!   glass, metal and car paint reflect the sky and the city's glow in it,
//!   and shadowed sides take the sky's colour, instead of a flat ambient.
//! - **Tiers.** [`Quality`] Low, Medium, High pick the effects; the
//!   [`QualitySetting`] (Lighting's `quality`: Auto or a fixed tier) is the
//!   player's. Auto starts at High and lets [`throttle`] step down when the
//!   frame time stays over budget and back up when it has stayed under,
//!   with hysteresis and a back-off so it does not flicker between two.
//! - **One place.** [`apply_camera_effects`] derives the whole set of
//!   camera components from the tier and the view (heat vision adds
//!   motion blur and drops the fog) every frame and only touches the
//!   camera when that set changes. Nothing is stored to be put back, so a
//!   mode switched on and off cannot leave an effect behind.

use bevy::anti_alias::taa::TemporalAntiAliasing;
use bevy::asset::RenderAssetUsages;
use bevy::core_pipeline::prepass::{DepthPrepass, MotionVectorPrepass, NormalPrepass};
use bevy::light::{DirectionalLightShadowMap, EnvironmentMapLight, FogVolume, VolumetricFog, VolumetricLight};
use bevy::pbr::{ScreenSpaceAmbientOcclusion, ScreenSpaceAmbientOcclusionQualityLevel};
use bevy::post_process::bloom::Bloom;
use bevy::post_process::motion_blur::MotionBlur;
use bevy::prelude::*;
use bevy::render::camera::{MipBias, TemporalJitter};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension};
use bevy::render::view::Msaa;

use super::{sky, Lighting, RenderStats, SunTag};
use crate::camera::OrbitCamera;
use crate::sim::Sim;

/// A graphics quality tier.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default, Hash)]
pub enum Quality {
    /// Multisampling, no ambient occlusion or volumetric fog, small shadows.
    Low,
    /// Temporal anti-aliasing and light ambient occlusion.
    Medium,
    /// Everything: full ambient occlusion, volumetric fog, big shadows.
    #[default]
    High,
}

impl Quality {
    pub fn label(self) -> &'static str {
        match self {
            Quality::Low => "Low",
            Quality::Medium => "Medium",
            Quality::High => "High",
        }
    }

    fn down(self) -> Option<Quality> {
        match self {
            Quality::Low => None,
            Quality::Medium => Some(Quality::Low),
            Quality::High => Some(Quality::Medium),
        }
    }

    fn up(self) -> Option<Quality> {
        match self {
            Quality::Low => Some(Quality::Medium),
            Quality::Medium => Some(Quality::High),
            Quality::High => None,
        }
    }
}

/// The player's graphics setting: a tier, or Auto (the throttler's pick).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
pub enum QualitySetting {
    #[default]
    Auto,
    Fixed(Quality),
}

impl QualitySetting {
    pub const ALL: [QualitySetting; 4] = [QualitySetting::Auto, QualitySetting::Fixed(Quality::Low), QualitySetting::Fixed(Quality::Medium), QualitySetting::Fixed(Quality::High)];

    pub fn label(self) -> &'static str {
        match self {
            QualitySetting::Auto => "Auto",
            QualitySetting::Fixed(q) => q.label(),
        }
    }

    /// `auto`, `low`, `medium`, `high` (any case).
    pub fn parse(s: &str) -> Option<QualitySetting> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(QualitySetting::Auto),
            "low" => Some(QualitySetting::Fixed(Quality::Low)),
            "medium" | "med" => Some(QualitySetting::Fixed(Quality::Medium)),
            "high" => Some(QualitySetting::Fixed(Quality::High)),
            _ => None,
        }
    }

    /// The starting setting: `ND_QUALITY` if set to one of the above, else Auto.
    pub fn from_env() -> QualitySetting {
        std::env::var("ND_QUALITY").ok().and_then(|v| QualitySetting::parse(&v)).unwrap_or_default()
    }
}

/// What the renderer is running at, for the Dev window and the API.
#[derive(Clone, Debug, Default)]
pub struct Graphics {
    /// The tier in force (the setting's, or Auto's current pick).
    pub tier: Quality,
    /// Frame time, smoothed over about a second, in milliseconds.
    pub frame_ms: f32,
    /// Of which the sim's ticks, smoothed the same way.
    pub sim_ms: f32,
    /// Tier changes Auto has made this run.
    pub steps: u32,
    /// Why Auto last moved (or holds), in a few words.
    pub reason: &'static str,
    /// The effects on the camera now, e.g. "TAA · SSAO high · vol. fog".
    pub effects: String,
}

/// Frame time Auto aims at: 60 frames a second.
pub const TARGET_MS: f32 = 1000.0 / 60.0;
/// Step down when the smoothed frame time stays above this (about 53 fps)
/// for [`DOWN_AFTER`] seconds.
const DOWN_MS: f32 = 18.8;
const DOWN_AFTER: f32 = 2.0;
/// Step up when it stays below this (about 58 fps; with vsync on a 60 Hz
/// screen a frame never comes in under 16.7 ms) for the current wait.
const UP_MS: f32 = 17.2;
/// The first wait before trying a tier up; doubled every time a step up
/// has to be taken back, up to [`UP_WAIT_MAX`], and reset after a step up
/// holds for [`UP_HELD`] seconds.
const UP_WAIT: f32 = 8.0;
const UP_WAIT_MAX: f32 = 240.0;
const UP_HELD: f32 = 30.0;
/// After any change, measurements are ignored this long: pipelines for the
/// new effects compile and the first frames are slow for that reason only.
const SETTLE: f32 = 3.0;

/// The throttler's state.
#[derive(Resource)]
pub(super) struct Throttle {
    tier: Quality,
    ema_ms: f32,
    sim_ms: f32,
    over: f32,
    under: f32,
    settle: f32,
    up_wait: f32,
    /// Seconds since the last step up, while it may still be taken back.
    since_up: Option<f32>,
    steps: u32,
    reason: &'static str,
}

impl Default for Throttle {
    fn default() -> Self {
        Self { tier: Quality::High, ema_ms: TARGET_MS, sim_ms: 0.0, over: 0.0, under: 0.0, settle: SETTLE, up_wait: UP_WAIT, since_up: None, steps: 0, reason: "starting" }
    }
}

impl Throttle {
    /// One frame of `dt` seconds, `sim_ms` of it spent ticking the sim.
    /// Returns the tier to draw at.
    fn step(&mut self, setting: QualitySetting, dt: f32, sim_ms: f32) -> Quality {
        let ms = dt * 1000.0;
        // Loads, rebuilds and window drags: not the renderer's steady cost.
        if ms > 250.0 {
            self.over = 0.0;
            self.under = 0.0;
        } else {
            let k = (dt / 0.8).min(1.0);
            self.ema_ms += (ms - self.ema_ms) * k;
            self.sim_ms += (sim_ms - self.sim_ms) * k;
        }
        if let QualitySetting::Fixed(q) = setting {
            if self.tier != q {
                self.tier = q;
                self.reset("fixed by the setting");
            }
            self.reason = "fixed by the setting";
            return q;
        }
        if ms > 250.0 {
            return self.tier;
        }
        if self.settle > 0.0 {
            self.settle -= dt;
            return self.tier;
        }
        if let Some(t) = self.since_up.as_mut() {
            *t += dt;
            if *t > UP_HELD {
                self.since_up = None;
                self.up_wait = UP_WAIT;
            }
        }
        if self.ema_ms > DOWN_MS {
            self.over += dt;
            self.under = 0.0;
        } else if self.ema_ms < UP_MS {
            self.under += dt;
            self.over = 0.0;
        } else {
            self.over = 0.0;
            self.under = 0.0;
        }
        // A frame the sim fills is not the renderer's fault: cutting
        // effects would not buy it back.
        let sim_bound = self.sim_ms > 0.5 * self.ema_ms;
        if self.over > DOWN_AFTER {
            if sim_bound {
                self.reason = "holding: the sim fills the frame";
                self.over = 0.0;
            } else if let Some(q) = self.tier.down() {
                // A step up that could not hold: wait longer next time.
                if self.since_up.take().is_some() {
                    self.up_wait = (self.up_wait * 2.0).min(UP_WAIT_MAX);
                }
                self.tier = q;
                self.steps += 1;
                self.reset("stepped down: over budget");
            } else {
                self.reason = "lowest tier, over budget";
                self.over = 0.0;
            }
        } else if self.under > self.up_wait {
            if let Some(q) = self.tier.up() {
                self.tier = q;
                self.steps += 1;
                self.since_up = Some(0.0);
                self.reset("stepped up: under budget");
            } else {
                self.reason = "highest tier";
                self.under = 0.0;
            }
        }
        self.tier
    }

    fn reset(&mut self, reason: &'static str) {
        self.over = 0.0;
        self.under = 0.0;
        self.settle = SETTLE;
        self.reason = reason;
    }
}

/// Effects switched off for profiling: `ND_LOOK_OFF=env,haze,ssao,taa,volumetric`.
fn off(name: &str) -> bool {
    use std::sync::OnceLock;
    static OFF: OnceLock<Vec<String>> = OnceLock::new();
    OFF.get_or_init(|| std::env::var("ND_LOOK_OFF").unwrap_or_default().split(',').map(|s| s.trim().to_ascii_lowercase()).collect()).iter().any(|s| s == name)
}

/// Anti-aliasing: temporal (needs no multisampling), or 4x multisampling.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Aa {
    Taa,
    Msaa4,
}

/// Everything the tier and the view put on the camera.
#[derive(Clone, Copy, PartialEq, Debug)]
struct Fx {
    aa: Aa,
    ssao: Option<ScreenSpaceAmbientOcclusionQualityLevel>,
    /// Volumetric fog ray-march steps.
    volumetric: Option<u32>,
    motion_blur: bool,
    shadow_map: usize,
    bloom_mip: u32,
}

impl Fx {
    fn of(tier: Quality, heat: bool) -> Fx {
        let mut fx = match tier {
            Quality::High => Fx { aa: Aa::Taa, ssao: Some(ScreenSpaceAmbientOcclusionQualityLevel::High), volumetric: Some(40), motion_blur: false, shadow_map: 4096, bloom_mip: 512 },
            Quality::Medium => Fx { aa: Aa::Taa, ssao: Some(ScreenSpaceAmbientOcclusionQualityLevel::Low), volumetric: None, motion_blur: false, shadow_map: 2048, bloom_mip: 512 },
            Quality::Low => Fx { aa: Aa::Msaa4, ssao: None, volumetric: None, motion_blur: false, shadow_map: 1024, bloom_mip: 256 },
        };
        if heat {
            // The drone's sensor smears what moves; its sky is black.
            fx.motion_blur = true;
            fx.volumetric = None;
        }
        if off("ssao") {
            fx.ssao = None;
        }
        if off("volumetric") {
            fx.volumetric = None;
        }
        if off("taa") && fx.aa == Aa::Taa {
            fx.aa = Aa::Msaa4;
        }
        fx
    }

    fn describe(&self) -> String {
        let mut parts = vec![match self.aa {
            Aa::Taa => "TAA".to_string(),
            Aa::Msaa4 => "MSAA 4x".to_string(),
        }];
        if let Some(q) = self.ssao {
            parts.push(format!("SSAO {}", format!("{q:?}").to_lowercase()));
        }
        if let Some(s) = self.volumetric {
            parts.push(format!("vol. fog {s}"));
        }
        if self.motion_blur {
            parts.push("motion blur".into());
        }
        parts.push(format!("shadows {}", self.shadow_map));
        parts.join(" · ")
    }
}

pub(super) struct LookPlugin;

impl Plugin for LookPlugin {
    fn build(&self, app: &mut App) {
        // The SSAO shader with a city-scale radius in place of Bevy's own
        // (registered under the same embedded path, so Bevy's pipeline
        // loads this one). The default plugins are added before ours.
        if let Some(reg) = app.world().get_resource::<bevy::asset::io::embedded::EmbeddedAssetRegistry>() {
            reg.insert_asset(std::path::PathBuf::new(), std::path::Path::new("bevy_pbr/ssao/ssao.wgsl"), include_str!("ssao_city.wgsl").as_bytes());
        }
        app.init_resource::<Throttle>()
            .init_resource::<SkyCube>()
            .init_resource::<AppliedFx>()
            .add_systems(Update, throttle.before(crate::sim::SimSet::Sync))
            .add_systems(PostUpdate, (apply_camera_effects, sync_sky_reflections, sync_fog_volume).chain().after(super::heat::apply_heat));
    }
}

/// The effects last put on the camera.
#[derive(Resource, Default)]
struct AppliedFx(Option<Fx>);

/// Measure the frame and pick the tier (see [`Throttle`]).
fn throttle(time: Res<Time<Real>>, sim: Res<Sim>, lighting: Res<Lighting>, mut state: ResMut<Throttle>, mut stats: ResMut<RenderStats>) {
    let dt = time.delta_secs();
    if dt <= 0.0 {
        return;
    }
    if sim.full_rebuild {
        // A city just loaded: the next frames rebuild every mesh.
        state.settle = SETTLE;
    }
    let tier = state.step(lighting.quality, dt, sim.tick_ms);
    let g = &mut stats.graphics;
    g.tier = tier;
    g.frame_ms = state.ema_ms;
    g.sim_ms = state.sim_ms;
    g.steps = state.steps;
    g.reason = state.reason;
}

/// Put the tier's and the view's effects on the camera; reset TAA's
/// history when the camera jumps (a remote pose, a follow starting), so a
/// cut does not smear the old view into the new.
#[allow(clippy::type_complexity)]
fn apply_camera_effects(
    mut commands: Commands,
    lighting: Res<Lighting>,
    mut stats: ResMut<RenderStats>,
    mut shadow_map: ResMut<DirectionalLightShadowMap>,
    mut cams: Query<(Entity, &mut Msaa, &mut Bloom, Option<&mut TemporalAntiAliasing>, &GlobalTransform), With<OrbitCamera>>,
    sun: Query<(Entity, Has<VolumetricLight>), With<SunTag>>,
    mut applied: ResMut<AppliedFx>,
    mut last_pose: Local<Option<(Vec3, Vec3)>>,
) {
    let Ok((cam, mut msaa, mut bloom, taa, tf)) = cams.single_mut() else { return };
    let fx = Fx::of(stats.graphics.tier, lighting.heat);
    // A cut: more than 40 m or about 10 degrees in one frame.
    let pose = (tf.translation(), tf.forward().as_vec3());
    let cut = last_pose.is_some_and(|(p, f)| p.distance(pose.0) > 40.0 || f.dot(pose.1) < 0.985);
    *last_pose = Some(pose);
    if let Some(mut taa) = taa {
        if cut {
            taa.reset = true;
        }
    }
    for (e, has) in &sun {
        if !has {
            commands.entity(e).insert(VolumetricLight);
        }
    }
    if applied.0 == Some(fx) {
        return;
    }
    applied.0 = Some(fx);
    stats.graphics.effects = fx.describe();
    let mut ec = commands.entity(cam);
    // Off with everything this system manages, then on with the wanted set
    // (TAA's required jitter and mip bias come off with it explicitly:
    // required components stay when the component that pulled them in goes).
    ec.remove::<(TemporalAntiAliasing, TemporalJitter, MipBias, ScreenSpaceAmbientOcclusion, VolumetricFog, MotionBlur, DepthPrepass, NormalPrepass, MotionVectorPrepass)>();
    let depth = fx.aa == Aa::Taa || fx.ssao.is_some() || fx.motion_blur || fx.volumetric.is_some();
    if depth {
        ec.insert(DepthPrepass);
    }
    if fx.aa == Aa::Taa || fx.motion_blur {
        ec.insert(MotionVectorPrepass);
    }
    if fx.aa == Aa::Taa {
        ec.insert(TemporalAntiAliasing::default());
    }
    if let Some(q) = fx.ssao {
        // Thickness to match the radius's city scale (`ssao_city.wgsl`).
        ec.insert((NormalPrepass, ScreenSpaceAmbientOcclusion { quality_level: q, constant_object_thickness: 1.0 }));
    }
    if let Some(steps) = fx.volumetric {
        // Ambient light for the haze: set from the sky by `sync_fog_volume`.
        ec.insert(VolumetricFog {
            ambient_color: Color::BLACK,
            ambient_intensity: SKY_NITS,
            // Ray starts dithered per pixel; TAA blends the steps away.
            jitter: if fx.aa == Aa::Taa { 8.0 } else { 0.0 },
            step_count: steps,
        });
    }
    if fx.motion_blur {
        ec.insert(MotionBlur { shutter_angle: 1.25, samples: 3 });
    }
    *msaa = if fx.aa == Aa::Msaa4 { Msaa::Sample4 } else { Msaa::Off };
    if shadow_map.size != fx.shadow_map {
        shadow_map.size = fx.shadow_map;
    }
    bloom.max_mip_dimension = fx.bloom_mip;
}

// ── reflections of the sky ──────────────────────────────────────────────────

/// Side of each face of the sharpest level of the reflection cubemap; each
/// further level halves it, down to 1 (six levels), and stands for a
/// rougher surface (Bevy samples level `roughness * 5`).
const SPECULAR: u32 = 32;
/// Side of the faces of the diffuse (irradiance) cubemap.
const DIFFUSE: u32 = 8;
/// Brightness of the reflections against the dome: the dome is unlit (its
/// colours go to the screen as they are), lit surfaces are scaled by the
/// camera's exposure (EV100 9.7 by default, about 1/1000), so 1000 makes a
/// mirror show the dome as bright as the dome itself.
const SKY_NITS: f32 = 1000.0;
/// The reflections are kept below the dome's brightness: at a grazing
/// angle every surface turns mirror (Fresnel), and full-strength
/// reflections of the glowing horizon washed the colour out of paint and
/// asphalt alike. The sky's diffuse light stays at full strength.
const SPECULAR_SCALE: f32 = 0.18;
/// At night the environment light is dimmed further: the dome's haze is
/// painted brighter than the light it really gives, so the streets stay
/// dark between the neon.
const NIGHT_ENV: f32 = 0.55;
/// Seconds between repaints at most: the sky changes slowly, and a repaint
/// is a couple of milliseconds on the main thread.
const REPAINT_EVERY: f32 = 0.5;

#[derive(Resource, Default)]
pub(super) struct SkyCube {
    maps: Option<(Handle<Image>, Handle<Image>)>,
    painted: Option<u64>,
    /// Seconds since the maps were last repainted.
    age: f32,
}

/// Linear float to half float bits (no NaN or denormal handling: sky
/// colours are small and positive).
fn half(x: f32) -> [u8; 2] {
    let b = x.max(0.0).to_bits();
    let exp = ((b >> 23) & 0xff) as i32 - 127 + 15;
    let h: u16 = if exp <= 0 {
        0
    } else if exp >= 31 {
        0x7bff
    } else {
        ((exp as u16) << 10) | ((b & 0x7f_ffff) >> 13) as u16
    };
    h.to_le_bytes()
}

/// The direction through texel `(x, y)` of cube face `face` (+X, -X, +Y,
/// -Y, +Z, -Z, as wgpu lays out a cube texture).
fn cube_dir(face: u32, x: u32, y: u32, n: u32) -> Vec3 {
    let u = 2.0 * (x as f32 + 0.5) / n as f32 - 1.0;
    let v = 2.0 * (y as f32 + 0.5) / n as f32 - 1.0;
    let d = match face {
        0 => Vec3::new(1.0, -v, -u),
        1 => Vec3::new(-1.0, -v, u),
        2 => Vec3::new(u, 1.0, v),
        3 => Vec3::new(u, -1.0, -v),
        4 => Vec3::new(u, -v, 1.0),
        _ => Vec3::new(-u, -v, -1.0),
    };
    d.normalize()
}

/// What a reflection sees in direction `d`: the dome above the horizon;
/// below it the city, darker than the haze it fades into.
fn env_radiance(look: &sky::SkyLook, d: Vec3) -> [f32; 3] {
    if d.y < 0.0 {
        let k = 0.35 + 0.65 * (1.0 + d.y * 4.0).max(0.0);
        let h = look.horizon;
        [h[0] * k, h[1] * k, h[2] * k]
    } else {
        sky::sky_color(look, d)
    }
}

/// The sky blurred over a cone of half-angle `spread` (radians) round `d`:
/// the centre and two rings, weighted towards the middle as a glossy lobe
/// is.
fn env_blurred(look: &sky::SkyLook, d: Vec3, spread: f32) -> [f32; 3] {
    if spread < 0.01 {
        return env_radiance(look, d);
    }
    let t = d.any_orthonormal_vector();
    let b = d.cross(t);
    let mut sum = env_radiance(look, d);
    let mut w = 1.0;
    for (angle, count, weight) in [(spread * 0.5, 6, 0.8), (spread, 10, 0.45)] {
        let (s, c) = angle.sin_cos();
        for i in 0..count {
            let a = i as f32 / count as f32 * std::f32::consts::TAU + angle;
            let dir = (d * c + (t * a.cos() + b * a.sin()) * s).normalize();
            let r = env_radiance(look, dir);
            for k in 0..3 {
                sum[k] += r[k] * weight;
            }
            w += weight;
        }
    }
    [sum[0] / w, sum[1] / w, sum[2] / w]
}

fn push_rgba(out: &mut Vec<u8>, c: [f32; 3]) {
    for ch in [c[0], c[1], c[2], 1.0] {
        out.extend_from_slice(&half(ch));
    }
}

/// The reflection cubemap with its levels, face by face (each face with
/// all its levels, Bevy's default layout): level `i` blurred as a surface
/// of perceptual roughness `i / 5` would see it.
fn specular_bytes(look: &sky::SkyLook) -> Vec<u8> {
    let levels = SPECULAR.trailing_zeros() + 1;
    let mut out = Vec::new();
    for face in 0..6 {
        for level in 0..levels {
            let n = SPECULAR >> level;
            let roughness = level as f32 / (levels - 1) as f32;
            // GGX's lobe widens with the square of the roughness.
            let spread = (roughness * roughness * 1.4).min(1.2);
            for y in 0..n {
                for x in 0..n {
                    let c = env_blurred(look, cube_dir(face, x, y, n), spread);
                    push_rgba(&mut out, [c[0] * SPECULAR_SCALE, c[1] * SPECULAR_SCALE, c[2] * SPECULAR_SCALE]);
                }
            }
        }
    }
    out
}

/// The diffuse cubemap: for each normal, the cosine-weighted average of
/// the sky round it (what a matte surface facing that way is lit by).
fn diffuse_bytes(look: &sky::SkyLook) -> Vec<u8> {
    // The sky sampled once in 128 directions spread over the sphere.
    const N: usize = 128;
    let golden = std::f32::consts::PI * (3.0 - 5f32.sqrt());
    let samples: Vec<(Vec3, [f32; 3])> = (0..N)
        .map(|i| {
            let y = 1.0 - 2.0 * (i as f32 + 0.5) / N as f32;
            let r = (1.0 - y * y).sqrt();
            let a = golden * i as f32;
            let d = Vec3::new(a.cos() * r, y, a.sin() * r);
            (d, env_radiance(look, d))
        })
        .collect();
    let mut out = Vec::new();
    for face in 0..6 {
        for y in 0..DIFFUSE {
            for x in 0..DIFFUSE {
                let n = cube_dir(face, x, y, DIFFUSE);
                let (mut sum, mut w) = ([0.0f32; 3], 0.0);
                for (d, c) in &samples {
                    let k = n.dot(*d);
                    if k > 0.0 {
                        for i in 0..3 {
                            sum[i] += c[i] * k;
                        }
                        w += k;
                    }
                }
                push_rgba(&mut out, [sum[0] / w, sum[1] / w, sum[2] / w]);
            }
        }
    }
    out
}

fn cube_image(size: u32, levels: u32, data: Vec<u8>) -> Image {
    let mut img = Image::new(Extent3d { width: size, height: size, depth_or_array_layers: 6 }, TextureDimension::D2, Vec::new(), TextureFormat::Rgba16Float, RenderAssetUsages::RENDER_WORLD);
    img.data = Some(data);
    img.texture_descriptor.mip_level_count = levels;
    img.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    img
}

/// Keep the sky's reflection and diffuse cubemaps painted like the dome
/// (prefiltered here on the CPU, a couple of milliseconds at most twice a
/// second: Bevy's realtime filter of a generated map cost tens of
/// milliseconds a frame) and the camera's environment light on them. Heat
/// vision and the focus views draw in flat stand-ins, which take no
/// reflections anyway; the light is dimmed out under heat to be sure.
fn sync_sky_reflections(
    mut commands: Commands,
    time: Res<Time<Real>>,
    look: Res<sky::SkyLook>,
    lighting: Res<Lighting>,
    mut cube: ResMut<SkyCube>,
    mut images: ResMut<Assets<Image>>,
    mut cams: Query<(Entity, Option<&mut EnvironmentMapLight>), With<OrbitCamera>>,
) {
    let Ok((cam, env)) = cams.single_mut() else { return };
    if off("env") {
        return;
    }
    cube.age += time.delta_secs();
    let (specular, diffuse) = match &cube.maps {
        Some(m) => m.clone(),
        None => {
            let s = images.add(cube_image(SPECULAR, SPECULAR.trailing_zeros() + 1, specular_bytes(&look)));
            let d = images.add(cube_image(DIFFUSE, 1, diffuse_bytes(&look)));
            cube.maps = Some((s.clone(), d.clone()));
            cube.painted = Some(look.version);
            cube.age = 0.0;
            (s, d)
        }
    };
    if cube.painted != Some(look.version) && cube.age > REPAINT_EVERY {
        if let Some(mut img) = images.get_mut(&specular) {
            img.data = Some(specular_bytes(&look));
        }
        if let Some(mut img) = images.get_mut(&diffuse) {
            img.data = Some(diffuse_bytes(&look));
        }
        cube.painted = Some(look.version);
        cube.age = 0.0;
    }
    let intensity = if lighting.heat { 0.0 } else { SKY_NITS * (NIGHT_ENV + (1.0 - NIGHT_ENV) * look.daylight) };
    match env {
        Some(mut e) => {
            if (e.intensity - intensity).abs() > 0.5 {
                e.intensity = intensity;
            }
        }
        None => {
            commands.entity(cam).insert(EnvironmentMapLight { diffuse_map: diffuse, specular_map: specular, intensity, ..default() });
        }
    }
}

// ── volumetric haze ─────────────────────────────────────────────────────────

/// The haze: a slab of fog lying on the city round the camera, as wide as
/// the view usually reaches and as high as the towers. It exists only while
/// the camera has volumetric fog (High): Bevy keeps a camera's volumetric
/// fog in the render world after the component is removed, so taking the
/// fog off means taking away the volume it would draw.
#[derive(Component)]
struct HazeVolume;

const HAZE_WIDTH: f32 = 2400.0;
const HAZE_HEIGHT: f32 = 140.0;

/// Keep the haze round the camera, its density and colour following the
/// hour: thin by day (light shafts between the towers at a low sun),
/// thicker at night, glowing with the city's light from below. Its own
/// light is the horizon's colour, so where it is thick it fades into the
/// same haze as the distance fog and the dome.
fn sync_fog_volume(
    mut commands: Commands,
    look: Res<sky::SkyLook>,
    applied: Res<AppliedFx>,
    mut cams: Query<(&OrbitCamera, Option<&mut VolumetricFog>)>,
    mut haze: Query<(Entity, &mut Transform, &mut FogVolume), With<HazeVolume>>,
) {
    let Ok((cam, vol)) = cams.single_mut() else { return };
    let wanted = applied.0.is_some_and(|fx| fx.volumetric.is_some()) && !off("haze");
    if !wanted {
        for (e, ..) in &haze {
            commands.entity(e).despawn();
        }
        return;
    }
    let Ok((_, mut tf, mut fog)) = haze.single_mut() else {
        commands.spawn((FogVolume::default(), Transform::from_scale(Vec3::new(HAZE_WIDTH, HAZE_HEIGHT, HAZE_WIDTH)), HazeVolume));
        return;
    };
    let centre = Vec3::new(cam.target.x, HAZE_HEIGHT * 0.5 - 4.0, cam.target.z);
    if tf.translation.distance(centre) > 1.0 {
        tf.translation = centre;
    }
    let day = look.daylight;
    let night = 1.0 - day;
    let density = 0.0005 * day + 0.0011 * night;
    // A touch of the light-pollution band's tint in the fog itself at night.
    let g = look.glow;
    let lum = (g[0] + g[1] + g[2]).max(1e-4) / 3.0;
    let tint = Color::linear_rgb(1.0 * day + (g[0] / lum).min(2.0) * night, 1.0 * day + (g[1] / lum).min(2.0) * night, 1.0 * day + (g[2] / lum).min(2.0) * night);
    if (fog.density_factor - density).abs() > 1e-6 || fog.fog_color != tint {
        fog.density_factor = density;
        fog.fog_color = tint;
        fog.absorption = 0.08;
        fog.scattering = 0.3;
        fog.scattering_asymmetry = 0.65;
    }
    if let Some(mut v) = vol {
        let h = look.horizon;
        let ambient = Color::linear_rgb(h[0], h[1], h[2]);
        if v.ambient_color != ambient {
            v.ambient_color = ambient;
            // Scaled like the sky's reflections (`SKY_NITS`): lit by the
            // horizon's colour, the haze converges on it.
            v.ambient_intensity = SKY_NITS;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(t: &mut Throttle, setting: QualitySetting, ms: f32, secs: f32) -> Quality {
        let mut q = t.tier;
        let n = (secs * 1000.0 / ms) as usize;
        for _ in 0..n {
            q = t.step(setting, ms / 1000.0, 0.0);
        }
        q
    }

    #[test]
    fn steps_down_when_slow_and_holds() {
        let mut t = Throttle::default();
        assert_eq!(run(&mut t, QualitySetting::Auto, 16.7, 10.0), Quality::High);
        // 40 fps: down a tier after the settle and the wait, then another.
        assert_eq!(run(&mut t, QualitySetting::Auto, 25.0, 5.6), Quality::Medium);
        assert_eq!(run(&mut t, QualitySetting::Auto, 25.0, 6.0), Quality::Low);
        // Over budget at the lowest tier: it stays there.
        assert_eq!(run(&mut t, QualitySetting::Auto, 25.0, 10.0), Quality::Low);
    }

    #[test]
    fn steps_up_when_fast_with_back_off() {
        let mut t = Throttle { tier: Quality::Low, ..default() };
        // Fast frames: up after the settle and the first wait.
        assert_eq!(run(&mut t, QualitySetting::Auto, 10.0, SETTLE + UP_WAIT + 1.0), Quality::Medium);
        // Medium is too slow: it is taken back, and the next try waits twice as long.
        assert_eq!(run(&mut t, QualitySetting::Auto, 25.0, SETTLE + DOWN_AFTER + 1.5), Quality::Low);
        assert_eq!(t.up_wait, UP_WAIT * 2.0);
        assert_eq!(run(&mut t, QualitySetting::Auto, 10.0, SETTLE + UP_WAIT + 1.0), Quality::Low);
        assert_eq!(run(&mut t, QualitySetting::Auto, 10.0, UP_WAIT), Quality::Medium);
    }

    #[test]
    fn noise_around_the_target_does_not_flicker() {
        let mut t = Throttle::default();
        let mut changes = 0;
        let mut last = t.tier;
        // Frames alternating 15 and 20 ms (an average just over 60 fps
        // with stutter): the smoothed time sits between the thresholds.
        for i in 0..6000 {
            let ms = if i % 2 == 0 { 15.0 } else { 20.0 };
            let q = t.step(QualitySetting::Auto, ms / 1000.0, 0.0);
            if q != last {
                changes += 1;
                last = q;
            }
        }
        assert_eq!(changes, 0);
    }

    #[test]
    fn a_fixed_setting_pins_the_tier() {
        let mut t = Throttle::default();
        assert_eq!(run(&mut t, QualitySetting::Fixed(Quality::Medium), 40.0, 20.0), Quality::Medium);
        assert_eq!(run(&mut t, QualitySetting::Fixed(Quality::High), 40.0, 20.0), Quality::High);
    }

    #[test]
    fn a_full_sim_frame_holds_the_tier() {
        let mut t = Throttle::default();
        for _ in 0..1000 {
            t.step(QualitySetting::Auto, 0.025, 15.0);
        }
        assert_eq!(t.tier, Quality::High);
    }

    #[test]
    fn heat_vision_adds_motion_blur_and_drops_the_fog() {
        let fx = Fx::of(Quality::High, true);
        assert!(fx.motion_blur && fx.volumetric.is_none());
        let fx = Fx::of(Quality::High, false);
        assert!(!fx.motion_blur && fx.volumetric.is_some());
        assert_eq!(Fx::of(Quality::Low, false).aa, Aa::Msaa4);
    }

    #[test]
    fn half_floats() {
        assert_eq!(half(1.0), 0x3c00u16.to_le_bytes());
        assert_eq!(half(0.5), 0x3800u16.to_le_bytes());
        assert_eq!(half(0.0), [0, 0]);
    }

    #[test]
    fn cube_faces_point_outward() {
        for face in 0..6 {
            let d = cube_dir(face, 4, 4, 8);
            let axis = [Vec3::X, -Vec3::X, Vec3::Y, -Vec3::Y, Vec3::Z, -Vec3::Z][face as usize];
            assert!(d.dot(axis) > 0.99, "face {face}: {d}");
        }
    }
}
