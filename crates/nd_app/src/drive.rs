//! [r5-drive] Drive mode: the third camera next to Orbit and Walk, where
//! the player drives a car round the city at a car's speed.
//!
//! - **Controls** (`drive_keys`, `pad_drive`): keyboard only is enough. W /
//!   Up throttle, S / Down brake and then reverse, A D / Left Right steer,
//!   Space handbrake, H horn, Q / E look left and right, R / F (or Home /
//!   End) look up and down, C look behind, `.` `,` zoom, V the bonnet view,
//!   L lane assist, Esc or Tab back to Orbit at the car's spot (Shift+Tab
//!   or the top bar's Drive button gets in). The on-screen pad drives too.
//!   A right-drag (or Alt+left-drag) looks round, from the cursor's
//!   movement (`nav::NavInput::drag_delta`), never the raw mouse alone, so a
//!   remote desktop works the same.
//! - **The car** (`DriveCar::step`): simple arcade handling, a bicycle model
//!   with the steering eased in from digital keys and less lock at speed.
//!   It keeps to the surface the world gives (`World::drive_ground`: the
//!   road at its level, so up ramps and onto viaducts and bridges, down
//!   into tunnels, the ground off the road); buildings, water, walls,
//!   drops off a deck and the other traffic bump it back rather than wreck
//!   it.
//! - **The traffic** sees it (`nd_sim::traffic::player`): the car is not
//!   one of the sim's cars (those move on lanes, a tick of several seconds
//!   at a time, on routes; this one steers freely in real time), but it is
//!   handed to the traffic before every tick as a body on the road. Drivers
//!   queue behind it and wait for it in a junction; pulled onto the
//!   shoulder (parked) it is passed. It is never saved and never in the
//!   sim without Drive mode, so saves and every sim run without a player
//!   are exactly as before. The player obeys the lights or not: a red run
//!   is told in the HUD.
//! - While driving the game runs at 1× (traffic at its true speed, as the
//!   player's car); the speed it had comes back on the way out.

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};
use nd_sim::{DriveBlock, DriveGround};

use crate::chase::{clear_ground, wrap, Chase, Cut, Nudge, Subject, SubjectKind};
use crate::nav::{left_drag_role, LeftDrag, NavInput, NavMove, NavPad, NavSettings, TAP_SECS};
use crate::sim::Sim;
use crate::ui::UiFlags;

// ── the controls ─────────────────────────────────────────────────────────────

/// What the driver asks of the car this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Controls {
    /// The accelerator, 0..1.
    pub throttle: f32,
    /// The brake pedal, 0..1: brakes while rolling forward, reverses once
    /// stopped (as an arcade car does).
    pub brake: f32,
    /// Steering, -1 (full left) .. 1 (full right).
    pub steer: f32,
    pub handbrake: bool,
    pub horn: bool,
}

impl Controls {
    /// Two sets of controls together (keys and the pad): the stronger of each.
    pub fn with(self, o: Controls) -> Controls {
        let pick = |a: f32, b: f32| if b.abs() > a.abs() { b } else { a };
        Controls {
            throttle: self.throttle.max(o.throttle).clamp(0.0, 1.0),
            brake: self.brake.max(o.brake).clamp(0.0, 1.0),
            steer: pick(self.steer, o.steer).clamp(-1.0, 1.0),
            handbrake: self.handbrake || o.handbrake,
            horn: self.horn || o.horn,
        }
    }
}

/// What the driver asks of the camera: turn round the car (+ left, as Q),
/// tilt (+ looks down on it from higher), zoom (+ in), look behind.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Look {
    pub turn: f32,
    pub tilt: f32,
    pub zoom: f32,
    pub behind: bool,
}

/// Drive mode's keys. W / Up throttle, S / Down brake (then reverse), A /
/// Left and D / Right steer, Space handbrake, H horn; Q / E turn the camera,
/// R / F or Home / End tilt it, `.` `,` (and Page Up / Down, [ ]) zoom, C
/// looks behind.
pub fn drive_keys(held: impl Fn(KeyCode) -> bool) -> (Controls, Look) {
    let k = |a: KeyCode, b: KeyCode| (held(a) || held(b)) as i32 as f32;
    let c = Controls {
        throttle: k(KeyCode::KeyW, KeyCode::ArrowUp),
        brake: k(KeyCode::KeyS, KeyCode::ArrowDown),
        steer: k(KeyCode::KeyD, KeyCode::ArrowRight) - k(KeyCode::KeyA, KeyCode::ArrowLeft),
        handbrake: held(KeyCode::Space),
        horn: held(KeyCode::KeyH),
    };
    let zin = held(KeyCode::Period) || held(KeyCode::BracketLeft) || held(KeyCode::PageUp);
    let zout = held(KeyCode::Comma) || held(KeyCode::BracketRight) || held(KeyCode::PageDown);
    let look = Look {
        turn: held(KeyCode::KeyQ) as i32 as f32 - held(KeyCode::KeyE) as i32 as f32,
        tilt: k(KeyCode::KeyR, KeyCode::Home) - k(KeyCode::KeyF, KeyCode::End),
        zoom: zin as i32 as f32 - zout as i32 as f32,
        behind: held(KeyCode::KeyC),
    };
    (c, look)
}

