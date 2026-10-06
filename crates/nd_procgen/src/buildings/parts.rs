//! The parts toolkit every style is assembled from: footprint helpers,
//! facades, roofs, site furniture and props. Everything is placed in parcel
//! coordinates through the [`Frame`], except the `*_world` variants used by
//! the street generator (and by parks and rail, which lay out from world
//! polygons and pass [`Frame::world`]).
//!
//! Flat detail that is only ever seen from one side (panes, frames, signs,
//! stains, painted lines) is a [`decal`]: one quad lifted a few centimetres
//! off its wall, not a closed box. A box spends twelve triangles where two
//! do the job, and a tower has thousands of these.

use super::{Ctx, Face, Frame};
use crate::mesh::MeshData;
use crate::palette::Palette;
use crate::surface::{Geometry, Surface};
use glam::{Vec2, Vec3};
use nd_sim::zoning::{point_in_polygon, polygon_area, polygon_centroid};
use rand::seq::IndexedRandom;
use rand::Rng;

use Surface::*;

// ── colours and chance ───────────────────────────────────────────────────────

/// `random_bool` with the probability clamped: rule files and wear
/// arithmetic can produce chances a little over one, and `random_bool`
/// panics on them.
pub fn coin(rng: &mut impl Rng, p: f32) -> bool {
    let p = if p.is_finite() { p.clamp(0.0, 1.0) } else { 0.0 };
    rng.random_bool(p as f64)
}

/// Vertex alpha at or above this marks a window light in the Glow mesh; the
/// fraction above it is the window's rank in `0..1`. The renderer lights a
/// building's windows whose rank is below its lit share and swaps the mark
/// for that share's lookup (opaque glow ignores alpha anyway).
pub const WINDOW_LIGHT_MARK: f32 = 2.0;

