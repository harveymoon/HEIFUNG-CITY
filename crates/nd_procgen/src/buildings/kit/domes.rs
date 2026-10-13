//! [capsule] Round parts for the Capsule district look: smooth surfaces of
//! revolution (domes, spheres, pills, saucers, tubes, vaults), porthole
//! bands, accent stripes, round doors, radar balls, dishes, landing pads,
//! and the garden kit (lawns, round planters, ball trees, globe lamps).
//!
//! [`revolve`] turns a profile round an axis with smooth normals (from the
//! profile's slope, so a dome shades round, not faceted) and creases where
//! the profile turns sharply (a saucer's rim, a drum's flat top). Poles are
//! one vertex with a fan, so there are no degenerate triangles at a crown.
//! Every triangle is wound to face along its vertex normals.

use super::super::parts::*;
use super::super::{Ctx, Frame};
use crate::depth::{FRAME, PANE};
use crate::mesh::MeshData;
use crate::surface::{Geometry, Surface, Surface::*};
use glam::{Vec2, Vec3};
use rand::Rng;
use std::f32::consts::{FRAC_PI_2, TAU};

/// The axis a profile is turned round: a world origin, the unit axis, and
/// two unit directions square to it (angle 0 and a quarter turn).
#[derive(Clone, Copy, Debug)]
pub struct Axis {
    pub origin: Vec3,
    pub dir: Vec3,
    pub b1: Vec3,
    pub b2: Vec3,
}

impl Axis {
    /// Upright through the lot-local point `p`, from height `y`; angle 0 is
    /// the lot's +u, a quarter turn its +v (so angles are lot-local).
    pub fn vertical(f: &Frame, p: Vec2, y: f32) -> Self {
        Axis { origin: f.at2(p, y), dir: Vec3::Y, b1: f.dir3(Vec2::X), b2: f.dir3(Vec2::Y) }
    }

    /// Level, from the lot-local point `a` towards `b` at height `y`; angle 0
    /// is to the side, a quarter turn straight up.
    pub fn level(f: &Frame, a: Vec2, b: Vec2, y: f32) -> Self {
        let dir = f.dir3((b - a).normalize_or(Vec2::X));
        let b1 = dir.cross(Vec3::Y).normalize_or(Vec3::X);
        Axis { origin: f.at2(a, y), dir, b1, b2: Vec3::Y }
    }

    /// Through the world point `origin` along the unit `dir`.
    pub fn along(origin: Vec3, dir: Vec3) -> Self {
        let dir = dir.normalize_or(Vec3::Y);
        let side = if dir.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
        let b1 = dir.cross(side).normalize();
        let b2 = dir.cross(b1).normalize();
        Axis { origin, dir, b1, b2 }
    }

    fn radial(&self, a: f32) -> Vec3 {
        self.b1 * a.cos() + self.b2 * a.sin()
    }
}

/// Profile turns sharper than this (radians) are creased: two rings of
/// vertices with their own normals instead of one averaged.
const CREASE: f32 = 0.6;
/// A profile point this close to the axis is a pole: one vertex.
const POLE: f32 = 1e-3;

/// Unit outward normal of a profile segment (r, y): to the right of travel.
fn seg_normal(d: Vec2) -> Vec2 {
    Vec2::new(d.y, -d.x).normalize_or(Vec2::X)
}

