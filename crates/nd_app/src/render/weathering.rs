//! [r12-grime] Soft edges and shader grime: the city weathered in the
//! shader, city-wide at once, with no extra triangles and no work a frame.
//!
//! The walls of every building, the pavements and the lot floors carry a
//! weathering pair per vertex in their second UV set (written when their
//! meshes are built anyway: `nd_procgen::weathering`). Those meshes wear
//! a small extension of their surface's own material ([`WeatherMaterial`],
//! one per surface, so no draw call is added) whose fragment shader reads
//! the pair and:
//!
//! - **on walls** lays a dark grime band where the wall meets its ground
//!   (ragged, taller the dirtier), rain streaks hanging under each
//!   storey's sills, dirt in the corners the ambient occlusion finds,
//!   blotchy staining, and damp and moss low on the shaded north side
//!   (and on old roofs) in a damp climate; desert grime is pale dust,
//!   volcanic grime soot;
//! - **on the ground** creeps weeds and grass in ragged clumps over the
//!   pavement from its back edge (and in from a lot's edge over its
//!   gravel), washes soil over the very edge, lays grime along the kerb,
//!   and breaks the paving with patches and cracks, all scaled by the
//!   street's land value, with dirt in the corners again from the AO.
//!
//! How much comes from the building (age, condition, tier, land use,
//! style: new, rich and glass stay clean, the capsule domes spotless) or
//! the street (land value), baked when the mesh is built; the climate's
//! colours and damp are uniforms. Fine detail fades with distance, so
//! from far off it reads as a little darkening at the foot of things.
//!
//! The extension keeps the standard lighting and the chase camera's cut
//! (`cutaway`): it takes the cut's uniform at the same binding and uses the
//! cut's own prepass. Each weathered mesh is two entities sharing the mesh,
//! the plain one and a twin wearing the weathering, one of them hidden:
//! outside the normal view (heat vision, focus and ghost views) and with
//! Weathering off in Settings (`ND_WEATHERING=0`) the plain one shows, so
//! the material swapper and the cut work on it as before and nothing is
//! spent. (Switching one entity between material types made Bevy
//! specialise it again every frame.)

use bevy::asset::uuid_handle;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::{Shader, ShaderRef};
use nd_procgen::weathering::{climate_grime, takes_grime};
use nd_procgen::Surface;

use bevy::camera::primitives::{Aabb, MeshAabb};
use bevy::camera::visibility::{RenderLayers, VisibilityRange};

use super::cutaway::CutParams;
use super::swap::{Ghost, View};
use super::{Lighting, Materials, Overlays};
use crate::sim::Sim;

/// A surface material with the weathering.
pub(super) type WeatherMaterial = ExtendedMaterial<StandardMaterial, Weathering>;

/// The climate's grime as the shader takes it (linear colours).
#[derive(ShaderType, Clone, Copy, Debug, Default, PartialEq, Reflect)]
pub(super) struct WeatherParams {
    /// rgb: the dirt at the foot of walls and in corners; a: strength.
    pub grime: Vec4,
    /// rgb: moss and algae; a: damp (0..1).
    pub moss: Vec4,
    /// rgb: weeds over paving edges; a: how much grows (0..1).
    pub weeds: Vec4,
    /// x: how strongly this surface takes grime; y: storey height (m);
    /// z: [r14-photofix] how green things grow in today's cold (0..1).
    pub surface: Vec4,
}

impl WeatherParams {
    pub(super) fn of(climate: nd_sim::Climate, s: Surface) -> Self {
        Self::of_at(climate, s, 15.0)
    }

    /// [`WeatherParams::of`] at `temp_c` degrees: weeds and moss lose their
    /// green below about 8 °C, to a third at -4 °C and colder.
    pub(super) fn of_at(climate: nd_sim::Climate, s: Surface, temp_c: f32) -> Self {
        let c = climate_grime(climate);
        let v = |rgb: [f32; 3], a: f32| Vec4::new(rgb[0], rgb[1], rgb[2], a);
        // Ground surfaces carry their own wear in the channel: full take.
        let take = if matches!(s, Surface::Paving | Surface::WornPaving | Surface::Dirt) { 1.0 } else { takes_grime(s) };
        Self { grime: v(c.grime, strength()), moss: v(c.moss, c.damp), weeds: v(c.weeds, c.growth), surface: Vec4::new(take, STOREY, green_in(temp_c), 0.0) }
    }
}

