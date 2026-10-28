//! [viewer] The building viewer's turntable: one building on a plain floor
//! against a neutral backdrop, filmed into an image the viewer window shows.
//!
//! The building is the city's own drawing (the viewer hands over the
//! geometry `generate_building` made, window lights marked), spawned far
//! outside the map on a render layer only the turntable camera sees, with
//! its own key, fill and rim lights, its own ambient light and fog (the
//! fog melts the floor into the backdrop), and its own copy of the window
//! glow material, so day or night and lights on or off here change nothing
//! in the city. The lights turn with the camera, as a product turntable's
//! do: the building's sunny side follows the view.
//!
//! Nothing is spawned while the viewer is closed; closing it takes the
//! stage down and frees the image.

use std::path::PathBuf;

use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::RenderLayers;
use bevy::camera::{Hdr, RenderTarget};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::{AmbientLight, CascadeShadowConfigBuilder, NotShadowCaster, NotShadowReceiver};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use bevy_egui::{egui, EguiTextureHandle, EguiUserTextures};
use nd_procgen::Geometry;

use super::lights::{self, WindowLights, WindowMaterial};
use super::{to_mesh, Materials};

/// The stage's render layer (the thumbnails use 7).
const LAYER: usize = 8;
/// Where the stage stands: far outside any map.
const STAGE: Vec3 = Vec3::new(-90_000.0, 0.0, 90_000.0);
/// The image the camera films, in pixels.
pub const IMAGE_W: u32 = 1280;
pub const IMAGE_H: u32 = 800;
/// Height over width of the image.
pub fn turntable_aspect() -> f32 {
    IMAGE_H as f32 / IMAGE_W as f32
}
/// Vertical field of view.
const FOV: f32 = 0.5;
/// Frames a new building stands before a screenshot of it is taken (the
/// first frames of a new material may still be compiling).
const SHOT_SETTLE: u32 = 12;
/// Frames the stage stands, after it is put up, before the first one.
const STAGE_SETTLE: u32 = 40;

/// The turntable, as the viewer drives it.
#[derive(Resource)]
pub struct Turntable {
    /// Put up while the viewer is open.
    pub active: bool,
    /// Camera angle round the building and above the floor, radians.
    pub yaw: f32,
    pub pitch: f32,
    /// Distance as a share of the framing distance (1 frames it).
    pub zoom: f32,
    pub auto_rotate: bool,
    /// Night lighting (moonlight, glow at full); day otherwise.
    pub night: bool,
    /// Window lights and signs on.
    pub lights: bool,
    /// The image as an egui texture, while the stage stands.
    pub texture: Option<egui::TextureId>,
    /// What is on the stage (or about to be): the viewer's key for it.
    pub key: Option<String>,
    /// The building's bounding box size (width, height, depth), metres.
    pub size: Vec3,
    /// Screenshots of the image asked for, written once the building has stood a few frames.
    pub shots: Vec<PathBuf>,
    /// Paths written since the last look, for the API's replies.
    pub written: Vec<PathBuf>,
    pending: Option<Geometry>,
    stage: Option<Stage>,
}

impl Default for Turntable {
    fn default() -> Self {
        Self { active: false, yaw: 0.6, pitch: 0.32, zoom: 1.0, auto_rotate: true, night: false, lights: true, texture: None, key: None, size: Vec3::ZERO, shots: Vec::new(), written: Vec::new(), pending: None, stage: None }
    }
}

impl Turntable {
    /// Put `geo` on the stage under `key`, unless it is already there.
    pub fn show(&mut self, key: &str, geo: Geometry) {
        if self.key.as_deref() == Some(key) && self.pending.is_none() {
            return;
        }
        self.key = Some(key.to_string());
        self.pending = Some(geo);
    }

    /// Whether `key` is what stands (or is about to).
    pub fn showing(&self, key: &str) -> bool {
        self.key.as_deref() == Some(key)
    }

    /// Back to the starting angle and framing.
    pub fn reset_view(&mut self) {
        self.yaw = 0.6;
        self.pitch = 0.32;
        self.zoom = 1.0;
    }

    /// Frames the current building has stood (0 while none).
    pub fn settled(&self) -> u32 {
        self.stage.as_ref().map_or(0, |s| s.frames.min(s.subject_frames))
    }
}