/// Turn `profile` ((radius, height along the axis), outside on the right of
/// travel: bottom to top for a wall or dome) round `ax`, `sides` steps over
/// `arc` (radians from angle 0; `None` the whole turn). Ends that come to
/// the axis are poles; the others are left open.
pub fn revolve(m: &mut MeshData, ax: &Axis, profile: &[(f32, f32)], sides: usize, arc: Option<(f32, f32)>, color: [f32; 4]) {
    // Drop repeated points: a zero-length segment would draw slivers.
    let mut pts: Vec<Vec2> = Vec::with_capacity(profile.len());
    for &(r, y) in profile {
        let p = Vec2::new(r.max(0.0), y);
        if !p.is_finite() {
            return;
        }
        if pts.last().is_none_or(|q| q.distance(p) > 1e-3) {
            pts.push(p);
        }
    }
    let n = pts.len();
    if n < 2 || sides < 3 {
        return;
    }
    let (a0, a1) = arc.unwrap_or((0.0, TAU));
    let cols = sides + 1;
    let rmax = pts.iter().map(|p| p.x).fold(0.0f32, f32::max).max(0.1);
    // One or two rings per point: (normal into the segment before, normal
    // into the segment after).
    struct Ring {
        pole: bool,
        start: [u32; 2],
    }
    let mut rings: Vec<Ring> = Vec::with_capacity(n);
    let mut v = 0.0f32;
    for k in 0..n {
        if k > 0 {
            v += pts[k].distance(pts[k - 1]);
        }
        let din = (k > 0).then(|| (pts[k] - pts[k - 1]).normalize_or_zero());
        let dout = (k + 1 < n).then(|| (pts[k + 1] - pts[k]).normalize_or_zero());
        let (nin, nout) = match (din, dout) {
            (Some(a), Some(b)) if a.angle_to(b).abs() > CREASE => (seg_normal(a), seg_normal(b)),
            (Some(a), Some(b)) => {
                let s = seg_normal(a + b);
                (s, s)
            }
            (Some(a), None) => (seg_normal(a), seg_normal(a)),
            (None, Some(b)) => (seg_normal(b), seg_normal(b)),
            (None, None) => return,
        };
        let pole = pts[k].x < POLE && (k == 0 || k + 1 == n);
        let mut start = [0u32; 2];
        for (slot, nrm) in [nin, nout].into_iter().enumerate() {
            if slot == 1 && nin == nout {
                start[1] = start[0];
                break;
            }
            start[slot] = m.positions.len() as u32;
            if pole {
                let p = ax.origin + ax.dir * pts[k].y;
                let nn = (ax.dir * nrm.y.signum()).normalize_or(Vec3::Y);
                m.positions.push(p.to_array());
                m.normals.push(nn.to_array());
                m.colors.push(color);
                m.uvs.push([0.0, v]);
                m.tangents.push([ax.b1.x, ax.b1.y, ax.b1.z, 1.0]);
                continue;
            }
            for j in 0..cols {
                let a = a0 + (a1 - a0) * j as f32 / sides as f32;
                let rad = ax.radial(a);
                let p = ax.origin + ax.dir * pts[k].y + rad * pts[k].x;
                let nn = (rad * nrm.x + ax.dir * nrm.y).normalize_or(rad);
                let t = (ax.b2 * a.cos() - ax.b1 * a.sin()).normalize_or(ax.b1);
                m.positions.push(p.to_array());
                m.normals.push(nn.to_array());
                m.colors.push(color);
                m.uvs.push([a * rmax, v]);
                m.tangents.push([t.x, t.y, t.z, 1.0]);
            }
        }
        rings.push(Ring { pole, start });
    }
    // A triangle wound to face along its vertex normals; slivers dropped.
    let tri = |m: &mut MeshData, a: u32, b: u32, c: u32| {
        let [pa, pb, pc] = [a, b, c].map(|i| Vec3::from(m.positions[i as usize]));
        let cross = (pb - pa).cross(pc - pa);
        if cross.length() < 1e-6 {
            return;
        }
        let nsum: Vec3 = [a, b, c].iter().map(|&i| Vec3::from(m.normals[i as usize])).sum();
        if cross.dot(nsum) < 0.0 {
            m.indices.extend_from_slice(&[a, c, b]);
        } else {
            m.indices.extend_from_slice(&[a, b, c]);
        }
    };
    for k in 0..n - 1 {
        let (lo, hi) = (&rings[k], &rings[k + 1]);
        let (sa, sb) = (lo.start[1], hi.start[0]);
        match (lo.pole, hi.pole) {
            (true, true) => {}
            (true, false) => {
                for j in 0..sides as u32 {
                    tri(m, sa, sb + j, sb + j + 1);
                }
            }
            (false, true) => {
                for j in 0..sides as u32 {
                    tri(m, sa + j, sa + j + 1, sb);
                }
            }
            (false, false) => {
                for j in 0..sides as u32 {
                    tri(m, sa + j, sa + j + 1, sb + j + 1);
                    tri(m, sa + j, sb + j + 1, sb + j);
                }
            }
        }
    }
}

