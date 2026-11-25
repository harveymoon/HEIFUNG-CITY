//! [r17-people] Near figures: within a few dozen metres of the camera a
//! citizen (or a visitor) is drawn as the detailed figure of
//! `nd_procgen::people_near` instead of the low-poly one, and walks.
//!
//! How it fits the far figures:
//! - The far figure stays as it is (its entity, its pose, its material,
//!   the selection and spotlight, heat vision and the focus views). The near
//!   figure is a separate entity from a small pool, lent to whoever is close
//!   ([`NearPeople`]): it copies the far entity's place, turn and visibility
//!   every frame, after the lift onto decks and bridges.
//! - The swap is a cross-fade: the near entity's `VisibilityRange` ends
//!   where the far one's (given only while a near one is lent) begins, and
//!   Bevy dithers the two across that band. The band is [`NEAR_BAND`]
//!   (orbit) or [`STREET_BAND`] (walk, drive, follow: everyone close is
//!   near), and a figure is lent a few metres before the band and kept a
//!   few metres past it, so it never flickers between the two.
//! - Looks: built from the far figure's own `PersonKey` (same skin, hair,
//!   colours and night trim) plus its extras (carry, umbrella in the rain,
//!   hood up, an older stoop), cached per look (`NEAR_MESH_CAP`) and shared
//!   by everyone who looks alike; at most [`BUILDS_PER_FRAME`] are built a
//!   frame (the rest stay far until their turn).
//! - The walk is in the vertex shader below: one shared mesh per look, each
//!   figure's stride phase and swing in its `MeshTag`, worked out here from
//!   how far the figure moved (so the feet keep pace with the ground, up to
//!   a brisk cadence at high game speeds).
//! - Off for the plain figures (`ND_PLAIN_PEOPLE=1`), the stand-in views
//!   (heat, focus, ghost) and `ND_NEAR_PEOPLE=0` (for comparing frames).

use super::*;
use bevy::asset::uuid_handle;
use bevy::camera::visibility::VisibilityRange;
use bevy::mesh::MeshTag;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::{Shader, ShaderRef};
use nd_procgen::people_near::{self as near, NearKey};

/// Orbit view: near figures fade out from 30 to 42 m.
pub(super) const NEAR_BAND: (f32, f32) = (30.0, 42.0);
/// Walk, drive and follow views: from 44 to 58 m.
pub(super) const STREET_BAND: (f32, f32) = (44.0, 58.0);
/// Lent this far before the band's end, kept this far past it.
const LEND_BEFORE: f32 = 3.0;
const KEEP_PAST: f32 = 10.0;
/// Near figures at most at once.
pub(super) const NEAR_CAP: usize = 360;
/// Near looks cached at most.
pub(super) const NEAR_MESH_CAP: usize = 640;
/// New near looks built a frame at most.
pub(super) const BUILDS_PER_FRAME: usize = 10;
/// How far the candidates reach (the street band's end and the keep).
pub(super) const CANDIDATE_REACH: f32 = STREET_BAND.1 + KEEP_PAST + 2.0;

pub(super) const RIG_SHADER: Handle<Shader> = uuid_handle!("6b1f2d0e-93c4-4a57-8e21-5d0c7a9e4f36");
pub(super) const RIG_PREPASS_SHADER: Handle<Shader> = uuid_handle!("0c4e8a71-2f5b-4d93-a6e8-91b7d3c5f024");