/// What stands while the viewer is open.
struct Stage {
    camera: Entity,
    image: Handle<Image>,
    key_light: Entity,
    fill: Entity,
    rim: Entity,
    floor: Entity,
    floor_mat: Handle<StandardMaterial>,
    glow_mat: Handle<WindowMaterial>,
    lit: Handle<Image>,
    building: Vec<Entity>,
    glow: Vec<Entity>,
    /// The building's middle on the stage and its bounding radius.
    centre: Vec3,
    radius: f32,
    /// Half its width across the floor (the diagonal) and half its height.
    half_flat: f32,
    half_height: f32,
    /// The lighting last put up: (night, lights).
    lit_as: Option<(bool, bool)>,
    /// Frames since the stage went up, and since this building did.
    frames: u32,
    subject_frames: u32,
}

pub struct TurntablePlugin;

impl Plugin for TurntablePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Turntable>().add_systems(Update, run_turntable.after(super::update_daylight));
    }
}

/// The backdrop and floor colour: a soft grey by day, near black by night.
fn backdrop(night: bool) -> Color {
    if night { Color::linear_rgb(0.010, 0.011, 0.018) } else { Color::linear_rgb(0.30, 0.31, 0.33) }
}

fn lit_texel(share: f32) -> Image {
    Image::new(Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }, TextureDimension::D2, vec![(share.clamp(0.0, 1.0) * 255.0) as u8], TextureFormat::R8Unorm, RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD)
}

/// Bounds of a geometry's vertices.
fn bounds(g: &Geometry) -> Option<(Vec3, Vec3)> {
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    for (_, m) in g.iter() {
        for p in &m.positions {
            let v = Vec3::from_array(*p);
            lo = lo.min(v);
            hi = hi.max(v);
        }
    }
    (lo.x <= hi.x).then_some((lo, hi))
}

