//! [tools] Footbridges in the world, and the Intersection editor's road
//! view.
//!
//! The road view is a top-down orthographic camera over the junction being
//! edited, rendering into an image the editor shows (`ui::junction`). It
//! sees only its own render layer, which the roads, lane arrows, paths and
//! footbridges are on as well as the main one: no buildings, terrain or
//! traffic, just the roads. It has its own light on that layer, and runs
//! only while the editor is open.

use bevy::camera::visibility::RenderLayers;
use bevy::camera::{RenderTarget, ScalingMode};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy_egui::{EguiTextureHandle, EguiUserTextures};
use std::hash::{Hash, Hasher};

use super::*;
use crate::tools::ToolState;

/// The editor's render layer.
pub const JUNCTION_LAYER: usize = 6;
/// Pixels of the road view's image (shown at `ui::junction::VIEW_PX`
/// points, so sharp on high-DPI).
const VIEW_SIZE: u32 = 880;

/// Footbridge meshes.
#[derive(Component, Clone)]
pub struct FootbridgeTag;

/// Footbridges, rebuilt when the walk graph or the roads change.
pub(super) fn sync_footbridges(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, sim: Res<Sim>, assets: Res<GameAssets>, mats: Res<Materials>, mut last: Local<Option<u64>>, existing: Query<Entity, With<FootbridgeTag>>) {
    let w = &sim.world;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (w.walk.version, w.roads.version, w.walk.footbridges.len(), w.relief).hash(&mut h);
    let key = h.finish();
    if *last == Some(key) && !assets.palette_changed && !sim.full_rebuild {
        return;
    }
    *last = Some(key);
    despawn_all(&mut commands, &existing);
    if w.walk.footbridges.is_empty() {
        return;
    }
    let geo = nd_procgen::footbridge::footbridges(w, &assets.palette);
    for e in spawn_geometry(&mut commands, &mut meshes, &mats, &geo, FootbridgeTag) {
        commands.entity(e).insert(RenderLayers::from_layers(&[0, JUNCTION_LAYER]));
    }
}

/// Put the roads (and what the editor shows with them) on the editor's
/// layer too.
#[allow(clippy::type_complexity)]
pub(super) fn tag_road_layers(mut commands: Commands, q: Query<Entity, (Or<(With<RoadsTag>, With<LaneArrowsTag>, With<PathsTag>, With<FootbridgeTag>)>, Without<RenderLayers>)>) {
    for e in &q {
        commands.entity(e).insert(RenderLayers::from_layers(&[0, JUNCTION_LAYER]));
    }
}

/// The road view's camera, light and image.
#[derive(Resource, Default)]
pub struct JunctionCam {
    camera: Option<Entity>,
    light: Option<Entity>,
    image: Option<Handle<Image>>,
    texture: Option<bevy_egui::egui::TextureId>,
    framed: Option<(Vec2, f32)>,
}

/// Run the road view while the editor is open.
pub(super) fn sync_junction_camera(mut commands: Commands, mut cam: ResMut<JunctionCam>, mut tools: ResMut<ToolState>, sim: Res<Sim>, mut images: ResMut<Assets<Image>>, mut egui_tex: ResMut<EguiUserTextures>, mut transforms: Query<(&mut Transform, &mut Projection)>) {
    let Some(state) = tools.junction_editor.as_mut() else {
        for e in [cam.camera.take(), cam.light.take()].into_iter().flatten() {
            commands.entity(e).despawn();
        }
        cam.framed = None;
        return;
    };
    if cam.image.is_none() {
        let image = images.add(Image::new_target_texture(VIEW_SIZE, VIEW_SIZE, TextureFormat::Rgba8UnormSrgb, None));
        cam.texture = Some(egui_tex.add_image(EguiTextureHandle::Strong(image.clone())));
        cam.image = Some(image);
    }
    state.texture = cam.texture;
    let w = &sim.world;
    // The road surface height there, so the camera sits well above it.
    let z = w.surface_height(state.at);
    let centre = Vec3::new(state.at.x, z, state.at.y);
    let eye = Transform::from_translation(centre + Vec3::Y * 400.0).looking_at(centre, Vec3::NEG_Z);
    let projection = Projection::Orthographic(OrthographicProjection { scaling_mode: ScalingMode::Fixed { width: state.extent, height: state.extent }, near: 0.0, far: 900.0, ..OrthographicProjection::default_3d() });
    let layer = RenderLayers::layer(JUNCTION_LAYER);
    match cam.camera {
        Some(e) => {
            if cam.framed != Some((state.at, state.extent)) {
                if let Ok((mut tf, mut proj)) = transforms.get_mut(e) {
                    *tf = eye;
                    *proj = projection;
                }
            }
        }
        None => {
            let Some(image) = cam.image.clone() else { return };
            let e = commands
                .spawn((
                    Camera3d::default(),
                    Camera { order: -8, clear_color: ClearColorConfig::Custom(Color::linear_rgb(0.006, 0.008, 0.016)), ..default() },
                    RenderTarget::Image(image.into()),
                    projection,
                    eye,
                    layer.clone(),
                ))
                .id();
            cam.camera = Some(e);
            let light = commands.spawn((DirectionalLight { illuminance: 9_000.0, shadow_maps_enabled: false, ..default() }, Transform::from_translation(centre).looking_to(Vec3::new(-0.3, -1.0, -0.2), Vec3::Y), layer)).id();
            cam.light = Some(light);
        }
    }
    cam.framed = Some((state.at, state.extent));
}
