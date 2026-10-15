//! [chase-cam] The follow camera as a game's chase camera: low behind the
//! followed thing, looking the way it is going, swinging round with it on
//! the turns. Pure maths here (the pose from a heading, the smoothing, the
//! ground clearance, which scenery stands in the way), so it can be tested
//! without an app; `camera::follow_and_fly` drives it and
//! `render::cutaway` cuts away what the camera sees through.
//!
//! Sim positions are `Vec2` (x, y) and headings radians from +x in that
//! plane; the world is Y-up with the sim's y as world z.

use bevy::math::{Vec2, Vec3};

/// What is being chased, for how far back and how high the camera sits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SubjectKind {
    Walker,
    Cyclist,
    /// A pavement delivery robot.
    Bot,
    Car,
    /// A bus, lorry, fire engine or ambulance.
    Big,
    Rv,
    Train,
    Metro,
    /// A ship `length` metres long.
    Ship(f32),
    /// A drone taxi, helicopter or the airlift.
    Air,
}

/// Where the camera sits for a kind of subject, in metres from the
/// subject's floor (the road, pavement, deck or rails under it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Profile {
    /// Behind the subject, along the ground.
    pub back: f32,
    /// The eye's height over the floor.
    pub height: f32,
    /// The height looked at over the floor (about the subject's roof or
    /// shoulders), a little ahead of it so the road in front shows.
    pub aim: f32,
    pub ahead: f32,
    /// About half the subject's length: the cut-away stops short of it.
    pub radius: f32,
}

impl SubjectKind {
    pub fn profile(self) -> Profile {
        match self {
            SubjectKind::Walker => Profile { back: 3.5, height: 2.0, aim: 1.45, ahead: 2.5, radius: 0.5 },
            SubjectKind::Cyclist => Profile { back: 4.0, height: 2.2, aim: 1.45, ahead: 3.5, radius: 1.0 },
            SubjectKind::Bot => Profile { back: 2.8, height: 1.5, aim: 0.6, ahead: 2.0, radius: 0.6 },
            SubjectKind::Car => Profile { back: 7.5, height: 3.1, aim: 1.25, ahead: 5.0, radius: 2.4 },
            SubjectKind::Big => Profile { back: 13.0, height: 5.2, aim: 2.6, ahead: 7.0, radius: 6.0 },
            SubjectKind::Rv => Profile { back: 9.5, height: 3.9, aim: 1.9, ahead: 6.0, radius: 3.6 },
            SubjectKind::Train => Profile { back: 30.0, height: 9.5, aim: 3.5, ahead: 14.0, radius: 9.0 },
            // A tunnel is low: the camera stays inside it.
            SubjectKind::Metro => Profile { back: 22.0, height: 4.4, aim: 2.6, ahead: 12.0, radius: 9.0 },
            SubjectKind::Ship(len) => {
                let len = len.clamp(20.0, 300.0);
                Profile { back: len * 0.9 + 18.0, height: len * 0.28 + 9.0, aim: 6.0, ahead: len * 0.4, radius: len * 0.5 }
            }
            SubjectKind::Air => Profile { back: 20.0, height: 6.5, aim: 0.5, ahead: 10.0, radius: 6.0 },
        }
    }

    /// Vehicles face the way they drive (a reversing car still faces
    /// forward), so the camera keeps behind their nose whatever their
    /// speed. A walker's heading is where they last stepped: only trusted
    /// while they are moving.
    pub fn heading_trusted_standing(self) -> bool {
        !matches!(self, SubjectKind::Walker | SubjectKind::Bot)
    }
}

/// The player's nudge on the chase camera: a turn round the subject and a
/// tilt (right-drag, Q/E), easing back behind it once let go, and the
/// zoom (the wheel).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Nudge {
    /// Turn (radians, round the subject) and tilt (radians, up).
    pub look: Vec2,
    /// Times the profile's distance.
    pub zoom: f32,
    /// Seconds since the player last turned it.
    pub idle: f32,
}

impl Default for Nudge {
    fn default() -> Self {
        Self { look: Vec2::ZERO, zoom: 1.0, idle: 0.0 }
    }
}