/// The on-screen pad (and the remote `nav` holds) in Drive mode: the arrows
/// are the pedals and the wheel, the turn and tilt buttons the camera,
/// + and − the zoom.
pub fn pad_drive(m: NavMove) -> (Controls, Look) {
    let c = Controls { throttle: m.pan.y.max(0.0).min(1.0), brake: (-m.pan.y).max(0.0).min(1.0), steer: m.pan.x.clamp(-1.0, 1.0), ..Default::default() };
    (c, Look { turn: m.yaw, tilt: m.tilt, zoom: m.zoom, behind: false })
}

// ── the car ──────────────────────────────────────────────────────────────────

/// The chase camera's distance in Drive mode, times a followed car's.
const DRIVE_ZOOM: f32 = 1.45;
/// Top speed on the road and off it, and in reverse (m/s).
pub const TOP_SPEED: f32 = 36.0;
pub const OFF_ROAD_TOP: f32 = 10.0;
pub const REVERSE_TOP: f32 = 7.0;
/// Pulling away (m/s²), easing off towards the top speed.
const ENGINE: f32 = 4.2;
const BRAKE: f32 = 9.0;
const HANDBRAKE: f32 = 6.0;
/// Rolling resistance (m/s²) and air (per (m/s)²).
const ROLL: f32 = 0.3;
const AIR: f32 = 0.0004;
/// Wheelbase (m) and full lock (rad) at a crawl; less at speed.
const WHEELBASE: f32 = 2.7;
const MAX_LOCK: f32 = 0.6;
/// Sideways grip (m/s²): at speed the lock is cut so a turn never pulls
/// harder than this (more with the handbrake's swing).
const GRIP: f32 = 8.0;
/// How fast the wheel turns in from a key, and back to centre (per s).
const STEER_IN: f32 = 3.5;
const STEER_OUT: f32 = 6.0;
/// The body's half length and width (m), as the traffic sees it.
pub const HALF_LEN: f32 = nd_sim::traffic::PLAYER_HALF_LEN;
pub const HALF_WID: f32 = nd_sim::traffic::PLAYER_HALF_WID;
/// What is left of the speed after a bump, the other way.
const BOUNCE: f32 = 0.3;
/// Longest physics step (s): a slow frame is cut into these.
const MAX_STEP: f32 = 1.0 / 60.0;

/// Another vehicle's body on the road: centre, heading, half length, half
/// width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    pub pos: Vec2,
    pub heading: f32,
    pub half_len: f32,
    pub half_wid: f32,
}

impl Body {
    fn axes(&self) -> (Vec2, Vec2) {
        let d = Vec2::new(self.heading.cos(), self.heading.sin());
        (d, Vec2::new(-d.y, d.x))
    }

    /// Do two bodies overlap (the separating axis test on their four axes)?
    pub fn overlaps(&self, o: &Body) -> bool {
        let (a0, a1) = self.axes();
        let (b0, b1) = o.axes();
        let t = o.pos - self.pos;
        for axis in [a0, a1, b0, b1] {
            let ra = self.half_len * a0.dot(axis).abs() + self.half_wid * a1.dot(axis).abs();
            let rb = o.half_len * b0.dot(axis).abs() + o.half_wid * b1.dot(axis).abs();
            if t.dot(axis).abs() > ra + rb {
                return false;
            }
        }
        true
    }

    /// The points round the body that must stand on drivable ground: its
    /// corners and the middles of its sides.
    pub fn probes(&self) -> [Vec2; 6] {
        let (d, r) = self.axes();
        let (l, w) = (d * self.half_len, r * self.half_wid);
        [self.pos + l + w, self.pos + l - w, self.pos - l + w, self.pos - l - w, self.pos + w, self.pos - w]
    }
}

/// The player's car.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DriveCar {
    pub pos: Vec2,
    /// Sim radians from +x (heading +y is a right turn from +x).
    pub heading: f32,
    /// Speed along the heading (m/s, negative reversing).
    pub v: f32,
    /// Acceleration along the heading last step (m/s²), smoothed.
    pub accel: f32,
    /// The height of what it stands on.
    pub floor: f32,
    /// The wheel, -1..1 (eased towards the controls).
    pub wheel: f32,
    pub on_road: bool,
    /// Bumps so far, and what the last one was into.
    pub bumps: u32,
    pub last_bump: Option<Bump>,
}

/// What the car bumped into.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bump {
    Scenery(DriveBlock),
    Car,
}

impl Bump {
    pub fn label(self) -> &'static str {
        match self {
            Bump::Scenery(b) => b.label(),
            Bump::Car => "another car",
        }
    }
}