/// Put the stage up or down, swap in a new building, keep the camera and
/// lights on the orbit, and take the screenshots asked for.
#[allow(clippy::too_many_arguments)]
fn run_turntable(
    mut commands: Commands,
    mut tt: ResMut<Turntable>,
    mut images: ResMut<Assets<Image>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut std_mats: ResMut<Assets<StandardMaterial>>,
    mut win_mats: ResMut<Assets<WindowMaterial>>,
    mut egui_tex: ResMut<EguiUserTextures>,
    mats: Option<Res<Materials>>,
    time: Res<Time>,
    mut fog: Query<&mut DistanceFog, With<TurntableCamera>>,
    mut bloom: Query<&mut Bloom, With<TurntableCamera>>,
    mut cams: Query<&mut Camera, With<TurntableCamera>>,
) {
    let tt = &mut *tt;
    if !tt.active {
        if let Some(s) = tt.stage.take() {
            for e in [s.camera, s.key_light, s.fill, s.rim, s.floor].into_iter().chain(s.building).chain(s.glow) {
                commands.entity(e).despawn();
            }
            egui_tex.remove_image(&s.image);
            tt.texture = None;
            // The next opening builds its building again.
            tt.key = None;
            tt.pending = None;
        }
        return;
    }
    let Some(mats) = mats else { return };
    let layer = RenderLayers::layer(LAYER);
    if tt.stage.is_none() {
        let image = images.add(Image::new_target_texture(IMAGE_W, IMAGE_H, TextureFormat::Rgba8UnormSrgb, None));
        let camera = commands
            .spawn((
                Camera3d::default(),
                Hdr,
                Tonemapping::TonyMcMapface,
                Bloom { intensity: 0.1, low_frequency_boost: 0.4, ..default() },
                Camera { order: -3, clear_color: ClearColorConfig::Custom(backdrop(tt.night)), ..default() },
                RenderTarget::Image(image.clone().into()),
                Projection::Perspective(PerspectiveProjection { fov: FOV, near: 0.5, far: 20_000.0, ..default() }),
                DistanceFog { color: backdrop(tt.night), falloff: FogFalloff::Linear { start: 400.0, end: 900.0 }, ..default() },
                AmbientLight { color: Color::WHITE, brightness: 300.0, affects_lightmapped_meshes: true },
                Transform::from_translation(STAGE + Vec3::new(0.0, 40.0, 120.0)).looking_at(STAGE, Vec3::Y),
                TurntableCamera,
                layer.clone(),
            ))
            .id();
        let light = |commands: &mut Commands, shadows: bool| commands.spawn((DirectionalLight { illuminance: 0.0, shadow_maps_enabled: shadows, ..default() }, Transform::from_translation(STAGE), layer.clone())).id();
        let key_light = light(&mut commands, true);
        let fill = light(&mut commands, false);
        let rim = light(&mut commands, false);
        let floor_mat = std_mats.add(StandardMaterial { base_color: backdrop(tt.night), perceptual_roughness: 0.92, reflectance: 0.2, ..default() });
        let floor = commands.spawn((Mesh3d(meshes.add(Circle::new(6_000.0).mesh().resolution(96))), MeshMaterial3d(floor_mat.clone()), Transform::from_translation(STAGE - Vec3::Y * 0.04).with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)), NotShadowCaster, layer.clone())).id();
        let lit = images.add(lit_texel(1.0));
        let base = std_mats.get(&mats.glow()).cloned().unwrap_or_else(|| StandardMaterial { base_color: Color::WHITE, unlit: true, ..default() });
        let glow_mat = win_mats.add(WindowMaterial { base, extension: WindowLights { lit: lit.clone(), clock: Vec4::ZERO, cut: default() } });
        tt.texture = Some(egui_tex.add_image(EguiTextureHandle::Strong(image.clone())));
        tt.stage = Some(Stage { camera, image, key_light, fill, rim, floor, floor_mat, glow_mat, lit, building: Vec::new(), glow: Vec::new(), centre: STAGE + Vec3::Y * 10.0, radius: 20.0, half_flat: 15.0, half_height: 10.0, lit_as: None, frames: 0, subject_frames: 0 });
    }
    let stage = tt.stage.as_mut().unwrap();
    stage.frames += 1;
    stage.subject_frames += 1;

    // A new building: the old one down, this one up, centred on the stage.
    if let Some(mut geo) = tt.pending.take() {
        for e in stage.building.drain(..).chain(stage.glow.drain(..)) {
            commands.entity(e).despawn();
        }
        // [r4-brick] Up close on the stage: the detail layers too (sills,
        // glazing bars, AC units), as the city draws them near the camera.
        geo.flatten_detail();
        lights::tag_windows(&mut geo, Some(0));
        let (lo, hi) = bounds(&geo).unwrap_or((Vec3::splat(-5.0), Vec3::splat(5.0)));
        let mid = (lo + hi) * 0.5;
        let offset = STAGE - Vec3::new(mid.x, 0.0, mid.z);
        stage.centre = STAGE + Vec3::new(0.0, mid.y.max(2.0), 0.0);
        stage.radius = ((hi - lo) * 0.5).length().max(6.0);
        stage.half_flat = (Vec2::new(hi.x - lo.x, hi.z - lo.z) * 0.5).length().max(4.0);
        stage.half_height = ((hi.y - lo.y.min(0.0)) * 0.5).max(3.0);
        tt.size = hi - lo;
        commands.entity(stage.floor).insert(Transform::from_translation(STAGE + Vec3::Y * (lo.y.min(0.0) - 0.04)).with_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)));
        for (s, data) in geo.iter() {
            if data.positions.is_empty() {
                continue;
            }
            let mesh = Mesh3d(meshes.add(to_mesh(data, !s.is_glow())));
            let tf = Transform::from_translation(offset);
            if s.is_glow() {
                let vis = if tt.lights { Visibility::Inherited } else { Visibility::Hidden };
                let e = commands.spawn((mesh, MeshMaterial3d(stage.glow_mat.clone()), tf, vis, NotShadowCaster, NotShadowReceiver, layer.clone())).id();
                stage.glow.push(e);
            } else {
                let e = commands.spawn((mesh, MeshMaterial3d(mats.surface(s)), tf, layer.clone())).id();
                stage.building.push(e);
            }
        }
        stage.subject_frames = 0;
        // Shadows reach over the building and its floor from any distance.
        let reach = stage.radius * (2.0 / (FOV * 0.5).sin() * 1.15 * 3.0 + 2.0);
        commands.entity(stage.key_light).insert(CascadeShadowConfigBuilder { num_cascades: 2, minimum_distance: 0.5, maximum_distance: reach, first_cascade_far_bound: reach * 0.35, ..default() }.build());
    }

    // The orbit.
    if tt.auto_rotate {
        tt.yaw = (tt.yaw + time.delta_secs() * 0.3).rem_euclid(std::f32::consts::TAU);
    }
    tt.pitch = tt.pitch.clamp(-0.05, 1.45);
    tt.zoom = tt.zoom.clamp(0.25, 4.0);
    // Framed to fill the image: the floor diagonal across its width, the
    // height (and the near half of the footprint, seen from above) up it.
    let t = (FOV * 0.5).tan();
    let aspect = IMAGE_W as f32 / IMAGE_H as f32;
    let up = stage.half_height * tt.pitch.cos() + stage.half_flat * tt.pitch.sin().abs();
    let frame = (stage.half_flat / (t * aspect)).max(up / t) * 1.12 + stage.half_flat * 0.5;
    let dist = frame * tt.zoom;
    let dir = Vec3::new(tt.yaw.sin() * tt.pitch.cos(), tt.pitch.sin(), tt.yaw.cos() * tt.pitch.cos());
    let eye = stage.centre + dir * dist;
    commands.entity(stage.camera).insert(Transform::from_translation(eye).looking_at(stage.centre, Vec3::Y));
    // The lights turn with the camera: the key from the front left and
    // above, a soft fill from the right, a rim from behind.
    let rot = Quat::from_rotation_y(tt.yaw);
    let aim = |d: Vec3| Transform::from_translation(STAGE).looking_to(rot * d, Vec3::Y);
    commands.entity(stage.key_light).insert(aim(Vec3::new(0.55, -0.75, -0.45)));
    commands.entity(stage.fill).insert(aim(Vec3::new(-0.7, -0.35, -0.3)));
    commands.entity(stage.rim).insert(aim(Vec3::new(-0.1, -0.5, 0.85)));
    // The fog fades the floor into the backdrop a little past the building.
    let fog_start = dist + stage.radius * 1.5;
    for mut f in &mut fog {
        f.color = backdrop(tt.night);
        f.falloff = FogFalloff::Linear { start: fog_start, end: fog_start + stage.radius * 6.0 + 200.0 };
        f.directional_light_color = Color::NONE;
    }
    // The city's daylight writes every bloom; this one is the turntable's.
    for mut b in &mut bloom {
        let k = if tt.night { 0.16 } else { 0.06 };
        if b.intensity != k {
            b.intensity = k;
        }
    }

    // Day or night, lights on or off.
    let want = (tt.night, tt.lights);
    if stage.lit_as != Some(want) {
        stage.lit_as = Some(want);
        let (night, on) = want;
        let (key, key_c, fill, rim, amb, amb_c) = if night {
            (1_000.0, Color::linear_rgb(0.55, 0.65, 1.0), 220.0, 1_100.0, 120.0, Color::linear_rgb(0.5, 0.45, 0.9))
        } else {
            (9_500.0, Color::linear_rgb(1.0, 0.95, 0.86), 2_600.0, 3_000.0, 420.0, Color::linear_rgb(0.85, 0.9, 1.0))
        };
        let rim_c = if night { Color::linear_rgb(0.9, 0.35, 0.9) } else { Color::linear_rgb(0.9, 0.95, 1.0) };
        commands.entity(stage.key_light).insert(DirectionalLight { illuminance: key, color: key_c, shadow_maps_enabled: true, ..default() });
        commands.entity(stage.fill).insert(DirectionalLight { illuminance: fill, color: Color::linear_rgb(0.75, 0.82, 1.0), shadow_maps_enabled: false, ..default() });
        commands.entity(stage.rim).insert(DirectionalLight { illuminance: rim, color: rim_c, shadow_maps_enabled: false, ..default() });
        commands.entity(stage.camera).insert(AmbientLight { color: amb_c, brightness: amb, affects_lightmapped_meshes: true });
        // Changed in place: a new `Camera` would forget its target's size
        // for a frame (a zero-sized cluster texture, a render error).
        for mut c in &mut cams {
            c.clear_color = ClearColorConfig::Custom(backdrop(night));
        }
        if let Some(mut m) = std_mats.get_mut(&stage.floor_mat) {
            m.base_color = backdrop(night);
        }
        // The glow as the city shows it at noon or at midnight.
        if let Some(mut m) = win_mats.get_mut(&stage.glow_mat) {
            let k = if night { 1.0 } else { 0.3 };
            m.base.base_color = Color::linear_rgb(k, k, k);
            m.extension.clock = Vec4::new(0.0, if night { 1.0 } else { 0.0 }, 0.0, 0.0);
        }
        if let Some(mut img) = images.get_mut(&stage.lit) {
            *img = lit_texel(if !on { 0.0 } else if night { 0.8 } else { 0.55 });
        }
        let vis = if on { Visibility::Inherited } else { Visibility::Hidden };
        for &e in &stage.glow {
            commands.entity(e).insert(vis);
        }
    }

    // Screenshots of the image, once the building has stood a moment.
    if !tt.shots.is_empty() && stage.frames >= STAGE_SETTLE && stage.subject_frames >= SHOT_SETTLE && tt.pending.is_none() {
        for path in std::mem::take(&mut tt.shots) {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            commands.spawn(Screenshot::image(stage.image.clone())).observe(save_to_disk(path.clone()));
            tt.written.push(path);
        }
    }
}

/// The turntable's camera.
#[derive(Component)]
pub struct TurntableCamera;