/// Sides for a round part of radius `r`: enough to read round from close
/// up, no more than a far-off dome needs.
pub fn sides_for(r: f32) -> usize {
    if r < 0.8 {
        8
    } else if r < 2.5 {
        12
    } else if r < 6.0 {
        20
    } else if r < 11.0 {
        28
    } else {
        36
    }
}

/// [`revolve`] round an upright axis through the lot-local `p`, in `s`.
pub fn turn(g: &mut Geometry, f: &Frame, s: Surface, p: Vec2, profile: &[(f32, f32)], sides: usize, color: [f32; 3]) {
    revolve(g.part(s), &Axis::vertical(f, p, 0.0), profile, sides, None, rgba(color));
}

/// A quarter ellipse from the springing (`r` out at `y0`) to the crown
/// `h` above, as a profile: a dome over a drum of radius `r`.
pub fn dome_profile(r: f32, y0: f32, h: f32, segs: usize) -> Vec<(f32, f32)> {
    (0..=segs)
        .map(|k| {
            let a = k as f32 / segs as f32 * FRAC_PI_2;
            let rr = if k == segs { 0.0 } else { a.cos() * r };
            (rr, y0 + a.sin() * h)
        })
        .collect()
}

/// A whole ellipsoid (`r` round, `ry` half-height, centred at `cy`) from
/// pole to pole.
pub fn ball_profile(r: f32, ry: f32, cy: f32, segs: usize) -> Vec<(f32, f32)> {
    (0..=segs)
        .map(|k| {
            let a = -FRAC_PI_2 + k as f32 / segs as f32 * std::f32::consts::PI;
            let rr = if k == 0 || k == segs { 0.0 } else { a.cos() * r };
            (rr, cy + a.sin() * ry)
        })
        .collect()
}

/// A point on an ellipse (`r` round, `ry` half-height, centre `cy`) at
/// latitude `phi`, pushed `off` out along its normal: for bands that hug a
/// dome or a ball.
pub fn ellipse_at(r: f32, ry: f32, cy: f32, phi: f32, off: f32) -> (f32, f32) {
    let n = Vec2::new(ry * phi.cos(), r * phi.sin()).normalize_or(Vec2::X);
    (phi.cos() * r + n.x * off, cy + phi.sin() * ry + n.y * off)
}

/// A band hugging an ellipse between latitudes `phi0` and `phi1`, `off`
/// proud of it: an accent stripe round a dome or a ball.
#[allow(clippy::too_many_arguments)]
pub fn ellipse_band(g: &mut Geometry, f: &Frame, s: Surface, p: Vec2, r: f32, ry: f32, cy: f32, phi0: f32, phi1: f32, off: f32, color: [f32; 3]) {
    let prof: Vec<(f32, f32)> = (0..=3).map(|k| ellipse_at(r, ry, cy, phi0 + (phi1 - phi0) * k as f32 / 3.0, off)).collect();
    turn(g, f, s, p, &prof, sides_for(r), color);
}

/// An accent stripe round an upright drum of radius `r`, from `y` up `h`,
/// standing 5 cm proud.
#[allow(clippy::too_many_arguments)]
pub fn drum_stripe(g: &mut Geometry, f: &Frame, s: Surface, p: Vec2, r: f32, y: f32, h: f32, color: [f32; 3]) {
    turn(g, f, s, p, &[(r + 0.05, y), (r + 0.05, y + h)], sides_for(r), color);
}