/// The rig shader: the main pass (`prepass` false) or the depth / normal /
/// motion-vector prepass, both posing the vertices the same way.
fn rig_shader_src_for(prepass: bool) -> String {
    let header = if prepass { RIG_PREPASS_HEADER } else { RIG_MAIN_HEADER };
    let vs = if prepass { RIG_PREPASS_VS } else { RIG_MAIN_VS };
    format!(
        "{header}{common}{vs}",
        common = format!(
            r#"
@group(#{{MATERIAL_BIND_GROUP}}) @binding(100) var<uniform> rig_params: vec4<f32>;

const TAU: f32 = 6.28318530718;
const PI: f32 = 3.14159265359;
const RIDE_HIP: vec2<f32> = vec2<f32>({hx}, {hy});
const RIDE_BB: vec2<f32> = vec2<f32>({bx}, {by});
const RIDE_CRANK: f32 = {crank};
const RIDE_ANKLE: vec2<f32> = vec2<f32>({ax}, {ay});
const RIDE_THIGH: f32 = {thigh};
const RIDE_SHIN: f32 = {shin};
const RIDE_REST: f32 = {rest};
const WALK_DIP: f32 = {dip};

fn unpack_pivot(v: u32) -> vec2<f32> {{
    return vec2<f32>(f32(v >> 8u) * 0.01 - 1.28, f32(v & 255u) * 0.01);
}}

fn rot2(p: vec2<f32>, pivot: vec2<f32>, a: f32) -> vec2<f32> {{
    let d = p - pivot;
    let s = sin(a);
    let c = cos(a);
    return pivot + vec2<f32>(d.x * c - d.y * s, d.x * s + d.y * c);
}}

// Thigh and shin angles of a rider's leg with the pedal at crank angle phi.
fn ride_leg(phi: f32) -> vec2<f32> {{
    let ankle = RIDE_BB + vec2<f32>(cos(phi), sin(phi)) * RIDE_CRANK + RIDE_ANKLE;
    let d = ankle - RIDE_HIP;
    let len = clamp(length(d), 0.05, RIDE_THIGH + RIDE_SHIN - 0.001);
    let base = atan2(d.y, d.x);
    let cos_a = clamp((RIDE_THIGH * RIDE_THIGH + len * len - RIDE_SHIN * RIDE_SHIN) / (2.0 * RIDE_THIGH * len), -1.0, 1.0);
    let thigh = base + acos(cos_a);
    let knee = RIDE_HIP + vec2<f32>(cos(thigh), sin(thigh)) * RIDE_THIGH;
    let s = ankle - knee;
    return vec2<f32>(thigh, atan2(s.y, s.x));
}}

// A vertex posed by its limb (uv_b: nd_procgen::people_near::rig_aux) and
// the figure's stride in the mesh tag (walk_tag).
struct Posed {{
    pos: vec3<f32>,
    nrm: vec3<f32>,
}}

fn pose(p_in: vec3<f32>, n_in: vec3<f32>, uv_b: vec2<f32>, instance_index: u32) -> Posed {{
    var pos = p_in;
    var nrm = n_in;
    // The limb this vertex belongs to and the joints it turns about
    // (nd_procgen::people_near::rig_aux), the stride in the mesh tag.
    let tag = mesh_functions::get_tag(instance_index);
    let a0 = u32(round(uv_b.x));
    let code = a0 & 31u;
    let parent = unpack_pivot(a0 >> 5u);
    let own = unpack_pivot(u32(round(uv_b.y)));
    let amp = f32((tag >> 16u) & 255u) / 255.0;
    let phi = f32(tag & 65535u) / 65536.0 * TAU;
    let s = sin(phi);
    let c = cos(phi);
    var a_own = 0.0;
    var a_par = 0.0;
    var bob = true;
    switch code {{
        case 1u, 2u: {{
            let sd = select(-1.0, 1.0, code == 1u);
            a_own = amp * 0.40 * s * sd;
        }}
        case 3u, 4u: {{
            let sd = select(-1.0, 1.0, code == 3u);
            a_own = -amp * (0.08 + 0.62 * max(c * sd, 0.0));
            a_par = amp * 0.40 * s * sd;
        }}
        case 5u, 6u: {{
            let sd = select(-1.0, 1.0, code == 5u);
            a_own = -amp * 0.32 * s * sd;
        }}
        case 7u, 8u: {{
            let sd = select(-1.0, 1.0, code == 7u);
            a_own = amp * (0.05 + 0.30 * max(-s * sd, 0.0));
            a_par = -amp * 0.32 * s * sd;
        }}
        case 10u, 11u, 12u, 13u: {{
            bob = false;
            let off = select(0.0, PI, code == 11u || code == 13u);
            let r0 = ride_leg(RIDE_REST + off);
            let r1 = ride_leg(phi + RIDE_REST + off);
            let dt = r1.x - r0.x;
            if (code == 10u || code == 11u) {{
                a_own = dt;
            }} else {{
                a_own = (r1.y - r0.y) - dt;
                a_par = dt;
            }}
        }}
        case 14u: {{
            bob = false;
            a_own = phi;
        }}
        default: {{}}
    }}
    if (code != 0u) {{
        var q = rot2(pos.xy, own, a_own);
        q = rot2(q, parent, a_par);
        pos = vec3<f32>(q, pos.z);
        let m = rot2(rot2(nrm.xy, vec2<f32>(0.0), a_own), vec2<f32>(0.0), a_par);
        nrm = vec3<f32>(m, nrm.z);
    }}
    if (bob) {{
        pos.y = pos.y - amp * WALK_DIP * (0.5 - 0.5 * cos(2.0 * phi));
    }}
    return Posed(pos, nrm);
}}

"#,
        hx = near::RIDE_HIP.x,
        hy = near::RIDE_HIP.y,
        bx = near::RIDE_BB.x,
        by = near::RIDE_BB.y,
        crank = near::RIDE_CRANK,
        ax = near::RIDE_ANKLE.x,
        ay = near::RIDE_ANKLE.y,
        thigh = near::RIDE_THIGH,
        shin = near::RIDE_SHIN,
        rest = flt(near::RIDE_REST),
        dip = near::WALK_DIP,
    
        )
    )
}

fn rig_shader_src() -> String {
    rig_shader_src_for(false)
}

const RIG_MAIN_HEADER: &str = r#"
#import bevy_pbr::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}
"#;

const RIG_PREPASS_HEADER: &str = r#"
#import bevy_pbr::{
    mesh_functions,
    prepass_io::{Vertex, VertexOutput},
    view_transformations::position_world_to_clip,
}
"#;

