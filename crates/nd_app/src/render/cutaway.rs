//! [chase-cam] Cutting away what stands between the chase camera and what
//! it follows: buildings, decks, trees, lamp posts.
//!
//! Buildings are batched per chunk (one mesh per surface per 128 m cell),
//! so a single building cannot be hidden on its own. Instead the scenery
//! meshes whose bounds reach the line of sight wear, while the chase is on,
//! a cutting copy of their own material: the same `StandardMaterial` with a
//! small extension that discards every fragment inside a cone from the eye
//! to the subject (narrow at the eye, about the subject's size at its end,
//! stopping short of it), above a floor line from the ground under the eye
//! to the subject's floor, so the road it drives on stays. The cone's edge
//! dithers over a metre (TAA smooths it into a fade). The building window
//! glow already has its own material (`lights::WindowMaterial`) and cuts
//! the same way in its own shader. The camera sets the cone each frame
//! (`camera::CameraCut`, from `chase::Cut`).
//!
//! Only the main camera's views are cut: the shader compares the view's
//! position with the cut's eye, so the sun's shadow maps (buildings keep
//! their shadows) and the junction editor's camera see everything.
//! Outside the normal view (heat vision, the focus and ghost views) nothing
//! is cut and the originals are back for the material swapper.

use bevy::asset::uuid_handle;
use bevy::camera::primitives::Aabb;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::{Shader, ShaderRef};
use std::collections::HashMap;

use super::lights::WindowMaterial;
use super::swap::{Ghost, SwapOriginal, View};
use super::{BuildingTag, Lighting, Materials, Overlays, ParkTag, RenderStats, RoadsTag, ScatterTag, VacantTag};
use crate::chase::{Cut, CUT_FLOOR, CUT_SOFT};

/// The cut as the shaders take it.
#[derive(ShaderType, Clone, Copy, Debug, Default, PartialEq, Reflect)]
pub(super) struct CutParams {
    /// xyz: the eye; w: 1 when cutting.
    pub eye: Vec4,
    /// xyz: the subject's middle; w: how far short of it the cut stops.
    /// (Not `target`: a reserved word in WGSL.)
    pub aim: Vec4,
    /// Radius at the eye and at the target; floor under the eye and
    /// under the target.
    pub shape: Vec4,
}

impl CutParams {
    pub(super) fn of(c: &Cut) -> Self {
        if !c.on {
            return Self::default();
        }
        Self { eye: c.eye.extend(1.0), aim: c.target.extend(c.margin), shape: Vec4::new(c.r_eye, c.r_target, c.ground_eye, c.ground_target) }
    }
}

