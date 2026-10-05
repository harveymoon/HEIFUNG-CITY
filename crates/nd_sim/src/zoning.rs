//! Zones (land-use polygons) and the parcels carved out of them.
//!
//! The road corridors are subtracted from a zone with real polygon booleans,
//! so a curved road removes exactly its own strip and nothing else. Each
//! remaining block is sliced along its road frontage into parcels of the
//! land use's size. A parcel is usable only when one of its edges lies along
//! a road; land with no road access is kept as unusable ground, and slivers
//! are never built on.

use crate::buildings::BuildingId;
use crate::roads::{project_on_segment, NodeId, RoadNetwork, SegmentId};
use geo::{Area, BooleanOps, Coord, LineString, MultiPolygon, Polygon};
use glam::Vec2;
use rand::Rng;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};

/// Pavement between the kerb and the parcel line, in metres.
pub const SIDEWALK: f32 = 3.2;
/// Parcels smaller than this are dropped as slivers.
const MIN_AREA: f32 = 60.0;
/// Area over squared perimeter below which a parcel is a sliver (a square is
/// 0.0625, an equilateral triangle 0.048, a 1:4 strip 0.04).
const MIN_COMPACTNESS: f32 = 0.030;
/// Shortest edge that counts as road frontage.
const MIN_FRONTAGE: f32 = 6.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LandUse {
    Residential,
    Commercial,
    Industrial,
    Entertainment,
    Civic,
    Park,
}

impl LandUse {
    pub const ALL: [LandUse; 6] = [
        LandUse::Residential,
        LandUse::Commercial,
        LandUse::Industrial,
        LandUse::Entertainment,
        LandUse::Civic,
        LandUse::Park,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LandUse::Residential => "Residential",
            LandUse::Commercial => "Commercial",
            LandUse::Industrial => "Industrial",
            LandUse::Entertainment => "Entertainment",
            LandUse::Civic => "Civic",
            LandUse::Park => "Park",
        }
    }

    pub fn index(self) -> usize {
        LandUse::ALL.iter().position(|&l| l == self).unwrap()
    }

    /// Typical frontage along the road, in metres.
    pub fn lot_width(self) -> f32 {
        match self {
            LandUse::Residential => 14.0,
            LandUse::Commercial => 18.0,
            LandUse::Industrial => 30.0,
            LandUse::Entertainment => 20.0,
            LandUse::Civic => 30.0,
            LandUse::Park => 24.0,
        }
    }

    pub fn lot_depth(self) -> f32 {
        match self {
            LandUse::Industrial | LandUse::Civic => 36.0,
            LandUse::Park => 28.0,
            _ => 24.0,
        }
    }
}

new_key_type! {
    pub struct ZoneId;
    pub struct LotId;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Zone {
    pub polygon: Vec<Vec2>,
    pub land_use: LandUse,
}

impl Zone {
    pub fn contains(&self, p: Vec2) -> bool {
        point_in_polygon(p, &self.polygon)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lot {
    pub zone: ZoneId,
    pub land_use: LandUse,
    /// Exact parcel outline, counter-clockwise.
    pub polygon: Vec<Vec2>,
    /// Bounding rectangle aligned to the frontage: front-left, front-right,
    /// back-right, back-left. The polygon fits inside it.
    pub corners: [Vec2; 4],
    /// The polygon edge that faces the road.
    pub front_edge: [Vec2; 2],
    /// Area centroid.
    pub center: Vec2,
    /// Direction from the lot towards the road.
    pub front_dir: Vec2,
    pub front_segment: SegmentId,
    pub front_t: f32,
    /// Road node citizens use to enter and leave the lot.
    pub access_node: NodeId,
    pub building: Option<BuildingId>,
    /// No edge touches a road, or the shape is a sliver: nothing can be
    /// built here.
    #[serde(default)]
    pub landlocked: bool,
}

impl Lot {
    /// Frontage of the bounding rectangle.
    pub fn width(&self) -> f32 {
        self.corners[0].distance(self.corners[1])
    }

    /// Depth of the bounding rectangle.
    pub fn depth(&self) -> f32 {
        self.corners[1].distance(self.corners[2])
    }

    pub fn area(&self) -> f32 {
        polygon_area(&self.polygon).abs()
    }

    pub fn contains(&self, p: Vec2) -> bool {
        point_in_polygon(p, &self.polygon)
    }

    /// Where people step off the pavement onto the lot.
    pub fn entrance(&self) -> Vec2 {
        self.front_edge[0].lerp(self.front_edge[1], 0.5)
    }

    pub fn usable(&self) -> bool {
        !self.landlocked
    }
}

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
            let x = pj.x + (p.y - pj.y) / (pj.y - pi.y) * (pi.x - pj.x);
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

fn dedupe(mut poly: Vec<Vec2>) -> Vec<Vec2> {
    poly.dedup_by(|a, b| a.distance(*b) < 0.05);
    if poly.len() > 1 && poly[0].distance(*poly.last().unwrap()) < 0.05 {
        poly.pop();
    }
    poly
}

fn ccw(poly: &[Vec2]) -> Vec<Vec2> {
    if polygon_area(poly) < 0.0 {
        poly.iter().rev().copied().collect()
    } else {
        poly.to_vec()
    }
}

fn extents(poly: &[Vec2], axis: Vec2) -> (f32, f32) {
    poly.iter().map(|p| p.dot(axis)).fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)))
}