const RIG_MAIN_VS: &str = r#"
@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    var pos = vertex.position;
    var nrm = vec3<f32>(0.0, 1.0, 0.0);
#ifdef VERTEX_NORMALS
    nrm = vertex.normal;
#endif
#ifdef VERTEX_UVS_B
    let posed = pose(pos, nrm, vertex.uv_b, vertex.instance_index);
    pos = posed.pos;
    nrm = posed.nrm;
#endif
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(nrm, vertex.instance_index);
#endif
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(pos, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, vertex.tangent, vertex.instance_index);
#endif
#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(vertex.instance_index, world_from_local[3]);
#endif
    return out;
}
"#;

const RIG_PREPASS_VS: &str = r#"
@vertex
fn vertex(vertex: Vertex) -> VertexOutput {
    var out: VertexOutput;
    var pos = vertex.position;
    var nrm = vec3<f32>(0.0, 1.0, 0.0);
#ifdef NORMAL_PREPASS_OR_DEFERRED_PREPASS
#ifdef VERTEX_NORMALS
    nrm = vertex.normal;
#endif
#endif
#ifdef VERTEX_UVS_B
    let posed = pose(pos, nrm, vertex.uv_b, vertex.instance_index);
    pos = posed.pos;
    nrm = posed.nrm;
#endif
    let world_from_local = mesh_functions::get_world_from_local(vertex.instance_index);
    out.world_position = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(pos, 1.0));
    out.position = position_world_to_clip(out.world_position.xyz);
#ifdef UNCLIPPED_DEPTH_ORTHO_EMULATION
    out.unclipped_depth = out.position.z;
    out.position.z = min(out.position.z, 1.0);
#endif
#ifdef VERTEX_UVS_A
    out.uv = vertex.uv;
#endif
#ifdef VERTEX_UVS_B
    out.uv_b = vertex.uv_b;
#endif
#ifdef NORMAL_PREPASS_OR_DEFERRED_PREPASS
#ifdef VERTEX_NORMALS
    out.world_normal = mesh_functions::mesh_normal_local_to_world(nrm, vertex.instance_index);
#endif
#ifdef VERTEX_TANGENTS
    out.world_tangent = mesh_functions::mesh_tangent_local_to_world(world_from_local, vertex.tangent, vertex.instance_index);
#endif
#endif
#ifdef VERTEX_COLORS
    out.color = vertex.color;
#endif
#ifdef MOTION_VECTOR_PREPASS
    // The limbs' own motion is left out: the figure's move is what TAA needs.
    let prev_model = mesh_functions::get_previous_world_from_local(vertex.instance_index);
    out.previous_world_position = mesh_functions::mesh_position_local_to_world(prev_model, vec4<f32>(pos, 1.0));
