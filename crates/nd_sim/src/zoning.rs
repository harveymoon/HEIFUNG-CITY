//! Zones (land-use polygons) and the lots carved out of them along roads.

use crate::buildings::BuildingId;
use crate::roads::{NodeId, RoadNetwork, SegmentId};
use glam::Vec2;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};
use std::collections::HashSet;

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

    /// Lot frontage along the road, in metres.
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
    /// Corners in order: front-left, front-right, back-right, back-left.
    pub corners: [Vec2; 4],
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
    pub fn width(&self) -> f32 {
        self.corners[0].distance(self.corners[1])
    }

    pub fn depth(&self) -> f32 {
        self.corners[1].distance(self.corners[2])
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

/// Carve lots along every road segment that runs through `zone`, skipping
/// space already claimed by `existing` lots. New lots are appended to `lots`.
pub fn generate_lots(
    zone_id: ZoneId,
    zone: &Zone,
    roads: &RoadNetwork,
    lots: &mut SlotMap<LotId, Lot>,
) -> Vec<LotId> {
    let lu = zone.land_use;
    let (w, d) = (lu.lot_width(), lu.lot_depth());
    let mut created = Vec::new();

    // Coarse occupancy grid of existing lot centres.
    let cell = 8.0;
    let mut occupied: HashSet<(i32, i32)> = HashSet::new();
    let key = |p: Vec2| ((p.x / cell).floor() as i32, (p.y / cell).floor() as i32);
    for lot in lots.values() {
        for c in lot_cells(lot, cell) {
            occupied.insert(c);
        }
    }

    for (sid, seg) in roads.alive_segments() {
        let (p0, p1) = roads.segment_points(sid);
        let len = p0.distance(p1);
        if len < w * 0.75 {
            continue;
        }
        let dir = (p1 - p0) / len;
        let normal = Vec2::new(-dir.y, dir.x);
        let hw = seg.half_width() + 1.0;
        let count = (len / w).floor().max(1.0) as usize;
        let actual_w = len / count as f32;

        for i in 0..count {
            let t0 = i as f32 * actual_w;
            let t1 = t0 + actual_w;
            for side in [-1.0f32, 1.0] {
                let n = normal * side;
                let fl = p0 + dir * t0 + n * hw;
                let fr = p0 + dir * t1 + n * hw;
                let br = fr + n * d;
                let bl = fl + n * d;
                let center = (fl + fr + br + bl) * 0.25;
                let inside = zone.contains(center) && zone.contains(bl) && zone.contains(br);
                if !inside {
                    continue;
                }
                let candidate = Lot {
                    zone: zone_id,
                    land_use: lu,
                    corners: [fl, fr, br, bl],
                    center,
                    front_dir: -n,
                    front_segment: sid,
                    front_t: ((t0 + t1) * 0.5) / len,
                    access_node: roads.nearest_endpoint(sid, ((t0 + t1) * 0.5) / len),
                    building: None,
                };
                let cells = lot_cells(&candidate, cell);
                if cells.iter().any(|c| occupied.contains(c)) {
                    continue;
                }
                // Keep lots off other roads.
                let blocked = roads.alive_segments().any(|(other, oseg)| {
                    if other == sid {
                        return false;
                    }
                    let (q0, q1) = roads.segment_points(other);
                    let (_, q) = crate::roads::project_on_segment(center, q0, q1);
                    center.distance(q) < oseg.half_width() + d * 0.45
                });
                if blocked {
                    continue;
                }
                for c in cells {
                    occupied.insert(c);
                }
                let _ = key;
                created.push(lots.insert(candidate));
            }
        }
    }
    created
}

fn lot_cells(lot: &Lot, cell: f32) -> Vec<(i32, i32)> {
    // Sample the lot interior on a small grid so neighbouring lots cannot overlap.
    let mut out = Vec::new();
    let steps = 3;
    for i in 0..steps {
        for j in 0..steps {
            let u = (i as f32 + 0.5) / steps as f32;
            let v = (j as f32 + 0.5) / steps as f32;
            let front = lot.corners[0].lerp(lot.corners[1], u);
            let back = lot.corners[3].lerp(lot.corners[2], u);
            let p = front.lerp(back, v);
            out.push(((p.x / cell).floor() as i32, (p.y / cell).floor() as i32));
        }
    }
    out
}