impl DriveCar {
    pub fn new(pos: Vec2, heading: f32, floor: f32) -> Self {
        Self { pos, heading, v: 0.0, accel: 0.0, floor, wheel: 0.0, on_road: true, bumps: 0, last_bump: None }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn body(&self) -> Body {
        Body { pos: self.pos, heading: self.heading, half_len: HALF_LEN, half_wid: HALF_WID }
    }

    pub fn dir(&self) -> Vec2 {
        Vec2::new(self.heading.cos(), self.heading.sin())
    }

    /// Drive on for `dt` seconds: `ground(p, floor)` is what stands at `p`
    /// for a car on `floor` (`World::drive_ground`), `cars` the other
    /// vehicles near it. Returns true when it bumped into something.
    pub fn step(&mut self, c: &Controls, dt: f32, ground: &impl Fn(Vec2, f32) -> Result<DriveGround, DriveBlock>, cars: &[Body]) -> bool {
        let mut bumped = false;
        let mut left = dt.clamp(0.0, 0.25);
        let v0 = self.v;
        while left > 1e-5 {
            let h = left.min(MAX_STEP);
            bumped |= self.substep(c, h, ground, cars);
            left -= h;
        }
        if dt > 0.0 {
            let a = (self.v - v0) / dt;
            self.accel += (a - self.accel) * (dt * 8.0).min(1.0);
        }
        bumped
    }

    fn substep(&mut self, c: &Controls, dt: f32, ground: &impl Fn(Vec2, f32) -> Result<DriveGround, DriveBlock>, cars: &[Body]) -> bool {
        // The wheel eases in from a key and back to the centre.
        let want = c.steer.clamp(-1.0, 1.0);
        let rate = if want == 0.0 || want * self.wheel < 0.0 { STEER_OUT } else { STEER_IN };
        let d = want - self.wheel;
        self.wheel += d.clamp(-rate * dt, rate * dt);
        self.v = speed_after(self.v, c, self.on_road, dt);
        // A bicycle: the turn rate from the speed and the lock, less lock at
        // speed so a tap at 100 km/h is a lane change, not a spin; the
        // handbrake swings the tail round.
        let grip = if c.handbrake && self.v.abs() > 4.0 { GRIP * 1.6 } else { GRIP };
        let max_lock = MAX_LOCK.min((grip * WHEELBASE / (self.v * self.v).max(1e-3)).atan());
        let lock = self.wheel * max_lock;
        let yaw = self.v / WHEELBASE * lock.tan();
        let heading = wrap(self.heading + yaw * dt);
        let dir = Vec2::new(heading.cos(), heading.sin());
        let pos = self.pos + dir * self.v * dt;
        // Where it stood was clear (or it would be stuck in a wall for good:
        // spawned there, or a building put up round it): free to move off.
        let stuck = ground(self.pos, self.floor).is_err();
        match self.check(pos, heading, ground, cars) {
            Ok(g) => {
                self.pos = pos;
                self.heading = heading;
                self.floor = g.height();
                self.on_road = g.on_road();
                false
            }
            Err(_) if stuck => {
                self.pos = pos;
                self.heading = heading;
                false
            }
            Err(what) => {
                // Slide along it if one of the axes is clear, else stand.
                let slide = [Vec2::new(pos.x, self.pos.y), Vec2::new(self.pos.x, pos.y)]
                    .into_iter()
                    .filter(|&p| p.distance(self.pos) > 1e-4)
                    .find_map(|p| self.check(p, self.heading, ground, cars).ok().map(|g| (p, g)));
                let hard = self.v.abs() > 2.0;
                match slide {
                    Some((p, g)) if !matches!(what, Bump::Car) => {
                        self.pos = p;
                        self.floor = g.height();
                        self.on_road = g.on_road();
                        self.v *= 0.92;
                    }
                    _ => self.v = -self.v * BOUNCE,
                }
                if hard {
                    self.bumps += 1;
                    self.last_bump = Some(what);
                }
                hard
            }
        }
    }

    /// Can the body stand at `pos` facing `heading`? The ground under its
    /// middle, or what is in the way.
    fn check(&self, pos: Vec2, heading: f32, ground: &impl Fn(Vec2, f32) -> Result<DriveGround, DriveBlock>, cars: &[Body]) -> Result<DriveGround, Bump> {
        let centre = ground(pos, self.floor).map_err(Bump::Scenery)?;
        let body = Body { pos, heading, half_len: HALF_LEN, half_wid: HALF_WID };
        for p in body.probes() {
            ground(p, centre.height()).map_err(Bump::Scenery)?;
        }
        if cars.iter().any(|o| o.overlaps(&body)) {
            return Err(Bump::Car);
        }
        Ok(centre)
    }
}

/// The speed after `dt` with these controls: the accelerator pulls up to
/// the top speed (off the road a crawl's), the brake stops it and then
/// reverses, the handbrake drags, and it rolls to a stop on its own.
pub fn speed_after(v: f32, c: &Controls, on_road: bool, dt: f32) -> f32 {
    let top = if on_road { TOP_SPEED } else { OFF_ROAD_TOP };
    let mut a = 0.0;
    if v >= -0.3 {
        // Rolling forward or stopped: the accelerator pulls, the brake brakes.
        if c.throttle > 0.0 {
            a += ENGINE * c.throttle * (1.0 - (v / top).clamp(0.0, 1.0).powi(2));
        }
        if c.brake > 0.0 {
            a -= if v > 0.3 { BRAKE * c.brake } else { ENGINE * 0.7 * c.brake * (1.0 - (-v / REVERSE_TOP).clamp(0.0, 1.0)) };
        }
    } else {
        // Reversing: the brake pedal is the reverse gear, the accelerator brakes.
        if c.brake > 0.0 {
            a -= ENGINE * 0.7 * c.brake * (1.0 - (-v / REVERSE_TOP).clamp(0.0, 1.0));
        }
        if c.throttle > 0.0 {
            a += BRAKE * c.throttle;
        }
    }
    // Drag against the motion; it never turns the car round.
    let mut drag = ROLL + AIR * v * v + if c.handbrake { HANDBRAKE } else { 0.0 };
    if v.abs() > top {
        drag += 3.0;
    }
    let mut nv = v + a * dt;
    let before = nv;
    nv -= nv.signum() * drag * dt;
    if nv.signum() != before.signum() || (nv.abs() < 0.05 && a == 0.0) {
        nv = 0.0;
    }
    nv
}

/// Lane assist: the wheel that keeps the car along its lane, from how far
/// its heading is off the lane's and how far it sits right of the lane's
/// centre (m).
pub fn assist_steer(heading: f32, lane_heading: f32, right_of_centre: f32, v: f32) -> f32 {
    if v.abs() < 2.0 {
        return 0.0;
    }
    let err = wrap(lane_heading - heading) - (right_of_centre * 0.25).atan();
    (err * 2.5 * v.signum()).clamp(-0.7, 0.7)
}

/// A compass bearing (degrees clockwise from north) for a sim heading.
/// North is -y in sim axes (up the map as the orbit camera starts), east +x.
pub fn bearing(heading: f32) -> f32 {
    let d = Vec2::new(heading.cos(), heading.sin());
    d.x.atan2(-d.y).to_degrees().rem_euclid(360.0)
}

/// The compass point for a bearing: N, NE, E, ...
pub fn compass_point(deg: f32) -> &'static str {
    const P: [&str; 8] = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    P[((deg.rem_euclid(360.0) + 22.5) / 45.0) as usize % 8]
}

