//! The parts toolkit every style is assembled from: footprint helpers,
//! facades, roofs, site furniture and props. Everything is placed in parcel
//! coordinates through the [`Frame`], except the `*_world` variants used by
//! the street generator.

use super::{Ctx, Face, Frame};
use crate::mesh::MeshData;
use crate::palette::{RUST, RUST_STAIN};
use crate::surface::{Geometry, Surface};
use glam::{Vec2, Vec3};
use nd_sim::zoning::{point_in_polygon, polygon_area, polygon_centroid};
use rand::seq::IndexedRandom;
use rand::Rng;

use Surface::*;

// ── colours ──────────────────────────────────────────────────────────────────

pub fn rgba(c: [f32; 3]) -> [f32; 4] {
    [c[0], c[1], c[2], 1.0]
}

pub fn mul(c: [f32; 3], k: f32) -> [f32; 3] {
    [c[0] * k, c[1] * k, c[2] * k]
}

pub fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

pub fn pick(rng: &mut impl Rng, v: &[[f32; 3]]) -> [f32; 3] {
    *v.choose(rng).unwrap_or(&[0.5, 0.5, 0.5])
}

pub const DARK: [f32; 3] = [0.09, 0.09, 0.10];
pub const CHARCOAL: [f32; 3] = [0.16, 0.16, 0.17];

// ── polygons (lot-local) ─────────────────────────────────────────────────────

pub fn bounds(poly: &[Vec2]) -> (Vec2, Vec2) {
    poly.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)))
}

pub fn poly_center(poly: &[Vec2]) -> Vec2 {
    polygon_centroid(poly)
}

/// Largest distance from the centroid to a vertex.
pub fn poly_radius(poly: &[Vec2]) -> f32 {
    let c = poly_center(poly);
    poly.iter().map(|p| p.distance(c)).fold(0.0, f32::max)
}

/// Distance from the centroid to the nearest edge.
pub fn inradius(poly: &[Vec2]) -> f32 {
    let c = poly_center(poly);
    let n = poly.len();
    (0..n)
        .map(|i| {
            let a = poly[i];
            let b = poly[(i + 1) % n];
            let (_, q) = nd_sim::roads::project_on_segment(c, a, b);
            c.distance(q)
        })
        .fold(f32::MAX, f32::min)
}

/// Offset a counter-clockwise polygon inward (positive) or outward (negative).
/// Lots are concave and have sharp and near-straight corners, so the mitre at
/// each vertex is limited to 2.5 times the offset (a bevel beyond that), and
/// an inward offset drops any vertex that ends up outside the original polygon
/// or closer to its boundary than half the offset (a pinched spike). When too
/// little survives, or the offset is near the inradius, the polygon is scaled
/// about its centroid instead.
pub fn offset_poly(poly: &[Vec2], inset: f32) -> Vec<Vec2> {
    let n = poly.len();
    if n < 3 {
        return poly.to_vec();
    }
    if inset.abs() < 1e-4 {
        return poly.to_vec();
    }
    let scaled = || {
        let r = poly_radius(poly).max(1e-3);
        let k = ((r - inset) / r).clamp(0.1, 2.0);
        scale_poly(poly, k)
    };
    if inset > 0.0 && inset > inradius(poly) * 0.85 {
        return scaled();
    }
    let limit = inset.abs() * 2.5;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let p0 = poly[(i + n - 1) % n];
        let p1 = poly[i];
        let p2 = poly[(i + 1) % n];
        let d1 = (p1 - p0).normalize_or_zero();
        let d2 = (p2 - p1).normalize_or_zero();
        let n1 = Vec2::new(-d1.y, d1.x) * inset;
        let n2 = Vec2::new(-d2.y, d2.x) * inset;
        // Intersection of the two shifted lines, limited to the mitre length.
        let a = p0 + n1;
        let b = p1 + n2;
        let denom = d1.perp_dot(d2);
        let q = if denom.abs() < 1e-4 {
            p1 + (n1 + n2) * 0.5
        } else {
            let t = (b - a).perp_dot(d2) / denom;
            a + d1 * t
        };
        let d = q - p1;
        let len = d.length();
        let q = if len > limit && len > 1e-6 {
            let dir = (n1 + n2).normalize_or_zero();
            if dir == Vec2::ZERO { p1 + d * (limit / len) } else { p1 + dir * limit }
        } else {
            q
        };
        if !q.x.is_finite() || !q.y.is_finite() {
            return scaled();
        }
        out.push(q);
    }
    if inset > 0.0 {
        out.retain(|q| point_in_polygon(*q, poly) && distance_to_boundary(*q, poly) >= inset * 0.5);
        if out.len() < 3 {
            return scaled();
        }
    }
    let area = polygon_area(&out);
    let ok = if inset > 0.0 { area > 1.0 && area < polygon_area(poly) } else { area >= polygon_area(poly) && area < polygon_area(poly) * 4.0 + 50.0 * limit };
    if !ok {
        return scaled();
    }
    out
}

/// Pull a point back inside a polygon along the line to the polygon's
/// anchor point (a bisection, so it ends just inside the edge).
pub fn clamp_into(poly: &[Vec2], p: Vec2) -> Vec2 {
    if point_in_polygon(p, poly) {
        return p;
    }
    let a = interior_anchor(poly);
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..14 {
        let mid = (lo + hi) * 0.5;
        if point_in_polygon(a.lerp(p, mid), poly) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    a.lerp(p, (lo - 0.02).max(0.0))
}

/// Distance from a point to the nearest edge of a polygon.
pub fn distance_to_boundary(p: Vec2, poly: &[Vec2]) -> f32 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (_, q) = nd_sim::roads::project_on_segment(p, poly[i], poly[(i + 1) % n]);
            p.distance(q)
        })
        .fold(f32::MAX, f32::min)
}

pub fn scale_poly(poly: &[Vec2], k: f32) -> Vec<Vec2> {
    let c = poly_center(poly);
    poly.iter().map(|p| c + (*p - c) * k).collect()
}

pub fn shift_poly(poly: &[Vec2], d: Vec2) -> Vec<Vec2> {
    poly.iter().map(|p| *p + d).collect()
}

pub fn rotate_poly(poly: &[Vec2], angle: f32) -> Vec<Vec2> {
    let c = poly_center(poly);
    let (s, co) = angle.sin_cos();
    poly.iter()
        .map(|p| {
            let d = *p - c;
            c + Vec2::new(d.x * co - d.y * s, d.x * s + d.y * co)
        })
        .collect()
}

pub fn rect(cu: f32, cv: f32, hw: f32, hd: f32) -> Vec<Vec2> {
    vec![Vec2::new(cu - hw, cv - hd), Vec2::new(cu + hw, cv - hd), Vec2::new(cu + hw, cv + hd), Vec2::new(cu - hw, cv + hd)]
}

pub fn rect_yaw(cu: f32, cv: f32, hw: f32, hd: f32, yaw: f32) -> Vec<Vec2> {
    rotate_poly(&rect(cu, cv, hw, hd), yaw)
}

pub fn ngon(cu: f32, cv: f32, r: f32, sides: usize, rot: f32) -> Vec<Vec2> {
    (0..sides)
        .map(|i| {
            let a = rot + i as f32 / sides as f32 * std::f32::consts::TAU;
            Vec2::new(cu + a.cos() * r, cv + a.sin() * r)
        })
        .collect()
}