// ── geo bridges ──────────────────────────────────────────────────────────────

fn to_geo(poly: &[Vec2]) -> Polygon<f32> {
    let coords: Vec<Coord<f32>> = poly.iter().map(|p| Coord { x: p.x, y: p.y }).collect();
    Polygon::new(LineString::from(coords), vec![])
}

/// Exterior ring as a counter-clockwise polygon without the closing point.
fn from_geo(p: &Polygon<f32>) -> Vec<Vec2> {
    let mut pts: Vec<Vec2> = p.exterior().0.iter().map(|c| Vec2::new(c.x, c.y)).collect();
    if pts.len() > 1 && pts[0].distance(*pts.last().unwrap()) < 1e-3 {
        pts.pop();
    }
    ccw(&dedupe(pts))
}

/// Half-plane `(p - origin) . normal >= 0` as a very large polygon.
fn halfplane(origin: Vec2, normal: Vec2) -> Polygon<f32> {
    let t = Vec2::new(-normal.y, normal.x);
    let big = 20_000.0;
    to_geo(&[origin - t * big, origin + t * big, origin + t * big + normal * big, origin - t * big + normal * big])
}

/// Rectangle around a road segment, extended by `hw` past each end so the
/// strips of a bend overlap and junction corners get cleared.
fn corridor(a: Vec2, b: Vec2, hw: f32) -> Polygon<f32> {
    let d = (b - a).normalize_or_zero();
    let n = Vec2::new(-d.y, d.x) * hw;
    let a = a - d * hw;
    let b = b + d * hw;
    to_geo(&[a + n, b + n, b - n, a - n])
}

/// Distance from a point to the nearest street centreline, with that
/// segment. Paths never count as frontage.
fn nearest_road(roads: &RoadNetwork, p: Vec2) -> Option<(SegmentId, f32, f32)> {
    let mut best: Option<(SegmentId, f32, f32)> = None;
    for (sid, seg) in roads.alive_segments() {
        if !seg.is_street() {
            continue;
        }
        let (a, b) = roads.segment_points(sid);
        let (t, q) = project_on_segment(p, a, b);
        let dist = p.distance(q);
        if best.map_or(true, |(_, _, d)| dist < d) {
            best = Some((sid, t, dist));
        }
    }
    best
}

/// Is this edge lying along a road's pavement line? Returns the segment and
/// the parameter of the edge midpoint on it.
fn edge_on_road(roads: &RoadNetwork, a: Vec2, b: Vec2) -> Option<(SegmentId, f32)> {
    let len = a.distance(b);
    if len < 1.0 {
        return None;
    }
    let mid = (a + b) * 0.5;
    let (sid, t, dist) = nearest_road(roads, mid)?;
    let seg = roads.segment(sid);
    let (p0, p1) = roads.segment_points(sid);
    let rd = (p1 - p0).normalize_or_zero();
    let ed = (b - a) / len;
    let parallel = rd.dot(ed).abs() > 0.8;
    let on_line = (dist - seg.half_width() - SIDEWALK).abs() < 2.5;
    if parallel && on_line {
        Some((sid, t))
    } else {
        None
    }
}

