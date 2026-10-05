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
/// Sim minutes a rider waits before giving up and walking. Vehicles move
/// `traffic_seconds_per_tick` (4) seconds per clock minute, so a bus loop of
/// a kilometre comes round in well under an hour; three hours is a generous
/// ceiling for a sparse or broken line.
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
/// A ride may be this many times the crow-flight distance, plus
/// [`RIDE_SLACK`] metres, before walking is the better idea. Buses run a
/// loop one way, so the "forward" ride from a stop to the one just behind
/// it goes nearly all the way round; this is what rules that out.
pub const RIDE_DETOUR: f32 = 2.5;
pub const RIDE_SLACK: f32 = 200.0;

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

    /// Metres a bus drives from stop `a` forward round the loop to stop
    /// `b`, along the legs' lane polylines; stop to stop as the crow flies
    /// when the legs are not computed yet (right after a load).
    pub fn ride_length(&self, a: usize, b: usize) -> f32 {
        let n = self.stops.len();
        if n < 2 {
            return 0.0;
        }
        (0..self.hops(a, b))
            .map(|k| {
                let i = (a + k) % n;
                match self.legs.get(i) {
                    Some(leg) if self.legs.len() == n => crate::geom::path_length(leg),
                    _ => self.stops[i].pos.distance(self.stops[(i + 1) % n].pos),
                }
            })
            .sum()
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

    /// The route and stop pair for a trip, if a line serves both ends: among
    /// the stops within reach of each end, the pair with the shortest
    /// walking, as long as the ride saves real distance and, riding forward
    /// round the loop from the boarding stop, is no more than
    /// [`RIDE_DETOUR`] times the crow-flight distance plus [`RIDE_SLACK`].
    /// So a rider whose nearest stop is just past the destination's does
    /// not go all the way round; a stop a little further back may do.
    pub fn plan_trip(&self, from: Vec2, to: Vec2) -> Option<(RouteId, usize, usize)> {
        let direct = from.distance(to);
        if direct < BUS_TRIP_MIN {
            return None;
        }
        let mut best: Option<(f32, RouteId, usize, usize)> = None;
        for (id, route) in &self.routes {
            if !route.runnable() || route.buses == 0 {
                continue;
            }
            let within = |p: Vec2| -> Vec<(usize, f32)> { route.stops.iter().enumerate().map(|(i, s)| (i, s.pos.distance(p))).filter(|(_, d)| *d <= STOP_REACH).collect() };
            let (boards, alights) = (within(from), within(to));
            for &(a, da) in &boards {
                for &(b, db) in &alights {
                    if a == b {
                        continue;
                    }
                    let walking = da + db;
                    if walking + 60.0 > direct {
                        continue;
                    }
                    if route.ride_length(a, b) > direct * RIDE_DETOUR + RIDE_SLACK {
                        continue;
                    }
                    if best.is_none_or(|(w, ..)| walking < w) {
                        best = Some((walking, id, a, b));
                    }
                }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A runnable loop with stops at `pts`, legs as straight lines.
    fn line(transit: &mut Transit, pts: &[Vec2], buses: u8) -> RouteId {
        let id = transit.new_route();
        let r = &mut transit.routes[id];
        r.stops = pts.iter().map(|&p| BusStop { segment: SegmentId(0), pos: p }).collect();
        let n = pts.len();
        r.legs = (0..n).map(|i| vec![pts[i], pts[(i + 1) % n]]).collect();
        r.buses = buses;
        id
    }

    /// A 600 x 600 m loop served clockwise: six stops, 2065 m round.
    fn block() -> [Vec2; 6] {
        [Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0), Vec2::new(300.0, 300.0), Vec2::new(200.0, 600.0), Vec2::new(-200.0, 600.0), Vec2::new(-300.0, 300.0)]
    }

    #[test]
    fn rides_go_forward_round_the_loop_and_never_nearly_all_the_way_round() {
        let mut t = Transit::default();
        let id = line(&mut t, &block(), 1);
        let r = &t.routes[id];
        assert!(r.runnable());
        assert_eq!(r.hops(5, 2), 3);
        let diag = 100.0f32.hypot(300.0);
        assert!((r.ride_length(0, 1) - 400.0).abs() < 1e-3);
        assert!((r.ride_length(1, 0) - (400.0 + 4.0 * diag)).abs() < 1e-2, "forward from 1 to 0 is the loop bar one leg");
        // West to east along the bottom: board 0, alight 1, a forward ride.
        assert_eq!(t.plan_trip(Vec2::new(-230.0, -20.0), Vec2::new(230.0, -20.0)), Some((id, 0, 1)));
        // East to west along the same street: the only stops within reach
        // are 1 and 0, and riding forward from 1 to 0 is 1665 m for a 460 m
        // trip, more than 2.5 x 460 + 200. Walking it is.
        assert_eq!(t.plan_trip(Vec2::new(230.0, -20.0), Vec2::new(-230.0, -20.0)), None);
        // Short trips are walked; lines with no buses out do not count.
        assert_eq!(t.plan_trip(Vec2::new(-230.0, -20.0), Vec2::new(-100.0, -20.0)), None);
        t.routes[id].buses = 0;
        assert_eq!(t.plan_trip(Vec2::new(-230.0, -20.0), Vec2::new(230.0, -20.0)), None);
    }

    #[test]
    fn a_stop_further_back_is_taken_when_the_nearest_one_rides_the_long_way() {
        // A -> B -> C -> D -> E -> A, 2600 m round, with E a short hop before A.
        let stops = [Vec2::ZERO, Vec2::new(500.0, 0.0), Vec2::new(500.0, 800.0), Vec2::new(0.0, 800.0), Vec2::new(0.0, 200.0)];
        let mut t = Transit::default();
        let id = line(&mut t, &stops, 1);
        // Starting between A and E, a step nearer E, bound for D.
        let (from, to) = (Vec2::new(0.0, 105.0), Vec2::new(20.0, 800.0));
        let r = &t.routes[id];
        assert!(r.ride_length(4, 3) > from.distance(to) * RIDE_DETOUR + RIDE_SLACK, "E to D is the whole loop");
        assert!(r.ride_length(0, 3) <= from.distance(to) * RIDE_DETOUR + RIDE_SLACK, "A to D is not");
        assert_eq!(t.plan_trip(from, to), Some((id, 0, 3)), "board at A, not at the nearer E");
    }

    #[test]
    fn ride_length_falls_back_to_stop_distances_without_legs() {
        let mut t = Transit::default();
        let id = line(&mut t, &block(), 1);
        t.routes[id].legs.clear();
        let r = &t.routes[id];
        assert!((r.ride_length(0, 2) - (400.0 + 100.0f32.hypot(300.0))).abs() < 1e-3);
        assert_eq!(r.ride_length(2, 2), 0.0);
        let mut single = Transit::default();
        let s = line(&mut single, &[Vec2::ZERO], 1);
        assert_eq!(single.routes[s].ride_length(0, 0), 0.0);
    }

    #[test]
    fn stop_near_finds_the_closest_stop_of_any_line() {
        let mut t = Transit::default();
        assert!(t.stop_near(Vec2::ZERO, 50.0).is_none());
        let a = line(&mut t, &block(), 1);
        let b = line(&mut t, &[Vec2::new(0.0, 500.0), Vec2::new(100.0, 500.0), Vec2::new(50.0, 600.0)], 1);
        assert_eq!(t.stop_near(Vec2::new(-190.0, 5.0), 20.0), Some((a, 0)));
        assert_eq!(t.stop_near(Vec2::new(60.0, 590.0), 20.0), Some((b, 2)));
        assert!(t.stop_near(Vec2::new(0.0, 300.0), 50.0).is_none(), "nothing within 50 m");
        // With several stops in range, the closest wins.
        assert_eq!(t.stop_near(Vec2::new(-200.0, 60.0), 300.0), Some((a, 0)));
    }
}

/// The ride in the middle of a trip.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum TransitLeg {
    Bus { route: RouteId, board: usize, alight: usize },
    Rail { board: crate::buildings::BuildingId, alight: crate::buildings::BuildingId },
}

/// A citizen's transit trip in progress: walk to the stop or station,
/// wait, ride, then carry on to the destination.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct TransitTrip {
    pub leg: TransitLeg,
    pub target: crate::buildings::BuildingId,
    pub purpose: crate::citizens::Purpose,
    /// Tick the wait began (0 until the rider reaches the stop).
    pub since: u64,
}