/// Keep a lot-local polygon inside the parcel. A polygon whose centre is off
/// the lot (odd lots are triangles and slivers, and generators lay things out
/// in the lot's bounding box) is first moved onto it, then the polygon is
/// scaled about its centre until every vertex is inside.
pub fn fit_inside(c: &Ctx, poly: &[Vec2]) -> Vec<Vec2> {
    let mut p = poly.to_vec();
    if p.len() < 3 {
        return p;
    }
    let centre = poly_center(&p);
    if !c.inside(centre) {
        let anchor = interior_anchor(&c.lot_poly);
        p = shift_poly(&p, anchor - centre);
    }
    for _ in 0..24 {
        if p.iter().all(|v| c.inside(*v)) {
            break;
        }
        p = scale_poly(&p, 0.9);
    }
    p
}

/// A point well inside a polygon: the centroid when it is inside, else the
/// deepest point among samples between the centroid and the edge midpoints.
pub fn interior_anchor(poly: &[Vec2]) -> Vec2 {
    let c = poly_center(poly);
    if point_in_polygon(c, poly) {
        return c;
    }
    let n = poly.len();
    let mut best = (f32::MIN, c);
    for i in 0..n {
        let m = (poly[i] + poly[(i + 1) % n]) * 0.5;
        for k in 1..8 {
            let t = k as f32 / 8.0;
            let p = m.lerp(c, t);
            if point_in_polygon(p, poly) {
                let d = distance_to_boundary(p, poly);
                if d > best.0 {
                    best = (d, p);
                }
            }
        }
    }
    best.1
}

// ── solids ───────────────────────────────────────────────────────────────────

pub fn extrude(g: &mut Geometry, f: &Frame, s: Surface, poly: &[Vec2], y0: f32, y1: f32, color: [f32; 3]) {
    if poly.len() < 3 {
        return;
    }
    g.part(s).frustum(&f.poly3(poly, y0), &f.poly3(poly, y1), rgba(color));
}

/// Solid between two polygons with the same vertex count.
pub fn loft(g: &mut Geometry, f: &Frame, s: Surface, bottom: &[Vec2], top: &[Vec2], y0: f32, y1: f32, color: [f32; 3]) {
    if bottom.len() < 3 || bottom.len() != top.len() {
        return;
    }
    g.part(s).frustum(&f.poly3(bottom, y0), &f.poly3(top, y1), rgba(color));
}

pub fn ground(g: &mut Geometry, f: &Frame, s: Surface, poly: &[Vec2], y: f32, color: [f32; 3]) {
    let world: Vec<Vec2> = poly.iter().map(|p| f.to_world(p.x, p.y)).collect();
    g.part(s).ground_polygon(&world, y, rgba(color));
}

/// Low wall along every edge of a footprint, on the inside of the edge.
pub fn parapet(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y: f32, h: f32, thick: f32, s: Surface, color: [f32; 3]) {
    for face in c.faces(poly) {
        face.bx(g, &c.frame, s, 0.0, y + h * 0.5, -thick * 0.5, Vec3::new(face.len + thick, h, thick), color);
    }
}

/// Band that stands proud of every edge (floor plates, string courses).
pub fn cornice(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y: f32, h: f32, proud: f32, s: Surface, color: [f32; 3]) {
    for face in c.faces(poly) {
        face.bx(g, &c.frame, s, 0.0, y, proud * 0.5, Vec3::new(face.len + proud * 2.0, h, proud + 0.02), color);
    }
}

/// Columns spaced along the edges of a footprint, slightly inside it.
#[allow(clippy::too_many_arguments)]
pub fn pilotis(g: &mut Geometry, c: &Ctx, poly: &[Vec2], y0: f32, y1: f32, spacing: f32, size: f32, s: Surface, color: [f32; 3]) {
    for face in c.faces(poly) {
        if face.len < size * 2.5 {
            continue;
        }
        let n = (face.len / spacing).floor().max(1.0) as i32;
        for k in 0..=n {
            let t = -face.len * 0.5 + k as f32 * (face.len / n as f32);
            let t = t.clamp(-face.len * 0.5 + size, face.len * 0.5 - size);
            face.bx(g, &c.frame, s, t, (y0 + y1) * 0.5, -size * 0.6, Vec3::new(size, y1 - y0, size), color);
        }
    }
}

/// Steps around a footprint.
pub fn plinth(g: &mut Geometry, c: &Ctx, poly: &[Vec2], steps: usize, rise: f32, tread: f32, s: Surface, color: [f32; 3]) {
    for k in 0..steps {
        let p = offset_poly(poly, -(tread * (steps - k) as f32));
        // Steps that would spill over the lot edge are dropped.
        let p = if p.iter().all(|q| c.inside(*q)) { p } else { fit_inside(c, &p) };
        extrude(g, &c.frame, s, &p, 0.0, rise * (k + 1) as f32, color);
    }
}

// ── facades ──────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
pub struct WindowSpec {
    pub w: f32,
    pub h: f32,
    pub spacing: f32,
    /// Height of the sill above the floor.
    pub sill: f32,
    pub lit: f32,
    /// Fraction of windows boarded with plywood.
    pub boarded: f32,
    /// Chance of an air conditioner under a window.
    pub ac: f32,
    pub frame: [f32; 3],
}

impl WindowSpec {
    pub fn from_ctx(c: &Ctx) -> Self {
        Self {
            w: c.rules.window_size[0],
            h: c.rules.window_size[1],
            spacing: c.rules.window_spacing,
            sill: (c.fh() - c.rules.window_size[1]) * 0.5,
            lit: c.lit_chance(),
            boarded: c.wear * 0.5,
            ac: c.style.clutter * 0.3,
            frame: c.style.trim_color,
        }
    }
}

/// Punched windows on one face for `floors` storeys starting at `y0`.
#[allow(clippy::too_many_arguments)]
pub fn windows(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, floors: i32, fh: f32, spec: &WindowSpec) {
    let usable = face.len - 1.4;
    let count = (usable / spec.spacing).floor() as i32;
    if count < 1 || floors < 1 {
        return;
    }
    let step = usable / count as f32;
    let w = spec.w.min(step * 0.8);
    let f = &c.frame;
    for k in 0..floors {
        let y = y0 + k as f32 * fh + spec.sill + spec.h * 0.5;
        for i in 0..count {
            let t = -usable * 0.5 + (i as f32 + 0.5) * step;
            face.bx(g, f, Matte, t, y, 0.03, Vec3::new(w + 0.24, spec.h + 0.24, 0.10), spec.frame);
            let r: f32 = rng.random();
            if r < spec.boarded {
                face.bx(g, f, Wood, t, y, 0.08, Vec3::new(w, spec.h, 0.06), c.palette.plywood);
            } else if rng.random_bool(spec.lit as f64) {
                let wc = pick(rng, &c.style.window_colors);
                face.bx(g, f, Glow, t, y, 0.08, Vec3::new(w, spec.h, 0.05), mul(wc, rng.random_range(0.55..1.0)));
            } else {
                face.bx(g, f, Glass, t, y, 0.08, Vec3::new(w, spec.h, 0.05), c.palette.glass_dark);
            }
            if rng.random_bool(spec.ac as f64) {
                ac_unit(g, c, face, t + w * 0.25, y - spec.h * 0.5 - 0.35, 0.32);
            }
        }
    }
}

/// Continuous recessed window band per floor with lit segments.
pub fn band_windows(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, floors: i32, fh: f32) {
    let f = &c.frame;
    let len = face.len * 0.92;
    let lit = c.lit_chance();
    let segs = (len / 3.0).floor().max(1.0) as i32;
    let seg = len / segs as f32;
    for k in 0..floors {
        let y = y0 + k as f32 * fh + fh * 0.55;
        face.bx(g, f, Glass, 0.0, y, 0.02, Vec3::new(len, fh * 0.46, 0.08), c.palette.glass_dark);
        face.bx(g, f, Matte, 0.0, y - fh * 0.26, 0.05, Vec3::new(len, 0.12, 0.12), c.style.trim_color);
        for i in 0..segs {
            if !rng.random_bool(lit as f64) {
                continue;
            }
            let t = -len * 0.5 + (i as f32 + 0.5) * seg;
            let wc = pick(rng, &c.style.window_colors);
            face.bx(g, f, Glow, t, y, 0.07, Vec3::new(seg * 0.9, fh * 0.38, 0.04), mul(wc, rng.random_range(0.5..0.95)));
        }
    }
}

