//! Window lights that follow the people inside.
//!
//! Buildings are batched per chunk, so a building's windows cannot be
//! switched by regenerating its mesh. Instead every window light carries its
//! building's slot and its own rank in the vertex UVs (the glow material has
//! no texture, so they are free), and the chunk Glow meshes are drawn with a
//! small extension of the glow material that looks the building's lit share
//! up in a 256 x 256 one-byte texture and discards windows ranked above it,
//! uncovering the dark glass laid behind each one. Updating the lights is a
//! 64 KB texture write a few times per sim hour; no mesh is touched.

use bevy::asset::{uuid_handle, RenderAssetUsages};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, TextureDimension, TextureFormat};
use bevy::shader::{Shader, ShaderRef};
use nd_procgen::lights::{lit_share, Occupancy};
use nd_procgen::{Geometry, Surface, WINDOW_LIGHT_MARK};
use nd_sim::BuildingId;
use slotmap::Key;

use super::Materials;
use crate::sim::Sim;

/// Side of the square lit-share texture: one texel per building slot.
pub(super) const LIGHT_TEX: u32 = 256;
/// Sim minutes between light updates (four times an hour).
const UPDATE_EVERY_TICKS: u64 = 15;

pub(super) const WINDOW_SHADER: Handle<Shader> = uuid_handle!("6f0b8f9e-5d1c-4a87-9e43-1f7c2a9d3b51");

const WINDOW_SHADER_SRC: &str = r#"
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::main_pass_post_lighting_processing,
}

@group(#{MATERIAL_BIND_GROUP}) @binding(100) var lit_texture: texture_2d<f32>;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
#ifdef VERTEX_UVS_A
    // uv.x: the building's slot, or -1 for glow that is always on; uv.y:
    // this window's rank. A window lights when its rank is below the
    // building's lit share.
    if in.uv.x >= 0.0 {
        let slot = u32(in.uv.x + 0.5);
        let dims = textureDimensions(lit_texture);
        let share = textureLoad(lit_texture, vec2<u32>(slot % dims.x, min(slot / dims.x, dims.y - 1u)), 0).r;
        if in.uv.y >= share {
            discard;
        }
    }
#endif
    let pbr_input = pbr_input_from_standard_material(in, is_front);
    var out: FragmentOutput;
    out.color = main_pass_post_lighting_processing(pbr_input, pbr_input.material.base_color);
    return out;
}
"#;

/// The building glow material: the shared unlit glow plus the lit-share
/// lookup.
pub(super) type WindowMaterial = ExtendedMaterial<StandardMaterial, WindowLights>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub(super) struct WindowLights {
    #[texture(100)]
    pub lit: Handle<Image>,
}

impl MaterialExtension for WindowLights {
    fn fragment_shader() -> ShaderRef {
        WINDOW_SHADER.into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

pub(super) fn add_shader(mut shaders: ResMut<Assets<Shader>>) {
    let _ = shaders.insert(WINDOW_SHADER.id(), Shader::from_wgsl(WINDOW_SHADER_SRC, "nd_app/window_lights.wgsl"));
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
/// back; every other glow vertex gets `uv.x = -1`, always on.
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
        } else {
            *uv = [-1.0, 0.0];
        }
    }
}

/// Recount who is in every building and rewrite the lit-share texture,
/// every `UPDATE_EVERY_TICKS` sim minutes (and after a load).
pub(super) fn update_window_lights(
    sim: Res<Sim>,
    mats: Res<Materials>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<WindowMaterial>>,
    mut last: Local<Option<(u64, usize)>>,
) {
    let w = &sim.world;
    let key = (w.clock.tick / UPDATE_EVERY_TICKS, w.buildings.len());
    if *last == Some(key) && !sim.full_rebuild {
        return;
    }
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
        data[slot as usize] = (lit_share(&o) * 255.0).round() as u8;
    }
    // Touch the material so its bind group picks up the new texture.
    let _ = materials.get_mut(&mats.windows);
}