/// The cut test in WGSL, its uniform at `binding` of the material group.
/// The same sums as `chase::Cut::depth`.
pub(super) fn cut_wgsl(binding: u32) -> String {
    format!(
        r#"
struct CutParams {{
    eye: vec4<f32>,
    aim: vec4<f32>,
    shape: vec4<f32>,
}}
@group(#{{MATERIAL_BIND_GROUP}}) @binding({binding}) var<uniform> cut_params: CutParams;

// Interleaved gradient noise: the dither of the cut's soft edge.
fn cut_noise(p: vec2<f32>) -> f32 {{
    return fract(52.9829189 * fract(dot(p, vec2(0.06711056, 0.00583715))));
}}

// Does the chase camera cut this fragment away? `view_pos` is the
// rendering view's eye: only the main camera's own views cut.
fn cut_away(world: vec3<f32>, frag: vec2<f32>, view_pos: vec3<f32>) -> bool {{
    if cut_params.eye.w < 0.5 {{
        return false;
    }}
    if distance(view_pos, cut_params.eye.xyz) > 1.0 {{
        return false;
    }}
    let axis = cut_params.aim.xyz - cut_params.eye.xyz;
    let len = length(axis);
    if len < 0.01 {{
        return false;
    }}
    let d = axis / len;
    let rel = world - cut_params.eye.xyz;
    let s = dot(rel, d);
    let end = len - cut_params.aim.w;
    if s <= 0.0 || s >= end {{
        return false;
    }}
    let f = s / len;
    if world.y < mix(cut_params.shape.z, cut_params.shape.w, f) + {floor:.3} {{
        return false;
    }}
    let r = mix(cut_params.shape.x, cut_params.shape.y, f);
    let edge = min(r - length(rel - d * s), end - s);
    if edge <= 0.0 {{
        return false;
    }}
    return clamp(edge / {soft:.3}, 0.0, 1.0) > cut_noise(frag);
}}
"#,
        floor = CUT_FLOOR,
        soft = CUT_SOFT,
    )
}

const CUT_SHADER: Handle<Shader> = uuid_handle!("3c5e1b7a-8d24-4f19-a6c2-7e9b0d41f583");
const CUT_PREPASS_SHADER: Handle<Shader> = uuid_handle!("b8d27f04-1e6a-4c93-8f5d-2a7c6e0b9d14");

/// The main pass: the standard material's own lighting, after the cut.
const CUT_MAIN_SRC: &str = r#"
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
    mesh_view_bindings::view,
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    if cut_away(in.world_position.xyz, in.position.xy, view.world_position) {
        discard;
    }
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
    var out: FragmentOutput;
    if (pbr_input.material.flags & STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        out.color = apply_pbr_lighting(pbr_input);
    } else {
        out.color = pbr_input.material.base_color;
    }
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
"#;

/// The prepass (depth, normals, motion for SSAO and TAA): Bevy's own
/// standard-material prepass, after the cut, or a cut wall would still
/// hide the car in the depth buffer.
const CUT_PREPASS_SRC: &str = r#"
#import bevy_pbr::{
    pbr_prepass_functions,
    pbr_bindings,
    pbr_types,
    pbr_functions,
    pbr_functions::SampleBias,
    prepass_io,
    mesh_bindings::mesh,
    mesh_view_bindings::view,
}
#import bevy_render::bindless::{bindless_samplers_filtering, bindless_textures_2d}
#ifdef BINDLESS
#import bevy_pbr::pbr_bindings::material_indices
#endif

#ifdef PREPASS_FRAGMENT
@fragment
fn fragment(in: prepass_io::VertexOutput, @builtin(front_facing) is_front: bool) -> prepass_io::FragmentOutput {
    if cut_away(in.world_position.xyz, in.position.xy, view.world_position) {
        discard;
    }
#ifdef BINDLESS
    let slot = mesh[in.instance_index].material_and_lightmap_bind_group_slot & 0xffffu;
    let flags = pbr_bindings::material_array[material_indices[slot].material].flags;
    let uv_transform = pbr_bindings::material_array[material_indices[slot].material].uv_transform;
#else
    let flags = pbr_bindings::material.flags;
    let uv_transform = pbr_bindings::material.uv_transform;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    pbr_functions::visibility_range_dither(in.position, in.visibility_range_dither);
#endif
    pbr_prepass_functions::prepass_alpha_discard(in);

    var out: prepass_io::FragmentOutput;
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.frag_depth = in.unclipped_depth;
#endif
#ifdef NORMAL_PREPASS
    if (flags & pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT) == 0u {
        let double_sided = (flags & pbr_types::STANDARD_MATERIAL_FLAGS_DOUBLE_SIDED_BIT) != 0u;
        let world_normal = pbr_functions::prepare_world_normal(in.world_normal, double_sided, is_front);
        var normal = world_normal;
#ifdef VERTEX_UVS
#ifdef VERTEX_TANGENTS
#ifdef STANDARD_MATERIAL_NORMAL_MAP
#ifdef STANDARD_MATERIAL_NORMAL_MAP_UV_B
        let uv = (uv_transform * vec3(in.uv_b, 1.0)).xy;
#else
        let uv = (uv_transform * vec3(in.uv, 1.0)).xy;
#endif
        var bias: SampleBias;
        bias.mip_bias = view.mip_bias;
        let Nt = textureSampleBias(
#ifdef BINDLESS
            bindless_textures_2d[material_indices[slot].normal_map_texture],
            bindless_samplers_filtering[material_indices[slot].normal_map_sampler],
#else
            pbr_bindings::normal_map_texture,
            pbr_bindings::normal_map_sampler,
#endif
            uv,
            bias.mip_bias,
        ).rgb;
        let TBN = pbr_functions::calculate_tbn_mikktspace(normal, in.world_tangent);
        normal = pbr_functions::apply_normal_mapping(flags, TBN, double_sided, is_front, Nt);
#endif
#endif
#endif
        out.normal = vec4(normal * 0.5 + vec3(0.5), 1.0);
    } else {
        out.normal = vec4(in.world_normal * 0.5 + vec3(0.5), 1.0);
    }
#endif
#ifdef MOTION_VECTOR_PREPASS
    out.motion_vector = pbr_prepass_functions::calculate_motion_vector(in.world_position, in.previous_world_position);
#endif
    return out;
}
#else
@fragment
fn fragment(in: prepass_io::VertexOutput) {
    if cut_away(in.world_position.xyz, in.position.xy, view.world_position) {
        discard;
    }
    pbr_prepass_functions::prepass_alpha_discard(in);
}
#endif
"#;

