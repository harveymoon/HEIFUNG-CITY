//! [heat] Drone heat vision: the city as a thermal camera on a drone sees
//! it. Toggled with H, the top bar's Heat button or the API's
//! `heat_vision` (all through [`Lighting::heat`]).
//!
//! Two halves:
//!
//! - **What is hot.** Every mesh entity's material is swapped for a heat
//!   stand-in while the mode is on: buildings, ground, roads and water go
//!   to a dim unlit grey (their vertex colours and textures still show, so
//!   the streets stay readable), while cars, people, trains, drones,
//!   emergency units and fires emit a flat grey at their "temperature",
//!   lights and engines hottest. The stand-ins are derived once per
//!   original material and class and cached; the swapping itself is
//!   shared with the focus views (`render/swap.rs`), which keeps the
//!   original handle on the entity and puts it back when the mode goes
//!   off. Systems that set a material while the mode is on (a citizen
//!   going indoors, a signal changing) are caught by change detection and
//!   swapped again.
//! - **How it looks.** A fullscreen pass ([`HeatPost`], before bloom, after
//!   motion blur) maps the frame's brightness, which is now temperature,
//!   through a cold-teal to magenta to white-hot palette and adds faint
//!   scanlines, sensor noise, a vignette and a targeting reticle. Bevy's
//!   per-object motion blur smears whatever moves (put on the camera by
//!   `look::apply_camera_effects`, which derives the camera's effects
//!   from the quality tier and this mode every frame). The sun's shadows
//!   are switched off meanwhile (nothing lit by it shows), and the sky and
//!   fog go black.

use bevy::asset::uuid_handle;
use bevy::core_pipeline::fullscreen_material::{FullscreenMaterial, FullscreenMaterialPlugin};
use bevy::core_pipeline::Core3dSystems;
use bevy::ecs::schedule::ScheduleConfigs;
use bevy::ecs::system::BoxedSystem;
use bevy::pbr::DistanceFog;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::extract_component::ExtractComponent;
use bevy::render::render_resource::ShaderType;
use bevy::shader::{Shader, ShaderRef};

use super::{Lighting, RenderState, SunTag};
use crate::ui::UiFlags;

const HEAT_SHADER: Handle<Shader> = uuid_handle!("2c7d4e1a-8b3f-4f6a-9d25-7e1c0b8a4f93");

/// Brightness the cool world is drawn at: lit surfaces, unlit glow (signs,
/// lamps, which are bright to begin with) and translucent layers.
const COOL_LIT: f32 = 0.035;
const COOL_GLOW: f32 = 0.02;
const COOL_CLEAR: f32 = 0.02;
/// Window lights: a building's lit rooms leak a little warmth.
pub(super) const COOL_WINDOW: f32 = 0.05;

/// Bloom while the camera sees heat: hot bodies bleed a little into the
/// cold around them, as on a real sensor.
const HEAT_BLOOM: f32 = 0.16;

/// The heat pass's settings, on the camera while the mode is on.
#[derive(Component, ExtractComponent, Clone, Copy, ShaderType, Default)]
pub(super) struct HeatPost {
    /// Seconds, for the sensor noise and the scan band.
    time: f32,
    /// Overall strength of the scanlines and noise, `0..1`.
    grain: f32,
    _pad: Vec2,
}

impl FullscreenMaterial for HeatPost {
    fn fragment_shader() -> ShaderRef {
        HEAT_SHADER.into()
    }

    /// After motion blur (so the trails are recoloured with the rest) and
    /// before bloom (so the white-hot spots glow).
    fn schedule_configs(system: ScheduleConfigs<BoxedSystem>) -> ScheduleConfigs<BoxedSystem> {
        system.in_set(Core3dSystems::PostProcess).after(bevy::post_process::motion_blur::motion_blur).before(bevy::post_process::bloom::bloom)
    }
}

const HEAT_SHADER_SRC: &str = r#"
#import bevy_core_pipeline::fullscreen_vertex_shader::FullscreenVertexOutput

struct HeatPost {
    time: f32,
    grain: f32,
    pad: vec2<f32>,
}

@group(0) @binding(0) var screen_texture: texture_2d<f32>;
@group(0) @binding(1) var screen_sampler: sampler;
@group(0) @binding(2) var<uniform> settings: HeatPost;