/// Lit segments around a glass tower, per floor, standing just proud of the
/// facets so they read through the glazing.
pub fn curtain_lights(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2], y0: f32, floors: i32, fh: f32) {
    let lit = c.lit_chance();
    let f = &c.frame;
    for face in c.faces(poly) {
        let segs = (face.len / 3.2).floor().max(1.0) as i32;
        let seg = face.len / segs as f32;
        for k in 0..floors {
            let y = y0 + k as f32 * fh + fh * 0.5;
            for i in 0..segs {
                if !rng.random_bool(lit as f64) {
                    continue;
                }
                let t = -face.len * 0.5 + (i as f32 + 0.5) * seg;
                let wc = pick(rng, &c.style.window_colors);
                face.bx(g, f, Glow, t, y, 0.05, Vec3::new(seg * 0.85, fh * 0.42, 0.04), mul(wc, rng.random_range(0.35..0.8)));
            }
        }
    }
}

/// Glass shopfront with a lit interior, a fascia sign band and shutters where
/// the building is run down.
pub fn storefront(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, h: f32) {
    let f = &c.frame;
    let len = face.len * 0.9;
    let units = (len / 6.0).floor().max(1.0) as i32;
    let unit = len / units as f32;
    for i in 0..units {
        let t = -len * 0.5 + (i as f32 + 0.5) * unit;
        let shuttered = rng.random_bool((c.wear * 0.7) as f64);
        if shuttered {
            face.bx(g, f, Metal, t, y0 + h * 0.45, 0.06, Vec3::new(unit * 0.9, h * 0.9, 0.08), mul(pick(rng, &c.palette.metal_paint), 0.8));
            grime(g, c, rng, face, y0, y0 + h, 2.0);
        } else {
            face.bx(g, f, Glass, t, y0 + h * 0.45, 0.04, Vec3::new(unit * 0.9, h * 0.8, 0.06), c.palette.glass_dark);
            if rng.random_bool((0.5 + 0.4 * c.lit_chance()) as f64) {
                let wc = pick(rng, &c.palette.glow_colors);
                face.bx(g, f, Glow, t, y0 + h * 0.45, 0.07, Vec3::new(unit * 0.86, h * 0.72, 0.03), mul(wc, 0.6));
            }
            // door
            face.bx(g, f, Matte, t + unit * 0.3, y0 + 1.1, 0.09, Vec3::new(1.0, 2.2, 0.06), DARK);
        }
        // fascia and sign
        face.bx(g, f, Matte, t, y0 + h * 0.93, 0.12, Vec3::new(unit * 0.96, h * 0.14, 0.2), mul(c.style.trim_color, 0.8));
        if rng.random_bool(c.rules.sign_chance as f64) {
            sign_board(g, c, rng, face, t, y0 + h * 0.93, unit * 0.7, h * 0.1);
        }
        if rng.random_bool(0.35) {
            awning(g, c, rng, face, t, y0 + h * 0.78, unit * 0.8);
        }
    }
}

/// Roll-up door with a lit lamp above it.
pub fn roller_door(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, w: f32, h: f32, color: [f32; 3]) {
    let f = &c.frame;
    face.bx(g, f, Metal, t, h * 0.5, 0.05, Vec3::new(w, h, 0.08), color);
    face.bx(g, f, Matte, t, h + 0.15, 0.08, Vec3::new(w + 0.4, 0.3, 0.14), CHARCOAL);
    face.bx(g, f, Glow, t, h + 0.5, 0.25, Vec3::new(0.5, 0.18, 0.3), c.palette.street_lamp);
}

/// Recessed doorway with glass doors, a canopy and lamps.
pub fn entrance(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, w: f32, h: f32) {
    let f = &c.frame;
    face.bx(g, f, Matte, t, h * 0.5, -0.3, Vec3::new(w + 0.6, h, 0.8), DARK);
    face.bx(g, f, Glass, t, h * 0.5, -0.5, Vec3::new(w, h * 0.95, 0.06), c.palette.glass_dark);
    let lit = rng.random_bool((0.6 + 0.4 * c.lit_chance()) as f64);
    if lit {
        face.bx(g, f, Glow, t, h * 0.5, -0.45, Vec3::new(w * 0.9, h * 0.85, 0.03), mul(pick(rng, &c.style.window_colors), 0.7));
    }
    canopy(g, c, face, t, h + 0.3, w + 1.6, 2.0, mul(c.style.trim_color, 0.9));
    face.bx(g, f, Matte, t, 0.08, 1.0, Vec3::new(w + 1.2, 0.16, 2.2), mul(c.palette.paving, 1.1));
}

pub fn canopy(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, w: f32, depth: f32, color: [f32; 3]) {
    let f = &c.frame;
    face.bx(g, f, Panel, t, y, depth * 0.5, Vec3::new(w, 0.25, depth), color);
    face.bx(g, f, Glow, t, y - 0.16, depth * 0.5, Vec3::new(w * 0.5, 0.06, depth * 0.3), mul(c.palette.lamp, 0.8));
}

pub fn awning(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y: f32, w: f32) {
    let f = &c.frame;
    let col = pick(rng, &c.palette.awning_colors);
    let col = mul(col, 1.0 - 0.4 * c.wear);
    face.bx(g, f, Tarp, t, y, 0.7, Vec3::new(w, 0.1, 1.4), col);
    face.bx(g, f, Tarp, t, y - 0.2, 1.4, Vec3::new(w, 0.4, 0.06), col);
    for side in [-1.0f32, 1.0] {
        face.bx(g, f, Matte, t + side * w * 0.48, y - 0.1, 0.7, Vec3::new(0.05, 0.2, 1.4), CHARCOAL);
    }
}

/// Sign: dark backing with blocks of glowing lettering.
pub fn sign_board(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y: f32, w: f32, h: f32) {
    let f = &c.frame;
    let sc = pick(rng, &c.palette.sign_colors);
    let k = 1.0 - 0.6 * c.wear * rng.random::<f32>();
    face.bx(g, f, Matte, t, y, 0.16, Vec3::new(w + 0.2, h + 0.2, 0.12), DARK);
    let letters = ((w / (h * 0.8)).floor() as i32).clamp(2, 12);
    let lw = w / letters as f32;
    for i in 0..letters {
        if rng.random_bool(0.15 + 0.5 * c.wear as f64) && i % 3 == 1 {
            continue; // dead letters
        }
        let lt = t - w * 0.5 + (i as f32 + 0.5) * lw;
        face.bx(g, f, Glow, lt, y, 0.24, Vec3::new(lw * 0.7, h * 0.7, 0.05), mul(sc, k));
    }
}