// ── the state ───────────────────────────────────────────────────────────────

/// The camera in Drive mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DriveView {
    /// Low behind the car (the chase camera).
    #[default]
    Chase,
    /// From the bonnet, the driver's view of the road.
    Bonnet,
}

/// Drive mode's state.
#[derive(Resource, Default)]
pub struct Drive {
    pub car: Option<DriveCar>,
    pub view: DriveView,
    pub assist: bool,
    chase: Chase,
    pub nudge: Nudge,
    behind: bool,
    /// The game's speed and pause before driving, put back after.
    restore: Option<(usize, bool)>,
    /// The city this drive is in (`Sim::generation`).
    generation: u64,
    /// For the HUD: the street, its limit (m/s), the vehicles waiting
    /// behind, in a tunnel, the last red run.
    pub street: Option<String>,
    pub limit: Option<f32>,
    pub held: usize,
    pub in_tunnel: bool,
    pub red_runs: u32,
    /// The connector lanes the car was last seen before (a red run is
    /// entering a junction while all of them show red).
    approach: Option<Vec<nd_sim::LaneId>>,
    honking: bool,
    /// [remote] Controls held by the remote API for so many seconds.
    pub remote: Option<(Controls, f32)>,
    pub just_started: bool,
}

impl Drive {
    pub fn active(&self) -> bool {
        self.car.is_some()
    }

    /// Put the car on the street nearest `near`, in the lane on its right,
    /// facing along it; the game to 1×. None when there is no street.
    pub fn begin(&mut self, sim: &mut Sim, near: Vec2) -> Option<DriveCar> {
        let car = spawn_point(&sim.world, near)?;
        self.car = Some(car);
        self.chase.restart(None);
        // A little further back than a followed car: the road ahead shows
        // over the roof.
        self.nudge = Nudge { zoom: DRIVE_ZOOM, ..Nudge::default() };
        self.generation = sim.generation;
        self.restore.get_or_insert((sim.speed_index, sim.paused));
        sim.speed_index = 0;
        sim.paused = false;
        self.approach = None;
        self.just_started = true;
        Some(car)
    }

    /// Out of the car: the traffic forgets it, the game's speed comes back.
    pub fn end(&mut self, sim: &mut Sim) -> Option<DriveCar> {
        sim.player_car = None;
        if let Some((speed, paused)) = self.restore.take() {
            sim.speed_index = speed;
            sim.paused = paused;
        }
        self.remote = None;
        self.street = None;
        self.held = 0;
        self.car.take()
    }