/// [r14-photofix] How green weeds and moss show at `temp_c`: full from
/// 8 °C, a third at -4 °C and below (the photographer found a saturated
/// green band along the shopfronts in snow at -2 °C).
pub(super) fn green_in(temp_c: f32) -> f32 {
    let t = ((temp_c + 4.0) / 12.0).clamp(0.0, 1.0);
    0.33 + 0.67 * t * t * (3.0 - 2.0 * t)
}

/// Storey height the sill streaks repeat at (the styles' usual floor).
const STOREY: f32 = 3.2;

/// `ND_WEATHERING_K` scales the grime (dev, for judging the look); 1 by
/// default.
fn strength() -> f32 {
    std::env::var("ND_WEATHERING_K").ok().and_then(|v| v.parse::<f32>().ok()).map_or(1.0, |k| k.clamp(0.0, 4.0))
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub(super) struct Weathering {
    /// [chase-cam] The cut, at the binding the cut's prepass reads.
    #[uniform(100)]
    pub cut: CutParams,
    #[uniform(101)]
    pub params: WeatherParams,
}

impl MaterialExtension for Weathering {
    fn fragment_shader() -> ShaderRef {
        WEATHER_SHADER.into()
    }
    fn prepass_fragment_shader() -> ShaderRef {
        super::cutaway::CUT_PREPASS_SHADER.into()
    }
}

const WEATHER_SHADER: Handle<Shader> = uuid_handle!("5d0e7c21-9a43-4b8e-b6f1-2c84e09a7d36");

const WEATHER_HEAD_SRC: &str = r#"
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::alpha_discard,
    forward_io::{VertexOutput, FragmentOutput},
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    pbr_types::STANDARD_MATERIAL_FLAGS_UNLIT_BIT,
    mesh_view_bindings::view,
}
"#;

/// The weathering itself: hashes, noise, the wall and the ground looks.
/// Kept apart from the entry point so a test can validate it with naga.
pub(super) const WEATHER_FN_SRC: &str = r#"
struct WeatherParams {
    grime: vec4<f32>,
    moss: vec4<f32>,
    weeds: vec4<f32>,
    surface: vec4<f32>,
}
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> weather: WeatherParams;

// A cheap 2D hash in 0..1 (no sines: steady at a city's coordinates).
fn w_hash(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2(0.1031, 0.1030));
    q += dot(q, q.yx + 33.33);
    return fract((q.x + q.y) * q.x);
}

// Smooth value noise in 0..1, lattice spacing 1.
fn w_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let s = f * f * (3.0 - 2.0 * f);
    let a = w_hash(i);
    let b = w_hash(i + vec2(1.0, 0.0));
    let c = w_hash(i + vec2(0.0, 1.0));
    let d = w_hash(i + vec2(1.0, 1.0));
    return mix(mix(a, b, s.x), mix(c, d, s.x), s.y);
}

// How much of the fine detail shows at this distance: all of it close
// up, none from far off (where it would only shimmer).
fn w_near(world: vec3<f32>, eye: vec3<f32>) -> f32 {
    return 1.0 - smoothstep(120.0, 650.0, distance(world, eye));
}

