//! [r17-cars] Car paint: one material for every vehicle body, near and far.
//!
//! The vehicle meshes (`nd_procgen::vehicles`) carry per vertex what each
//! part is (`uv.x`: paint, glass, tyre, chrome, black trim, alloy, rough,
//! gloss black), the paint's finish (`uv.y`'s whole part: solid, gloss,
//! metallic, pearl, matte wrap, worn), how dirty it is there (`uv.y`'s
//! fraction) and a position on the body (the second UV set). This
//! extension of the standard material reads them and sets the surface per
//! fragment before the standard lighting:
//!
//! - **paint**: a clear coat over the base on all but worn and matte cars
//!   (sky and lamps slide over the bodywork), metallic flakes that glint
//!   close up, a pearl's hue that shifts towards the edges;
//! - **glass** dark and glassy (it reflects the sky and the lamps), chrome
//!   a mirror, tyres and plastics dull;
//! - **dirt** low on the body: browner, rougher, the coat dulled;
//! - **rain**: a water film (glossier, darker on rough parts, the coat
//!   full) and beads on the upper surfaces, following the wet streets;
//! - **night**: the neon of the street reflected in the bodywork and the
//!   glass, as vertical sign strips round the horizon that slide over the
//!   car as it moves (`neon_env`), strongest at grazing angles.
//!
//! One material instance for every body keeps every variant's cars in one
//! batch. The swapper (`swap.rs`) gives stand-in views (heat, focus,
//! ghost) their own copies with the effects off (`look.z`).

use bevy::asset::uuid_handle;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::{Shader, ShaderRef};

use super::Lighting;
use crate::sim::Sim;

pub(crate) type CarPaintMaterial = ExtendedMaterial<StandardMaterial, CarPaint>;

#[derive(ShaderType, Clone, Copy, Debug, Default, PartialEq, Reflect)]
pub(crate) struct CarPaintParams {
    /// x: night (0..1), y: wet (0..1), z: 1 for a stand-in (no effects),
    /// w: the neon reflections' strength.
    pub look: Vec4,
    pub neon_a: Vec4,
    pub neon_b: Vec4,
    pub neon_c: Vec4,
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub(crate) struct CarPaint {
    #[uniform(100)]
    pub params: CarPaintParams,
}

impl MaterialExtension for CarPaint {
    fn fragment_shader() -> ShaderRef {
        CAR_SHADER.into()
    }
}

const CAR_SHADER: Handle<Shader> = uuid_handle!("3c9e2f71-6b8a-4d05-a1e7-58c2d4f90b16");

const CAR_HEAD_SRC: &str = r#"
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
    mesh_view_bindings::view,
}
#ifdef VISIBILITY_RANGE_DITHER
#import bevy_pbr::pbr_functions::visibility_range_dither
#endif
"#;

/// The paint itself. Kept apart from the entry point so a test can
/// validate it with naga.
pub(crate) const CAR_FN_SRC: &str = r#"
struct CarPaintParams {
    look: vec4<f32>,
    neon_a: vec4<f32>,
    neon_b: vec4<f32>,
    neon_c: vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> car: CarPaintParams;

fn c_hash(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2(0.1031, 0.1030));
    q += dot(q, q.yx + 33.33);
    return fract((q.x + q.y) * q.x);
}

struct CarSurf {
    base: vec3<f32>,
    rough: f32,
    metal: f32,
    refl: f32,
    coat: f32,
    coat_rough: f32,
}