    /// Hand the car to the traffic, and read back what it did.
    pub fn tell_traffic(&mut self, sim: &mut Sim) {
        let Some(car) = self.car else {
            sim.player_car = None;
            return;
        };
        // [r7-sim-thread] Handed to the sim thread with the next batch.
        let p = sim.world.player_car(car.pos, car.heading, car.v, car.accel, car.floor);
        sim.player_car = Some(p);
        self.held = sim.world.traffic.player_held.len();
    }

    /// Drive the car on `dt` seconds with these controls (the lane assist
    /// steering when they do not): the frame's step, and the remote
    /// `drive`'s. Returns true on a bump.
    pub fn advance(&mut self, sim: &Sim, mut c: Controls, dt: f32) -> bool {
        let Some(mut car) = self.car else { return false };
        let w = &sim.world;
        if self.assist && c.steer == 0.0 {
            if let Some((_, d, side)) = w.drive_road_at(car.pos, car.floor).and_then(|g| w.drive_lane(&g, car.pos, car.heading)) {
                c.steer = assist_steer(car.heading, d.y.atan2(d.x), side, car.v);
            }
        }
        let bodies = nearby_bodies(sim, car.pos, car.floor);
        let bumped = car.step(&c, dt, &|p, f| w.drive_ground(p, f), &bodies);
        self.car = Some(car);
        bumped
    }

    /// The street, its limit, the tunnel and a red light run, from where
    /// the car is now.
    pub(crate) fn read_road(&mut self, sim: &Sim, flags: &mut UiFlags) {
        let Some(car) = self.car else { return };
        let w = &sim.world;
        let g = w.drive_road_at(car.pos, car.floor);
        self.street = g.and_then(|g| w.drive_street_name(&g));
        self.limit = g.and_then(|g| w.drive_limit_at(&g, car.pos, car.heading));
        self.in_tunnel = g.is_some_and(|g| w.drive_in_tunnel(&g));
        // Into a junction from a lane whose way on was red: a red run.
        match g {
            Some(DriveGround::Road { .. }) => {
                self.approach = g.and_then(|g| w.drive_lane_ahead(&g, car.pos, car.heading));
            }
            Some(DriveGround::Junction { .. }) => {
                if let Some(conns) = self.approach.take() {
                    let lanes = &w.traffic.net.lanes;
                    let red = !conns.is_empty() && conns.iter().all(|&l| lanes.get(l as usize).is_some_and(|l| !l.green));
                    if red && car.v > 3.0 {
                        self.red_runs += 1;
                        flags.message = "You ran a red light.".into();
                    }
                }
            }
            _ => self.approach = None,
        }
    }
}

/// Where a drive starts near `near`: on the nearest street, in the right
/// hand lane, facing along it.
pub fn spawn_point(w: &nd_sim::World, near: Vec2) -> Option<DriveCar> {
    let (sid, _, q) = w.roads.nearest_street(near)?;
    let lanes = w.traffic.net.segment_lanes.get(&sid)?;
    let lane = lanes.iter().filter_map(|&l| w.traffic.net.lanes.get(l as usize)).min_by(|a, b| a.project(q).1.total_cmp(&b.project(q).1))?;
    let (s, _) = lane.project(q);
    let (p, d) = lane.point_at(s);
    let floor = w.drive_road_at(p, w.surface_height(p)).map_or_else(|| w.surface_height(p), |g| g.height());
    let (t, _) = nd_sim::geom::project_on_segment(p, w.roads.segment_points(sid).0, w.roads.segment_points(sid).1);
    let floor = if w.relief { w.road_height(sid, t) } else { floor };
    Some(DriveCar::new(p, d.y.atan2(d.x), floor))
}

/// The other vehicles near `p` where they are drawn this frame.
pub fn nearby_bodies(sim: &Sim, p: Vec2, floor: f32) -> Vec<Body> {
    let w = &sim.world;
    let f = sim.frac();
    let mut out = Vec::new();
    for car in w.traffic.cars.values() {
        if car.pos.distance(p) > 40.0 {
            continue;
        }
        let (pos, heading) = car.pose_at(f);
        if pos.distance(p) > 12.0 || (w.car_height_at(car, pos) - floor).abs() > 2.5 {
            continue;
        }
        // Drawn centred on its position.
        out.push(Body { pos, heading, half_len: car.length() * 0.5, half_wid: nd_sim::traffic::hazard_width(car) * 0.5 });
    }
    out
}

// ── the systems ─────────────────────────────────────────────────────────────

pub struct DrivePlugin;

impl Plugin for DrivePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Drive>()
            .add_systems(Update, drive_input.after(crate::nav::gather).before(crate::camera::control_camera))
            .add_systems(bevy_egui::EguiPrimaryContextPass, hud);
    }
}