/// A scenery material with the cut.
pub(super) type CutMaterial = ExtendedMaterial<StandardMaterial, CutAway>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub(super) struct CutAway {
    #[uniform(100)]
    pub cut: CutParams,
}

impl MaterialExtension for CutAway {
    fn fragment_shader() -> ShaderRef {
        CUT_SHADER.into()
    }
    fn prepass_fragment_shader() -> ShaderRef {
        CUT_PREPASS_SHADER.into()
    }
}

pub(super) fn add_shader(mut shaders: ResMut<Assets<Shader>>) {
    let common = cut_wgsl(100);
    let _ = shaders.insert(CUT_SHADER.id(), Shader::from_wgsl(format!("{CUT_MAIN_SRC}{common}"), "nd_app/cutaway.wgsl"));
    let _ = shaders.insert(CUT_PREPASS_SHADER.id(), Shader::from_wgsl(format!("{CUT_PREPASS_SRC}{common}"), "nd_app/cutaway_prepass.wgsl"));
}

/// A scenery mesh wearing a cutting material: the material it wears
/// otherwise.
#[derive(Component)]
pub(super) struct CutOriginal(Handle<StandardMaterial>);

#[derive(Resource, Default)]
pub(super) struct CutState {
    /// Cutting copies by original material.
    cuts: HashMap<AssetId<StandardMaterial>, (Handle<StandardMaterial>, Handle<CutMaterial>)>,
    /// What the window glow was last given.
    windows: CutParams,
}

/// Scenery that can stand between the camera and the subject: buildings
/// (and their cranes), streets (decks, lamps, poles, street trees), trees
/// and park and vacant-lot dressing. Not the terrain (the camera keeps
/// above it) nor vehicles and people.
type Scenery = Or<(With<BuildingTag>, With<RoadsTag>, With<ScatterTag>, With<ParkTag>, With<VacantTag>)>;

/// Beyond the reach that picks a mesh, how much further it may drift
/// before it gets its own material back (so meshes at the edge do not
/// swap every frame).
const KEEP: f32 = 15.0;