#endif
#ifdef VERTEX_OUTPUT_INSTANCE_INDEX
    out.instance_index = vertex.instance_index;
#endif
#ifdef VISIBILITY_RANGE_DITHER
    out.visibility_range_dither = mesh_functions::get_visibility_range_dither_level(vertex.instance_index, world_from_local[3]);
#endif
    return out;
}
"#;

/// A float as WGSL wants it (always with a point).
fn flt(v: f32) -> String {
    let s = format!("{v}");
    if s.contains('.') { s } else { format!("{s}.0") }
}

pub(super) fn add_rig_shader(mut shaders: ResMut<Assets<Shader>>) {
    let _ = shaders.insert(RIG_SHADER.id(), Shader::from_wgsl(rig_shader_src_for(false), "nd_app/people_rig.wgsl"));
    let _ = shaders.insert(RIG_PREPASS_SHADER.id(), Shader::from_wgsl(rig_shader_src_for(true), "nd_app/people_rig_prepass.wgsl"));
}

/// The near figures' material: the far figures' (vertex colours, the
/// night-trim palette as emissive map) with the walking vertex shader.
pub(super) type RigMaterial = ExtendedMaterial<StandardMaterial, PeopleRig>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub(super) struct PeopleRig {
    /// Unused (a bind group needs something).
    #[uniform(100)]
    pub params: Vec4,
}

impl MaterialExtension for PeopleRig {
    fn vertex_shader() -> ShaderRef {
        RIG_SHADER.into()
    }
    // [r17-people] The prepass poses the limbs too: without it the figures
    // had no depth or motion vectors, and TAA blended the wall behind them
    // through (stippled, half see-through people close to the camera).
    fn prepass_vertex_shader() -> ShaderRef {
        RIG_PREPASS_SHADER.into()
    }
    fn enable_shadows() -> bool {
        false
    }
}

/// A near entity.
#[derive(Component)]
pub(super) struct NearProxy;

/// Someone close enough for a near figure this frame, as the far
/// renderers (citizens, visitors) found them.
#[derive(Clone, Copy)]
pub(super) struct NearCandidate {
    /// The far figure's entity.
    pub src: Entity,
    pub key: NearKey,
    pub stature: f32,
    /// A fixed rank from the citizen's seed: who keeps their umbrella in a
    /// tight crowd ([`spread_umbrellas`]).
    pub rank: u32,
}

/// A near figure lent to a far one.
struct Lent {
    proxy: Entity,
    key: NearKey,
    stature: f32,
    phase: f32,
    amp: f32,
    last: Option<Vec3>,
    tag: u32,
}

#[derive(Resource, Default)]
pub(super) struct NearPeople {
    /// Filled by `sync_citizens` and `sync_tourists` each frame.
    pub candidates: Vec<NearCandidate>,
    mat: Option<Handle<RigMaterial>>,
    meshes: std::collections::BTreeMap<NearKey, (Handle<Mesh>, u64, usize)>,
    frame: u64,
    free: Vec<Entity>,
    lent: HashMap<Entity, Lent>,
    band: Option<(f32, f32)>,
    pub built: usize,
}

/// `ND_NEAR_PEOPLE=0`: no near figures (for comparing frames).
pub(super) fn near_off() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| std::env::var("ND_NEAR_PEOPLE").is_ok_and(|v| v == "0"))
}

impl NearPeople {
    /// Whether the far renderers should list candidates this frame.
    pub(super) fn wanted(plain: bool) -> bool {
        !plain && !near_off()
    }

    fn mesh(&mut self, key: &NearKey, builds: &mut usize, meshes: &mut Assets<Mesh>) -> Option<(Handle<Mesh>, usize)> {
        let frame = self.frame;
        if let Some(e) = self.meshes.get_mut(key) {
            e.1 = frame;
            return Some((e.0.clone(), e.2));
        }
        if *builds >= BUILDS_PER_FRAME {
            return None;
        }
        if self.meshes.len() >= NEAR_MESH_CAP {
            // The look longest off screen goes (in key order among equals).
            let oldest = self.meshes.iter().filter(|(_, e)| e.1 < frame).min_by_key(|(_, e)| e.1).map(|(k, _)| *k)?;
            self.meshes.remove(&oldest);
        }
        *builds += 1;
        self.built += 1;
        let _b = crate::frame_stats::span("near_build");
        let data = near::near_person_mesh(key);
        let tris = data.indices.len() / 3;
        let h = meshes.add(shared_mesh(&data, false));
        self.meshes.insert(*key, (h.clone(), frame, tris));
        Some((h, tris))
    }
}