// The surface of a part: kind 0 paint (finish `fin`), 1 glass, 2 tyre,
// 3 chrome, 4 black trim, 5 alloy, 6 rough, 7 gloss black; `dirt` 0..1
// here, `wet` 0..1 now.
fn car_surface(base: vec3<f32>, kind: u32, fin: u32, dirt: f32, wet: f32) -> CarSurf {
    var s: CarSurf;
    s.base = base;
    s.rough = 0.5;
    s.metal = 0.0;
    s.refl = 0.5;
    s.coat = 0.0;
    s.coat_rough = 0.1;
    switch kind {
        case 0u: {
            switch fin {
                case 0u: { s.rough = 0.46; s.coat = 0.35; s.coat_rough = 0.22; }
                case 1u: { s.rough = 0.32; s.coat = 1.0; s.coat_rough = 0.05; }
                case 2u: { s.rough = 0.36; s.metal = 0.5; s.coat = 1.0; s.coat_rough = 0.035; }
                case 3u: { s.rough = 0.3; s.metal = 0.28; s.coat = 1.0; s.coat_rough = 0.03; }
                case 4u: { s.rough = 0.7; s.metal = 0.06; }
                default: { s.rough = 0.8; }
            }
        }
        case 1u: { s.rough = 0.035; s.refl = 0.75; }
        case 2u: { s.rough = 0.88; s.refl = 0.25; }
        case 3u: { s.rough = 0.1; s.metal = 1.0; }
        case 4u: { s.rough = 0.58; s.refl = 0.35; }
        case 5u: { s.rough = 0.28; s.metal = 0.85; }
        case 6u: { s.rough = 0.92; s.refl = 0.3; }
        default: { s.rough = 0.1; s.coat = 1.0; s.coat_rough = 0.04; }
    }
    // Road dirt: browner, rougher, the coat dulled (glass keeps a little).
    let d = select(dirt, dirt * 0.35, kind == 1u);
    s.base = mix(s.base, vec3(0.13, 0.115, 0.095) * 0.6 + s.base * 0.4, d * 0.6);
    s.rough = mix(s.rough, 0.9, d * 0.75);
    s.coat = s.coat * (1.0 - d * 0.8);
    // A film of water: glossier, rough parts darker, a full coat.
    let film = wet * select(1.0, 0.55, kind == 2u);
    s.base = s.base * mix(1.0, 0.8, film * smoothstep(0.3, 0.8, s.rough));
    s.rough = mix(s.rough, s.rough * 0.35, film);
    s.coat = max(s.coat, film * 0.9);
    s.coat_rough = mix(s.coat_rough, 0.02, film);
    return s;
}

// The street's neon as seen along `r` from `p`: lit sign strips of three
// colours round the car, sliding as it moves, and the glow of the lit,
// often wet street under them. Seen from the game's cameras (above the
// cars) a door reflects mostly what is below the horizon, so the signs
// are spread over both: shopfronts and their reflections in the road.
fn neon_env(r: vec3<f32>, p: vec3<f32>) -> vec3<f32> {
    let el = r.y;
    if el > 0.75 {
        return vec3(0.0);
    }
    let az = atan2(r.z, r.x);
    let a = az * 3.2 + dot(p.xz, vec2(0.061, 0.047));
    let k = floor(a);
    let f = fract(a);
    let lit = step(0.3, c_hash(vec2(k, 17.0))) * (0.6 + 0.8 * c_hash(vec2(k, 23.0)));
    let strip = smoothstep(0.04, 0.12, f) * (1.0 - smoothstep(0.42, 0.52, f));
    let lo = -0.45 + 0.4 * c_hash(vec2(k + 5.0, 3.0));
    let hi = lo + 0.18 + 0.35 * c_hash(vec2(k + 9.0, 7.0));
    let v = smoothstep(lo - 0.03, lo + 0.02, el) * (1.0 - smoothstep(hi - 0.03, hi + 0.02, el));
    let pick = c_hash(vec2(k * 1.3, 11.0));
    var col = car.neon_c.rgb;
    if pick < 0.36 {
        col = car.neon_a.rgb;
    } else if pick < 0.7 {
        col = car.neon_b.rgb;
    }
    // The street's glow, strongest round the horizon and in the road.
    let glow = (1.0 - smoothstep(0.0, 0.6, abs(el + 0.1))) * 0.14;
    return col * (lit * strip * v + glow);
}
"#;

const CAR_MAIN_SRC: &str = r#"
@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
#ifdef VISIBILITY_RANGE_DITHER
    visibility_range_dither(in.position, in.visibility_range_dither);
#endif
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    var glint = vec3(0.0);
    let unlit = (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) != 0u;
