//! Buses. The player draws routes as a loop of stops on streets and places a
//! depot; buses leave the depot in the morning, run the loop all day,
//! dwelling at every stop to let people off and on, and go home at night.
//! People without a bike take the bus for long trips when a route serves
//! both ends. The depot is the condition: no depot, no buses.

use crate::roads::{RoadNetwork, SegmentId};
use crate::traffic::LaneNetwork;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};

new_key_type! {
    pub struct RouteId;
}

/// How far from a street a click still makes a stop.
pub const STOP_SNAP: f32 = 14.0;
/// How far people walk to or from a stop.
pub const STOP_REACH: f32 = 110.0;
/// Trips shorter than this are walked, bus or no bus.
pub const BUS_TRIP_MIN: f32 = 160.0;
pub const BUS_CAPACITY: usize = 40;
/// Seconds a bus stands at a stop.
pub const DWELL_SECONDS: f32 = 18.0;
/// Sim minutes a rider waits before giving up and walking. Traffic runs
/// at the walking time scale (a bus loop of a kilometre takes an hour or
/// two of clock), and the bus is still many times faster than feet.
pub const WAIT_GIVE_UP: u64 = 180;
pub const SERVICE_START: u32 = 5 * 60 + 30;
pub const SERVICE_END: u32 = 23 * 60 + 30;
/// Ticks between two buses leaving a depot.
pub const DISPATCH_GAP: u64 = 4;
pub const BUS_FARE: f32 = 2.5;
pub const BUS_UPKEEP_PER_DAY: f32 = 150.0;
pub const DEPOT_COST: f32 = 15_000.0;
pub const DEPOT_UPKEEP_PER_DAY: f32 = 250.0;
pub const DEPOT_CAPACITY: u32 = 10;
pub const DEPOT_JOBS: u16 = 12;
pub const DEPOT_MIN_AREA: f32 = 500.0;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BusStop {
    pub segment: SegmentId,
    /// Kerb-side point on the lane the stop serves.
    pub pos: Vec2,
}

