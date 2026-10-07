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

use crate::sim::{Sim, SimSet};
use crate::tools::Selection;
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
/// Walk mode moves in real time: a person's stroll and, with Shift, a jog.
const WALK_SPEED: f32 = 1.5;
const RUN_SPEED: f32 = 5.0;
const LOOK_SPEED: f32 = 0.0022;
const ORBIT_FOV: f32 = 0.785;
const WALK_FOV: f32 = 1.2;
/// The fog is solid this far from the camera; the ways to elsewhere run on
/// past it from anywhere the camera may go, so their ends (where traffic
/// appears and is taken off) are never seen.
const SIGHT: f32 = nd_sim::landscape::SIGHT;
const BOUND: f32 = nd_sim::landscape::CAMERA_BOUND;

/// Where the mouse cursor hits the ground plane, in sim coordinates.
#[derive(Resource, Default)]
pub struct Cursor {
    pub ground: Option<Vec2>,
}

/// [camera] The thing the camera is locked onto: a person, a vehicle, a
/// delivery robot or a building. While set, the orbit target rides on it
/// (pan keys and drags do nothing; rotate and zoom still work) and Walk
/// view looks out through their eyes. Esc, the banner's button or the
/// thing leaving the city ends it.
#[derive(Resource, Default)]
pub struct CameraFollow {
    pub target: Option<Selection>,
    /// The `Sim::generation` the target belongs to: a loaded city reuses ids.
    pub generation: u64,
    /// Point of view: the head's turn and tilt from straight ahead
    /// (right-drag), springing back when let go.
    pub pov_look: Vec2,
    /// The smoothed heading the point of view turns with.
    pov_yaw: Option<f32>,
    /// Who drives the followed car: when it parks and leaves the road the
    /// camera stays with them on foot.
    driver: Option<nd_sim::CitizenId>,
}

impl CameraFollow {
    pub fn start(&mut self, sel: Selection, generation: u64) {
        self.target = Some(sel);
        self.generation = generation;
        self.pov_look = Vec2::ZERO;
        self.pov_yaw = None;
        self.driver = None;
    }

    pub fn stop(&mut self) {
        self.target = None;
        self.pov_yaw = None;
        self.driver = None;
    }

    /// Looking through the eyes of someone in a vehicle (or the vehicle
    /// itself): the selection's outline box would be drawn round the eye.
    pub fn pov_in_vehicle(&self, sim: &Sim, mode: CameraMode) -> bool {
        if mode != CameraMode::Walk {
            return false;
        }
        match self.target {
            Some(Selection::Car(_) | Selection::Parked(_) | Selection::Bot(_)) => true,
            Some(Selection::Citizen(id)) => sim.world.citizens.get(id).is_some_and(|c| matches!(c.activity, nd_sim::Activity::Driving { .. } | nd_sim::Activity::Riding { .. } | nd_sim::Activity::Passenger { seated: true, .. })),
            _ => false,
        }
    }

    pub fn is(&self, sel: Selection) -> bool {
        self.target == Some(sel)
    }
}

/// [camera] Where a smooth camera move is headed: a fixed point or a thing
/// that may move while the camera travels.
#[derive(Clone, Copy, PartialEq)]
pub enum FlyGoal {
    Point(Vec2),
    Thing(Selection),
}

/// [camera] A smooth move of the orbit camera ("zoom to", the "go" buttons):
/// target, distance and pitch ease from where they were to the goal, with a
/// hop out over long trips so the city is seen passing under.
#[derive(Resource, Default)]
pub struct CameraFly {
    pub flight: Option<Flight>,
}

#[derive(Clone, Copy)]
pub struct Flight {
    pub goal: FlyGoal,
    /// Distance and pitch at the end; `None` keeps the pitch at 0.7 and
    /// caps the distance at 70 m.
    pub distance: Option<f32>,
    pub pitch: Option<f32>,
    start: Option<(Vec3, f32, f32)>,
    last_goal: Option<Vec2>,
    t: f32,
    duration: f32,
}

impl CameraFly {
    /// Fly to `goal`, ending `distance` away at `pitch` (radians).
    pub fn to(&mut self, goal: FlyGoal, distance: Option<f32>, pitch: Option<f32>) {
        self.flight = Some(Flight { goal, distance, pitch, start: None, last_goal: None, t: 0.0, duration: 1.0 });
    }

    pub fn flying(&self) -> bool {
        self.flight.is_some()
    }
}