#ifdef VERTEX_UVS_A
    if car.look.z < 0.5 && !unlit {
        let kind = u32(in.uv.x + 0.5);
        let fin = u32(floor(in.uv.y + 0.001));
        let dirt = clamp(in.uv.y - f32(fin), 0.0, 0.95);
        var s = car_surface(pbr_input.material.base_color.rgb, kind, fin, dirt, car.look.y);
        let v = pbr_input.V;
        let world = in.world_position.xyz;
        let near = 1.0 - smoothstep(8.0, 40.0, distance(world, view.world_position.xyz));
        let ndv = clamp(dot(pbr_input.N, v), 0.0, 1.0);
        if kind == 0u && fin == 3u {
            // Pearl: the hue turns towards the edges.
            let edge = pow(1.0 - ndv, 2.0);
            s.base = mix(s.base, s.base.gbr * 0.55 + s.base * 0.45 + vec3(0.03), edge * 0.65);
        }
#ifdef VERTEX_UVS_B
        if kind == 0u && (fin == 2u || fin == 3u) && near > 0.0 {
            // Metallic flakes under the coat: a few cells tilt their
            // normal and catch the light.
            let cell = floor(in.uv_b * 700.0);
            let h = c_hash(cell);
            let j = vec3(c_hash(cell + 1.7), c_hash(cell + 3.1), c_hash(cell + 5.3)) - vec3(0.5);
            pbr_input.N = normalize(pbr_input.N + j * 0.5 * near * step(0.55, h));
        }
        if car.look.y > 0.05 {
            // Beads of rain on the upper surfaces.
            let up = smoothstep(0.35, 0.85, pbr_input.N.y);
            let cell = floor(in.uv_b * 55.0);
            let drop = step(0.8, c_hash(cell)) * up * car.look.y * near;
            s.base = s.base * (1.0 - drop * 0.35);
            s.rough = mix(s.rough, 0.05, drop);
        }
#endif
        pbr_input.material.base_color = vec4(s.base, pbr_input.material.base_color.a);
        pbr_input.material.perceptual_roughness = s.rough;
        pbr_input.material.metallic = s.metal;
        pbr_input.material.reflectance = vec3(s.refl);
        pbr_input.material.clearcoat = s.coat;
        pbr_input.material.clearcoat_perceptual_roughness = s.coat_rough;
        // The neon of the street in glossy parts after dark.
        let night = car.look.x * car.look.w;
        if night > 0.01 && (kind == 0u || kind == 1u || kind == 3u || kind == 7u) {
            let n = normalize(pbr_input.clearcoat_N);
            let cv = clamp(dot(n, v), 0.0, 1.0);
            let fres = 0.04 + 0.96 * pow(1.0 - cv, 5.0);
            let gloss = select(max(s.coat * (1.0 - s.coat_rough * 4.0), (1.0 - s.rough) * 0.5), 1.0, kind == 1u || kind == 3u);
            let r = reflect(-v, n);
            glint = neon_env(r, world) * fres * gloss * night;
        }
    }
