//! Two cameras in one: an orbit camera for planning and a first-person walk
//! mode for standing on the pavement. Tab switches between them. Also owns
//! the cursor-to-ground projection used by the tools.

use bevy::camera::Hdr;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll, MouseScrollUnit};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

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

/// Which camera the player is using. The UI and the tools read this.
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CameraMode {
    #[default]
    Orbit,
    Walk,
}

/// First-person state: eye position and look angles.
#[derive(Resource)]
pub struct WalkState {
    pub pos: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    last_mode: CameraMode,
}

impl Default for WalkState {
    fn default() -> Self {
        Self { pos: Vec3::new(0.0, EYE_HEIGHT, 0.0), yaw: 0.0, pitch: 0.0, last_mode: CameraMode::Orbit }
    }
}

const EYE_HEIGHT: f32 = 1.7;
const WALK_SPEED: f32 = 2.4;
const RUN_SPEED: f32 = 7.5;
const LOOK_SPEED: f32 = 0.0022;
const ORBIT_FOV: f32 = 0.785;
const WALK_FOV: f32 = 1.2;

/// Where the mouse cursor hits the ground plane, in sim coordinates.
#[derive(Resource, Default)]
pub struct Cursor {
    pub ground: Option<Vec2>,
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Cursor>()
            .init_resource::<CameraMode>()
            .init_resource::<WalkState>()
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
        Projection::Perspective(PerspectiveProjection { fov: ORBIT_FOV, near: 0.1, ..default() }),
        OrbitCamera { target: Vec3::ZERO, yaw: 0.7, pitch: 0.85, distance: 520.0 },
        Transform::from_xyz(0.0, 400.0, 400.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

#[allow(clippy::too_many_arguments)]
fn control_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    flags: Res<UiFlags>,
    sim: Res<Sim>,
    mut mode: ResMut<CameraMode>,
    mut walk: ResMut<WalkState>,
    mut q: Query<(&mut OrbitCamera, &mut Transform, &mut Projection, &mut DistanceFog)>,
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let Ok((mut cam, mut tf, mut proj, mut fog)) = q.single_mut() else { return };
    let dt = time.delta_secs();

    // Tab toggles walk mode; Escape leaves it.
    if keys.just_pressed(KeyCode::Tab) || (keys.just_pressed(KeyCode::Escape) && *mode == CameraMode::Walk) {
        *mode = if *mode == CameraMode::Walk { CameraMode::Orbit } else { CameraMode::Walk };
    }
    if *mode != walk.last_mode {
        walk.last_mode = *mode;
        match *mode {
            CameraMode::Walk => {
                // Step onto the pavement nearest the orbit target, facing along the street.
                let target = Vec2::new(cam.target.x, cam.target.z);
                let roads = &sim.world.roads;
                let (p, yaw) = match roads.nearest_on_network(target) {
                    Some((sid, _, q)) => {
                        let (a, b) = roads.segment_points(sid);
                        let d = (b - a).normalize_or_zero();
                        let right = Vec2::new(-d.y, d.x);
                        let hw = roads.segment(sid).half_width();
                        let side = if (target - q).dot(right) >= 0.0 { 1.0 } else { -1.0 };
                        // Stand at the back of the pavement, a few metres along from the
                        // projection so lamps and shelters are not in the face.
                        (q + right * side * (hw + 2.1) + d * 4.0, d.x.atan2(d.y))
                    }
                    None => (target, 0.0),
                };
                walk.pos = Vec3::new(p.x, EYE_HEIGHT, p.y);
                walk.yaw = yaw;
                walk.pitch = -0.05;
                *proj = Projection::Perspective(PerspectiveProjection { fov: WALK_FOV, near: 0.08, ..default() });
                fog.falloff = FogFalloff::Linear { start: 120.0, end: 900.0 };
                for mut c in &mut cursors {
                    c.grab_mode = CursorGrabMode::Locked;
                    c.visible = false;
                }
            }
            CameraMode::Orbit => {
                cam.target = Vec3::new(walk.pos.x, 0.0, walk.pos.z);
                cam.distance = cam.distance.min(140.0);
                cam.yaw = walk.yaw + std::f32::consts::PI;
                *proj = Projection::Perspective(PerspectiveProjection { fov: ORBIT_FOV, near: 0.1, ..default() });
                fog.falloff = FogFalloff::Linear { start: 500.0, end: 2600.0 };
                for mut c in &mut cursors {
                    c.grab_mode = CursorGrabMode::None;
                    c.visible = true;
                }
            }
        }
    }

    if *mode == CameraMode::Walk {
        walk.yaw -= motion.delta.x * LOOK_SPEED;
        walk.pitch = (walk.pitch - motion.delta.y * LOOK_SPEED).clamp(-1.35, 1.35);
        let (sy, cy) = walk.yaw.sin_cos();
        let forward = Vec3::new(sy, 0.0, cy);
        let right = Vec3::new(cy, 0.0, -sy);
        let mut mv = Vec3::ZERO;
        if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
            mv += forward;
        }
        if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
            mv -= forward;
        }
        if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
            mv += right;
        }
        if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
            mv -= right;
        }
        let run = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
        let speed = if run { RUN_SPEED } else { WALK_SPEED };
        if mv != Vec3::ZERO {
            walk.pos += mv.normalize() * speed * dt;
        }
        let limit = sim.world.config.half_extent + 150.0;
        walk.pos.x = walk.pos.x.clamp(-limit, limit);
        walk.pos.z = walk.pos.z.clamp(-limit, limit);
        walk.pos.y = EYE_HEIGHT;
        let (sp, cp) = walk.pitch.sin_cos();
        let look = Vec3::new(sy * cp, sp, cy * cp);
        tf.translation = walk.pos;
        tf.look_to(look, Vec3::Y);
        return;
    }

    // F6: focus the busiest junction, close enough to watch the driving.
    if keys.just_pressed(KeyCode::F6) {
        let roads = &sim.world.roads;
        let cars: Vec<Vec2> = sim.world.traffic.cars.values().map(|c| c.pos).collect();
        let mut best: Option<(usize, Vec2)> = None;
        for node in roads.nodes.iter() {
            let alive = node.segments.iter().filter(|&&s| roads.segment(s).alive).count();
            if alive < 3 {
                continue;
            }
            let n = cars.iter().filter(|p| p.distance(node.pos) < 60.0).count();
            if best.map_or(true, |(bn, _)| n > bn) {
                best = Some((n, node.pos));
            }
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
        cam.pitch = (cam.pitch + motion.delta.y * 0.005).clamp(0.08, 1.5);
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
            cam.distance = (cam.distance * (1.0 - lines * 0.12)).clamp(12.0, 2500.0);
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
        cam.distance = (cam.distance * (1.0 - 1.5 * dt)).max(12.0);
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
    mode: Res<CameraMode>,
    flags: Res<UiFlags>,
    mut cursor: ResMut<Cursor>,
) {
    cursor.ground = None;
    if *mode == CameraMode::Walk || flags.pointer_over_ui {
        return;
    }
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