/// [camera] How close "zoom to" ends on each kind of thing.
pub fn zoom_distance(sim: &Sim, sel: Selection) -> f32 {
    match sel {
        Selection::Citizen(_) => 32.0,
        Selection::Bot(_) => 22.0,
        Selection::Car(id) => {
            if sim.world.traffic.cars.get(id).is_some_and(|c| c.bus.is_some() || c.cargo.is_some()) {
                55.0
            } else {
                40.0
            }
        }
        Selection::Parked(_) => 35.0,
        Selection::Building(id) => {
            let w = &sim.world;
            let r = w.buildings.get(id).and_then(|b| w.lots.get(b.lot)).map_or(30.0, |l| l.polygon.iter().map(|p| p.distance(l.center)).fold(0.0, f32::max));
            let tall = w.buildings.get(id).map_or(0.0, |b| b.floors as f32 * 3.5);
            (r * 2.6 + tall * 1.2).clamp(60.0, 400.0)
        }
        Selection::None => 70.0,
    }
}

/// [camera] Where a selected thing is drawn this frame, its heading (for a
/// point of view; none for a building) and the height of its eyes.
#[derive(Clone, Copy, Debug)]
pub struct ThingPose {
    pub pos: Vec2,
    /// Radians in the sim's x-y plane from +x, as cars and robots carry it.
    pub heading: Option<f32>,
    /// Eye height in world y: ground (or road deck) plus head height.
    pub eye: f32,
}

pub fn thing_pose(sim: &Sim, sel: Selection) -> Option<ThingPose> {
    let w = &sim.world;
    let f = sim.frac();
    let car_pose = |id: nd_sim::CarId, head: f32| {
        w.traffic.cars.get(id).map(|car| {
            let (p, h) = car.pose_at(f);
            ThingPose { pos: p, heading: Some(h), eye: w.car_height_at(car, p) + head }
        })
    };
    // Where a walker or a robot stands: the level they are on (under a
    // viaduct, not on its deck), as `render::lift_movers` draws them.
    let ground = |p: Vec2| w.surface_height_near(p, w.ground_height(p));
    match sel {
        Selection::None => None,
        Selection::Car(id) => {
            let big = w.traffic.cars.get(id).is_some_and(|c| c.bus.is_some() || c.cargo.is_some());
            car_pose(id, if big { 2.6 } else { 1.25 })
        }
        Selection::Parked(id) => w.vehicles.cars.get(id).and_then(|c| c.spot()).map(|s| ThingPose { pos: s.pos, heading: Some(s.heading), eye: ground(s.pos) + 1.2 }),
        Selection::Building(id) => w.buildings.get(id).and_then(|b| w.lots.get(b.lot)).map(|l| ThingPose { pos: l.center, heading: None, eye: ground(l.center) + EYE_HEIGHT }),
        Selection::Bot(id) => w.bots.bots.get(id).map(|b| {
            let p = if b.prev_pos.distance(b.pos) < 30.0 { b.prev_pos.lerp(b.pos, f) } else { b.pos };
            ThingPose { pos: p, heading: Some(lerp_angle(b.prev_heading, b.heading, f)), eye: ground(p) + 0.75 }
        }),
        Selection::Citizen(id) => {
            let c = w.citizens.get(id)?;
            let riding = match &c.activity {
                nd_sim::Activity::Driving { car, .. } => car_pose(*car, 1.2),
                nd_sim::Activity::Riding { bus } => car_pose(*bus, 2.6),
                nd_sim::Activity::Passenger { driver, seated: true, .. } => match w.citizens.get(*driver).map(|d| &d.activity) {
                    Some(nd_sim::Activity::Driving { car, .. }) => car_pose(*car, 1.2),
                    _ => None,
                },
                _ => None,
            };
            riding.or_else(|| {
                let p = if c.prev_pos.distance(c.pos) < 30.0 { c.prev_pos.lerp(c.pos, f) } else { c.pos };
                Some(ThingPose { pos: p, heading: Some(lerp_angle(c.prev_heading, c.heading, f)), eye: ground(p) + EYE_HEIGHT - 0.05 })
            })
        }
    }
}

fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let d = (b - a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
    a + d * t
}