#endif
    var out: FragmentOutput;
    if unlit {
        out.color = pbr_input.material.base_color;
    } else {
        out.color = apply_pbr_lighting(pbr_input);
        out.color = vec4(out.color.rgb + glint, out.color.a);
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
"#;

pub(crate) fn add_shader(mut shaders: ResMut<Assets<Shader>>) {
    let main = format!("{CAR_HEAD_SRC}{CAR_FN_SRC}{CAR_MAIN_SRC}");
    let _ = shaders.insert(CAR_SHADER.id(), Shader::from_wgsl(main, "nd_app/car_paint.wgsl"));
}

/// The base every car's paint starts from: white (the vertex colours are
/// the paint), a clear coat so the shader's coat is compiled in.
pub(crate) fn base_paint() -> StandardMaterial {
    StandardMaterial { base_color: Color::WHITE, perceptual_roughness: 0.4, metallic: 0.0, reflectance: 0.5, clearcoat: 1.0, clearcoat_perceptual_roughness: 0.05, ..default() }
}

/// The street's three neon colours (linear) for the reflections.
fn neons(pal: &nd_procgen::Palette) -> [Vec4; 3] {
    let at = |i: usize| -> Vec4 {
        let c = pal.vehicle_neon.get(i % pal.vehicle_neon.len().max(1)).copied().unwrap_or([1.0, 0.3, 1.0]);
        // The palette's neon is HDR; a sign seen in glossy paint at a grazing
        // angle is about as bright as the sign.
        Vec4::new(c[0] * 1.5, c[1] * 1.5, c[2] * 1.5, 1.0)
    };
    [at(0), at(1), at(3)]
}

/// The shared paint: made once at startup (`VehicleRender::paint`).
pub(super) fn setup_car_paint(mut vr: ResMut<super::VehicleRender>, assets: Res<crate::assets::GameAssets>, mut mats: ResMut<Assets<CarPaintMaterial>>) {
    if vr.paint.is_some() {
        return;
    }
    let [a, b, c] = neons(&assets.palette);
    let params = CarPaintParams { look: Vec4::new(0.0, 0.0, 0.0, neon_strength()), neon_a: a, neon_b: b, neon_c: c };
    vr.paint = Some(mats.add(CarPaintMaterial { base: base_paint(), extension: CarPaint { params } }));
}

/// `ND_CAR_NEON` scales the night reflections (dev, for judging the look).
fn neon_strength() -> f32 {
    std::env::var("ND_CAR_NEON").ok().and_then(|v| v.parse::<f32>().ok()).map_or(1.0, |k| k.clamp(0.0, 4.0))
}

/// Keep the paint's night and wet with the sky and the streets: the
/// streets' own wetting (`sync_wet_roads`: soaks in about 12 s of rain,
/// dries in 45 s). Written only when a quantised value moves.
pub(super) fn sync_car_paint(sim: Res<Sim>, time: Res<Time>, lighting: Res<Lighting>, vr: Res<super::VehicleRender>, mut mats: ResMut<Assets<CarPaintMaterial>>, mut wet: Local<f32>, mut applied: Local<Option<(i32, i32)>>) {
    let _t = crate::frame_stats::span("sync_car_paint");
    let Some(h) = vr.paint.as_ref() else { return };
    // Rain, or snow melting on the bodywork above freezing.
    let warm = sim.world.weather.temp_c > 0.5;
    let rain = super::climate::precip_shown(&sim.world, &lighting).filter(|(k, _)| *k == "rain" || warm).map(|(_, i)| i).unwrap_or(0.0);
    let dt = time.delta_secs().min(0.25);
    *wet = if rain > *wet { (*wet + dt / 12.0).min(rain.max(0.35)) } else { (*wet - dt / 45.0).max(rain) };
    if !lighting.show_weather {
        *wet = 0.0;
    }
    let night = super::daylight::night_strength(&sim, &lighting);
    let key = ((night * 32.0).round() as i32, (*wet * 32.0).round() as i32);
    if *applied == Some(key) {
        return;
    }
    *applied = Some(key);
    if let Some(mut m) = mats.get_mut(h) {
        m.extension.params.look.x = key.0 as f32 / 32.0;
        m.extension.params.look.y = key.1 as f32 / 32.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The paint's WGSL is valid: its functions with a fragment that calls
    /// them (a slip would leave every car undrawn).
    #[test]
    fn the_car_paint_shader_parses_and_validates() {
        let src = format!(
            "{}
@fragment fn main(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {{
    var acc = vec3(0.0);
    for (var k = 0u; k < 8u; k++) {{
        let s = car_surface(vec3(0.4, 0.1, 0.1), k, k % 6u, 0.3, 0.5);
        acc += s.base * s.rough + vec3(s.metal + s.refl + s.coat + s.coat_rough);
    }}
    acc += neon_env(normalize(vec3(1.0, 0.1, 0.3)), p.xyz);
    return vec4(acc, 1.0);
}}",
            CAR_FN_SRC.replace("#{MATERIAL_BIND_GROUP}", "2")
        );
        let module = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).expect("valid");
    }
}