/// Keys, the pad and the remote's holds into the car; the car on one frame;
/// the traffic told where it is.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_input(
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut mode: ResMut<crate::camera::CameraMode>,
    mut sim: ResMut<Sim>,
    mut drive: ResMut<Drive>,
    mut flags: ResMut<UiFlags>,
    (nav, settings, mut pad): (Res<NavInput>, Res<NavSettings>, ResMut<NavPad>),
    tools: Res<crate::tools::ToolState>,
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
    sound: Option<Res<crate::audio::Sound>>,
) {
    if *mode != crate::camera::CameraMode::Drive || !drive.active() {
        if sim.player_car.is_some() {
            sim.player_car = None;
        }
        return;
    }
    // A loaded or new city: out of the car (the camera's switch to Orbit
    // ends the drive).
    if drive.generation != sim.generation {
        sim.player_car = None;
        *mode = crate::camera::CameraMode::Orbit;
        return;
    }
    let dt = time.delta_secs().min(0.1);
    let typing = flags.keyboard_captured;
    let held = |k: KeyCode| !typing && keys.pressed(k);
    let key = |k: KeyCode| !typing && keys.just_pressed(k);
    let (mut c, mut look) = drive_keys(held);
    // [laptop-nav] The pad: the arrows drive, the rest is the camera.
    let mut padmove = pad.hold;
    if !pad.tap.is_zero() && dt > 0.0 {
        padmove.add(pad.tap.scaled(TAP_SECS / dt));
    }
    let (pc, pl) = pad_drive(padmove);
    c = c.with(pc).with(Controls { handbrake: pad.handbrake, horn: pad.horn, ..Default::default() });
    look.turn += pl.turn;
    look.tilt += pl.tilt;
    look.zoom += pl.zoom;
    pad.handbrake = false;
    pad.horn = false;
    if let Some((rc, left)) = drive.remote.as_mut() {
        c = c.with(*rc);
        *left -= dt;
        if *left <= 0.0 {
            drive.remote = None;
        }
    }
    if key(KeyCode::KeyV) {
        drive.view = if drive.view == DriveView::Chase { DriveView::Bonnet } else { DriveView::Chase };
    }
    if key(KeyCode::KeyL) {
        drive.assist = !drive.assist;
        flags.message = if drive.assist { "Lane assist on: it keeps you in your lane while you do not steer." } else { "Lane assist off." }.into();
    }
    // The pointer stays free: a right-drag (or Alt+left-drag) looks round,
    // from the cursor's movement.
    for mut co in &mut cursors {
        if co.grab_mode != CursorGrabMode::None {
            co.grab_mode = CursorGrabMode::None;
            co.visible = true;
        }
    }
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    let role = left_drag_role(nav.left_on_map && buttons.pressed(MouseButton::Left), alt, false, false, false, nav.left_dragging);
    let dragging = (buttons.pressed(MouseButton::Right) || role == LeftDrag::Orbit) && !flags.pointer_over_ui;
    let mut turn = Vec2::new(-look.turn * 1.2 * dt, look.tilt * dt);
    if dragging {
        let d = nav.drag_delta();
        let k = settings.turn_speed;
        let tilt_k = if settings.invert_tilt { -k } else { k };
        turn += Vec2::new(d.x * k, d.y * tilt_k) * 0.005;
    }
    let turning = turn != Vec2::ZERO || dragging;
    drive.nudge.turn(turn);
    drive.nudge.settle(dt, turning);
    if look.zoom != 0.0 {
        drive.nudge.zoom_by(look.zoom * 1.5 * dt / 0.12);
    }
    if !flags.pointer_over_ui && !tools.captures_wheel(false) && nav.zoom != 0.0 {
        drive.nudge.zoom_by(nav.zoom);
    }
    drive.behind = look.behind;

    if !drive.active() {
        return;
    }
    let bumped = drive.advance(&sim, c, dt);
    let Some(car) = drive.car else { return };
    if bumped {
        if let Some(b) = car.last_bump {
            flags.message = format!("Bump! Into {}.", b.label());
        }
    }
    // The horn: a honk from the car itself.
    if c.horn && !drive.honking {
        if let Some(s) = sound.as_ref().filter(|s| !s.disabled) {
            let mut u = nd_audio::SpotUpdate::new(0x0d71_7e00 + drive.red_runs as u64 + car.bumps as u64 + (time.elapsed_secs() * 10.0) as u64, nd_audio::spots::SpotKind::Horn, 0.9);
            u.dist = 3.0;
            s.handle.send(nd_audio::Command::Spot(u));
        }
    }
    drive.honking = c.horn;
    drive.read_road(&sim, &mut flags);
    drive.tell_traffic(&mut sim);
}

