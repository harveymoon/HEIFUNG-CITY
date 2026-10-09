//! Plain 2-D geometry shared by the simulation: polyline and polygon
//! helpers, one polyline offset with a single mitre policy, and a uniform
//! grid for nearest-thing queries.
//!
//! Everything here is pure and allocation-light; the modules that own the
//! data (roads, zoning, walk, park, rail) re-export what their callers
//! already import. [review] The shared formulas (smoothstep, polyline and
//! segment distances, segment crossings, extents, angles, the convex hull)
//! were gathered here from their copies; add new ones here too.

use glam::Vec2;

// ── segments and polylines ───────────────────────────────────────────────────

/// Project `p` onto segment `ab`; returns `(t, point)` with `t` clamped to
/// the segment, so the point is the closest point of the segment.
pub fn project_on_segment(p: Vec2, a: Vec2, b: Vec2) -> (f32, Vec2) {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 < 1e-6 {
        return (0.0, a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (t, a + ab * t)
}

/// Total length of a polyline.
pub fn path_length(pts: &[Vec2]) -> f32 {
    pts.windows(2).map(|w| w[0].distance(w[1])).sum()
}

/// Point and unit direction at arc length `s` along a polyline (clamped to
/// both ends; a single point faces +x).
pub fn point_along(pts: &[Vec2], s: f32) -> (Vec2, Vec2) {
    if pts.len() < 2 {
        return (pts.first().copied().unwrap_or(Vec2::ZERO), Vec2::X);
    }
    let mut s = s.max(0.0);
    for w in pts.windows(2) {
        let len = w[0].distance(w[1]);
        if s <= len {
            let d = (w[1] - w[0]).normalize_or_zero();
            return (w[0] + d * s, d);
        }
        s -= len;
    }
    let n = pts.len();
    (pts[n - 1], (pts[n - 1] - pts[n - 2]).normalize_or_zero())
}

/// Arc length of the closest point on a polyline to `p`, and the distance
/// to it. `None` for fewer than two points.
pub fn project_on_poly(pts: &[Vec2], p: Vec2) -> Option<(f32, f32)> {
    let mut best: Option<(f32, f32)> = None;
    let mut acc = 0.0;
    for w in pts.windows(2) {
        let (t, q) = project_on_segment(p, w[0], w[1]);
        let len = w[0].distance(w[1]);
        let d = p.distance(q);
        if best.is_none_or(|(_, bd)| d < bd) {
            best = Some((acc + t * len, d));
        }
        acc += len;
    }
    best
}

/// Two segments run along each other when both ends of `cd` lie within this
/// distance of the line through `ab`.
pub const COLLINEAR_TOL: f32 = 1.0;

/// The stretch where segment `cd` runs along segment `ab` (collinear within
/// [`COLLINEAR_TOL`], same or opposite direction), as arc lengths `(lo, hi)`
/// measured from `a`. `None` when the segments are not collinear or the
/// shared stretch has no length.
pub fn shared_stretch(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Option<(f32, f32)> {
    let ab = b - a;
    let len = ab.length();
    if len < 1e-3 {
        return None;
    }
    let dir = ab / len;
    let nrm = Vec2::new(-dir.y, dir.x);
    if (c - a).dot(nrm).abs() > COLLINEAR_TOL || (d - a).dot(nrm).abs() > COLLINEAR_TOL {
        return None;
    }
    let tc = (c - a).dot(dir);
    let td = (d - a).dot(dir);
    let (lo, hi) = (tc.min(td).max(0.0), tc.max(td).min(len));
    (hi > lo).then_some((lo, hi))
}

// ── polygons ─────────────────────────────────────────────────────────────────

/// Even-odd point-in-polygon test; a polygon with fewer than three vertices
/// contains nothing.
pub fn point_in_polygon(p: Vec2, poly: &[Vec2]) -> bool {
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (pi, pj) = (poly[i], poly[j]);
        if (pi.y > p.y) != (pj.y > p.y) {
            let x = pi.x + (p.y - pi.y) / (pj.y - pi.y) * (pj.x - pi.x);
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Signed area; positive means counter-clockwise.
pub fn polygon_area(poly: &[Vec2]) -> f32 {
    let n = poly.len();
    let mut a = 0.0;
    for i in 0..n {
        let j = (i + 1) % n;
        a += poly[i].perp_dot(poly[j]);
    }
    a * 0.5
}

/// Area centroid; the vertex mean for a degenerate (zero-area) polygon.
pub fn polygon_centroid(poly: &[Vec2]) -> Vec2 {
    let a = polygon_area(poly);
    if a.abs() < 1e-3 {
        return poly.iter().fold(Vec2::ZERO, |s, p| s + *p) / poly.len().max(1) as f32;
    }
    let n = poly.len();
    let mut c = Vec2::ZERO;
    for i in 0..n {
        let j = (i + 1) % n;
        let cross = poly[i].perp_dot(poly[j]);
        c += (poly[i] + poly[j]) * cross;
    }
    c / (6.0 * a)
}

/// Perimeter of a closed polygon.
pub fn perimeter(poly: &[Vec2]) -> f32 {
    let n = poly.len();
    (0..n).map(|i| poly[i].distance(poly[(i + 1) % n])).sum()
}

/// Length of boundary two closed polygons share: summed over every pair of
/// edges that run along each other (collinear within [`COLLINEAR_TOL`]).
/// Neighbouring parcels cut from one block share whole edges; parcels on
/// opposite sides of a street share none, the street being wider than the
/// tolerance.
pub fn shared_boundary(a: &[Vec2], b: &[Vec2]) -> f32 {
    let (na, nb) = (a.len(), b.len());
    if na < 2 || nb < 2 || !boxes_overlap(bounds(a), bounds(b), COLLINEAR_TOL) {
        return 0.0;
    }
    let mut total = 0.0;
    for i in 0..na {
        let (p, q) = (a[i], a[(i + 1) % na]);
        for j in 0..nb {
            if let Some((lo, hi)) = shared_stretch(p, q, b[j], b[(j + 1) % nb]) {
                total += hi - lo;
            }
        }
    }
    total
}

/// Axis-aligned bounds `(lo, hi)` of a point set. An empty set gives an
/// inverted box (`lo` is `MAX`, `hi` is `MIN`), which overlaps nothing.
pub fn bounds(pts: &[Vec2]) -> (Vec2, Vec2) {
    pts.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)))
}

/// Do two axis-aligned boxes overlap, with `slack` of tolerance?
pub fn boxes_overlap(a: (Vec2, Vec2), b: (Vec2, Vec2), slack: f32) -> bool {
    !(a.1.x < b.0.x - slack || b.1.x < a.0.x - slack || a.1.y < b.0.y - slack || b.1.y < a.0.y - slack)
}

/// Distance from `p` to the outline (edges) of a closed polygon; `MAX` for
/// an empty polygon.
pub fn distance_to_outline(p: Vec2, poly: &[Vec2]) -> f32 {
    let n = poly.len();
    (0..n).map(|i| p.distance(project_on_segment(p, poly[i], poly[(i + 1) % n]).1)).fold(f32::MAX, f32::min)
}

/// Does any pair of non-adjacent edges of this closed polygon cross? A
/// bow-tie zone is not a region, and boolean ops give nonsense for it.
pub fn self_intersects(poly: &[Vec2]) -> bool {
    let n = poly.len();
    if n < 4 {
        return false;
    }
    for i in 0..n {
        for j in (i + 2)..n {
            // Edges i and j are adjacent when j follows i, or when j is the
            // last edge and i the first (they meet at vertex 0).
            if i == 0 && j == n - 1 {
                continue;
            }
            if segments_cross(poly[i], poly[(i + 1) % n], poly[j], poly[(j + 1) % n]) {
                return true;
            }
        }
    }
    false
}

// ── offsets ──────────────────────────────────────────────────────────────────

/// Mitre policy for every polyline and ring offset in the simulation: a
/// corner's offset vertex sits on the bisector at `offset / cos(half turn)`,
/// capped at this many offsets. The cap keeps a hairpin (which would want an
/// unbounded spike) to a blunt mitre; turns up to 120 degrees are exact.
pub const MITRE_LIMIT: f32 = 2.0;

/// Direction and scale of the offset at a vertex between incoming direction
/// `d_in` and outgoing direction `d_out`: the mitre normal and how far along
/// it one unit of offset moves. A degenerate corner (reversal) falls back to
/// the incoming normal.
fn mitre(d_in: Vec2, d_out: Vec2) -> (Vec2, f32) {
    let n_in = Vec2::new(-d_in.y, d_in.x);
    let n_out = Vec2::new(-d_out.y, d_out.x);
    let m = (n_in + n_out).normalize_or_zero();
    if m == Vec2::ZERO {
        (n_in, 1.0)
    } else {
        (m, (1.0 / m.dot(n_in).max(1e-3)).min(MITRE_LIMIT))
    }
}

/// Open polyline shifted to its left by `offset` (negative for the right),
/// with mitred joins under [`MITRE_LIMIT`]. Ends are offset square to their
/// own leg. Repeated points share the neighbouring leg's direction.
pub fn offset_polyline(points: &[Vec2], offset: f32) -> Vec<Vec2> {
    let n = points.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let d_in = if i > 0 { (points[i] - points[i - 1]).normalize_or_zero() } else { Vec2::ZERO };
        let d_out = if i + 1 < n { (points[i + 1] - points[i]).normalize_or_zero() } else { Vec2::ZERO };
        let d_in = if d_in == Vec2::ZERO { d_out } else { d_in };
        let d_out = if d_out == Vec2::ZERO { d_in } else { d_out };
        let (m, scale) = mitre(d_in, d_out);
        out.push(points[i] + m * offset * scale);
    }
    out
}

/// Area under which an inset ring counts as collapsed.
pub const INSET_MIN_AREA: f32 = 4.0;

/// Closed ring inset towards its interior by `d` whatever its winding, same
/// vertex count, mitred under [`MITRE_LIMIT`]. Empty when the ring was
/// thinner than `2 d` and the inset turned it inside out: the result has
/// (almost) no area left, its winding flipped (a strip), or most of its
/// edges now run backwards (a small square comes out point-reflected, with
/// its winding and a positive area intact, so the area test alone misses it).
pub fn inset_ring(poly: &[Vec2], d: f32) -> Vec<Vec2> {
    let n = poly.len();
    let area = polygon_area(poly);
    let sign = if area > 0.0 { 1.0 } else { -1.0 };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let p0 = poly[(i + n - 1) % n];
        let p1 = poly[i];
        let p2 = poly[(i + 1) % n];
        let d1 = (p1 - p0).normalize_or_zero();
        let d2 = (p2 - p1).normalize_or_zero();
        let (m, scale) = mitre(d1, d2);
        out.push(p1 + m * sign * d * scale);
    }
    let inner = polygon_area(&out);
    let reversed = (0..n).filter(|&i| (out[(i + 1) % n] - out[i]).dot(poly[(i + 1) % n] - poly[i]) <= 0.0).count();
    if inner.abs() < INSET_MIN_AREA || inner * sign <= 0.0 || reversed * 2 > n {
        return Vec::new();
    }
    out
}