// A wall standing on ground at `ground` (world y), worn `wear`. `ao` is
// the ambient occlusion here (1 open, 0 shut in). Returns the colour.
fn weather_wall(base: vec3<f32>, world: vec3<f32>, n: vec3<f32>, wear_in: f32, ground: f32, ao: f32, near: f32) -> vec3<f32> {
    let wear = clamp(wear_in * weather.surface.x * weather.grime.a, 0.0, 1.5);
    if wear <= 0.001 {
        return base;
    }
    // How much of it shows: quick to take hold, full from about 0.55.
    let wk = min(wear * 1.8, 1.0);
    let h = world.y - ground;
    let upright = 1.0 - abs(n.y);
    let roof = smoothstep(0.6, 0.9, n.y);
    // Along the wall, for streaks and the band's ragged top.
    let t = normalize(vec2(-n.z, n.x) + vec2(1e-4, 0.0));
    let u = dot(world.xz, t);
    let big = w_noise(vec2(u * 0.13, h * 0.11) + world.xz * 0.02);
    // Grime band at the foot: splash-back and soil, ragged at its top.
    let band_h = 0.3 + 1.0 * wear;
    let rag = (w_noise(vec2(u * 1.9, 3.1)) - 0.5) * 0.45 * band_h;
    let band = (1.0 - smoothstep(0.0, band_h, h + rag)) * upright;
    // Rain streaks under each storey's sill line, in narrow columns.
    let storey = weather.surface.y;
    let below = fract((0.95 - h) / storey) * storey;
    let lane = floor((h - 0.95) / storey);
    // [r14-photofix] Each storey of each building its own: the columns, how
    // far they run and how dark they are vary by storey and by where the
    // wall stands (a 23 m cell, about a building), so the streaks do not
    // repeat floor after floor (photo report 2026-11-22, finding 11).
    let site = w_hash(floor(world.xz / 23.0) + vec2(lane * 3.7, 11.0));
    let col = smoothstep(0.55, 0.85, w_noise(vec2(u * 2.6 + site * 17.0, lane * 7.31)));
    let len = (0.35 + 1.8 * wear) * (0.45 + 1.1 * site);
    let streak = col * (0.35 + 0.65 * w_hash(vec2(floor(u * 1.3), lane + site * 5.0))) * exp(-below / len) * step(1.6, h) * upright * near;
    // Corners and recesses: where the occlusion is, the dirt is.
    let corner = clamp((1.0 - ao) * 1.6, 0.0, 1.0);
    // Blotchy staining over the whole face, broad and faint.
    let stain = (big - 0.45) * 0.5 * near;
    let dirt = clamp(band * (0.75 + 0.2 * wk) + streak * 0.7 + corner * 0.6 + max(stain, 0.0) * (0.45 + 0.35 * roof), 0.0, 0.92) * wk;
    var c = mix(base, weather.grime.rgb, dirt);
    // Damp and moss: low on the shaded (north, -z) side, patchy; on old
    // roofs in patches; rising damp a little everywhere at the foot.
    let damp = weather.moss.a;
    if damp > 0.001 {
        let shade = smoothstep(0.1, 0.85, -n.z) * upright;
        let low = 1.0 - smoothstep(0.0, 1.2 + 2.6 * wear, h);
        let patchy = smoothstep(0.42, 0.78, w_noise(world.xz * 0.9 + vec2(0.0, h * 1.4)) * 0.7 + big * 0.3);
        let wet = damp * wk * (shade * low + 0.35 * band);
        c *= 1.0 - 0.35 * wet;
        let moss = damp * wk * patchy * (shade * low * 0.8 + roof * 0.45 * smoothstep(0.35, 0.8, wear)) * (0.4 + 0.6 * near) * weather.surface.z;
        c = mix(c, weather.moss.rgb, clamp(moss, 0.0, 0.85));
    }
    return c;
}