/// A soft line of light round a drum, lit only after dark: the accent
/// bands' glow.
pub fn night_ring(g: &mut Geometry, f: &Frame, p: Vec2, r: f32, y: f32, h: f32, color: [f32; 3]) {
    let from = g.get(Glow).colors.len();
    turn(g, f, Glow, p, &[(r + 0.08, y), (r + 0.08, y + h)], sides_for(r), color);
    mark_glow(g, from, NIGHT_MARK);
}

/// A round window (or a slot with round ends) on a curved wall: `at` a
/// world point on the true surface, `n` its outward unit normal, `win` the
/// opening's half-width and half-height (equal: a porthole), and
/// `bend` the surface's radius of curvature there. The frame and the pane
/// stand clear of the curve by the sag of the frame's rim, so neither dips
/// into the wall; the pane is a switchable light ([`window_light`]) or dark
/// glass.
#[allow(clippy::too_many_arguments)]
pub fn porthole(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, at: Vec3, n: Vec3, win: Vec2, bend: f32, rim: [f32; 3], lit: f32) {
    let n = n.normalize_or_zero();
    let t1 = Vec3::Y.cross(n);
    if t1.length() < 0.3 {
        return;
    }
    let t1 = t1.normalize();
    let t2 = n.cross(t1).normalize();
    let r = win.y.max(0.05);
    let half = (win.x - r).max(0.0);
    let pad = r * 0.3;
    let rf = half + r + pad;
    let sag = rf * rf / (2.0 * bend.max(rf * 1.2));
    // A stadium: two half circles `half` either side of the middle (one
    // circle when `half` is 0).
    let ring = |rad: f32, off: f32| -> Vec<Vec3> {
        let o = at + n * off;
        if half < 0.01 {
            return (0..10)
                .map(|i| {
                    let a = i as f32 / 10.0 * TAU;
                    o + (t1 * a.cos() + t2 * a.sin()) * rad
                })
                .collect();
        }
        let mut v = Vec::with_capacity(10);
        for (side, a0) in [(1.0f32, -FRAC_PI_2), (-1.0, FRAC_PI_2)] {
            for i in 0..5 {
                let a = a0 + i as f32 / 4.0 * std::f32::consts::PI;
                v.push(o + t1 * (side * half + a.cos() * rad) + t2 * (a.sin() * rad));
            }
        }
        v
    };
    g.part(Matte).cap(&ring(r + pad, sag + FRAME), n, rgba(rim));
    let pane = ring(r, sag + PANE);
    if coin(rng, lit) {
        let col = mul(pick(rng, &c.style.window_colors), rng.random_range(0.6..1.0));
        window_light(g, c, |g| g.part(Glow).cap(&pane, n, rgba(col)));
    } else {
        g.part(Glass).cap(&pane, n, rgba(c.palette.glass_dark));
    }
}

/// Whether the angle `a` lies within any `(angle, half-width)` of `skip`.
fn skipped(a: f32, skip: &[(f32, f32)]) -> bool {
    skip.iter().any(|&(s, gap)| {
        let d = (a - s).rem_euclid(TAU);
        d.min(TAU - d) < gap
    })
}

/// Porthole spacing on a ring of radius `r`: about 2.3 m, opened out on a
/// big drum so a row has at most fourteen (the triangle budget: a window is
/// about twenty triangles, a floor gets three hundred).
pub fn porthole_spacing(r: f32) -> f32 {
    (TAU * r / 14.0).max(2.3)
}