/// Index of the longest edge that lies along a road, else the longest edge.
fn frontage_edge(poly: &[Vec2], roads: &RoadNetwork) -> (usize, Option<(SegmentId, f32)>) {
    let n = poly.len();
    let mut best_road: Option<(f32, usize, (SegmentId, f32))> = None;
    let mut best_any: (f32, usize) = (0.0, 0);
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        let len = a.distance(b);
        if len > best_any.0 {
            best_any = (len, i);
        }
        if let Some(hit) = edge_on_road(roads, a, b) {
            if best_road.map_or(true, |(l, _, _)| len > l) {
                best_road = Some((len, i, hit));
            }
        }
    }
    match best_road {
        Some((_, i, hit)) => (i, Some(hit)),
        None => (best_any.1, None),
    }
}

/// Sharpest reflex (inward-pointing) corner of a counter-clockwise polygon
/// and its turning angle in radians. Gentle concavity along a curved road
/// is tolerated; only corners sharper than `min_turn` count.
fn reflex_vertex(poly: &[Vec2], min_turn: f32) -> Option<(usize, f32)> {
    let n = poly.len();
    let mut best: Option<(usize, f32)> = None;
    for i in 0..n {
        let a = poly[(i + n - 1) % n];
        let b = poly[i];
        let c = poly[(i + 1) % n];
        let d1 = (b - a).normalize_or_zero();
        let d2 = (c - b).normalize_or_zero();
        let cross = d1.perp_dot(d2);
        if cross >= 0.0 {
            continue;
        }
        let turn = cross.abs().clamp(0.0, 1.0).asin();
        let turn = if d1.dot(d2) < 0.0 { std::f32::consts::PI - turn } else { turn };
        if turn > min_turn && best.map_or(true, |(_, t)| turn > t) {
            best = Some((i, turn));
        }
    }
    best
}

/// Straight runs of road frontage: consecutive road-facing edges that bend
/// by less than ~25 degrees between them (a curved road is one run, a block
/// corner separates two). Returns (edge indices, total length), longest
/// first. Cuts are placed along a run, perpendicular to the local road, so
/// straight frontages get regular lots, curves get radial ones, and corners
/// are never cut right at the corner.
fn frontage_runs(poly: &[Vec2], roads: &RoadNetwork) -> Vec<(Vec<usize>, f32)> {
    let n = poly.len();
    let on: Vec<bool> = (0..n).map(|i| edge_on_road(roads, poly[i], poly[(i + 1) % n]).is_some()).collect();
    if !on.iter().any(|&b| b) {
        return Vec::new();
    }
    let dir = |i: usize| (poly[(i + 1) % n] - poly[i]).normalize_or_zero();
    // Start at an edge that begins a run: after a gap or after a corner.
    let begins = |i: usize| {
        let prev = (i + n - 1) % n;
        !on[prev] || dir(prev).dot(dir(i)) < 0.9
    };
    let start = (0..n).find(|&i| on[i] && begins(i)).unwrap_or(0);
    let mut runs: Vec<(Vec<usize>, f32)> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut cur_len = 0.0;
    for k in 0..n {
        let i = (start + k) % n;
        if on[i] {
            if k > 0 && begins(i) && !cur.is_empty() {
                runs.push((std::mem::take(&mut cur), cur_len));
                cur_len = 0.0;
            }
            cur.push(i);
            cur_len += poly[i].distance(poly[(i + 1) % n]);
        } else if !cur.is_empty() {
            runs.push((std::mem::take(&mut cur), cur_len));
            cur_len = 0.0;
        }
    }
    if !cur.is_empty() {
        runs.push((cur, cur_len));
    }
    runs.sort_by(|a, b| b.1.total_cmp(&a.1));
    runs
}