/// Tall narrow sign hung off the facade.
pub fn vertical_sign(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y0: f32, h: f32) {
    let f = &c.frame;
    let sc = pick(rng, &c.palette.sign_colors);
    let w = 1.1;
    face.bx(g, f, Matte, t, y0 + h * 0.5, 0.8, Vec3::new(w + 0.2, h + 0.2, 0.3), DARK);
    for k in 0..2 {
        face.bx(g, f, Matte, t, y0 + h * (0.25 + 0.5 * k as f32), 0.35, Vec3::new(0.08, 0.08, 0.7), CHARCOAL);
    }
    let cells = (h / (w * 0.9)).floor().max(1.0) as i32;
    for i in 0..cells {
        if rng.random_bool((c.wear * 0.4) as f64) {
            continue;
        }
        let y = y0 + h * 0.5 - h * 0.5 + (i as f32 + 0.5) * (h / cells as f32);
        face.bx(g, f, Glow, t, y, 0.97, Vec3::new(w * 0.7, (h / cells as f32) * 0.7, 0.05), mul(sc, 0.9));
    }
}

/// A framed LED billboard: a grid of brightly lit cells in two colours plus a
/// dim frame. Dead pixels appear with wear.
pub fn led_screen(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y: f32, w: f32, h: f32) {
    let f = &c.frame;
    face.bx(g, f, Matte, t, y, 0.18, Vec3::new(w + 0.4, h + 0.4, 0.36), DARK);
    let a = pick(rng, &c.palette.led_colors);
    let b = pick(rng, &c.palette.led_colors);
    let cols = 5;
    let rows = 3;
    let cw = w / cols as f32;
    let ch = h / rows as f32;
    for i in 0..cols {
        for j in 0..rows {
            if rng.random_bool((c.wear * 0.5) as f64) {
                continue;
            }
            let tt = ((i as f32 / cols as f32 + j as f32 / rows as f32) * 0.5 + rng.random_range(-0.2..0.2)).clamp(0.0, 1.0);
            let col = mix(a, b, tt);
            let k = rng.random_range(0.6..1.1);
            face.bx(g, f, Glow, t - w * 0.5 + (i as f32 + 0.5) * cw, y - h * 0.5 + (j as f32 + 0.5) * ch, 0.37, Vec3::new(cw * 0.96, ch * 0.96, 0.05), mul(col, k));
        }
    }
}

/// Roof billboard on steel legs.
pub fn billboard(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, yaw: f32, y_base: f32, w: f32, h: f32) {
    let f = &c.frame;
    let face = Face::from_edge(p - Vec2::new(yaw.cos(), yaw.sin()) * w * 0.5, p + Vec2::new(yaw.cos(), yaw.sin()) * w * 0.5);
    let legs = 1.2;
    for side in [-0.4f32, 0.4] {
        face.bx(g, f, Matte, side * w, y_base + legs * 0.5, 0.0, Vec3::new(0.25, legs, 0.25), c.palette.steel);
    }
    led_screen(g, c, rng, &face, 0.0, y_base + legs + h * 0.5, w, h);
}

/// Rust and soot streaks on a face between two heights.
pub fn grime(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, y1: f32, amount: f32) {
    let f = &c.frame;
    let n = ((c.style.grime + c.wear) * amount * rng.random_range(0.5..1.5)) as i32;
    for _ in 0..n {
        let t = rng.random_range(-face.len * 0.45..face.len * 0.45);
        let sh = rng.random_range(0.8..(y1 - y0).max(1.0));
        face.bx(g, f, Rust, t, y1 - sh * 0.5, 0.02, Vec3::new(rng.random_range(0.3..1.2), sh, 0.03), RUST_STAIN);
    }
}

/// Vertical downpipes with a rust patch where they leak.
pub fn downpipes(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y_top: f32) {
    let f = &c.frame;
    let n = (face.len / 9.0).floor() as i32 + 1;
    for i in 0..n {
        let t = -face.len * 0.5 + 0.6 + i as f32 * (face.len - 1.2) / n.max(1) as f32;
        face.bx(g, f, Metal, t, y_top * 0.5, 0.12, Vec3::new(0.14, y_top, 0.14), mul(c.style.trim_color, 0.7));
        if rng.random_bool(c.wear as f64) {
            face.bx(g, f, Rust, t, y_top * 0.3, 0.03, Vec3::new(0.5, y_top * 0.4, 0.03), RUST_STAIN);
        }
    }
}

/// Zigzag fire escape down a face: landings per floor, diagonal stair runs.
#[allow(clippy::too_many_arguments)]
pub fn fire_escape(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y0: f32, floors: i32, fh: f32) {
    let f = &c.frame;
    let steel = mul(c.palette.steel, 0.4 + 0.2 * rng.random::<f32>());
    let w = 4.0;
    let d = 1.1;
    for k in 1..floors {
        let y = y0 + k as f32 * fh;
        face.bx(g, f, Metal, t, y, d * 0.5, Vec3::new(w, 0.08, d), steel);
        face.bx(g, f, Matte, t, y + 0.5, d - 0.03, Vec3::new(w, 0.04, 0.04), steel);
        face.bx(g, f, Matte, t, y + 1.0, d - 0.03, Vec3::new(w, 0.04, 0.04), steel);
        for side in [-1.0f32, 1.0] {
            face.bx(g, f, Matte, t + side * w * 0.5, y + 0.5, d - 0.03, Vec3::new(0.05, 1.0, 0.05), steel);
        }
        // stair run to the floor below, alternating direction
        let dir = if k % 2 == 0 { 1.0 } else { -1.0 };
        let a = f.at2(face.at(t - dir * w * 0.45, d * 0.5), y);
        let b = f.at2(face.at(t + dir * w * 0.45, d * 0.5), y - fh + 0.1);
        g.part(Metal).beam(a, b, 0.5, rgba(steel));
    }
    // ladder down from the first landing
    face.bx(g, f, Matte, t + w * 0.4, y0 + fh * 0.6, d * 0.5, Vec3::new(0.5, fh * 0.9, 0.05), steel);
}

#[allow(clippy::too_many_arguments)]
pub fn balcony(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, w: f32, depth: f32, slab: [f32; 3], rail: [f32; 3]) {
    let f = &c.frame;
    face.bx(g, f, Concrete, t, y, depth * 0.5, Vec3::new(w, 0.22, depth), slab);
    face.bx(g, f, Matte, t, y + 0.6, depth - 0.05, Vec3::new(w, 1.0, 0.05), rail);
    for side in [-1.0f32, 1.0] {
        face.bx(g, f, Matte, t + side * (w * 0.5 - 0.03), y + 0.6, depth * 0.5, Vec3::new(0.05, 1.0, depth), rail);
    }
}

pub fn ac_unit(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, d: f32) {
    let f = &c.frame;
    face.bx(g, f, Metal, t, y, d, Vec3::new(0.8, 0.6, 0.35), mul(c.palette.steel, 0.9));
    face.bx(g, f, Matte, t, y, d + 0.19, Vec3::new(0.5, 0.5, 0.03), DARK);
    face.bx(g, f, Matte, t, y - 0.33, d * 0.6, Vec3::new(0.7, 0.05, 0.6), CHARCOAL);
}

// ── roofs ────────────────────────────────────────────────────────────────────

