//! Parcels, zones (land-use polygons) and lots.
//!
//! Parcels are cut along every street and path as soon as it is built,
//! zoned or not ([`generate_parcels`]): the road corridors are subtracted
//! from the land within reach of the roads with real polygon booleans, so a
//! curved road removes exactly its own strip and nothing else, and each
//! remaining block is sliced along its road frontage into parcels of a
//! home's size. A parcel is usable only when one of its edges lies along a
//! road; land with no road access is kept as landlocked backland, and
//! slivers are never built on. Zoning then gives parcels a land use
//! ([`crate::plan::parcel_zoning`]) and every zoned parcel becomes a lot;
//! uses that want wider lots join neighbouring parcels ([`join_parcels`]).
//!
//! [`generate_lots`] still cuts a single zone polygon on its own, at the
//! land use's own lot size: tests and the building previews use it.

use crate::buildings::BuildingId;
use crate::geom::{self, bounds as bounds_of, boxes_overlap, perimeter, Grid};
use crate::roads::{NodeId, RoadNetwork, SegmentId};
use geo::{Area, BooleanOps, Coord, LineString, MultiPolygon, Polygon};
use glam::Vec2;
use rand::Rng;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};
use std::collections::HashMap;

pub use crate::geom::{distance_to_outline, point_in_polygon, polygon_area, polygon_centroid};

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
    /// Homes and parks may line a footpath; everything else needs a street.
    pub fn fronts_paths(self) -> bool {
        matches!(self, LandUse::Residential | LandUse::Park)
    }

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

    /// Smallest parcel this use may take: no works yard or civic site on a
    /// corner scrap.
    pub fn min_parcel_area(self) -> f32 {
        match self {
            LandUse::Industrial | LandUse::Civic => 200.0,
            _ => MIN_AREA,
        }
    }

    /// Does this use join neighbouring parcels into bigger lots? Every use
    /// whose lots are bigger than a home's does; parks join up as clusters
    /// instead ([`crate::park`]).
    pub fn joins_parcels(self) -> bool {
        self != LandUse::Park && self.lot_area() > PARCEL_USE.lot_area() * 1.1
    }

    /// Uses whose lots run deeper than a parcel (works yards, civic
    /// grounds) take the backland behind their lots, as the deeper rows
    /// they were cut to always did.
    pub fn takes_backland(self) -> bool {
        self != LandUse::Park && self.lot_depth() > PARCEL_USE.lot_depth()
    }

    /// The area of a typical lot: its width by a row's usual depth.
    pub fn lot_area(self) -> f32 {
        self.lot_width() * self.lot_depth() * 1.15
    }

    /// Parse a label or key ("residential", "Park", ...).
    pub fn parse(s: &str) -> Option<LandUse> {
        LandUse::ALL.into_iter().find(|l| l.label().eq_ignore_ascii_case(s.trim()))
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
    /// Pedestrians who passed this lot today and in the current hour.
    #[serde(default)]
    pub footfall_today: u32,
    #[serde(default)]
    pub footfall_hour: u32,
    /// The street a car can reach this lot from and where it stops. Lots
    /// that line a path have no car access; residents park nearby.
    #[serde(default)]
    pub car_segment: Option<SegmentId>,
    #[serde(default)]
    pub car_point: Vec2,
    /// This lot's share of a park: paths, benches and features.
    #[serde(default)]
    pub park: Option<crate::park::ParkLayout>,
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

    /// Does the lot front a street (as opposed to a footpath)?
    pub fn car_access(&self) -> bool {
        self.car_segment == Some(self.front_segment)
    }
}

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

fn segments_cross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let d1 = (b - a).perp_dot(c - a);
    let d2 = (b - a).perp_dot(d - a);
    let d3 = (d - c).perp_dot(a - c);
    let d4 = (d - c).perp_dot(b - c);
    d1 * d2 < 0.0 && d3 * d4 < 0.0
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

/// Area of a boolean-ops polygon as f32.
fn area_f(p: &Polygon<f64>) -> f32 {
    p.unsigned_area() as f32
}

fn to_geo(poly: &[Vec2]) -> Polygon<f64> {
    // Boolean ops want counter-clockwise exteriors; a clockwise ring reads as
    // a negative region and the results are garbage.
    let poly = ccw(poly);
    let coords: Vec<Coord<f64>> = poly.iter().map(|p| Coord { x: p.x as f64, y: p.y as f64 }).collect();
    Polygon::new(LineString::from(coords), vec![])
}

/// Exterior ring as a counter-clockwise polygon without the closing point.
fn from_geo(p: &Polygon<f64>) -> Vec<Vec2> {
    let mut pts: Vec<Vec2> = p.exterior().0.iter().map(|c| Vec2::new(c.x as f32, c.y as f32)).collect();
    if pts.len() > 1 && pts[0].distance(*pts.last().unwrap()) < 1e-3 {
        pts.pop();
    }
    ccw(&dedupe(pts))
}

/// Half-plane `(p - origin) . normal >= 0` as a rectangle just big enough
/// to cover `reach` metres around the origin. Keeping it local keeps the
/// boolean ops precise in f32.
fn halfplane(origin: Vec2, normal: Vec2, reach: f32) -> Polygon<f64> {
    let t = Vec2::new(-normal.y, normal.x);
    let big = reach;
    to_geo(&[origin - t * big, origin + t * big, origin + t * big + normal * big, origin - t * big + normal * big])
}

/// Chains of road pieces joined at plain two-way nodes, with the corridor
/// half-width of the chain and whether each end sits at a junction or dead
/// end (where the corridor is extended to clear the corner).
fn road_chains(roads: &RoadNetwork) -> Vec<(Vec<Vec2>, f32, bool, bool, SegmentId)> {
    let degree = |n: NodeId| roads.node(n).segments.iter().filter(|&&s| roads.segment(s).alive).count();
    let hw_of = |s: SegmentId| {
        let seg = roads.segment(s);
        if seg.is_street() { seg.half_width() + SIDEWALK } else { seg.half_width() + 1.0 }
    };
    let next_of = |node: NodeId, from: SegmentId| -> Option<SegmentId> {
        if degree(node) != 2 {
            return None;
        }
        let other = roads.node(node).segments.iter().copied().find(|&s| s != from && roads.segment(s).alive)?;
        ((hw_of(other) - hw_of(from)).abs() < 0.01 && roads.segment(other).tunnel == roads.segment(from).tunnel && roads.spans_lots(other) == roads.spans_lots(from)).then_some(other)
    };
    let far = |s: SegmentId, from: NodeId| { let seg = roads.segment(s); if seg.a == from { seg.b } else { seg.a } };
    let mut used = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (sid, seg) in roads.alive_segments() {
        // Tunnels take no ground: the parcels above them are whole. Nor
        // does a viaduct high enough to span over them (its columns find
        // room between the buildings).
        if used.contains(&sid.0) || seg.tunnel || roads.spans_lots(sid) {
            continue;
        }
        // Walk back to the chain start.
        let (mut cur, mut node) = (sid, seg.a);
        let mut guard = 0;
        while let Some(prev) = next_of(node, cur) {
            if prev == sid || guard > 10_000 {
                break;
            }
            guard += 1;
            node = far(prev, node);
            cur = prev;
        }
        let start = node;
        let mut pts = vec![roads.node(node).pos];
        let mut s = cur;
        loop {
            used.insert(s.0);
            node = far(s, node);
            pts.push(roads.node(node).pos);
            match next_of(node, s) {
                Some(n) if !used.contains(&n.0) => s = n,
                _ => break,
            }
        }
        let end_junction = degree(node) != 2 || node == start;
        let start_junction = degree(start) != 2 || node == start;
        out.push((pts, hw_of(cur), start_junction, end_junction, cur));
    }
    out
}

/// Corridor polygon of a chain: both sides offset with mitred joins (the
/// shared [`geom::MITRE_LIMIT`] policy) so a curve's outline is smooth, ends
/// extended by the half-width at junctions and dead ends so the corners get
/// cleared.
fn chain_corridor(pts: &[Vec2], hw: f32, ext_start: bool, ext_end: bool) -> Option<Polygon<f64>> {
    if pts.len() < 2 {
        return None;
    }
    let mut p = pts.to_vec();
    let n = p.len();
    let d0 = (p[1] - p[0]).normalize_or_zero();
    let d1 = (p[n - 1] - p[n - 2]).normalize_or_zero();
    p[0] -= d0 * if ext_start { hw } else { 0.5 };
    p[n - 1] += d1 * if ext_end { hw } else { 0.5 };
    let left = geom::offset_polyline(&p, hw);
    let right = geom::offset_polyline(&p, -hw);
    let mut ring = left;
    ring.extend(right.iter().rev());
    if polygon_area(&ring).abs() < 1.0 {
        return None;
    }
    Some(to_geo(&ring))
}

/// Corridor edge distance of a road: where parcels start.
fn corridor_edge(seg: &crate::roads::Segment) -> f32 {
    if seg.is_street() { seg.half_width() + SIDEWALK } else { seg.half_width() + 1.0 }
}