fn proxy_range(band: (f32, f32)) -> VisibilityRange {
    VisibilityRange { start_margin: -2.0..-1.0, end_margin: band.0..band.1, use_aabb: false }
}

fn far_range(band: (f32, f32)) -> VisibilityRange {
    VisibilityRange { start_margin: band.0..band.1, end_margin: 1.0e6..1.0e6 + 1.0, use_aabb: false }
}

/// Umbrellas closer than this to another one: only one of them stays up.
pub(super) const UMBRELLA_GAP: f32 = 0.8;

/// [r17-people] People queued on almost the same spot (a bus stop in the
/// snow) each put their umbrella up and the canopies piled into a layered
/// stack. Going through the umbrella holders in a fixed order (their seed
/// rank), one keeps it up only if no one kept before stands within
/// [`UMBRELLA_GAP`] (a figure that put theirs down needs half as much
/// again before it goes back up, so passers-by do not flicker); the others
/// stand under it with it down (a hoodie's hood up instead).
fn spread_umbrellas(by_src: &mut HashMap<Entity, NearCandidate>, candidates: &[NearCandidate], lent: &HashMap<Entity, Lent>, sources: &Query<(&Transform, &Visibility), Without<NearProxy>>) {
    let mut holders: Vec<(u32, u64, Entity, SVec2)> = candidates
        .iter()
        .filter(|c| c.key.extras.umbrella)
        .filter_map(|c| sources.get(c.src).ok().map(|(t, _)| (c.rank, c.src.to_bits(), c.src, SVec2::new(t.translation.x, t.translation.z))))
        .collect();
    holders.sort_by_key(|h| (h.0, h.1));
    holders.dedup_by_key(|h| h.2);
    let mut grid: HashMap<(i32, i32), Vec<SVec2>> = HashMap::new();
    let cell = |p: SVec2| ((p.x / UMBRELLA_GAP).floor() as i32, (p.y / UMBRELLA_GAP).floor() as i32);
    for (_, _, src, p) in holders {
        let was_down = lent.get(&src).is_some_and(|l| !l.key.extras.umbrella);
        let gap = if was_down { UMBRELLA_GAP * 1.5 } else { UMBRELLA_GAP };
        let (cx, cy) = cell(p);
        let crowded = (-2..=2).any(|dx| (-2..=2).any(|dy| grid.get(&(cx + dx, cy + dy)).is_some_and(|v| v.iter().any(|q| q.distance(p) < gap))));
        if crowded {
            if let Some(c) = by_src.get_mut(&src) {
                let hoodie = nd_sim::looks::Outfit::ALL.get(c.key.base.outfit as usize) == Some(&nd_sim::looks::Outfit::Hoodie);
                c.key.extras.umbrella = false;
                c.key.extras.hood_up = hoodie;
            }
        } else {
            grid.entry((cx, cy)).or_default().push(p);
        }
    }
}

