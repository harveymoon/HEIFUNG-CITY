//! [rail-crossing] Level crossing gear: where the signal posts, lamps and
//! barrier arms of a crossing stand (`rigs`), the fixed parts (posts,
//! crossbucks, lamp backboards and hoods, bells, barrier housings:
//! `furniture`), and an arm in its own frame (`arm_geometry`) for the app
//! to swing about its pivot.
//!
//! Each street arm whose traffic arrives at the crossing gets a post on the
//! verge to the arriving traffic's right, just in from the stop line, with
//! two red lamps side by side facing the traffic, a crossbuck above them
//! and a bell on top. A crossing guarded by barriers puts a half-barrier at
//! the same spot, its arm reaching over the arriving lanes to the centre
//! line, and a short gate over the pavement behind it; a footpath crossing
//! gets a small gate across the path. Lamps and arms are the app's to
//! light and move.

use crate::buildings::parts::{mul, rgba};
use crate::palette::Palette;
use crate::surface::{Geometry, Surface::*};
use glam::{Vec2, Vec3};
use nd_sim::level_crossing::{CrossingGuard, LevelCrossing};
use nd_sim::zoning::SIDEWALK;

/// The pavement's top, as the street mesh lays it.
pub const KERB_H: f32 = 0.16;
/// Height of a barrier's pivot over the ground (a footpath gate's lower).
pub const PIVOT_Y: f32 = 1.0;
pub const GATE_PIVOT_Y: f32 = 0.85;
/// Height of the flashing lamps' centres.
pub const LAMP_Y: f32 = 2.75;
/// The lamps' spacing either side of the post.
pub const LAMP_SPREAD: f32 = 0.38;
/// Size of a lamp's lens.
pub const LAMP_SIZE: f32 = 0.27;
/// How far an arm stands up from the horizontal when raised (radians).
pub const RAISED: f32 = 1.48;
/// Red of the barrier's stripes.
pub const STRIPE_RED: [f32; 3] = [0.62, 0.05, 0.04];

/// A barrier arm (or gate): its pivot, the way it points when down, its
/// length, and whether it is a small one (a pavement or path gate).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ArmRig {
    pub pivot: Vec3,
    pub along: Vec2,
    pub len: f32,
    pub small: bool,
}

impl ArmRig {
    /// Where its lamps sit along it, in its own frame (+x along the arm).
    pub fn lamps(&self) -> Vec<Vec3> {
        if self.small {
            vec![Vec3::new(self.len - 0.1, 0.07, 0.0)]
        } else {
            vec![Vec3::new(self.len - 0.12, 0.09, 0.0), Vec3::new(self.len * 0.5, 0.09, 0.0)]
        }
    }
}

/// One signal post with its lamps and arms.
#[derive(Clone, Debug, PartialEq)]
pub struct CrossingRig {
    pub post: Vec2,
    /// The way the lamps face: out towards the arriving traffic.
    pub facing: Vec2,
    /// The two lamps, left then right as the arriving driver sees them.
    pub lamps: [Vec3; 2],
    pub arms: Vec<ArmRig>,
}

/// Where a crossing's gear stands.
pub fn rigs(c: &LevelCrossing) -> Vec<CrossingRig> {
    let mut out = Vec::new();
    for a in &c.arms {
        if !a.path && a.lanes_in == 0 {
            // Traffic only leaves this way (a one-way street): nothing faces it.
            continue;
        }
        let h = a.heading();
        let right = Vec2::new(-h.y, h.x);
        let pivot = a.pivot(c.pos);
        let post = pivot + right * if a.path { 0.45 } else { 0.75 };
        let facing = a.away;
        // The driver's left is the post's `-right`.
        let lamp = |side: f32| {
            let p = post + facing * 0.16 + right * side * LAMP_SPREAD;
            Vec3::new(p.x, LAMP_Y, p.y)
        };
        let mut arms = Vec::new();
        match c.guard {
            CrossingGuard::Barriers if !a.path => {
                arms.push(ArmRig { pivot: Vec3::new(pivot.x, PIVOT_Y, pivot.y), along: -right, len: a.arm_length(), small: false });
                // The pavement behind: a short gate out over it.
                arms.push(ArmRig { pivot: Vec3::new(pivot.x, GATE_PIVOT_Y, pivot.y), along: right, len: (SIDEWALK - 0.9).max(1.0), small: true });
            }
            CrossingGuard::Footpath => {
                arms.push(ArmRig { pivot: Vec3::new(pivot.x, GATE_PIVOT_Y, pivot.y), along: -right, len: a.arm_length(), small: true });
            }
            _ => {}
        }
        out.push(CrossingRig { post, facing, lamps: [lamp(-1.0), lamp(1.0)], arms });
    }
    out
}

