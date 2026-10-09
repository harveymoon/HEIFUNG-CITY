//! Footprint geometry in lot coordinates: bounds, offsets, clipping, fitting
//! shapes inside the lot, and the shape builders the styles use.

#[allow(unused_imports)]
use super::*;

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

/// Offset each edge of a counter-clockwise polygon inward by its own
/// distance (`insets[i]` for the edge from vertex `i` to `i + 1`; zero
/// leaves the edge where it is). Unlike [`offset_poly`] every vertex is
/// kept, so edge `i` of the result is the shifted edge `i`: a plant field
/// insets the sides it fences and runs flush to the sides it shares with
/// the next parcel of the field. Mitres are limited to 2.5 times the larger
/// inset; meant for the near-rectangular parcels of a block.
pub fn offset_edges(poly: &[Vec2], insets: &[f32]) -> Vec<Vec2> {
    let n = poly.len();
    if n < 3 || insets.len() != n {
        return poly.to_vec();
    }
    (0..n)
        .map(|i| {
            let prev = (i + n - 1) % n;
            let (p0, p1, p2) = (poly[prev], poly[i], poly[(i + 1) % n]);
            let d1 = (p1 - p0).normalize_or_zero();
            let d2 = (p2 - p1).normalize_or_zero();
            let n1 = Vec2::new(-d1.y, d1.x) * insets[prev];
            let n2 = Vec2::new(-d2.y, d2.x) * insets[i];
            let a = p0 + n1;
            let b = p1 + n2;
            let denom = d1.perp_dot(d2);
            let q = if denom.abs() < 1e-4 { p1 + (n1 + n2) * 0.5 } else { a + d1 * ((b - a).perp_dot(d2) / denom) };
            let limit = insets[prev].max(insets[i]).abs() * 2.5;
            let d = q - p1;
            if !q.is_finite() {
                p1
            } else if d.length() > limit && limit > 0.0 {
                p1 + d * (limit / d.length())
            } else {
                q
            }
        })
        .collect()
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

/// `p` moved `d` towards `target` (a corner pulled in towards the middle).
pub fn toward(p: Vec2, target: Vec2, d: f32) -> Vec2 {
    p + (target - p).normalize_or_zero() * d
}

impl Ctx<'_> {
    /// Whether every point lies on the lot (lot-local).
    pub fn all_inside(&self, pts: &[Vec2]) -> bool {
        pts.iter().all(|p| self.inside_local(*p))
    }
}