// ── uniform grid ─────────────────────────────────────────────────────────────

/// A uniform grid over axis-aligned boxes, for "what is near this point"
/// queries. Items are the indices `0..n` in the order they were given; each
/// is stored in every cell its box touches, so a query never misses an item
/// whose closest point is in a cell it visited.
///
/// Built once per rebuild and queried many times; a rebuild is `O(n)` so
/// building per query would be no better than the linear scan it replaces.
#[derive(Clone, Debug, Default)]
pub struct Grid {
    cell: f32,
    origin: Vec2,
    nx: i32,
    ny: i32,
    cells: Vec<Vec<u32>>,
    /// Every item's box, so a box query can answer exactly.
    boxes: Vec<(Vec2, Vec2)>,
}

impl Grid {
    /// Index boxes of `cell` metres per cell. Items with an inverted (empty)
    /// or non-finite box are left out and never returned.
    pub fn new(cell: f32, boxes: impl IntoIterator<Item = (Vec2, Vec2)>) -> Grid {
        let boxes: Vec<(Vec2, Vec2)> = boxes.into_iter().collect();
        let cell = if cell.is_finite() && cell > 0.0 { cell } else { 1.0 };
        let valid = |b: &(Vec2, Vec2)| b.0.is_finite() && b.1.is_finite() && b.0.x <= b.1.x && b.0.y <= b.1.y;
        let (lo, hi) = boxes.iter().filter(|b| valid(b)).fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), b| (lo.min(b.0), hi.max(b.1)));
        if lo.x > hi.x {
            return Grid { cell, origin: Vec2::ZERO, nx: 0, ny: 0, cells: Vec::new(), boxes };
        }
        // Cap the cell count so a huge sparse set does not allocate a sea of
        // empty cells; the grid just gets coarser.
        let span = (hi - lo).max(Vec2::splat(cell));
        let cell = cell.max(span.max_element() / 512.0);
        let nx = ((span.x / cell).floor() as i32 + 1).max(1);
        let ny = ((span.y / cell).floor() as i32 + 1).max(1);
        let mut g = Grid { cell, origin: lo, nx, ny, cells: vec![Vec::new(); (nx * ny) as usize], boxes };
        for i in 0..g.boxes.len() {
            let b = g.boxes[i];
            if !valid(&b) {
                continue;
            }
            let (x0, y0) = g.cell_of(b.0);
            let (x1, y1) = g.cell_of(b.1);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    g.cells[(y * nx + x) as usize].push(i as u32);
                }
            }
        }
        g
    }

    /// Number of items given at build time (including ones left out).
    pub fn len(&self) -> usize {
        self.boxes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Cell coordinates of a point, clamped onto the grid.
    fn cell_of(&self, p: Vec2) -> (i32, i32) {
        let c = (p - self.origin) / self.cell;
        ((c.x.floor() as i32).clamp(0, self.nx - 1), (c.y.floor() as i32).clamp(0, self.ny - 1))
    }

    /// Items whose box touches `(lo, hi)`, each once, in ascending index
    /// order (the order a linear scan would visit them).
    pub fn in_box(&self, lo: Vec2, hi: Vec2) -> Vec<u32> {
        let mut out = Vec::new();
        if self.cells.is_empty() || !(lo.is_finite() && hi.is_finite()) {
            return out;
        }
        // A box entirely off the grid touches nothing.
        let grid_hi = self.origin + Vec2::new(self.nx as f32, self.ny as f32) * self.cell;
        if hi.x < self.origin.x || hi.y < self.origin.y || lo.x > grid_hi.x || lo.y > grid_hi.y {
            return out;
        }
        let (x0, y0) = self.cell_of(lo);
        let (x1, y1) = self.cell_of(hi);
        for y in y0..=y1 {
            for x in x0..=x1 {
                out.extend(self.cells[(y * self.nx + x) as usize].iter().copied().filter(|&i| boxes_overlap(self.boxes[i as usize], (lo, hi), 0.0)));
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// The item nearest to `p` by `dist` (which returns the item's distance,
    /// or `INFINITY` to skip it), with that distance. Cells are visited in
    /// rings outward from `p`; the search stops once no unvisited cell can
    /// hold anything closer. Exact ties go to the lower index, as a linear
    /// scan keeping the first best would resolve them.
    pub fn nearest(&self, p: Vec2, mut dist: impl FnMut(u32) -> f32) -> Option<(u32, f32)> {
        if self.cells.is_empty() || !p.is_finite() {
            return None;
        }
        let (cx, cy) = self.cell_of(p);
        // Distance from p to the far side of its own cell row/column decides
        // how far ring r reaches; the conservative bound below uses the
        // ring's inner edge, which is at least (r - 1) cells from p plus
        // p's distance to its cell's border on the near side.
        let local = (p - self.origin) / self.cell - Vec2::new(cx as f32, cy as f32);
        let to_border = local.min(Vec2::ONE - local).min_element().clamp(0.0, 1.0) * self.cell;
        let max_ring = (cx.max(self.nx - 1 - cx)).max(cy.max(self.ny - 1 - cy));
        let mut best: Option<(u32, f32)> = None;
        let mut consider = |i: u32, best: &mut Option<(u32, f32)>| {
            let d = dist(i);
            if !d.is_finite() {
                return;
            }
            match best {
                Some((bi, bd)) if d > *bd || (d == *bd && i >= *bi) => {}
                _ => *best = Some((i, d)),
            }
        };
        for r in 0..=max_ring {
            let (x0, x1) = ((cx - r).max(0), (cx + r).min(self.nx - 1));
            let (y0, y1) = ((cy - r).max(0), (cy + r).min(self.ny - 1));
            for y in y0..=y1 {
                let edge_row = y == cy - r || y == cy + r;
                if edge_row {
                    for x in x0..=x1 {
                        for &i in &self.cells[(y * self.nx + x) as usize] {
                            consider(i, &mut best);
                        }
                    }
                } else {
                    for x in [cx - r, cx + r] {
                        if x >= 0 && x < self.nx {
                            for &i in &self.cells[(y * self.nx + x) as usize] {
                                consider(i, &mut best);
                            }
                        }
                    }
                }
            }
            // Anything not yet seen sits in ring r + 1 or beyond, whose
            // nearest point is at least this far from p.
            let bound = r as f32 * self.cell + to_border;
            if best.is_some_and(|(_, d)| d <= bound) {
                break;
            }
        }
        best
    }
}

// ── scalar, angle and segment helpers ───────────────────────────────────────

/// Hermite smoothstep of `x` between the edges `e0` and `e1`, `0..1`.
pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// An angle wrapped into `-PI..PI`.
pub fn wrap_angle(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

/// Angle `a` to `b` by `t`, the short way round.
pub fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let mut d = (b - a) % std::f32::consts::TAU;
    if d > std::f32::consts::PI {
        d -= std::f32::consts::TAU;
    } else if d < -std::f32::consts::PI {
        d += std::f32::consts::TAU;
    }
    a + d * t
}

/// Distance from `p` to a polyline (`INFINITY` with fewer than two points).
pub fn distance_to_polyline(p: Vec2, pts: &[Vec2]) -> f32 {
    pts.windows(2).map(|w| project_on_segment(p, w[0], w[1]).1.distance(p)).fold(f32::INFINITY, f32::min)
}

/// The span `(lo, hi)` of a point set projected on `axis`.
pub fn extents(poly: &[Vec2], axis: Vec2) -> (f32, f32) {
    poly.iter().map(|p| p.dot(axis)).fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)))
}