/// Lend, update and take back the near figures (after the far figures
/// are posed and lifted).
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_near_people(
    mut commands: Commands,
    time: Res<Time>,
    mut near_state: ResMut<NearPeople>,
    people: Res<people::PeopleRender>,
    mode: Res<crate::camera::CameraMode>,
    follow: Res<crate::camera::CameraFollow>,
    (overlays, lighting, tools): (Res<Overlays>, Res<Lighting>, Res<crate::tools::ToolState>),
    mut stats: ResMut<RenderStats>,
    mut meshes: ResMut<Assets<Mesh>>,
    (std_mats, mut rig_mats): (Res<Assets<StandardMaterial>>, ResMut<Assets<RigMaterial>>),
    camera: Query<&GlobalTransform, With<Camera3d>>,
    sources: Query<(&Transform, &Visibility), Without<NearProxy>>,
    mut proxies: Query<(&mut Transform, &mut Visibility, &mut Mesh3d, &mut MeshTag), With<NearProxy>>,
) {
    let _t = crate::frame_stats::span("sync_near_people");
    let st = &mut *near_state;
    st.frame += 1;
    let candidates = std::mem::take(&mut st.candidates);
    let normal = swap::View::wanted(&overlays, &lighting, swap::Ghost::wanted(&tools)) == swap::View::Normal;
    let on = normal && NearPeople::wanted(people::plain());
    let take_back = |commands: &mut Commands, st: &mut NearPeople, src: Entity, proxies: &mut Query<(&mut Transform, &mut Visibility, &mut Mesh3d, &mut MeshTag), With<NearProxy>>| {
        if let Some(l) = st.lent.remove(&src) {
            if let Ok((_, mut vis, _, _)) = proxies.get_mut(l.proxy) {
                vis.set_if_neq(Visibility::Hidden);
            }
            st.free.push(l.proxy);
            if let Ok(mut e) = commands.get_entity(src) {
                e.try_remove::<VisibilityRange>();
            }
        }
    };
    if !on {
        let all: Vec<Entity> = st.lent.keys().copied().collect();
        for src in all {
            take_back(&mut commands, st, src, &mut proxies);
        }
        stats.people_near = 0;
        stats.people_near_tris = 0;
        return;
    }
    // The material: the far figures' own, with the rig; its glow follows
    // theirs at night.
    let far_mat = people.material_handle().and_then(|h| std_mats.get(&h));
    let Some(far_mat) = far_mat else { return };
    let mat = match &st.mat {
        Some(m) => m.clone(),
        None => {
            let m = rig_mats.add(RigMaterial { base: far_mat.clone(), extension: PeopleRig::default() });
            st.mat = Some(m.clone());
            m
        }
    };
    if rig_mats.get(&mat).is_some_and(|m| m.base.emissive != far_mat.emissive) {
        if let Some(mut m) = rig_mats.get_mut(&mat) {
            m.base.emissive = far_mat.emissive;
        }
    }
    let street = mode.street() || follow.target.is_some();
    let band = if street { STREET_BAND } else { NEAR_BAND };
    let band_moved = st.band != Some(band);
    st.band = Some(band);
    let eye = camera.iter().next().map_or(Vec3::ZERO, |t| t.translation());
    let dt = time.delta_secs().clamp(1e-4, 0.25);
    // Keep or take back those lent: gone from the candidates, or past the
    // keep.
    let mut by_src: HashMap<Entity, NearCandidate> = HashMap::new();
    for c in &candidates {
        by_src.entry(c.src).or_insert(*c);
    }
    spread_umbrellas(&mut by_src, &candidates, &st.lent, &sources);
    let lent_now: Vec<Entity> = st.lent.keys().copied().collect();
    for src in lent_now {
        let keep = by_src.contains_key(&src) && sources.get(src).is_ok_and(|(t, _)| t.translation.distance(eye) < band.1 + KEEP_PAST);
        if !keep {
            take_back(&mut commands, st, src, &mut proxies);
        }
    }
    // Lend to the nearest not yet lent, up to the cap.
    let mut builds = 0usize;
    for c in &candidates {
        let Some(c) = by_src.get(&c.src) else { continue };
        if st.lent.len() >= NEAR_CAP {
            break;
        }
        if st.lent.contains_key(&c.src) {
            continue;
        }
        let Ok((t, _)) = sources.get(c.src) else { continue };
        if t.translation.distance(eye) > band.1 + LEND_BEFORE {
            continue;
        }
        let Some((mesh, _)) = st.mesh(&c.key, &mut builds, &mut meshes) else { continue };
        let proxy = match st.free.pop() {
            Some(p) => {
                if let Ok((_, _, mut m, _)) = proxies.get_mut(p) {
                    m.0 = mesh;
                }
                commands.entity(p).insert(proxy_range(band));
                p
            }
            None => commands.spawn((Mesh3d(mesh), MeshMaterial3d(mat.clone()), Transform::from_translation(t.translation), Visibility::Hidden, NotShadowCaster, NotShadowReceiver, MeshTag(0), proxy_range(band), NearProxy)).id(),
        };
        commands.entity(c.src).try_insert(far_range(band));
        st.lent.insert(c.src, Lent { proxy, key: c.key, stature: c.stature, phase: (c.src.to_bits() % 97) as f32 / 97.0, amp: 0.0, last: None, tag: u32::MAX });
    }
    if band_moved {
        for (src, l) in st.lent.iter() {
            commands.entity(*src).try_insert(far_range(band));
            commands.entity(l.proxy).insert(proxy_range(band));
        }
    }
    // Pose every near figure on its far one, and its stride.
    let mut shown = 0usize;
    let mut tris = 0usize;
    let srcs: Vec<Entity> = st.lent.keys().copied().collect();
    for src in srcs {
        let Some(c) = by_src.get(&src).copied() else { continue };
        let Ok((t, vis)) = sources.get(src) else { continue };
        // A new look (riding now, the rain): its mesh when there is one.
        let new_mesh = if st.lent[&src].key != c.key { st.mesh(&c.key, &mut builds, &mut meshes) } else { None };
        let mesh_tris = st.meshes.get(&st.lent[&src].key).map_or(0, |e| e.2);
        let l = st.lent.get_mut(&src).unwrap();
        l.stature = c.stature;
        if let Some((m, _)) = &new_mesh {
            l.key = c.key;
            if let Ok((_, _, mut pm, _)) = proxies.get_mut(l.proxy) {
                pm.0 = m.clone();
            }
        }
        let Ok((mut pt, mut pv, _, mut tag)) = proxies.get_mut(l.proxy) else { continue };
        pv.set_if_neq(*vis);
        if *vis == Visibility::Hidden {
            l.last = None;
            continue;
        }
        shown += 1;
        tris += mesh_tris;
        let at = t.translation;
        let moved = l.last.map_or(0.0, |p| SVec2::new(at.x - p.x, at.z - p.z).length());
        let moved = if moved > 6.0 { 0.0 } else { moved };
        l.last = Some(at);
        let speed = moved / dt;
        let k = &l.key.base;
        if k.ride {
            // A turn of the cranks carries a city bike about 4 m.
            l.phase = (l.phase + (moved / 4.0).min(1.6 * dt)).fract();
            l.amp = 0.0;
        } else if k.sit != 0 {
            l.amp = 0.0;
        } else {
            let stride = 1.45 * l.stature.max(0.4);
            l.phase = (l.phase + (moved / stride).min(2.1 * dt)).fract();
            let want = ((speed - 0.08) / 0.7).clamp(0.0, 1.0) * if l.key.extras.old { 0.65 } else { 1.0 };
            l.amp += (want - l.amp) * (dt * 6.0).min(1.0);
        }
        let new_tag = near::walk_tag(l.phase, l.amp);
        if new_tag != l.tag {
            l.tag = new_tag;
            tag.0 = new_tag;
        }
        let want_t = Transform { translation: at, rotation: t.rotation, scale: Vec3::splat(l.stature) };
        if *pt != want_t {
            *pt = want_t;
        }
    }
    stats.people_near = shown;
    stats.people_near_tris = tris;
    stats.people_near_lent = st.lent.len();
    stats.people_near_meshes = st.meshes.len();
    stats.people_near_built = st.built;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rig_shader_has_the_rust_numbers() {
        let s = rig_shader_src();
        assert!(s.contains("const RIDE_HIP: vec2<f32> = vec2<f32>(-0.17, 0.97);"), "{s}");
        assert!(s.contains("const RIDE_REST: f32 = 0.0;"));
        assert!(!s.contains("{{"), "the braces came through formatting");
    }

    #[test]
    fn the_prepass_poses_and_moves_too() {
        let s = rig_shader_src_for(true);
        assert!(s.contains("prepass_io::{Vertex, VertexOutput}") && s.contains("previous_world_position") && s.contains("fn pose("));
        let m = rig_shader_src_for(false);
        assert!(m.contains("forward_io::{Vertex, VertexOutput}") && m.contains("pose(pos, nrm, vertex.uv_b"));
    }

    #[test]
    fn the_bands_cross_fade_and_nest() {
        for band in [NEAR_BAND, STREET_BAND] {
            assert!(band.0 < band.1);
            let (p, f) = (proxy_range(band), far_range(band));
            assert_eq!(p.end_margin, f.start_margin, "the near one fades out where the far one fades in");
        }
        assert!(CANDIDATE_REACH > STREET_BAND.1 + KEEP_PAST);
    }
}
