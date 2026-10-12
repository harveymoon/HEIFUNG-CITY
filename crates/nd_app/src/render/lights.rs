//! Window lights that follow the people inside.
//!
//! Buildings are batched per chunk, so a building's windows cannot be
//! switched by regenerating its mesh. Instead every window light carries its
//! building's slot and its own rank in the vertex UVs (the glow material has
//! no texture, so they are free), and the chunk Glow meshes are drawn with a
//! small extension of the glow material that looks the building's lit share
//! up in a 256 x 256 one-byte texture and discards windows ranked above it,
//! uncovering the dark glass laid behind each one. Updating the lights is a
//! 64 KB texture write every few sim minutes; no mesh is touched.
//!
//! Nothing switches in step. Each building takes up its new share at its
//! own minute and eases towards it a few windows at a time, and in the
//! shader each window's threshold wanders a little on its own slow clock
//! (someone going to the kitchen, a lamp switched off in the next room),
//! so a block twinkles window by window instead of flipping at once.

use bevy::asset::{uuid_handle, RenderAssetUsages};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::shader::{Shader, ShaderRef};
use nd_procgen::lights::{lit_share, Occupancy};
use nd_procgen::{Geometry, Surface, NEON_MARK, NIGHT_MARK, WINDOW_LIGHT_MARK};
use nd_sim::BuildingId;
use slotmap::Key;

use super::Materials;
use crate::sim::Sim;

/// Side of the square lit-share texture: one texel per building slot.
pub(super) const LIGHT_TEX: u32 = 256;
/// Sim minutes between light updates. Each building picks up its target
/// only on its own turn, one in [`STAGGER`] updates, so a building sees a
/// new share every quarter hour but the city's buildings do not all see
/// it on the same minute.
const UPDATE_EVERY_TICKS: u64 = 3;
const STAGGER: u64 = 5;

pub(super) const WINDOW_SHADER: Handle<Shader> = uuid_handle!("6f0b8f9e-5d1c-4a87-9e43-1f7c2a9d3b51");
/// [look] The same windows in the depth prepass (SSAO, TAA): an unlit
/// window must leave no depth behind either, or it would hide the dark
/// glass laid behind it.
pub(super) const WINDOW_PREPASS_SHADER: Handle<Shader> = uuid_handle!("9a1c3e57-24b8-4d6f-8e0a-5b7c9d1f2e34");

/// Bindings and the lit test, shared by the main pass and the prepass.
const WINDOW_COMMON_SRC: &str = r#"
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var lit_texture: texture_2d<f32>;
// x: sim minutes (wrapped), for each window's own wandering; y: how much
// night it is (0 by day, 1 at night), for landmark neon and night beams.
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> lights_clock: vec4<f32>;

fn window_hash(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2(127.1, 311.7))) * 43758.5453);
}

// uv.x: the building's slot, or -1 for glow that is always on, -2 for a
// landmark's neon (dimmed by day), -3 for glow lit only after dark; uv.y:
// this window's rank. A window lights when its rank is below the
// building's lit share.
fn window_dark(uv: vec2<f32>) -> bool {
    if uv.x < 0.0 {
        return uv.x < -2.5 && lights_clock.y < 0.5;
    }
    let slot = u32(uv.x + 0.5);
    let dims = textureDimensions(lit_texture);
    let share = textureLoad(lit_texture, vec2<u32>(slot % dims.x, min(slot / dims.x, dims.y - 1u)), 0).r;
    if share <= 0.004 {
        return true;
    }
    // Each window (its building slot and rank) re-rolls a small offset
    // to its threshold every 40..180 sim minutes, at its own phase: the
    // building's share holds on average while single rooms come and go.
    if share < 0.996 {
        let key = vec2(uv.x * 0.618 + 3.1, uv.y * 977.0);
        let period = 40.0 + 140.0 * window_hash(key);
        let epoch = floor(lights_clock.x / period + window_hash(key.yx + 1.7));
        let jitter = (window_hash(key + vec2(epoch * 0.731, epoch * 1.37)) - 0.5) * 0.26;
        if uv.y + jitter >= share {
            return true;
        }
    }
    return false;
}
"#;

const WINDOW_SHADER_SRC: &str = r#"
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::main_pass_post_lighting_processing,
}
"#;

const WINDOW_MAIN_SRC: &str = r#"
@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
#ifdef VERTEX_UVS_A
    if window_dark(in.uv) {
        discard;
    }
    // A landmark's neon, drawn bright to carry at night, dims by day to
    // about the signs' glow.
    if in.uv.x < -1.5 && in.uv.x > -2.5 {
        let k = mix(0.4, 1.0, lights_clock.y);
        pbr_input.material.base_color = vec4(pbr_input.material.base_color.rgb * k, pbr_input.material.base_color.a);
    }
#endif
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, pbr_input.material.base_color);
    return out;
}
"#;