/// A flat board facing both ways: `along` and `up` in its plane.
fn board(g: &mut Geometry, center: Vec3, along: Vec3, up: Vec3, w: f32, h: f32, facing: Vec3, color: [f32; 4]) {
    g.part(Matte).decal(center + facing * 0.012, along, up, w, h, facing, color);
    g.part(Matte).decal(center - facing * 0.012, along, up, w, h, -facing, color);
}

/// The fixed parts of a crossing's gear, at ground height 0.
pub fn furniture(g: &mut Geometry, pal: &Palette, rigs: &[CrossingRig]) {
    let white = rgba(mul(pal.chalk, 0.95));
    let grey = rgba(mul(pal.steel, 0.7));
    let black = rgba([0.02, 0.02, 0.025]);
    let red = rgba(STRIPE_RED);
    for r in rigs {
        let f3 = Vec3::new(r.facing.x, 0.0, r.facing.y);
        let side = Vec2::new(r.facing.y, -r.facing.x);
        let s3 = Vec3::new(side.x, 0.0, side.y);
        let yaw = r.facing.y.atan2(r.facing.x);
        // Footing and post.
        g.part(Concrete).boxed(Vec3::new(r.post.x, KERB_H + 0.08, r.post.y), Vec3::new(0.5, 0.16, 0.5), yaw, rgba(mul(pal.concrete[0], 0.9)));
        g.part(Metal).cylinder(r.post, 0.075, 0.065, KERB_H, 4.25, 8, grey);
        // The lamps' crossbar, black backboard and hoods.
        let c = Vec3::new(r.post.x, LAMP_Y, r.post.y) + f3 * 0.1;
        g.part(Matte).boxed(c - f3 * 0.02, Vec3::new(0.06, 0.08, LAMP_SPREAD * 2.0 + 0.2), yaw, grey);
        board(g, c + f3 * 0.02, s3, Vec3::Y, LAMP_SPREAD * 2.0 + 0.55, 0.62, f3, black);
        for l in &r.lamps {
            g.part(Matte).boxed(Vec3::new(l.x, l.y + LAMP_SIZE * 0.62, l.z) + f3 * 0.07, Vec3::new(0.22, 0.04, LAMP_SIZE + 0.06), yaw, black);
            // A white ring round the lens.
            board(g, Vec3::new(l.x, l.y, l.z) - f3 * 0.04, s3, Vec3::Y, LAMP_SIZE + 0.1, LAMP_SIZE + 0.1, f3, rgba(mul(pal.chalk, 0.7)));
        }
        // Crossbuck: two boards in an X, white with a red edge.
        let x = Vec3::new(r.post.x, 3.72, r.post.y) + f3 * 0.1;
        let d1 = (s3 + Vec3::Y).normalize();
        let d2 = (s3 - Vec3::Y).normalize();
        for (along, up) in [(d1, d2), (d2, d1)] {
            board(g, x, along, up, 1.3, 0.24, f3, red);
            board(g, x + f3 * 0.006, along, up, 1.2, 0.14, f3, white);
        }
        // The bell on top.
        g.part(Metal).cylinder(r.post, 0.17, 0.12, 4.25, 4.5, 10, rgba([0.08, 0.08, 0.09]));
        g.part(Metal).cylinder(r.post, 0.05, 0.02, 4.5, 4.6, 6, rgba([0.08, 0.08, 0.09]));
        // Barrier housings at the pivots.
        for a in &r.arms {
            let ayaw = a.along.y.atan2(a.along.x);
            let (w, h) = if a.small { (0.32, a.pivot.y - KERB_H + 0.15) } else { (0.46, a.pivot.y - KERB_H + 0.2) };
            g.part(Matte).boxed(Vec3::new(a.pivot.x, KERB_H + h * 0.5, a.pivot.z), Vec3::new(w, h, w * 0.9), ayaw, white);
            g.part(Matte).boxed(Vec3::new(a.pivot.x, KERB_H + h + 0.02, a.pivot.z), Vec3::new(w + 0.04, 0.04, w * 0.9 + 0.04), ayaw, rgba(STRIPE_RED));
        }
    }
}