/// Distance from a point to the nearest road centreline, with that
/// segment. Paths count only when `allow_paths` (homes may line a path).
fn nearest_road(roads: &RoadNetwork, p: Vec2, allow_paths: bool) -> Option<(SegmentId, f32, f32)> {
    // Highways and bridges front nothing: no driveway joins either.
    let (sid, t, q) = roads.nearest_where(p, |seg| !seg.is_rail() && !seg.fronts_nothing() && !seg.bridge && (seg.is_street() || allow_paths))?;
    Some((sid, t, p.distance(q)))
}

/// Is this edge lying along a road's pavement line? Returns the segment and
/// the parameter of the edge midpoint on it.
fn edge_on_road(roads: &RoadNetwork, a: Vec2, b: Vec2, allow_paths: bool) -> Option<(SegmentId, f32)> {
    let len = a.distance(b);
    if len < 1.0 {
        return None;
    }
    let mid = (a + b) * 0.5;
    let (sid, t, dist) = nearest_road(roads, mid, allow_paths)?;
    let seg = roads.segment(sid);
    let (p0, p1) = roads.segment_points(sid);
    let rd = (p1 - p0).normalize_or_zero();
    let ed = (b - a) / len;
    let parallel = rd.dot(ed).abs() > 0.8;
    let on_line = (dist - corridor_edge(seg)).abs() < 2.5;
    if parallel && on_line {
        Some((sid, t))
    } else {
        None
    }
}

