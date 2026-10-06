//! Trains. A trunk line crosses the map from the start; the player lays
//! branches off it and puts stations and freight yards on lots beside the
//! track. Passenger trains run the line end to end on a headway, calling
//! at every station; freight trains come when a yard has goods waiting.
//! Trains are not saved: they are back on the next headway after a load.

use crate::buildings::BuildingId;
use crate::citizens::CitizenId;
use crate::roads::{project_on_segment, NodeId, RoadNetwork, SegmentId, END_SNAP_RADIUS, RAIL_HALF_WIDTH};
use crate::traffic::Cargo;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};

pub use crate::geom::{path_length, point_along, project_on_poly};

new_key_type! {
    pub struct TrainId;
    pub struct FacilityId;
}

/// Where the trunk railway runs, in metres south of the centre.
pub const TRUNK_Y: f32 = -480.0;
/// Tightest curve a train takes; sharper control-point corners are eased.
pub const MIN_RADIUS: f32 = 120.0;
/// Length of the turnout: a branch runs beside the line it leaves before
/// merging, like a real switch, instead of meeting it square on.
pub const SWITCH_LEAD: f32 = 60.0;
/// Control-point turns below this are left as the spline makes them.
pub const EASE_TURN: f32 = 0.35;
/// How far off parallel to the street a platform may lie (radians).
pub const PLATFORM_SKEW: f32 = 0.6;
/// Track or street this close to a placed facility is what it fuses to.
pub const FUSE_RAIL: f32 = 40.0;
pub const FUSE_ROAD: f32 = 40.0;
/// A fused road must lie this close to the facility's road edge.
pub const ROAD_EDGE_FUSE: f32 = 14.0;
/// From the footprint edge to the track centreline.
pub const RAIL_GAP: f32 = 6.0;
/// From the footprint edge to a 1+1 street's centreline (lane, shoulder, pavement).
pub const ROAD_GAP: f32 = 7.6;
/// Connector stubs reach this far past the footprint at each end.
pub const STUB_LEAD: f32 = 24.0;
/// At most this many trains out at once.
pub const MAX_TRAINS: usize = 6;
/// How close to the track a lot must come to host a station or yard.
pub const STATION_REACH: f32 = 24.0;
pub const STATION_COST: f32 = 25_000.0;
pub const STATION_UPKEEP_PER_DAY: f32 = 300.0;
pub const STATION_JOBS: u16 = 8;
pub const STATION_PLATFORM: f32 = 70.0;
/// [subway] The Central Station: a grand hall over long intercity
/// platforms, with a metro station under it and a bus terminal at its
/// front. One per city.
pub const CENTRAL_COST: f32 = 120_000.0;
pub const CENTRAL_UPKEEP_PER_DAY: f32 = 1_100.0;
pub const CENTRAL_JOBS: u16 = 60;
pub const CENTRAL_PLATFORM: f32 = 120.0;
/// People walk further to a Central Station than to a halt.
pub const CENTRAL_REACH: f32 = 420.0;
pub const YARD_COST: f32 = 30_000.0;
pub const YARD_UPKEEP_PER_DAY: f32 = 400.0;
pub const YARD_JOBS: u16 = 20;
pub const YARD_MIN_AREA: f32 = 800.0;
pub const YARD_APRON: f32 = 90.0;
/// Sim minutes between passenger trains, and between freight trains.
pub const PASSENGER_HEADWAY: u64 = 120;
pub const FREIGHT_HEADWAY: u64 = 180;
pub const SERVICE_START: u32 = 5 * 60;
pub const SERVICE_END: u32 = 23 * 60;
pub const PASSENGER_SPEED: f32 = 25.0;
pub const FREIGHT_SPEED: f32 = 15.0;
/// Seconds a train stands at a station or yard.
pub const DWELL_SECONDS: f32 = 30.0;
pub const TRAIN_CAPACITY: usize = 200;
/// How far people walk to a station, and the shortest trip worth a train.
pub const RAIL_REACH: f32 = 240.0;
pub const RAIL_TRIP_MIN: f32 = 500.0;
pub const RAIL_FARE: f32 = 4.0;
/// Share of the import bill a buyer pays when goods come by rail.
pub const RAIL_IMPORT_FACTOR: f32 = 0.8;
/// Export income multiplier for towns with a freight yard.
pub const RAIL_EXPORT_BONUS: f32 = 1.2;
pub const LOCO_LEN: f32 = 18.0;
pub const COACH_LEN: f32 = 19.0;
pub const WAGON_LEN: f32 = 14.0;
pub const COUPLING: f32 = 1.2;
/// A train keeps this much clear track ahead.
pub const BLOCK: f32 = 90.0;

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
}