/// The walk camera's yaw (forward = (sin, cos) in x, z) for a sim heading.
fn yaw_of_heading(h: f32) -> f32 {
    h.cos().atan2(h.sin())
}

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Cursor>()
            .init_resource::<CameraMode>()
            .init_resource::<WalkState>()
            .init_resource::<CameraFollow>()
            .init_resource::<CameraFly>()
            .add_systems(Startup, spawn_camera)
            .add_systems(Update, (control_camera, project_cursor).chain().before(SimSet::Input))
            // After the sim has stepped and the renderer has placed things,
            // so the camera sits on the thing where it is drawn this frame.
            .add_systems(Update, follow_and_fly.after(SimSet::Sync));
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
            falloff: FogFalloff::Linear { start: 500.0, end: SIGHT },
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
    mut keys: ResMut<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    scroll: Res<AccumulatedMouseScroll>,
    flags: Res<UiFlags>,
    sim: Res<Sim>,
    mut mode: ResMut<CameraMode>,
    mut walk: ResMut<WalkState>,
    mut q: Query<(&mut OrbitCamera, &mut Transform, &mut Projection, &mut DistanceFog)>,
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
    tools: Res<crate::tools::ToolState>,
    (mut follow, mut fly): (ResMut<CameraFollow>, ResMut<CameraFly>),
) {
    let Ok((mut cam, mut tf, mut proj, mut fog)) = q.single_mut() else { return };
    let dt = time.delta_secs();
    // [camera] A loaded or new city: the followed id means someone else now.
    if follow.target.is_some() && follow.generation != sim.generation {
        follow.stop();
        fly.flight = None;
    }
    // [camera] Escape first ends a follow (and its point of view) or a
    // flight, and is used up by it: the selection stays for a second Escape.
    if keys.just_pressed(KeyCode::Escape) && (follow.target.is_some() || fly.flying()) {
        follow.stop();
        fly.flight = None;
        if *mode == CameraMode::Walk {
            *mode = CameraMode::Orbit;
        }
        keys.clear_just_pressed(KeyCode::Escape);
    }
    // Following something with a heading: Walk view is their point of view.
    let pov_capable = follow.target.and_then(|t| thing_pose(&sim, t)).is_some_and(|p| p.heading.is_some());
    let locked = follow.target.is_some() || fly.flying();
    // egui uses Tab for focus traversal and takes typed keys while a text
    // field has focus, so the one-shot keys stand down then. Escape always
    // works: it is how you get off the pavement.
    let key = |k: KeyCode| !flags.keyboard_captured && keys.just_pressed(k);
    // Held movement keys stand down too: typing "Westside" into a name
    // field must not pan and turn the camera.
    let held = |k: KeyCode| !flags.keyboard_captured && keys.pressed(k);

    // Tab toggles walk mode; Escape leaves it.
    if key(KeyCode::Tab) || (keys.just_pressed(KeyCode::Escape) && *mode == CameraMode::Walk) {
        *mode = if *mode == CameraMode::Walk { CameraMode::Orbit } else { CameraMode::Walk };
    }
    let pov = *mode == CameraMode::Walk && pov_capable;
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
                // A followed thing without a heading (a building) has no
                // point of view: walking starts beside it and ends the follow.
                if !pov && follow.target.is_some() {
                    follow.stop();
                }
                follow.pov_look = Vec2::ZERO;
                // Through someone's eyes the pointer stays free for the
                // banner's buttons; walking yourself, the mouse is the head.
                for mut c in &mut cursors {
                    c.grab_mode = if pov { CursorGrabMode::None } else { CursorGrabMode::Locked };
                    c.visible = pov;
                }
            }
            CameraMode::Orbit => {
                cam.target = Vec3::new(walk.pos.x, 0.0, walk.pos.z);
                cam.distance = cam.distance.min(140.0);
                cam.yaw = walk.yaw + std::f32::consts::PI;
                *proj = Projection::Perspective(PerspectiveProjection { fov: ORBIT_FOV, near: 0.1, ..default() });
                fog.falloff = FogFalloff::Linear { start: 500.0, end: SIGHT };
                for mut c in &mut cursors {
                    c.grab_mode = CursorGrabMode::None;
                    c.visible = true;
                }
            }
        }
    }

    // [camera] Their point of view: `follow_and_fly` places the eye; here
    // only the head turns, with a right-drag, and springs back when let go.
    if *mode == CameraMode::Walk && pov {
        if buttons.pressed(MouseButton::Right) {
            follow.pov_look.x -= motion.delta.x * LOOK_SPEED;
            follow.pov_look.y = (follow.pov_look.y - motion.delta.y * LOOK_SPEED).clamp(-1.2, 1.2);
        } else {
            let k = (dt * 2.5).min(1.0);
            follow.pov_look *= 1.0 - k;
        }
        for mut c in &mut cursors {
            if c.grab_mode != CursorGrabMode::None {
                c.grab_mode = CursorGrabMode::None;
                c.visible = true;
            }
        }
        return;
    }
    if *mode == CameraMode::Walk {
        walk.yaw -= motion.delta.x * LOOK_SPEED;
        walk.pitch = (walk.pitch - motion.delta.y * LOOK_SPEED).clamp(-1.35, 1.35);
        let (sy, cy) = walk.yaw.sin_cos();
        let forward = Vec3::new(sy, 0.0, cy);
        // right = forward x up in this Y-up, right-handed world.
        let right = Vec3::new(-cy, 0.0, sy);
        let mut mv = Vec3::ZERO;
        if held(KeyCode::KeyW) || held(KeyCode::ArrowUp) {
            mv += forward;
        }
        if held(KeyCode::KeyS) || held(KeyCode::ArrowDown) {
            mv -= forward;
        }
        if held(KeyCode::KeyD) || held(KeyCode::ArrowRight) {
            mv += right;
        }
        if held(KeyCode::KeyA) || held(KeyCode::ArrowLeft) {
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
        // Eyes above whatever is underfoot: hills, bridge decks.
        walk.pos.y = EYE_HEIGHT + sim.world.surface_height(Vec2::new(walk.pos.x, walk.pos.z));
        let (sp, cp) = walk.pitch.sin_cos();
        let look = Vec3::new(sy * cp, sp, cy * cp);
        tf.translation = walk.pos;
        tf.look_to(look, Vec3::Y);
        return;
    }

    // F6: focus the busiest junction, close enough to watch the driving.
    // [camera] None of the jumps while the camera is locked on something.
    if key(KeyCode::F6) && !locked {
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
    for (k, tier) in [(KeyCode::F7, Tier::Low), (KeyCode::F8, Tier::Mid), (KeyCode::F9, Tier::High)] {
        if key(k) && !locked {
            let candidates: Vec<_> = sim.world.buildings.values().filter(|b| b.tier == tier && b.land_use != nd_sim::LandUse::Park).filter_map(|b| sim.world.lots.get(b.lot)).map(|l| l.center).collect();
            if let Some(c) = candidates.choose(&mut rand::rng()) {
                cam.target = Vec3::new(c.x, 0.0, c.y);
                cam.distance = 110.0;
                cam.pitch = 0.55;
            }
        }
    }

    // F12: jump to a random park.
    if key(KeyCode::F12) && !locked {
        let candidates: Vec<_> = sim.world.lots.values().filter(|l| l.park.is_some()).map(|l| l.center).collect();
        if let Some(c) = candidates.choose(&mut rand::rng()) {
            cam.target = Vec3::new(c.x, 0.0, c.y);
            cam.distance = 90.0;
            cam.pitch = 0.6;
        }
    }
    // F5: jump to a station or freight yard.
    if key(KeyCode::F5) && !locked {
        let candidates: Vec<_> = sim.world.buildings.values().filter(|b| b.rail.is_some()).filter_map(|b| sim.world.lots.get(b.lot)).map(|l| l.center).collect();
        if let Some(c) = candidates.choose(&mut rand::rng()) {
            cam.target = Vec3::new(c.x, 0.0, c.y);
            cam.distance = 140.0;
            cam.pitch = 0.55;
        }
    }
    // F11: jump to a random power plant.
    if key(KeyCode::F11) && !locked {
        let candidates: Vec<_> = sim.world.buildings.values().filter(|b| b.plant.is_some()).filter_map(|b| sim.world.lots.get(b.lot)).map(|l| l.center).collect();
        if let Some(c) = candidates.choose(&mut rand::rng()) {
            cam.target = Vec3::new(c.x, 0.0, c.y);
            cam.distance = 150.0;
            cam.pitch = 0.5;
        }
    }

    // Rotate with the right mouse button, pan with the middle button or shift+left.
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    // The zone brush and marquee erase with a right-drag: no orbit then.
    if buttons.pressed(MouseButton::Right) && !tools.captures_right_drag() {
        cam.yaw -= motion.delta.x * 0.005;
        cam.pitch = (cam.pitch + motion.delta.y * 0.005).clamp(0.08, 1.5);
    } else if !locked && (buttons.pressed(MouseButton::Middle) || (shift && buttons.pressed(MouseButton::Left))) {
        let (s, c) = cam.yaw.sin_cos();
        let right = Vec3::new(c, 0.0, -s);
        let forward = Vec3::new(s, 0.0, c);
        let k = cam.distance * 0.0016;
        cam.target -= right * motion.delta.x * k;
        cam.target -= forward * motion.delta.y * k;
    }

    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    // Ctrl+wheel sizes the Road tool's roundabout instead of zooming.
    if !flags.pointer_over_ui && !tools.captures_wheel(ctrl) {
        let lines = match scroll.unit {
            MouseScrollUnit::Line => scroll.delta.y,
            MouseScrollUnit::Pixel => scroll.delta.y / 60.0,
        };
        if lines != 0.0 && !fly.flying() {
            cam.distance = (cam.distance * (1.0 - lines * 0.12)).clamp(12.0, 2500.0);
        }
    }

    // Keyboard pan and rotate.
    let mut pan = Vec2::ZERO;
    if held(KeyCode::KeyW) || held(KeyCode::ArrowUp) {
        pan.y += 1.0;
    }
    if held(KeyCode::KeyS) || held(KeyCode::ArrowDown) {
        pan.y -= 1.0;
    }
    if held(KeyCode::KeyA) || held(KeyCode::ArrowLeft) {
        pan.x -= 1.0;
    }
    if held(KeyCode::KeyD) || held(KeyCode::ArrowRight) {
        pan.x += 1.0;
    }
    if pan != Vec2::ZERO && !locked {
        let (s, c) = cam.yaw.sin_cos();
        let right = Vec3::new(c, 0.0, -s);
        let forward = Vec3::new(-s, 0.0, -c);
        let speed = cam.distance * 0.9 * dt;
        cam.target += (right * pan.x + forward * pan.y) * speed;
    }
    if held(KeyCode::BracketLeft) {
        cam.distance = (cam.distance * (1.0 - 1.5 * dt)).max(12.0);
    }
    if held(KeyCode::BracketRight) {
        cam.distance = (cam.distance * (1.0 + 1.5 * dt)).min(2500.0);
    }
    if held(KeyCode::KeyQ) {
        cam.yaw += 1.2 * dt;
    }
    if held(KeyCode::KeyE) {
        cam.yaw -= 1.2 * dt;
    }

    // The orbit centre rides on the ground, eased so hills do not jolt it.
    let ground = sim.world.surface_height(Vec2::new(cam.target.x, cam.target.z));
    cam.target.y += (ground - cam.target.y) * (dt * 6.0).min(1.0);
    place_orbit(&mut cam, &mut tf);
}

/// The camera stays over the map: target and eye within the bound. Then
/// the eye is set from the orbit's target, angles and distance.
fn place_orbit(cam: &mut OrbitCamera, tf: &mut Transform) {
    cam.target.x = cam.target.x.clamp(-BOUND, BOUND);
    cam.target.z = cam.target.z.clamp(-BOUND, BOUND);
    let (sy, cy) = cam.yaw.sin_cos();
    let (sp, cp) = cam.pitch.sin_cos();
    let offset = Vec3::new(sy * cp, sp, cy * cp) * cam.distance;
    // An eye that would leave the bound slides the whole view back in,
    // keeping its angle (clamping the eye alone stacked it over the
    // target and the camera looked straight down).
    let eye = cam.target + offset;
    let over = Vec3::new(eye.x - eye.x.clamp(-BOUND, BOUND), 0.0, eye.z - eye.z.clamp(-BOUND, BOUND));
    cam.target -= over;
    let eye = cam.target + offset;
    tf.translation = eye;
    tf.look_at(cam.target, Vec3::Y);
}

/// [camera] After the sim has stepped: carry a flight one frame further,
/// else keep the orbit target on the followed thing, or in its point of
/// view put the eye where its eyes are. A followed thing that is gone ends
/// the follow.
#[allow(clippy::too_many_arguments)]
fn follow_and_fly(
    time: Res<Time>,
    sim: Res<Sim>,
    mode: Res<CameraMode>,
    mut walk: ResMut<WalkState>,
    mut follow: ResMut<CameraFollow>,
    mut fly: ResMut<CameraFly>,
    mut flags: ResMut<UiFlags>,
    mut q: Query<(&mut OrbitCamera, &mut Transform)>,
    mut selection: ResMut<Selection>,
) {
    let Ok((mut cam, mut tf)) = q.single_mut() else { return };
    let dt = time.delta_secs().min(0.1);
    let w = &sim.world;
    if *mode == CameraMode::Walk {
        fly.flight = None;
    }
    if let Some(flight) = fly.flight.as_mut() {
        let goal = match flight.goal {
            FlyGoal::Point(p) => Some(p),
            FlyGoal::Thing(sel) => thing_pose(&sim, sel).map(|t| t.pos),
        }
        .or(flight.last_goal);
        let Some(goal) = goal else {
            fly.flight = None;
            return;
        };
        flight.last_goal = Some(goal);
        let (from, from_d, from_p) = *flight.start.get_or_insert((cam.target, cam.distance, cam.pitch));
        let trip = Vec2::new(from.x, from.z).distance(goal);
        if flight.t == 0.0 {
            flight.duration = (0.7 + trip / 1400.0).clamp(0.7, 2.2);
        }
        flight.t = (flight.t + dt / flight.duration).min(1.0);
        let t = flight.t;
        let e = t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
        let to_d = flight.distance.unwrap_or(from_d.min(70.0)).clamp(12.0, 2500.0);
        let to_p = flight.pitch.unwrap_or(0.7).clamp(0.08, 1.5);
        // Out and back in over a long trip, so the move reads as travel.
        let hop = (trip * 0.45).min(700.0) * (std::f32::consts::PI * e).sin();
        let g = Vec3::new(goal.x, w.surface_height(goal), goal.y);
        cam.target = from.lerp(g, e);
        cam.distance = (from_d.ln() + (to_d.ln() - from_d.ln()) * e).exp() + hop;
        cam.pitch = from_p + (to_p - from_p) * e;
        place_orbit(&mut cam, &mut tf);
        if flight.t >= 1.0 {
            fly.flight = None;
        }
        return;
    }
    let Some(sel) = follow.target else { return };
    if let Selection::Car(id) = sel {
        if let Some(car) = w.traffic.cars.get(id) {
            follow.driver = w.citizens.contains_key(car.citizen).then_some(car.citizen);
        }
    }
    let Some(pose) = thing_pose(&sim, sel) else {
        // A car that parked: stay with its driver, now on foot.
        if let Some(d) = follow.driver.take().filter(|d| w.citizens.contains_key(*d)) {
            follow.target = Some(Selection::Citizen(d));
            // The inspector was on the car: it moves to the driver too.
            if *selection == sel {
                *selection = Selection::Citizen(d);
            }
            flags.message = format!("{} parked; following them on foot.", w.citizens[d].name);
            return;
        }
        follow.stop();
        flags.message = "Stopped following: they are gone.".into();
        return;
    };
    if *mode == CameraMode::Walk {
        let Some(h) = pose.heading else { return };
        // Turn with them, smoothed: a car's heading twitches at junctions.
        let want = yaw_of_heading(h);
        let yaw = match follow.pov_yaw {
            Some(y) => lerp_angle(y, want, (dt * 4.0).min(1.0)),
            None => want,
        };
        follow.pov_yaw = Some(yaw);
        walk.pos = Vec3::new(pose.pos.x, pose.eye, pose.pos.y);
        walk.yaw = yaw + follow.pov_look.x;
        walk.pitch = (-0.06 + follow.pov_look.y).clamp(-1.35, 1.35);
        let (sy, cy) = walk.yaw.sin_cos();
        let (sp, cp) = walk.pitch.sin_cos();
        let look = Vec3::new(sy * cp, sp, cy * cp);
        // A little forward of the head, so their own body is not in view.
        let ahead = Vec3::new(yaw.sin(), 0.0, yaw.cos()) * 0.3;
        tf.translation = walk.pos + ahead;
        tf.look_to(look, Vec3::Y);
        return;
    }
    cam.target = Vec3::new(pose.pos.x, w.surface_height(pose.pos), pose.pos.y);
    place_orbit(&mut cam, &mut tf);
}

fn project_cursor(
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<OrbitCamera>>,
    mode: Res<CameraMode>,
    flags: Res<UiFlags>,
    sim: Res<Sim>,
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
    let mut hit = ray.origin + dir * t;
    // On a landscape, walk the ray onto the ground: re-intersect at the
    // height found under the last guess until it settles.
    if !sim.world.landscape.is_flat() {
        for _ in 0..6 {
            let h = sim.world.surface_height(Vec2::new(hit.x, hit.z));
            let t = (h - ray.origin.y) / dir.y;
            let next = ray.origin + dir * t;
            let settled = next.distance(hit) < 0.2;
            hit = next;
            if settled {
                break;
            }
        }
    }
    cursor.ground = Some(Vec2::new(hit.x, hit.z));
}