/// A row of portholes round an upright drum (lot-local centre `p`, radius
/// `r`) at height `y`, about `spacing` apart, leaving out those within
/// `gap` radians of any `(angle, gap)` in `skip` (a door, a link tube).
#[allow(clippy::too_many_arguments)]
pub fn porthole_row(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, r: f32, y: f32, win: Vec2, spacing: f32, skip: &[(f32, f32)], rim: [f32; 3]) {
    // Slots sit close, in a band; portholes keep their spacing.
    let spacing = if win.x > win.y * 1.2 { spacing.max(win.x * 2.0 + 0.45) } else { spacing.max(win.x * 2.6 + 0.6) };
    let count = ((TAU * r / spacing).floor() as usize).clamp(3, 40);
    let lit = c.lit_chance();
    let phase = (y * 0.37).fract() * TAU / count as f32 * 0.5;
    for i in 0..count {
        let a = phase + i as f32 / count as f32 * TAU;
        if skipped(a, skip) {
            continue;
        }
        let dir = Vec2::from_angle(a);
        let at = c.frame.at2(p + dir * r, y);
        porthole(g, c, rng, at, c.frame.dir3(dir), win, r, rim, lit);
    }
}

/// A row of portholes round an ellipsoid (a ball or a dome: lot-local
/// centre `p`, `r` round, `ry` half-height, centre height `cy`) at
/// latitude `phi`.
#[allow(clippy::too_many_arguments)]
pub fn porthole_row_ball(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, r: f32, ry: f32, cy: f32, phi: f32, win: Vec2, spacing: f32, skip: &[(f32, f32)], rim: [f32; 3]) {
    if phi.abs() > 1.05 {
        return;
    }
    // Slots sit close, in a band; portholes keep their spacing.
    let spacing = if win.x > win.y * 1.2 { spacing.max(win.x * 2.0 + 0.45) } else { spacing.max(win.x * 2.6 + 0.6) };
    let ring_r = r * phi.cos();
    let count = ((TAU * ring_r / spacing).floor() as usize).clamp(3, 40);
    let lit = c.lit_chance();
    let n2 = Vec2::new(ry * phi.cos(), r * phi.sin()).normalize_or(Vec2::X);
    // The tighter of the two curvatures there.
    let bend = (ry * ry / r).min(r * r / ry).min(ring_r / n2.x.max(0.2)).max(0.5);
    let y = cy + ry * phi.sin();
    for i in 0..count {
        let a = (phi * 1.3).fract() + i as f32 / count as f32 * TAU;
        if skipped(a, skip) {
            continue;
        }
        let dir = Vec2::from_angle(a);
        let at = c.frame.at2(p + dir * ring_r, y);
        let n = c.frame.dir3(dir) * n2.x + Vec3::Y * n2.y;
        porthole(g, c, rng, at, n, win, bend, rim, lit);
    }
}

/// A level tube between two lot-local points at height `y` (axis), open at
/// both ends (they are sunk into what they join): a glass link between two
/// domes, with a white collar near each end.
#[allow(clippy::too_many_arguments)]
pub fn link_tube(g: &mut Geometry, c: &Ctx, a: Vec2, b: Vec2, y: f32, r: f32, glass: [f32; 3], collar: [f32; 3]) {
    let len = a.distance(b);
    if len < 0.5 {
        return;
    }
    let ax = Axis::level(&c.frame, a, b, y);
    let sides = sides_for(r).min(14);
    revolve(g.part(Glass), &ax, &[(r, 0.0), (r, len)], sides, None, rgba(glass));
    for t in [0.18f32, 0.82] {
        let m = len * t;
        revolve(g.part(Matte), &ax, &[(r + 0.06, m - 0.25), (r + 0.06, m + 0.25)], sides, None, rgba(collar));
    }
}