/// A barrier arm in its own frame: the pivot at the origin, the arm along
/// +x (lying down), red and white stripes, a counterweight behind the
/// pivot. Its lamps are the app's (`ArmRig::lamps`).
pub fn arm_geometry(pal: &Palette, len: f32, small: bool) -> Geometry {
    let mut g = Geometry::default();
    let white = rgba(mul(pal.chalk, 1.0));
    let red = rgba(STRIPE_RED);
    let t: f32 = if small { 0.07 } else { 0.11 };
    let stripe: f32 = if small { 0.35 } else { 0.55 };
    let mut x = 0.15;
    let mut k = 0;
    while x < len - 1e-3 {
        let l = stripe.min(len - x);
        g.part(Matte).boxed(Vec3::new(x + l * 0.5, 0.0, 0.0), Vec3::new(l, t, t), 0.0, if k % 2 == 0 { red } else { white });
        x += l;
        k += 1;
    }
    let dark = rgba([0.12, 0.12, 0.13]);
    g.part(Metal).boxed(Vec3::ZERO, Vec3::new(0.24, 0.24, t + 0.12), 0.0, dark);
    let back = if small { 0.35 } else { 0.6 };
    g.part(Metal).boxed(Vec3::new(-back * 0.5, 0.0, 0.0), Vec3::new(back, t * 0.8, t * 0.8), 0.0, dark);
    g.part(Metal).boxed(Vec3::new(-back, 0.0, 0.0), Vec3::new(if small { 0.2 } else { 0.32 }, if small { 0.22 } else { 0.34 }, t + 0.1), 0.0, dark);
    g
}

#[cfg(test)]
mod tests {
    use super::*;
    use nd_sim::level_crossing::CrossingArm;

    fn crossing(guard: CrossingGuard, fwd: u8, bwd: u8) -> LevelCrossing {
        let hw = (fwd + bwd) as f32 * 3.5 * 0.5 + 1.5;
        let arm = |away: Vec2, lanes_in, lanes_out| CrossingArm { segment: 0, away, lanes_in, lanes_out, half_width: hw, stop: 8.0, path: false };
        LevelCrossing { pos: Vec2::ZERO, rail_dir: Vec2::X, guard, arms: vec![arm(Vec2::NEG_Y, fwd, bwd), arm(Vec2::Y, bwd, fwd)], ..Default::default() }
    }

    #[test]
    fn half_barriers_stand_on_the_verge_and_reach_the_centre_line() {
        let c = crossing(CrossingGuard::Barriers, 2, 2);
        let r = rigs(&c);
        assert_eq!(r.len(), 2);
        for rig in &r {
            let a = rig.arms[0];
            // Off the carriageway (half-width 8.5 m), on the traffic's right.
            assert!(a.pivot.x.abs() > 8.5, "pivot on the verge: {:?}", a.pivot);
            let tip = Vec2::new(a.pivot.x, a.pivot.z) + a.along * a.len;
            assert!(tip.x.abs() < 0.6, "tip at the centre line: {tip:?}");
            // Facing the arriving traffic, before the track.
            assert!(rig.post.y.abs() > 4.0 && rig.facing.dot(rig.post) > 0.0);
        }
        // The two approaches' arms are on opposite sides of the street.
        assert!(r[0].arms[0].pivot.x * r[1].arms[0].pivot.x < 0.0);
    }

    #[test]
    fn a_lights_only_crossing_has_no_arms() {
        let r = rigs(&crossing(CrossingGuard::Lights, 1, 1));
        assert_eq!(r.len(), 2);
        assert!(r.iter().all(|x| x.arms.is_empty()));
        let mut g = Geometry::default();
        furniture(&mut g, &Palette::default(), &r);
        assert!(g.iter().count() > 0);
    }
}
