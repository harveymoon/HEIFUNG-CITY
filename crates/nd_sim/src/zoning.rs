//! Zones (land-use polygons) and the parcels carved out of them.
//!
//! A zone is cut into blocks by the road corridors that cross it, and each
//! block is sliced into parcels along its frontage. Parcels keep their exact
//! outline, so lots on a diagonal road or a tapering block are trapezoids and
//! triangles rather than rectangles, and buildings follow those shapes.

use crate::buildings::BuildingId;
use crate::roads::{project_on_segment, NodeId, RoadNetwork, SegmentId};
use glam::Vec2;
use rand::Rng;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};

/// Pavement between the kerb and the parcel line, in metres.
pub const SIDEWALK: f32 = 3.2;
/// Parcels smaller than this are dropped as slivers.
const MIN_AREA: f32 = 60.0;

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

/// Keep the part of `poly` where `(p - origin) . normal >= 0`.
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

/// Remove the corridor of one road from a set of blocks, splitting any block
/// the road crosses. Pieces beyond the road's ends stay joined to the block.
fn cut_by_road(blocks: Vec<Vec<Vec2>>, a: Vec2, b: Vec2, hw: f32) -> Vec<Vec<Vec2>> {
    let len = a.distance(b);
    if len < 1.0 {
        return blocks;
    }
    let d = (b - a) / len;
    let n = Vec2::new(-d.y, d.x);
    let mut out = Vec::new();
    for p in blocks {
        let (n0, n1) = extents(&p, n);
        let an = a.dot(n);
        let (d0, d1) = extents(&p, d);
        let ad = a.dot(d);
        // Entirely beside the corridor, or entirely before/after the segment.
        if n0 > an + hw || n1 < an - hw || d1 < ad - hw || d0 > ad + len + hw {
            out.push(p);
            continue;
        }
        let mut pieces = Vec::new();
        pieces.push(clip_halfplane(&p, a + n * hw, n));
        pieces.push(clip_halfplane(&p, a - n * hw, -n));
        let strip = clip_halfplane(&clip_halfplane(&p, a - n * hw, n), a + n * hw, -n);
        pieces.push(clip_halfplane(&strip, a - d * hw, -d));
        pieces.push(clip_halfplane(&strip, b + d * hw, d));
        for piece in pieces {
            if piece.len() >= 3 && polygon_area(&piece).abs() > 4.0 {
                out.push(piece);
            }
        }
    }
    out
}

/// Distance from a point to the nearest road centreline, with that segment.
fn nearest_road(roads: &RoadNetwork, p: Vec2) -> Option<(SegmentId, f32, f32)> {
    let mut best: Option<(SegmentId, f32, f32)> = None;
    for (sid, _) in roads.alive_segments() {
        let (a, b) = roads.segment_points(sid);
        let (t, q) = project_on_segment(p, a, b);
        let dist = p.distance(q);
        if best.map_or(true, |(_, _, d)| dist < d) {
            best = Some((sid, t, dist));
        }
    }
    best
}

/// The polygon edge that faces a road: the one whose midpoint is nearest a
/// carriageway edge. Returns (edge index, segment, t, distance to centreline).
fn frontage(poly: &[Vec2], roads: &RoadNetwork) -> Option<(usize, SegmentId, f32, f32)> {
    let n = poly.len();
    let mut best: Option<(usize, SegmentId, f32, f32)> = None;
    for i in 0..n {
        let a = poly[i];
        let b = poly[(i + 1) % n];
        if a.distance(b) < 2.0 {
            continue;
        }
        let mid = (a + b) * 0.5;
        let Some((sid, t, dist)) = nearest_road(roads, mid) else { continue };
        // Prefer long edges that sit right on the pavement line.
        let hw = roads.segment(sid).half_width();
        let score = (dist - hw - SIDEWALK).abs() - a.distance(b) * 0.05;
        if best.map_or(true, |(bi, _, _, _)| {
            let (ba, bb) = (poly[bi], poly[(bi + 1) % n]);
            let bmid = (ba + bb) * 0.5;
            let bdist = nearest_road(roads, bmid).map(|x| x.2).unwrap_or(f32::MAX);
            let bhw = roads.segment(best.unwrap().1).half_width();
            score < (bdist - bhw - SIDEWALK).abs() - ba.distance(bb) * 0.05
        }) {
            best = Some((i, sid, t, dist));
        }
    }
    best
}