// Ground `packed` in from a soft edge (`weathering::pack_edge`), worn
// `wear`. Returns the colour.
fn weather_ground(base: vec3<f32>, world: vec3<f32>, wear_in: f32, packed: f32, ao: f32, near: f32) -> vec3<f32> {
    let wear = clamp(wear_in * weather.surface.x * weather.grime.a, 0.0, 1.5);
    if wear <= 0.001 {
        return base;
    }
    let wk = min(wear * 1.5, 1.0);
    let k = floor(packed / 16.0);
    let span = k / 10.0;
    let d = packed - 16.0 * k - 0.5;
    let p = world.xz;
    let rag = w_noise(p * 2.3) * 0.6 + w_noise(p * 6.7) * 0.4;
    // Weeds creep in clumps along the edge, further the more neglected.
    // [r14-photofix] In tufts hugging the edge (most within 30-60 cm),
    // patchy, a grey-green, and fewer in the cold: a band up to 1.9 m
    // deep of saturated green, cut off hard where a shop's step met the
    // pavement, read as paint (photo report 2026-11-22, finding 10).
    let growth = weather.weeds.a * weather.surface.z;
    let clump = smoothstep(0.25, 0.8, w_noise(p * 0.42 + vec2(5.3, 1.7)));
    let reach = (0.12 + 0.45 * wk) * (0.35 + 1.15 * clump);
    let patch = smoothstep(0.3, 0.7, w_noise(p * 3.1 + vec2(4.4, 0.9)));
    let weeds = (1.0 - smoothstep(0.0, reach, d + (rag - 0.5) * 0.3)) * growth * (0.35 + 0.65 * patch);
    let tuft = 0.55 + 0.6 * w_noise(p * 11.0);
    let lum = dot(weather.weeds.rgb, vec3(0.2126, 0.7152, 0.0722));
    let weed_rgb = mix(vec3(lum, lum, lum * 0.9), weather.weeds.rgb, 0.55);
    // Soil washed over the very edge, and grime along the hard edge
    // across (the kerb), in the gutter of the slabs.
    let soil = (1.0 - smoothstep(0.0, 0.2 + 0.45 * wk, d + (rag - 0.5) * 0.15)) * wk;
    let front = select(99.0, span - d, span > 0.05);
    let gutter = (1.0 - smoothstep(0.0, 0.18 + 0.25 * wk, front + (rag - 0.5) * 0.08)) * wk;
    // Patched slabs, cracks (contours of a noise), dirt in the corners.
    let pn = w_noise(p * 0.55 + vec2(17.3, 4.1));
    let patched = smoothstep(0.80 - 0.2 * wk, 0.83 - 0.2 * wk, pn) * wk;
    // [r14-photofix] The contour of a warped noise: unwarped, it ringed the
    // lattice points in little closed loops (hearts) on a grid.
    let warp = vec2(w_noise(p * 0.31 + vec2(7.1, 2.3)), w_noise(p * 0.31 + vec2(1.9, 8.4))) * 1.6;
    let crack = (1.0 - smoothstep(0.0, 0.03, abs(w_noise(p * 1.15 + warp + vec2(3.7, 9.2)) - 0.5))) * smoothstep(0.4, 0.85, w_noise(p * 0.27)) * smoothstep(0.45, 0.6, w_noise(p * 4.3 + vec2(2.2, 0.7))) * wk * near;
    let corner = clamp((1.0 - ao) * 1.6, 0.0, 1.0) * min(wk + 0.2, 1.0);
    var c = base;
    c = mix(c, c * vec3(0.74, 0.73, 0.72), patched * 0.8);
    c = mix(c, weather.grime.rgb, clamp(soil * 0.55 + gutter * 0.6 + corner * 0.5 + crack * 0.6, 0.0, 0.9));
    c = mix(c, weed_rgb * tuft, clamp(weeds * 0.8 + crack * growth * 0.25, 0.0, 0.85));
    return c;
}

fn weather_color(base: vec3<f32>, world: vec3<f32>, n: vec3<f32>, aux: vec2<f32>, ao: f32, eye: vec3<f32>) -> vec3<f32> {
    if aux.x == 0.0 {
        return base;
    }
    let near = w_near(world, eye);
    if aux.x > 0.0 {
        return weather_wall(base, world, n, aux.x, aux.y, ao, near);
    }
    return weather_ground(base, world, -aux.x, aux.y, ao, near);
}
"#;

const WEATHER_MAIN_SRC: &str = r#"
@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    if cut_away(in.world_position.xyz, in.position.xy, view.world_position) {
        discard;
    }
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    pbr_input.material.base_color = alpha_discard(pbr_input.material, pbr_input.material.base_color);
#ifdef VERTEX_UVS_B
    let ao = pbr_input.diffuse_occlusion.g;
    let rgb = weather_color(pbr_input.material.base_color.rgb, in.world_position.xyz, normalize(in.world_normal), in.uv_b, ao, view.world_position);
    pbr_input.material.base_color = vec4(rgb, pbr_input.material.base_color.a);
#endif
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

pub(super) fn add_shader(mut shaders: ResMut<Assets<Shader>>) {
    let cut = super::cutaway::cut_wgsl(100);
    let main = format!("{WEATHER_HEAD_SRC}{cut}{WEATHER_FN_SRC}{WEATHER_MAIN_SRC}");
    let _ = shaders.insert(WEATHER_SHADER.id(), Shader::from_wgsl(main, "nd_app/weathering.wgsl"));
}

/// A mesh whose vertices carry the weathering channel, drawn on this
/// surface. It keeps its plain surface material (so the material swapper
/// and the cut work on it as on any mesh) and gets a [`Twin`]: the same
/// mesh with the surface's [`WeatherMaterial`]. One of the two is shown.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Weathered(pub Surface);