/// Do segments `ab` and `cd` cross properly (each strictly straddling the
/// other's line)?
pub fn segments_cross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let d1 = (b - a).perp_dot(c - a);
    let d2 = (b - a).perp_dot(d - a);
    let d3 = (d - c).perp_dot(a - c);
    let d4 = (d - c).perp_dot(b - c);
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// Parameters `(t, u)` of the intersection of `p0p1` and `q0q1` within
/// both segments, their ends included (to `eps`).
pub fn segment_intersection_closed(p0: Vec2, p1: Vec2, q0: Vec2, q1: Vec2, eps: f32) -> Option<(f32, f32)> {
    let r = p1 - p0;
    let s = q1 - q0;
    let denom = r.perp_dot(s);
    if denom.abs() < 1e-6 {
        return None;
    }
    let qp = q0 - p0;
    let t = qp.perp_dot(s) / denom;
    let u = qp.perp_dot(r) / denom;
    (t >= -eps && t <= 1.0 + eps && u >= -eps && u <= 1.0 + eps).then(|| (t.clamp(0.0, 1.0), u.clamp(0.0, 1.0)))
}

/// Parameters `(t, u)` of the intersection of `p0p1` and `q0q1`, when it is
/// strictly inside both segments.
pub fn segment_intersection(p0: Vec2, p1: Vec2, q0: Vec2, q1: Vec2) -> Option<(f32, f32)> {
    let r = p1 - p0;
    let s = q1 - q0;
    let denom = r.perp_dot(s);
    if denom.abs() < 1e-6 {
        return None;
    }
    let qp = q0 - p0;
    let t = qp.perp_dot(s) / denom;
    let u = qp.perp_dot(r) / denom;
    let eps = 1e-4;
    if t > eps && t < 1.0 - eps && u > eps && u < 1.0 - eps {
        Some((t, u))
    } else {
        None
    }
}