/// A glass barrel vault over a walk from `a` to `b` (lot-local), springing
/// at `y`: the curved canopy over a dome hall's entrance, with white ribs.
#[allow(clippy::too_many_arguments)]
pub fn glass_vault(g: &mut Geometry, c: &Ctx, a: Vec2, b: Vec2, y: f32, r: f32, glass: [f32; 3], rib: [f32; 3]) {
    let len = a.distance(b);
    if len < 1.0 {
        return;
    }
    let ax = Axis::level(&c.frame, a, b, y);
    revolve(g.part(Glass), &ax, &[(r, 0.0), (r, len)], 12, Some((0.0, std::f32::consts::PI)), rgba(glass));
    let ribs = ((len / 2.5).floor() as i32).max(1);
    for k in 0..=ribs {
        let m = (len * k as f32 / ribs as f32).clamp(0.2, len - 0.2);
        revolve(g.part(Matte), &ax, &[(r + 0.08, m - 0.12), (r + 0.08, m + 0.12)], 12, Some((0.0, std::f32::consts::PI)), rgba(rib));
    }
    // Posts down to the ground at the outer end.
    let d = (b - a).normalize_or(Vec2::X);
    let side = Vec2::new(-d.y, d.x) * (r + 0.02);
    for s in [side, -side] {
        let p = b - d * 0.2 + s;
        c.frame.bx(g, Matte, p.x, y * 0.5, p.y, Vec3::new(0.18, y, 0.18), rib);
    }
}

/// An entrance pod: a short level tube of radius `r` out of a drum (centre
/// `p`, radius `drum_r`) towards `dir`, its end a round door with a lit
/// glass hatch and a step. Returns where the door is (lot-local), or `None`
/// when there is no room for it on the lot.
#[allow(clippy::too_many_arguments)]
pub fn entry_pod(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, drum_r: f32, dir: Vec2, r: f32, wall: [f32; 3], accent: [f32; 3]) -> Option<Vec2> {
    let dir = dir.normalize_or(Vec2::NEG_Y);
    let len = 1.8 + r * 0.4;
    let end = p + dir * (drum_r + len);
    let side = Vec2::new(-dir.y, dir.x) * (r + 0.2);
    if ![end + dir * 0.9, end + side, end - side].iter().all(|q| c.inside_local(*q) && distance_to_boundary(*q, &c.lot_poly) > 0.3) {
        return None;
    }
    let start = p + dir * (drum_r - 0.6);
    let y = r + 0.05;
    let ax = Axis::level(&c.frame, start, end, y);
    let l = start.distance(end);
    let sides = sides_for(r);
    revolve(g.part(Matte), &ax, &[(r, 0.0), (r, l), (0.0, l)], sides, None, rgba(wall));
    revolve(g.part(Matte), &ax, &[(r + 0.05, l - 0.55), (r + 0.05, l - 0.3)], sides, None, rgba(accent));
    // The round door on the flat end.
    let n = c.frame.dir3(dir);
    let centre = c.frame.at2(end, y - r * 0.12);
    let t1 = Vec3::Y.cross(n).normalize_or(Vec3::X);
    let disc = |rad: f32, off: f32| -> Vec<Vec3> {
        (0..16)
            .map(|i| {
                let a = i as f32 / 16.0 * TAU;
                centre + n * off + (t1 * a.cos() + Vec3::Y * a.sin()) * rad
            })
            .collect()
    };
    let dr = r * 0.72;
    g.part(Matte).cap(&disc(dr + 0.14, FRAME), n, rgba(mul(accent, 0.85)));
    g.part(Glass).cap(&disc(dr, PANE), n, rgba(c.palette.glass_dark));
    if coin(rng, 0.55 + 0.4 * c.lit_chance()) {
        let col = mul(pick(rng, &c.style.window_colors), 0.75);
        window_light(g, c, |g| g.part(Glow).cap(&disc(dr * 0.8, PANE + 0.02), n, rgba(col)));
    }
    // A round step.
    let step = end + dir * 0.55;
    turn(g, &c.frame, Matte, step, &[(0.0, 0.02), (0.9, 0.02), (0.9, 0.16), (0.0, 0.16)], 12, mul(wall, 0.85));
    Some(end)
}