impl BusStop {
    /// Snap a click to the nearest street: the stop sits by the kerb on
    /// the side that was clicked, so buses driving that way serve it.
    pub fn snap(roads: &RoadNetwork, p: Vec2) -> Option<BusStop> {
        let (segment, _, q) = roads.nearest_street(p)?;
        if q.distance(p) > STOP_SNAP {
            return None;
        }
        let (a, b) = roads.segment_points(segment);
        let d = (b - a).normalize_or_zero();
        let right = Vec2::new(-d.y, d.x);
        let side = if (p - q).dot(right) >= 0.0 { 1.0 } else { -1.0 };
        let hw = roads.segment(segment).half_width();
        Some(BusStop { segment, pos: q + right * side * (hw - 1.4).max(1.5) })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BusRoute {
    pub name: String,
    /// Index into the vehicle palette, so a line's buses match.
    pub color: u8,
    pub stops: Vec<BusStop>,
    /// Buses the line should run.
    pub buses: u8,
    pub fare: f32,
    #[serde(default)]
    pub riders_today: u32,
    /// Lane polyline of each leg (stop i to i+1, wrapping), for drawing.
    #[serde(skip)]
    pub legs: Vec<Vec<Vec2>>,
    /// Legs with no road between their stops.
    #[serde(skip)]
    pub broken: Vec<usize>,
}

impl BusRoute {
    pub fn runnable(&self) -> bool {
        self.stops.len() >= 2 && self.broken.is_empty()
    }

    /// Stops from `a` forward around the loop to `b`.
    pub fn hops(&self, a: usize, b: usize) -> usize {
        let n = self.stops.len().max(1);
        (b + n - a) % n
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TransitStats {
    pub buses: u32,
    pub depots: u32,
    pub depot_capacity: u32,
    pub waiting: u32,
    pub riding: u32,
    pub riders_today: u32,
    pub fares_today: f32,
    pub upkeep_today: f32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Transit {
    pub routes: SlotMap<RouteId, BusRoute>,
    #[serde(default)]
    pub stats: TransitStats,
    #[serde(default)]
    pub next_line: u32,
    #[serde(default)]
    pub last_dispatch_tick: u64,
    /// Bumped on every edit, so the renderer knows to redraw stops.
    #[serde(skip)]
    pub version: u64,
}

impl Transit {
    pub fn new_route(&mut self) -> RouteId {
        self.next_line += 1;
        let n = self.next_line;
        self.version += 1;
        self.routes.insert(BusRoute { name: format!("Line {n}"), color: (n % 8) as u8, stops: Vec::new(), buses: 1, fare: BUS_FARE, riders_today: 0, legs: Vec::new(), broken: Vec::new() })
    }

    /// Recompute every leg's lane polyline after stops or roads change.
    pub fn refresh_legs(&mut self, net: &LaneNetwork) {
        self.version += 1;
        for route in self.routes.values_mut() {
            route.legs.clear();
            route.broken.clear();
            let n = route.stops.len();
            if n < 2 {
                continue;
            }
            for i in 0..n {
                let a = route.stops[i];
                let b = route.stops[(i + 1) % n];
                match net.find_route(a.segment, a.pos, b.segment, b.pos) {
                    Some((lanes, s0, s1)) => route.legs.push(net.polyline(&lanes, s0, s1)),
                    None => {
                        route.legs.push(vec![a.pos, b.pos]);
                        route.broken.push(i);
                    }
                }
            }
        }
    }

    /// The route and stop pair for a trip, if a line serves both ends: the
    /// pair with the shortest walking at either end, as long as the ride
    /// goes forward round the loop and saves real distance.
    pub fn plan_trip(&self, from: Vec2, to: Vec2) -> Option<(RouteId, usize, usize)> {
        if from.distance(to) < BUS_TRIP_MIN {
            return None;
        }
        let mut best: Option<(f32, RouteId, usize, usize)> = None;
        for (id, route) in &self.routes {
            if !route.runnable() || route.buses == 0 {
                continue;
            }
            let nearest = |p: Vec2| -> Option<(usize, f32)> {
                route.stops.iter().enumerate().map(|(i, s)| (i, s.pos.distance(p))).filter(|(_, d)| *d <= STOP_REACH).min_by(|a, b| a.1.total_cmp(&b.1))
            };
            let (Some((a, da)), Some((b, db))) = (nearest(from), nearest(to)) else { continue };
            if a == b {
                continue;
            }
            let walking = da + db;
            if walking + 60.0 > from.distance(to) {
                continue;
            }
            if best.is_none_or(|(w, ..)| walking < w) {
                best = Some((walking, id, a, b));
            }
        }
        best.map(|(_, id, a, b)| (id, a, b))
    }

    /// Nearest stop of any route to a point within `radius`.
    pub fn stop_near(&self, p: Vec2, radius: f32) -> Option<(RouteId, usize)> {
        let mut best: Option<(f32, RouteId, usize)> = None;
        for (id, route) in &self.routes {
            for (i, s) in route.stops.iter().enumerate() {
                let d = s.pos.distance(p);
                if d <= radius && best.is_none_or(|(bd, ..)| d < bd) {
                    best = Some((d, id, i));
                }
            }
        }
        best.map(|(_, id, i)| (id, i))
    }

    /// Minutes of the day when buses are out.
    pub fn in_service(minute_of_day: u32) -> bool {
        (SERVICE_START..SERVICE_END).contains(&minute_of_day)
    }
}

/// A citizen's bus trip in progress: walk to `board`, ride to `alight`,
/// then carry on to the destination.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct TransitTrip {
    pub route: RouteId,
    pub board: usize,
    pub alight: usize,
    pub target: crate::buildings::BuildingId,
    pub purpose: crate::citizens::Purpose,
    /// Tick the wait began (0 until the rider reaches the stop).
    pub since: u64,
}
