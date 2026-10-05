//! Orbit camera and the cursor-to-ground projection used by the tools.

use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::ui::UiFlags;

#[derive(Component)]
pub struct OrbitCamera {
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
}

/// Where the mouse cursor hits the ground plane, in sim coordinates.
#[derive(Resource, Default)]
pub struct Cursor {
    pub ground: Option<Vec2>,
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Cursor>()
            .add_systems(Startup, spawn_camera)
            .add_systems(Update, (control_camera, project_cursor).chain().before(crate::sim::SimSet::Input));
    }
}

fn spawn_camera(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Hdr,
        Tonemapping::TonyMcMapface,
        Bloom { intensity: 0.22, low_frequency_boost: 0.6, ..default() },
        DistanceFog {
            color: Color::linear_rgb(0.03, 0.02, 0.07),
            falloff: FogFalloff::Linear { start: 700.0, end: 2600.0 },
            ..default()
        },
        OrbitCamera { target: Vec3::ZERO, yaw: 0.7, pitch: 0.85, distance: 520.0 },
        Transform::from_xyz(0.0, 400.0, 400.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

fn control_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    flags: Res<UiFlags>,
    mut q: Query<(&mut OrbitCamera, &mut Transform)>,
) {
    let Ok((mut cam, mut tf)) = q.single_mut() else { return };
    let dt = time.delta_secs();

    // Rotate with the right mouse button, pan with the middle button or shift+left.
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if buttons.pressed(MouseButton::Right) {
        cam.yaw -= motion.delta.x * 0.005;
        cam.pitch = (cam.pitch - motion.delta.y * 0.005).clamp(0.15, 1.5);
    } else if buttons.pressed(MouseButton::Middle) || (shift && buttons.pressed(MouseButton::Left)) {
        let (s, c) = cam.yaw.sin_cos();
        let right = Vec3::new(c, 0.0, -s);
        let forward = Vec3::new(s, 0.0, c);
        let k = cam.distance * 0.0016;
        cam.target -= right * motion.delta.x * k;
        cam.target += forward * motion.delta.y * k;
    }

    if !flags.pointer_over_ui {
        let lines = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 60.0,
        };
        if lines != 0.0 {
            cam.distance = (cam.distance * (1.0 - lines * 0.12)).clamp(25.0, 2500.0);
        }
    }

    // Keyboard pan and rotate.
    let mut pan = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
        pan.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
        pan.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
        pan.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
        pan.x += 1.0;
    }
    if pan != Vec2::ZERO {
        let (s, c) = cam.yaw.sin_cos();
        let right = Vec3::new(c, 0.0, -s);
        let forward = Vec3::new(-s, 0.0, -c);
        let speed = cam.distance * 0.9 * dt;
        cam.target += (right * pan.x + forward * pan.y) * speed;
    }
    if keys.pressed(KeyCode::KeyQ) {
        cam.yaw += 1.2 * dt;
    }
    if keys.pressed(KeyCode::KeyE) {
        cam.yaw -= 1.2 * dt;
    }

    let (sy, cy) = cam.yaw.sin_cos();
    let (sp, cp) = cam.pitch.sin_cos();
    let offset = Vec3::new(sy * cp, sp, cy * cp) * cam.distance;
    tf.translation = cam.target + offset;
    tf.look_at(cam.target, Vec3::Y);
}

fn project_cursor(
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<OrbitCamera>>,
    mut cursor: ResMut<Cursor>,
) {
    cursor.ground = None;
    let Ok(window) = windows.single() else { return };
    let Ok((camera, gt)) = cameras.single() else { return };
    let Some(pos) = window.cursor_position() else { return };
    let Ok(ray) = camera.viewport_to_world(gt, pos) else { return };
    let dir = ray.direction.as_vec3();
    if dir.y >= -1e-4 {
        return;
    }
    let t = -ray.origin.y / dir.y;
    let hit = ray.origin + dir * t;
    cursor.ground = Some(Vec2::new(hit.x, hit.z));
}