/// On a weathered mesh: its twin (read by the tests; the twin finds its
/// mesh through [`TwinOf`]).
#[derive(Component)]
pub(super) struct Twin(#[allow(dead_code)] Entity);

/// On a twin: the weathered mesh it stands in for (gone: the twin goes).
#[derive(Component)]
pub(super) struct TwinOf(Entity);

/// Whether the weathering is drawn: the setting (Settings, Graphics),
/// unless `ND_WEATHERING=0` turns it off for the run.
pub(super) fn env_allows() -> bool {
    std::env::var("ND_WEATHERING").map_or(true, |v| v != "0")
}

/// One weathering material per lit surface (index = `Surface::index()`;
/// none for glow), each a copy of the surface's material.
pub(super) fn make_materials(surfaces: &[Handle<StandardMaterial>], std_mats: &Assets<StandardMaterial>, weather_mats: &mut Assets<WeatherMaterial>) -> Vec<Option<Handle<WeatherMaterial>>> {
    Surface::ALL
        .iter()
        .zip(surfaces)
        .map(|(&s, h)| {
            let base = std_mats.get(h).filter(|_| !s.is_glow())?;
            Some(weather_mats.add(WeatherMaterial { base: base.clone(), extension: Weathering { cut: CutParams::default(), params: WeatherParams::of(nd_sim::Climate::default(), s) } }))
        })
        .collect()
}

/// Keep the weathering copies looking like their surfaces: a surface
/// material changed (wet streets, a reloaded look) is copied over, and the
/// climate's grime set when the city's climate changes. Nothing to do on
/// an ordinary frame.
pub(super) fn sync_weather_materials(
    sim: Res<Sim>,
    mats: Res<Materials>,
    mut events: MessageReader<AssetEvent<StandardMaterial>>,
    std_mats: Res<Assets<StandardMaterial>>,
    mut weather_mats: ResMut<Assets<WeatherMaterial>>,
    mut climate: Local<Option<(nd_sim::Climate, i32)>>,
) {
    // [r14-photofix] And the temperature, in 2 degree steps (the green of
    // the weeds and moss follows it).
    let now = sim.world.landscape.params.climate;
    let step = (sim.world.weather.temp_c / 2.0).round() as i32;
    if *climate != Some((now, step)) {
        *climate = Some((now, step));
        for (&s, h) in Surface::ALL.iter().zip(&mats.weather) {
            if let Some(mut m) = h.as_ref().and_then(|h| weather_mats.get_mut(h)) {
                m.extension.params = WeatherParams::of_at(now, s, step as f32 * 2.0);
            }
        }
    }
    for ev in events.read() {
        let AssetEvent::Modified { id } = ev else { continue };
        let Some(k) = mats.surfaces.iter().position(|h| h.id() == *id) else { continue };
        let (Some(o), Some(Some(h))) = (std_mats.get(*id), mats.weather.get(k)) else { continue };
        if let Some(mut m) = weather_mats.get_mut(h) {
            m.base = o.clone();
        }
    }
}

/// Show the weathering: each weathered mesh gets a twin (the same mesh,
/// wearing its surface's weathering material) when it first appears, and
/// one of the two is shown: the twin in the normal view with the setting
/// on, the plain mesh otherwise (heat vision, the focus and ghost views,
/// setting off). In the Underground view the twins are hidden and the
/// plain meshes left to it (`holo::apply_depth`); the map view hides and
/// restores both itself. Twins whose mesh is gone
/// go too. Nothing changes material type: an entity switched between
/// material types was specialised again every frame (300 ms frames).
#[allow(clippy::type_complexity)]
pub(super) fn apply_weathering(
    mut commands: Commands,
    lighting: Res<Lighting>,
    overlays: Res<Overlays>,
    tools: Option<Res<crate::tools::ToolState>>,
    mats: Res<Materials>,
    meshes: Res<Assets<Mesh>>,
    mut new: Query<(Entity, &Weathered, &Mesh3d, &Transform, &mut Visibility, Option<&Aabb>, Option<&VisibilityRange>), Without<Twin>>,
    mut plain: Query<(&Twin, &mut Visibility, Option<&RenderLayers>), (With<Weathered>, Without<TwinOf>)>,
    mut twins: Query<(Entity, &TwinOf, &mut Visibility, Has<RenderLayers>), Without<Weathered>>,
) {
    let under = tools.as_deref().is_some_and(|t| t.depth == crate::tools::Depth::Underground);
    let normal = View::wanted(&overlays, &lighting, tools.as_deref().and_then(Ghost::wanted)) == View::Normal;
    let want = lighting.weathering && normal && !under && !overlays.map;
    let (show, hide) = (Visibility::Inherited, Visibility::Hidden);
    for (e, w, mesh, tf, mut vis, aabb, range) in &mut new {
        let Some(Some(h)) = mats.weather.get(w.0.index()) else { continue };
        let mut t = commands.spawn((Mesh3d(mesh.0.clone()), MeshMaterial3d(h.clone()), *tf, TwinOf(e), if want { show } else { hide }));
        // Its bounds: the mesh's own (render-world meshes keep their data
        // only until uploaded, in the frame they were added).
        if let Some(b) = aabb.copied().or_else(|| meshes.get(&mesh.0).and_then(|m| m.compute_aabb())) {
            t.insert(b);
        }
        if let Some(r) = range {
            t.insert(r.clone());
        }
        let id = t.id();
        commands.entity(e).insert(Twin(id));
        if want {
            vis.set_if_neq(hide);
        }
    }
    // The map view hides every mesh itself (`mapview::hide_for_map`) and
    // brings back what it hid.
    let map = overlays.map;
    if !under && !map {
        for (_, mut vis, _) in &mut plain {
            vis.set_if_neq(if want { hide } else { show });
        }
    }
    for (t, of, mut vis, layered) in &mut twins {
        let Ok((_, _, layers)) = plain.get(of.0) else {
            commands.entity(t).try_despawn();
            continue;
        };
        // [junction editor] On the plain mesh's render layers too.
        if let (Some(l), false) = (layers, layered) {
            commands.entity(t).try_insert(l.clone());
        }
        if !map {
            vis.set_if_neq(if want { show } else { hide });
        }
    }
}

/// [chase-cam] The weathering materials cut like the rest (`cutaway`).
pub(super) fn set_cut(mats: &Materials, weather_mats: &mut Assets<WeatherMaterial>, params: CutParams) {
    for h in mats.weather.iter().flatten() {
        if let Some(mut m) = weather_mats.get_mut(h) {
            m.extension.cut = params;
        }
    }
}

/// Does this mesh carry the weathering channel (so it should wear the
/// weathering)?
pub(super) fn has_channel(data: &nd_procgen::MeshData) -> bool {
    data.aux.iter().any(|a| a[0] != 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The weathering WGSL is valid: the functions with a fragment that
    /// calls them (a reserved word or a type slip would leave every
    /// building undrawn).
    #[test]
    fn the_weathering_shader_parses_and_validates() {
        let src = format!(
            "{}
@fragment fn main(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {{
    let a = weather_color(vec3(0.5), p.xyz, vec3(0.0, 0.0, -1.0), vec2(0.6, 1.0), 0.8, vec3(0.0));
    let b = weather_color(a, p.xyz, vec3(0.0, 1.0, 0.0), vec2(-0.5, 37.2), 1.0, vec3(0.0));
    return vec4(b, 1.0);
}}",
            WEATHER_FN_SRC.replace("#{MATERIAL_BIND_GROUP}", "2")
        );
        let module = naga::front::wgsl::parse_str(&src).unwrap_or_else(|e| panic!("{}", e.emit_to_string(&src)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).expect("valid");
    }

    /// [r12-grime] A weathered mesh gets a twin with the weathering; the
    /// twin shows and the plain mesh hides with the setting on, the other
    /// way round with it off, and the twin goes with its mesh. Neither
    /// ever changes material type.
    #[test]
    fn twins_show_with_the_setting_and_go_with_their_mesh() {
        use bevy::render::render_resource::PrimitiveTopology;
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_plugins(AssetPlugin::default());
        app.init_asset::<StandardMaterial>().init_asset::<WeatherMaterial>().init_asset::<super::super::lights::WindowMaterial>().init_asset::<Mesh>();
        app.init_resource::<Lighting>().init_resource::<Overlays>();
        app.world_mut().resource_mut::<Lighting>().weathering = true;
        let brick = app.world_mut().resource_mut::<Assets<StandardMaterial>>().add(StandardMaterial::default());
        let grimy = app.world_mut().resource_mut::<Assets<WeatherMaterial>>().add(WeatherMaterial { base: StandardMaterial::default(), extension: Weathering { cut: CutParams::default(), params: WeatherParams::default() } });
        let mut mats = Materials::for_test(brick.clone(), Handle::default());
        mats.weather = vec![None; Surface::ALL.len()];
        mats.weather[Surface::Brick.index()] = Some(grimy.clone());
        app.insert_resource(mats);
        app.add_systems(Update, apply_weathering);
        let mesh = app.world_mut().resource_mut::<Assets<Mesh>>().add(Mesh::new(PrimitiveTopology::TriangleList, bevy::asset::RenderAssetUsages::all()));
        let wall = app.world_mut().spawn((Mesh3d(mesh.clone()), MeshMaterial3d(brick.clone()), Transform::IDENTITY, Visibility::Inherited, Weathered(Surface::Brick))).id();
        app.update();
        app.update();
        let twin = app.world().get::<Twin>(wall).expect("a twin").0;
        let w = app.world();
        assert_eq!(w.get::<MeshMaterial3d<WeatherMaterial>>(twin).map(|m| m.0.clone()), Some(grimy.clone()));
        assert_eq!(w.get::<Mesh3d>(twin).map(|m| m.0.clone()), Some(mesh.clone()));
        assert_eq!(w.get::<Visibility>(twin), Some(&Visibility::Inherited));
        assert_eq!(w.get::<Visibility>(wall), Some(&Visibility::Hidden));
        // Setting off: the plain mesh shows; materials untouched.
        app.world_mut().resource_mut::<Lighting>().weathering = false;
        app.update();
        let w = app.world();
        assert_eq!(w.get::<Visibility>(twin), Some(&Visibility::Hidden));
        assert_eq!(w.get::<Visibility>(wall), Some(&Visibility::Inherited));
        assert!(w.get::<MeshMaterial3d<StandardMaterial>>(wall).is_some() && w.get::<MeshMaterial3d<WeatherMaterial>>(wall).is_none());
        // Heat vision wants the plain mesh even with the setting on.
        app.world_mut().resource_mut::<Lighting>().weathering = true;
        app.world_mut().resource_mut::<Lighting>().heat = true;
        app.update();
        assert_eq!(app.world().get::<Visibility>(wall), Some(&Visibility::Inherited));
        app.world_mut().resource_mut::<Lighting>().heat = false;
        app.update();
        assert_eq!(app.world().get::<Visibility>(wall), Some(&Visibility::Hidden));
        // The mesh goes: its twin follows.
        app.world_mut().despawn(wall);
        app.update();
        app.update();
        assert!(app.world().get_entity(twin).is_err());
    }

    /// The edge packing the shader unpacks round-trips, and interpolates
    /// linearly across a strip of one span.
    #[test]
    fn edge_packing_round_trips_and_interpolates() {
        use nd_procgen::weathering::{pack_edge, unpack_edge};
        for (d, span) in [(0.0, 2.6), (2.6, 2.6), (1.3, 2.6), (0.0, 0.0), (7.5, 0.0)] {
            let (d2, s2) = unpack_edge(pack_edge(d, span));
            assert!((d2 - d).abs() < 1e-3 && (s2 - span).abs() < 1e-3, "{d} {span} -> {d2} {s2}");
        }
        let (a, b) = (pack_edge(0.0, 2.6), pack_edge(2.6, 2.6));
        let (mid, _) = unpack_edge((a + b) * 0.5);
        assert!((mid - 1.3).abs() < 1e-3);
    }

    /// Ground surfaces take their wear from the channel; walls by their
    /// material; glow never gets a weathering material.
    #[test]
    fn params_follow_the_surface_and_climate() {
        let p = WeatherParams::of(nd_sim::Climate::Tropical, Surface::Brick);
        assert_eq!(p.surface.x, 1.0);
        assert!(WeatherParams::of(nd_sim::Climate::Tropical, Surface::Glass).surface.x < 0.5);
        assert_eq!(WeatherParams::of(nd_sim::Climate::Desert, Surface::Paving).surface.x, 1.0);
        assert_eq!(WeatherParams::of(nd_sim::Climate::Desert, Surface::Brick).moss.w, 0.0);
        assert!(p.moss.w > 0.5);
        // [r14-photofix] Weeds and moss lose their green in the cold.
        let warm = WeatherParams::of_at(nd_sim::Climate::Volcanic, Surface::Paving, 20.0).surface.z;
        let snow = WeatherParams::of_at(nd_sim::Climate::Volcanic, Surface::Paving, -2.0).surface.z;
        assert_eq!(warm, 1.0);
        assert!(snow < 0.5 && snow >= 0.33, "{snow}");
    }
}