pub const ZOOM_MIN: f32 = 0.55;
pub const ZOOM_MAX: f32 = 3.0;
/// How long a turn by the player holds before it eases back.
pub const NUDGE_HOLD: f32 = 1.2;
const TILT_MIN: f32 = -0.25;
const TILT_MAX: f32 = 1.1;

impl Nudge {
    /// The player turned it by `d` (turn, tilt) this frame.
    pub fn turn(&mut self, d: Vec2) {
        if d == Vec2::ZERO {
            return;
        }
        self.look.x = wrap(self.look.x + d.x);
        self.look.y = (self.look.y + d.y).clamp(TILT_MIN, TILT_MAX);
        self.idle = 0.0;
    }

    /// The wheel: `lines` up zooms in.
    pub fn zoom_by(&mut self, lines: f32) {
        self.zoom = (self.zoom * (1.0 - lines * 0.12)).clamp(ZOOM_MIN, ZOOM_MAX);
    }

    /// Time passing with nobody turning it: after a moment it swings back
    /// behind the subject.
    pub fn settle(&mut self, dt: f32, held: bool) {
        if held {
            self.idle = 0.0;
            return;
        }
        self.idle += dt;
        if self.idle > NUDGE_HOLD {
            self.look *= (-2.2 * dt).exp();
            if self.look.length() < 1e-3 {
                self.look = Vec2::ZERO;
            }
        }
    }
}

/// An angle wrapped to (-pi, pi].
pub fn wrap(a: f32) -> f32 {
    let t = std::f32::consts::TAU;
    let r = (a + std::f32::consts::PI).rem_euclid(t) - std::f32::consts::PI;
    if r <= -std::f32::consts::PI { r + t } else { r }
}

/// What the camera knows of the subject this frame.
#[derive(Clone, Copy, Debug)]
pub struct Subject {
    pub pos: Vec2,
    pub heading: f32,
    /// The floor under it (road, pavement, deck, rails, water).
    pub floor: f32,
    pub kind: SubjectKind,
    /// In a tunnel or the metro: no ground clamps (the floor is the
    /// tunnel's).
    pub underground: bool,
}

/// The chase camera's smoothed state.
#[derive(Clone, Copy, Debug, Default)]
pub struct Chase {
    /// The heading the camera swings behind (sim radians).
    yaw: Option<f32>,
    /// The floor height, eased so steps in the decks do not jolt.
    floor: Option<f32>,
    last_pos: Option<Vec2>,
    /// Ground speed, smoothed (m/s).
    pub speed: f32,
    /// The camera's eye and look point when the chase took over, and how
    /// far (0..1) it has swung in from there.
    from: Option<(Vec3, Vec3)>,
    intro: f32,
}

/// Fastest the camera swings round (radians per second): a U-turn or a
/// heading that flips swings round in about a second and a half.
const MAX_SWING: f32 = 2.4;
/// How long the swing in from where the camera was takes (s).
const INTRO: f32 = 0.9;

/// A pose for the camera: where the eye is and the point it looks at.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub eye: Vec3,
    pub look_at: Vec3,
    /// The point the cut-away aims at: the middle of the subject.
    pub focus: Vec3,
}

impl Chase {
    /// Start over (a new subject), swinging in from the camera's pose.
    pub fn restart(&mut self, from: Option<(Vec3, Vec3)>) {
        *self = Chase { from, ..Chase::default() };
    }

    /// The smoothed heading the camera is behind (sim radians), if any yet.
    pub fn yaw(&self) -> Option<f32> {
        self.yaw
    }

