//! [scene] Things standing in or beside the road that traffic slows for and
//! steers round, and the slow off-lane moves of a vehicle at work.
//!
//! - Scene zones (`SceneZone`): a fire engine parked at the kerb at a fire
//!   (set by the world each tick, `Traffic::scenes`), and any vehicle doing
//!   a manoeuvre (below), taken from where it is at every substep. A driver
//!   coming up to one slows to a crawl (everyone looks) and eases sideways
//!   away from it where its body would touch, up to `MAX_DODGE` (a little
//!   over the centre line or into the next lane); with no room to pass it
//!   stops short of it and waits (and eventually looks for another way, as
//!   for any blocked lane). Sirens are not held up.
//! - Manoeuvres (`StuntKind::Manoeuvre`): a tow truck at work leaves the
//!   lanes to drive slowly against a one-way street for the last stretch
//!   to its job, or to turn round in the street (a U-turn), with its
//!   ambers on. It follows a short list of waypoints (`DriveState::path`)
//!   at a crawl, nose along its way, and turns to face `face` at the last
//!   one. The world puts it back on the lanes (`end_stunt`) when it is done.

use super::*;

/// How far ahead (m) a driver reacts to a scene.
const SCENE_LOOK: f32 = 35.0;
/// Speed (m/s) going past a scene.
const SCENE_CRAWL: f32 = 4.5;
/// Furthest (m) a driver eases sideways to get past one.
const MAX_DODGE: f32 = 1.9;
/// Clearance (m) kept from a scene's body.
const SCENE_CLEAR: f32 = 0.35;
/// Sideways speed (m/s) of the dodge.
const DODGE_RATE: f32 = 1.4;

/// Something standing in or beside the road: where, which way it faces,
/// its half length and half width, and the speed past it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneZone {
    pub pos: Vec2,
    pub heading: f32,
    pub half_len: f32,
    pub half_wid: f32,
}

impl SceneZone {
    pub fn new(pos: Vec2, heading: f32, half_len: f32, half_wid: f32) -> Self {
        Self { pos, heading, half_len, half_wid }
    }
}

/// What a scene ahead asks of a driver: a gap to stop in (no room to
/// pass), a speed cap, and how far sideways to ease (m, + right).
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct SceneAsk {
    pub stop: Option<f32>,
    pub cap: Option<f32>,
    pub dodge: f32,
}

impl Traffic {
    /// Every scene this substep: the world's, and the vehicles doing a
    /// manoeuvre off the lanes.
    pub(super) fn scene_zones(&self) -> Vec<SceneZone> {
        let mut z = self.scenes.clone();
        for c in self.cars.values() {
            if let Some(Stunt { kind: StuntKind::Manoeuvre { .. }, .. }) = c.drive.stunt {
                z.push(SceneZone::new(c.pos, c.heading, c.length() * 0.5 + 0.4, super::hazard::body_width(c) * 0.5 + 0.2));
            }
        }
        z
    }