/// Index of the longest edge that lies along a road, else the longest edge.
fn frontage_edge(poly: &[Vec2], roads: &RoadNetwork, allow_paths: bool) -> (usize, Option<(SegmentId, f32)>) {
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
        if let Some(hit) = edge_on_road(roads, a, b, allow_paths) {
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
fn frontage_runs(poly: &[Vec2], roads: &RoadNetwork, allow_paths: bool) -> Vec<(Vec<usize>, f32)> {
    let n = poly.len();
    let on: Vec<bool> = (0..n).map(|i| edge_on_road(roads, poly[i], poly[(i + 1) % n], allow_paths).is_some()).collect();
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
    // A winding frontage (an S-curve is one run) is split into chunks that
    // turn by less than about 50 degrees in total, so each chunk has a
    // meaningful inward direction to peel a row from.
    let mut chunked: Vec<(Vec<usize>, f32)> = Vec::new();
    for (chain, _) in runs {
        let mut cur: Vec<usize> = Vec::new();
        let mut cur_len = 0.0;
        let mut turn = 0.0;
        for (k, &i) in chain.iter().enumerate() {
            if k > 0 {
                let d0 = dir(chain[k - 1]);
                let d1 = dir(i);
                turn += d0.perp_dot(d1).abs().clamp(0.0, 1.0).asin();
                if turn > 0.9 && cur_len > 20.0 {
                    chunked.push((std::mem::take(&mut cur), cur_len));
                    cur_len = 0.0;
                    turn = 0.0;
                }
            }
            cur.push(i);
            cur_len += poly[i].distance(poly[(i + 1) % n]);
        }
        if !cur.is_empty() {
            chunked.push((cur, cur_len));
        }
    }
    chunked.sort_by(|a, b| b.1.total_cmp(&a.1));
    chunked
}

/// Cut a polygon by a line, returning the pieces on both sides. A cut whose
/// pieces do not add up to the original (a numerical failure) is rejected.
fn cut(poly: &Polygon<f64>, origin: Vec2, normal: Vec2) -> Vec<Polygon<f64>> {
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

/// Cut a polygon by the road direction at arc length `s` along a run.
fn cut_along_run(poly: &Polygon<f64>, pts: &[Vec2], chain: &[usize], s: f32, rng: &mut impl Rng) -> Vec<Polygon<f64>> {
    let n = pts.len();
    let mut acc = 0.0;
    for &i in chain {
        let a = pts[i];
        let b = pts[(i + 1) % n];
        let len = a.distance(b);
        if acc + len >= s {
            let q = a.lerp(b, ((s - acc) / len).clamp(0.05, 0.95));
            let d = (b - a) / len;
            let angle: f32 = if rng.random_bool(0.12) { rng.random_range(-0.18..0.18) } else { 0.0 };
            let (sn, co) = angle.sin_cos();
            let normal = Vec2::new(d.x * co - d.y * sn, d.x * sn + d.y * co);
            return cut(poly, q, normal);
        }
        acc += len;
    }
    Vec::new()
}

/// One-sided strip of depth `d` on the interior side of edge `a -> b` of a
/// counter-clockwise polygon, extended a little along the edge so strips of
/// neighbouring edges overlap around bends.
fn edge_strip(a: Vec2, b: Vec2, d: f32) -> Polygon<f64> {
    let dir = (b - a).normalize_or_zero();
    let n = Vec2::new(-dir.y, dir.x);
    let a = a - dir * d * 0.6 - n * 0.5;
    let b = b + dir * d * 0.6 - n * 0.5;
    to_geo(&[a, b, b + n * (d + 0.5), a + n * (d + 0.5)])
}

/// Union of the edge strips of a run.
fn run_band(pts: &[Vec2], chain: &[usize], d: f32) -> MultiPolygon<f64> {
    let n = pts.len();
    let mut acc: Option<MultiPolygon<f64>> = None;
    for &i in chain {
        let s = edge_strip(pts[i], pts[(i + 1) % n], d);
        acc = Some(match acc {
            None => MultiPolygon(vec![s]),
            Some(m) => m.union(&s),
        });
    }
    acc.unwrap_or(MultiPolygon(vec![]))
}

/// Twice the area over the perimeter: about the thickness of a long piece.
fn thickness(p: &Polygon<f64>) -> f32 {
    let pts = from_geo(p);
    let per = perimeter(&pts);
    if per < 1e-3 {
        0.0
    } else {
        2.0 * area_f(&p) / per
    }
}

/// Slice a block into parcels. Rows one lot deep are peeled off every road
/// frontage at once (a union of strips along the road edges, so curved roads
/// get curved rows and nothing can fold over); the first frontage claims the
/// corners. The row depth grows while the leftover core would be a thin
/// strip, so two facing frontages meet in the middle. Rows are then cut
/// across at lot width, perpendicular to the local road. Whatever has no
/// frontage stays as unusable ground.
fn subdivide(poly: Polygon<f64>, lu: LandUse, roads: &RoadNetwork, rng: &mut impl Rng, depth: u32, out: &mut Vec<Vec<Vec2>>) {
    let area = area_f(&poly);
    if area < MIN_AREA {
        return;
    }
    let pts = from_geo(&poly);
    if pts.len() < 3 {
        return;
    }
    if depth > 60 {
        out.push(pts);
        return;
    }
    if let Some(hole) = poly.interiors().first() {
        if let Some(c) = hole.0.first() {
            let origin = Vec2::new(c.x as f32, c.y as f32);
            let pieces = cut(&poly, origin, Vec2::X);
            if pieces.is_empty() {
                out.push(pts);
                return;
            }
            for piece in pieces {
                subdivide(piece, lu, roads, rng, depth + 1, out);
            }
            return;
        }
    }
    if let Some((i, _)) = reflex_vertex(&pts, 0.6) {
        let n = pts.len();
        let a = pts[(i + n - 1) % n];
        let b = pts[i];
        let d = (b - a).normalize_or_zero();
        let on_road = edge_on_road(roads, a, b, lu.fronts_paths()).is_some();
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
    let allow = lu.fronts_paths();
    let runs = frontage_runs(&pts, roads, allow);
    let Some((chain, chain_len)) = runs.first().cloned() else {
        out.push(pts);
        return;
    };

    // Peel rows off every frontage, deepening them while the core would be thin.
    let mut chosen: Option<(Vec<MultiPolygon<f64>>, MultiPolygon<f64>)> = None;
    for k in [1.15f32, 1.45, 1.8] {
        let d = ld * k;
        let mut bands: Vec<MultiPolygon<f64>> = Vec::new();
        let mut claimed: Option<MultiPolygon<f64>> = None;
        for (ch, _) in &runs {
            let strip = run_band(&pts, ch, d);
            let mut band = poly.intersection(&strip);
            if let Some(c) = &claimed {
                band = band.difference(c);
            }
            claimed = Some(match claimed {
                None => strip,
                Some(c) => c.union(&strip),
            });
            bands.push(band);
        }
        let core = match &claimed {
            Some(c) => poly.difference(c),
            None => MultiPolygon(vec![poly.clone()]),
        };
        let thin = core.0.iter().any(|p| area_f(&p) >= MIN_AREA && thickness(p) < ld * 0.6);
        chosen = Some((bands, core));
        if !thin {
            break;
        }
    }
    let (bands, core) = chosen.unwrap();
    let core_area: f32 = core.0.iter().map(|p| area_f(&p)).sum();
    let band_area: f32 = bands.iter().flat_map(|m| m.0.iter()).map(|p| area_f(&p)).sum();

    if core_area >= MIN_AREA && band_area >= MIN_AREA && band_area < area * 0.995 {
        for band in bands {
            for piece in band.0 {
                subdivide(piece, lu, roads, rng, depth + 1, out);
            }
        }
        for piece in core.0 {
            // No frontage left here: unusable ground, kept whole.
            let p = from_geo(&piece);
            if area_f(&piece) >= MIN_AREA && p.len() >= 3 {
                out.push(p);
            }
        }
        return;
    }

    // A long thin piece whose frontages are only at its ends (a strip
    // squeezed between two roads' rows): halve it across its long axis.
    {
        let n = pts.len();
        let (mut li, mut ll) = (0, 0.0);
        for i in 0..n {
            let l = pts[i].distance(pts[(i + 1) % n]);
            if l > ll {
                li = i;
                ll = l;
            }
        }
        let axis = (pts[(li + 1) % n] - pts[li]).normalize_or_zero();
        if axis != Vec2::ZERO {
            let (u0, u1) = extents(&pts, axis);
            let perp = Vec2::new(-axis.y, axis.x);
            let (v0, v1) = extents(&pts, perp);
            let long = u1 - u0;
            let short = v1 - v0;
            if chain_len < lw * 1.7 && long > lw * 2.2 && long > short * 2.5 {
                let origin = axis * ((u0 + u1) * 0.5) + perp * ((v0 + v1) * 0.5);
                let pieces = cut(&poly, origin, axis);
                if pieces.len() >= 2 {
                    for piece in pieces {
                        subdivide(piece, lu, roads, rng, depth + 1, out);
                    }
                    return;
                }
            }
        }
    }

    // Already a strip along its frontage: cut across at lot width. A cut
    // that would leave a sliver is moved along the road and retried.
    if chain_len > lw * 1.7 && area > lw * ld * 0.9 {
        // A long run is halved first, so the recursion stays shallow and
        // the remainder of every cut is itself a well-formed strip.
        if chain_len > lw * 8.0 {
            let pieces = cut_along_run(&poly, &pts, &chain, chain_len * rng.random_range(0.45..0.55), rng);
            if pieces.len() >= 2 && pieces.iter().all(|p| area_f(p) >= MIN_AREA) {
                for piece in pieces {
                    subdivide(piece, lu, roads, rng, depth + 1, out);
                }
                return;
            }
        }
        // Walk along the frontage until the piece cut off is about a lot's
        // area; at a block corner this turns the narrow tip into a corner lot
        // rather than a sliver. Stop at the middle so the rest stays sliceable.
        let target = lw * ld * rng.random_range(0.8..1.2);
        let mut s = lw * rng.random_range(0.7..1.1);
        while s < chain_len * 0.55 {
            let pieces = cut_along_run(&poly, &pts, &chain, s, rng);
            if pieces.len() >= 2 {
                let small = pieces.iter().map(|p| area_f(&p)).fold(f32::MAX, f32::min);
                // Compactness is asked only of pieces that are final: a
                // remainder still several lots long is cut again and may
                // be as thin as a row of lots is.
                let compact_ok = pieces.iter().all(|p| {
                    let q = from_geo(p);
                    let a = area_f(&p);
                    let more_to_cut = a >= lw * ld * 2.5 && frontage_runs(&q, roads, allow).first().is_some_and(|(_, l)| *l >= lw * 2.0);
                    a < MIN_AREA * 0.5 || more_to_cut || a / perimeter(&q).powi(2) >= MIN_COMPACTNESS * 0.9
                });
                let frontage_ok = pieces.iter().all(|p| {
                    let q = from_geo(p);
                    area_f(&p) < MIN_AREA * 0.5 || frontage_runs(&q, roads, allow).first().map_or(false, |(_, l)| *l >= lw * 0.55)
                });
                let wide_enough = pieces.iter().all(|p| {
                    let q = from_geo(p);
                    area_f(&p) < MIN_AREA * 0.5 || frontage_runs(&q, roads, allow).first().map_or(false, |(_, l)| *l >= lw * 0.8)
                });
                if (small >= target * 0.75 || (small >= MIN_AREA * 1.6 && wide_enough)) && compact_ok && frontage_ok {
                    for piece in pieces {
                        subdivide(piece, lu, roads, rng, depth + 1, out);
                    }
                    return;
                }
            }
            s += lw * 0.35;
        }
    }
    out.push(pts);
}


/// Does a parcel have a usable road edge?
fn fronted(poly: &[Vec2], lu: LandUse, roads: &RoadNetwork) -> bool {
    let (fi, road) = frontage_edge(poly, roads, lu.fronts_paths());
    let n = poly.len();
    road.is_some() && poly[fi].distance(poly[(fi + 1) % n]) >= MIN_FRONTAGE.min(lu.lot_width() * 0.45)
}

/// Backland (pieces with no road edge, or slivers) is folded into the
/// neighbouring usable parcel that shares the longest boundary, as long as
/// the result stays convex and no more than three lots deep. Real
/// subdivisions do the same: lots run back until they meet.
fn merge_backland(mut parcels: Vec<Vec<Vec2>>, lu: LandUse, roads: &RoadNetwork) -> Vec<Vec<Vec2>> {
    let lw = lu.lot_width();
    let ld = lu.lot_depth();
    let max_area = lw * ld * 3.2;
    for _ in 0..4 {
        let n = parcels.len();
        let good: Vec<bool> = parcels
            .iter()
            .map(|p| fronted(p, lu, roads) && reflex_vertex(p, 0.6).is_none() && polygon_area(p).abs() / perimeter(p).powi(2) >= MIN_COMPACTNESS)
            .collect();
        let boxes: Vec<(Vec2, Vec2)> = parcels.iter().map(|p| bounds_of(p)).collect();
        let mut removed = vec![false; n];
        let mut merged_any = false;
        for i in 0..n {
            if good[i] || removed[i] || reflex_vertex(&parcels[i], 0.6).is_some() {
                continue;
            }
            let per_i = perimeter(&parcels[i]);
            let gi = to_geo(&parcels[i]);
            let mut best: Option<(f32, usize, Vec<Vec2>)> = None;
            for j in 0..n {
                if i == j || !good[j] || removed[j] {
                    continue;
                }
                if !boxes_overlap(boxes[i], boxes[j], 0.5) {
                    continue;
                }
                let u: MultiPolygon<f64> = gi.union(&to_geo(&parcels[j]));
                if u.0.len() != 1 {
                    continue;
                }
                let up = from_geo(&u.0[0]);
                if up.len() < 3 || polygon_area(&up) > max_area {
                    continue;
                }
                let shared = (per_i + perimeter(&parcels[j]) - perimeter(&up)) * 0.5;
                if shared < 3.0 || reflex_vertex(&up, 0.6).is_some() {
                    continue;
                }
                let (fi, _) = frontage_edge(&up, roads, lu.fronts_paths());
                let m = up.len();
                let along = (up[(fi + 1) % m] - up[fi]).normalize_or_zero();
                let inward = Vec2::new(-along.y, along.x);
                let (v0, v1) = extents(&up, inward);
                if v1 - v0 > ld * 3.0 {
                    continue;
                }
                if best.as_ref().map_or(true, |(b, _, _)| shared > *b) {
                    best = Some((shared, j, up));
                }
            }
            if let Some((_, j, up)) = best {
                parcels[j] = up;
                removed[i] = true;
                merged_any = true;
            }
        }
        parcels = parcels.into_iter().zip(removed).filter(|(_, r)| !r).map(|(p, _)| p).collect();
        if !merged_any {
            break;
        }
    }
    parcels
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

/// Carve `zone` into parcels: subtract every road corridor and the ground
/// taken by the `covering` zones (drawn later, so they win), slice the
/// blocks along their frontages, and keep parcels whose centre is not inside
/// a covering zone or an existing lot. Parcels with no road frontage or a
/// sliver shape are kept as landlocked, unusable ground. New lots are
/// appended to `lots`. A self-crossing outline is not a region and yields
/// nothing: boolean ops on it would produce garbage parcels.
pub fn generate_lots(
    zone_id: ZoneId,
    zone: &Zone,
    roads: &RoadNetwork,
    lots: &mut SlotMap<LotId, Lot>,
    covering: &[Zone],
) -> Vec<LotId> {
    generate_lots_wet(zone_id, zone, roads, lots, covering, &WaterMask::default())
}

/// Water to keep parcels off: polygons (with holes) and their boxes.
#[derive(Clone, Default)]
pub struct WaterMask {
    polys: Vec<((Vec2, Vec2), Polygon<f64>)>,
}

impl WaterMask {
    /// From outlines with holes, as `Landscape::water_polygons` gives them.
    pub fn new(polys: &[(Vec<Vec2>, Vec<Vec<Vec2>>)]) -> Self {
        let mut out = Vec::new();
        for (outer, holes) in polys {
            if outer.len() < 3 || geom::self_intersects(outer) {
                continue;
            }
            let ring = |r: &[Vec2]| LineString::from(ccw(r).iter().map(|p| Coord { x: p.x as f64, y: p.y as f64 }).collect::<Vec<_>>());
            let poly = Polygon::new(ring(outer), holes.iter().filter(|h| h.len() >= 3).map(|h| ring(h)).collect());
            out.push((bounds_of(outer), poly));
        }
        WaterMask { polys: out }
    }

    pub fn is_empty(&self) -> bool {
        self.polys.is_empty()
    }

    /// The water touching a box, as one multipolygon.
    fn near(&self, lo: Vec2, hi: Vec2) -> Option<MultiPolygon<f64>> {
        let mut acc: Option<MultiPolygon<f64>> = None;
        for (b, p) in &self.polys {
            if !boxes_overlap(*b, (lo, hi), 1.0) {
                continue;
            }
            acc = Some(match acc {
                None => MultiPolygon(vec![p.clone()]),
                Some(m) => m.union(p),
            });
        }
        acc
    }
}

/// [`generate_lots`] that also keeps parcels off the water.
pub fn generate_lots_wet(
    zone_id: ZoneId,
    zone: &Zone,
    roads: &RoadNetwork,
    lots: &mut SlotMap<LotId, Lot>,
    covering: &[Zone],
    water: &WaterMask,
) -> Vec<LotId> {
    let lu = zone.land_use;
    let mut created = Vec::new();
    if zone.polygon.len() < 3 || roads.is_empty() || geom::self_intersects(&zone.polygon) {
        return created;
    }
    let mut rng = crate::rng_for(polygon_seed(&zone.polygon), 5);
    let zone_poly = to_geo(&ccw(&zone.polygon));
    let (zlo, zhi) = bounds_of(&zone.polygon);

    // Blocks: the zone minus every nearby road corridor (one smooth polygon
    // per chain of road pieces).
    let mut corridors: Option<MultiPolygon<f64>> = None;
    for (pts, hw, ext_a, ext_b, _) in road_chains(roads) {
        let (lo, hi) = pts.iter().fold((Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        let (lo, hi) = (lo - Vec2::splat(hw * 2.0), hi + Vec2::splat(hw * 2.0));
        if hi.x < zlo.x || hi.y < zlo.y || lo.x > zhi.x || lo.y > zhi.y {
            continue;
        }
        let Some(c) = chain_corridor(&pts, hw, ext_a, ext_b) else { continue };
        corridors = Some(match corridors {
            None => MultiPolygon(vec![c]),
            Some(m) => m.union(&c),
        });
    }
    let mut blocks: MultiPolygon<f64> = match &corridors {
        Some(m) => zone_poly.difference(m),
        None => MultiPolygon(vec![zone_poly]),
    };
    // Ground under a newer zone is theirs: take it out before slicing, so
    // the parcels along the shared boundary stop at it instead of straddling
    // it. (The centre test below still guards against slivers of overlap.)
    let mut covered: Option<MultiPolygon<f64>> = None;
    for z in covering {
        if z.polygon.len() < 3 || geom::self_intersects(&z.polygon) || !boxes_overlap(bounds_of(&z.polygon), (zlo, zhi), 0.0) {
            continue;
        }
        let c = to_geo(&z.polygon);
        covered = Some(match covered {
            None => MultiPolygon(vec![c]),
            Some(m) => m.union(&c),
        });
    }
    if let Some(c) = &covered {
        blocks = blocks.difference(c);
    }
    // Rivers, lakes and the sea are nobody's parcel.
    if let Some(w) = water.near(zlo, zhi) {
        blocks = blocks.difference(&w);
    }

    let mut parcels: Vec<Vec<Vec2>> = Vec::new();
    for block in blocks.0 {
        subdivide(block, lu, roads, &mut rng, 0, &mut parcels);
    }
    let parcels = merge_backland(parcels, lu, roads);

    // Lots already there (other zones) by bounding box, so each parcel is
    // tested against its neighbours only. Lots made by this call are few
    // and checked directly.
    let existing: Vec<(LotId, (Vec2, Vec2))> = lots.iter().map(|(id, l)| (id, bounds_of(&l.polygon))).collect();
    let existing_grid = Grid::new(60.0, existing.iter().map(|(_, b)| *b));
    let mut mine: Vec<(LotId, (Vec2, Vec2))> = Vec::new();
    let shares_ground = |l: &Lot, center: Vec2, poly: &[Vec2]| l.contains(center) || point_in_polygon(l.center, poly);

    for poly in parcels {
        let poly = ccw(&poly);
        if poly.len() < 3 {
            continue;
        }
        let center = inner_center(&poly);
        if covering.iter().any(|z| z.contains(center)) {
            continue;
        }
        let pbox = bounds_of(&poly);
        let taken = existing_grid.in_box(pbox.0, pbox.1).into_iter().any(|i| shares_ground(&lots[existing[i as usize].0], center, &poly))
            || mine.iter().any(|(id, b)| boxes_overlap(*b, pbox, 0.0) && shares_ground(&lots[*id], center, &poly));
        if taken {
            continue;
        }
        let Some(lot) = make_lot(zone_id, lu, poly, center, roads) else { continue };
        let id = lots.insert(lot);
        mine.push((id, pbox));
        created.push(id);
    }
    created
}

/// A lot of land use `lu` on a parcel outline (counter-clockwise) with its
/// interior centre: frontage, the frontage-aligned rectangle, the street a
/// car reaches it from, and whether it is landlocked for that use (no
/// frontage it may use, a sharp inward corner, a sliver, too narrow).
/// None when there is no road anywhere to hang it on.
pub fn make_lot(zone_id: ZoneId, lu: LandUse, poly: Vec<Vec2>, center: Vec2, roads: &RoadNetwork) -> Option<Lot> {
    let area = polygon_area(&poly);
    let compact = area / perimeter(&poly).powi(2);
    let (fi, road) = frontage_edge(&poly, roads, lu.fronts_paths());
    let n = poly.len();
    let fa = poly[fi];
    let fb = poly[(fi + 1) % n];
    let along = (fb - fa).normalize_or_zero();
    if along == Vec2::ZERO {
        return None;
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
    #[cfg(test)]
    if landlocked {
        let reason = if !has_frontage { "nofront" } else if sharp { "sharp" } else if compact < MIN_COMPACTNESS { "compact" } else { "narrow" };
        DEAD_REASONS.with(|r| r.borrow_mut().push((center, reason, area, compact)));
    }
    let (sid, t) = match road {
        Some(hit) => hit,
        None => {
            let (s, t, _) = nearest_road(roads, center, true)?;
            (s, t)
        }
    };
    let fronts_street = roads.segment(sid).is_street();
    let entrance = fa.lerp(fb, 0.5);
    let (car_segment, car_point) = if fronts_street {
        (Some(sid), entrance)
    } else {
        match roads.nearest_street(entrance) {
            Some((cs, _, q)) if q.distance(entrance) < 260.0 => (Some(cs), q),
            _ => (None, entrance),
        }
    };
    Some(Lot {
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
        footfall_today: 0,
        footfall_hour: 0,
        car_segment,
        car_point,
        park: None,
    })
}

/// [free-placement] The lot under a building the player placed: ground the
/// city took out of the zoning. Its use is [`LandUse::Civic`]; its frontage
/// is a street where it has one, else the footpath it lines (a metro
/// entrance or a battery yard may stand on a path), so ground any zone could
/// use is ground a city building can use.
pub fn make_placed_lot(zone_id: ZoneId, poly: Vec<Vec2>, center: Vec2, roads: &RoadNetwork) -> Option<Lot> {
    let street = make_lot(zone_id, LandUse::Civic, poly.clone(), center, roads);
    let mut lot = match street {
        Some(l) if !l.landlocked => l,
        other => match make_lot(zone_id, LandUse::Residential, poly, center, roads) {
            Some(l) if !l.landlocked => l,
            fallback => other.or(fallback)?,
        },
    };
    lot.land_use = LandUse::Civic;
    Some(lot)
}

/// [free-placement] Does an outline front a street or footpath at all,
/// whatever its shape? A city building's recut lot keeps the building as
/// long as it does (a sliver or a sharp corner makes ground unfit to grow
/// on, not a placed plant or station any less reachable).
pub fn fronts_a_way(poly: &[Vec2], roads: &RoadNetwork) -> bool {
    poly.len() >= 3 && frontage_edge(poly, roads, true).1.is_some()
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

// ── parcels cut before zoning ───────────────────────────────────────────────

/// Every parcel is cut to a home's lot; uses that want wider lots join
/// neighbouring parcels ([`join_parcels`]).
pub const PARCEL_USE: LandUse = LandUse::Residential;
/// Parcels reach this far behind the edge of a street's or path's corridor.
pub const PARCEL_REACH: f32 = 60.0;
/// Landlocked pieces bigger than this many parcels are chopped up, so a
/// park can be painted onto backland a piece at a time.
const BACKLAND_PARCELS: f32 = 4.0;
/// Land enclosed by the reach of the roads around it is parcelled too, up
/// to this area (m²); bigger enclosures keep their open middle.
const ENCLOSED_FILL: f64 = 40_000.0;
/// Resolution of parcel keys, in metres.
const KEY_UNIT: f32 = 0.25;
/// Long straight frontages are first cut across where the coordinate along
/// the road is a multiple of this, so a block's parcels do not depend on
/// where its far end is.
const CHUNK: f32 = 96.0;

/// A parcel's identity across rebuilds: its centre and frontage edge on a
/// quarter-metre grid. Lot ids change every time the lots are rebuilt; a key
/// stays the same as long as the parcel does. A parcel reshaped by road
/// works is found again by the old centre ([`ParcelMap::find`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ParcelKey {
    pub center: (i32, i32),
    pub front: [(i32, i32); 2],
}

impl ParcelKey {
    pub fn new(center: Vec2, front: [Vec2; 2]) -> Self {
        let q = |p: Vec2| ((p.x / KEY_UNIT).round() as i32, (p.y / KEY_UNIT).round() as i32);
        Self { center: q(center), front: [q(front[0]), q(front[1])] }
    }

    pub fn center(&self) -> Vec2 {
        Vec2::new(self.center.0 as f32, self.center.1 as f32) * KEY_UNIT
    }
}

/// A piece of land along a road, cut whether or not it is zoned.
#[derive(Clone, Debug)]
pub struct Parcel {
    pub key: ParcelKey,
    /// Counter-clockwise outline.
    pub polygon: Vec<Vec2>,
    /// A point inside the outline (the centroid when that is inside).
    pub center: Vec2,
    pub front_edge: [Vec2; 2],
    pub area: f32,
    /// Has a frontage a home may use (a street or a footpath).
    pub path_frontage: bool,
    /// Has a street frontage, which shops, industry and the rest need.
    pub street_frontage: bool,
    /// Too steep to level, or standing in the water.
    pub rough: bool,
    /// Land use as built (from the master plan); None is unzoned.
    pub land_use: Option<LandUse>,
    /// The lot this parcel is part of, once zoned and built into one.
    pub lot: Option<LotId>,
}

impl Parcel {
    /// No frontage anyone may use, or rough ground: backland.
    pub fn landlocked(&self) -> bool {
        self.rough || !self.path_frontage
    }

    /// Why this parcel cannot take `lu`, or None if it can. The ruleset:
    /// parks go anywhere, backland and rough ground included (a park is
    /// reached through the park); homes need a street or path to front;
    /// everything else needs a street; industry and civic sites are not put
    /// on scraps, but their deep yards may take the backland behind them
    /// (it is folded into the lot in front, see [`join_parcels`]).
    pub fn refusal(&self, lu: LandUse) -> Option<&'static str> {
        if lu == LandUse::Park {
            return None;
        }
        if self.rough {
            return Some("too steep or wet to build on");
        }
        if !self.path_frontage {
            if lu.takes_backland() {
                return None;
            }
            return Some("landlocked: no street or path to front");
        }
        if !lu.fronts_paths() && !self.street_frontage {
            return Some("fronts only a footpath; this use needs a street");
        }
        if self.area < lu.min_parcel_area() {
            return Some("too small for this use");
        }
        None
    }

    pub fn accepts(&self, lu: LandUse) -> bool {
        self.refusal(lu).is_none()
    }

    pub fn contains(&self, p: Vec2) -> bool {
        point_in_polygon(p, &self.polygon)
    }
}

/// The parcels of the city with a key index and a spatial grid.
#[derive(Clone, Debug, Default)]
pub struct ParcelMap {
    pub parcels: Vec<Parcel>,
    by_key: HashMap<ParcelKey, usize>,
    grid: Grid,
    /// Bumped every time the parcels are cut again.
    pub version: u64,
}

impl ParcelMap {
    pub fn new(parcels: Vec<Parcel>, version: u64) -> Self {
        let by_key = parcels.iter().enumerate().map(|(i, p)| (p.key, i)).collect();
        let grid = Grid::new(48.0, parcels.iter().map(|p| bounds_of(&p.polygon)));
        ParcelMap { parcels, by_key, grid, version }
    }

    pub fn len(&self) -> usize {
        self.parcels.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parcels.is_empty()
    }

    /// The parcel under a point.
    pub fn at(&self, p: Vec2) -> Option<usize> {
        self.grid.in_box(p, p).into_iter().map(|i| i as usize).find(|&i| self.parcels[i].contains(p))
    }

    /// Parcels a brush of radius `r` at `c` touches.
    pub fn in_circle(&self, c: Vec2, r: f32) -> Vec<usize> {
        self.grid
            .in_box(c - Vec2::splat(r), c + Vec2::splat(r))
            .into_iter()
            .map(|i| i as usize)
            .filter(|&i| {
                let p = &self.parcels[i];
                p.contains(c) || distance_to_outline(c, &p.polygon) <= r
            })
            .collect()
    }

    /// Parcels whose centre lies inside `poly`.
    pub fn in_polygon(&self, poly: &[Vec2]) -> Vec<usize> {
        if poly.len() < 3 {
            return Vec::new();
        }
        let (lo, hi) = bounds_of(poly);
        self.grid.in_box(lo, hi).into_iter().map(|i| i as usize).filter(|&i| point_in_polygon(self.parcels[i].center, poly)).collect()
    }

    /// The parcel a key names: the same parcel if it is still there, else
    /// the one that now holds its old centre.
    pub fn find(&self, key: &ParcelKey) -> Option<usize> {
        self.by_key.get(key).copied().or_else(|| self.at(key.center()))
    }
}

/// Cut parcels along every street and path: the land within
/// [`PARCEL_REACH`] of the corridors, minus every corridor (track and
/// highway included) and the water, clipped to `clip`, each block sliced as
/// [`subdivide`] does for a home's lot. Long straight frontages are first
/// chunked at fixed world positions along the road ([`CHUNK`]), and every
/// chunk draws its own random stream from its own shape, so road works
/// elsewhere never move a parcel here: the same road gives the same
/// parcels.
pub fn generate_parcels(roads: &RoadNetwork, water: &WaterMask, clip: Option<(Vec2, Vec2)>) -> Vec<Parcel> {
    let mut out = Vec::new();
    if roads.is_empty() {
        return out;
    }
    let mut corridors: Vec<Polygon<f64>> = Vec::new();
    let mut reach: Vec<Polygon<f64>> = Vec::new();
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for (pts, hw, ext_a, ext_b, sid) in road_chains(roads) {
        if let Some(c) = chain_corridor(&pts, hw, ext_a, ext_b) {
            corridors.push(c);
        }
        let seg = roads.segment(sid);
        // Highways, their slip roads, tunnels and track front nothing, so
        // no parcels hang off them.
        if seg.is_rail() || seg.fronts_nothing() {
            continue;
        }
        let r = hw + PARCEL_REACH;
        for p in &pts {
            lo = lo.min(*p - Vec2::splat(r + hw));
            hi = hi.max(*p + Vec2::splat(r + hw));
        }
        reach_pieces(&pts, hw, r, ext_a, ext_b, &mut reach);
    }
    if reach.is_empty() {
        return out;
    }
    let mut region: MultiPolygon<f64> = geo::unary_union(&reach);
    // Ground the reach does not quite get to but that roads close round
    // (the middle of a big block) is still that block's: fill such holes,
    // or the block would be split around a speck of nobody's land.
    region = MultiPolygon(
        region
            .0
            .into_iter()
            .map(|p| {
                let keep: Vec<LineString<f64>> = p.interiors().iter().filter(|r| Polygon::new((*r).clone(), vec![]).unsigned_area() > ENCLOSED_FILL).cloned().collect();
                Polygon::new(p.exterior().clone(), keep)
            })
            .collect(),
    );
    let corridors: MultiPolygon<f64> = geo::unary_union(&corridors);
    region = region.difference(&corridors);
    if let Some((clo, chi)) = clip {
        region = region.intersection(&MultiPolygon(vec![to_geo(&[clo, Vec2::new(chi.x, clo.y), chi, Vec2::new(clo.x, chi.y)])]));
    }
    if let Some(w) = water.near(lo, hi) {
        region = region.difference(&w);
    }
    let mut polys: Vec<Vec<Vec2>> = Vec::new();
    for block in region.0 {
        if area_f(&block) < MIN_AREA {
            continue;
        }
        for chunk in chunk_block(block, roads) {
            let chunk = canonical(chunk);
            let mut rng = crate::rng_for(block_seed(&chunk), 5);
            let mut pieces: Vec<Vec<Vec2>> = Vec::new();
            subdivide(chunk, PARCEL_USE, roads, &mut rng, 0, &mut pieces);
            for piece in merge_backland(pieces, PARCEL_USE, roads) {
                chop_backland(piece, roads, 0, &mut polys);
            }
        }
    }
    for poly in polys {
        if let Some(p) = parcel_of(poly, roads) {
            out.push(p);
        }
    }
    out
}

/// Cut a block across every long straight frontage edge where the
/// coordinate along the edge's direction is a multiple of [`CHUNK`]. The
/// lines are fixed in the world, so the chunks away from a change keep
/// their exact shape.
fn chunk_block(block: Polygon<f64>, roads: &RoadNetwork) -> Vec<Polygon<f64>> {
    let pts = from_geo(&block);
    let n = pts.len();
    let mut lines: Vec<(Vec2, Vec2)> = Vec::new();
    let margin = PARCEL_USE.lot_width() * 1.5;
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        if a.distance(b) < CHUNK * 2.0 || edge_on_road(roads, a, b, true).is_none() {
            continue;
        }
        let d = (b - a).normalize_or_zero();
        // Canonical direction, so both sides of a road agree on the lines.
        let d = if d.x < -1e-6 || (d.x.abs() <= 1e-6 && d.y < 0.0) { -d } else { d };
        let (s0, s1) = (a.dot(d).min(b.dot(d)), a.dot(d).max(b.dot(d)));
        let mut k = ((s0 + margin) / CHUNK).ceil() as i64;
        while (k as f32) * CHUNK <= s1 - margin {
            let s = k as f32 * CHUNK;
            let t = (s - a.dot(d)) / (b - a).dot(d);
            lines.push((a.lerp(b, t), d));
            k += 1;
        }
    }
    let mut pieces = vec![block];
    for (origin, normal) in lines {
        let mut next = Vec::new();
        for piece in pieces {
            let (lo, hi) = extents(&from_geo(&piece), normal);
            let at = origin.dot(normal);
            if at - lo > 1.0 && hi - at > 1.0 {
                let cut_pieces = cut(&piece, origin, normal);
                if cut_pieces.len() >= 2 {
                    next.extend(cut_pieces);
                    continue;
                }
            }
            next.push(piece);
        }
        pieces = next;
    }
    pieces
}

/// Ground within `r` of a road's centreline, as overlapping convex pieces
/// (a rectangle per leg, a disc at every bend) for one robust union. The
/// ends stop where the corridor does, so a dead end grows no backland cap.
fn reach_pieces(pts: &[Vec2], hw: f32, r: f32, ext_start: bool, ext_end: bool, out: &mut Vec<Polygon<f64>>) {
    if pts.len() < 2 {
        return;
    }
    let mut p = pts.to_vec();
    let n = p.len();
    let d0 = (p[1] - p[0]).normalize_or_zero();
    let d1 = (p[n - 1] - p[n - 2]).normalize_or_zero();
    p[0] -= d0 * if ext_start { hw } else { 0.5 };
    p[n - 1] += d1 * if ext_end { hw } else { 0.5 };
    for w in p.windows(2) {
        let d = (w[1] - w[0]).normalize_or_zero();
        if d == Vec2::ZERO {
            continue;
        }
        let nrm = Vec2::new(-d.y, d.x) * r;
        out.push(to_geo(&[w[0] - nrm, w[1] - nrm, w[1] + nrm, w[0] + nrm]));
    }
    for &c in &p[1..n - 1] {
        let ring: Vec<Vec2> = (0..16).map(|k| {
            let a = k as f32 / 16.0 * std::f32::consts::TAU;
            c + Vec2::new(a.cos(), a.sin()) * r
        }).collect();
        out.push(to_geo(&ring));
    }
}

/// The same block always starts at the same corner, so the same shape is
/// always cut the same way whatever the boolean ops handed back.
fn canonical(p: Polygon<f64>) -> Polygon<f64> {
    let ring = from_geo(&p);
    if ring.len() < 3 {
        return p;
    }
    let k = (0..ring.len()).min_by(|&a, &b| ring[a].x.total_cmp(&ring[b].x).then(ring[a].y.total_cmp(&ring[b].y))).unwrap_or(0);
    let coords: Vec<Coord<f64>> = ring[k..].iter().chain(ring[..k].iter()).map(|q| Coord { x: q.x as f64, y: q.y as f64 }).collect();
    Polygon::new(LineString::from(coords), p.interiors().to_vec())
}

/// Seed from a block's area and bounding box, rounded to the metre: the
/// same land gives the same stream.
fn block_seed(p: &Polygon<f64>) -> u64 {
    let ring = from_geo(p);
    let (lo, hi) = bounds_of(&ring);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for v in [area_f(p), lo.x, lo.y, hi.x, hi.y] {
        h ^= v.round() as i64 as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// Halve big landlocked pieces across their long axis until each is a few
/// parcels' worth.
fn chop_backland(poly: Vec<Vec2>, roads: &RoadNetwork, depth: u32, out: &mut Vec<Vec<Vec2>>) {
    let limit = PARCEL_USE.lot_width() * PARCEL_USE.lot_depth() * BACKLAND_PARCELS;
    if depth > 10 || poly.len() < 3 || polygon_area(&poly).abs() <= limit || fronted(&poly, PARCEL_USE, roads) {
        out.push(poly);
        return;
    }
    let pts = ccw(&poly);
    let n = pts.len();
    let (mut li, mut ll) = (0, 0.0);
    for i in 0..n {
        let l = pts[i].distance(pts[(i + 1) % n]);
        if l > ll {
            li = i;
            ll = l;
        }
    }
    let axis = (pts[(li + 1) % n] - pts[li]).normalize_or_zero();
    let (u0, u1) = extents(&pts, axis);
    let perp = Vec2::new(-axis.y, axis.x);
    let (v0, v1) = extents(&pts, perp);
    let origin = axis * ((u0 + u1) * 0.5) + perp * ((v0 + v1) * 0.5);
    let pieces = cut(&to_geo(&pts), origin, axis);
    if pieces.len() < 2 {
        out.push(pts);
        return;
    }
    for piece in pieces {
        chop_backland(from_geo(&piece), roads, depth + 1, out);
    }
}

/// A parcel from an outline, with its frontage judged for homes and for
/// everything else. None when there is no road anywhere to hang it on.
fn parcel_of(poly: Vec<Vec2>, roads: &RoadNetwork) -> Option<Parcel> {
    let poly = ccw(&poly);
    if poly.len() < 3 {
        return None;
    }
    let center = inner_center(&poly);
    let home = make_lot(ZoneId::default(), PARCEL_USE, poly.clone(), center, roads)?;
    let street = make_lot(ZoneId::default(), LandUse::Commercial, poly.clone(), center, roads).is_some_and(|l| !l.landlocked);
    Some(Parcel {
        key: ParcelKey::new(center, home.front_edge),
        area: polygon_area(&poly).abs(),
        center,
        front_edge: home.front_edge,
        path_frontage: !home.landlocked,
        street_frontage: street,
        rough: false,
        land_use: None,
        lot: None,
        polygon: poly,
    })
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

/// Width along the frontage `lu` would use, or None with no frontage.
fn frontage_width(poly: &[Vec2], lu: LandUse, roads: &RoadNetwork) -> Option<f32> {
    let (fi, road) = frontage_edge(poly, roads, lu.fronts_paths());
    road?;
    let n = poly.len();
    let along = (poly[(fi + 1) % n] - poly[fi]).normalize_or_zero();
    if along == Vec2::ZERO {
        return None;
    }
    let (u0, u1) = extents(poly, along);
    Some(u1 - u0)
}

/// Drop vertices where the outline runs on nearly straight, so two joined
/// parcels have one frontage edge rather than two.
fn simplify_collinear(poly: &[Vec2]) -> Vec<Vec2> {
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

/// [plant-scale] How thick a piece of ground is: twice its area over its
/// perimeter (a square's half side, a long strip's width).
pub fn site_thickness(poly: &[Vec2]) -> f32 {
    let per = perimeter(poly);
    if per <= 0.0 { 0.0 } else { 2.0 * polygon_area(poly).abs() / per }
}

/// [plant-scale] A site for a building the player puts on unzoned ground:
/// parcel `start` joined with as few of its neighbours in `polys` as it
/// takes to reach `need` square metres with a frontage (a street or a
/// path), never past `cap`. Each step takes the neighbour that reaches the
/// need with the smallest site, else the one sharing the longest boundary
/// (compact sites, not strings of parcels); the joined outline stays one
/// convex-ish piece. Returns the members (indices into `polys`, sorted) and
/// the outline; short of `need` when the neighbours run out.
pub fn grow_site(polys: &[Vec<Vec2>], start: usize, need: f32, min_thick: f32, cap: f32, roads: &RoadNetwork) -> (Vec<usize>, Vec<Vec2>) {
    // [plant-scale] "Fronted" also means thick enough to build on.
    let fronted = |p: &[Vec2]| frontage_width(p, PARCEL_USE, roads).is_some() && site_thickness(p) >= min_thick;
    let mut members = vec![start];
    let mut outline = polys[start].clone();
    for _ in 0..12 {
        let area = polygon_area(&outline).abs();
        let has_front = fronted(&outline);
        if area >= need && has_front {
            break;
        }
        let bo = bounds_of(&outline);
        let go = to_geo(&outline);
        let per = perimeter(&outline);
        // (rank, score, index, outline): a site that reaches the need with a
        // frontage ranks 2 and scores smaller-is-better; otherwise one that
        // gains a frontage ranks 1, the rest 0, scored by shared boundary.
        let mut best: Option<(u8, f32, usize, Vec<Vec2>)> = None;
        for (j, pj) in polys.iter().enumerate() {
            if members.contains(&j) || pj.len() < 3 || !boxes_overlap(bo, bounds_of(pj), 0.5) {
                continue;
            }
            let u: MultiPolygon<f64> = go.union(&to_geo(pj));
            if u.0.len() != 1 || !u.0[0].interiors().is_empty() {
                continue;
            }
            let up = simplify_collinear(&from_geo(&u.0[0]));
            if up.len() < 3 || reflex_vertex(&up, 0.6).is_some() {
                continue;
            }
            let shared = (per + perimeter(pj) - perimeter(&up)) * 0.5;
            let au = polygon_area(&up).abs();
            if shared < 4.0 || au > cap {
                continue;
            }
            let front = fronted(&up);
            let (rank, score) = if au >= need && front { (2u8, -au) } else { (u8::from(front && !has_front), shared) };
            if best.as_ref().is_none_or(|b| (rank, score) > (b.0, b.1)) {
                best = Some((rank, score, j, up));
            }
        }
        let Some((_, _, j, up)) = best else { break };
        members.push(j);
        outline = up;
    }
    members.sort();
    (members, outline)
}

/// Join neighbouring parcels given the same land use into lots about the
/// size that use wants (works yards and civic sites span two or three
/// parcels, a shop or a club takes a small neighbour). A piece under three
/// quarters of the use's lot area takes the neighbour that keeps the joined
/// lot smallest, as long as the result is one convex-ish piece with
/// frontage, at most 1.35 lot areas and 1.6 lot widths. Then backland
/// (pieces with no frontage this use can take) is folded into the
/// neighbouring lot sharing the longest boundary, while the result stays
/// convex-ish, under 2.8 lot areas and three lots deep; what cannot be
/// folded stays a landlocked lot of its own. Returns the members of each
/// lot (indices into `polys`) and its outline. Uses that do not join get
/// one lot per parcel.
pub fn join_parcels(polys: &[Vec<Vec2>], lu: LandUse, roads: &RoadNetwork) -> Vec<(Vec<usize>, Vec<Vec2>)> {
    let mut groups: Vec<Option<(Vec<usize>, Vec<Vec2>)>> = polys.iter().enumerate().map(|(i, p)| Some((vec![i], p.clone()))).collect();
    if lu.joins_parcels() {
        let lw = lu.lot_width();
        let target = lu.lot_area();
        for _ in 0..4 {
            let mut merged = false;
            for i in 0..groups.len() {
                let Some((_, pi)) = groups[i].clone() else { continue };
                if frontage_width(&pi, lu, roads).is_none() || polygon_area(&pi).abs() >= target * 0.75 {
                    continue;
                }
                let bi = bounds_of(&pi);
                let gi = to_geo(&pi);
                let per_i = perimeter(&pi);
                let mut best: Option<(f32, usize, Vec<Vec2>)> = None;
                for (j, g) in groups.iter().enumerate() {
                    if i == j {
                        continue;
                    }
                    let Some((_, pj)) = g else { continue };
                    if !boxes_overlap(bi, bounds_of(pj), 0.5) {
                        continue;
                    }
                    let u: MultiPolygon<f64> = gi.union(&to_geo(pj));
                    if u.0.len() != 1 || !u.0[0].interiors().is_empty() {
                        continue;
                    }
                    let up = simplify_collinear(&from_geo(&u.0[0]));
                    if up.len() < 3 {
                        continue;
                    }
                    let shared = (per_i + perimeter(pj) - perimeter(&up)) * 0.5;
                    if shared < 4.0 || reflex_vertex(&up, 0.6).is_some() {
                        continue;
                    }
                    let Some(wu) = frontage_width(&up, lu, roads) else { continue };
                    let au = polygon_area(&up).abs();
                    if wu > lw * 1.6 || au > target * 1.35 {
                        continue;
                    }
                    if best.as_ref().map_or(true, |b| au < b.0) {
                        best = Some((au, j, up));
                    }
                }
                if let Some((_, j, up)) = best {
                    let (mj, _) = groups[j].take().unwrap();
                    let g = groups[i].as_mut().unwrap();
                    g.0.extend(mj);
                    g.0.sort();
                    g.1 = up;
                    merged = true;
                }
            }
            if !merged {
                break;
            }
        }
    }
    if lu.takes_backland() {
        let ld = lu.lot_depth();
        let max_area = lu.lot_area() * 2.8;
        for _ in 0..4 {
            let mut merged = false;
            for i in 0..groups.len() {
                let Some((_, pi)) = groups[i].clone() else { continue };
                if frontage_width(&pi, lu, roads).is_some() {
                    continue;
                }
                let bi = bounds_of(&pi);
                let gi = to_geo(&pi);
                let per_i = perimeter(&pi);
                let mut best: Option<(f32, usize, Vec<Vec2>)> = None;
                for (j, g) in groups.iter().enumerate() {
                    if i == j {
                        continue;
                    }
                    let Some((_, pj)) = g else { continue };
                    if !boxes_overlap(bi, bounds_of(pj), 0.5) || frontage_width(pj, lu, roads).is_none() {
                        continue;
                    }
                    let u: MultiPolygon<f64> = gi.union(&to_geo(pj));
                    if u.0.len() != 1 || !u.0[0].interiors().is_empty() {
                        continue;
                    }
                    let up = simplify_collinear(&from_geo(&u.0[0]));
                    if up.len() < 3 || polygon_area(&up).abs() > max_area || reflex_vertex(&up, 0.6).is_some() {
                        continue;
                    }
                    let shared = (per_i + perimeter(pj) - perimeter(&up)) * 0.5;
                    if shared < 3.0 {
                        continue;
                    }
                    let (fi, _) = frontage_edge(&up, roads, lu.fronts_paths());
                    let m = up.len();
                    let along = (up[(fi + 1) % m] - up[fi]).normalize_or_zero();
                    let (v0, v1) = extents(&up, Vec2::new(-along.y, along.x));
                    if v1 - v0 > ld * 3.0 {
                        continue;
                    }
                    if best.as_ref().map_or(true, |b| shared > b.0) {
                        best = Some((shared, j, up));
                    }
                }
                if let Some((_, j, up)) = best {
                    let (mi, _) = groups[i].take().unwrap();
                    let g = groups[j].as_mut().unwrap();
                    g.0.extend(mi);
                    g.0.sort();
                    g.1 = up;
                    merged = true;
                }
            }
            if !merged {
                break;
            }
        }
    }
    groups.into_iter().flatten().collect()
}

#[cfg(test)]
thread_local! {
    static DEAD_REASONS: std::cell::RefCell<Vec<(Vec2, &'static str, f32, f32)>> = std::cell::RefCell::new(Vec::new());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [tools] The outline Harvey's geothermal plant's parcels joined into
    /// on a recut: a slit out to a tip and straight back where two shared
    /// edges did not quite match. It read as a sliver, the recut lot was
    /// landlocked, and the plant was lost to a road 100 m away.
    #[test]
    fn a_joined_outline_has_no_slit() {
        let v = |x: f32, y: f32| Vec2::new(x, y);
        let slit = vec![v(-516.75287, -221.87045), v(-507.64688, -247.44885), v(-467.52332, -231.34691), v(-470.71185, -222.39044), v(-503.92023, -234.21283), v(-470.71185, -222.39044), v(-474.94632, -210.49606)];
        let compact = |p: &[Vec2]| polygon_area(p).abs() / perimeter(p).powi(2);
        assert!(compact(&slit) < MIN_COMPACTNESS, "the slit reads as a sliver");
        let clean = simplify_collinear(&slit);
        assert_eq!(clean.len(), 4, "{clean:?}");
        assert!(compact(&clean) > MIN_COMPACTNESS && reflex_vertex(&clean, 0.6).is_none(), "{clean:?}");
        assert!((polygon_area(&clean).abs() - polygon_area(&slit).abs()).abs() < 1.0, "no ground lost");
    }

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
            let (_, _, d) = nearest_road(&roads, mid, false).unwrap();
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
        dump_lots(&roads, &lots, "ND_DUMP2");
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

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<Vec2> {
        vec![Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)]
    }

    #[test]
    fn partially_overlapping_zones_do_not_share_ground() {
        let roads = roads_cross();
        // An older residential zone in the corner of the cross, and a newer
        // commercial one drawn over its eastern part; both front the road
        // along y = 0.
        let older = Zone { polygon: rect(10.0, 10.0, 150.0, 150.0), land_use: LandUse::Residential };
        let newer = Zone { polygon: rect(90.0, 10.0, 230.0, 170.0), land_use: LandUse::Commercial };
        let mut lots = SlotMap::with_key();
        let new_ids = generate_lots(ZoneId::default(), &newer, &roads, &mut lots, &[]);
        let old_ids = generate_lots(ZoneId::default(), &older, &roads, &mut lots, &[newer.clone()]);
        assert!(new_ids.iter().any(|&id| lots[id].usable()) && old_ids.iter().any(|&id| lots[id].usable()));
        // The older zone's parcels stop at the newer zone's edge instead of
        // running under it.
        for &id in &old_ids {
            for p in &lots[id].polygon {
                assert!(p.x <= 90.0 + 0.05, "old-zone parcel vertex {p:?} lies inside the newer zone");
            }
        }
        // No two usable parcels overlap by more than a sliver.
        let good = usable(&lots);
        for i in 0..good.len() {
            for j in (i + 1)..good.len() {
                let inter = to_geo(&good[i].polygon).intersection(&to_geo(&good[j].polygon)).unsigned_area() as f32;
                let smaller = good[i].area().min(good[j].area());
                assert!(inter < 1.0 && inter < smaller * 0.01, "parcels overlap by {inter:.1} m2 of {smaller:.0}: {:?} / {:?}", good[i].center, good[j].center);
            }
        }
        // Both zones still get their row along each frontage: about 280 m
        // of frontage on the older zone and 140 m on the newer, a lot deep.
        let total: f32 = good.iter().map(|l| l.area()).sum();
        assert!(total > 420.0 * 24.0 * 0.6, "usable {total}");
    }

    #[test]
    fn a_self_crossing_zone_yields_nothing_instead_of_garbage() {
        let roads = roads_cross();
        let bow = Zone { polygon: vec![Vec2::new(10.0, 10.0), Vec2::new(150.0, 150.0), Vec2::new(150.0, 10.0), Vec2::new(10.0, 150.0)], land_use: LandUse::Residential };
        let mut lots = SlotMap::with_key();
        assert!(generate_lots(ZoneId::default(), &bow, &roads, &mut lots, &[]).is_empty());
        // As a covering zone it is ignored for the ground subtraction and
        // only its centre test applies: the zone under it still parcels.
        let zone = Zone { polygon: rect(10.0, 10.0, 150.0, 150.0), land_use: LandUse::Commercial };
        let ids = generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[bow]);
        assert!(!ids.is_empty());
        assert!(ids.iter().all(|&id| polygon_area(&lots[id].polygon) > 0.0 && lots[id].polygon.iter().all(|p| p.is_finite())));
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
    fn long_frontages_parcel_all_the_way_along() {
        // A single-sided frontage of 400 m and 1200 m used to parcel into
        // nothing: no cut passed the compactness test of its remainder.
        for &len in &[400.0f32, 1200.0] {
            let mut roads = RoadNetwork::default();
            roads.add_polyline(&[Vec2::new(-len, 0.0), Vec2::new(len, 0.0)], 1, 1);
            for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Industrial, LandUse::Entertainment, LandUse::Civic, LandUse::Park] {
                let half = len * 0.5;
                let zone = Zone {
                    polygon: vec![Vec2::new(-half, 10.0), Vec2::new(half, 10.0), Vec2::new(half, 70.0), Vec2::new(-half, 70.0)],
                    land_use: lu,
                };
                let mut lots = SlotMap::with_key();
                generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
                let n = usable(&lots).len();
                let expect = (len / lu.lot_width() * 0.5) as usize;
                assert!(n >= expect, "{lu:?} on {len} m: {n} usable lots, expected at least {expect}");
                let spread = usable(&lots).iter().map(|l| l.center.x).fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
                assert!(spread.1 - spread.0 > len * 0.8, "{lu:?} on {len} m: lots cover only {:.0}..{:.0}", spread.0, spread.1);
            }
        }
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
    fn homes_line_paths_but_shops_do_not() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        roads.add_polyline_kind(&[Vec2::new(-300.0, 80.0), Vec2::new(300.0, 80.0)], 0, 0, crate::roads::RoadKind::Path);
        // Homes: lots front the path too, without car access, and know the nearest street.
        let homes = Zone {
            polygon: vec![Vec2::new(-80.0, 10.0), Vec2::new(80.0, 10.0), Vec2::new(80.0, 150.0), Vec2::new(-80.0, 150.0)],
            land_use: LandUse::Residential,
        };
        let mut lots = SlotMap::with_key();
        generate_lots(ZoneId::default(), &homes, &roads, &mut lots, &[]);
        let on_path: Vec<&Lot> = lots.values().filter(|l| l.usable() && !l.car_access()).collect();
        assert!(!on_path.is_empty(), "homes line the path");
        assert!(on_path.iter().all(|l| l.car_segment.is_some() && (l.car_point.y).abs() < 1.0), "their car point is on the street");
        assert!(lots.values().filter(|l| l.usable()).any(|l| l.center.y > 90.0), "land beyond the path is used");
        // Shops: the path is not frontage.
        let zone = Zone {
            polygon: vec![Vec2::new(-80.0, 10.0), Vec2::new(80.0, 10.0), Vec2::new(80.0, 150.0), Vec2::new(-80.0, 150.0)],
            land_use: LandUse::Commercial,
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

    #[test]
    fn s_curve_through_a_block_keeps_most_land_usable() {
        let mut roads = RoadNetwork::default();
        for c in [-70.0f32, 70.0] {
            roads.add_polyline(&[Vec2::new(-300.0, c), Vec2::new(300.0, c)], 1, 1);
            roads.add_polyline(&[Vec2::new(c, -300.0), Vec2::new(c, 300.0)], 1, 1);
        }
        let curve = crate::plan::sample_curve(&[Vec2::new(-20.0, -70.0), Vec2::new(-35.0, -20.0), Vec2::new(25.0, 20.0), Vec2::new(10.0, 70.0)]);
        roads.add_polyline(&curve, 1, 1);
        let zone = Zone {
            polygon: vec![Vec2::new(-65.0, -65.0), Vec2::new(65.0, -65.0), Vec2::new(65.0, 65.0), Vec2::new(-65.0, 65.0)],
            land_use: LandUse::Residential,
        };
        let mut lots = SlotMap::with_key();
        generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
        let good: Vec<&Lot> = lots.values().filter(|l| l.usable()).collect();
        let usable_area: f32 = good.iter().map(|l| l.area()).sum();
        let curve_len: f32 = curve.windows(2).map(|w| w[0].distance(w[1])).sum();
        let block_area = 122.0 * 122.0 - 140.0 * 2.0 * 8.95 - curve_len * 2.0 * 8.95;
        let dead: Vec<f32> = lots.values().filter(|l| l.landlocked).map(|l| l.area()).collect();
        assert!(good.len() >= 28, "only {} usable parcels around the curve; usable {usable_area:.0} of {block_area:.0}; dead {:?}", good.len(), dead);
        assert!(usable_area > block_area * 0.6, "usable {usable_area} of {block_area}");
        // Lots along the curve are not too deep or too shallow.
        for l in &good {
            let (w, d) = (l.width(), l.depth());
            assert!(d > 7.0 && d < 24.0 * 3.2, "odd lot depth {d}");
            assert!(w < 14.0 * 3.6, "lot too wide: {w} (a wedge spanning the block)");
        }
    }

    #[test]
    fn dump_s_curve_parcels_for_inspection() {
        let mut roads = RoadNetwork::default();
        for c in [-70.0f32, 70.0] {
            roads.add_polyline(&[Vec2::new(-300.0, c), Vec2::new(300.0, c)], 1, 1);
            roads.add_polyline(&[Vec2::new(c, -300.0), Vec2::new(c, 300.0)], 1, 1);
        }
        let curve = crate::plan::sample_curve(&[Vec2::new(-20.0, -70.0), Vec2::new(-35.0, -20.0), Vec2::new(25.0, 20.0), Vec2::new(10.0, 70.0)]);
        roads.add_polyline(&curve, 1, 1);
        let zone = Zone {
            polygon: vec![Vec2::new(-65.0, -65.0), Vec2::new(65.0, -65.0), Vec2::new(65.0, 65.0), Vec2::new(-65.0, 65.0)],
            land_use: LandUse::Residential,
        };
        let mut lots = SlotMap::with_key();
        generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
        dump_lots(&roads, &lots, "ND_DUMP");
    }

    fn dump_lots(roads: &RoadNetwork, lots: &SlotMap<LotId, Lot>, var: &str) {
        let Ok(path) = std::env::var(var) else { return };
        let mut out = String::new();
        for (sid, _) in roads.alive_segments() {
            let (a, b) = roads.segment_points(sid);
            out.push_str(&format!("road|{},{};{},{}
", a.x, a.y, b.x, b.y));
        }
        let reasons = DEAD_REASONS.with(|r| r.borrow().clone());
        for l in lots.values() {
            let pts: Vec<String> = l.polygon.iter().map(|p| format!("{},{}", p.x, p.y)).collect();
            let kind = if l.usable() {
                "lot".to_string()
            } else {
                let r = reasons.iter().find(|(c, _, _, _)| c.distance(l.center) < 0.01).map(|(_, r, a, k)| format!("dead:{r}:{a:.0}:{k:.3}")).unwrap_or_else(|| "dead".into());
                r
            };
            out.push_str(&format!("{}|{}
", kind, pts.join(";")));
        }
        let _ = std::fs::write(path, out);
    }

}