/// Scatter rooftop clutter inside a footprint at height `y`.
pub fn roof_gear(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2], y: f32, density: f32) {
    let area = polygon_area(poly).abs();
    let inner = offset_poly(poly, 1.3);
    if inner.len() < 3 {
        return;
    }
    let (lo, hi) = bounds(&inner);
    let n = ((area / 60.0) * density).round().clamp(0.0, 9.0) as i32;
    let f = &c.frame;
    let mut placed = 0;
    for _ in 0..n * 6 {
        if placed >= n {
            break;
        }
        let p = Vec2::new(rng.random_range(lo.x..=hi.x), rng.random_range(lo.y..=hi.y));
        if !point_in_polygon(p, &inner) {
            continue;
        }
        placed += 1;
        match rng.random_range(0..9) {
            0 | 1 => {
                let s = rng.random_range(0.8..1.6);
                f.bx(g, Metal, p.x, y + s * 0.45, p.y, Vec3::new(s * 1.3, s * 0.9, s), mul(c.palette.steel, 0.85));
                f.bx(g, Matte, p.x, y + s * 0.92, p.y, Vec3::new(s * 1.1, 0.04, s * 0.8), DARK);
            }
            2 => water_tank(g, f, p, y, rng.random_range(0.9..1.4), rng.random_range(1.6..2.6), if rng.random_bool(0.5) { RUST } else { mul(c.palette.steel, 0.8) }),
            3 => vent_stack(g, f, p, y, rng.random_range(0.25..0.45), rng.random_range(1.2..3.0), mul(c.palette.steel, 0.7)),
            4 => dish(g, f, p, y, rng.random_range(0.5..1.1), c.palette.steel),
            5 => antenna_mast(g, c, p, y, rng.random_range(4.0..10.0)),
            6 => solar_array(g, f, p, y, rng.random_range(2.0..4.0), rng.random_range(1.5..3.0), rng.random_range(0.0..1.5), c.palette.glass_dark),
            _ => {
                // stair bulkhead
                f.bx(g, Concrete, p.x, y + 1.3, p.y, Vec3::new(3.0, 2.6, 2.4), mul(pick(rng, &c.palette.concrete), 0.9));
                f.bx(g, Matte, p.x, y + 1.2, p.y - 1.25, Vec3::new(1.0, 2.1, 0.08), DARK);
            }
        }
    }
}

pub fn water_tank(g: &mut Geometry, f: &Frame, p: Vec2, y: f32, r: f32, h: f32, color: [f32; 3]) {
    let w = f.to_world(p.x, p.y);
    let legs = 1.0;
    for (du, dv) in [(-0.6f32, -0.6f32), (0.6, -0.6), (0.6, 0.6), (-0.6, 0.6)] {
        f.bx(g, Matte, p.x + du * r, y + legs * 0.5, p.y + dv * r, Vec3::new(0.12, legs, 0.12), CHARCOAL);
    }
    g.part(if color == RUST { Rust } else { Metal }).cylinder(w, r, r, y + legs, y + legs + h, 12, rgba(color));
    g.part(Metal).cylinder(w, r, r * 0.3, y + legs + h, y + legs + h + 0.5, 12, rgba(mul(color, 0.8)));
}

pub fn vent_stack(g: &mut Geometry, f: &Frame, p: Vec2, y: f32, r: f32, h: f32, color: [f32; 3]) {
    let w = f.to_world(p.x, p.y);
    g.part(Metal).cylinder(w, r, r, y, y + h, 10, rgba(color));
    g.part(Metal).cylinder(w, r * 1.4, r * 1.4, y + h, y + h + 0.3, 10, rgba(mul(color, 0.8)));
}

pub fn dish(g: &mut Geometry, f: &Frame, p: Vec2, y: f32, r: f32, color: [f32; 3]) {
    f.bx(g, Matte, p.x, y + 0.6, p.y, Vec3::new(0.1, 1.2, 0.1), CHARCOAL);
    let w = f.to_world(p.x, p.y);
    g.part(Metal).frustum(&MeshData::ngon(w, r * 0.2, 10, 0.0, y + 1.0), &MeshData::ngon(w + Vec2::new(0.0, 0.3), r, 10, 0.0, y + 1.4), rgba(color));
}

pub fn antenna_mast(g: &mut Geometry, c: &Ctx, p: Vec2, y: f32, h: f32) {
    let f = &c.frame;
    f.bx(g, Matte, p.x, y + h * 0.5, p.y, Vec3::new(0.25, h, 0.25), c.palette.steel);
    f.bx(g, Matte, p.x, y + h * 0.7, p.y, Vec3::new(1.4, 0.06, 0.06), c.palette.steel);
    f.bx(g, Glow, p.x, y + h + 0.15, p.y, Vec3::new(0.35, 0.35, 0.35), c.palette.aviation_light);
}

#[allow(clippy::too_many_arguments)]
pub fn solar_array(g: &mut Geometry, f: &Frame, p: Vec2, y: f32, w: f32, d: f32, yaw: f32, color: [f32; 3]) {
    f.bx_yaw(g, Glass, p.x, y + 0.6, p.y, Vec3::new(w, 0.08, d), color, yaw);
    f.bx_yaw(g, Matte, p.x, y + 0.3, p.y, Vec3::new(w * 0.8, 0.6, 0.1), CHARCOAL, yaw);
}

pub fn helipad(g: &mut Geometry, c: &Ctx, p: Vec2, y: f32, r: f32) {
    let f = &c.frame;
    let w = f.to_world(p.x, p.y);
    g.part(Concrete).cylinder(w, r, r, y, y + 0.3, 16, rgba(mul(pick_first(&c.palette.concrete), 0.8)));
    g.part(Glow).frustum(&MeshData::ngon(w, r * 0.92, 16, 0.0, y + 0.31), &MeshData::ngon(w, r * 0.92, 16, 0.0, y + 0.36), rgba(mul(c.palette.lamp, 0.4)));
    f.bx(g, Glow, p.x, y + 0.4, p.y, Vec3::new(r * 0.9, 0.08, 0.3), mul(c.palette.lamp, 0.9));
    f.bx(g, Glow, p.x - r * 0.35, y + 0.4, p.y, Vec3::new(0.3, 0.08, r * 0.8), mul(c.palette.lamp, 0.9));
    f.bx(g, Glow, p.x + r * 0.35, y + 0.4, p.y, Vec3::new(0.3, 0.08, r * 0.8), mul(c.palette.lamp, 0.9));
}

fn pick_first(v: &[[f32; 3]]) -> [f32; 3] {
    v.first().copied().unwrap_or([0.5, 0.5, 0.5])
}

/// Big HVAC / chiller unit with fan rings on top.
#[allow(clippy::too_many_arguments)]
pub fn chiller(g: &mut Geometry, c: &Ctx, p: Vec2, y: f32, w: f32, d: f32, yaw: f32, color: [f32; 3]) {
    let f = &c.frame;
    let h = 2.2;
    f.bx_yaw(g, Panel, p.x, y + h * 0.5, p.y, Vec3::new(w, h, d), color, yaw);
    let fans = (w / 2.2).floor().max(1.0) as i32;
    let (s, co) = yaw.sin_cos();
    for i in 0..fans {
        let t = -w * 0.5 + (i as f32 + 0.5) * (w / fans as f32);
        let q = Vec2::new(p.x + t * co, p.y + t * s);
        let wq = f.to_world(q.x, q.y);
        let fr = (0.9f32).min(d * 0.4);
        g.part(Matte).cylinder(wq, fr, fr, y + h, y + h + 0.3, 12, rgba(DARK));
    }
}

// ── site ─────────────────────────────────────────────────────────────────────

pub fn tree(g: &mut Geometry, f: &Frame, p: Vec2, h: f32, canopy: f32, bark: [f32; 3], leaf: [f32; 3]) {
    tree_world(g, f.to_world(p.x, p.y), h, canopy, bark, leaf);
}

pub fn tree_world(g: &mut Geometry, w: Vec2, h: f32, canopy: f32, bark: [f32; 3], leaf: [f32; 3]) {
    g.part(Wood).cylinder(w, 0.22, 0.16, 0.0, h, 7, rgba(bark));
    let c = g.part(Foliage);
    c.frustum(&MeshData::ngon(w, canopy * 0.5, 7, 0.0, h), &MeshData::ngon(w, canopy * 0.2, 7, 0.4, h + canopy * 0.8), rgba(leaf));
    c.frustum(&MeshData::ngon(w, canopy * 0.35, 7, 0.3, h - canopy * 0.3), &MeshData::ngon(w, canopy * 0.5, 7, 0.0, h), rgba(mul(leaf, 0.85)));
}