// Cold to hot: black, deep teal, indigo, magenta, red, orange, yellow,
// white. Linear HDR, before tonemapping.
fn thermal(h: f32) -> vec3<f32> {
    var c = mix(vec3(0.000, 0.002, 0.006), vec3(0.004, 0.022, 0.034), smoothstep(0.0, 0.03, h));
    c = mix(c, vec3(0.012, 0.070, 0.085), smoothstep(0.03, 0.09, h));
    c = mix(c, vec3(0.090, 0.030, 0.220), smoothstep(0.09, 0.22, h));
    c = mix(c, vec3(0.480, 0.020, 0.320), smoothstep(0.22, 0.48, h));
    c = mix(c, vec3(1.250, 0.140, 0.070), smoothstep(0.48, 0.85, h));
    c = mix(c, vec3(2.100, 0.750, 0.050), smoothstep(0.85, 1.35, h));
    c = mix(c, vec3(3.000, 2.300, 0.550), smoothstep(1.35, 2.10, h));
    c = mix(c, vec3(4.500, 4.500, 4.000), smoothstep(2.10, 3.20, h));
    return c;
}

fn hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(12.9898, 78.233))) * 43758.5453);
}

// A thin line of width `w` pixels at distance `d` from it.
fn line(d: f32, w: f32) -> f32 {
    return 1.0 - smoothstep(w * 0.5, w * 0.5 + 1.0, abs(d));
}

@fragment
fn fragment(in: FullscreenVertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(screen_texture));
    let px = in.uv * dims;
    let src = textureSample(screen_texture, screen_sampler, in.uv).rgb;
    let h = dot(src, vec3(0.2126, 0.7152, 0.0722));
    var col = thermal(h);

    // Scanlines every three pixels, and a slow brighter band rolling down.
    let scan = 1.0 - 0.10 * settings.grain * step(2.0, px.y % 3.0);
    let band_y = fract(settings.time * 0.07);
    let band = 1.0 + 0.06 * settings.grain * exp(-pow((in.uv.y - band_y) * 18.0, 2.0));
    col = col * scan * band;

    // Sensor noise, changing every frame-ish.
    let n = hash(floor(px) + vec2(floor(settings.time * 30.0) * 7.0, 3.0)) - 0.5;
    col = col + vec3(n * 0.012 * settings.grain) * (0.3 + min(h, 1.0));

    // Vignette.
    let d = distance(in.uv, vec2(0.5));
    col = col * (1.0 - 0.7 * smoothstep(0.42, 0.78, d));

    // Reticle: a centre cross with a gap, and corner brackets.
    let c = px - dims * 0.5;
    let arm = 22.0;
    let gap = 7.0;
    var r = 0.0;
    r = max(r, line(c.y, 1.0) * step(gap, abs(c.x)) * step(abs(c.x), arm));
    r = max(r, line(c.x, 1.0) * step(gap, abs(c.y)) * step(abs(c.y), arm));
    let m = 0.08 * min(dims.x, dims.y);
    let q = abs(c) - (dims * 0.5 - vec2(m, m));
    let blen = 0.06 * min(dims.x, dims.y);
    let bx = line(q.x, 1.5) * step(-blen, q.y) * step(q.y, 0.0);
    let by = line(q.y, 1.5) * step(-blen, q.x) * step(q.x, 0.0);
    r = max(r, max(bx, by));
    col = mix(col, vec3(0.35, 0.95, 0.95), r * 0.55);

    return vec4(col, 1.0);
}
"#;

/// What the stand-in for a material is derived as.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum HeatClass {
    Cool,
    /// Emitting at this temperature (in thousandths, for hashing).
    Hot(u32),
}

pub(super) struct HeatPlugin;

impl Plugin for HeatPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FullscreenMaterialPlugin::<HeatPost>::default())
            .add_systems(Startup, add_shader)
            .add_systems(Update, toggle_key)
            .add_systems(PostUpdate, apply_heat);
    }
}

fn add_shader(mut shaders: ResMut<Assets<Shader>>) {
    let _ = shaders.insert(HEAT_SHADER.id(), Shader::from_wgsl(HEAT_SHADER_SRC, "nd_app/heat_vision.wgsl"));
}

fn toggle_key(keys: Res<ButtonInput<KeyCode>>, flags: Res<UiFlags>, mode: Res<crate::camera::CameraMode>, mut lighting: ResMut<Lighting>) {
    // [r5-drive] At the wheel H is the horn.
    if keys.just_pressed(KeyCode::KeyH) && !flags.keyboard_captured && *mode != crate::camera::CameraMode::Drive {
        lighting.heat = !lighting.heat;
    }
}

