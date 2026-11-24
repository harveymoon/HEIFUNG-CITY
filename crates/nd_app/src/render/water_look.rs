//! [r15-water] The water's surface: a small extension of the standard
//! material for the water sheet (`nd_procgen::terrain::build_water`) and the
//! far water.
//!
//! The sheet was a flat vertex-coloured plane at alpha 0.86, its shading
//! stepping from cell to cell. Now, all in the fragment shader:
//! - ripples and waves as a normal (a few wind waves plus fine chop, rivers'
//!   chop carried down along their current, `uv` = the current in m/s);
//! - how deep the water is under each pixel, from the depth prepass where
//!   the camera has one (the ground behind the sheet), else from the
//!   distance to the shore (`uv_b.x`): clear and pale in the shallows,
//!   murky and dark in deep water;
//! - a lap of foam where the water meets anything it washes against;
//! - the sky reflected (its zenith, horizon and the city's glow band from
//!   `sky::SkyLook`, the sun's glint), and at night streaks of coloured
//!   light broken up by the ripples, as neon smears across a harbour.
//!
//! Frozen water (`uv_b.y`, or the ice's alpha) keeps its flat, still look.
//! No texture is sampled: nothing repeats on a grid.

use bevy::asset::uuid_handle;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::{Shader, ShaderRef};

use super::sky::SkyLook;

pub(super) const WATER_SHADER: Handle<Shader> = uuid_handle!("3b7e1c52-8a94-4f0d-b6e2-71c95a0d4e18");

const WATER_SHADER_SRC: &str = r#"
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
    view_transformations::depth_ndc_to_view_z,
}
#ifdef DEPTH_PREPASS
#import bevy_pbr::prepass_utils::prepass_depth
#endif