pub fn lamp_post(g: &mut Geometry, f: &Frame, p: Vec2, h: f32, glow: [f32; 3]) {
    lamp_post_world(g, f.to_world(p.x, p.y), h, glow);
}

pub fn lamp_post_world(g: &mut Geometry, w: Vec2, h: f32, glow: [f32; 3]) {
    g.part(Matte).cylinder(w, 0.12, 0.08, 0.0, h, 8, rgba(CHARCOAL));
    g.part(Matte).boxed(Vec3::new(w.x, h + 0.15, w.y), Vec3::new(0.6, 0.3, 0.6), 0.0, rgba(CHARCOAL));
    g.part(Glow).boxed(Vec3::new(w.x, h - 0.05, w.y), Vec3::new(0.5, 0.15, 0.5), 0.0, rgba(glow));
}

pub fn bench_world(g: &mut Geometry, w: Vec2, yaw: f32, wood: [f32; 3], metal: [f32; 3]) {
    let m = g.part(Wood);
    m.boxed(Vec3::new(w.x, 0.45, w.y), Vec3::new(1.8, 0.08, 0.5), yaw, rgba(wood));
    let (s, c) = yaw.sin_cos();
    let back = Vec2::new(-s, c) * 0.22;
    m.boxed(Vec3::new(w.x + back.x, 0.8, w.y + back.y), Vec3::new(1.8, 0.45, 0.06), yaw, rgba(wood));
    let mm = g.part(Matte);
    for side in [-0.8f32, 0.8] {
        let o = Vec2::new(c, s) * side;
        mm.boxed(Vec3::new(w.x + o.x, 0.22, w.y + o.y), Vec3::new(0.08, 0.44, 0.5), yaw, rgba(metal));
    }
}

pub fn bin_world(g: &mut Geometry, w: Vec2, color: [f32; 3]) {
    g.part(Metal).cylinder(w, 0.35, 0.35, 0.0, 1.0, 10, rgba(color));
    g.part(Matte).cylinder(w, 0.37, 0.37, 1.0, 1.1, 10, rgba(DARK));
}

pub fn bollard_world(g: &mut Geometry, w: Vec2, color: [f32; 3]) {
    g.part(Matte).cylinder(w, 0.14, 0.12, 0.0, 0.9, 8, rgba(color));
}

/// Rail fence between two lot-local points, optionally topped with barbed wire.
pub fn fence(g: &mut Geometry, c: &Ctx, a: Vec2, b: Vec2, h: f32, barbed: bool) {
    let f = &c.frame;
    let len = a.distance(b);
    if len < 0.5 {
        return;
    }
    let face = Face::from_edge(a, b);
    let posts = (len / 2.6).ceil().max(1.0) as i32;
    let color = mul(c.palette.steel, 0.5);
    for i in 0..=posts {
        let t = -len * 0.5 + i as f32 * (len / posts as f32);
        face.bx(g, f, Matte, t, h * 0.5, 0.0, Vec3::new(0.08, h, 0.08), color);
    }
    for k in 0..3 {
        let y = h * (0.25 + 0.35 * k as f32);
        face.bx(g, f, Matte, 0.0, y, 0.0, Vec3::new(len, 0.04, 0.04), color);
    }
    // mesh panel: thin, dark, slightly translucent-looking grey
    face.bx(g, f, Matte, 0.0, h * 0.5, 0.0, Vec3::new(len, h * 0.96, 0.015), [0.28, 0.29, 0.30]);
    if barbed {
        let wa = f.at2(a, h + 0.2);
        let wb = f.at2(b, h + 0.2);
        g.part(Rust).tube(wa, wb, 0.12, 5, rgba(RUST));
    }
}

/// Fence around a polygon, leaving one face open as a gate.
pub fn fence_poly(g: &mut Geometry, c: &Ctx, poly: &[Vec2], h: f32, skip_face: Option<usize>, barbed: bool) {
    let n = poly.len();
    for i in 0..n {
        if skip_face == Some(i) {
            // gate posts and a sliding gate half-open
            let a = poly[i];
            let b = poly[(i + 1) % n];
            let face = Face::from_edge(a, b);
            let f = &c.frame;
            for side in [-1.0f32, 1.0] {
                face.bx(g, f, Matte, side * face.len * 0.5, h * 0.6, 0.0, Vec3::new(0.2, h * 1.2, 0.2), mul(c.palette.steel, 0.6));
            }
            face.bx(g, f, Matte, -face.len * 0.2, h * 0.5, 0.15, Vec3::new(face.len * 0.55, h * 0.9, 0.05), mul(c.palette.steel, 0.5));
            continue;
        }
        fence(g, c, poly[i], poly[(i + 1) % n], h, barbed);
    }
}

pub fn bollards(g: &mut Geometry, c: &Ctx, face: &Face, n: i32, d: f32) {
    let f = &c.frame;
    for i in 0..n {
        let t = -face.len * 0.4 + i as f32 * (face.len * 0.8 / (n - 1).max(1) as f32);
        face.bx(g, f, Matte, t, 0.45, d, Vec3::new(0.25, 0.9, 0.25), mul(c.palette.steel, 0.7));
    }
}

#[allow(clippy::too_many_arguments)]
pub fn tank_vertical(g: &mut Geometry, f: &Frame, p: Vec2, r: f32, y0: f32, h: f32, s: Surface, color: [f32; 3]) {
    let w = f.to_world(p.x, p.y);
    g.part(s).cylinder(w, r, r, y0, y0 + h, 14, rgba(color));
    g.part(Matte).cylinder(w, r * 1.03, r * 1.03, y0 + h, y0 + h + 0.15, 14, rgba(CHARCOAL));
    // ladder and top rail
    g.part(Matte).boxed(Vec3::new(w.x + r, y0 + h * 0.5, w.y), Vec3::new(0.08, h, 0.5), 0.0, rgba(CHARCOAL));
    g.part(Matte).cylinder(w, r * 0.95, r * 0.95, y0 + h + 0.15, y0 + h + 1.0, 14, rgba(mul(CHARCOAL, 0.5)));
}

pub fn tank_horizontal(g: &mut Geometry, f: &Frame, p: Vec2, angle: f32, len: f32, r: f32, y: f32, color: [f32; 3]) {
    let w = f.to_world(p.x, p.y);
    let a = f.yaw() + angle;
    let dir = Vec2::new(a.cos(), a.sin());
    g.part(Metal).pipe(w, dir, len, r, y, 12, rgba(color));
    for t in [-0.35f32, 0.35] {
        let q = w + dir * (len * t);
        g.part(Concrete).boxed(Vec3::new(q.x, (y - r) * 0.5, q.y), Vec3::new(0.8, y - r, r * 1.6), a, rgba(mul(color, 0.6)));
    }
}

pub fn chimney(g: &mut Geometry, f: &Frame, p: Vec2, r: f32, y0: f32, h: f32, color: [f32; 3], light: [f32; 3]) {
    let w = f.to_world(p.x, p.y);
    g.part(Metal).cylinder(w, r, r * 0.75, y0, y0 + h, 10, rgba(color));
    g.part(Matte).cylinder(w, r * 0.8, r * 0.8, y0 + h, y0 + h + 0.4, 10, rgba(DARK));
    g.part(Glow).boxed(Vec3::new(w.x, y0 + h + 0.6, w.y), Vec3::new(0.35, 0.35, 0.35), 0.0, rgba(light));
}