/// A radar ball on three legs and a ring, standing on `y` at lot-local `p`.
pub fn radar_ball(g: &mut Geometry, c: &Ctx, p: Vec2, y: f32, r: f32, ball: [f32; 3], legs: [f32; 3]) {
    let f = &c.frame;
    let leg_h = r * 0.9;
    let foot = r * 0.75;
    for k in 0..3 {
        let a = k as f32 / 3.0 * TAU + 0.4;
        let base = f.at2(p + Vec2::from_angle(a) * foot, y);
        let top = f.at2(p + Vec2::from_angle(a) * r * 0.35, y + leg_h + r * 0.4);
        g.part(Matte).tube(base, top, 0.08 + r * 0.02, 5, rgba(legs));
    }
    turn(g, f, Matte, p, &[(foot + 0.06, y + leg_h * 0.45), (foot + 0.06, y + leg_h * 0.45 + 0.12)], 12, legs);
    turn(g, f, Matte, p, &ball_profile(r, r, y + leg_h + r, 8), sides_for(r).min(20), ball);
    aviation_light_local(g, f, c.palette, p.x, y + leg_h + 2.0 * r + 0.15, p.y, 0.25);
}

/// A satellite dish on a post at lot-local `p`, tipped `tilt` from upright
/// towards `facing` (radians, lot-local).
#[allow(clippy::too_many_arguments)]
pub fn dish_on_post(g: &mut Geometry, c: &Ctx, p: Vec2, y: f32, r: f32, tilt: f32, facing: f32, color: [f32; 3]) {
    let f = &c.frame;
    let post_h = r * 0.9 + 0.4;
    turn(g, f, Matte, p, &[(0.12, y), (0.12, y + post_h)], 6, c.palette.steel);
    let d2 = Vec2::from_angle(facing);
    let up = (Vec3::Y * tilt.cos() + f.dir3(d2) * tilt.sin()).normalize();
    let ax = Axis::along(f.at2(p, y + post_h), up);
    let depth = r * 0.32;
    // The back (convex, facing away) and the bowl (facing into the dish).
    revolve(g.part(Matte), &ax, &[(0.18, -0.02), (r, depth - 0.02)], sides_for(r).min(16), None, rgba(mul(color, 0.85)));
    revolve(g.part(Matte), &ax, &[(r, depth), (0.18, 0.0)], sides_for(r).min(16), None, rgba(color));
    g.part(Matte).tube(ax.origin, ax.origin + up * (r * 0.9), 0.04, 4, rgba(c.palette.steel));
}

/// A round landing pad (lot-local centre `p`, radius `r`): a white disc
/// with an accent ring and a mark, and lights round its rim after dark.
pub fn landing_pad(g: &mut Geometry, c: &Ctx, p: Vec2, r: f32, deck: [f32; 3], accent: [f32; 3]) {
    let f = &c.frame;
    let s = sides_for(r).max(24);
    turn(g, f, Paving, p, &[(r, 0.0), (r, 0.28), (0.0, 0.28)], s, deck);
    turn(g, f, Matte, p, &[(r * 0.86, 0.31), (r * 0.72, 0.31)], s, accent);
    turn(g, f, Matte, p, &[(r * 0.3, 0.31), (0.0, 0.31)], 12, accent);
    let from = g.get(Glow).colors.len();
    let n = (s / 3).max(6);
    for k in 0..n {
        let a = k as f32 / n as f32 * TAU;
        let q = p + Vec2::from_angle(a) * (r * 0.94);
        f.bx(g, Glow, q.x, 0.33, q.y, Vec3::new(0.18, 0.08, 0.18), mul(accent, 1.4));
    }
    mark_glow(g, from, NIGHT_MARK);
}

