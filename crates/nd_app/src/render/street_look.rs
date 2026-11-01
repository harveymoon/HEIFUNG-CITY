//! [look] The street at eye level: lamps that light the road at night, roads
//! that go dark and glossy in the rain, and how far you see (and in what
//! colour) by climate, weather and hour.
//!
//! - **Lamps.** Every lamp post records its lantern (`Geometry::lamps`); at
//!   night a small pool of real point lights (no shadows, clustered) is hung
//!   on the lamps in view nearest the camera, fading out at the pool's edge
//!   so moving the camera does not pop them (none at Low). Every lamp also
//!   throws a cheap pool of light on the ground: an additive disc under the
//!   lantern, one merged mesh per patch of the map, that fades in where the
//!   real lights stop, so a city seen from far off is lit street by street
//!   instead of in a small ring round the middle of the view.
//! - **Wet roads.** While it rains the asphalt and paving darken and lose
//!   most of their roughness (the roughness maps keep their variation, so
//!   worn patches stay dull and the smooth ones read as puddles). They dry
//!   slowly after the rain. Snow climates stay dry.
//! - **Fog.** The haze takes a climate's tint (dusty in the desert, cold
//!   and pale in the arctic, wet green-grey in the tropics, sulphur grey on
//!   volcanic maps) and closes in under rain, snow and at night in humid
//!   climates; the falloff follows the camera (walk view sees less).