/// Shortest distance between segments `p0`-`p1` and `q0`-`q1` (0 when
/// they cross).
pub fn segment_distance_between(p0: Vec2, p1: Vec2, q0: Vec2, q1: Vec2) -> f32 {
    if segment_intersection(p0, p1, q0, q1).is_some() {
        return 0.0;
    }
    let d = |p: Vec2, a: Vec2, b: Vec2| project_on_segment(p, a, b).1.distance(p);
    d(p0, q0, q1).min(d(p1, q0, q1)).min(d(q0, p0, p1)).min(d(q1, p0, p1))
}

/// Convex hull, counter-clockwise (Andrew's monotone chain).
pub fn convex_hull(points: &[Vec2]) -> Vec<Vec2> {
    let mut pts: Vec<Vec2> = points.iter().copied().filter(|p| p.is_finite()).collect();
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup_by(|a, b| a.distance(*b) < 1e-3);
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a - o).perp_dot(b - o);
    let mut lower: Vec<Vec2> = Vec::new();
    for &p in &pts {
        while lower.len() >= 2 && cross(lower[lower.len() - 2], lower[lower.len() - 1], p) <= 0.0 {
            lower.pop();
        }
        lower.push(p);
    }
    let mut upper: Vec<Vec2> = Vec::new();
    for &p in pts.iter().rev() {
        while upper.len() >= 2 && cross(upper[upper.len() - 2], upper[upper.len() - 1], p) <= 0.0 {
            upper.pop();
        }
        upper.push(p);
    }
    lower.pop();
    upper.pop();
    lower.extend(upper);
    lower
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_boundary_counts_touching_edges_only() {
        let sq = |x: f32, y: f32, w: f32, h: f32| vec![Vec2::new(x, y), Vec2::new(x + w, y), Vec2::new(x + w, y + h), Vec2::new(x, y + h)];
        // Side by side: the whole 20 m side is shared, whichever way round.
        let a = sq(0.0, 0.0, 30.0, 20.0);
        let b = sq(30.0, 0.0, 25.0, 20.0);
        assert!((shared_boundary(&a, &b) - 20.0).abs() < 1e-3);
        assert!((shared_boundary(&b, &a) - 20.0).abs() < 1e-3);
        // Offset half a side along: half shared.
        let c = sq(30.0, 10.0, 25.0, 20.0);
        assert!((shared_boundary(&a, &c) - 10.0).abs() < 1e-3);
        // Across a 12 m street: nothing shared.
        let d = sq(42.0, 0.0, 25.0, 20.0);
        assert_eq!(shared_boundary(&a, &d), 0.0);
        // Touching at a corner only: nothing shared.
        let e = sq(30.0, 20.0, 10.0, 10.0);
        assert!(shared_boundary(&a, &e) < 1e-3);
    }

    #[test]
    fn projection_and_arc_length_helpers_agree() {
        let pts = [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0)];
        assert_eq!(path_length(&pts), 20.0);
        let (q, d) = point_along(&pts, 15.0);
        assert!(q.distance(Vec2::new(10.0, 5.0)) < 1e-4 && d.distance(Vec2::Y) < 1e-4);
        let (s, dist) = project_on_poly(&pts, Vec2::new(12.0, 5.0)).unwrap();
        assert!((s - 15.0).abs() < 1e-4 && (dist - 2.0).abs() < 1e-4);
        assert_eq!(project_on_segment(Vec2::new(5.0, 3.0), Vec2::ZERO, Vec2::new(10.0, 0.0)), (0.5, Vec2::new(5.0, 0.0)));
        assert!(point_along(&[], 3.0) == (Vec2::ZERO, Vec2::X));
        assert!(project_on_poly(&[Vec2::ONE], Vec2::ZERO).is_none());
    }

    #[test]
    fn shared_stretch_finds_the_overlap_either_direction() {
        let (a, b) = (Vec2::ZERO, Vec2::new(10.0, 0.0));
        assert_eq!(shared_stretch(a, b, Vec2::new(4.0, 0.5), Vec2::new(14.0, 0.5)), Some((4.0, 10.0)));
        assert_eq!(shared_stretch(a, b, Vec2::new(14.0, 0.0), Vec2::new(4.0, 0.0)), Some((4.0, 10.0)));
        assert!(shared_stretch(a, b, Vec2::new(4.0, 3.0), Vec2::new(14.0, 3.0)).is_none(), "too far off the line");
        assert!(shared_stretch(a, b, Vec2::new(12.0, 0.0), Vec2::new(20.0, 0.0)).is_none(), "no overlap");
    }

    #[test]
    fn polygon_helpers_handle_both_windings() {
        let ccw = [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 6.0), Vec2::new(0.0, 6.0)];
        let cw: Vec<Vec2> = ccw.iter().rev().copied().collect();
        assert_eq!(polygon_area(&ccw), 60.0);
        assert_eq!(polygon_area(&cw), -60.0);
        assert_eq!(polygon_centroid(&cw), Vec2::new(5.0, 3.0));
        assert_eq!(perimeter(&ccw), 32.0);
        assert!(point_in_polygon(Vec2::new(1.0, 1.0), &cw) && !point_in_polygon(Vec2::new(11.0, 1.0), &ccw));
        assert_eq!(distance_to_outline(Vec2::new(5.0, 2.0), &ccw), 2.0);
        assert_eq!(bounds(&ccw), (Vec2::ZERO, Vec2::new(10.0, 6.0)));
        assert!(boxes_overlap(bounds(&ccw), (Vec2::new(10.5, 0.0), Vec2::new(12.0, 1.0)), 0.5));
        assert!(!boxes_overlap(bounds(&ccw), (Vec2::new(10.6, 0.0), Vec2::new(12.0, 1.0)), 0.5));
        let bow = [Vec2::ZERO, Vec2::new(10.0, 10.0), Vec2::new(10.0, 0.0), Vec2::new(0.0, 10.0)];
        assert!(self_intersects(&bow) && !self_intersects(&ccw));
    }

    #[test]
    fn offsets_are_exact_on_square_corners_and_capped_on_hairpins() {
        let square = [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)];
        let inner = inset_ring(&square, 1.0);
        assert_eq!(inner.len(), 4);
        for (p, want) in inner.iter().zip([Vec2::new(1.0, 1.0), Vec2::new(9.0, 1.0), Vec2::new(9.0, 9.0), Vec2::new(1.0, 9.0)]) {
            assert!(p.distance(want) < 1e-4, "{p:?} vs {want:?}");
        }
        // Winding does not matter: clockwise insets inward too.
        let cw: Vec<Vec2> = square.iter().rev().copied().collect();
        assert!((polygon_area(&inset_ring(&cw, 1.0)).abs() - 64.0).abs() < 1e-3);
        assert!(inset_ring(&square, 6.0).is_empty(), "thinner than twice the inset collapses");
        assert!((polygon_area(&inset_ring(&square, 3.0)) - 16.0).abs() < 1e-3, "a deep inset that still leaves room works");
        let strip = [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 3.0), Vec2::new(0.0, 3.0)];
        assert!(inset_ring(&strip, 2.0).is_empty(), "a strip inset past its middle turns inside out");
        assert_eq!(inset_ring(&strip, 1.0).len(), 4);
        // A hairpin polyline's mitre is capped instead of spiking.
        let hairpin = [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(0.0, 0.5)];
        let off = offset_polyline(&hairpin, 1.0);
        assert!(off[1].distance(hairpin[1]) <= MITRE_LIMIT + 1e-4);
        // A straight polyline is simply shifted left.
        let line = [Vec2::ZERO, Vec2::new(10.0, 0.0)];
        assert_eq!(offset_polyline(&line, 2.0), vec![Vec2::new(0.0, 2.0), Vec2::new(10.0, 2.0)]);
        assert_eq!(offset_polyline(&line, -2.0), vec![Vec2::new(0.0, -2.0), Vec2::new(10.0, -2.0)]);
    }

    #[test]
    fn grid_nearest_matches_a_linear_scan_including_ties() {
        // Points on a lattice, so exact ties are common.
        let pts: Vec<Vec2> = (0..20).flat_map(|y| (0..20).map(move |x| Vec2::new(x as f32 * 10.0, y as f32 * 10.0))).collect();
        let grid = Grid::new(25.0, pts.iter().map(|p| (*p, *p)));
        let mut seed = 7u32;
        for _ in 0..500 {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let q = Vec2::new((seed >> 8) as f32 / 16_777_216.0 * 260.0 - 30.0, (seed >> 20) as f32 / 4096.0 * 260.0 - 30.0);
            // Half the queries land exactly between lattice points.
            let q = if seed & 1 == 0 { Vec2::new((q.x / 5.0).round() * 5.0, (q.y / 5.0).round() * 5.0) } else { q };
            let lin = pts.iter().enumerate().fold(None, |best: Option<(usize, f32)>, (i, p)| {
                let d = p.distance(q);
                if best.is_none_or(|(_, bd)| d < bd) { Some((i, d)) } else { best }
            });
            let got = grid.nearest(q, |i| pts[i as usize].distance(q));
            assert_eq!(got.map(|(i, _)| i as usize), lin.map(|(i, _)| i), "query {q:?}");
        }
        // Filtering skips items; an all-skipped grid finds nothing.
        assert!(grid.nearest(Vec2::ZERO, |_| f32::INFINITY).is_none());
        let far = grid.nearest(Vec2::new(500.0, 500.0), |i| pts[i as usize].distance(Vec2::new(500.0, 500.0))).unwrap();
        assert_eq!(far.0 as usize, pts.len() - 1);
        // Box queries return each item once, ascending.
        let hits = grid.in_box(Vec2::new(-1.0, -1.0), Vec2::new(11.0, 11.0));
        assert_eq!(hits, vec![0, 1, 20, 21]);
        assert!(Grid::new(10.0, std::iter::empty()).nearest(Vec2::ZERO, |_| 0.0).is_none());
    }
}
