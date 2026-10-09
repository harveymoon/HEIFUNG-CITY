//! Rail facilities (stations, the central station, freight yards) and how
//! they link to the track and the streets.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RailFacility {
    Station,
    FreightYard,
    /// [subway] The city's main station: intercity platforms, a metro
    /// station and a bus terminal under one grand roof.
    Central,
}

impl RailFacility {
    pub fn label(self) -> &'static str {
        match self {
            RailFacility::Station => "railway station",
            RailFacility::FreightYard => "freight yard",
            RailFacility::Central => "Central Station",
        }
    }

    /// Passenger trains call here.
    pub fn is_passenger(self) -> bool {
        matches!(self, RailFacility::Station | RailFacility::Central)
    }

    /// Length of the platform (or apron) along the track.
    pub fn platform_length(self) -> f32 {
        match self {
            RailFacility::Station => STATION_PLATFORM,
            RailFacility::FreightYard => YARD_APRON,
            RailFacility::Central => CENTRAL_PLATFORM,
        }
    }

    /// How far people walk to it to take a train.
    pub fn reach(self) -> f32 {
        match self {
            RailFacility::Central => CENTRAL_REACH,
            _ => RAIL_REACH,
        }
    }

    pub fn cost(self) -> f32 {
        match self {
            RailFacility::Station => STATION_COST,
            RailFacility::FreightYard => YARD_COST,
            RailFacility::Central => CENTRAL_COST,
        }
    }

    pub fn upkeep_per_day(self) -> f32 {
        match self {
            RailFacility::Station => STATION_UPKEEP_PER_DAY,
            RailFacility::FreightYard => YARD_UPKEEP_PER_DAY,
            RailFacility::Central => CENTRAL_UPKEEP_PER_DAY,
        }
    }

    pub fn jobs(self) -> u16 {
        match self {
            RailFacility::Station => STATION_JOBS,
            RailFacility::FreightYard => YARD_JOBS,
            RailFacility::Central => CENTRAL_JOBS,
        }
    }
}

/// How a building sits on the railway: the track beside it and the
/// platform or apron along that track, on the building's side.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RailLink {
    pub kind: RailFacility,
    pub segment: SegmentId,
    /// Platform ends in world coordinates, parallel to the track.
    pub platform: [Vec2; 2],
    /// +1 or -1: which side of the track (relative to its a->b direction).
    pub side: f32,
    /// Whether the track beside it reaches the rest of the railway, and the
    /// building's street reaches the city. Nothing runs until both do.
    #[serde(default)]
    pub rail_connected: bool,
    #[serde(default)]
    pub road_connected: bool,
    /// [sweep-sim] The street stub reaches the city over streets or paths:
    /// enough for a passenger station (people walk in), not for a yard.
    #[serde(default)]
    pub walk_connected: bool,
}

impl RailLink {
    pub fn works(&self) -> bool {
        self.rail_connected && (self.road_connected || (self.kind.is_passenger() && self.walk_connected))
    }

    /// [sweep-sim] Working, but reached on foot only (no road yet).
    pub fn walk_only(&self) -> bool {
        self.works() && !self.road_connected
    }
}

/// A station or yard placed as a piece, Cities Skylines style: a stretch of
/// track along one side, a stretch of street along the other. Set down
/// beside existing track or street it fuses to them; otherwise the stubs
/// are laid for the player to join up.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Facility {
    pub kind: RailFacility,
    pub center: Vec2,
    /// Unit direction of the track side; the rail lies on `perp(along)`.
    pub along: Vec2,
    pub building: Option<BuildingId>,
    #[serde(default)]
    pub lot: Option<crate::zoning::LotId>,
}

impl Facility {
    /// Footprint along the track and across, in metres.
    pub fn size(kind: RailFacility) -> (f32, f32) {
        match kind {
            RailFacility::Station => (64.0, 28.0),
            RailFacility::FreightYard => (110.0, 56.0),
            RailFacility::Central => (150.0, 64.0),
        }
    }

    /// Towards the rail side.
    pub fn inward(&self) -> Vec2 {
        Vec2::new(-self.along.y, self.along.x)
    }