/// Pipe along a sequence of lot-local (u, y, v) points with elbows.
pub fn pipe_run(g: &mut Geometry, f: &Frame, pts: &[Vec3], r: f32, color: [f32; 3]) {
    for w in pts.windows(2) {
        let a = f.at(w[0].x, w[0].y, w[0].z);
        let b = f.at(w[1].x, w[1].y, w[1].z);
        g.part(Metal).tube(a, b, r, 8, rgba(color));
        g.part(Metal).boxed(b, Vec3::splat(r * 2.4), f.yaw(), rgba(mul(color, 0.85)));
    }
}

/// A shipping container with ribs and doors, placed in parcel coordinates.
#[allow(clippy::too_many_arguments)]
pub fn container_unit(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, u: f32, v: f32, yaw: f32, len: f32, wid: f32, h: f32, y0: f32, color: [f32; 3]) {
    let f = &c.frame;
    let yc = y0 + h * 0.5;
    f.bx_yaw(g, Metal, u, yc, v, Vec3::new(len, h, wid), color, yaw);
    let (s, co) = yaw.sin_cos();
    let place = |g: &mut Geometry, sfc: Surface, lu: f32, y: f32, lv: f32, size: Vec3, col: [f32; 3]| {
        let p = Vec2::new(u + lu * co - lv * s, v + lu * s + lv * co);
        f.bx_yaw(g, sfc, p.x, y, p.y, size, col, yaw);
    };
    // corner posts and door end
    for (lu, lv) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        place(g, Matte, lu * (len * 0.5 - 0.1), yc, lv * (wid * 0.5 - 0.1), Vec3::new(0.2, h * 1.01, 0.2), mul(color, 0.6));
    }
    let door = if rng.random_bool(0.5) { -1.0 } else { 1.0 };
    place(g, Matte, door * (len * 0.5 + 0.02), yc, 0.0, Vec3::new(0.04, h * 0.92, wid * 0.92), mul(color, 0.55));
    for k in 0..2 {
        place(g, Matte, door * (len * 0.5 + 0.05), yc, (k as f32 - 0.5) * wid * 0.5, Vec3::new(0.06, h * 0.7, 0.08), mul(c.palette.steel, 0.6));
    }
    // rust by wear
    for _ in 0..((c.wear * 3.0) as i32) {
        let side = if rng.random_bool(0.5) { -1.0 } else { 1.0 };
        place(g, Rust, rng.random_range(-len * 0.4..len * 0.4), yc + rng.random_range(-h * 0.3..h * 0.3), side * (wid * 0.5 + 0.03), Vec3::new(rng.random_range(0.4..1.5), rng.random_range(0.3..1.2), 0.03), RUST_STAIN);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Vehicle {
    Car,
    Van,
    Truck,
    Wreck,
    Bus,
}

pub fn vehicle(g: &mut Geometry, f: &Frame, rng: &mut impl Rng, p: Vec2, yaw: f32, kind: Vehicle, color: [f32; 3]) {
    let w = f.to_world(p.x, p.y);
    let a = f.yaw() + yaw;
    let m = |g: &mut Geometry, s: Surface, dx: f32, y: f32, dz: f32, size: Vec3, col: [f32; 3]| {
        let (sn, cs) = a.sin_cos();
        let q = Vec3::new(w.x + dx * cs - dz * sn, y, w.y + dx * sn + dz * cs);
        g.part(s).boxed(q, size, a, rgba(col));
    };
    match kind {
        Vehicle::Car => {
            m(g, Metal, 0.0, 0.7, 0.0, Vec3::new(4.2, 0.9, 1.85), color);
            m(g, Glass, -0.3, 1.4, 0.0, Vec3::new(2.2, 0.6, 1.6), [0.12, 0.15, 0.18]);
            for (dx, dz) in [(-1.4f32, -0.95f32), (1.4, -0.95), (-1.4, 0.95), (1.4, 0.95)] {
                m(g, Matte, dx, 0.33, dz, Vec3::new(0.66, 0.66, 0.22), DARK);
            }
        }
        Vehicle::Van => {
            m(g, Metal, 0.0, 1.2, 0.0, Vec3::new(5.2, 1.9, 2.0), color);
            m(g, Glass, 2.1, 1.7, 0.0, Vec3::new(0.9, 0.7, 1.9), [0.12, 0.15, 0.18]);
            for (dx, dz) in [(-1.6f32, -1.0f32), (1.6, -1.0), (-1.6, 1.0), (1.6, 1.0)] {
                m(g, Matte, dx, 0.38, dz, Vec3::new(0.75, 0.75, 0.25), DARK);
            }
        }
        Vehicle::Truck => {
            m(g, Metal, -1.5, 1.6, 0.0, Vec3::new(7.0, 2.6, 2.4), color);
            m(g, Metal, 3.2, 1.3, 0.0, Vec3::new(2.2, 2.2, 2.3), mul(pick(rng, &[[0.6, 0.6, 0.62], [0.2, 0.25, 0.4], [0.5, 0.12, 0.1]]), 1.0));
            m(g, Glass, 4.2, 1.8, 0.0, Vec3::new(0.3, 0.8, 2.1), [0.12, 0.15, 0.18]);
            for dx in [-3.5f32, -2.3, 2.6] {
                for dz in [-1.1f32, 1.1] {
                    m(g, Matte, dx, 0.5, dz, Vec3::new(1.0, 1.0, 0.35), DARK);
                }
            }
        }
        Vehicle::Bus => {
            m(g, Metal, 0.0, 1.6, 0.0, Vec3::new(11.5, 3.0, 2.5), color);
            m(g, Glass, 0.0, 2.2, 0.0, Vec3::new(11.0, 0.9, 2.56), [0.12, 0.15, 0.18]);
            m(g, Glass, 5.6, 1.9, 0.0, Vec3::new(0.4, 1.6, 2.3), [0.12, 0.15, 0.18]);
            for dx in [-3.8f32, 3.6] {
                for dz in [-1.15f32, 1.15] {
                    m(g, Matte, dx, 0.5, dz, Vec3::new(1.0, 1.0, 0.3), DARK);
                }
            }
        }
        Vehicle::Wreck => {
            m(g, Rust, 0.0, 0.4, 0.0, Vec3::new(4.0, 0.8, 1.8), mul(color, 0.6));
            m(g, Rust, -0.3, 0.95, 0.0, Vec3::new(2.0, 0.4, 1.5), RUST);
            if rng.random_bool(0.5) {
                m(g, Matte, 1.3, 0.3, 0.95, Vec3::new(0.66, 0.6, 0.22), DARK);
            }
        }
    }
}

/// Heap of scrap: tyres, sheet, drums and a car on top sometimes.
pub fn scrap_pile(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, r: f32, h: f32) {
    let f = &c.frame;
    let n = (r * r * 1.2) as i32 + 4;
    for _ in 0..n {
        let d = rng.random_range(0.0..r);
        let a = rng.random_range(0.0..std::f32::consts::TAU);
        let q = p + Vec2::new(a.cos(), a.sin()) * d;
        let size = Vec3::new(rng.random_range(0.5..2.2), rng.random_range(0.3..1.2), rng.random_range(0.5..2.0));
        let y = (1.0 - d / r) * h * rng.random_range(0.3..1.0);
        let (s, col) = match rng.random_range(0..4) {
            0 => (Rust, RUST),
            1 => (Metal, pick(rng, &c.palette.container_colors)),
            2 => (Tarp, pick(rng, &c.palette.tarp_colors)),
            _ => (Matte, DARK),
        };
        f.bx_yaw(g, s, q.x, y + size.y * 0.5, q.y, size, mul(col, 0.8), rng.random_range(0.0..3.0));
    }
    // tyre stack
    let w = f.to_world(p.x + r * 0.6, p.y);
    for k in 0..rng.random_range(2..6) {
        g.part(Matte).cylinder(w, 0.45, 0.45, k as f32 * 0.25, k as f32 * 0.25 + 0.22, 10, rgba(DARK));
    }
}

/// Market stall against a face: tarp roof, counter, a sign strip.
pub fn stall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, d: f32) {
    let f = &c.frame;
    let tc = mul(pick(rng, &c.palette.tarp_colors), 0.9);
    face.bx(g, f, Tarp, t, 2.4, d, Vec3::new(4.2, 0.1, 2.2), tc);
    face.bx(g, f, Wood, t, 0.5, d, Vec3::new(3.6, 1.0, 1.2), c.palette.wood);
    for side in [-1.0f32, 1.0] {
        face.bx(g, f, Matte, t + side * 2.0, 1.2, d + 1.0, Vec3::new(0.08, 2.4, 0.08), CHARCOAL);
        face.bx(g, f, Matte, t + side * 2.0, 1.2, d - 1.0, Vec3::new(0.08, 2.4, 0.08), CHARCOAL);
    }
    let sc = pick(rng, &c.palette.sign_colors);
    face.bx(g, f, Glow, t, 2.1, d + 1.1, Vec3::new(3.0, 0.3, 0.06), mul(sc, 0.9 - 0.5 * c.wear));
    // goods
    for i in 0..4 {
        face.bx(g, f, Tarp, t - 1.4 + i as f32 * 0.9, 1.2, d, Vec3::new(0.6, 0.4, 0.6), pick(rng, &c.palette.awning_colors));
    }
}