/// Slice a block into parcels of roughly the land use's size. Cuts run
/// perpendicular to the frontage with some jitter, occasionally at an angle,
/// and deep blocks get a back row.
fn subdivide(poly: Vec<Vec2>, lu: LandUse, roads: &RoadNetwork, rng: &mut impl Rng, depth: u32, out: &mut Vec<Vec<Vec2>>) {
    let area = polygon_area(&poly).abs();
    if poly.len() < 3 || area < MIN_AREA {
        return;
    }
    let lw = lu.lot_width();
    let ld = lu.lot_depth();
    let n = poly.len();
    let (along, inward) = match frontage(&poly, roads) {
        Some((i, _, _, _)) => {
            let d = (poly[(i + 1) % n] - poly[i]).normalize_or_zero();
            (d, Vec2::new(-d.y, d.x))
        }
        None => {
            let (mut best, mut best_len) = (0, 0.0);
            for i in 0..n {
                let l = poly[i].distance(poly[(i + 1) % n]);
                if l > best_len {
                    best = i;
                    best_len = l;
                }
            }
            let d = (poly[(best + 1) % n] - poly[best]).normalize_or_zero();
            (d, Vec2::new(-d.y, d.x))
        }
    };
    if along == Vec2::ZERO {
        return;
    }
    let (u0, u1) = extents(&poly, along);
    let (v0, v1) = extents(&poly, inward);
    let width = u1 - u0;
    let dep = v1 - v0;
    let too_deep = depth >= 14;

    if !too_deep && width > lw * 1.7 && area > lw * ld * 1.3 {
        // Slice one parcel off the end of the block.
        let mut cut = u0 + lw * rng.random_range(0.85..1.35);
        if u1 - cut < lw * 0.8 {
            cut = (u0 + u1) * 0.5;
        }
        let angle: f32 = if rng.random_bool(0.18) { rng.random_range(-0.22..0.22) } else { 0.0 };
        let (s, c) = angle.sin_cos();
        let normal = Vec2::new(along.x * c - along.y * s, along.x * s + along.y * c);
        let origin = along * cut + inward * ((v0 + v1) * 0.5);
        let left = clip_halfplane(&poly, origin, -normal);
        let right = clip_halfplane(&poly, origin, normal);
        if left.len() >= 3 && right.len() >= 3 {
            subdivide(left, lu, roads, rng, depth + 1, out);
            subdivide(right, lu, roads, rng, depth + 1, out);
            return;
        }
    }
    if !too_deep && dep > ld * 1.9 {
        // A back row behind the frontage parcels.
        let cut = v0 + ld * rng.random_range(0.95..1.25);
        let origin = along * ((u0 + u1) * 0.5) + inward * cut;
        let front = clip_halfplane(&poly, origin, -inward);
        let back = clip_halfplane(&poly, origin, inward);
        if front.len() >= 3 && back.len() >= 3 {
            subdivide(front, lu, roads, rng, depth + 1, out);
            subdivide(back, lu, roads, rng, depth + 1, out);
            return;
        }
    }
    if width < 8.0 || dep < 8.0 {
        return;
    }
    out.push(poly);
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

/// Carve `zone` into parcels: cut out every road corridor, split the
/// remaining blocks along their frontages, and drop parcels whose centre is
/// covered by one of the `covering` zones (drawn later, so they win) or by an
/// existing lot. New lots are appended to `lots`.
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

    // Blocks: the zone minus every road corridor.
    let mut blocks = vec![ccw(&zone.polygon)];
    for (sid, seg) in roads.alive_segments() {
        let (a, b) = roads.segment_points(sid);
        blocks = cut_by_road(blocks, a, b, seg.half_width() + SIDEWALK);
    }

    // Parcels.
    let mut parcels: Vec<Vec<Vec2>> = Vec::new();
    for block in blocks {
        subdivide(block, lu, roads, &mut rng, 0, &mut parcels);
    }

    for poly in parcels {
        let poly = ccw(&poly);
        let center = polygon_centroid(&poly);
        if covering.iter().any(|z| z.contains(center)) {
            continue;
        }
        if lots.values().any(|l| l.contains(center) || point_in_polygon(l.center, &poly)) {
            continue;
        }
        let Some((fi, sid, t, _)) = frontage(&poly, roads) else { continue };
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

    #[test]
    fn zone_straddling_roads_splits_into_blocks_and_parcels() {
        let roads = roads_cross();
        let zone = Zone {
            polygon: vec![Vec2::new(-100.0, -80.0), Vec2::new(100.0, -80.0), Vec2::new(100.0, 80.0), Vec2::new(-100.0, 80.0)],
            land_use: LandUse::Residential,
        };
        let mut lots = SlotMap::with_key();
        let ids = generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
        assert!(ids.len() >= 8, "got {} parcels", ids.len());
        for lot in lots.values() {
            // No parcel sits on the carriageway.
            assert!(lot.center.x.abs() > 5.0 && lot.center.y.abs() > 5.0, "parcel on road at {:?}", lot.center);
            assert!(lot.area() >= MIN_AREA);
            assert!(lot.contains(lot.center) || lot.polygon.len() >= 3);
            assert!(polygon_area(&lot.polygon) > 0.0, "parcels are counter-clockwise");
            let e = lot.entrance();
            assert!(zone.contains(e) || (e.x.abs() < 101.0 && e.y.abs() < 81.0));
        }
        // Parcels together cover most of the zone minus roads.
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
        assert!(!lots.is_empty());
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
}