/// Cut a polygon by a line, returning the pieces on both sides.
fn cut(poly: &Polygon<f32>, origin: Vec2, normal: Vec2) -> Vec<Polygon<f32>> {
    let mut out = Vec::new();
    for hp in [halfplane(origin, normal), halfplane(origin, -normal)] {
        let piece: MultiPolygon<f32> = poly.intersection(&hp);
        for p in piece.0 {
            if p.unsigned_area() > 1.0 {
                out.push(p);
            }
        }
    }
    out
}

/// Slice a block into parcels of roughly the land use's size. Cuts run
/// perpendicular to the road frontage with some jitter, occasionally at an
/// angle, deep blocks get a back row, and concave pieces are cut straight at
/// their reflex corner so every parcel ends up convex.
fn subdivide(poly: Polygon<f32>, lu: LandUse, roads: &RoadNetwork, rng: &mut impl Rng, depth: u32, out: &mut Vec<Vec<Vec2>>) {
    let area = poly.unsigned_area();
    if area < MIN_AREA {
        return;
    }
    let pts = from_geo(&poly);
    if pts.len() < 3 {
        return;
    }
    if depth > 40 {
        out.push(pts);
        return;
    }
    // Holes (a block wrapped around a loop road) cannot be parcels: cut
    // straight through the hole so the pieces are simple.
    if let Some(hole) = poly.interiors().first() {
        if let Some(c) = hole.0.first() {
            let origin = Vec2::new(c.x, c.y);
            for piece in cut(&poly, origin, Vec2::X) {
                subdivide(piece, lu, roads, rng, depth + 1, out);
            }
            return;
        }
    }
    // Sharp inward corners get one straight cut: across the road if the
    // corner sits on a road edge, else along the edge that leads into it.
    if let Some((i, _)) = reflex_vertex(&pts, 0.6) {
        let n = pts.len();
        let a = pts[(i + n - 1) % n];
        let b = pts[i];
        let d = (b - a).normalize_or_zero();
        let on_road = edge_on_road(roads, a, b).is_some();
        let normal = if on_road { d } else { Vec2::new(-d.y, d.x) };
        let pieces = cut(&poly, b, normal);
        if pieces.len() >= 2 {
            for piece in pieces {
                subdivide(piece, lu, roads, rng, depth + 1, out);
            }
            return;
        }
    }

    let lw = lu.lot_width();
    let ld = lu.lot_depth();
    let n = pts.len();
    let runs = frontage_runs(&pts, roads);
    let Some((chain, chain_len)) = runs.first().cloned() else {
        // No road frontage at all: one piece of unusable ground.
        out.push(pts);
        return;
    };
    // Slice along the longest straight frontage, perpendicular to the local
    // road direction. Corner lots keep both of their frontages.
    if chain_len > lw * 1.7 && area > lw * ld * 1.3 {
        let mut s = lw * rng.random_range(0.85..1.35);
        if chain_len - s < lw * 0.8 {
            s = chain_len * 0.5;
        }
        let mut acc = 0.0;
        for &i in &chain {
            let a = pts[i];
            let b = pts[(i + 1) % n];
            let len = a.distance(b);
            if acc + len >= s {
                let q = a.lerp(b, ((s - acc) / len).clamp(0.05, 0.95));
                let d = (b - a) / len;
                let angle: f32 = if rng.random_bool(0.15) { rng.random_range(-0.2..0.2) } else { 0.0 };
                let (sn, co) = angle.sin_cos();
                let normal = Vec2::new(d.x * co - d.y * sn, d.x * sn + d.y * co);
                let pieces = cut(&poly, q, normal);
                if pieces.len() >= 2 {
                    for piece in pieces {
                        subdivide(piece, lu, roads, rng, depth + 1, out);
                    }
                    return;
                }
                break;
            }
            acc += len;
        }
    }
    // Depth: a back row behind the frontage parcels.
    let longest = chain.iter().copied().max_by(|&a, &b| pts[a].distance(pts[(a + 1) % n]).total_cmp(&pts[b].distance(pts[(b + 1) % n]))).unwrap_or(chain[0]);
    let along = (pts[(longest + 1) % n] - pts[longest]).normalize_or_zero();
    if along == Vec2::ZERO {
        out.push(pts);
        return;
    }
    let inward = Vec2::new(-along.y, along.x);
    let (u0, u1) = extents(&pts, along);
    let (v0, v1) = extents(&pts, inward);
    let dep = v1 - v0;
    if dep > ld * 1.9 {
        let c = v0 + ld * rng.random_range(0.95..1.25);
        let origin = along * ((u0 + u1) * 0.5) + inward * c;
        let pieces = cut(&poly, origin, inward);
        if pieces.len() >= 2 {
            for piece in pieces {
                subdivide(piece, lu, roads, rng, depth + 1, out);
            }
            return;
        }
    }
    out.push(pts);
}