/// [r5-drive] The camera on the car, after the sim has stepped and the
/// renderer placed the traffic: the chase camera (low behind, swinging
/// round with it; what stands between cut away) or the bonnet view.
#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_camera(
    time: Res<Time>,
    sim: Res<Sim>,
    mode: Res<crate::camera::CameraMode>,
    mut drive: ResMut<Drive>,
    mut follow: ResMut<crate::camera::CameraFollow>,
    mut cut: ResMut<crate::camera::CameraCut>,
    mut q: Query<(&mut crate::camera::OrbitCamera, &mut Transform)>,
) {
    if *mode != crate::camera::CameraMode::Drive {
        return;
    }
    let Some(car) = drive.car else { return };
    let Ok((mut cam, mut tf)) = q.single_mut() else { return };
    let dt = time.delta_secs().min(0.1);
    let w = &sim.world;
    if drive.just_started {
        drive.just_started = false;
        let from = (tf.translation, tf.translation + tf.forward().as_vec3() * 30.0);
        drive.chase.restart(Some(from));
    }
    let dir = car.dir();
    match drive.view {
        DriveView::Bonnet => {
            cut.0.on = false;
            let yaw = car.heading + drive.nudge.look.x + if drive.behind { std::f32::consts::PI } else { 0.0 };
            let pitch = (-0.04 - drive.nudge.look.y * 0.6).clamp(-1.0, 0.8);
            let fwd = Vec3::new(yaw.cos() * pitch.cos(), pitch.sin(), yaw.sin() * pitch.cos());
            let eye = Vec3::new(car.pos.x, car.floor + 1.3, car.pos.y) + Vec3::new(dir.x, 0.0, dir.y) * 0.4;
            tf.translation = eye;
            tf.look_to(fwd, Vec3::Y);
        }
        DriveView::Chase => {
            let sub = Subject { pos: car.pos, heading: car.heading, floor: car.floor, kind: SubjectKind::Car, underground: drive.in_tunnel };
            let mut nudge = drive.nudge;
            if drive.behind {
                nudge.look.x = wrap(nudge.look.x + std::f32::consts::PI);
            }
            let p = SubjectKind::Car.profile();
            let pose = drive.chase.step(&sub, &nudge, dt);
            let eye = if sub.underground {
                Vec3::new(pose.eye.x, pose.eye.y.max(sub.floor + 1.0), pose.eye.z)
            } else {
                clear_ground(pose.eye, pose.focus, |q| ground_under(w, q), 0.6)
            };
            tf.translation = eye;
            tf.look_at(pose.look_at, Vec3::Y);
            let ground_eye = if sub.underground { sub.floor } else { w.surface_height_near(Vec2::new(eye.x, eye.z), sub.floor) };
            cut.0 = Cut::for_pose(&crate::chase::Pose { eye, ..pose }, &p, ground_eye, sub.floor);
            cut.0.on = !follow.cutaway_off;
        }
    }
    // The orbit keeps the same eye (lamps, sounds and the remote API read
    // it), and the Underground view comes on in a tunnel.
    let base = Vec3::new(car.pos.x, car.floor, car.pos.y);
    let off = tf.translation - base;
    let d = off.length().max(0.5);
    cam.target = base;
    cam.distance = d;
    cam.yaw = off.x.atan2(off.z);
    cam.pitch = (off.y / d).clamp(-1.0, 1.0).asin();
    follow.chase_under = drive.in_tunnel;
}

/// The ground the chase camera stays above (as `camera::chase_ground`).
fn ground_under(w: &nd_sim::World, p: Vec2) -> f32 {
    if !w.relief {
        return 0.0;
    }
    let g = w.ground_height(p);
    if !w.landscape.is_flat() && w.landscape.is_water(p) { g.max(nd_sim::landscape::WATER_LEVEL) } else { g }
}

// ── the HUD ─────────────────────────────────────────────────────────────────