/// A round planter: a white tub with a clipped ball of shrub in it.
pub fn planter(g: &mut Geometry, f: &Frame, p: Vec2, r: f32, tub: [f32; 3], leaf: [f32; 3]) {
    turn(g, f, Matte, p, &[(r * 0.85, 0.0), (r, 0.5), (r * 0.82, 0.62), (0.0, 0.62)], sides_for(r).min(12), tub);
    turn(g, f, Foliage, p, &dome_profile(r * 0.78, 0.55, r * 0.75, 3), 10, leaf);
}

/// A ball tree: a slim trunk and a round crown, the garden tree of the
/// look.
pub fn ball_tree(g: &mut Geometry, f: &Frame, p: Vec2, h: f32, r: f32, bark: [f32; 3], leaf: [f32; 3]) {
    turn(g, f, Wood, p, &[(0.2, 0.0), (0.13, h - r * 0.5)], 6, bark);
    turn(g, f, Foliage, p, &ball_profile(r, r * 0.92, h, 6), 10, leaf);
}

/// A globe lamp on a slim post, lit after dark.
pub fn globe_lamp(g: &mut Geometry, c: &Ctx, p: Vec2, h: f32) {
    let f = &c.frame;
    turn(g, f, Matte, p, &[(0.07, 0.0), (0.05, h)], 6, c.palette.steel);
    let from = g.get(Glow).colors.len();
    turn(g, f, Glow, p, &ball_profile(0.24, 0.24, h + 0.22, 4), 8, mul(c.palette.street_lamp, 0.9));
    mark_glow(g, from, NIGHT_MARK);
}

/// Round footprints packed into `poly` (lot-local): one per entry of
/// `max_r`, biggest room first, each at most its entry and at least
/// `min_r`, `gap` from the others; stops at the first that does not fit.
/// Candidates are a grid over the polygon scored by their room (distance to
/// the edge and to the circles already placed), so a triangle or an L gets
/// its circles in its widest parts.
pub fn pack_circles(poly: &[Vec2], max_r: &[f32], min_r: f32, gap: f32) -> Vec<(Vec2, f32)> {
    use nd_sim::zoning::point_in_polygon;
    let mut out: Vec<(Vec2, f32)> = Vec::new();
    if poly.len() < 3 {
        return out;
    }
    let (lo, hi) = bounds(poly);
    let n = 18;
    let mut cands: Vec<Vec2> = Vec::with_capacity(n * n + 1);
    cands.push(interior_anchor(poly));
    for i in 0..n {
        for j in 0..n {
            let q = Vec2::new(lo.x + (hi.x - lo.x) * (i as f32 + 0.5) / n as f32, lo.y + (hi.y - lo.y) * (j as f32 + 0.5) / n as f32);
            if point_in_polygon(q, poly) {
                cands.push(q);
            }
        }
    }
    let edge: Vec<f32> = cands.iter().map(|q| if point_in_polygon(*q, poly) { distance_to_boundary(*q, poly) } else { -1.0 }).collect();
    for &cap in max_r {
        let room = |i: usize| -> f32 {
            let q = cands[i];
            let mut d = edge[i];
            for (o, r) in &out {
                d = d.min(q.distance(*o) - r - gap);
            }
            d
        };
        // Biggest room; ties to the one nearest the front (smallest v) for a
        // stable pick. A circle limited by `cap` prefers the room nearest
        // the front of the candidates that hold it.
        let mut best: Option<(usize, f32)> = None;
        for i in 0..cands.len() {
            let r = room(i).min(cap);
            let better = match best {
                None => true,
                Some((bi, br)) => r > br + 0.05 || ((r - br).abs() <= 0.05 && cands[i].y < cands[bi].y),
            };
            if better {
                best = Some((i, r));
            }
        }
        match best {
            Some((i, r)) if r >= min_r => out.push((cands[i], r)),
            _ => break,
        }
    }
    out
}

/// Whether a circle (lot-local) lies on the lot with `margin` to spare.
pub fn circle_fits(c: &Ctx, p: Vec2, r: f32, margin: f32) -> bool {
    c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) >= r + margin
}