pub fn drum_fire(g: &mut Geometry, f: &Frame, p: Vec2) {
    let w = f.to_world(p.x, p.y);
    g.part(Rust).cylinder(w, 0.3, 0.3, 0.0, 0.9, 10, rgba(RUST));
    g.part(Glow).boxed(Vec3::new(w.x, 1.0, w.y), Vec3::new(0.3, 0.25, 0.3), 0.0, rgba([2.0, 0.9, 0.3]));
}

/// Sagging string of bulbs between two lot-local (u, y, v) anchors.
pub fn string_lights(g: &mut Geometry, f: &Frame, a: Vec3, b: Vec3, n: i32, color: [f32; 3]) {
    let wa = f.at(a.x, a.y, a.z);
    let wb = f.at(b.x, b.y, b.z);
    g.part(Matte).wire(wa, wb, 0.5, 0.03, rgba(DARK));
    for i in 1..n {
        let t = i as f32 / n as f32;
        let p = wa.lerp(wb, t) - Vec3::Y * (0.5 * 4.0 * t * (1.0 - t) + 0.15);
        g.part(Glow).boxed(p, Vec3::splat(0.16), 0.0, rgba(color));
    }
}

/// External zigzag stair up the side of a pile.
#[allow(clippy::too_many_arguments)]
pub fn stair_zigzag(g: &mut Geometry, f: &Frame, u: f32, v: f32, levels: i32, rise: f32, run: f32, color: [f32; 3]) {
    let mut y = 0.0;
    for l in 0..levels {
        let steps = 8;
        let dir = if l % 2 == 0 { 1.0 } else { -1.0 };
        for s in 0..steps {
            let t = (s as f32 + 0.5) / steps as f32;
            f.bx(g, Metal, u, y + t * rise, v + dir * (t * run - run * 0.5), Vec3::new(0.9, 0.16, 0.42), color);
        }
        f.bx(g, Matte, u - 0.5, y + rise * 0.5 + 0.5, v, Vec3::new(0.06, rise + 1.0, run), color);
        f.bx(g, Metal, u, y + rise + 0.08, v + dir * run * 0.5, Vec3::new(1.0, 0.16, 1.0), color);
        y += rise;
    }
    f.bx(g, Matte, u, y * 0.5, v + run * 0.55, Vec3::new(0.15, y, 0.15), color);
    f.bx(g, Matte, u, y * 0.5, v - run * 0.55, Vec3::new(0.15, y, 0.15), color);
}

pub fn ladder(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y0: f32, y1: f32, color: [f32; 3]) {
    let f = &c.frame;
    let h = y1 - y0;
    for side in [-0.25f32, 0.25] {
        face.bx(g, f, Matte, t + side, y0 + h * 0.5, 0.2, Vec3::new(0.05, h, 0.05), color);
    }
    let rungs = (h / 0.3) as i32;
    for k in 0..rungs {
        face.bx(g, f, Matte, t, y0 + (k as f32 + 0.5) * 0.3, 0.2, Vec3::new(0.5, 0.04, 0.04), color);
    }
}

pub fn hedge(g: &mut Geometry, c: &Ctx, a: Vec2, b: Vec2, h: f32, w: f32, color: [f32; 3]) {
    let face = Face::from_edge(a, b);
    face.bx(g, &c.frame, Foliage, 0.0, h * 0.5, 0.0, Vec3::new(face.len, h, w), color);
}

pub fn pool(g: &mut Geometry, c: &Ctx, cu: f32, cv: f32, w: f32, d: f32) {
    let f = &c.frame;
    f.bx(g, Paving, cu, 0.12, cv, Vec3::new(w + 1.6, 0.24, d + 1.6), mul(c.palette.paving, 1.2));
    f.bx(g, Glow, cu, 0.2, cv, Vec3::new(w, 0.1, d), mul(c.palette.water, 0.7));
}

/// Concrete jersey barrier in front of a face.
pub fn barrier(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, d: f32, w: f32) {
    let f = &c.frame;
    let col = mul(pick_first(&c.palette.concrete), 0.9);
    face.bx(g, f, Concrete, t, 0.25, d, Vec3::new(w, 0.5, 0.7), col);
    face.bx(g, f, Concrete, t, 0.7, d, Vec3::new(w, 0.4, 0.35), col);
}

/// Floodlight mast.
pub fn floodlight(g: &mut Geometry, c: &Ctx, p: Vec2, h: f32) {
    let f = &c.frame;
    f.bx(g, Matte, p.x, h * 0.5, p.y, Vec3::new(0.3, h, 0.3), mul(c.palette.steel, 0.6));
    f.bx(g, Matte, p.x, h + 0.2, p.y, Vec3::new(1.6, 0.4, 0.5), CHARCOAL);
    f.bx(g, Glow, p.x, h + 0.2, p.y - 0.28, Vec3::new(1.4, 0.3, 0.06), mul(c.palette.street_lamp, 1.3));
}

/// Gantry crane spanning across the lot at `v`, running along u.
pub fn gantry_crane(g: &mut Geometry, c: &Ctx, u: f32, v0: f32, v1: f32, h: f32) {
    let f = &c.frame;
    let steel = mul(c.palette.metal_paint[0], 1.1);
    for v in [v0, v1] {
        for du in [-1.6f32, 1.6] {
            f.bx(g, Matte, u + du, h * 0.5, v, Vec3::new(0.4, h, 0.4), steel);
        }
        f.bx(g, Matte, u, h, v, Vec3::new(4.0, 0.5, 0.5), steel);
    }
    let span = v1 - v0;
    f.bx(g, Metal, u, h + 0.5, (v0 + v1) * 0.5, Vec3::new(1.2, 1.0, span + 2.0), steel);
    f.bx(g, Matte, u, h - 0.3, (v0 + v1) * 0.4, Vec3::new(1.6, 0.6, 2.0), CHARCOAL);
    f.bx(g, Matte, u, h * 0.6, (v0 + v1) * 0.4, Vec3::new(0.05, h * 0.5, 0.05), DARK);
    f.bx(g, Glow, u, h + 1.1, v0, Vec3::new(0.3, 0.3, 0.3), c.palette.aviation_light);
}