/// Draw a window light with `emit` (which adds Glow). With
/// [`Ctx::dynamic_lights`] the new glow vertices are marked with
/// [`WINDOW_LIGHT_MARK`] plus a rank hashed from their position, and a
/// copy in dark glass is laid just behind them, so the window still reads as
/// glazing when the renderer switches the light off. Without it this is
/// just `emit`.
pub fn window_light(g: &mut Geometry, c: &Ctx, emit: impl FnOnce(&mut Geometry)) {
    let (vstart, istart) = (g.get(Glow).positions.len(), g.get(Glow).indices.len());
    emit(g);
    if !c.dynamic_lights {
        return;
    }
    let m = g.part(Glow);
    if m.positions.len() == vstart {
        return;
    }
    let p = m.positions[vstart];
    let h = (p[0].to_bits() ^ p[1].to_bits().rotate_left(11) ^ p[2].to_bits().rotate_left(22)).wrapping_mul(0x9E37_79B1);
    let rank = ((h >> 8) as f32 / (1u32 << 24) as f32).min(0.999);
    for col in &mut m.colors[vstart..] {
        col[3] = WINDOW_LIGHT_MARK + rank;
    }
    let mut glass = MeshData {
        positions: m.positions[vstart..].to_vec(),
        normals: m.normals[vstart..].to_vec(),
        colors: vec![rgba(c.palette.glass_dark); m.positions.len() - vstart],
        uvs: m.uvs[vstart..].to_vec(),
        tangents: m.tangents[vstart..].to_vec(),
        indices: m.indices[istart..].iter().map(|i| i - vstart as u32).collect(),
    };
    for (pos, n) in glass.positions.iter_mut().zip(&glass.normals) {
        for k in 0..3 {
            pos[k] -= n[k] * 0.02;
        }
    }
    g.part(Glass).append(&glass);
}

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
    // Two source vertices a few centimetres apart collapse onto each other
    // once offset; a zero-length face would then pass for the front face.
    let out = clean_poly(&out, 0.05);
    if out.len() < 3 {
        return scaled();
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
/// scaled about its centre until every vertex, and three points along every
/// edge, is inside. The edge points matter on concave lots, where an edge
/// between two inside vertices can still cut across a notch.
pub fn fit_inside(c: &Ctx, poly: &[Vec2]) -> Vec<Vec2> {
    let mut p = poly.to_vec();
    if p.len() < 3 {
        return p;
    }
    let anchor = interior_anchor(&c.lot_poly);
    let centre = poly_center(&p);
    if !c.inside_local(centre) {
        p = shift_poly(&p, anchor - centre);
    }
    for _ in 0..30 {
        if poly_inside(c, &p) {
            break;
        }
        // Shrink, and edge towards the deep interior: a polygon centred a
        // hand's breadth inside a slanted edge never fits by shrinking alone.
        p = scale_poly(&p, 0.9);
        let d = anchor - poly_center(&p);
        p = shift_poly(&p, d * 0.15);
    }
    p
}

/// Whether a lot-local polygon lies on the parcel: its vertices and points
/// every metre and a half along each edge, close enough that a diagonal lot
/// edge cannot slip between two samples by more than a few centimetres.
pub fn poly_inside(c: &Ctx, poly: &[Vec2]) -> bool {
    let n = poly.len();
    (0..n).all(|i| {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let steps = (a.distance(b) / 1.5).ceil().max(1.0) as i32;
        (0..steps).all(|k| c.inside_local(a.lerp(b, k as f32 / steps as f32)))
    })
}

/// Drop consecutive vertices closer than `eps`. Lot outlines from the
/// zoner sometimes repeat a vertex, which gives offset_poly a zero-length
/// edge and a mitre pointing nowhere.
pub fn clean_poly(poly: &[Vec2], eps: f32) -> Vec<Vec2> {
    let mut out: Vec<Vec2> = Vec::with_capacity(poly.len());
    for p in poly {
        if out.last().is_none_or(|q| q.distance(*p) > eps) {
            out.push(*p);
        }
    }
    while out.len() > 1 && out[0].distance(out[out.len() - 1]) <= eps {
        out.pop();
    }
    out
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

/// The parcel inset by `inset`, floored with `s` at ground level: the yard
/// nearly every industrial, transport and civic style starts from.
pub fn yard(g: &mut Geometry, c: &Ctx, inset: f32, s: Surface, color: [f32; 3]) -> Vec<Vec2> {
    let poly = offset_poly(&c.lot_poly, inset);
    ground(g, &c.frame, s, &poly, 0.03, color);
    poly
}

/// One quad on a face: `w` along it, `h` high, centred `t` from the middle
/// and `d` proud of the wall, facing the way the face does. Two triangles,
/// four vertices, for every pane, frame, sign letter, LED cell and stain
/// that a closed box used to spend twelve on.
#[allow(clippy::too_many_arguments)]
pub fn decal(g: &mut Geometry, f: &Frame, s: Surface, face: &Face, t: f32, y: f32, d: f32, w: f32, h: f32, color: [f32; 3]) {
    if w <= 0.0 || h <= 0.0 {
        return;
    }
    let centre = f.at2(face.at(t, d), y);
    let along = f.dir3(face.tangent);
    let out = f.dir3(face.out);
    g.part(s).decal(centre, along, Vec3::Y, w, h, out, rgba(color));
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
        let p = if p.iter().all(|q| c.inside_local(*q)) { p } else { fit_inside(c, &p) };
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
            face.decal(g, f, Matte, t, y, 0.03, w + 0.24, spec.h + 0.24, spec.frame);
            let r: f32 = rng.random();
            if r < spec.boarded {
                face.decal(g, f, Wood, t, y, 0.05, w, spec.h, c.palette.plywood);
            } else if coin(rng, spec.lit) {
                let wc = pick(rng, &c.style.window_colors);
                let col = mul(wc, rng.random_range(0.55..1.0));
                window_light(g, c, |g| face.decal(g, f, Glow, t, y, 0.05, w, spec.h, col));
            } else {
                face.decal(g, f, Glass, t, y, 0.05, w, spec.h, c.palette.glass_dark);
            }
            if coin(rng, spec.ac) {
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
        face.decal(g, f, Glass, 0.0, y, 0.03, len, fh * 0.46, c.palette.glass_dark);
        face.bx(g, f, Matte, 0.0, y - fh * 0.26, 0.05, Vec3::new(len, 0.12, 0.12), c.style.trim_color);
        for i in 0..segs {
            if !coin(rng, lit) {
                continue;
            }
            let t = -len * 0.5 + (i as f32 + 0.5) * seg;
            let wc = pick(rng, &c.style.window_colors);
            let col = mul(wc, rng.random_range(0.5..0.95));
            window_light(g, c, |g| face.decal(g, f, Glow, t, y, 0.05, seg * 0.9, fh * 0.38, col));
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
                if !coin(rng, lit) {
                    continue;
                }
                let t = -face.len * 0.5 + (i as f32 + 0.5) * seg;
                let wc = pick(rng, &c.style.window_colors);
                let col = mul(wc, rng.random_range(0.35..0.8));
                window_light(g, c, |g| face.decal(g, f, Glow, t, y, 0.05, seg * 0.85, fh * 0.42, col));
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
        let shuttered = coin(rng, c.wear * 0.7);
        if shuttered {
            face.decal(g, f, Metal, t, y0 + h * 0.45, 0.06, unit * 0.9, h * 0.9, mul(pick(rng, &c.palette.metal_paint), 0.8));
            grime(g, c, rng, face, y0, y0 + h, 2.0);
        } else {
            face.decal(g, f, Glass, t, y0 + h * 0.45, 0.04, unit * 0.9, h * 0.8, c.palette.glass_dark);
            if coin(rng, 0.5 + 0.4 * c.lit_chance()) {
                let wc = pick(rng, &c.palette.glow_colors);
                window_light(g, c, |g| face.decal(g, f, Glow, t, y0 + h * 0.45, 0.06, unit * 0.86, h * 0.72, mul(wc, 0.6)));
            }
            // door
            face.decal(g, f, Matte, t + unit * 0.3, y0 + 1.1, 0.08, 1.0, 2.2, DARK);
        }
        // fascia and sign
        face.bx(g, f, Matte, t, y0 + h * 0.93, 0.12, Vec3::new(unit * 0.96, h * 0.14, 0.2), mul(c.style.trim_color, 0.8));
        if coin(rng, c.rules.sign_chance) {
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
    face.decal(g, f, Metal, t, h * 0.5, 0.05, w, h, color);
    face.bx(g, f, Matte, t, h + 0.15, 0.08, Vec3::new(w + 0.4, 0.3, 0.14), CHARCOAL);
    face.bx(g, f, Glow, t, h + 0.5, 0.25, Vec3::new(0.5, 0.18, 0.3), c.palette.street_lamp);
}

/// Recessed doorway with glass doors, a canopy and lamps.
pub fn entrance(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, w: f32, h: f32) {
    let f = &c.frame;
    face.bx(g, f, Matte, t, h * 0.5, -0.3, Vec3::new(w + 0.6, h, 0.8), DARK);
    face.decal(g, f, Glass, t, h * 0.5, 0.12, w, h * 0.95, c.palette.glass_dark);
    let lit = coin(rng, 0.6 + 0.4 * c.lit_chance());
    if lit {
        let col = mul(pick(rng, &c.style.window_colors), 0.7);
        window_light(g, c, |g| face.decal(g, f, Glow, t, h * 0.5, 0.14, w * 0.9, h * 0.85, col));
    }
    canopy(g, c, face, t, h + 0.3, w + 1.6, 2.0, mul(c.style.trim_color, 0.9));
    // Doorstep paving, as deep as the forecourt allows.
    let step = (2.2f32).min(c.reach_out(face) - 0.2);
    if step > 0.4 {
        face.bx(g, f, Matte, t, 0.08, step * 0.5, Vec3::new(w + 1.2, 0.16, step), mul(c.palette.paving, 1.1));
    }
}

/// Canopy over a face, `depth` out from it but never past the lot edge.
pub fn canopy(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, w: f32, depth: f32, color: [f32; 3]) {
    let f = &c.frame;
    let depth = depth.min(c.reach_out(face) - 0.2);
    if depth < 0.5 {
        return;
    }
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
        if coin(rng, 0.15 + 0.5 * c.wear) && i % 3 == 1 {
            continue; // dead letters
        }
        let lt = t - w * 0.5 + (i as f32 + 0.5) * lw;
        face.decal(g, f, Glow, lt, y, 0.23, lw * 0.7, h * 0.7, mul(sc, k));
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
        if coin(rng, c.wear * 0.4) {
            continue;
        }
        let y = y0 + (i as f32 + 0.5) * (h / cells as f32);
        face.decal(g, f, Glow, t, y, 0.96, w * 0.7, (h / cells as f32) * 0.7, mul(sc, 0.9));
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
            if coin(rng, c.wear * 0.5) {
                continue;
            }
            let tt = ((i as f32 / cols as f32 + j as f32 / rows as f32) * 0.5 + rng.random_range(-0.2..0.2)).clamp(0.0, 1.0);
            let col = mix(a, b, tt);
            let k = rng.random_range(0.6..1.1);
            face.decal(g, f, Glow, t - w * 0.5 + (i as f32 + 0.5) * cw, y - h * 0.5 + (j as f32 + 0.5) * ch, 0.37, cw * 0.96, ch * 0.96, mul(col, k));
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
        face.decal(g, f, Rust, t, y1 - sh * 0.5, 0.02, rng.random_range(0.3..1.2), sh, c.palette.rust_stain);
    }
    // A run-down building gets tagged at street level whatever its style.
    if c.wear > 0.5 && y0 < 2.5 {
        graffiti(g, c, rng, face, y0, (y0 + 3.0).min(y1), (amount * 0.5).clamp(0.6, 2.5));
    }
}

/// Vertical downpipes with a rust patch where they leak.
pub fn downpipes(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y_top: f32) {
    let f = &c.frame;
    let n = (face.len / 9.0).floor() as i32 + 1;
    for i in 0..n {
        let t = -face.len * 0.5 + 0.6 + i as f32 * (face.len - 1.2) / n.max(1) as f32;
        face.bx(g, f, Metal, t, y_top * 0.5, 0.12, Vec3::new(0.14, y_top, 0.14), mul(c.style.trim_color, 0.7));
        if coin(rng, c.wear) {
            face.decal(g, f, Rust, t, y_top * 0.3, 0.03, 0.5, y_top * 0.4, c.palette.rust_stain);
        }
    }
}

/// Zigzag fire escape down a face: landings per floor, diagonal stair runs.
#[allow(clippy::too_many_arguments)]
pub fn fire_escape(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, y0: f32, floors: i32, fh: f32) {
    let f = &c.frame;
    let steel = mul(c.palette.steel, 0.4 + 0.2 * rng.random::<f32>());
    // Rusted through once the building is run down.
    let (s, steel) = outdoor_steel(c, steel);
    let rail = if s == Rust { Rust } else { Matte };
    let w = 4.0;
    let d = 1.1;
    for k in 1..floors {
        let y = y0 + k as f32 * fh;
        face.bx(g, f, s, t, y, d * 0.5, Vec3::new(w, 0.08, d), steel);
        face.bx(g, f, rail, t, y + 0.5, d - 0.03, Vec3::new(w, 0.04, 0.04), steel);
        face.bx(g, f, rail, t, y + 1.0, d - 0.03, Vec3::new(w, 0.04, 0.04), steel);
        for side in [-1.0f32, 1.0] {
            face.bx(g, f, rail, t + side * w * 0.5, y + 0.5, d - 0.03, Vec3::new(0.05, 1.0, 0.05), steel);
        }
        // stair run to the floor below, alternating direction
        let dir = if k % 2 == 0 { 1.0 } else { -1.0 };
        let a = f.at2(face.at(t - dir * w * 0.45, d * 0.5), y);
        let b = f.at2(face.at(t + dir * w * 0.45, d * 0.5), y - fh + 0.1);
        g.part(s).beam(a, b, 0.5, rgba(steel));
    }
    // ladder down from the first landing
    face.bx(g, f, rail, t + w * 0.4, y0 + fh * 0.6, d * 0.5, Vec3::new(0.5, fh * 0.9, 0.05), steel);
}

#[allow(clippy::too_many_arguments)]
pub fn balcony(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, w: f32, depth: f32, slab: [f32; 3], rail: [f32; 3]) {
    let f = &c.frame;
    face.bx(g, f, Concrete, t, y, depth * 0.5, Vec3::new(w, 0.22, depth), slab);
    // Rail panels are decals: the front faces the street, each side faces
    // along the wall. Nobody stands on the balcony to see their backs, and a
    // tower has hundreds of them.
    face.decal(g, f, Matte, t, y + 0.6, depth - 0.05, w, 1.0, rail);
    let along = f.dir3(face.tangent);
    let out = f.dir3(face.out);
    for side in [-1.0f32, 1.0] {
        let centre = f.at2(face.at(t + side * (w * 0.5 - 0.03), depth * 0.5), y + 0.6);
        g.part(Matte).decal(centre, out, Vec3::Y, depth, 1.0, along * side, rgba(rail));
    }
}

pub fn ac_unit(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y: f32, d: f32) {
    let f = &c.frame;
    face.bx(g, f, Metal, t, y, d, Vec3::new(0.8, 0.6, 0.35), mul(c.palette.steel, 0.9));
    face.decal(g, f, Matte, t, y, d + 0.18, 0.5, 0.5, DARK);
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
            2 => {
                let (s, col) = if rng.random_bool(0.5) { (Rust, c.palette.rust) } else { (Metal, mul(c.palette.steel, 0.8)) };
                water_tank(g, f, p, y, rng.random_range(0.9..1.4), rng.random_range(1.6..2.6), s, col)
            }
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

/// Rooftop tank on four legs. `s` is the drum's surface (rusted or painted
/// steel), passed explicitly rather than guessed from the colour.
#[allow(clippy::too_many_arguments)]
pub fn water_tank(g: &mut Geometry, f: &Frame, p: Vec2, y: f32, r: f32, h: f32, s: Surface, color: [f32; 3]) {
    let w = f.to_world(p.x, p.y);
    let legs = 1.0;
    for (du, dv) in [(-0.6f32, -0.6f32), (0.6, -0.6), (0.6, 0.6), (-0.6, 0.6)] {
        f.bx(g, Matte, p.x + du * r, y + legs * 0.5, p.y + dv * r, Vec3::new(0.12, legs, 0.12), CHARCOAL);
    }
    g.part(s).cylinder(w, r, r, y + legs, y + legs + h, 12, rgba(color));
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
    g.part(Concrete).cylinder(w, r, r, y, y + 0.3, 16, rgba(mul(c.palette.concrete(0), 0.8)));
    g.part(Glow).frustum(&MeshData::ngon(w, r * 0.92, 16, 0.0, y + 0.31), &MeshData::ngon(w, r * 0.92, 16, 0.0, y + 0.36), rgba(mul(c.palette.lamp, 0.4)));
    f.bx(g, Glow, p.x, y + 0.4, p.y, Vec3::new(r * 0.9, 0.08, 0.3), mul(c.palette.lamp, 0.9));
    f.bx(g, Glow, p.x - r * 0.35, y + 0.4, p.y, Vec3::new(0.3, 0.08, r * 0.8), mul(c.palette.lamp, 0.9));
    f.bx(g, Glow, p.x + r * 0.35, y + 0.4, p.y, Vec3::new(0.3, 0.08, r * 0.8), mul(c.palette.lamp, 0.9));
    g.pads.push(Vec3::new(w.x, y + 0.36, w.y));
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

/// The one tree family. `Round` is the street and garden tree; the others
/// come from parks and open ground.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TreeKind {
    /// Trunk with a two-tier faceted canopy.
    Round,
    /// Three stacked cones.
    Conifer,
    /// Leaning trunk with a crown of fronds.
    Palm,
    /// Bare tapering trunk with a few branches, no canopy.
    Dead,
}

/// A round tree on the ground at a world point.
pub fn tree_world(g: &mut Geometry, w: Vec2, h: f32, canopy: f32, bark: [f32; 3], leaf: [f32; 3]) {
    tree_kind(g, w, 0.0, TreeKind::Round, h, canopy, bark, leaf);
}

/// Any tree, standing on ground at height `y` at a world point. Branch and
/// lean directions come from the position so no RNG is consumed: callers
/// that place thousands of trees keep their streams stable.
#[allow(clippy::too_many_arguments)]
pub fn tree_kind(g: &mut Geometry, w: Vec2, y: f32, kind: TreeKind, h: f32, canopy: f32, bark: [f32; 3], leaf: [f32; 3]) {
    use std::f32::consts::TAU;
    match kind {
        TreeKind::Round => {
            g.part(Wood).cylinder(w, 0.22, 0.16, y, y + h, 7, rgba(bark));
            let c = g.part(Foliage);
            c.frustum(&MeshData::ngon(w, canopy * 0.5, 7, 0.0, y + h), &MeshData::ngon(w, canopy * 0.2, 7, 0.4, y + h + canopy * 0.8), rgba(leaf));
            c.frustum(&MeshData::ngon(w, canopy * 0.35, 7, 0.3, y + h - canopy * 0.3), &MeshData::ngon(w, canopy * 0.5, 7, 0.0, y + h), rgba(mul(leaf, 0.85)));
        }
        TreeKind::Conifer => {
            g.part(Wood).cylinder(w, 0.22, 0.12, y, y + h * 0.3, 6, rgba(bark));
            for k in 0..3 {
                let y0 = y + h * (0.25 + 0.22 * k as f32);
                let rr = canopy * (1.0 - 0.22 * k as f32);
                g.part(Foliage).frustum(&MeshData::ngon(w, rr, 8, 0.3 * k as f32, y0), &MeshData::ngon(w, rr * 0.12, 8, 0.3 * k as f32, y0 + h * 0.32), rgba(mul(leaf, 1.0 - 0.06 * k as f32)));
            }
        }
        TreeKind::Palm => {
            let lean = Vec2::new(w.y * 0.013, w.x * 0.011).normalize_or_zero() * 0.9;
            let top = Vec3::new(w.x + lean.x, y + h, w.y + lean.y);
            let mid = Vec3::new(w.x + lean.x * 0.5, y + h * 0.55, w.y + lean.y * 0.5);
            g.part(Wood).beam(Vec3::new(w.x, y, w.y), mid, 0.34, rgba(bark));
            g.part(Wood).beam(mid, top, 0.26, rgba(bark));
            for k in 0..7 {
                let a = k as f32 / 7.0 * TAU;
                let dir = Vec3::new(a.cos(), 0.35, a.sin()).normalize();
                let m = top + dir * 1.6;
                let tip = m + Vec3::new(dir.x, -0.5, dir.z).normalize() * 1.8;
                g.part(Foliage).beam(top, m, 0.5, rgba(leaf));
                g.part(Foliage).beam(m, tip, 0.32, rgba(mul(leaf, 0.9)));
            }
            g.part(Foliage).cylinder(Vec2::new(top.x, top.z), 0.5, 0.5, y + h - 0.4, y + h + 0.3, 6, rgba(mul(leaf, 0.8)));
        }
        TreeKind::Dead => {
            let wood = g.part(Wood);
            wood.cylinder(w, 0.18, 0.07, y - 0.1, y + h, 5, rgba(bark));
            let n = 2 + ((w.x * 0.37 + w.y * 0.61).abs() as i32 % 2);
            for k in 0..n {
                let a = k as f32 * 2.1 + w.x * 0.37 + w.y * 0.13;
                let len = (h * 0.3).clamp(0.8, 1.8);
                let from = Vec3::new(w.x, y + h * (0.55 + 0.12 * k as f32), w.y);
                let to = from + Vec3::new(a.cos() * len, len * 0.7, a.sin() * len);
                wood.beam(from, to, 0.08, rgba(bark));
            }
        }
    }
}

pub fn lamp_post(g: &mut Geometry, f: &Frame, p: Vec2, h: f32, glow: [f32; 3]) {
    lamp_post_world(g, f.to_world(p.x, p.y), h, glow);
}

/// Lamp post on the lot, skipped when the point is off it or so close to
/// the edge that the lantern hangs over the street.
pub fn lamp_post_in(g: &mut Geometry, c: &Ctx, p: Vec2, h: f32, glow: [f32; 3]) {
    if c.inside_local(p) && distance_to_boundary(p, &c.lot_poly) > 0.35 {
        lamp_post(g, &c.frame, p, h, glow);
    }
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
    // mesh panel: thin, dark, slightly translucent-looking grey; a box, not
    // a decal, because a fence is seen from both sides
    face.bx(g, f, Matte, 0.0, h * 0.5, 0.0, Vec3::new(len, h * 0.96, 0.015), c.palette.fence_mesh);
    if barbed {
        let wa = f.at2(a, h + 0.2);
        let wb = f.at2(b, h + 0.2);
        g.part(Rust).tube(wa, wb, 0.12, 5, rgba(c.palette.rust));
    }
}

/// Fence round a yard with the gate on the face that looks at the road:
/// the perimeter every depot, plant and compound shares.
pub fn fence_lot(g: &mut Geometry, c: &Ctx, poly: &[Vec2], h: f32, barbed: bool) {
    fence_poly(g, c, poly, h, c.front_face_index(poly), barbed);
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

/// Bollards `d` out from a face, pulled back to the lot edge when the
/// forecourt is shallower than asked for.
pub fn bollards(g: &mut Geometry, c: &Ctx, face: &Face, n: i32, d: f32) {
    let f = &c.frame;
    let d = d.min(c.reach_out(face) - 0.4);
    if d < -c.fd {
        return;
    }
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

/// A container's paint as it looks after years at sea: the line colour
/// jittered in brightness and hue, and on some units bleached by the sun
/// toward a chalky, desaturated version of itself.
pub fn container_paint(rng: &mut impl Rng, base: [f32; 3], wear: f32) -> [f32; 3] {
    let k = rng.random_range(0.86..1.1);
    let mut c = [0usize, 1, 2].map(|i| base[i] * k * rng.random_range(0.93..1.07));
    if coin(rng, 0.15 + 0.35 * wear) {
        let lum = 0.3 * c[0] + 0.55 * c[1] + 0.15 * c[2];
        let chalk = [lum * 1.25 + 0.07, lum * 1.22 + 0.07, lum * 1.18 + 0.07];
        c = mix(c, chalk, rng.random_range(0.25..0.6));
    }
    c
}

/// Gradient decal: `top` at its upper edge fading to `bottom` at its lower
/// one. Rust running down from a rail, a stain under a casting.
#[allow(clippy::too_many_arguments)]
fn streak_decal(m: &mut MeshData, centre: Vec3, along: Vec3, w: f32, h: f32, facing: Vec3, top: [f32; 3], bottom: [f32; 3]) {
    let n0 = m.positions.len();
    m.decal(centre, along, Vec3::Y, w, h, facing, rgba(top));
    for i in n0..m.positions.len() {
        if m.positions[i][1] < centre.y {
            m.colors[i] = rgba(bottom);
        }
    }
}

/// Four side quads of a box, open top and bottom: rails, flanges, sleeves.
/// `corner(x, y, z)` maps unit corners (x, z in -1..1) at height y.
fn box_sides(m: &mut MeshData, corner: impl Fn(f32, f32, f32) -> Vec3, ya: f32, yb: f32, color: [f32; 4]) {
    let centre = (corner(-1.0, ya, -1.0) + corner(1.0, yb, 1.0)) * 0.5;
    for (a, b) in [((-1.0f32, -1.0f32), (1.0f32, -1.0f32)), ((1.0, -1.0), (1.0, 1.0)), ((1.0, 1.0), (-1.0, 1.0)), ((-1.0, 1.0), (-1.0, -1.0))] {
        let q = [corner(a.0, ya, a.1), corner(b.0, ya, b.1), corner(b.0, yb, b.1), corner(a.0, yb, a.1)];
        let mid = (q[0] + q[2]) * 0.5;
        let out = Vec3::new(mid.x - centre.x, 0.0, mid.z - centre.z);
        m.quad_facing(q, out, color);
    }
}

/// Most triangles one container may spend, details and wear included.
pub const CONTAINER_TRIS: usize = 70;

/// An ISO shipping container placed in parcel coordinates: corrugated
/// walls and roof (the `Container` sheet), top and bottom side rails, corner
/// posts with castings, a door end with hinge posts, four locking bars and
/// the door seam, and a front wall recessed between its posts at the other
/// end. Each unit is painted its own way (see [`container_paint`]) and may
/// carry a shipping-line band or logo block, an ID marking, rust running
/// down from the top rail and castings, dents and repainted patches, more
/// of them the more worn the place is. Everything flat is a decal: a unit
/// stays within [`CONTAINER_TRIS`].
#[allow(clippy::too_many_arguments)]
pub fn container_unit(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, u: f32, v: f32, yaw: f32, len: f32, wid: f32, h: f32, y0: f32, color: [f32; 3]) {
    let f = &c.frame;
    let pal = c.palette;
    let (s, co) = yaw.sin_cos();
    let at = |lu: f32, y: f32, lv: f32| f.at2(Vec2::new(u + lu * co - lv * s, v + lu * s + lv * co), y);
    let long = f.dir3(Vec2::new(co, s));
    let across = f.dir3(Vec2::new(-s, co));
    let (hl, hw) = (len * 0.5, wid * 0.5);
    let top = y0 + h;
    let (rail_b, rail_t) = (0.16f32.min(h * 0.1), 0.14f32.min(h * 0.1));
    let (wall_a, wall_b) = (y0 + rail_b, top - rail_t);
    let wall_mid = (wall_a + wall_b) * 0.5;
    let wall_h = wall_b - wall_a;
    // Rails stand proud of the sheet, posts proud of the rails.
    let (rail_out, post_out) = (0.025, 0.045);
    let tier_wear = if c.tier == nd_sim::Tier::Low { 0.2 } else { 0.0 };
    let wear = (c.wear * 0.75 + tier_wear + rng.random_range(-0.1..0.2)).clamp(0.0, 1.0);
    let paint = container_paint(rng, color, wear);
    let frame_col = mul(paint, 0.72);
    let door = if rng.random_bool(0.5) { -1.0f32 } else { 1.0 };

    // Corrugated sheet: both sides, the door leaves, the recessed front
    // wall and the roof.
    {
        let m = g.part(Container);
        for side in [-1.0f32, 1.0] {
            let q = [at(-hl, wall_a, side * hw), at(hl, wall_a, side * hw), at(hl, wall_b, side * hw), at(-hl, wall_b, side * hw)];
            m.quad_facing(q, across * side, rgba(paint));
        }
        for (end, inset) in [(door, 0.0f32), (-door, 0.08)] {
            let x = end * (hl - inset);
            let q = [at(x, wall_a, -hw), at(x, wall_a, hw), at(x, wall_b, hw), at(x, wall_b, -hw)];
            m.quad_facing(q, long * end, rgba(if inset > 0.0 { mul(paint, 0.9) } else { paint }));
        }
        let (rl, rw) = (hl + rail_out, hw + rail_out);
        m.quad_facing([at(-rl, top, -rw), at(rl, top, -rw), at(rl, top, rw), at(-rl, top, rw)], Vec3::Y, rgba(mul(paint, 0.94)));
    }
    // Top and bottom rails all round (headers and sills at the ends), and
    // the underside where the unit is off the ground.
    {
        let m = g.part(Matte);
        let (rl, rw) = (hl + rail_out, hw + rail_out);
        let corner = |a: f32, y: f32, b: f32| at(a * rl, y, b * rw);
        box_sides(m, corner, wall_b, top, rgba(frame_col));
        box_sides(m, corner, y0, wall_a, rgba(mul(frame_col, 0.85)));
        if y0 > 0.05 {
            m.quad_facing([at(-rl, y0, -rw), at(-rl, y0, rw), at(rl, y0, rw), at(rl, y0, -rw)], Vec3::NEG_Y, rgba(mul(frame_col, 0.5)));
        }
        // Corner posts on the long sides, with a casting at the top of each.
        for side in [-1.0f32, 1.0] {
            for end in [-1.0f32, 1.0] {
                m.decal(at(end * (hl - 0.09), (y0 + top) * 0.5, side * (hw + post_out)), long, Vec3::Y, 0.18, h, across * side, rgba(frame_col));
                m.decal(at(end * (hl - 0.09), top - 0.09, side * (hw + post_out + 0.006)), long, Vec3::Y, 0.2, 0.18, across * side, rgba(mul(frame_col, 0.75)));
            }
        }
        // Front end: two posts framing the recessed wall.
        for side in [-1.0f32, 1.0] {
            m.decal(at(-door * (hl + post_out), (y0 + top) * 0.5, side * (hw - 0.09)), across, Vec3::Y, 0.18, h, long * -door, rgba(frame_col));
        }
        // Door end: hinge posts at the outer edges, four locking bars, the
        // seam between the leaves.
        let dx = door * (hl + post_out);
        for side in [-1.0f32, 1.0] {
            m.decal(at(dx, (y0 + top) * 0.5, side * (hw - 0.11)), across, Vec3::Y, 0.22, h, long * door, rgba(mul(frame_col, 0.8)));
            for k in [0.3f32, 0.72] {
                m.decal(at(door * (hl + 0.03), wall_mid, side * hw * k), across, Vec3::Y, 0.05, wall_h + 0.1, long * door, rgba(mix(mul(paint, 0.6), pal.steel, 0.25)));
            }
        }
        m.decal(at(door * (hl + 0.02), wall_mid, 0.0), across, Vec3::Y, 0.035, wall_h, long * door, rgba(DARK));
        // ID marking stencilled on one leaf.
        m.decal(at(door * (hl + 0.025), wall_b - 0.32, -door * hw * 0.5), across, Vec3::Y, 0.7, 0.18, long * door, rgba(mix(pal.chalk, paint, 0.25)));
    }
    // Livery and wear: at most three more decals per unit.
    let mut extras = 0;
    let side_decal = |g: &mut Geometry, side: f32, lu: f32, y: f32, off: f32, w: f32, hh: f32, col: [f32; 3]| {
        g.part(Container).decal(at(lu, y, side * (hw + off)), long, Vec3::Y, w, hh, across * side, rgba(col));
    };
    let marks = &pal.container_marks;
    if coin(rng, 0.4 * (1.0 - 0.4 * wear)) && !marks.is_empty() {
        // A contrasting colour from the line's livery list.
        let lum = |c: [f32; 3]| 0.3 * c[0] + 0.55 * c[1] + 0.15 * c[2];
        let mut mark = pick(rng, marks);
        if (lum(mark) - lum(paint)).abs() < 0.12 {
            mark = pick(rng, marks);
        }
        let mark = mix(mark, paint, 0.15 * wear);
        let side = if rng.random_bool(0.5) { 1.0f32 } else { -1.0 };
        if rng.random_bool(0.5) {
            // A band along the upper part of the side.
            let bh = rng.random_range(0.25..0.5f32).min(wall_h * 0.25);
            side_decal(g, side, 0.0, wall_b - bh * 0.5 - rng.random_range(0.1..0.5), 0.01, len - 0.6, bh, mark);
        } else {
            // A logo block towards one end.
            let (bw, bh) = (rng.random_range(1.0..2.2f32).min(len * 0.4), rng.random_range(0.5..1.0f32).min(wall_h * 0.5));
            let lu = rng.random_range(-1.0..1.0f32) * (hl - bw * 0.5 - 0.4).max(0.0);
            side_decal(g, side, lu, wall_mid + rng.random_range(0.0..0.3), 0.01, bw, bh, mark);
        }
        extras += 1;
    }
    if coin(rng, 0.1 + 0.25 * wear) && extras < 3 {
        // A repainted patch in not-quite-matching paint, or bare primer.
        let col = if rng.random_bool(0.7) { container_paint(rng, paint, 0.0) } else { mix([0.42, 0.40, 0.37], paint, 0.2) };
        let (pw, ph) = (rng.random_range(0.6..2.4f32).min(len * 0.4), rng.random_range(0.5..1.6f32).min(wall_h * 0.8));
        let lu = rng.random_range(-1.0..1.0f32) * (hl - pw * 0.5 - 0.3).max(0.0);
        let side = if rng.random_bool(0.5) { 1.0f32 } else { -1.0 };
        let y = wall_a + ph * 0.5 + rng.random_range(0.0..(wall_h - ph).max(0.01));
        side_decal(g, side, lu, y, 0.013, pw, ph, col);
        extras += 1;
    }
    if coin(rng, 0.15 + 0.4 * wear) && extras < 3 {
        // A dent from a twistlock or a forklift: a darker patch low down.
        let (dw, dh) = (rng.random_range(0.3..0.9f32), rng.random_range(0.2..0.6f32));
        let lu = rng.random_range(-1.0..1.0f32) * (hl - dw * 0.5 - 0.3).max(0.0);
        let side = if rng.random_bool(0.5) { 1.0f32 } else { -1.0 };
        side_decal(g, side, lu, wall_a + dh * 0.5 + rng.random_range(0.0..0.6), 0.016, dw, dh, mul(paint, 0.68));
        extras += 1;
    }
    // Rust bleeding down from the top rail and out of the castings.
    let streaks = ((wear * 2.6 + rng.random_range(0.0..0.8)) as usize).min(3 - extras);
    for _ in 0..streaks {
        let side = if rng.random_bool(0.5) { 1.0f32 } else { -1.0 };
        let at_casting = rng.random_bool(0.4);
        let sw = if at_casting { rng.random_range(0.2..0.4f32) } else { rng.random_range(0.12..0.5f32) };
        let sh = rng.random_range(0.4..1.0f32) * wall_h * (0.4 + 0.6 * wear);
        let lu = if at_casting { (hl - 0.32) * if rng.random_bool(0.5) { 1.0 } else { -1.0 } } else { rng.random_range(-1.0..1.0f32) * (hl - 0.5).max(0.0) };
        let rust = mix(pal.rust_stain, pal.rust, rng.random_range(0.0..0.6));
        streak_decal(g.part(Container), at(lu, wall_b - sh * 0.5, side * (hw + 0.02)), long, sw, sh, across * side, rust, mix(rust, paint, 0.85));
    }
}

/// A steel flat bar from `a` to `b`, `width` wide in the plane whose
/// normal is `n`: a cross brace. Two faces, no edges.
pub fn flat_bar(m: &mut MeshData, a: Vec3, b: Vec3, width: f32, n: Vec3, color: [f32; 3]) {
    let side = (b - a).cross(n).normalize_or_zero() * (width * 0.5);
    if side == Vec3::ZERO {
        return;
    }
    let q = [a - side, b - side, b + side, a + side];
    m.quad_facing(q, n, rgba(color));
    m.quad_facing(q, -n, rgba(mul(color, 0.8)));
}

/// Section of the I-beam props: flange width and section depth 0.2 m.
pub const IBEAM_FLANGE: f32 = 0.2;

/// A steel I-beam prop: two flanges and a web from `base` to `top`, at
/// lot-local `p`, flanges parallel to `yaw` (relative to the frame). On
/// the ground it stands on a base plate on a small concrete pad; on a roof,
/// on the plate alone. A cap plate takes the load at the top. Rust-brown,
/// each one its own shade; 22 triangles, 24 with the pad.
#[allow(clippy::too_many_arguments)]
pub fn ibeam_support(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, p: Vec2, yaw: f32, base: f32, top: f32, on_ground: bool) {
    let f = &c.frame;
    let pal = c.palette;
    let a = f.yaw() + yaw;
    let w = f.to_world(p.x, p.y);
    let (s, co) = a.sin_cos();
    let at = |x: f32, y: f32, z: f32| Vec3::new(w.x + x * co - z * s, y, w.y + x * s + z * co);
    let rust = mul(mix(pal.rust, pal.rust_stain, rng.random_range(0.0..0.7)), rng.random_range(0.8..1.1));
    let plate = mul(pal.rust_stain, 0.8);
    let mut y = base;
    if on_ground {
        // A pad cast flush with the yard, just proud of the dirt.
        y += 0.07;
        g.part(Concrete).quad_facing([at(-0.3, y, -0.3), at(0.3, y, -0.3), at(0.3, y, 0.3), at(-0.3, y, 0.3)], Vec3::Y, rgba(mul(pal.concrete(0), rng.random_range(0.7..0.9))));
    }
    // Base plate: a thin sheet, its top is all that shows.
    y += 0.02;
    g.part(Matte).quad_facing([at(-0.18, y, -0.18), at(0.18, y, -0.18), at(0.18, y, 0.18), at(-0.18, y, 0.18)], Vec3::Y, rgba(plate));
    // Cap plate bolted under the container: its top is hidden against the
    // floor, its underside only seen from below, so just the edges.
    let y1 = top - 0.05;
    box_sides(g.part(Matte), |x, yy, z| at(x * 0.17, yy, z * 0.17), y1, top, rgba(plate));
    if y1 <= y {
        return;
    }
    // Section: flanges 0.025 thick, a 0.018 web.
    let (b, d, tf, tw) = (IBEAM_FLANGE, IBEAM_FLANGE, 0.025f32, 0.018f32);
    // Each flange is its outer and inner face (its 25 mm edges are a pixel
    // at street distance), the web its two faces: six quads.
    let m = g.part(Rust);
    let across = Vec3::new(-s, 0.0, co);
    for side in [-1.0f32, 1.0] {
        for (z, face) in [(side * d * 0.5, side), (side * (d * 0.5 - tf), -side)] {
            let col = if face == side { rust } else { mul(rust, 0.75) };
            m.quad_facing([at(-b * 0.5, y, z), at(b * 0.5, y, z), at(b * 0.5, y1, z), at(-b * 0.5, y1, z)], across * face, rgba(col));
        }
    }
    for side in [-1.0f32, 1.0] {
        let x = side * tw * 0.5;
        let z = d * 0.5 - tf;
        m.quad_facing([at(x, y, -z), at(x, y, z), at(x, y1, z), at(x, y1, -z)], Vec3::new(co, 0.0, s) * side, rgba(mul(rust, 0.85)));
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

impl Vehicle {
    /// Footprint: half-length and half-width, plus how far its centre sits
    /// ahead of the anchor point.
    fn footprint(self) -> (f32, f32, f32) {
        match self {
            Vehicle::Car => (2.1, 0.95, 0.0),
            Vehicle::Van => (2.6, 1.0, 0.0),
            Vehicle::Truck => (4.9, 1.2, -0.3),
            Vehicle::Wreck => (2.0, 0.9, 0.0),
            Vehicle::Bus => (5.75, 1.25, 0.0),
        }
    }
}

/// Whether a vehicle at `p` facing `yaw` (relative to the frame) stands
/// wholly on the lot. Checking only the centre parked lorries in the road.
pub fn vehicle_fits(c: &Ctx, p: Vec2, yaw: f32, kind: Vehicle) -> bool {
    let (hl, hw, ahead) = kind.footprint();
    let d = Vec2::new(yaw.cos(), yaw.sin());
    let n = Vec2::new(-d.y, d.x);
    let centre = p + d * ahead;
    [centre + d * hl + n * hw, centre + d * hl - n * hw, centre - d * hl + n * hw, centre - d * hl - n * hw, centre].iter().all(|q| c.inside_local(*q))
}

/// A vehicle standing on a surface at height `y` (a deck, the ground),
/// facing `yaw` relative to the frame. Parking decks, depots and yards all
/// draw their cars through here; the generator is `crate::vehicles`.
#[allow(clippy::too_many_arguments)]
pub fn vehicle(g: &mut Geometry, f: &Frame, pal: &Palette, rng: &mut impl Rng, p: Vec2, y: f32, yaw: f32, kind: Vehicle, color: [f32; 3]) {
    use crate::vehicles::Parked;
    let kind = match kind {
        Vehicle::Car => Parked::Car,
        Vehicle::Van => Parked::Van,
        Vehicle::Truck => Parked::Truck,
        Vehicle::Wreck => Parked::Wreck,
        Vehicle::Bus => Parked::Bus,
    };
    // Trucks sat a little behind their anchor (see the footprint).
    let back = if kind == Parked::Truck { -0.3 } else { 0.0 };
    let a = f.yaw() + yaw;
    let w = f.to_world(p.x, p.y) + Vec2::new(a.cos(), a.sin()) * back;
    crate::vehicles::draw_parked(g, pal, rng, w, y, a, kind, color);
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
            0 => (Rust, c.palette.rust),
            1 => (Metal, pick(rng, &c.palette.container_colors)),
            2 => (Tarp, pick(rng, &c.palette.tarp_colors)),
            _ => (Matte, DARK),
        };
        // Pieces near the edge of the heap can stick out past the fence.
        if !c.inside_local(q) || distance_to_boundary(q, &c.lot_poly) < size.x.max(size.z) * 0.75 {
            continue;
        }
        f.bx_yaw(g, s, q.x, y + size.y * 0.5, q.y, size, mul(col, 0.8), rng.random_range(0.0..3.0));
    }
    // tyre stack
    let tp = p + Vec2::new(r * 0.6, 0.0);
    if !c.inside_local(tp) || distance_to_boundary(tp, &c.lot_poly) < 0.6 {
        return;
    }
    let w = f.to_world(tp.x, tp.y);
    for k in 0..rng.random_range(2..6) {
        g.part(Matte).cylinder(w, 0.45, 0.45, k as f32 * 0.25, k as f32 * 0.25 + 0.22, 10, rgba(DARK));
    }
}

/// Market stall against a face: tarp roof, counter, a sign strip.
pub fn stall(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, t: f32, d: f32) {
    let f = &c.frame;
    // The tarp roof is 4.2 by 2.2: the whole stall has to fit on the lot.
    if [(-2.1f32, -1.1f32), (2.1, -1.1), (2.1, 1.1), (-2.1, 1.1)].iter().any(|(dt, dd)| !c.inside_local(face.at(t + dt, d + dd))) {
        return;
    }
    let tc = mul(pick(rng, &c.palette.tarp_colors), 0.9);
    face.bx(g, f, Tarp, t, 2.4, d, Vec3::new(4.2, 0.1, 2.2), tc);
    face.bx(g, f, Wood, t, 0.5, d, Vec3::new(3.6, 1.0, 1.2), c.palette.wood);
    for side in [-1.0f32, 1.0] {
        face.bx(g, f, Matte, t + side * 2.0, 1.2, d + 1.0, Vec3::new(0.08, 2.4, 0.08), CHARCOAL);
        face.bx(g, f, Matte, t + side * 2.0, 1.2, d - 1.0, Vec3::new(0.08, 2.4, 0.08), CHARCOAL);
    }
    let sc = pick(rng, &c.palette.sign_colors);
    face.bx(g, f, Glow, t, 2.1, d + 1.1, Vec3::new(3.0, 0.3, 0.06), mul(sc, 0.9 - 0.5 * c.wear));
    // goods: two crates of produce on the counter
    for i in 0..2 {
        face.bx(g, f, Tarp, t - 0.9 + i as f32 * 1.8, 1.2, d, Vec3::new(1.4, 0.4, 0.6), pick(rng, &c.palette.awning_colors));
    }
}

pub fn drum_fire(g: &mut Geometry, f: &Frame, pal: &Palette, p: Vec2) {
    let w = f.to_world(p.x, p.y);
    g.part(Rust).cylinder(w, 0.3, 0.3, 0.0, 0.9, 10, rgba(pal.rust));
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

/// Outdoor steelwork rusts once a building is run down: the surface and
/// colour for stairs, ladders and fire escapes.
pub fn outdoor_steel(c: &Ctx, color: [f32; 3]) -> (Surface, [f32; 3]) {
    // Cheap buildings let it go sooner.
    let threshold = if c.tier == nd_sim::Tier::Low { 0.2 } else { 0.4 };
    if c.wear > threshold { (Rust, mix(color, c.palette.rust, 0.55)) } else { (Metal, color) }
}

/// External zigzag stair up the side of a pile. Rusted when the lot is worn.
#[allow(clippy::too_many_arguments)]
pub fn stair_zigzag(g: &mut Geometry, c: &Ctx, u: f32, v: f32, levels: i32, rise: f32, run: f32, color: [f32; 3]) {
    let f = &c.frame;
    let (s, color) = outdoor_steel(c, color);
    let rail = if s == Rust { Rust } else { Matte };
    let mut y = 0.0;
    for l in 0..levels {
        let steps = 8;
        let dir = if l % 2 == 0 { 1.0 } else { -1.0 };
        for st in 0..steps {
            let t = (st as f32 + 0.5) / steps as f32;
            f.bx(g, s, u, y + t * rise, v + dir * (t * run - run * 0.5), Vec3::new(0.9, 0.16, 0.42), color);
        }
        f.bx(g, rail, u - 0.5, y + rise * 0.5 + 0.5, v, Vec3::new(0.06, rise + 1.0, run), color);
        f.bx(g, s, u, y + rise + 0.08, v + dir * run * 0.5, Vec3::new(1.0, 0.16, 1.0), color);
        y += rise;
    }
    f.bx(g, rail, u, y * 0.5, v + run * 0.55, Vec3::new(0.15, y, 0.15), color);
    f.bx(g, rail, u, y * 0.5, v - run * 0.55, Vec3::new(0.15, y, 0.15), color);
}

pub fn ladder(g: &mut Geometry, c: &Ctx, face: &Face, t: f32, y0: f32, y1: f32, color: [f32; 3]) {
    let f = &c.frame;
    let (s, color) = outdoor_steel(c, color);
    let s = if s == Rust { Rust } else { Matte };
    let h = y1 - y0;
    for side in [-0.25f32, 0.25] {
        face.bx(g, f, s, t + side, y0 + h * 0.5, 0.2, Vec3::new(0.05, h, 0.05), color);
    }
    let rungs = (h / 0.3) as i32;
    for k in 0..rungs {
        face.bx(g, f, s, t, y0 + (k as f32 + 0.5) * 0.3, 0.2, Vec3::new(0.5, 0.04, 0.04), color);
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
    let d = d.min(c.reach_out(face) - 0.6);
    if !c.inside_local(face.at(t, d)) {
        return;
    }
    let col = mul(c.palette.concrete(0), 0.9);
    face.bx(g, f, Concrete, t, 0.25, d, Vec3::new(w, 0.5, 0.7), col);
    face.bx(g, f, Concrete, t, 0.7, d, Vec3::new(w, 0.4, 0.35), col);
}

/// Floodlight mast, skipped when its head would hang over the lot edge.
pub fn floodlight(g: &mut Geometry, c: &Ctx, p: Vec2, h: f32) {
    let f = &c.frame;
    if !c.inside_local(p) || distance_to_boundary(p, &c.lot_poly) < 0.9 {
        return;
    }
    f.bx(g, Matte, p.x, h * 0.5, p.y, Vec3::new(0.3, h, 0.3), mul(c.palette.steel, 0.6));
    f.bx(g, Matte, p.x, h + 0.2, p.y, Vec3::new(1.6, 0.4, 0.5), CHARCOAL);
    f.bx(g, Glow, p.x, h + 0.2, p.y - 0.28, Vec3::new(1.4, 0.3, 0.06), mul(c.palette.street_lamp, 1.3));
}

/// Gantry crane spanning across the lot at `v`, running along u.
pub fn gantry_crane(g: &mut Geometry, c: &Ctx, u: f32, v0: f32, v1: f32, h: f32) {
    let f = &c.frame;
    let steel = mul(c.palette.metal_paint(0), 1.1);
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

// ── graffiti ─────────────────────────────────────────────────────────────────

/// Spray paint on a face between two heights: bubble-letter pieces with an
/// outline, quick tag strokes and the odd throw-up with drips, all thin
/// boxes a few centimetres proud of the wall at arm's reach. `amount` scales
/// how much of the face is covered; wear adds more.
pub fn graffiti(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, face: &Face, y0: f32, y1: f32, amount: f32) {
    let a = c.frame.to_world(face.a.x, face.a.y);
    let b = c.frame.to_world(face.b.x, face.b.y);
    graffiti_world(g, rng, a, b, y0, y1, amount, c.wear, c.palette);
}

/// Spray colours: dimmed LED hues, awning paints, chalk white and black.
fn spray_colour(rng: &mut impl Rng, pal: &Palette) -> [f32; 3] {
    match rng.random_range(0..7) {
        0..=2 => mul(pick(rng, &pal.led_colors), 0.32),
        3 | 4 => mul(pick(rng, &pal.awning_colors), 1.25),
        5 => pal.chalk,
        _ => DARK,
    }
}

/// World-space graffiti on the wall from `a` to `b`; the right-hand side of
/// the a→b direction is the painted side, as with [`Face::from_edge`].
#[allow(clippy::too_many_arguments)]
pub fn graffiti_world(g: &mut Geometry, rng: &mut impl Rng, a: Vec2, b: Vec2, y0: f32, y1: f32, amount: f32, wear: f32, pal: &Palette) {
    let len = a.distance(b);
    let span = y1 - y0;
    if len < 2.5 || span < 0.8 {
        return;
    }
    let tangent = (b - a) / len;
    let out = Vec2::new(tangent.y, -tangent.x);
    let along3 = Vec3::new(tangent.x, 0.0, tangent.y);
    let out3 = Vec3::new(out.x, 0.0, out.y);
    let mid = (a + b) * 0.5;
    let pieces = ((len / 7.0) * amount * (0.4 + wear) * rng.random_range(0.6..1.4)).round().clamp(0.0, 6.0) as i32;
    for _ in 0..pieces {
        let w = rng.random_range(1.4f32..3.6).min(len * 0.8);
        let h = rng.random_range(0.6f32..1.3).min(span * 0.8);
        let room = len * 0.5 - w * 0.5 - 0.3;
        let t = if room > 0.05 { rng.random_range(-room..room) } else { 0.0 };
        // Painted from the ground: the piece sits between knee and head height.
        let top = (span - h - 0.2).clamp(0.31, 2.4);
        let yc = y0 + h * 0.5 + rng.random_range(0.3..top);
        let col = spray_colour(rng, pal);
        let outline = if rng.random_bool(0.6) { DARK } else { pal.chalk };
        // Every stroke is a decal: paint has no back.
        let place = |g: &mut Geometry, dt: f32, y: f32, d: f32, size: Vec3, c: [f32; 3]| {
            let p = mid + tangent * (t + dt) + out * d;
            g.part(Matte).decal(Vec3::new(p.x, y, p.y), along3, Vec3::Y, size.x, size.y, out3, rgba(c));
        };
        match rng.random_range(0..5) {
            0 | 1 => {
                // Bubble letters: fat blocks in two colours with an outline behind.
                let n = ((w / 0.65) as i32).clamp(2, 6);
                let lw = w / n as f32;
                let second = spray_colour(rng, pal);
                for i in 0..n {
                    let lt = -w * 0.5 + (i as f32 + 0.5) * lw;
                    let lh = h * rng.random_range(0.7..1.0);
                    let dy = rng.random_range(-0.08..0.08);
                    place(g, lt, yc + dy, 0.02, Vec3::new(lw * 0.98, lh + 0.14, 0.015), outline);
                    place(g, lt, yc + dy, 0.035, Vec3::new(lw * 0.8, lh, 0.015), if i % 2 == 0 { col } else { second });
                }
            }
            2 | 3 => {
                // Tag: a few quick strokes and a flick or two.
                for _ in 0..rng.random_range(3..6) {
                    let sw = rng.random_range(0.5..w);
                    let slack = (w - sw) * 0.5;
                    let st = if slack > 0.01 { rng.random_range(-slack..slack) } else { 0.0 };
                    let sy = yc + rng.random_range(-h * 0.45..h * 0.45);
                    place(g, st, sy, 0.03, Vec3::new(sw, rng.random_range(0.07..0.16), 0.012), col);
                }
                for _ in 0..rng.random_range(1..3) {
                    let st = rng.random_range(-w * 0.45..w * 0.45);
                    place(g, st, yc, 0.03, Vec3::new(rng.random_range(0.07..0.14), h * rng.random_range(0.5..1.0), 0.012), col);
                }
            }
            _ => {
                // Throw-up: one filled block with an outline and drips below.
                place(g, 0.0, yc, 0.02, Vec3::new(w + 0.16, h + 0.16, 0.015), outline);
                place(g, 0.0, yc, 0.035, Vec3::new(w, h, 0.015), col);
                let under = yc - h * 0.5;
                for _ in 0..rng.random_range(1..4) {
                    let dt = rng.random_range(-w * 0.45..w * 0.45);
                    let dl = rng.random_range(0.2f32..0.7).min(under - y0 - 0.05).max(0.05);
                    place(g, dt, under - dl * 0.5, 0.03, Vec3::new(0.05, dl, 0.012), col);
                }
            }
        }
    }
}

// ── open ground ──────────────────────────────────────────────────────────────

/// A boulder: a faceted lump sunk a little into the ground at height `y`.
pub fn rock_world(g: &mut Geometry, rng: &mut impl Rng, p: Vec2, y: f32, r: f32, color: [f32; 3]) {
    let sides = rng.random_range(5..8);
    let rot = rng.random_range(0.0..std::f32::consts::TAU);
    let h = r * rng.random_range(0.5..1.0);
    let shift = Vec2::new(rng.random_range(-0.3..0.3), rng.random_range(-0.3..0.3)) * r;
    let bottom = MeshData::ngon(p, r, sides, rot, y - r * 0.25);
    let top = MeshData::ngon(p + shift, r * rng.random_range(0.35..0.6), sides, rot + 0.4, y + h);
    g.part(Stone).frustum(&bottom, &top, rgba(color));
}

/// A low bush: two stacked frusta that read as a rounded clump.
pub fn scrub_world(g: &mut Geometry, rng: &mut impl Rng, p: Vec2, y: f32, r: f32, color: [f32; 3]) {
    let rot = rng.random_range(0.0..std::f32::consts::TAU);
    let h = r * rng.random_range(0.6..1.1);
    let b = MeshData::ngon(p, r * 0.45, 6, rot, y);
    let m = MeshData::ngon(p, r, 6, rot + 0.3, y + h * 0.55);
    let t = MeshData::ngon(p, r * 0.35, 6, rot + 0.6, y + h);
    let f = g.part(Foliage);
    f.frustum(&b, &m, rgba(color));
    f.frustum(&m, &t, rgba(mul(color, 1.1)));
}

// ── sampling and scatter ─────────────────────────────────────────────────────

/// Random point inside a polygon at least `margin` from its edges, if one
/// turns up in a couple of dozen tries. The one rejection sampler behind
/// `Ctx::random_inside`, vacant lots, sites and yards.
pub fn random_in_poly(rng: &mut impl Rng, poly: &[Vec2], margin: f32) -> Option<Vec2> {
    let inner = if margin > 0.0 { offset_poly(poly, margin) } else { poly.to_vec() };
    if inner.len() < 3 {
        return None;
    }
    let (lo, hi) = bounds(&inner);
    if hi.x - lo.x < 0.2 || hi.y - lo.y < 0.2 {
        return None;
    }
    for _ in 0..24 {
        let p = Vec2::new(rng.random_range(lo.x..=hi.x), rng.random_range(lo.y..=hi.y));
        // offset_poly falls back to scaling when the margin is near the
        // inradius, which does not keep the margin on a long thin lot, so
        // the distance is checked as well.
        if point_in_polygon(p, &inner) && (margin <= 0.0 || distance_to_boundary(p, poly) >= margin) {
            return Some(p);
        }
    }
    None
}

/// Tufts of dry weeds scattered over a lot-local polygon.
pub fn weeds(g: &mut Geometry, c: &Ctx, rng: &mut impl Rng, poly: &[Vec2], n: i32) {
    let f = &c.frame;
    let (lo, hi) = bounds(poly);
    for _ in 0..n {
        let p = Vec2::new(rng.random_range(lo.x..=hi.x), rng.random_range(lo.y..=hi.y));
        if !point_in_polygon(p, poly) {
            continue;
        }
        let s = rng.random_range(0.4..1.1);
        f.bx_yaw(g, Foliage, p.x, s * 0.3, p.y, Vec3::new(s, s * 0.6, s * 0.8), mul(c.palette.foliage_dry, rng.random_range(0.7..1.1)), rng.random_range(0.0..1.5));
    }
}

// ── railway ──────────────────────────────────────────────────────────────────

/// Single track in world coordinates: ballast `ballast_w` wide, a raised
/// bed, sleepers every 0.7 m and two rails. The main line and yard sidings
/// are drawn by the same hand.
pub fn railway_track(g: &mut Geometry, pal: &Palette, pts: &[Vec2], ballast_w: f32) {
    if pts.len() < 2 {
        return;
    }
    g.part(Dirt).path_strip(pts, 0.0, ballast_w, 0.04, rgba(mul(pal.gravel, 0.85)));
    g.part(Dirt).path_strip(pts, 0.0, 3.0, 0.1, rgba(pal.gravel));
    let len = crate::mesh::path_length(pts);
    let mut s = 0.35;
    while s < len {
        let (p, d) = crate::mesh::point_along(pts, s);
        g.part(Wood).boxed(Vec3::new(p.x, 0.17, p.y), Vec3::new(0.24, 0.14, 2.5), d.y.atan2(d.x), rgba(mul(pal.wood, 0.55)));
        s += 0.7;
    }
    for side in [-1.0f32, 1.0] {
        g.part(Metal).path_box(pts, side * 0.72, 0.08, 0.24, 0.38, rgba(mul(pal.steel, 0.75)));
    }
}
