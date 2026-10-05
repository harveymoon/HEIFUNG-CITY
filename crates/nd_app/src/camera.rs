//! Orbit camera and the cursor-to-ground projection used by the tools.

use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use crate::sim::Sim;
use crate::ui::UiFlags;
use nd_sim::Tier;
use rand::seq::IndexedRandom;

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
        Bloom { intensity: 0.08, low_frequency_boost: 0.4, ..default() },
        DistanceFog {
            color: Color::linear_rgb(0.03, 0.02, 0.07),
            falloff: FogFalloff::Linear { start: 500.0, end: 2600.0 },
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
    sim: Res<Sim>,
    mut q: Query<(&mut OrbitCamera, &mut Transform)>,
) {
    let Ok((mut cam, mut tf)) = q.single_mut() else { return };
    let dt = time.delta_secs();

    // F6: focus the busiest junction, close enough to watch the driving.
    if keys.just_pressed(KeyCode::F6) {
        let roads = &sim.world.roads;
        let cars: Vec<Vec2> = sim.world.traffic.cars.values().map(|c| c.pos).collect();
        let mut best: Option<(usize, Vec2)> = None;
        for (i, node) in roads.nodes.iter().enumerate() {
            let alive = node.segments.iter().filter(|&&s| roads.segment(s).alive).count();
            if alive < 3 {
                continue;
            }
            let n = cars.iter().filter(|p| p.distance(node.pos) < 60.0).count();
            if best.map_or(true, |(bn, _)| n > bn) {
                best = Some((n, node.pos));
            }
            let _ = i;
        }
        if let Some((_, p)) = best {
            cam.target = Vec3::new(p.x, 0.0, p.y);
            cam.distance = 75.0;
            cam.pitch = 0.75;
        }
    }
    // F7 / F8 / F9: jump to a random low, middle or high tier building.
    for (key, tier) in [(KeyCode::F7, Tier::Low), (KeyCode::F8, Tier::Mid), (KeyCode::F9, Tier::High)] {
        if keys.just_pressed(key) {
            let candidates: Vec<_> = sim.world.buildings.values().filter(|b| b.tier == tier && b.land_use != nd_sim::LandUse::Park).filter_map(|b| sim.world.lots.get(b.lot)).map(|l| l.center).collect();
            if let Some(c) = candidates.choose(&mut rand::rng()) {
                cam.target = Vec3::new(c.x, 0.0, c.y);
                cam.distance = 110.0;
                cam.pitch = 0.55;
            }
        }
    }

    // Rotate with the right mouse button, pan with the middle button or shift+left.
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    if buttons.pressed(MouseButton::Right) {
        cam.yaw -= motion.delta.x * 0.005;
        cam.pitch = (cam.pitch + motion.delta.y * 0.005).clamp(0.15, 1.5);
    } else if buttons.pressed(MouseButton::Middle) || (shift && buttons.pressed(MouseButton::Left)) {
        let (s, c) = cam.yaw.sin_cos();
        let right = Vec3::new(c, 0.0, -s);
        let forward = Vec3::new(s, 0.0, c);
        let k = cam.distance * 0.0016;
        cam.target -= right * motion.delta.x * k;
        cam.target -= forward * motion.delta.y * k;
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
    if keys.pressed(KeyCode::BracketLeft) {
        cam.distance = (cam.distance * (1.0 - 1.5 * dt)).max(25.0);
    }
    if keys.pressed(KeyCode::BracketRight) {
        cam.distance = (cam.distance * (1.0 + 1.5 * dt)).min(2500.0);
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