/// Speed, the street, its limit, a heading-up map of the streets round the
/// car with a compass, and the keys.
fn hud(mut contexts: bevy_egui::EguiContexts, sim: Res<Sim>, drive: Res<Drive>, mode: Res<crate::camera::CameraMode>, flags: Res<UiFlags>) -> Result {
    use bevy_egui::egui;
    use egui::{Color32, Stroke};
    if *mode != crate::camera::CameraMode::Drive || flags.photo_mode {
        return Ok(());
    }
    let Some(car) = drive.car else { return Ok(()) };
    let ctx = contexts.ctx_mut()?;
    let map = flags.map_rect(ctx.content_rect());
    let gold = crate::ui::theme::GOLD;
    let cyan = Color32::from_rgb(120, 230, 255);
    let kmh = car.v.abs() * 3.6;
    let over = drive.limit.is_some_and(|l| car.v.abs() > l * 1.1 + 0.5);
    egui::Area::new(egui::Id::new("drive_hud")).order(egui::Order::Foreground).fixed_pos(egui::pos2(map.right() - 16.0, map.top() + 16.0)).pivot(egui::Align2::RIGHT_TOP).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).fill(crate::ui::theme::PANEL_FILL).stroke(Stroke::new(1.0, gold.gamma_multiply(0.6))).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
            ui.horizontal(|ui| {
                // The heading-up map.
                let size = 132.0;
                let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
                let c = rect.center();
                let r = size * 0.5 - 2.0;
                let p = ui.painter();
                p.circle(c, r, Color32::from_rgb(8, 10, 20), Stroke::new(1.5, gold.gamma_multiply(0.7)));
                let scale = r / 140.0;
                // Sim offset to screen: the car's heading is up.
                let (s, co) = (car.heading + std::f32::consts::FRAC_PI_2).sin_cos();
                let to_screen = |q: Vec2| {
                    let d = (q - car.pos) * scale;
                    // Rotate by -(heading + 90°) so the heading points up.
                    let x = d.x * co + d.y * s;
                    let y = -d.x * s + d.y * co;
                    c + egui::vec2(x, y)
                };
                let w = &sim.world;
                for sid in w.roads.segments_in_box(car.pos - Vec2::splat(150.0), car.pos + Vec2::splat(150.0)) {
                    let seg = w.roads.segment(sid);
                    if !seg.alive || !seg.is_street() {
                        continue;
                    }
                    let (a, b) = w.roads.segment_points(sid);
                    let (sa, sb) = (to_screen(a), to_screen(b));
                    if sa.distance(c) > r * 1.6 && sb.distance(c) > r * 1.6 {
                        continue;
                    }
                    let col = if seg.tunnel { Color32::from_rgb(90, 80, 140) } else if seg.viaduct || seg.bridge { Color32::from_rgb(200, 160, 255) } else { Color32::from_rgb(70, 110, 140) };
                    let width = (seg.half_width() * 2.0 * scale).max(1.5);
                    let pts = clip_segment(sa, sb, c, r - 1.0);
                    if let Some((x, y)) = pts {
                        p.line_segment([x, y], Stroke::new(width, col));
                    }
                }
                // Other cars, as dots.
                for other in w.traffic.cars.values() {
                    if other.pos.distance(car.pos) < 140.0 {
                        let q = to_screen(other.pos);
                        if q.distance(c) < r - 2.0 {
                            p.circle_filled(q, 1.6, Color32::from_rgb(255, 200, 120));
                        }
                    }
                }
                // The car: an arrow pointing up.
                p.add(egui::Shape::convex_polygon(vec![c + egui::vec2(0.0, -7.0), c + egui::vec2(5.0, 6.0), c + egui::vec2(-5.0, 6.0)], cyan, Stroke::NONE));
                // North on the rim.
                let north = to_screen(car.pos + Vec2::new(0.0, -1.0) / scale * (r - 9.0));
                p.text(north, egui::Align2::CENTER_CENTER, "N", crate::ui::theme::display(12.0), Color32::from_rgb(255, 80, 120));
                ui.vertical(|ui| {
                    let col = if over { Color32::from_rgb(255, 90, 90) } else { cyan };
                    ui.label(egui::RichText::new(format!("{kmh:>3.0}")).font(crate::ui::theme::display(34.0)).color(col));
                    ui.label(egui::RichText::new(if car.v < -0.3 { "km/h  reverse" } else { "km/h" }).color(crate::ui::theme::IVORY));
                    if let Some(l) = drive.limit {
                        ui.label(egui::RichText::new(format!("limit {:.0}", l * 3.6)).color(if over { Color32::from_rgb(255, 90, 90) } else { crate::ui::theme::IVORY }));
                    }
                    let b = bearing(car.heading);
                    ui.label(egui::RichText::new(format!("heading {} {:03.0}°", compass_point(b), b)).color(gold));
                    let street = drive.street.clone().unwrap_or_else(|| if car.on_road { "road".into() } else { "off the road".into() });
                    ui.label(egui::RichText::new(street).color(egui::Color32::WHITE).strong());
                    if drive.in_tunnel {
                        ui.label(egui::RichText::new("in a tunnel").color(crate::ui::theme::IVORY));
                    }
                    if drive.held > 0 {
                        ui.label(egui::RichText::new(format!("{} waiting behind you", drive.held)).color(crate::ui::theme::IVORY));
                    }
                    let extras = [drive.assist.then_some("lane assist"), (drive.view == DriveView::Bonnet).then_some("bonnet view")];
                    let extras: Vec<&str> = extras.into_iter().flatten().collect();
                    if !extras.is_empty() {
                        ui.label(egui::RichText::new(extras.join(" · ")).color(cyan));
                    }
                });
            });
            ui.set_max_width(330.0);
            ui.label(egui::RichText::new(crate::ui::help::CAMERA_DRIVE).small().color(crate::ui::theme::IVORY.gamma_multiply(0.75)));
        });
    });
    Ok(())
}

/// The part of the screen segment `a`-`b` inside the circle round `c` of
/// radius `r`.
fn clip_segment(a: bevy_egui::egui::Pos2, b: bevy_egui::egui::Pos2, c: bevy_egui::egui::Pos2, r: f32) -> Option<(bevy_egui::egui::Pos2, bevy_egui::egui::Pos2)> {
    let d = b - a;
    let f = a - c;
    let qa = d.dot(d);
    if qa < 1e-6 {
        return (f.length() <= r).then_some((a, b));
    }
    let qb = 2.0 * f.dot(d);
    let qc = f.dot(f) - r * r;
    let disc = qb * qb - 4.0 * qa * qc;
    if disc < 0.0 {
        return None;
    }
    let sq = disc.sqrt();
    let t0 = ((-qb - sq) / (2.0 * qa)).max(0.0);
    let t1 = ((-qb + sq) / (2.0 * qa)).min(1.0);
    (t0 < t1).then(|| (a + d * t0, a + d * t1))
}

#[cfg(test)]
#[path = "drive_tests.rs"]
mod tests;
