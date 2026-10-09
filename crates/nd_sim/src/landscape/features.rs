//! The water features of a landscape (rivers, lakes, the sea) and hot
//! spots, and what a water query returns.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum WaterKind {
    River,
    Stream,
    Lake,
    Sea,
}

impl WaterKind {
    pub fn label(self) -> &'static str {
        match self {
            WaterKind::River => "river",
            WaterKind::Stream => "stream",
            WaterKind::Lake => "lake",
            WaterKind::Sea => "sea",
        }
    }
}

/// A river or stream: a centreline from source to mouth (the water flows
/// from the first point to the last), a width and a current.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct River {
    pub points: Vec<Vec2>,
    pub width: f32,
    /// Metres per second.
    pub current: f32,
    pub stream: bool,
}

impl River {
    pub fn kind(&self) -> WaterKind {
        if self.stream { WaterKind::Stream } else { WaterKind::River }
    }

    /// Closest point on the centreline: (segment index, point, distance).
    pub fn nearest(&self, p: Vec2) -> (usize, Vec2, f32) {
        let mut best = (0, self.points[0], f32::INFINITY);
        for (i, w) in self.points.windows(2).enumerate() {
            let (_, q) = project_on_segment(p, w[0], w[1]);
            let d = q.distance(p);
            if d < best.2 {
                best = (i, q, d);
            }
        }
        best
    }

    /// Direction the water flows at segment `i`.
    pub fn flow(&self, i: usize) -> Vec2 {
        let i = i.min(self.points.len().saturating_sub(2));
        (self.points[i + 1] - self.points[i]).normalize_or_zero()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lake {
    pub outline: Vec<Vec2>,
    pub center: Vec2,
}

/// The sea: everything inside `outline` that is not on an island.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sea {
    pub outline: Vec<Vec2>,
    /// Land inside the outline.
    pub islands: Vec<Vec<Vec2>>,
    /// The real shore: the coast polyline (open) for a coast; empty when the
    /// shore is only the islands.
    pub coast: Vec<Vec2>,
    /// Which way is out to sea, for ships leaving the map.
    pub outward: SeaExit,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum SeaExit {
    /// Straight south off the map.
    South,
    /// Radially away from the centre.
    Radial,
}

impl Sea {
    pub fn contains(&self, p: Vec2) -> bool {
        point_in_polygon(p, &self.outline) && !self.islands.iter().any(|i| point_in_polygon(p, i))
    }

    /// Where a ship at `p` leaves the map.
    pub fn exit_from(&self, p: Vec2) -> Vec2 {
        match self.outward {
            // Out of sight, so ships come and go where nobody sees them.
            SeaExit::South => Vec2::new(p.x, -WAY_REACH),
            SeaExit::Radial => p.normalize_or(Vec2::NEG_Y) * WAY_REACH,
        }
    }

    /// Shore polylines: the coast and every island outline (closed).
    pub(super) fn shores(&self) -> Vec<Vec<Vec2>> {
        let mut out = Vec::new();
        if self.coast.len() >= 2 {
            out.push(self.coast.clone());
        }
        for i in &self.islands {
            let mut ring = i.clone();
            if let Some(&f) = i.first() {
                ring.push(f);
            }
            out.push(ring);
        }
        out
    }
}

/// Hot ground a geothermal plant can tap.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HotSpot {
    pub pos: Vec2,
    pub radius: f32,
    /// 0..1: share of a geothermal plant's nameplate it supports.
    pub heat: f32,
}

/// Water at a point: what it is, which feature, how fast and which way it
/// flows, and how wide it is (rivers; lakes and the sea report their size).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WaterHit {
    pub kind: WaterKind,
    pub index: usize,
    pub current: f32,
    pub width: f32,
    pub flow: Vec2,
}

/// The nearest water's edge to a point on land.
#[derive(Clone, Copy, Debug)]
pub struct Shore {
    /// A point on the waterline.
    pub point: Vec2,
    /// Unit vector from the land into the water.
    pub seaward: Vec2,
    pub distance: f32,
    pub water: WaterHit,
}