impl RailLink {
    pub fn works(&self) -> bool {
        self.rail_connected && self.road_connected
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
        Some(RailLink { kind, segment, platform: [mid - d * half + off, mid + d * half + off], side, rail_connected: false, road_connected: false })
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
fn straight_run(roads: &RoadNetwork, segment: SegmentId) -> (f32, f32) {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrainKind {
    Passenger,
    Freight,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Train {
    pub kind: TrainKind,
    /// Coaches or wagons behind the locomotive.
    pub cars: u8,
    pub nodes: Vec<NodeId>,
    pub poly: Vec<Vec2>,
    /// Head of the locomotive along the polyline, now and last tick.
    pub s: f32,
    #[serde(default)]
    pub prev_s: f32,
    pub speed: f32,
    /// Stops in order: the facility and where along the polyline it is.
    pub stops: Vec<(BuildingId, f32)>,
    pub next_stop: usize,
    pub dwell: f32,
    pub passengers: Vec<CitizenId>,
    pub cargo: Vec<Cargo>,
    pub color: u8,
    /// Standing because the block ahead is taken.
    pub held: bool,
    /// Arc range of the route that is off the trunk. Branches are single
    /// track with no passing loops, so one train holds the branch at a time.
    pub branch: Option<(f32, f32)>,
}

impl Train {
    pub fn car_len(&self) -> f32 {
        match self.kind {
            TrainKind::Passenger => COACH_LEN,
            TrainKind::Freight => WAGON_LEN,
        }
    }

    pub fn length(&self) -> f32 {
        LOCO_LEN + self.cars as f32 * (self.car_len() + COUPLING)
    }

    pub fn total(&self) -> f32 {
        path_length(&self.poly)
    }

    pub fn head(&self) -> Vec2 {
        point_along(&self.poly, self.s).0
    }

    /// On a branch right now.
    pub fn on_branch(&self) -> bool {
        self.branch.is_some_and(|(a, b)| self.s > a && self.s - self.length() < b)
    }

    /// The train has run its route to the end.
    pub fn finished(&self) -> bool {
        self.next_stop >= self.stops.len() && self.s >= self.total() - 0.5
    }

    /// Centre and heading of vehicle `k` (0 is the locomotive).
    pub fn vehicle_pose(&self, k: usize) -> (Vec2, f32) {
        self.vehicle_pose_at(k, self.s)
    }

    /// The same with the head at `s`: between ticks the renderer blends.
    pub fn vehicle_pose_at(&self, k: usize, s: f32) -> (Vec2, f32) {
        let len = if k == 0 { LOCO_LEN } else { self.car_len() };
        let back = if k == 0 { 0.0 } else { LOCO_LEN + COUPLING + (k as f32 - 1.0) * (self.car_len() + COUPLING) };
        let s_front = s - back;
        let (a, _) = point_along(&self.poly, s_front);
        let (b, _) = point_along(&self.poly, s_front - len);
        let d = (a - b).normalize_or_zero();
        ((a + b) * 0.5, d.y.atan2(d.x))
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RailStats {
    pub trains: u32,
    pub stations: u32,
    pub yards: u32,
    pub waiting: u32,
    pub riding: u32,
    pub riders_today: u32,
    pub fares_today: f32,
    pub passenger_trains_today: u32,
    pub freight_trains_today: u32,
    /// Units of goods brought in by rail today, and waiting at yards.
    pub freight_in_today: f32,
    pub freight_waiting: f32,
    pub upkeep_today: f32,
    /// Sim minutes until the next passenger train enters.
    pub next_passenger_in: u32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Rail {
    #[serde(skip)]
    pub trains: SlotMap<TrainId, Train>,
    #[serde(default)]
    pub stats: RailStats,
    #[serde(default)]
    pub last_passenger_tick: u64,
    #[serde(default)]
    pub last_freight_tick: u64,
}

impl Rail {
    pub fn in_service(minute_of_day: u32) -> bool {
        (SERVICE_START..SERVICE_END).contains(&minute_of_day)
    }

    /// Whether `train` must wait: another train's head or tail stands on
    /// this train's track within [`BLOCK`] metres ahead of its head.
    ///
    /// Each point is projected onto this train's polyline once, taking the
    /// single closest point. That assumes a route never passes the same
    /// place twice (no loops, no out-and-back over the same track), which
    /// holds for the trunk-and-branch railway: a train runs the line from
    /// one end to the other. A route that did fold back on itself could
    /// pick the wrong pass and miss a block.
    pub fn blocked(&self, id: TrainId) -> bool {
        let Some(me) = self.trains.get(id) else { return false };
        for (oid, other) in &self.trains {
            if oid == id {
                continue;
            }
            let head = other.head();
            let tail = point_along(&other.poly, other.s - other.length()).0;
            for p in [head, tail] {
                if let Some((s, d)) = project_on_poly(&me.poly, p) {
                    if d < 3.0 && s > me.s && s < me.s + BLOCK {
                        return true;
                    }
                }
            }
        }
        false
    }
}

/// Make a drawn line of track fit for trains: ends that join existing
/// track leave or arrive along it through a turnout, corners sharper than
/// the spline can soften are eased to the minimum radius, and the result
/// is a smooth curve sampled every few metres.
pub fn smooth_track(points: &[Vec2], roads: &RoadNetwork) -> Vec<Vec2> {
    if points.len() < 2 {
        return points.to_vec();
    }
    let mut pts: Vec<Vec2> = points.to_vec();
    for end in [false, true] {
        let n = pts.len();
        let (i_end, i_next) = if end { (n - 1, n - 2) } else { (0, 1) };
        let p = pts[i_end];
        let Some((sid, _, q)) = roads.nearest_rail(p) else { continue };
        if q.distance(p) > END_SNAP_RADIUS {
            continue;
        }
        let (a, b) = roads.segment_points(sid);
        let mut t = (b - a).normalize_or_zero();
        // Travel away from the switch towards the rest of the line.
        let away = (pts[i_next] - q).normalize_or_zero();
        if t.dot(away) < 0.0 {
            t = -t;
        }
        let side = Vec2::new(-t.y, t.x);
        let side = if side.dot(away) >= 0.0 { side } else { -side };
        let lead = q + t * SWITCH_LEAD + side * 4.0;
        pts[i_end] = q;
        if end {
            pts.insert(i_end, lead);
        } else {
            pts.insert(1, lead);
        }
    }
    // Ease sharp corners: a corner becomes two points along its legs.
    let mut eased: Vec<Vec2> = vec![pts[0]];
    for i in 1..pts.len() - 1 {
        let (a, b, c) = (pts[i - 1], pts[i], pts[i + 1]);
        let d1 = (b - a).normalize_or_zero();
        let d2 = (c - b).normalize_or_zero();
        let turn = d1.dot(d2).clamp(-1.0, 1.0).acos();
        if turn < EASE_TURN {
            eased.push(b);
            continue;
        }
        let tangent = (MIN_RADIUS * (turn * 0.5).tan()).min(a.distance(b) * 0.5).min(b.distance(c) * 0.5);
        eased.push(b - d1 * tangent);
        eased.push(b + d2 * tangent);
    }
    eased.push(*pts.last().unwrap());
    eased.dedup_by(|a, b| a.distance(*b) < 0.5);
    // Long legs get waypoints so the spline runs straight between the
    // bends instead of bulging towards the next control point.
    let mut dense: Vec<Vec2> = vec![eased[0]];
    for w in eased.windows(2) {
        let len = w[0].distance(w[1]);
        let n = (len / 30.0).floor() as usize;
        for k in 1..n {
            dense.push(w[0].lerp(w[1], k as f32 / n as f32));
        }
        dense.push(w[1]);
    }
    crate::plan::sample_curve(&dense)
}

/// The sharpest turn between consecutive legs of a polyline, in radians.
pub fn sharpest_turn(pts: &[Vec2]) -> f32 {
    let mut worst = 0.0f32;
    for w in pts.windows(3) {
        let d1 = (w[1] - w[0]).normalize_or_zero();
        let d2 = (w[2] - w[1]).normalize_or_zero();
        worst = worst.max(d1.dot(d2).clamp(-1.0, 1.0).acos());
    }
    worst
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A straight trunk along `TRUNK_Y`, as the world lays it.
    fn trunk() -> RoadNetwork {
        let mut net = RoadNetwork::default();
        net.add_polyline_kind(&[Vec2::new(-2000.0, TRUNK_Y), Vec2::new(2000.0, TRUNK_Y)], 0, 0, crate::roads::RoadKind::Rail);
        net
    }

    #[test]
    fn a_branch_snapped_at_both_ends_leaves_and_rejoins_along_the_trunk() {
        let net = trunk();
        // Drawn as a square loop off the trunk and back onto it.
        let drawn = [Vec2::new(-300.0, TRUNK_Y + 3.0), Vec2::new(-300.0, TRUNK_Y + 300.0), Vec2::new(300.0, TRUNK_Y + 300.0), Vec2::new(300.0, TRUNK_Y - 2.0)];
        let built = smooth_track(&drawn, &net);
        assert!(built.len() > drawn.len() * 4, "sampled into a dense curve: {} points", built.len());
        assert!(built.iter().all(|p| p.is_finite()));
        // Both ends sit on the trunk.
        assert!((built[0].y - TRUNK_Y).abs() < 0.01 && (built.last().unwrap().y - TRUNK_Y).abs() < 0.01);
        // And run along it for the switch lead before turning away.
        let (start_dir, end_dir) = (built[1] - built[0], built[built.len() - 1] - built[built.len() - 2]);
        assert!(start_dir.normalize().dot(Vec2::X).abs() > 0.95, "leaves the trunk tangentially: {start_dir:?}");
        assert!(end_dir.normalize().dot(Vec2::X).abs() > 0.95, "arrives tangentially: {end_dir:?}");
        // No corner tighter than the spline can take.
        assert!(sharpest_turn(&built) < 0.6, "sharpest turn {:.2}", sharpest_turn(&built));
        // Lines that touch no track are only eased, not snapped.
        let free = smooth_track(&[Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(100.0, 100.0)], &net);
        assert!(free[0] == Vec2::ZERO && *free.last().unwrap() == Vec2::new(100.0, 100.0));
        assert!(sharpest_turn(&free) < std::f32::consts::FRAC_PI_2 * 0.5);
        assert_eq!(smooth_track(&[Vec2::ZERO], &net), vec![Vec2::ZERO]);
    }

    #[test]
    fn sharpest_turn_measures_the_worst_corner() {
        assert_eq!(sharpest_turn(&[Vec2::ZERO, Vec2::X]), 0.0);
        let right_angle = [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(20.0, 10.0)];
        assert!((sharpest_turn(&right_angle) - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
        let gentle = [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::new(20.0, 1.0)];
        assert!(sharpest_turn(&gentle) < 0.11);
        let reversal = [Vec2::ZERO, Vec2::new(10.0, 0.0), Vec2::ZERO];
        assert!((sharpest_turn(&reversal) - std::f32::consts::PI).abs() < 1e-4);
    }

    fn train(poly: Vec<Vec2>, s: f32, branch: Option<(f32, f32)>) -> Train {
        Train { kind: TrainKind::Passenger, cars: 2, nodes: Vec::new(), poly, s, prev_s: s, speed: 0.0, stops: Vec::new(), next_stop: 0, dwell: 0.0, passengers: Vec::new(), cargo: Vec::new(), color: 0, held: false, branch }
    }

    #[test]
    fn blocks_hold_a_train_behind_another_on_the_same_track() {
        let line = vec![Vec2::ZERO, Vec2::new(1000.0, 0.0)];
        let mut rail = Rail::default();
        let behind = rail.trains.insert(train(line.clone(), 200.0, None));
        let ahead = rail.trains.insert(train(line.clone(), 200.0 + BLOCK * 0.5, None));
        assert!(rail.blocked(behind), "the train ahead has its tail in the block");
        assert!(!rail.blocked(ahead), "nothing ahead of the leader");
        // Far enough ahead to be clear.
        rail.trains[ahead].s = 200.0 + BLOCK + rail.trains[ahead].length() + 1.0;
        assert!(!rail.blocked(behind));
        // A train on a different track beside this one does not count.
        let other = vec![Vec2::new(0.0, 10.0), Vec2::new(1000.0, 10.0)];
        let beside = rail.trains.insert(train(other, 230.0, None));
        assert!(!rail.blocked(behind));
        assert!(!rail.blocked(beside));
        assert!(!rail.blocked(TrainId::default()), "an unknown train is never blocked");
    }

    #[test]
    fn on_branch_covers_the_whole_train_not_just_its_head() {
        let line = vec![Vec2::ZERO, Vec2::new(1000.0, 0.0)];
        let mut t = train(line, 0.0, Some((300.0, 600.0)));
        let len = t.length();
        t.s = 100.0;
        assert!(!t.on_branch(), "not yet there");
        t.s = 350.0;
        assert!(t.on_branch(), "head on the branch");
        t.s = 600.0 + len * 0.5;
        assert!(t.on_branch(), "head past the branch, tail still on it");
        t.s = 600.0 + len + 1.0;
        assert!(!t.on_branch(), "clear of it");
        assert!(!train(vec![Vec2::ZERO, Vec2::X], 0.5, None).on_branch(), "no branch on the route");
    }
}
