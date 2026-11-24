//! Polygon helpers for the cutter: the bridges to and from `geo`'s boolean
//! ops, half-plane cuts, overlap and outline comparisons, simplification.

use super::*;

/// Has a parcel changed shape? True when the areas differ by more than
/// `area_tol` (a fraction of the larger) or a vertex of either outline lies
/// more than `dist_tol` metres from the other outline.
pub fn outlines_differ(a: &[Vec2], b: &[Vec2], area_tol: f32, dist_tol: f32) -> bool {
    let (area_a, area_b) = (polygon_area(a).abs(), polygon_area(b).abs());
    if (area_a - area_b).abs() > area_tol * area_a.max(area_b).max(1e-3) {
        return true;
    }
    a.iter().any(|&p| distance_to_outline(p, b) > dist_tol) || b.iter().any(|&p| distance_to_outline(p, a) > dist_tol)
}

/// Do two simple polygons share ground (a vertex of one inside the other, or
/// crossing edges)? Outlines that merely touch do not count.
pub fn polygons_overlap(a: &[Vec2], b: &[Vec2]) -> bool {
    if a.len() < 3 || b.len() < 3 {
        return false;
    }
    if a.iter().any(|&p| point_in_polygon(p, b)) || b.iter().any(|&p| point_in_polygon(p, a)) {
        return true;
    }
    if point_in_polygon(polygon_centroid(a), b) || point_in_polygon(polygon_centroid(b), a) {
        return true;
    }
    for i in 0..a.len() {
        let (p, q) = (a[i], a[(i + 1) % a.len()]);
        for j in 0..b.len() {
            if segments_cross(p, q, b[j], b[(j + 1) % b.len()]) {
                return true;
            }
        }
    }
    false
}

/// Keep the part of a convex `poly` where `(p - origin) . normal >= 0`.
pub fn clip_halfplane(poly: &[Vec2], origin: Vec2, normal: Vec2) -> Vec<Vec2> {
    let n = poly.len();
    let mut out: Vec<Vec2> = Vec::with_capacity(n + 2);
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        let da = (a - origin).dot(normal);
        let db = (b - origin).dot(normal);
        if da >= 0.0 {
            out.push(a);
        }
        if (da >= 0.0) != (db >= 0.0) {
            let t = da / (da - db);
            out.push(a + (b - a) * t);
        }
    }
    dedupe(out)
}

pub(super) fn dedupe(mut poly: Vec<Vec2>) -> Vec<Vec2> {
    poly.dedup_by(|a, b| a.distance(*b) < 0.05);
    if poly.len() > 1 && poly[0].distance(*poly.last().expect("len > 1")) < 0.05 {
        poly.pop();
    }
    poly
}

pub(super) fn ccw(poly: &[Vec2]) -> Vec<Vec2> {
    if polygon_area(poly) < 0.0 {
        poly.iter().rev().copied().collect()
    } else {
        poly.to_vec()
    }
}

/// Area of a boolean-ops polygon as f32.
pub(super) fn area_f(p: &Polygon<f64>) -> f32 {
    p.unsigned_area() as f32
}

pub(super) fn to_geo(poly: &[Vec2]) -> Polygon<f64> {
    // Boolean ops want counter-clockwise exteriors; a clockwise ring reads as
    // a negative region and the results are garbage.
    let poly = ccw(poly);
    let coords: Vec<Coord<f64>> = poly.iter().map(|p| Coord { x: p.x as f64, y: p.y as f64 }).collect();
    Polygon::new(LineString::from(coords), vec![])
}

/// Exterior ring as a counter-clockwise polygon without the closing point.
pub(super) fn from_geo(p: &Polygon<f64>) -> Vec<Vec2> {
    let mut pts: Vec<Vec2> = p.exterior().0.iter().map(|c| Vec2::new(c.x as f32, c.y as f32)).collect();
    if pts.len() > 1 && pts[0].distance(*pts.last().expect("len > 1")) < 1e-3 {
        pts.pop();
    }
    ccw(&dedupe(pts))
}

/// Half-plane `(p - origin) . normal >= 0` as a rectangle just big enough
/// to cover `reach` metres around the origin. Keeping it local keeps the
/// boolean ops precise in f32.
pub(super) fn halfplane(origin: Vec2, normal: Vec2, reach: f32) -> Polygon<f64> {
    let t = Vec2::new(-normal.y, normal.x);
    let big = reach;
    to_geo(&[origin - t * big, origin + t * big, origin + t * big + normal * big, origin - t * big + normal * big])
}