    /// One frame: ease the heading, floor and speed towards the subject's
    /// and place the camera behind it.
    pub fn step(&mut self, s: &Subject, nudge: &Nudge, dt: f32) -> Pose {
        let dt = dt.clamp(0.0, 0.1);
        let p = s.kind.profile();
        // Speed from how far it moved: a jump (a teleport, a new tick
        // after a pause) does not count.
        let mut now = 0.0;
        if let Some(last) = self.last_pos {
            let moved = last.distance(s.pos);
            if dt > 0.0 && moved < 60.0 {
                now = moved / dt;
                self.speed += (now - self.speed) * (1.0 - (-4.0 * dt).exp());
            }
        }
        self.last_pos = Some(s.pos);
        // The heading to swing behind: a walker standing still keeps the
        // last one (their heading wanders while they wait).
        let trusted = s.kind.heading_trusted_standing() || now > 0.35;
        let yaw = match self.yaw {
            None => s.heading,
            Some(y) if !trusted => y,
            Some(y) => {
                let d = wrap(s.heading - y);
                // Slow to swing when barely moving (a car shuffling at a
                // junction), brisker at speed; never faster than MAX_SWING.
                let rate = 1.6 + (self.speed / 8.0).min(1.0) * 2.4;
                let step = d * (1.0 - (-rate * dt).exp());
                wrap(y + step.clamp(-MAX_SWING * dt, MAX_SWING * dt))
            }
        };
        self.yaw = Some(yaw);
        let floor = match self.floor {
            Some(f) if (s.floor - f).abs() < 25.0 => f + (s.floor - f) * (1.0 - (-7.0 * dt).exp()),
            _ => s.floor,
        };
        self.floor = Some(floor);
        let pose = place(s.pos, floor, yaw, &p, nudge);
        // Swing in from where the camera was when the chase began.
        let Some((from_eye, from_look)) = self.from else { return pose };
        self.intro = (self.intro + dt / INTRO).min(1.0);
        if self.intro >= 1.0 {
            self.from = None;
            return pose;
        }
        let t = self.intro;
        let e = t * t * (3.0 - 2.0 * t);
        Pose { eye: from_eye.lerp(pose.eye, e), look_at: from_look.lerp(pose.look_at, e), focus: pose.focus }
    }
}

/// The camera behind a subject at `pos` on `floor` facing `yaw` (sim
/// radians), as `profile` and the player's nudge put it.
pub fn place(pos: Vec2, floor: f32, yaw: f32, p: &Profile, nudge: &Nudge) -> Pose {
    let aim = Vec3::new(pos.x, floor + p.aim, pos.y);
    let turn = yaw + nudge.look.x;
    let fwd = Vec3::new(turn.cos(), 0.0, turn.sin());
    // The eye's elevation over the aim point, tilted by the player.
    let rise = p.height - p.aim;
    let base = rise.atan2(p.back);
    let elev = (base + nudge.look.y).clamp(-0.05, 1.35);
    let r = (p.back * p.back + rise * rise).sqrt() * nudge.zoom.clamp(ZOOM_MIN, ZOOM_MAX);
    let eye = aim - fwd * (elev.cos() * r) + Vec3::Y * (elev.sin() * r);
    // Look a little ahead of the subject, so the road in front is in view.
    let look_at = aim + fwd * p.ahead;
    Pose { eye, look_at, focus: Vec3::new(pos.x, floor + p.aim.min(p.height * 0.6), pos.y) }
}

/// Keep the eye above the ground (`ground(x, z)`, terrain and water) by
/// `clear`, and pull it in towards `aim` where a hill stands between them.
pub fn clear_ground(eye: Vec3, aim: Vec3, ground: impl Fn(Vec2) -> f32, clear: f32) -> Vec3 {
    let lift = |p: Vec3| {
        let g = ground(Vec2::new(p.x, p.z)) + clear;
        Vec3::new(p.x, p.y.max(g), p.z)
    };
    let eye = lift(eye);
    // March out from the subject: the first sample under the ground ends
    // the line of sight; the eye comes in to just short of it.
    const N: usize = 16;
    let mut ok = 0.0f32;
    for k in 1..=N {
        let f = k as f32 / N as f32;
        let p = aim.lerp(eye, f);
        if p.y < ground(Vec2::new(p.x, p.z)) + clear * 0.5 {
            return lift(aim.lerp(eye, ok.max(0.15)));
        }
        ok = f;
    }
    eye
}

/// What the camera cuts away: everything of the scenery inside a cone from
/// the eye to the subject (narrow at the eye, about the subject's size at
/// its end, stopping short of it), above a floor running from the ground
/// under the eye to the subject's, so the road it drives on stays.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cut {
    pub on: bool,
    pub eye: Vec3,
    pub target: Vec3,
    /// Stops this far short of the target.
    pub margin: f32,
    pub r_eye: f32,
    pub r_target: f32,
    pub ground_eye: f32,
    pub ground_target: f32,
}

/// Scenery this far over the floor line stays (kerbs, the road's paint).
pub const CUT_FLOOR: f32 = 0.35;
/// The cone's edge dithers over this many metres.
pub const CUT_SOFT: f32 = 1.0;