    /// What the scenes ahead of `car` (on `lane`) ask of it.
    pub(super) fn scene_ask(&self, car: &Car, lane: &Lane, zones: &[SceneZone]) -> SceneAsk {
        let mut ask = SceneAsk::default();
        if car.siren || zones.is_empty() {
            return ask;
        }
        let (_, d) = lane.point_at(car.s);
        let right = Vec2::new(-d.y, d.x);
        let (half_c, len_c) = (super::hazard::body_width(car) * 0.5, car.length());
        // The lane's own centre (the car's lateral offsets ride on top).
        let base = car.pos - right * (car.lateral + car.drive.weave + car.drive.offlaw.side + car.drive.dodge);
        for z in zones {
            let rel = z.pos - base;
            let along = rel.dot(d);
            if rel.length() > SCENE_LOOK + 15.0 {
                continue;
            }
            let zd = Vec2::new(z.heading.cos(), z.heading.sin());
            let (c, s) = (zd.dot(d).abs(), zd.perp_dot(d).abs());
            let hl = c * z.half_len + s * z.half_wid;
            let hw = s * z.half_len + c * z.half_wid;
            if along < -hl - len_c || along > SCENE_LOOK + hl {
                continue;
            }
            let lat = rel.dot(right);
            // Does its body reach our path, and which way round?
            let (lo, hi) = (lat - hw - SCENE_CLEAR, lat + hw + SCENE_CLEAR);
            let touches = hi > -half_c && lo < half_c;
            ask.cap = Some(ask.cap.map_or(SCENE_CRAWL, |x: f32| x.min(SCENE_CRAWL)));
            if !touches {
                continue;
            }
            // Ease away from it: left of it when it stands to the right.
            let want = if lat >= 0.0 { lo - half_c } else { hi + half_c };
            if want.abs() <= MAX_DODGE {
                if want.abs() > ask.dodge.abs() {
                    ask.dodge = want;
                }
            } else if along > 0.0 {
                // No room: stop short of it.
                let g = (along - hl - 1.5).max(0.0);
                ask.stop = Some(ask.stop.map_or(g, |x: f32| x.min(g)));
            }
        }
        ask
    }

    /// Ease every car's dodge towards what its scene asks (back to the
    /// lane once past).
    pub(super) fn ease_dodge(car: &mut Car, want: f32, dt: f32) {
        let step = DODGE_RATE * dt;
        let d = want - car.drive.dodge;
        car.drive.dodge += d.clamp(-step, step);
    }

    /// Take a vehicle off the lanes for a manoeuvre along `path` (the
    /// first waypoint is driven to first) at `speed`, ending facing `face`.
    pub fn begin_manoeuvre(&mut self, id: CarId, mut path: Vec<Vec2>, face: f32, speed: f32) {
        if path.is_empty() || !self.cars.contains_key(id) {
            return;
        }
        let to = path.remove(0);
        self.begin_stunt(id, StuntKind::Manoeuvre { to, face, speed });
        let c = &mut self.cars[id];
        c.drive.path = path;
        c.v = 0.0;
    }

    /// Done with its manoeuvre: at its last waypoint and turned to face.
    pub fn manoeuvre_done(&self, id: CarId) -> bool {
        self.cars.get(id).is_some_and(|c| matches!(c.drive.stunt, Some(Stunt { kind: StuntKind::Manoeuvre { .. }, settled: true, .. })) && c.drive.path.is_empty())
    }

    /// Is the vehicle doing a manoeuvre (off the lanes, at work)?
    pub fn in_manoeuvre(&self, id: CarId) -> bool {
        self.cars.get(id).is_some_and(|c| matches!(c.drive.stunt, Some(Stunt { kind: StuntKind::Manoeuvre { .. }, .. })))
    }
}

/// One substep of a manoeuvre: on to the waypoint at its speed (the next
/// from `path` once there), then round to face. Returns the new pose and
/// the speed.
pub(super) fn step_manoeuvre(st: &mut Stunt, path: &mut Vec<Vec2>, pos: Vec2, heading: f32, dt: f32) -> (Vec2, f32, f32) {
    let StuntKind::Manoeuvre { to, face, speed } = st.kind else { return (pos, heading, 0.0) };
    if st.settled {
        return (pos, lerp_angle(heading, face, (dt * 1.2).min(1.0)), 0.0);
    }
    let d = to - pos;
    let len = d.length();
    if len < 0.3 {
        match (!path.is_empty()).then(|| path.remove(0)) {
            Some(next) => st.kind = StuntKind::Manoeuvre { to: next, face, speed },
            None => st.settled = true,
        }
        return (to, heading, 0.0);
    }
    let dir = d / len;
    let want = dir.y.atan2(dir.x);
    // Slow while the nose is still coming round.
    let off = ((want - heading + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI).abs();
    let turn = (1.0 - off / std::f32::consts::PI).max(0.3);
    let v = speed * turn;
    let step = (v * dt).min(len);
    (pos + dir * step, lerp_angle(heading, want, (dt * 1.6).min(1.0)), v)
}