    /// Corners as the lot code expects them: front-left, front-right,
    /// back-right, back-left, with the street side as the front.
    pub fn corners(&self) -> [Vec2; 4] {
        let (w, d) = Self::size(self.kind);
        let a = self.along;
        let n = self.inward();
        let c = self.center;
        [c - a * (w * 0.5) - n * (d * 0.5), c + a * (w * 0.5) - n * (d * 0.5), c + a * (w * 0.5) + n * (d * 0.5), c - a * (w * 0.5) + n * (d * 0.5)]
    }

    pub fn footprint(&self) -> Vec<Vec2> {
        self.corners().to_vec()
    }

    /// Centreline of the track stub along the rail side.
    pub fn rail_line(&self) -> [Vec2; 2] {
        let (w, d) = Self::size(self.kind);
        let off = self.inward() * (d * 0.5 + RAIL_GAP);
        [self.center + off - self.along * (w * 0.5 + STUB_LEAD), self.center + off + self.along * (w * 0.5 + STUB_LEAD)]
    }

    /// Centreline of the street stub along the road side.
    pub fn road_line(&self) -> [Vec2; 2] {
        let (w, d) = Self::size(self.kind);
        let off = -self.inward() * (d * 0.5 + ROAD_GAP);
        [self.center + off - self.along * (w * 0.5 + STUB_LEAD), self.center + off + self.along * (w * 0.5 + STUB_LEAD)]
    }

    /// Midpoint of the street-side edge: the building's entrance.
    pub fn front_mid(&self) -> Vec2 {
        let (_, d) = Self::size(self.kind);
        self.center - self.inward() * (d * 0.5)
    }

    pub fn rail_mid(&self) -> Vec2 {
        let (_, d) = Self::size(self.kind);
        self.center + self.inward() * (d * 0.5)
    }

    /// Where a facility of `kind` would go for a click at `p`: aligned and
    /// shifted onto track within reach, else onto a street, else free.
    /// Returns the facility and whether the rail and road sides fuse to
    /// something already there.
    pub fn plan(kind: RailFacility, p: Vec2, roads: &RoadNetwork) -> (Facility, bool, bool) {
        let (_, d) = Self::size(kind);
        let mut f = Facility { kind, center: p, along: Vec2::X, building: None, lot: None };
        let mut rail_fused = false;
        let mut road_fused = false;
        if let Some((sid, _, q)) = roads.nearest_rail(p).filter(|(_, _, q)| q.distance(p) <= FUSE_RAIL) {
            let (a, b) = roads.segment_points(sid);
            let dir = (b - a).normalize_or_zero();
            let n = Vec2::new(-dir.y, dir.x);
            // The rail lies on the inward side: pick `along` so inward points
            // from the track towards the click side's opposite... the
            // building sits on the click's side of the track, rail inward.
            let side = if (p - q).dot(n) >= 0.0 { 1.0 } else { -1.0 };
            // inward must point from the building to the track: -side * n.
            let inward = -n * side;
            f.along = Vec2::new(inward.y, -inward.x);
            f.center = q + n * side * (d * 0.5 + RAIL_GAP);
            rail_fused = true;
        } else if let Some((sid, _, q)) = roads.nearest_street(p).filter(|(_, _, q)| q.distance(p) <= FUSE_ROAD) {
            let (a, b) = roads.segment_points(sid);
            let dir = (b - a).normalize_or_zero();
            let n = Vec2::new(-dir.y, dir.x);
            let side = if (p - q).dot(n) >= 0.0 { 1.0 } else { -1.0 };
            // The street is on the outward (front) side: inward points away from it.
            let inward = n * side;
            f.along = Vec2::new(inward.y, -inward.x);
            let hw = roads.segment(sid).half_width();
            f.center = q + n * side * (hw + crate::zoning::SIDEWALK + d * 0.5);
            road_fused = true;
        } else if let Some((sid, _, _)) = roads.nearest_street(p).filter(|(_, _, q)| q.distance(p) <= 160.0) {
            // Free-standing, but square to the nearest street for tidiness.
            let (a, b) = roads.segment_points(sid);
            f.along = (b - a).normalize_or_zero();
        }
        if rail_fused {
            // A street already along the road side counts too.
            let mid = f.front_mid() - f.inward() * ROAD_GAP;
            if let Some((_, _, q)) = roads.nearest_street(mid) {
                if q.distance(mid) <= ROAD_EDGE_FUSE {
                    road_fused = true;
                }
            }
        }
        (f, rail_fused, road_fused)
    }
}