// x: seconds (wrapped), y: night 0..1, z: wave strength, w: unused.
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> water_clock: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> water_zenith: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(102) var<uniform> water_horizon: vec4<f32>;
// rgb: the glow band, w: its height.
@group(#{MATERIAL_BIND_GROUP}) @binding(103) var<uniform> water_glow: vec4<f32>;
// xyz: toward the sun or moon, w: daylight.
@group(#{MATERIAL_BIND_GROUP}) @binding(104) var<uniform> water_sun: vec4<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(105) var<uniform> water_glare: vec4<f32>;

fn wh(p: vec2<f32>) -> f32 {
    let q = vec2<u32>(vec2<i32>(p) + vec2<i32>(1048576));
    var h = q.x * 1664525u + q.y * 1013904223u + 374761393u;
    h = (h ^ (h >> 16u)) * 2246822519u;
    h = (h ^ (h >> 13u)) * 3266489917u;
    h = h ^ (h >> 16u);
    return f32(h) * (1.0 / 4294967295.0);
}

// Value noise and its gradient.
fn vnoise(p: vec2<f32>) -> vec3<f32> {
    let i = floor(p);
    let f = p - i;
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let du = 30.0 * f * f * (f * (f - 2.0) + 1.0);
    let a = wh(i);
    let b = wh(i + vec2(1.0, 0.0));
    let c = wh(i + vec2(0.0, 1.0));
    let d = wh(i + vec2(1.0, 1.0));
    let k = a - b - c + d;
    let v = a + (b - a) * u.x + (c - a) * u.y + k * u.x * u.y;
    return vec3(v, du * vec2(b - a + k * u.y, c - a + k * u.x));
}

// The slope of the water surface (dh/dx, dh/dz) at p.
fn water_slope(p: vec2<f32>, flow: vec2<f32>, t: f32, calm: f32) -> vec2<f32> {
    var g = vec2(0.0);
    // Wind waves: a few trains of sines, each at its deep-water speed.
    let dirs = array<vec2<f32>, 4>(vec2(0.8, 0.6), vec2(0.96, -0.28), vec2(0.34, 0.94), vec2(-0.55, 0.83));
    let lens = array<f32, 4>(11.0, 6.3, 3.7, 2.1);
    for (var i = 0; i < 4; i = i + 1) {
        let k = 6.2831853 / lens[i];
        let w = sqrt(9.81 * k);
        let amp = lens[i] * 0.010 * calm;
        g += dirs[i] * (amp * k * cos(dot(dirs[i], p) * k - w * t + f32(i) * 1.7));
    }
    // Chop, carried by the current.
    let q = p - flow * t;
    let n1 = vnoise(q * 0.55 + vec2(t * 0.11, t * 0.07));
    let n2 = vnoise(q * 1.9 - vec2(t * 0.23, -t * 0.17));
    g += n1.yz * 0.55 * 0.16 + n2.yz * 1.9 * 0.05;
    // A river's ripples streak along its course.
    let speed = length(flow);
    if speed > 0.05 {
        let fd = flow / speed;
        let sd = vec2(-fd.y, fd.x);
        let s = vec2(dot(q, fd) * 0.35, dot(q, sd) * 1.4);
        let n3 = vnoise(s + vec2(0.0, t * 0.2));
        g += (fd * n3.y * 0.35 + sd * n3.z * 1.4) * 0.06 * min(speed, 2.0);
    }
    return g;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let t = water_clock.x;
    let night = water_clock.y;
    let p = in.world_position.xz;
    let base = pbr_input.material.base_color;
    var flow = vec2(0.0);
    var shore = -30.0;
    var frozen = base.a > 0.93;
#ifdef VERTEX_UVS_B
    flow = in.uv;
    shore = in.uv_b.x;
    frozen = frozen || in.uv_b.y > 0.5;
#endif
    let to_eye = view.world_position - in.world_position.xyz;
    let dist = length(to_eye);
    let V = to_eye / max(dist, 1e-3);

    // How deep the water is under this pixel.
    var thick = clamp(-shore, 0.0, 40.0) * 0.3 + 0.4;
#ifdef DEPTH_PREPASS
    let raw = prepass_depth(in.position, 0u);
    if raw > 0.0 {
        let along = max(depth_ndc_to_view_z(in.position.z) - depth_ndc_to_view_z(raw), 0.0);
        thick = along * max(V.y, 0.08);
    } else {
        thick = 60.0;
    }
#endif

    // The ripples, fading with distance so they never shimmer.
    var N = vec3(0.0, 1.0, 0.0);
    if !frozen {
        let fade = water_clock.z / (1.0 + dist / 220.0);
        let g = water_slope(p, flow, t, 1.0) * fade;
        N = normalize(vec3(-g.x, 1.0, -g.y));
    }
    pbr_input.N = N;
    pbr_input.V = V;

    var rgb = base.rgb;
    var alpha = base.a;
    var foam = 0.0;
    if !frozen {
        // Clear over the shallows, murky deep down.
        let murk = 1.0 - exp(-thick * 0.9);
        let clear = base.rgb + vec3(0.0, 0.015, 0.012);
        rgb = mix(clear, base.rgb * 0.7, murk);
        alpha = mix(0.45, 0.95, murk);
        // Foam lapping at whatever the water washes against.
        let n = vnoise(p * 0.9 + vec2(t * 0.25, -t * 0.18)).x;
        let edge = 1.0 - smoothstep(0.0, 0.22 + 0.18 * n, thick);
        let lap = 0.5 + 0.5 * sin(thick * 16.0 - t * 1.7 + n * 4.0);
        foam = edge * (0.45 + 0.55 * lap) * smoothstep(0.25, 0.65, n + 0.3 * edge);
        rgb = mix(rgb, vec3(0.7, 0.75, 0.76), foam * 0.6);
        alpha = max(alpha, foam * 0.75);
    }
    pbr_input.material.base_color = vec4(rgb, alpha);
    var color = apply_pbr_lighting(pbr_input);

    // The sky, mirrored: zenith to horizon, the glow band low down, the
    // sun's (or moon's) glint.
    let R = reflect(-V, N);
    let up = clamp(R.y, 0.0, 1.0);
    var sky = mix(water_horizon.rgb, water_zenith.rgb, sqrt(up));
    sky += water_glow.rgb * (1.0 - smoothstep(0.0, max(water_glow.w, 0.02), up));
    sky += water_glare.rgb * pow(max(dot(R, water_sun.xyz), 0.0), 220.0) * 4.0;
    let fres = 0.02 + 0.98 * pow(1.0 - max(dot(N, V), 0.0), 5.0);
    var refl = sky * fres * select(0.5, 0.25, frozen);
    // At night, the city's neon smeared across the water: streaks toward
    // the eye, broken up by the ripples.
    if night > 0.02 && !frozen {
        // Columns of light running toward the eye (as a lit window or a
        // sign across the water draws one), wavering with the ripples.
        let toward = normalize(view.world_position.xz - p);
        let side = vec2(-toward.y, toward.x);
        let a = dot(p, side);
        let b = dot(p, toward);
        let col = 0.5 + 0.5 * sin(a * 0.21 + 2.5 * sin(a * 0.047) + N.x * 6.0 + N.z * 4.0);
        let streak = pow(col, 14.0) * (0.55 + 0.45 * sin(b * 0.9 - t * 0.8 + N.z * 8.0));
        let hue = 0.5 + 0.5 * sin(a * 0.013 + 1.3);
        let neon = mix(vec3(1.0, 0.15, 0.6), vec3(0.1, 0.75, 1.0), hue);
        let near = 1.0 - smoothstep(250.0, 900.0, dist);
        refl += mix(water_glow.rgb * 2.0, neon, 0.6) * streak * night * near * (0.2 + 0.8 * fres) * 0.4;
    }
    color = vec4(color.rgb + refl * color.a, color.a);
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, color);
    return out;
}
"#;

/// The water's material: the standard one (vertex colours, blended) plus
/// the surface above.
pub(super) type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterSurface>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub(super) struct WaterSurface {
    /// x: seconds (wrapped); y: night 0..1; z: wave strength.
    #[uniform(100)]
    pub clock: Vec4,
    #[uniform(101)]
    pub zenith: Vec4,
    #[uniform(102)]
    pub horizon: Vec4,
    /// rgb: the glow band; w: its height.
    #[uniform(103)]
    pub glow: Vec4,
    /// xyz: toward the sun or moon; w: daylight.
    #[uniform(104)]
    pub sun: Vec4,
    #[uniform(105)]
    pub glare: Vec4,
}

impl MaterialExtension for WaterSurface {
    fn fragment_shader() -> ShaderRef {
        WATER_SHADER.into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

pub(super) fn add_water_shader(mut shaders: ResMut<Assets<Shader>>) {
    // `ND_WATER_SHADER=<file.wgsl>`: the surface from a file instead, for
    // tuning the look without a rebuild (a restart picks it up).
    let src = std::env::var("ND_WATER_SHADER").ok().and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_else(|| WATER_SHADER_SRC.to_string());
    let _ = shaders.insert(WATER_SHADER.id(), Shader::from_wgsl(src, "nd_app/water_surface.wgsl"));
}

/// The one water material, shared by the sheet and the far water.
#[derive(Resource, Default)]
pub(super) struct WaterLook {
    pub mat: Option<Handle<WaterMaterial>>,
}

impl WaterLook {
    pub(super) fn material(&mut self, materials: &mut Assets<WaterMaterial>) -> Handle<WaterMaterial> {
        self.mat
            .get_or_insert_with(|| {
                materials.add(WaterMaterial {
                    base: StandardMaterial { base_color: Color::WHITE, alpha_mode: AlphaMode::Blend, perceptual_roughness: 0.08, reflectance: 0.35, metallic: 0.0, ..default() },
                    extension: WaterSurface { clock: Vec4::new(0.0, 0.0, 1.0, 0.0), ..default() },
                })
            })
            .clone()
    }
}

/// Every frame: the waves' clock, and the sky the water mirrors.
pub(super) fn sync_water_look(time: Res<Time>, sky: Res<SkyLook>, look: Res<WaterLook>, mut materials: ResMut<Assets<WaterMaterial>>) {
    let Some(h) = &look.mat else { return };
    let Some(mut m) = materials.get_mut(h) else { return };
    let v3 = |c: [f32; 3], w: f32| Vec4::new(c[0], c[1], c[2], w);
    let e = &mut m.extension;
    e.clock = Vec4::new(time.elapsed_secs_wrapped() % 3600.0, (1.0 - sky.daylight).clamp(0.0, 1.0), 1.0, 0.0);
    e.zenith = v3(sky.zenith, 0.0);
    e.horizon = v3(sky.horizon, 0.0);
    e.glow = v3(sky.glow, sky.glow_height);
    e.sun = Vec4::new(sky.sun_dir.x, sky.sun_dir.y, sky.sun_dir.z, sky.daylight);
    e.glare = v3(sky.sun_glare, 0.0);
}