use bevy::asset::uuid_handle;
use bevy::camera::primitives::{Frustum, Sphere};
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::math::Vec3A;
use bevy::pbr::{DistanceFog, ExtendedMaterial, FogFalloff, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::{Shader, ShaderRef};
use nd_procgen::{MeshData, Surface};
use nd_sim::Climate;
use std::collections::HashMap;

use super::look::Quality;
use super::{sky, smoothstep, to_mesh, Lighting, Materials, RenderStats};
use crate::camera::{CameraMode, OrbitCamera};
use crate::sim::Sim;
use crate::tools::{Depth, ToolState};

/// The lamp heads of the street network (surface layer), as last built.
#[derive(Resource, Default)]
pub(super) struct StreetLamps {
    pub lamps: Vec<Vec3>,
    /// Bumped whenever `lamps` is rebuilt (the ground pools follow it).
    pub version: u64,
    /// Where the real lights end, metres from the eye: the ground pools
    /// fade in over this range. `None`: every lamp in view has a real
    /// light (or none is on), and the pools stay dark.
    pub reach: Option<(f32, f32)>,
}

#[derive(Component)]
pub(super) struct LampLight;

/// Most real lights at once, by tier.
pub(super) fn lamp_pool(tier: Quality) -> usize {
    match tier {
        Quality::High => 40,
        Quality::Medium => 18,
        Quality::Low => 0,
    }
}
/// Farthest a lamp gets a real light, metres from the focus (the eye, for
/// the street lamps).
const LAMP_REACH: f32 = 320.0;
/// Light of one lamp (lumens; the scene's sun is 11 klx by day, the moon
/// 650 lx, so a lamp has to be bright to show under it).
const LAMP_LUMENS: f32 = 2.6e6;
const LAMP_RANGE: f32 = 30.0;
/// Sodium-white.
const LAMP_RGB: [f32; 3] = [1.0, 0.72, 0.45];

/// The point the lamps gather round: the walker's eye, or the orbit
/// camera's target (what the player is looking at), pulled towards the eye
/// when it is low.
pub(super) fn lamp_focus(mode: CameraMode, eye: Vec3, cam: &OrbitCamera) -> Vec3 {
    match mode {
        CameraMode::Walk | CameraMode::Drive => eye,
        CameraMode::Orbit => {
            let low = 1.0 - smoothstep(30.0, 160.0, eye.y - cam.target.y);
            cam.target.lerp(eye, low * 0.5)
        }
    }
}

/// How much night it is for the lamps, 0..1.
pub(super) fn lamp_night(daylight: f32) -> f32 {
    1.0 - smoothstep(0.2, 0.6, daylight)
}

/// The `n` lamps nearest `focus` within reach, nearest first, with their
/// distance.
pub(super) fn nearest_lamps(lamps: &[Vec3], focus: Vec3, n: usize, out: &mut Vec<(f32, Vec3)>) {
    out.clear();
    if n == 0 {
        return;
    }
    out.extend(lamps.iter().map(|p| (p.distance(focus), *p)).filter(|(d, _)| *d < LAMP_REACH));
    if out.len() > n {
        out.select_nth_unstable_by(n - 1, |a, b| a.0.total_cmp(&b.0));
        out.truncate(n);
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
}

/// The `n` lamps nearest the eye among those `visible` (their light, not
/// just the lantern, in the view), within reach, nearest first, with their
/// distance. [w1-render] Picked round the orbit target instead, zoomed out
/// the lit lamps were a small ring in the middle of the view.
pub(super) fn lamps_in_view(lamps: &[Vec3], eye: Vec3, n: usize, visible: impl Fn(Vec3) -> bool, out: &mut Vec<(f32, Vec3)>) {
    out.clear();
    if n == 0 {
        return;
    }
    out.extend(lamps.iter().map(|p| (p.distance(eye), *p)).filter(|(d, p)| *d < LAMP_REACH && visible(*p)));
    if out.len() > n {
        out.select_nth_unstable_by(n - 1, |a, b| a.0.total_cmp(&b.0));
        out.truncate(n);
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
}

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_lamp_lights(
    mut commands: Commands,
    mut lamps: ResMut<StreetLamps>,
    look: Res<sky::SkyLook>,
    stats: Res<RenderStats>,
    lighting: Res<Lighting>,
    cams: Query<(&GlobalTransform, Option<&Frustum>), With<OrbitCamera>>,
    mut pool: Query<(&mut PointLight, &mut Transform, &mut Visibility), With<LampLight>>,
    mut near: Local<Vec<(f32, Vec3)>>,
    mut spawned: Local<usize>,
) {
    let want = lamp_pool(stats.graphics.tier);
    if *spawned < want {
        for _ in *spawned..want {
            commands.spawn((
                PointLight { color: Color::linear_rgb(LAMP_RGB[0], LAMP_RGB[1], LAMP_RGB[2]), intensity: 0.0, range: LAMP_RANGE, radius: 0.25, shadow_maps_enabled: false, ..default() },
                super::offstage(),
                Visibility::Hidden,
                NotShadowCaster,
                LampLight,
            ));
        }
        *spawned = want;
        return;
    }
    let night = lamp_night(look.daylight);
    let on = night > 0.01 && !lighting.heat;
    let Some((tf, frustum)) = cams.iter().next() else { return };
    if on {
        // A lamp counts as in view while its light could reach into it.
        let seen = |p: Vec3| frustum.is_none_or(|f| f.intersects_sphere(&Sphere { center: Vec3A::from(p), radius: LAMP_RANGE }, true));
        lamps_in_view(&lamps.lamps, tf.translation(), want, seen, &mut near);
    } else {
        near.clear();
    }
    // Fade the farthest of the pool out, so a lamp entering or leaving it
    // eases in rather than pops.
    let edge = near.last().map(|n| n.0).unwrap_or(0.0).max(30.0);
    // The ground pools take over where the real lights fade: from the
    // start of that fade when the pool is full, everywhere when there is
    // no pool (Low), nowhere when every lamp in view has its light.
    let reach = if !on {
        None
    } else if want == 0 {
        Some((0.0, 0.0))
    } else if near.len() >= want {
        Some((edge * 0.75, edge * 1.001))
    } else {
        Some((LAMP_REACH * 0.9, LAMP_REACH))
    };
    if lamps.reach != reach {
        lamps.reach = reach;
    }
    let mut lit = near.iter();
    for (mut light, mut ltf, mut vis) in &mut pool {
        match lit.next() {
            Some((d, p)) => {
                let fade = 1.0 - smoothstep(edge * 0.75, edge * 1.001, *d) * if near.len() >= want { 1.0 } else { 0.0 };
                let k = LAMP_LUMENS * night * fade.max(0.0);
                if (light.intensity - k).abs() > k * 0.01 + 1.0 {
                    light.intensity = k;
                }
                if ltf.translation != *p {
                    ltf.translation = *p;
                }
                if *vis != Visibility::Inherited {
                    *vis = Visibility::Inherited;
                }
            }
            None => {
                if *vis != Visibility::Hidden {
                    *vis = Visibility::Hidden;
                }
            }
        }
    }
}

// ── lamp pools: every lamp's light on the ground, cheaply ────────────────────

pub(super) const POOL_SHADER: Handle<Shader> = uuid_handle!("3c5e8a1f-7b20-4d6e-9f13-a8c4b2e6d071");

/// The pool's colour (base colour times the vertex colour: lamp tint and
/// falloff), faded in by the distance from the eye where the real lights
/// stop, and added to what is behind it (alpha 0: `AlphaMode::Add`).
const POOL_SHADER_SRC: &str = r#"
#import bevy_pbr::{
    pbr_fragment::pbr_input_from_standard_material,
    forward_io::{VertexOutput, FragmentOutput},
    mesh_view_bindings::view,
}

// x, y: the fade in, metres from the eye; z: strength.
@group(#{MATERIAL_BIND_GROUP}) @binding(100) var<uniform> pool_fade: vec4<f32>;
// x, y: the distance fog's start and end (the pools fade into it as the
// lit scene does; fog itself would add its colour, the pools being added).
@group(#{MATERIAL_BIND_GROUP}) @binding(101) var<uniform> pool_fog: vec4<f32>;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    let pbr_input = pbr_input_from_standard_material(in, is_front);
    let d = distance(in.world_position.xyz, view.world_position);
    var k = pool_fade.z;
    if pool_fade.y > pool_fade.x {
        k = k * smoothstep(pool_fade.x, pool_fade.y, d);
    }
    if pool_fog.y > pool_fog.x {
        k = k * clamp((pool_fog.y - d) / (pool_fog.y - pool_fog.x), 0.0, 1.0);
    }
    var out: FragmentOutput;
    out.color = vec4(pbr_input.material.base_color.rgb * k, 0.0);
    return out;
}
"#;

/// The ground pools' material: an unlit additive disc.
pub(super) type PoolMaterial = ExtendedMaterial<StandardMaterial, LampPools>;

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone)]
pub(super) struct LampPools {
    /// x, y: the fade in (metres from the eye); z: strength (night).
    #[uniform(100)]
    pub fade: Vec4,
    /// x, y: the distance fog's linear start and end.
    #[uniform(101)]
    pub fog: Vec4,
}

impl MaterialExtension for LampPools {
    fn fragment_shader() -> ShaderRef {
        POOL_SHADER.into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
}

pub(super) fn add_pool_shader(mut shaders: ResMut<Assets<Shader>>) {
    let _ = shaders.insert(POOL_SHADER.id(), Shader::from_wgsl(POOL_SHADER_SRC, "nd_app/lamp_pools.wgsl"));
}

#[derive(Component)]
pub(super) struct LampPoolTag;

#[derive(Default)]
pub(super) struct PoolState {
    version: Option<u64>,
    material: Option<Handle<PoolMaterial>>,
    applied: Option<[i32; 5]>,
}

/// Radius of a lamp's pool on the ground, metres.
pub(super) const POOL_R: f32 = 10.0;
/// Side of the patches the pools are merged into (one mesh each, so the
/// patches out of view are culled).
const POOL_PATCH: f32 = 300.0;
/// The falloff from the middle out: (radius share, brightness).
const POOL_RINGS: [(f32, f32); 3] = [(0.3, 0.55), (0.62, 0.18), (1.0, 0.0)];
const POOL_SIDES: usize = 10;
/// Pool brightness at full night, against the lamp's tint.
const POOL_STRENGTH: f32 = 0.2;
/// Lantern over the street's datum, for the ground under it (7 m posts).
const LANTERN_UP: f32 = 6.8;

/// The pools of `lamps`, each vertex set on the ground at `ground(p,
/// lantern)` (the surface under the lamp nearest its lantern: the deck of
/// a viaduct, the street under it) a little over the kerb top.
pub(super) fn pool_mesh(lamps: &[Vec3], ground: impl Fn(Vec2, f32) -> f32) -> MeshData {
    let mut m = MeshData::default();
    let lift = nd_procgen::depth::KERB_H + 0.04;
    for l in lamps {
        let c = Vec2::new(l.x, l.z);
        let hint = l.y - LANTERN_UP;
        let base = m.positions.len() as u32;
        let push = |p: Vec2, k: f32, m: &mut MeshData| {
            m.positions.push([p.x, ground(p, hint) + lift, p.y]);
            m.normals.push([0.0, 1.0, 0.0]);
            m.colors.push([LAMP_RGB[0] * k, LAMP_RGB[1] * k, LAMP_RGB[2] * k, 1.0]);
            m.uvs.push([0.0, 0.0]);
        };
        push(c, 1.0, &mut m);
        for (r, k) in POOL_RINGS {
            for i in 0..POOL_SIDES {
                let a = i as f32 / POOL_SIDES as f32 * std::f32::consts::TAU;
                push(c + Vec2::new(a.cos(), a.sin()) * POOL_R * r, k, &mut m);
            }
        }
        let n = POOL_SIDES as u32;
        let ring = |r: u32, i: u32| base + 1 + r * n + i % n;
        for i in 0..n {
            // Wound to face up.
            m.indices.extend_from_slice(&[base, ring(0, i + 1), ring(0, i)]);
            for r in 0..(POOL_RINGS.len() as u32 - 1) {
                m.indices.extend_from_slice(&[ring(r, i), ring(r, i + 1), ring(r + 1, i + 1), ring(r, i), ring(r + 1, i + 1), ring(r + 1, i)]);
            }
        }
    }
    m
}

/// Rebuild the ground pools when the street lamps change, and fade them
/// with the night and the real lights' reach. Hidden by day, in heat
/// vision and underground.
#[allow(clippy::too_many_arguments)]
pub(super) fn sync_lamp_pools(
    mut commands: Commands,
    sim: Res<Sim>,
    lamps: Res<StreetLamps>,
    look: Res<sky::SkyLook>,
    lighting: Res<Lighting>,
    tools: Res<ToolState>,
    fogs: Query<&DistanceFog, With<OrbitCamera>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<PoolMaterial>>,
    mut pools: Query<(Entity, &mut Visibility), With<LampPoolTag>>,
    mut state: Local<PoolState>,
) {
    let material = state
        .material
        .get_or_insert_with(|| {
            materials.add(PoolMaterial {
                base: StandardMaterial { base_color: Color::WHITE, unlit: true, alpha_mode: AlphaMode::Add, fog_enabled: false, double_sided: true, cull_mode: None, ..default() },
                extension: LampPools { fade: Vec4::ZERO, fog: Vec4::ZERO },
            })
        })
        .clone();
    if state.version != Some(lamps.version) {
        state.version = Some(lamps.version);
        for (e, _) in &pools {
            commands.entity(e).despawn();
        }
        let w = &sim.world;
        let mut patches: HashMap<(i32, i32), Vec<Vec3>> = HashMap::new();
        for l in &lamps.lamps {
            patches.entry(((l.x / POOL_PATCH).floor() as i32, (l.z / POOL_PATCH).floor() as i32)).or_default().push(*l);
        }
        let mut keys: Vec<_> = patches.keys().copied().collect();
        keys.sort_unstable();
        for k in keys {
            let data = pool_mesh(&patches[&k], |p, hint| w.surface_height_near(nd_sim::Vec2::new(p.x, p.y), hint));
            commands.spawn((Mesh3d(meshes.add(to_mesh(&data, false))), MeshMaterial3d(material.clone()), Transform::IDENTITY, Visibility::Hidden, NotShadowCaster, NotShadowReceiver, LampPoolTag));
        }
        state.applied = None;
        return;
    }
    let night = lamp_night(look.daylight);
    let show = night > 0.01 && !lighting.heat && tools.depth != Depth::Underground && lamps.reach.is_some();
    let want = if show { Visibility::Inherited } else { Visibility::Hidden };
    for (_, mut v) in &mut pools {
        if *v != want {
            *v = want;
        }
    }
    if !show {
        return;
    }
    let (start, end) = lamps.reach.unwrap_or((0.0, 0.0));
    let (f0, f1) = match fogs.iter().next().map(|f| f.falloff.clone()) {
        Some(FogFalloff::Linear { start, end }) => (start, end),
        _ => (0.0, 0.0),
    };
    let key = [start.round() as i32, end.round() as i32, (night * 64.0).round() as i32, (f0 / 10.0).round() as i32, (f1 / 10.0).round() as i32];
    if state.applied == Some(key) {
        return;
    }
    state.applied = Some(key);
    if let Some(mut m) = materials.get_mut(&material) {
        m.extension.fade = Vec4::new(key[0] as f32, key[1] as f32, POOL_STRENGTH * key[2] as f32 / 64.0, 0.0);
        m.extension.fog = Vec4::new(key[3] as f32 * 10.0, key[4] as f32 * 10.0, 0.0, 0.0);
    }
}

// ── wet roads ───────────────────────────────────────────────────────────────

/// The street surfaces that get wet.
const WET_SURFACES: [Surface; 5] = [Surface::Asphalt, Surface::WornAsphalt, Surface::Paving, Surface::WornPaving, Surface::Tactile];
/// Seconds (real) to soak through at full rain, and to dry.
const WET_UP_S: f32 = 12.0;
const DRY_S: f32 = 45.0;

/// How a surface looks at wetness `wet` (0..1): its base colour multiplier
/// and its roughness multiplier (the maps hold the absolute roughness).
pub(super) fn wet_look(wet: f32) -> (f32, f32) {
    let wet = wet.clamp(0.0, 1.0);
    (1.0 - 0.45 * wet, 1.0 - 0.78 * wet)
}

pub(super) fn sync_wet_roads(sim: Res<Sim>, time: Res<Time>, lighting: Res<Lighting>, mats: Res<Materials>, mut materials: ResMut<Assets<StandardMaterial>>, mut wet: Local<f32>, mut applied: Local<Option<i32>>) {
    let rain = super::climate::precip_shown(&sim.world, &lighting).filter(|(k, _)| *k == "rain").map(|(_, i)| i).unwrap_or(0.0);
    let dt = time.delta_secs().min(0.25);
    *wet = if rain > *wet { (*wet + dt / WET_UP_S).min(rain.max(0.35)) } else { (*wet - dt / DRY_S).max(rain) };
    // [show-weather] Weather hidden: the streets dry at once.
    if !lighting.show_weather {
        *wet = 0.0;
    }
    let key = (*wet * 48.0).round() as i32;
    if *applied == Some(key) {
        return;
    }
    *applied = Some(key);
    let (dark, rough) = wet_look(key as f32 / 48.0);
    for s in WET_SURFACES {
        if let Some(mut m) = materials.get_mut(&mats.surface(s)) {
            m.base_color = Color::linear_rgb(dark, dark, dark);
            m.perceptual_roughness = rough;
        }
    }
}

// ── fog by climate, weather and hour ────────────────────────────────────────

/// A climate's haze: its tint (relative; luminance is kept) and how far
/// one sees in clear weather against the temperate default.
pub(super) fn climate_haze(c: Climate) -> ([f32; 3], f32) {
    match c {
        Climate::Temperate => ([1.0, 1.0, 1.0], 1.0),
        Climate::Desert => ([1.18, 1.0, 0.78], 0.9),
        Climate::Tropical => ([0.92, 1.04, 0.98], 0.78),
        Climate::Arctic => ([0.9, 0.98, 1.14], 0.86),
        Climate::Volcanic => ([1.04, 1.0, 0.9], 0.72),
    }
}

fn lum(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// The fog colour `update_daylight` chose, tinted for the climate (most by
/// day; the city's glow owns the night) and greyed under rain or snow.
pub(super) fn tint_fog(fog: [f32; 3], climate: Climate, daylight: f32, precip: f32) -> [f32; 3] {
    let (tint, _) = climate_haze(climate);
    let l = lum(fog);
    let tl = lum(tint).max(1e-4);
    let k = 0.25 + 0.75 * daylight;
    let mut out = [0.0; 3];
    for i in 0..3 {
        let tinted = fog[i] * (1.0 + (tint[i] / tl - 1.0) * k);
        // Rain and snow: towards a flat grey of the same brightness, a
        // little darker.
        out[i] = (tinted + (l - tinted) * 0.6 * precip) * (1.0 - 0.15 * precip * daylight);
    }
    out
}

/// How far the fog lets you see, against the clear default: climate,
/// rain or snow, and a humid night's mist.
pub(super) fn visibility(climate: Climate, daylight: f32, precip: f32) -> f32 {
    let (_, clear) = climate_haze(climate);
    let humid = matches!(climate, Climate::Tropical | Climate::Temperate) as u8 as f32;
    clear * (1.0 - 0.5 * precip) * (1.0 - 0.15 * humid * (1.0 - daylight))
}

/// The falloff for the camera mode, scaled by [`visibility`], with the
/// orbit camera `distance` metres from what it looks at. Walk view sees
/// less (the far ground is not drawn at street level).
pub(super) fn fog_falloff(mode: CameraMode, vis: f32, distance: f32) -> (f32, f32) {
    let sight = nd_sim::landscape::SIGHT;
    match mode {
        CameraMode::Walk => (120.0 * vis, 900.0 * vis),
        // [r5-drive] At a car's speed the street ahead needs more reach.
        CameraMode::Drive => (220.0 * vis, 1500.0 * vis),
        // The far end stays at the edge of what is drawn, a little nearer
        // in thick weather. [w1-render] Pulled back as the camera does:
        // the city under a camera zoomed far out sits 2-3 km away, and
        // with the fog fixed round the eye it was drowned in haze. Up
        // close the haze is as it was; zoomed right out it starts about
        // where the city does and is solid well past the map's edge.
        CameraMode::Orbit => {
            let out = smoothstep(250.0, 2500.0, distance);
            (500.0 * vis + 0.9 * distance * out, sight * (0.75 + 0.25 * vis).min(1.0) + 1.1 * distance * out)
        }
    }
}

/// `(start, end)` kept solid before the edge of the far ground, `edge`
/// metres from the eye at its nearest: past it is only the sky dome.
pub(super) fn fog_short_of_edge((start, end): (f32, f32), edge: f32) -> (f32, f32) {
    let cut = end.min(edge.max(nd_sim::landscape::SIGHT));
    if cut < end {
        (start.min(cut - 600.0), cut)
    } else {
        (start, end)
    }
}

pub(super) fn sync_fog_reach(sim: Res<Sim>, look: Res<sky::SkyLook>, mode: Res<CameraMode>, lighting: Res<Lighting>, cams: Query<(&OrbitCamera, &GlobalTransform)>, mut fogs: Query<&mut DistanceFog, With<OrbitCamera>>) {
    if lighting.heat {
        return;
    }
    let w = &sim.world;
    let precip = super::climate::precip_shown(w, &lighting).map(|(_, i)| i).unwrap_or(0.0);
    let vis = visibility(w.landscape.params.climate, look.daylight, precip);
    let Some((cam, tf)) = cams.iter().next() else { return };
    let eye = tf.translation();
    // [horizon] The opaque far ground's edge (the ridges and the sky dome's
    // fog-coloured horizon lie past it).
    let reach = *nd_procgen::horizon::GROUND_REACH.last().unwrap_or(&nd_sim::landscape::FAR_GROUND);
    let edge = (reach - eye.x.abs().max(eye.z.abs())).hypot(eye.y.max(0.0)) - 150.0;
    let (start, end) = fog_short_of_edge(fog_falloff(*mode, vis, cam.distance), edge);
    for mut fog in &mut fogs {
        let differs = match fog.falloff {
            FogFalloff::Linear { start: s, end: e } => (s - start).abs() > 1.0 || (e - end).abs() > 1.0,
            _ => true,
        };
        if differs {
            fog.falloff = FogFalloff::Linear { start, end };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_lamps_picks_the_closest_in_reach() {
        let lamps: Vec<Vec3> = (0..50).map(|i| Vec3::new(i as f32 * 10.0, 7.0, 0.0)).collect();
        let mut out = Vec::new();
        nearest_lamps(&lamps, Vec3::new(105.0, 1.7, 0.0), 4, &mut out);
        assert_eq!(out.len(), 4);
        let xs: Vec<f32> = out.iter().map(|(_, p)| p.x).collect();
        assert!(xs[0] == 100.0 || xs[0] == 110.0, "{xs:?}");
        assert!(out.windows(2).all(|w| w[0].0 <= w[1].0));
        assert!(xs.iter().all(|x| (80.0..=130.0).contains(x)));
        // Out of reach: none.
        nearest_lamps(&lamps, Vec3::new(5000.0, 0.0, 0.0), 4, &mut out);
        assert!(out.is_empty());
        // Low tier: no pool.
        nearest_lamps(&lamps, Vec3::ZERO, lamp_pool(Quality::Low), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn lamps_in_view_are_the_nearest_the_eye_and_seen() {
        let lamps: Vec<Vec3> = (0..50).map(|i| Vec3::new(i as f32 * 10.0, 7.0, 0.0)).collect();
        let mut out = Vec::new();
        // Only x >= 200 is in view: the nearest seen to an eye at 0.
        lamps_in_view(&lamps, Vec3::new(0.0, 50.0, 0.0), 3, |p| p.x >= 200.0, &mut out);
        let xs: Vec<f32> = out.iter().map(|(_, p)| p.x).collect();
        assert_eq!(xs, vec![200.0, 210.0, 220.0]);
        // Out of reach of the eye: none.
        lamps_in_view(&lamps, Vec3::new(0.0, 2000.0, 0.0), 3, |_| true, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn lamp_pools_lie_on_the_ground_and_face_up() {
        let lamps = [Vec3::new(0.0, 6.8, 0.0), Vec3::new(40.0, 16.8, 0.0)];
        // A slope: the pool follows it vertex by vertex.
        let m = pool_mesh(&lamps, |p, hint| hint + p.x * 0.1);
        let per = 1 + POOL_RINGS.len() * POOL_SIDES;
        assert_eq!(m.positions.len(), 2 * per);
        let lift = nd_procgen::depth::KERB_H + 0.04;
        for (i, p) in m.positions.iter().enumerate() {
            let hint = if i < per { 0.0 } else { 10.0 };
            assert!((p[1] - (hint + p[0] * 0.1 + lift)).abs() < 1e-4, "{p:?}");
            let c = if i < per { 0.0 } else { 40.0 };
            assert!(((p[0] - c).powi(2) + p[2].powi(2)).sqrt() <= POOL_R + 1e-3);
        }
        // Bright in the middle, black at the rim.
        assert!(m.colors[0][0] > 0.9 && m.colors[per - 1][0] == 0.0);
        for t in m.indices.chunks(3) {
            let [a, b, c] = [t[0], t[1], t[2]].map(|i| Vec3::from(m.positions[i as usize]));
            assert!((b - a).cross(c - a).y > 0.0, "a triangle faces down");
        }
    }

    #[test]
    fn lamps_are_off_by_day_and_on_at_night() {
        assert_eq!(lamp_night(1.0), 0.0);
        assert_eq!(lamp_night(0.0), 1.0);
    }

    #[test]
    fn wet_roads_are_darker_and_glossier() {
        assert_eq!(wet_look(0.0), (1.0, 1.0));
        let (dark, rough) = wet_look(1.0);
        assert!(dark < 0.7 && rough < 0.3);
    }

    #[test]
    fn fog_closes_in_under_rain_and_keeps_its_brightness() {
        for c in Climate::ALL {
            assert!(visibility(c, 1.0, 1.0) < visibility(c, 1.0, 0.0));
            let fog = [0.5, 0.55, 0.6];
            let t = tint_fog(fog, c, 1.0, 0.0);
            assert!((lum(t) - lum(fog)).abs() < 0.06, "{c:?}: {t:?}");
        }
        assert_eq!(tint_fog([0.3, 0.3, 0.3], Climate::Temperate, 1.0, 0.0), [0.3, 0.3, 0.3]);
        let (s, e) = fog_falloff(CameraMode::Walk, 1.0, 0.0);
        assert!(s < e && e < fog_falloff(CameraMode::Orbit, 1.0, 100.0).1);
    }

    /// [w1-render] Zoomed far out, the fog starts past the city under the
    /// camera rather than in front of it; up close it is as it was.
    #[test]
    fn fog_draws_back_with_the_camera() {
        let sight = nd_sim::landscape::SIGHT;
        assert_eq!(fog_falloff(CameraMode::Orbit, 1.0, 120.0), (500.0, sight));
        let (s, e) = fog_falloff(CameraMode::Orbit, 1.0, 2500.0);
        assert!(s >= 2500.0 && e > s + 2000.0, "{s} {e}");
        // Never past the far ground's edge.
        assert_eq!(fog_short_of_edge((2750.0, 5350.0), 3400.0), (2750.0, 3400.0));
        assert_eq!(fog_short_of_edge((2750.0, 5350.0), 1000.0), (sight - 600.0, sight));
        // It recedes steadily as the camera pulls back.
        let mut last = fog_falloff(CameraMode::Orbit, 0.6, 0.0);
        for d in (100..=2500).step_by(100) {
            let now = fog_falloff(CameraMode::Orbit, 0.6, d as f32);
            assert!(now.0 >= last.0 && now.1 >= last.1 && now.0 < now.1);
            last = now;
        }
    }
}