const WINDOW_PREPASS_SRC: &str = r#"
#import bevy_pbr::{
    prepass_io::{VertexOutput, FragmentOutput},
    pbr_prepass_functions,
}
"#;

const WINDOW_PREPASS_MAIN_SRC: &str = r#"
#ifdef PREPASS_FRAGMENT
@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
#ifdef VERTEX_UVS_A
    if window_dark(in.uv) {
        discard;
    }
#endif
    var out: FragmentOutput;
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.frag_depth = in.unclipped_depth;
#endif
#ifdef NORMAL_PREPASS
    out.normal = vec4(in.world_normal * 0.5 + vec3(0.5), 1.0);
#endif
#ifdef MOTION_VECTOR_PREPASS
    out.motion_vector = pbr_prepass_functions::calculate_motion_vector(in.world_position, in.previous_world_position);
#endif
    return out;
}
#else
@fragment
fn fragment(in: VertexOutput) {
#ifdef VERTEX_UVS_A
    if window_dark(in.uv) {
        discard;
    }
#endif
}
#endif
"#;

/// The building glow material: the shared unlit glow plus the lit-share
/// lookup.
pub(super) type WindowMaterial = ExtendedMaterial<StandardMaterial, WindowLights>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub(super) struct WindowLights {
    #[texture(100)]
    pub lit: Handle<Image>,
    /// x: sim minutes, wrapped to keep f32 precision; y: night (0..1).
    #[uniform(101)]
    pub clock: Vec4,
}

impl MaterialExtension for WindowLights {
    fn fragment_shader() -> ShaderRef {
        WINDOW_SHADER.into()
    }
    fn prepass_fragment_shader() -> ShaderRef {
        WINDOW_PREPASS_SHADER.into()
    }
    fn enable_shadows() -> bool {
        false
    }
}

pub(super) fn add_shader(mut shaders: ResMut<Assets<Shader>>) {
    let main = format!("{WINDOW_SHADER_SRC}{WINDOW_COMMON_SRC}{WINDOW_MAIN_SRC}");
    let _ = shaders.insert(WINDOW_SHADER.id(), Shader::from_wgsl(main, "nd_app/window_lights.wgsl"));
    let prepass = format!("{WINDOW_PREPASS_SRC}{WINDOW_COMMON_SRC}{WINDOW_PREPASS_MAIN_SRC}");
    let _ = shaders.insert(WINDOW_PREPASS_SHADER.id(), Shader::from_wgsl(prepass, "nd_app/window_lights_prepass.wgsl"));
}