/// Camera, sky and sun for the mode: the post pass on the camera, black
/// sky and fog, hot bloom, no sun shadows. Everything else the mode needs
/// on the camera (motion blur, no volumetric fog) is derived from
/// [`Lighting::heat`] by `look::apply_camera_effects`, so nothing is stored
/// here to be put back: when the mode ends the post pass comes off and the
/// daylight pass rewrites the sky, fog, bloom and sun from scratch.
pub(super) fn apply_heat(
    mut commands: Commands,
    lighting: Res<Lighting>,
    time: Res<Time>,
    mut was_on: Local<bool>,
    mut render: ResMut<RenderState>,
    mut clear: ResMut<ClearColor>,
    mut cams: Query<(Entity, &mut Bloom, &mut DistanceFog, Option<&mut HeatPost>), With<crate::camera::OrbitCamera>>,
    mut sun: Query<&mut DirectionalLight, With<SunTag>>,
) {
    let on = lighting.heat;
    if on {
        clear.0 = Color::BLACK;
        for (e, mut bloom, mut fog, post) in &mut cams {
            fog.color = Color::BLACK;
            bloom.intensity = HEAT_BLOOM;
            match post {
                Some(mut p) => p.time = time.elapsed_secs_wrapped(),
                None => {
                    commands.entity(e).insert(HeatPost { time: time.elapsed_secs_wrapped(), grain: 1.0, _pad: Vec2::ZERO });
                }
            }
        }
        for mut s in &mut sun {
            s.shadow_maps_enabled = false;
        }
    } else {
        for (e, _, _, post) in &mut cams {
            if post.is_some() {
                commands.entity(e).remove::<HeatPost>();
            }
        }
        if *was_on {
            for mut s in &mut sun {
                s.shadow_maps_enabled = true;
            }
            // Sky, fog, bloom and glow are rewritten on the next daylight pass.
            render.daylight_key = None;
            render.glow_key = None;
        }
    }
    *was_on = on;
}

/// How hot an entity's body and its lights are, by what it is; `None` for
/// the cold world.
#[allow(clippy::type_complexity)]
pub(super) fn temperature(tags: (bool, bool, bool, bool, bool, bool, bool, bool, bool, bool)) -> Option<(f32, f32)> {
    let (car, citizen, bot, train, parked, ship, emergency, stunt, air, puff) = tags;
    Some(if citizen {
        (1.5, 1.5)
    } else if emergency {
        (0.85, 2.8)
    } else if stunt {
        (0.9, 2.0)
    } else if car {
        (0.75, 2.6)
    } else if air {
        (0.85, 2.4)
    } else if train {
        (0.65, 2.2)
    } else if bot {
        (0.6, 1.8)
    } else if ship {
        (0.45, 1.2)
    } else if puff {
        (0.3, 0.3)
    } else if parked {
        // Engines off, still a little warm.
        (0.2, 0.2)
    } else {
        return None;
    })
}

pub(super) fn class_of(o: &StandardMaterial, temp: Option<(f32, f32)>) -> HeatClass {
    match temp {
        None => HeatClass::Cool,
        Some((body, lights)) => {
            let t = if o.unlit { lights } else { body };
            let t = if matches!(o.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_)) { t } else { t * 0.5 };
            HeatClass::Hot((t * 1000.0) as u32)
        }
    }
}

/// The stand-in for `o`: cold surfaces keep their texture and vertex
/// colours, dimmed and unlit; hot ones emit their temperature flat, so a
/// red car and a white one read the same.
pub(super) fn stand_in(o: &StandardMaterial, class: HeatClass) -> StandardMaterial {
    let alpha = o.base_color.alpha();
    let base = StandardMaterial { alpha_mode: o.alpha_mode, double_sided: o.double_sided, cull_mode: o.cull_mode, fog_enabled: o.fog_enabled, ..default() };
    match class {
        HeatClass::Cool => {
            let k = if !matches!(o.alpha_mode, AlphaMode::Opaque | AlphaMode::Mask(_)) {
                COOL_CLEAR
            } else if o.unlit {
                COOL_GLOW
            } else {
                COOL_LIT
            };
            StandardMaterial { base_color: Color::linear_rgba(k, k, k, alpha), base_color_texture: o.base_color_texture.clone(), uv_transform: o.uv_transform, unlit: true, ..base }
        }
        HeatClass::Hot(t) => {
            let t = t as f32 / 1000.0;
            // Emissive alpha 0: not scaled by the camera's exposure.
            StandardMaterial { base_color: Color::linear_rgba(0.0, 0.0, 0.0, alpha), emissive: LinearRgba::new(t, t, t, 0.0), perceptual_roughness: 1.0, metallic: 0.0, reflectance: 0.0, unlit: false, ..base }
        }
    }
}
