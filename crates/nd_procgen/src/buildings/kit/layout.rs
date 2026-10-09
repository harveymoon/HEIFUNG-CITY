//! Layout in axis-aligned rectangles of the lot frame ([`R`]): `u` along
//! the street, `v` in from it, so the street side is `v0`. Shared by the
//! trades, the rural trades, the docks and the shore.

use super::super::parts::*;
use super::super::{Ctx, Face};
use crate::surface::{Geometry, Surface};
use glam::Vec2;
use rand::Rng;
use crate::depth::PANE;

// ── shared layout kit ────────────────────────────────────────────────────────

/// Axis-aligned rectangle in lot coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct R {
    pub u0: f32,
    pub u1: f32,
    pub v0: f32,
    pub v1: f32,
}

impl R {
    pub fn new(u0: f32, u1: f32, v0: f32, v1: f32) -> Self {
        Self { u0: u0.min(u1), u1: u0.max(u1), v0: v0.min(v1), v1: v0.max(v1) }
    }
    pub fn of(poly: &[Vec2]) -> Self {
        let (lo, hi) = bounds(poly);
        Self::new(lo.x, hi.x, lo.y, hi.y)
    }
    pub fn c(&self) -> Vec2 {
        Vec2::new((self.u0 + self.u1) * 0.5, (self.v0 + self.v1) * 0.5)
    }
    pub fn w(&self) -> f32 {
        self.u1 - self.u0
    }
    pub fn d(&self) -> f32 {
        self.v1 - self.v0
    }
    pub fn poly(&self) -> Vec<Vec2> {
        let c = self.c();
        rect(c.x, c.y, self.w() * 0.5, self.d() * 0.5)
    }
    /// The street side (faces -v).
    pub fn front(&self) -> Face {
        Face::new(Vec2::new(self.u0, self.v0), Vec2::new(self.u1, self.v0), Vec2::new(0.0, -1.0))
    }
    pub fn back(&self) -> Face {
        Face::new(Vec2::new(self.u0, self.v1), Vec2::new(self.u1, self.v1), Vec2::new(0.0, 1.0))
    }
    pub fn left(&self) -> Face {
        Face::new(Vec2::new(self.u0, self.v0), Vec2::new(self.u0, self.v1), Vec2::new(-1.0, 0.0))
    }
    pub fn right(&self) -> Face {
        Face::new(Vec2::new(self.u1, self.v0), Vec2::new(self.u1, self.v1), Vec2::new(1.0, 0.0))
    }
    pub fn faces(&self) -> [Face; 4] {
        [self.front(), self.right(), self.back(), self.left()]
    }
    pub fn inset(&self, k: f32) -> Self {
        let c = self.c();
        let (hw, hd) = ((self.w() * 0.5 - k).max(0.05), (self.d() * 0.5 - k).max(0.05));
        Self::new(c.x - hw, c.x + hw, c.y - hd, c.y + hd)
    }
}

/// The largest version of a wanted rectangle that stands on the parcel:
/// shrunk about its centre and nudged inward (see [`fit_inside`]); `None`
/// when less than `min` across is left.
pub(crate) fn fit(c: &Ctx, r: R, min: f32) -> Option<R> {
    if r.w() < min || r.d() < min {
        return None;
    }
    if poly_inside(c, &r.poly()) {
        return Some(r);
    }
    // Lots are cut by slanted streets: first trim the sides to the span the
    // parcel has across the rectangle's whole depth, then the ends to its
    // span along the width, keeping as much as possible.
    let mut t = r;
    if let Some((lo, hi)) = common_span(&c.lot_poly, t.v0, t.v1, false) {
        t = R::new(t.u0.max(lo + 0.05), t.u1.min(hi - 0.05), t.v0, t.v1);
    }
    if t.u1 > t.u0 {
        if let Some((lo, hi)) = common_span(&c.lot_poly, t.u0, t.u1, true) {
            t = R::new(t.u0, t.u1, t.v0.max(lo + 0.05), t.v1.min(hi - 0.05));
        }
    }
    if t.u1 > t.u0 && t.v1 > t.v0 && t.w() >= min && t.d() >= min && poly_inside(c, &t.poly()) {
        return Some(t);
    }
    let p = fit_inside(c, &r.poly());
    if !poly_inside(c, &p) {
        return None;
    }
    let out = R::of(&p);
    (out.w() >= min && out.d() >= min).then_some(out)
}

/// The interval of `u` a polygon covers on every line `v = x` for `x`
/// between `a` and `b` (with `vertical`, the interval of `v` on every line
/// `u = x`): the intersection of the spans of lines sampled a metre apart.
/// Treats the polygon as convex along each line (outermost crossings).
pub(crate) fn common_span(poly: &[Vec2], a: f32, b: f32, vertical: bool) -> Option<(f32, f32)> {
    let n = poly.len();
    let steps = ((b - a) / 1.0).ceil().max(1.0) as i32;
    let (mut lo, mut hi) = (f32::MIN, f32::MAX);
    for k in 0..=steps {
        let x = a + (b - a) * k as f32 / steps as f32;
        let (mut s0, mut s1) = (f32::MAX, f32::MIN);
        for i in 0..n {
            let (p, q) = (poly[i], poly[(i + 1) % n]);
            // Line `y = x` (or `u = x` when vertical) against the edge.
            let (pa, qa, pb, qb) = if vertical { (p.x, q.x, p.y, q.y) } else { (p.y, q.y, p.x, q.x) };
            if (pa - x) * (qa - x) > 0.0 || (pa - qa).abs() < 1e-6 {
                continue;
            }
            let t = (x - pa) / (qa - pa);
            let c = pb + (qb - pb) * t;
            s0 = s0.min(c);
            s1 = s1.max(c);
        }
        if s0 > s1 {
            return None;
        }
        lo = lo.max(s0);
        hi = hi.min(s1);
    }
    (hi > lo).then_some((lo, hi))
}

/// A solid block on a rectangle.
pub(crate) fn block(g: &mut Geometry, c: &Ctx, s: Surface, r: &R, y0: f32, y1: f32, color: [f32; 3]) {
    extrude(g, &c.frame, s, &r.poly(), y0, y1, color);
}

/// A window on a face: dark glass, lit (for the renderer to switch) by
/// chance, with a frame.
#[allow(clippy::too_many_arguments)]
pub(crate) fn pane(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y: f32, w: f32, h: f32, warm: bool) {
    face.window_frame(g, c, t, y, w, h, 0.2, c.style.trim_color);
    if coin(rng, c.lit_chance()) {
        let col = if warm { mul(c.palette.window_warm, rng.random_range(0.6..1.0)) } else { mul(pick(rng, &c.style.window_colors), rng.random_range(0.55..0.95)) };
        face.glaze(g, c, t, y, PANE, w, h, Pane::Lit(col));
    } else {
        face.glaze(g, c, t, y, PANE, w, h, Pane::Dark);
    }
}

/// A row of panes across a face, `n` per floor for `floors` floors.
#[allow(clippy::too_many_arguments)]
pub(crate) fn pane_row(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, floors: i32, fh: f32, spacing: f32, w: f32, h: f32, warm: bool) {
    let n = ((face.len - 1.0) / spacing).floor() as i32;
    if n < 1 {
        return;
    }
    let step = (face.len - 1.0) / n as f32;
    for k in 0..floors {
        for i in 0..n {
            let t = -(face.len - 1.0) * 0.5 + (i as f32 + 0.5) * step;
            pane(g, c, rng, face, t, y0 + k as f32 * fh + fh * 0.55, w.min(step * 0.8), h, warm);
        }
    }
}