impl Cut {
    pub fn for_pose(pose: &Pose, p: &Profile, ground_eye: f32, floor: f32) -> Cut {
        Cut { on: true, eye: pose.eye, target: pose.focus, margin: p.radius + 0.4, r_eye: 1.5, r_target: p.radius * 0.7 + 2.4, ground_eye, ground_target: floor }
    }

    /// How deep inside the cut `p` is (metres to its nearest edge); `None`
    /// outside. The shader (`render::cutaway`) does the same per pixel.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn depth(&self, p: Vec3) -> Option<f32> {
        if !self.on {
            return None;
        }
        let axis = self.target - self.eye;
        let len = axis.length();
        if len < 0.01 {
            return None;
        }
        let d = axis / len;
        let rel = p - self.eye;
        let s = rel.dot(d);
        let end = len - self.margin;
        if s <= 0.0 || s >= end {
            return None;
        }
        let f = s / len;
        if p.y < self.ground_eye + (self.ground_target - self.ground_eye) * f + CUT_FLOOR {
            return None;
        }
        let r = self.r_eye + (self.r_target - self.r_eye) * f;
        let edge = (r - (rel - d * s).length()).min(end - s);
        (edge > 0.0).then_some(edge)
    }

    /// Fully cut (past the dithered edge).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn cuts(&self, p: Vec3) -> bool {
        self.depth(p).is_some_and(|e| e >= CUT_SOFT)
    }

    /// Might a mesh within `radius` of `center` reach into the cut? A
    /// cheap test for which meshes get the cutting material.
    pub fn touches(&self, center: Vec3, radius: f32) -> bool {
        if !self.on {
            return false;
        }
        let ab = self.target - self.eye;
        let t = ((center - self.eye).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
        let closest = self.eye + ab * t;
        center.distance(closest) <= radius + self.r_eye.max(self.r_target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn car(pos: Vec2, heading: f32) -> Subject {
        Subject { pos, heading, floor: 0.0, kind: SubjectKind::Car, underground: false }
    }

    #[test]
    fn sits_low_behind_looking_ahead() {
        for kind in [SubjectKind::Car, SubjectKind::Walker, SubjectKind::Cyclist] {
            let p = kind.profile();
            let pose = place(Vec2::new(10.0, 20.0), 0.0, 0.0, &p, &Nudge::default());
            // Heading +x: the eye is behind (smaller x), on the line.
            assert!(pose.eye.x < 10.0 - p.back * 0.9 && pose.eye.x > 10.0 - p.back * 1.1, "{kind:?} {:?}", pose.eye);
            assert!((pose.eye.z - 20.0).abs() < 1e-4);
            assert!((pose.eye.y - p.height).abs() < 0.05, "{kind:?} {:?}", pose.eye);
            assert!(pose.look_at.x > 10.0);
        }
        let c = SubjectKind::Car.profile();
        assert!((2.5..=4.0).contains(&c.height) && (6.0..=9.0).contains(&c.back));
        let w = SubjectKind::Walker.profile();
        assert!((1.8..=2.2).contains(&w.height) && (3.0..=4.0).contains(&w.back));
        // Big things sit further back.
        for big in [SubjectKind::Big, SubjectKind::Train, SubjectKind::Ship(120.0), SubjectKind::Air, SubjectKind::Metro] {
            assert!(big.profile().back > c.back, "{big:?}");
        }
    }

    #[test]
    fn heading_north_puts_the_eye_south() {
        let pose = place(Vec2::ZERO, 5.0, std::f32::consts::FRAC_PI_2, &SubjectKind::Car.profile(), &Nudge::default());
        assert!(pose.eye.z < -6.0 && pose.eye.x.abs() < 1e-3);
        assert!(pose.eye.y > 5.0 + 2.5);
    }

    #[test]
    fn zoom_and_turn_are_limited_and_ease_back() {
        let mut n = Nudge::default();
        for _ in 0..100 {
            n.zoom_by(5.0);
        }
        assert_eq!(n.zoom, ZOOM_MIN);
        for _ in 0..100 {
            n.zoom_by(-5.0);
        }
        assert_eq!(n.zoom, ZOOM_MAX);
        n.turn(Vec2::new(2.0, 5.0));
        assert!(n.look.y <= TILT_MAX);
        // Held: stays. Let go: holds a moment, then eases back to behind.
        n.settle(0.5, true);
        assert!((n.look.x - 2.0).abs() < 1e-6);
        for _ in 0..30 {
            n.settle(1.0 / 60.0, false);
        }
        assert!((n.look.x - 2.0).abs() < 1e-6, "holds for a moment");
        for _ in 0..600 {
            n.settle(1.0 / 60.0, false);
        }
        assert!(n.look.length() < 0.01);
    }

    /// Driving straight at a steady speed the camera holds still behind;
    /// at a turn it swings round smoothly, never faster than MAX_SWING.
    #[test]
    fn swings_round_a_turn_smoothly() {
        let mut c = Chase::default();
        let n = Nudge::default();
        let dt = 1.0 / 60.0;
        let mut pos = Vec2::ZERO;
        let mut last_yaw = 0.0;
        for i in 0..600 {
            let heading = if i < 120 { 0.0 } else { std::f32::consts::FRAC_PI_2 };
            pos += Vec2::new(heading.cos(), heading.sin()) * 10.0 * dt;
            let pose = c.step(&car(pos, heading), &n, dt);
            let y = c.yaw().unwrap();
            assert!(wrap(y - last_yaw).abs() <= MAX_SWING * dt + 1e-5, "frame {i}");
            last_yaw = y;
            assert!(pose.eye.y > 2.0);
        }
        assert!((c.yaw().unwrap() - std::f32::consts::FRAC_PI_2).abs() < 0.01);
    }

    /// A jittering heading (stopped at a junction, shuffling) barely
    /// moves the camera; a walker standing still keeps the last heading.
    #[test]
    fn no_jitter_when_slow_or_stopped() {
        let mut c = Chase::default();
        let n = Nudge::default();
        let dt = 1.0 / 60.0;
        c.step(&car(Vec2::ZERO, 0.0), &n, dt);
        let mut worst: f32 = 0.0;
        for i in 0..240 {
            let h = if i % 2 == 0 { 0.08 } else { -0.08 };
            c.step(&car(Vec2::ZERO, h), &n, dt);
            worst = worst.max(c.yaw().unwrap().abs());
        }
        assert!(worst < 0.01, "jitter {worst}");
        // A walker who stops and turns on the spot: the camera stays.
        let mut c = Chase::default();
        let walker = |pos: Vec2, heading: f32| Subject { pos, heading, floor: 0.0, kind: SubjectKind::Walker, underground: false };
        let mut pos = Vec2::ZERO;
        for _ in 0..120 {
            pos.x += 1.4 * dt;
            c.step(&walker(pos, 0.0), &n, dt);
        }
        for _ in 0..240 {
            c.step(&walker(pos, 2.5), &n, dt);
        }
        assert!(c.yaw().unwrap().abs() < 0.05, "{:?}", c.yaw());
        // Moving off the other way, it swings round.
        for _ in 0..300 {
            pos.x -= 1.4 * dt;
            c.step(&walker(pos, std::f32::consts::PI), &n, dt);
        }
        assert!(wrap(c.yaw().unwrap() - std::f32::consts::PI).abs() < 0.05);
    }

    /// A heading that flips (a vehicle turned round in one tick) swings
    /// round over a second or more, not in a frame; the eye never jumps.
    #[test]
    fn a_flip_does_not_jump() {
        let mut c = Chase::default();
        let n = Nudge::default();
        let dt = 1.0 / 60.0;
        let mut last = c.step(&car(Vec2::ZERO, 0.0), &n, dt).eye;
        let mut frames = 0;
        for _ in 0..300 {
            let e = c.step(&car(Vec2::ZERO, std::f32::consts::PI), &n, dt).eye;
            assert!(e.distance(last) < 0.5, "eye jumped {}", e.distance(last));
            last = e;
            if wrap(c.yaw().unwrap() - std::f32::consts::PI).abs() > 0.05 {
                frames += 1;
            }
        }
        assert!(frames > 60, "swung round in {frames} frames");
    }

    #[test]
    fn swings_in_from_where_the_camera_was() {
        let mut c = Chase::default();
        let far = Vec3::new(0.0, 200.0, 200.0);
        c.restart(Some((far, Vec3::ZERO)));
        let n = Nudge::default();
        let first = c.step(&car(Vec2::ZERO, 0.0), &n, 1.0 / 60.0);
        assert!(first.eye.distance(far) < 5.0);
        let mut pose = first;
        for _ in 0..120 {
            pose = c.step(&car(Vec2::ZERO, 0.0), &n, 1.0 / 60.0);
        }
        assert_eq!(pose, place(Vec2::ZERO, 0.0, 0.0, &SubjectKind::Car.profile(), &n));
    }

    #[test]
    fn never_below_ground() {
        // A hill behind the car rising to 12 m.
        let hill = |p: Vec2| if p.x < -3.0 { (-p.x - 3.0) * 3.0 } else { 0.0f32 }.min(12.0);
        let aim = Vec3::new(0.0, 1.25, 0.0);
        let eye = Vec3::new(-7.5, 3.1, 0.0);
        let e = clear_ground(eye, aim, hill, 0.6);
        assert!(e.y >= hill(Vec2::new(e.x, e.z)) + 0.6 - 1e-4, "{e:?}");
        // Pulled in towards the car rather than hoisted over the hill.
        assert!(e.x > -7.5);
        // Flat ground: untouched.
        assert_eq!(clear_ground(eye, aim, |_| 0.0, 0.6), eye);
        // Any eye at all, on any ground, ends up over it.
        for k in 0..50 {
            let g = |p: Vec2| (p.x * 0.3 + k as f32).sin() * 6.0 + p.y * 0.1;
            let eye = Vec3::new(-8.0 + k as f32 * 0.3, -5.0 + k as f32 * 0.4, 3.0);
            let e = clear_ground(eye, aim, g, 0.5);
            assert!(e.y >= g(Vec2::new(e.x, e.z)) + 0.5 - 1e-4, "{k}: {e:?}");
        }
    }

    /// The cut takes what stands between the eye and the car, not the road
    /// under it, the car itself, or what is beside or behind.
    #[test]
    fn cuts_what_stands_between() {
        let p = SubjectKind::Car.profile();
        let pose = place(Vec2::ZERO, 0.0, 0.0, &p, &Nudge::default());
        let cut = Cut::for_pose(&pose, &p, 0.0, 0.0);
        // A wall half way between, at eye-line height: cut.
        let mid = pose.eye.lerp(pose.focus, 0.45);
        assert!(cut.cuts(mid), "{mid:?}");
        // The road under the line: kept.
        assert!(cut.depth(Vec3::new(mid.x, 0.1, mid.z)).is_none());
        // The car itself (at the focus) and its back bumper: kept.
        assert!(cut.depth(pose.focus).is_none());
        assert!(cut.depth(pose.focus - Vec3::X * 2.0).is_none());
        // Beyond the car, behind the camera, or well off to the side: kept.
        assert!(cut.depth(pose.focus + Vec3::X * 5.0).is_none());
        assert!(cut.depth(pose.eye - Vec3::X * 2.0).is_none());
        assert!(cut.depth(mid + Vec3::Z * 6.0).is_none());
        // Off: nothing.
        assert!(Cut::default().depth(mid).is_none());
    }

    /// Which meshes get the cutting material: those whose bounds reach the
    /// cone, not those streets away.
    #[test]
    fn picks_the_occluders_along_the_segment() {
        let p = SubjectKind::Car.profile();
        let pose = place(Vec2::new(100.0, 50.0), 0.0, 0.7, &p, &Nudge::default());
        let cut = Cut::for_pose(&pose, &p, 0.0, 0.0);
        let mid = pose.eye.lerp(pose.focus, 0.5);
        // A building block whose bounds hold the middle of the line.
        assert!(cut.touches(mid + Vec3::new(8.0, 10.0, 0.0), 20.0));
        // The block over the eye.
        assert!(cut.touches(pose.eye + Vec3::Y * 15.0, 16.0));
        // A block a street away.
        assert!(!cut.touches(mid + Vec3::new(0.0, 0.0, 60.0), 20.0));
        assert!(!Cut::default().touches(mid, 1000.0));
    }

    #[test]
    fn wrap_is_in_range() {
        for k in -40..40 {
            let a = wrap(k as f32 * 0.7);
            assert!(a > -std::f32::consts::PI - 1e-6 && a <= std::f32::consts::PI + 1e-6);
        }
    }
}