/// The lit-share texture, all on until the first update.
pub(super) fn lit_image() -> Image {
    let n = (LIGHT_TEX * LIGHT_TEX) as usize;
    Image::new(
        Extent3d { width: LIGHT_TEX, height: LIGHT_TEX, depth_or_array_layers: 1 },
        TextureDimension::D2,
        vec![255u8; n],
        TextureFormat::R8Unorm,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
}

/// A building's texel in the lit-share texture: its slotmap index, which
/// stays put while the building stands. `None` past the texture.
pub(super) fn slot_of(id: BuildingId) -> Option<u32> {
    let idx = (id.data().as_ffi() & 0xffff_ffff) as u32;
    (idx < LIGHT_TEX * LIGHT_TEX).then_some(idx)
}

/// Point one building's window lights at its slot: marked glow vertices
/// (see `parts::window_light`) get `uv = (slot, rank)` and their alpha
/// back; a landmark's neon (`NEON_MARK`) gets `uv.x = -2`, dimmed by day,
/// glow lit only after dark (`NIGHT_MARK`) `-3`; every other glow vertex
/// gets `uv.x = -1`, always on.
pub(super) fn tag_windows(g: &mut Geometry, slot: Option<u32>) {
    let m = g.part(Surface::Glow);
    for (c, uv) in m.colors.iter_mut().zip(m.uvs.iter_mut()) {
        if c[3] >= WINDOW_LIGHT_MARK {
            let rank = (c[3] - WINDOW_LIGHT_MARK).clamp(0.0, 0.999);
            c[3] = 1.0;
            *uv = match slot {
                Some(s) => [s as f32, rank],
                None => [-1.0, 0.0],
            };
        } else if c[3] >= NIGHT_MARK - 0.01 {
            c[3] = 1.0;
            *uv = [-3.0, 0.0];
        } else if c[3] >= NEON_MARK - 0.01 {
            c[3] = 1.0;
            *uv = [-2.0, 0.0];
        } else {
            *uv = [-1.0, 0.0];
        }
    }
}

/// A stable per-building number for its turn and its pace.
fn building_hash(slot: u32) -> u64 {
    let mut h = (slot as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h ^= h >> 31;
    h.wrapping_mul(0xBF58_476D_1CE4_E5B9) >> 17
}

/// Recount who is in every building every `UPDATE_EVERY_TICKS` sim minutes
/// (and after a load) and move each building's texel towards its new share
/// on that building's own turn, a few windows at a time.
pub(super) fn update_window_lights(
    sim: Res<Sim>,
    look: Res<super::sky::SkyLook>,
    mats: Res<Materials>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<WindowMaterial>>,
    mut last: Local<Option<(u64, usize)>>,
    mut minute: Local<u64>,
) {
    let w = &sim.world;
    // The windows' own clock, at most once a sim minute (a material
    // write rebuilds its bind group).
    let night = ((1.0 - look.daylight) * 32.0).round() / 32.0;
    let night_moved = materials.get(&mats.windows).is_some_and(|m| m.extension.clock.y != night);
    if *minute != w.clock.tick || sim.full_rebuild || night_moved {
        *minute = w.clock.tick;
        if let Some(mut m) = materials.get_mut(&mats.windows) {
            m.extension.clock.x = (w.clock.tick % 1_000_000) as f32 + sim.frac();
            m.extension.clock.y = night;
        }
    }
    let step = w.clock.tick / UPDATE_EVERY_TICKS;
    let key = (step, w.buildings.len());
    if *last == Some(key) && !sim.full_rebuild {
        return;
    }
    // [pt-visual] A jump in time (a long remote `step`, a fast-forward that
    // skips frames) or back in it (a new city from `generate_city` or New
    // game, whose clock starts again) snaps every building to its share:
    // the lights eased one turn per frame, so after a big step a new city's
    // towers stayed fully lit at night until a save and reload.
    let jumped = last.is_some_and(|(s, _)| step < s || step > s + STAGGER);
    let first = last.is_none() || sim.full_rebuild || jumped;
    *last = Some(key);
    let n = (LIGHT_TEX * LIGHT_TEX) as usize;
    // awake at home, asleep at home, staff in
    let mut counts: Vec<[u16; 3]> = vec![[0; 3]; n];
    for c in w.citizens.values() {
        let Some(b) = c.inside else { continue };
        let Some(slot) = slot_of(b) else { continue };
        let k = if c.home == Some(b) {
            if c.asleep { 1 } else { 0 }
        } else if c.work == Some(b) {
            2
        } else {
            continue;
        };
        let e = &mut counts[slot as usize][k];
        *e = e.saturating_add(1);
    }
    let hour = w.clock.day_fraction() * 24.0;
    let Some(mut img) = images.get_mut(&mats.lit) else { return };
    let Some(data) = img.data.as_mut() else { return };
    for (id, b) in w.buildings.iter() {
        let Some(slot) = slot_of(id) else { continue };
        let bh = building_hash(slot);
        // Not this building's turn: it keeps what it shows.
        if !first && (step + bh) % STAGGER != 0 {
            continue;
        }
        let [awake, asleep, staff] = counts[slot as usize];
        let o = Occupancy {
            homes: b.homes,
            jobs: b.jobs,
            awake_home: awake,
            asleep_home: asleep,
            staff_in: staff,
            visitors: b.visitors,
            open: b.opening_hours().contains(hour as u8),
            hour,
            powered: b.powered,
        };
        // [fix/buildings-ui] A building site or one coming down has no
        // power to the floors: the crew in it lights no windows, and the
        // lights come on only once it is finished.
        let dark = !b.operating();
        let target = if dark { 0 } else { (lit_share(&o) * 255.0).round() as u8 };
        let cur = &mut data[slot as usize];
        // A power cut is instant; otherwise ease over, at this building's
        // own pace (12..44 of 255 a turn), so lights come on and go off a
        // few windows at a time.
        if first || dark || (target == 0 && b.powered < 0.05) {
            *cur = target;
        } else {
            let pace = 12 + (bh >> 8) % 33;
            let d = target as i32 - *cur as i32;
            *cur = (*cur as i32 + d.clamp(-(pace as i32), pace as i32)) as u8;
        }
    }
    // Touch the material so its bind group picks up the new texture.
    let _ = materials.get_mut(&mats.windows);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [w1-render] Window lights point at the building's texel; a
    /// landmark's neon and its night-only glow get their own codes, the
    /// rest of the glow stays always on; every alpha comes back to 1.
    #[test]
    fn glow_marks_become_codes() {
        let mut g = Geometry::default();
        let m = g.part(Surface::Glow);
        for a in [1.0, NEON_MARK, NIGHT_MARK, WINDOW_LIGHT_MARK + 0.25] {
            m.positions.push([0.0; 3]);
            m.colors.push([1.0, 1.0, 1.0, a]);
            m.uvs.push([0.0; 2]);
        }
        tag_windows(&mut g, Some(9));
        let m = g.get(Surface::Glow);
        assert_eq!(m.uvs, vec![[-1.0, 0.0], [-2.0, 0.0], [-3.0, 0.0], [9.0, 0.25]]);
        assert!(m.colors.iter().all(|c| c[3] == 1.0));
    }
}