impl RailLink {
    /// The track beside a lot, if any comes within reach and runs along the
    /// lot's street frontage (track on one side, road on the other), with a
    /// platform of `length` centred on the lot.
    pub fn beside(roads: &RoadNetwork, polygon: &[Vec2], center: Vec2, front: [Vec2; 2], kind: RailFacility, length: f32) -> Option<RailLink> {
        let front_dir = (front[1] - front[0]).normalize_or_zero();
        let n = polygon.len();
        let mut samples: Vec<Vec2> = polygon.to_vec();
        for i in 0..n {
            samples.push((polygon[i] + polygon[(i + 1) % n]) * 0.5);
        }
        let mut best: Option<(f32, SegmentId)> = None;
        for p in &samples {
            if let Some((sid, _, q)) = roads.nearest_rail(*p) {
                let d = p.distance(q);
                let (a, b) = roads.segment_points(sid);
                let dir = (b - a).normalize_or_zero();
                let skew = dir.dot(front_dir).abs().clamp(0.0, 1.0).acos();
                if d <= STATION_REACH && skew <= PLATFORM_SKEW && best.is_none_or(|(bd, _)| d < bd) {
                    best = Some((d, sid));
                }
            }
        }
        let (_, segment) = best?;
        let (a, b) = roads.segment_points(segment);
        let seg_len = a.distance(b).max(1.0);
        let d = (b - a) / seg_len;
        let right = Vec2::new(-d.y, d.x);
        let (_, q) = project_on_segment(center, a, b);
        let side = if (center - q).dot(right) >= 0.0 { 1.0 } else { -1.0 };
        // The platform may run on along the straight the segment is part
        // of: track is split wherever anything joins or crosses it, and a
        // long platform (the Central Station's) must not be cut to one
        // short piece of it.
        let (lo, hi) = straight_run(roads, segment);
        let t = (center - a).dot(d);
        let half = (length * 0.5).min((hi - lo) * 0.45);
        let mid_t = t.clamp(lo + half, hi - half);
        let mid = a + d * mid_t;
        let off = right * side * (RAIL_HALF_WIDTH + 1.4);
        // The piece of track beside the platform's middle, which is what
        // trains route to and turn at (`plan_train_route`).
        let segment = roads.nearest_rail(mid).filter(|(_, _, q)| q.distance(mid) < 1.0).map_or(segment, |(sid, _, _)| sid);
        Some(RailLink { kind, segment, platform: [mid - d * half + off, mid + d * half + off], side, rail_connected: false, road_connected: false, walk_connected: false })
    }

    pub fn centre(&self) -> Vec2 {
        (self.platform[0] + self.platform[1]) * 0.5
    }

    /// Unit direction along the platform.
    pub fn along(&self) -> Vec2 {
        (self.platform[1] - self.platform[0]).normalize_or_zero()
    }
}

/// How far the track runs straight on either side of `segment`'s start,
/// along its direction: rail segments joined end to end and in line with it
/// (within a degree and half a metre). Metres from its `a` end.
pub(super) fn straight_run(roads: &RoadNetwork, segment: SegmentId) -> (f32, f32) {
    let (a, b) = roads.segment_points(segment);
    let len = a.distance(b);
    if len < 0.01 {
        return (0.0, len);
    }
    let d = (b - a) / len;
    let in_line = |p: Vec2| {
        let off = p - a;
        (off.dot(d), (off - d * off.dot(d)).length())
    };
    let (mut lo, mut hi) = (0.0f32, len);
    let seg = roads.segment(segment);
    for (start, forward) in [(seg.a, false), (seg.b, true)] {
        let mut node = start;
        let mut came = segment;
        // Bounded: a straight longer than a few kilometres is the trunk
        // running off the map, and no platform needs that much of it.
        for _ in 0..400 {
            let next = roads.neighbors(node).find(|(other, sid)| {
                *sid != came && roads.segment(*sid).is_rail() && {
                    let (t, off) = in_line(roads.node(*other).pos);
                    off < 0.5 && if forward { t > in_line(roads.node(node).pos).0 + 0.01 } else { t < in_line(roads.node(node).pos).0 - 0.01 }
                }
            });
            let Some((other, sid)) = next else { break };
            let (t, _) = in_line(roads.node(other).pos);
            if forward {
                hi = hi.max(t);
            } else {
                lo = lo.min(t);
            }
            node = other;
            came = sid;
        }
    }
    (lo, hi)
}