/// Cut a polygon by a line, returning the pieces on both sides. A cut whose
/// pieces do not add up to the original (a numerical failure) is rejected.
pub(super) fn cut(poly: &Polygon<f64>, origin: Vec2, normal: Vec2) -> Vec<Polygon<f64>> {
    let pts = from_geo(poly);
    let reach = pts.iter().map(|p| p.distance(origin)).fold(0.0, f32::max) + 10.0;
    let mut out = Vec::new();
    for hp in [halfplane(origin, normal, reach), halfplane(origin, -normal, reach)] {
        let piece: MultiPolygon<f64> = poly.intersection(&hp);
        for p in piece.0 {
            if area_f(&p) > 1.0 {
                out.push(p);
            }
        }
    }
    let total: f32 = out.iter().map(|p| area_f(&p)).sum();
    let original = area_f(&poly);
    if (total - original).abs() > original * 0.02 + 2.0 {
        return Vec::new();
    }
    out
}

/// One-sided strip of depth `d` on the interior side of edge `a -> b` of a
/// counter-clockwise polygon, extended a little along the edge so strips of
/// neighbouring edges overlap around bends.
pub(super) fn edge_strip(a: Vec2, b: Vec2, d: f32) -> Polygon<f64> {
    let dir = (b - a).normalize_or_zero();
    let n = Vec2::new(-dir.y, dir.x);
    let a = a - dir * d * 0.6 - n * 0.5;
    let b = b + dir * d * 0.6 - n * 0.5;
    to_geo(&[a, b, b + n * (d + 0.5), a + n * (d + 0.5)])
}

/// Twice the area over the perimeter: about the thickness of a long piece.
pub(super) fn thickness(p: &Polygon<f64>) -> f32 {
    let pts = from_geo(p);
    let per = perimeter(&pts);
    if per < 1e-3 {
        0.0
    } else {
        2.0 * area_f(&p) / per
    }
}

/// [free-placement] One outline for a set of touching parcels: their union
/// (the largest piece, should they not all touch), straight runs merged so
/// a frontage is one edge.
pub fn joined_outline(polys: &[Vec<Vec2>]) -> Option<Vec<Vec2>> {
    if polys.len() == 1 {
        return Some(polys[0].clone());
    }
    let best = union_outlines(polys).into_iter().max_by(|a, b| polygon_area(a).abs().total_cmp(&polygon_area(b).abs()))?;
    let simple = simplify_collinear(&best);
    Some(if simple.len() >= 3 { simple } else { best })
}

/// A point inside the polygon: the area centroid, or for a concave shape
/// whose centroid falls outside, an interior point.
pub fn inner_center(poly: &[Vec2]) -> Vec2 {
    let mut center = polygon_centroid(poly);
    if !point_in_polygon(center, poly) {
        use geo::InteriorPoint;
        if let Some(ip) = to_geo(poly).interior_point() {
            center = Vec2::new(ip.x() as f32, ip.y() as f32);
        }
    }
    center
}

/// The same block always starts at the same corner, so the same shape is
/// always cut the same way whatever the boolean ops handed back.
pub(super) fn canonical(p: Polygon<f64>) -> Polygon<f64> {
    let ring = from_geo(&p);
    if ring.len() < 3 {
        return p;
    }
    let k = (0..ring.len()).min_by(|&a, &b| ring[a].x.total_cmp(&ring[b].x).then(ring[a].y.total_cmp(&ring[b].y))).unwrap_or(0);
    let coords: Vec<Coord<f64>> = ring[k..].iter().chain(ring[..k].iter()).map(|q| Coord { x: q.x as f64, y: q.y as f64 }).collect();
    Polygon::new(LineString::from(coords), p.interiors().to_vec())
}

/// The outlines of the patches a set of parcels makes (touching parcels
/// merged into one), holes dropped.
pub fn union_outlines(polys: &[Vec<Vec2>]) -> Vec<Vec<Vec2>> {
    let geo_polys: Vec<Polygon<f64>> = polys.iter().filter(|p| p.len() >= 3).map(|p| to_geo(p)).collect();
    if geo_polys.is_empty() {
        return Vec::new();
    }
    let u: MultiPolygon<f64> = geo::unary_union(&geo_polys);
    u.0.iter().map(from_geo).filter(|p| p.len() >= 3).collect()
}

/// Drop vertices where the outline runs on nearly straight, so two joined
/// parcels have one frontage edge rather than two.
pub(super) fn simplify_collinear(poly: &[Vec2]) -> Vec<Vec2> {
    let mut pts = poly.to_vec();
    loop {
        let n = pts.len();
        if n <= 3 {
            return pts;
        }
        let drop = (0..n).find(|&i| {
            let a = pts[(i + n - 1) % n];
            let b = pts[i];
            let c = pts[(i + 1) % n];
            let d1 = (b - a).normalize_or_zero();
            let d2 = (c - b).normalize_or_zero();
            // A straight run, a doubled point, or the tip of a spike: the
            // union of parcels whose shared edge did not quite match
            // leaves a slit, out to a tip and straight back. A slit counts
            // twice in the perimeter, so the ground read as a sliver
            // (`MIN_COMPACTNESS`): a city building's recut lot came out
            // landlocked and the building was lost.
            d1.dot(d2) > 0.9985 || d1.dot(d2) < -0.9985 || a.distance(b) < 0.05
        });
        match drop {
            Some(i) => {
                pts.remove(i);
            }
            None => return pts,
        }
    }
}