/// Put the cutting copies on the scenery meshes that reach the line of
/// sight, and the originals back on the rest; keep the copies' cone (and
/// their look, which daylight and the weather change on the originals)
/// up to date. Runs after the material swapper.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply_cutaway(
    mut commands: Commands,
    cut: Option<Res<crate::camera::CameraCut>>,
    lighting: Res<Lighting>,
    overlays: Res<Overlays>,
    tools: Option<Res<crate::tools::ToolState>>,
    mats: Res<Materials>,
    mut state: ResMut<CutState>,
    mut stats: ResMut<RenderStats>,
    std_mats: Res<Assets<StandardMaterial>>,
    mut cut_mats: ResMut<Assets<CutMaterial>>,
    mut win_mats: ResMut<Assets<WindowMaterial>>,
    plain: Query<(Entity, &MeshMaterial3d<StandardMaterial>, &Aabb, &GlobalTransform), (Scenery, Without<CutOriginal>, Without<SwapOriginal>)>,
    worn: Query<(Entity, &CutOriginal, &Aabb, &GlobalTransform), Scenery>,
) {
    let normal = View::wanted(&overlays, &lighting, tools.as_deref().and_then(Ghost::wanted)) == View::Normal;
    let c = cut.map(|c| c.0).filter(|c| c.on && normal).unwrap_or_default();
    let params = CutParams::of(&c);
    if state.windows != params {
        state.windows = params;
        if let Some(mut m) = win_mats.get_mut(&mats.windows) {
            m.extension.cut = params;
        }
    }
    let reach = |aabb: &Aabb, gt: &GlobalTransform| (gt.transform_point(Vec3::from(aabb.center)), Vec3::from(aabb.half_extents).length());
    let mut wearing = 0;
    for (e, orig, aabb, gt) in &worn {
        let (center, r) = reach(aabb, gt);
        if c.touches(center, r + KEEP) {
            wearing += 1;
            continue;
        }
        commands.entity(e).try_remove::<(MeshMaterial3d<CutMaterial>, CutOriginal)>().try_insert(MeshMaterial3d(orig.0.clone()));
    }
    if !c.on {
        stats.cut_meshes = 0;
        return;
    }
    for (e, mat, aabb, gt) in &plain {
        let (center, r) = reach(aabb, gt);
        if !c.touches(center, r) {
            continue;
        }
        // Only opaque surfaces: the see-through ones (overlays) stay.
        let Some(o) = std_mats.get(&mat.0).filter(|o| matches!(o.alpha_mode, AlphaMode::Opaque)) else { continue };
        let handle = match state.cuts.get(&mat.0.id()) {
            Some((_, h)) => h.clone(),
            None => {
                let h = cut_mats.add(CutMaterial { base: o.clone(), extension: CutAway { cut: params } });
                state.cuts.insert(mat.0.id(), (mat.0.clone(), h.clone()));
                h
            }
        };
        commands.entity(e).try_remove::<MeshMaterial3d<StandardMaterial>>().try_insert((MeshMaterial3d(handle), CutOriginal(mat.0.clone())));
        wearing += 1;
    }
    stats.cut_meshes = wearing;
    // The cone moves every frame; the originals' look may have changed.
    for (orig, handle) in state.cuts.values() {
        let Some(o) = std_mats.get(orig) else { continue };
        if let Some(mut m) = cut_mats.get_mut(handle) {
            m.base = o.clone();
            m.extension.cut = params;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chase::{place, Nudge, SubjectKind};

    #[test]
    fn params_carry_the_cut() {
        assert_eq!(CutParams::of(&Cut::default()), CutParams::default());
        let p = SubjectKind::Car.profile();
        let pose = place(Vec2::new(3.0, 4.0), 1.0, 0.3, &p, &Nudge::default());
        let c = Cut::for_pose(&pose, &p, 0.5, 1.0);
        let k = CutParams::of(&c);
        assert_eq!(k.eye, pose.eye.extend(1.0));
        assert_eq!(k.aim.w, c.margin);
        assert_eq!(k.shape, Vec4::new(c.r_eye, c.r_target, 0.5, 1.0));
        // The shader's sums name the same constants as `Cut::depth`.
        let src = cut_wgsl(100);
        assert!(src.contains(&format!("{CUT_FLOOR:.3}")) && src.contains(&format!("{CUT_SOFT:.3}")));
        assert!(src.contains("@binding(100)"));
    }

    /// The WGSL is valid: the cut test on its own, called from a fragment
    /// (a reserved word as a field name once left every building's
    /// windows and every street undrawn).
    #[test]
    fn the_cut_shader_parses_and_validates() {
        let src = format!(
            "{}
@fragment fn main(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {{ if cut_away(p.xyz, p.xy, vec3(0.0)) {{ discard; }} return vec4(1.0); }}",
            cut_wgsl(100).replace("#{MATERIAL_BIND_GROUP}", "2")
        );
        let module = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).expect("valid");
    }

    /// Meshes along the line of sight wear the cutting copy while the chase
    /// is on, the rest keep their own; off, every mesh gets its own back.
    #[test]
    fn swaps_the_meshes_in_the_way_and_back() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_plugins(AssetPlugin::default());
        app.init_asset::<StandardMaterial>().init_asset::<CutMaterial>().init_asset::<WindowMaterial>().init_asset::<Image>();
        app.init_resource::<Lighting>().init_resource::<Overlays>().init_resource::<CutState>().init_resource::<RenderStats>().init_resource::<crate::camera::CameraCut>();
        let brick = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial::default());
        let overlay = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial { alpha_mode: AlphaMode::Blend, ..default() });
        let windows = {
            let lit = app.world_mut().resource_mut::<Assets<Image>>().add(Image::default());
            app.world_mut().resource_mut::<Assets<WindowMaterial>>().add(WindowMaterial { base: StandardMaterial::default(), extension: super::super::lights::WindowLights { lit, clock: Vec4::ZERO, cut: CutParams::default() } })
        };
        app.insert_resource(Materials::for_test(overlay.clone(), windows.clone()));
        app.add_systems(Update, apply_cutaway);
        let p = SubjectKind::Car.profile();
        let pose = place(Vec2::ZERO, 0.0, 0.0, &p, &Nudge::default());
        let mid = pose.eye.lerp(pose.focus, 0.5);
        let block = |at: Vec3| (Aabb::from_min_max(at - Vec3::splat(5.0), at + Vec3::splat(5.0)), GlobalTransform::IDENTITY, BuildingTag);
        let near = app.world_mut().spawn((MeshMaterial3d(brick.clone()), block(mid))).id();
        let far = app.world_mut().spawn((MeshMaterial3d(brick.clone()), block(mid + Vec3::new(0.0, 0.0, 200.0)))).id();
        let glassy = app.world_mut().spawn((MeshMaterial3d(overlay.clone()), block(mid))).id();
        let car = app.world_mut().spawn((MeshMaterial3d(brick.clone()), Aabb::from_min_max(mid - Vec3::ONE, mid + Vec3::ONE), GlobalTransform::IDENTITY)).id();
        app.world_mut().resource_mut::<crate::camera::CameraCut>().0 = Cut::for_pose(&pose, &p, 0.0, 0.0);
        app.update();
        let w = app.world();
        assert!(w.get::<MeshMaterial3d<CutMaterial>>(near).is_some());
        assert!(w.get::<MeshMaterial3d<StandardMaterial>>(near).is_none());
        assert!(w.get::<MeshMaterial3d<StandardMaterial>>(far).is_some());
        assert!(w.get::<MeshMaterial3d<StandardMaterial>>(glassy).is_some());
        assert!(w.get::<MeshMaterial3d<StandardMaterial>>(car).is_some(), "vehicles are not scenery");
        assert_eq!(w.resource::<RenderStats>().cut_meshes, 1);
        let wm = w.resource::<Assets<WindowMaterial>>().get(&windows).unwrap();
        assert_eq!(wm.extension.cut.eye.w, 1.0);
        // The chase ends: the original is back, and the windows stop cutting.
        app.world_mut().resource_mut::<crate::camera::CameraCut>().0.on = false;
        app.update();
        let w = app.world();
        assert_eq!(w.get::<MeshMaterial3d<StandardMaterial>>(near).map(|m| m.0.clone()), Some(brick.clone()));
        assert!(w.get::<MeshMaterial3d<CutMaterial>>(near).is_none());
        assert!(w.get::<CutOriginal>(near).is_none());
        assert_eq!(w.resource::<Assets<WindowMaterial>>().get(&windows).unwrap().extension.cut, CutParams::default());
        // Heat vision: nothing is cut even with the chase on.
        app.world_mut().resource_mut::<crate::camera::CameraCut>().0.on = true;
        app.world_mut().resource_mut::<Lighting>().heat = true;
        app.update();
        assert!(app.world().get::<MeshMaterial3d<StandardMaterial>>(near).is_some());
    }
}