fn polygon_seed(poly: &[Vec2]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in poly {
        for v in [p.x, p.y] {
            h ^= v.to_bits() as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

fn perimeter(poly: &[Vec2]) -> f32 {
    let n = poly.len();
    (0..n).map(|i| poly[i].distance(poly[(i + 1) % n])).sum()
}

/// Carve `zone` into parcels: subtract every road corridor, slice the blocks
/// along their frontages, and keep parcels whose centre is not covered by one
/// of the `covering` zones (drawn later, so they win) or an existing lot.
/// Parcels with no road frontage or a sliver shape are kept as landlocked,
/// unusable ground. New lots are appended to `lots`.
pub fn generate_lots(
    zone_id: ZoneId,
    zone: &Zone,
    roads: &RoadNetwork,
    lots: &mut SlotMap<LotId, Lot>,
    covering: &[Zone],
) -> Vec<LotId> {
    let lu = zone.land_use;
    let mut created = Vec::new();
    if zone.polygon.len() < 3 || roads.is_empty() {
        return created;
    }
    let mut rng = crate::rng_for(polygon_seed(&zone.polygon), 5);
    let zone_poly = to_geo(&ccw(&zone.polygon));
    let (zlo, zhi) = zone.polygon.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));

    // Blocks: the zone minus every nearby road corridor.
    let mut corridors: Option<MultiPolygon<f32>> = None;
    for (sid, seg) in roads.alive_segments() {
        let (a, b) = roads.segment_points(sid);
        let hw = if seg.is_street() { seg.half_width() + SIDEWALK } else { seg.half_width() + 1.0 };
        let lo = a.min(b) - Vec2::splat(hw * 2.0);
        let hi = a.max(b) + Vec2::splat(hw * 2.0);
        if hi.x < zlo.x || hi.y < zlo.y || lo.x > zhi.x || lo.y > zhi.y {
            continue;
        }
        let c = corridor(a, b, hw);
        corridors = Some(match corridors {
            None => MultiPolygon(vec![c]),
            Some(m) => m.union(&c),
        });
    }
    let blocks: MultiPolygon<f32> = match &corridors {
        Some(m) => zone_poly.difference(m),
        None => MultiPolygon(vec![zone_poly]),
    };

    let mut parcels: Vec<Vec<Vec2>> = Vec::new();
    for block in blocks.0 {
        subdivide(block, lu, roads, &mut rng, 0, &mut parcels);
    }

    for poly in parcels {
        let poly = ccw(&poly);
        if poly.len() < 3 {
            continue;
        }
        let center = polygon_centroid(&poly);
        if covering.iter().any(|z| z.contains(center)) {
            continue;
        }
        if lots.values().any(|l| l.contains(center) || point_in_polygon(l.center, &poly)) {
            continue;
        }
        let area = polygon_area(&poly);
        let compact = area / perimeter(&poly).powi(2);
        let (fi, road) = frontage_edge(&poly, roads);
        let n = poly.len();
        let fa = poly[fi];
        let fb = poly[(fi + 1) % n];
        let along = (fb - fa).normalize_or_zero();
        if along == Vec2::ZERO {
            continue;
        }
        let inward = Vec2::new(-along.y, along.x);
        let (u0, u1) = extents(&poly, along);
        let (v0, v1) = extents(&poly, inward);
        let corners = [
            along * u0 + inward * v0,
            along * u1 + inward * v0,
            along * u1 + inward * v1,
            along * u0 + inward * v1,
        ];
        let has_frontage = road.is_some() && fa.distance(fb) >= MIN_FRONTAGE.min(lu.lot_width() * 0.45);
        let sharp = reflex_vertex(&poly, 0.6).is_some();
        let landlocked = !has_frontage || sharp || compact < MIN_COMPACTNESS || (u1 - u0) < 8.0 || (v1 - v0) < 8.0;
        let (sid, t) = match road {
            Some(hit) => hit,
            None => match nearest_road(roads, center) {
                Some((s, t, _)) => (s, t),
                None => continue,
            },
        };
        let lot = Lot {
            zone: zone_id,
            land_use: lu,
            polygon: poly,
            corners,
            front_edge: [fa, fb],
            center,
            front_dir: -inward,
            front_segment: sid,
            front_t: t,
            access_node: roads.nearest_endpoint(sid, t),
            building: None,
            landlocked,
        };
        created.push(lots.insert(lot));
    }
    created
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roads_cross() -> RoadNetwork {
        let mut r = RoadNetwork::default();
        r.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        r.add_polyline(&[Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0)], 1, 1);
        r
    }

    fn usable(lots: &SlotMap<LotId, Lot>) -> Vec<&Lot> {
        lots.values().filter(|l| l.usable()).collect()
    }

    #[test]
    fn zone_straddling_roads_splits_into_blocks_and_parcels() {
        let roads = roads_cross();
        let zone = Zone {
            polygon: vec![Vec2::new(-100.0, -80.0), Vec2::new(100.0, -80.0), Vec2::new(100.0, 80.0), Vec2::new(-100.0, 80.0)],
            land_use: LandUse::Residential,
        };
        let mut lots = SlotMap::with_key();
        generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
        let good = usable(&lots);
        assert!(good.len() >= 8, "got {} usable parcels", good.len());
        for lot in lots.values() {
            assert!(lot.center.x.abs() > 5.0 && lot.center.y.abs() > 5.0, "parcel on road at {:?}", lot.center);
            assert!(polygon_area(&lot.polygon) > 0.0, "parcels are counter-clockwise");
            assert!(reflex_vertex(&lot.polygon, 0.6).is_none(), "parcels have no sharp inward corners");
        }
        for lot in &good {
            let mid = lot.entrance();
            let (_, _, d) = nearest_road(&roads, mid).unwrap();
            assert!((d - 5.75 - SIDEWALK).abs() < 2.6, "usable parcels front a road, got {d}");
        }
        let total: f32 = lots.values().map(|l| l.area()).sum();
        assert!(total > 200.0 * 160.0 * 0.6, "coverage {total}");
    }

    #[test]
    fn diagonal_road_makes_non_rectangular_parcels() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-200.0, -200.0), Vec2::new(200.0, 200.0)], 1, 1);
        let zone = Zone {
            polygon: vec![Vec2::new(-100.0, -100.0), Vec2::new(100.0, -100.0), Vec2::new(100.0, 100.0), Vec2::new(-100.0, 100.0)],
            land_use: LandUse::Commercial,
        };
        let mut lots = SlotMap::with_key();
        generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
        assert!(!usable(&lots).is_empty());
        let non_rect = lots.values().filter(|l| l.polygon.len() != 4).count();
        assert!(non_rect > 0, "expected triangles or pentagons where the diagonal road meets the zone edge");
    }

    #[test]
    fn covering_zone_takes_precedence() {
        let roads = roads_cross();
        let zone = Zone {
            polygon: vec![Vec2::new(10.0, 10.0), Vec2::new(150.0, 10.0), Vec2::new(150.0, 150.0), Vec2::new(10.0, 150.0)],
            land_use: LandUse::Residential,
        };
        let cover = Zone { polygon: zone.polygon.clone(), land_use: LandUse::Park };
        let mut lots = SlotMap::with_key();
        let ids = generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[cover]);
        assert!(ids.is_empty());
    }

    #[test]
    fn curved_road_cuts_only_its_own_strip_and_leaves_no_slivers() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        let curve = crate::plan::sample_curve(&[Vec2::new(-120.0, 0.0), Vec2::new(-20.0, 120.0), Vec2::new(120.0, 0.0)]);
        roads.add_polyline(&curve, 1, 1);
        let zone = Zone {
            polygon: vec![Vec2::new(-200.0, 10.0), Vec2::new(200.0, 10.0), Vec2::new(200.0, 200.0), Vec2::new(-200.0, 200.0)],
            land_use: LandUse::Residential,
        };
        let mut lots = SlotMap::with_key();
        generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
        let good = usable(&lots);
        assert!(good.len() >= 12, "got {} usable parcels", good.len());
        for lot in &good {
            let compact = lot.area() / perimeter(&lot.polygon).powi(2);
            assert!(compact >= MIN_COMPACTNESS, "sliver parcel with compactness {compact}");
            assert!(lot.front_edge[0].distance(lot.front_edge[1]) >= 5.0);
        }
        // Parcels well inside the arch, away from any road, are landlocked.
        assert!(lots.values().any(|l| l.landlocked), "deep land without frontage is marked unusable");
    }

    #[test]
    fn land_without_road_access_is_unusable() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        // A deep zone with a road only on its south side.
        let zone = Zone {
            polygon: vec![Vec2::new(-80.0, 10.0), Vec2::new(80.0, 10.0), Vec2::new(80.0, 130.0), Vec2::new(-80.0, 130.0)],
            land_use: LandUse::Residential,
        };
        let mut lots = SlotMap::with_key();
        generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
        let front = usable(&lots);
        assert!(!front.is_empty());
        assert!(front.iter().all(|l| l.center.y < 60.0), "only the first row fronts the road");
        assert!(lots.values().any(|l| l.landlocked && l.center.y > 60.0), "back land is landlocked");
    }

    #[test]
    fn paths_cut_lots_but_give_no_frontage() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        roads.add_polyline_kind(&[Vec2::new(-300.0, 80.0), Vec2::new(300.0, 80.0)], 0, 0, crate::roads::RoadKind::Path);
        let zone = Zone {
            polygon: vec![Vec2::new(-80.0, 10.0), Vec2::new(80.0, 10.0), Vec2::new(80.0, 150.0), Vec2::new(-80.0, 150.0)],
            land_use: LandUse::Residential,
        };
        let mut lots = SlotMap::with_key();
        generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
        assert!(lots.values().all(|l| (l.center.y - 80.0).abs() > 2.0), "no lot sits on the path");
        assert!(lots.values().filter(|l| l.usable()).all(|l| l.center.y < 60.0), "land beyond the path is not usable from it");
        assert!(lots.values().any(|l| l.center.y > 90.0 && l.landlocked));
    }

    #[test]
    fn every_road_side_of_a_block_gets_parcels_including_corners() {
        let mut roads = RoadNetwork::default();
        for c in [-70.0f32, 70.0] {
            roads.add_polyline(&[Vec2::new(-300.0, c), Vec2::new(300.0, c)], 1, 1);
            roads.add_polyline(&[Vec2::new(c, -300.0), Vec2::new(c, 300.0)], 1, 1);
        }
        let zone = Zone {
            polygon: vec![Vec2::new(-65.0, -65.0), Vec2::new(65.0, -65.0), Vec2::new(65.0, 65.0), Vec2::new(-65.0, 65.0)],
            land_use: LandUse::Residential,
        };
        let mut lots = SlotMap::with_key();
        generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
        let good: Vec<&Lot> = lots.values().filter(|l| l.usable()).collect();
        // Parcels front all four roads.
        for (axis, sign) in [(0, -1.0f32), (0, 1.0), (1, -1.0), (1, 1.0)] {
            let n = good.iter().filter(|l| {
                let e = l.entrance();
                let v = if axis == 0 { e.x } else { e.y };
                (v * sign) > 50.0
            }).count();
            assert!(n >= 3, "side {axis}/{sign} has only {n} usable parcels");
        }
        // Corners are claimed by a lot whose polygon reaches both roads.
        for (sx, sy) in [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let corner = Vec2::new(sx * 58.0, sy * 58.0);
            assert!(good.iter().any(|l| l.contains(corner)), "corner {corner:?} is not in a usable lot");
        }
        // Usable land covers most of the block.
        let usable_area: f32 = good.iter().map(|l| l.area()).sum();
        assert!(usable_area > 122.0 * 122.0 * 0.5, "usable {usable_area}");
    }
}