/// [r15-lots] (F1) The outline a lot's shape is judged by: the crumbs the
/// polygon booleans leave taken out. First the tips of thin spikes
/// pointing out (under 15 degrees, their triangle under 3% of the lot: a
/// hairline tail left where a shore or corridor cut slid past) are cut
/// off; then the outline is simplified so that every vertex of it stays
/// within 0.6 m of the result (Douglas-Peucker, measured against the
/// outline as it was, so a curve is never flattened step by step): a
/// metre-long zig where two cuts nearly met goes, a real notch (an L), a
/// slit cut in or a real shard stays. Judged on the raw outline, a
/// 3,500 m2 waterfront lot with a 45 m tail and house plots with a step
/// in the back fence read as "sharp" or "sliver" and were dead ground.
pub fn shape_outline(poly: &[Vec2]) -> Vec<Vec2> {
    simplify_ring(&cut_spikes(poly), 0.6)
}

/// [r15-lots] (F1) An outline with the tips of its thin spikes pointing
/// out cut off (under 15 degrees, each under 3% of the area). A zoned lot
/// is this outline ([`make_lot`]): its frontage rectangle, which the
/// building fills, then does not run out along the hairline.
pub fn cut_spikes(poly: &[Vec2]) -> Vec<Vec2> {
    let area = polygon_area(poly).abs();
    let mut pts = poly.to_vec();
    loop {
        let n = pts.len();
        if n <= 3 {
            return pts;
        }
        let mut best: Option<(f32, usize)> = None;
        for i in 0..n {
            let (a, b, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
            let tri = ((b - a).perp_dot(c - a) * 0.5).abs();
            let (ba, bc) = ((a - b).normalize_or_zero(), (c - b).normalize_or_zero());
            let convex = (b - a).perp_dot(c - b) > 0.0;
            if convex && ba != Vec2::ZERO && bc != Vec2::ZERO && ba.dot(bc) > 15f32.to_radians().cos() && tri < area * 0.03 && best.is_none_or(|(t, _)| tri < t) {
                best = Some((tri, i));
            }
        }
        match best {
            Some((_, i)) => {
                pts.remove(i);
            }
            None => return pts,
        }
    }
}

/// [r15-lots] Douglas-Peucker on a closed ring: the fewest vertices that
/// keep every vertex of `ring` within `tol` of the result.
fn simplify_ring(ring: &[Vec2], tol: f32) -> Vec<Vec2> {
    let n = ring.len();
    if n <= 3 {
        return ring.to_vec();
    }
    // Start at the vertex farthest from the middle (a real corner, never a
    // crumb), split there and at the vertex farthest from it.
    let mid = ring.iter().fold(Vec2::ZERO, |a, p| a + *p) / n as f32;
    let first = (0..n).max_by(|&a, &b| ring[a].distance(mid).total_cmp(&ring[b].distance(mid))).unwrap_or(0);
    let ring: Vec<Vec2> = (0..n).map(|k| ring[(first + k) % n]).collect();
    let ring = ring.as_slice();
    let far = (1..n).max_by(|&a, &b| ring[a].distance(ring[0]).total_cmp(&ring[b].distance(ring[0]))).unwrap_or(n / 2);
    fn seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f32 {
        let ab = b - a;
        let l2 = ab.length_squared();
        let t = if l2 < 1e-9 { 0.0 } else { ((p - a).dot(ab) / l2).clamp(0.0, 1.0) };
        p.distance(a + ab * t)
    }
    fn dp(pts: &[Vec2], tol: f32, keep: &mut Vec<bool>, lo: usize, hi: usize) {
        if hi <= lo + 1 {
            return;
        }
        let (mut worst, mut at) = (0.0, lo);
        for k in lo + 1..hi {
            let d = seg_dist(pts[k], pts[lo], pts[hi]);
            if d > worst {
                worst = d;
                at = k;
            }
        }
        if worst > tol {
            keep[at] = true;
            dp(pts, tol, keep, lo, at);
            dp(pts, tol, keep, at, hi);
        }
    }
    let mut chain: Vec<Vec2> = ring.to_vec();
    chain.push(ring[0]);
    let mut keep = vec![false; n + 1];
    keep[0] = true;
    keep[far] = true;
    dp(&chain, tol, &mut keep, 0, far);
    dp(&chain, tol, &mut keep, far, n);
    let out: Vec<Vec2> = (0..n).filter(|&i| keep[i]).map(|i| ring[i]).collect();
    if out.len() < 3 { ring.to_vec() } else { out }
}
