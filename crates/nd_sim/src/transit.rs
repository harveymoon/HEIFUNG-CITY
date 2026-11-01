//! Buses. The player draws routes as a loop of stops on streets and places a
//! depot; buses leave the depot in the morning, run the loop all day,
//! dwelling at every stop to let people off and on, and go home at night.
//! People without a bike take the bus for long trips when a route serves
//! both ends. The depot is the condition: no depot, no buses.

use crate::roads::{RoadNetwork, SegmentId};
use crate::traffic::{CarId, LaneId, LaneNetwork};

use crate::timetable::{Kind, Timeline, Timetable};
use glam::Vec2;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};

new_key_type! {
    pub struct RouteId;
}

/// How far from a street a click still makes a stop.
pub const STOP_SNAP: f32 = 14.0;
/// How far people walk to or from a stop. [pt-transit] 110 m left two
/// commutes in five with no stop at the door end, in Harvey's day-67 city;
/// how long the walk takes is in the rider's sums either way.
pub const STOP_REACH: f32 = 160.0;
/// Trips shorter than this are walked, bus or no bus.
pub const BUS_TRIP_MIN: f32 = 160.0;
pub const BUS_CAPACITY: usize = 40;
/// [bike-racks] Bikes a bus carries on its front rack.
pub const BUS_BIKE_RACK: usize = 4;
/// Seconds a bus stands at a stop.
pub const DWELL_SECONDS: f32 = 18.0;
/// Sim minutes a rider waits before giving up and walking. Vehicles move
/// `traffic_seconds_per_tick` (4) seconds per clock minute, so a bus loop of
/// a kilometre comes round in well under an hour; three hours is a generous
/// ceiling for a sparse or broken line.
pub const WAIT_GIVE_UP: u64 = 180;
pub const SERVICE_START: u32 = 5 * 60 + 30;
pub const SERVICE_END: u32 = 23 * 60 + 30;
/// [r4-metroloop] Minutes after a service ends that the last run may still
/// call (twice that for the trains, which cross the map); then whoever is
/// still waiting gives up (`service_over`).
pub const LAST_RUN_GRACE: u32 = 30;

/// [r4-metroloop] Has a service running `start..end` (minutes of the day;
/// `end` may be 1440) stopped for the night at least `grace` minutes ago?
pub fn service_over(minute_of_day: u32, start: u32, end: u32, grace: u32) -> bool {
    if (start..end).contains(&minute_of_day) {
        return false;
    }
    (minute_of_day + 1440 - end) % 1440 >= grace
}
/// Ticks between two buses leaving a depot.
pub const DISPATCH_GAP: u64 = 4;
/// [pt-transit] How far riders walk to change from one line to another.
pub const TRANSFER_WALK: f32 = 90.0;
/// [pt-transit] Minutes a change of line costs on top of the second wait
/// (finding the stop, the uncertainty).
pub const TRANSFER_PENALTY: f32 = 5.0;
/// [pt-transit] Minutes a rider expects to wait at the stop: half the
/// line's headway, within these bounds.
pub const MIN_EXPECTED_WAIT: f32 = 3.0;
pub const MAX_EXPECTED_WAIT: f32 = 30.0;
/// [pt-transit] The longest a line's buses are spaced when released from
/// the depot (ticks).
pub const MAX_RELEASE_GAP: u64 = 20;
/// [pt-transit] Ticks before a line whose bus could not be sent (no depot
/// with room, none that reaches the first stop) is tried again.
pub const DISPATCH_RETRY: u64 = 15;
/// [pt-transit] A bus that calls at a stop less than half a headway after
/// the one before it stands up to this many extra seconds, so buses on a
/// line spread out instead of bunching (only where other traffic can pass
/// it).
pub const HOLD_MAX_SECONDS: f32 = 45.0;
/// [pt-transit] A leg whose drive is longer than this many times the
/// distance between its stops, plus [`LONG_WAY_SLACK`] metres, is flagged
/// as going the long way round (a one-way street, a stop on the far kerb).
pub const LONG_WAY_FACTOR: f32 = 2.5;
pub const LONG_WAY_SLACK: f32 = 250.0;
/// [pt-transit] A click this near an existing stop of any line is that
/// stop: lines share the kerb.
pub const SHARE_STOP: f32 = 10.0;
pub const BUS_FARE: f32 = 2.5;
/// [pt-transit] The highest fare a line may charge.
pub const MAX_FARE: f32 = 20.0;
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
/// Metres per second a bus is assumed to average, stops and lights
/// included, for a leg no bus has timed yet (about 22 km/h).
pub const BUS_ESTIMATE_SPEED: f32 = 6.0;

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct BusStop {
    pub segment: SegmentId,
    /// Kerb-side point on the lane the stop serves.
    pub pos: Vec2,
    /// Who got on and off here, and how long the leg from here to the next
    /// stop takes. Kept on the stop itself so the figures follow it when
    /// stops before it are removed (the Lines window's strip diagram).
    #[serde(default)]
    pub stats: StopStats,
    /// [w1] On the way back of a two-way line: the far kerb of one of the
    /// line's own stops, kept in step with it (`World::set_bus_route_two_way`).
    /// Such stops come after all the others.
    #[serde(default)]
    pub back: bool,
}

/// A stop's ridership and the timing of the leg that leaves it, for the
/// player tuning a line. Counted, never drawn from the random generator.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StopStats {
    pub on_today: u32,
    pub off_today: u32,
    pub on_yesterday: u32,
    pub off_yesterday: u32,
    /// Sim minutes from pulling in at this stop to pulling in at the next
    /// (the dwell here plus the drive), smoothed over the buses that ran it;
    /// `0` until a bus has. Reset when the leg changes.
    pub leg_minutes: f32,
    /// Runs timed into `leg_minutes`.
    pub leg_runs: u32,
    /// [pt-transit] Tick the last bus called here (0: none yet), for
    /// keeping the buses spaced.
    #[serde(default)]
    pub last_call: u64,
}

impl StopStats {
    /// Weight of the newest run in `leg_minutes`: a few runs settle it,
    /// one jam does not swing it.
    const LEG_SMOOTHING: f32 = 0.3;

    /// Fold one timed run of the leg into the average.
    pub fn time_leg(&mut self, minutes: f32) {
        self.leg_minutes = if self.leg_runs == 0 { minutes } else { self.leg_minutes + (minutes - self.leg_minutes) * Self::LEG_SMOOTHING };
        self.leg_runs = self.leg_runs.saturating_add(1);
    }

    /// The leg changed (a stop moved, or the next stop went): its old time
    /// says nothing about the new one.
    pub fn forget_leg(&mut self) {
        self.leg_minutes = 0.0;
        self.leg_runs = 0;
    }

    /// Midnight: today's counts become yesterday's.
    pub fn roll_day(&mut self) {
        self.on_yesterday = self.on_today;
        self.off_yesterday = self.off_today;
        self.on_today = 0;
        self.off_today = 0;
    }
}

/// [r4] (K3) Why no stop can go at `p`, for the tools to say: a highway
/// or a tunnel there and no street.
pub fn stop_refusal(roads: &RoadNetwork, p: Vec2) -> String {
    match roads.nearest_street(p) {
        Some((sid, _, q)) if q.distance(p) <= STOP_SNAP => {
            let s = roads.segment(sid);
            if s.tunnel {
                "no stop in a tunnel: put it on a street at ground level".into()
            } else if s.is_highway() {
                "no stop on a highway carriageway: there is no kerb to pull in at (a slip road or a street will take one)".into()
            } else {
                "no kerb here a bus can pull in at".into()
            }
        }
        _ => "click next to a street".into(),
    }
}

impl BusStop {
    /// Snap a click to the nearest street: the stop sits by the kerb on
    /// the side that was clicked, so buses driving that way serve it.
    pub fn snap(roads: &RoadNetwork, p: Vec2) -> Option<BusStop> {
        // [r4] (K3) Only where a bus can pull in: not in a bored tunnel
        // under the street clicked (the stop snapped to a tunnel running
        // under a roundabout), nor on a highway carriageway. Slip roads
        // and one-way streets take stops at their kerb. Nor on a piece of
        // street too short to have lanes.
        let (segment, _, q) = roads.nearest_where(p, |s| s.stop_allowed() && roads.node(s.a).pos.distance(roads.node(s.b).pos) >= 0.5)?;
        if q.distance(p) > STOP_SNAP {
            return None;
        }
        let (a, b) = roads.segment_points(segment);
        let d = (b - a).normalize_or_zero();
        let right = Vec2::new(-d.y, d.x);
        let side = if (p - q).dot(right) >= 0.0 { 1.0 } else { -1.0 };
        let hw = roads.segment(segment).half_width();
        Some(BusStop { segment, pos: q + right * side * (hw - 1.4).max(1.5), stats: StopStats::default(), back: false })
    }

    /// [w1] The same spot on the far kerb, served by buses driving the other
    /// way: the way back of a two-way line. `None` on a one-way street (or a
    /// street that has gone).
    pub fn far_kerb(&self, roads: &RoadNetwork) -> Option<BusStop> {
        // The street the stop stands by now (its own id can be stale after
        // the roads were rebuilt), as `snap` finds it.
        let (segment, _, q) = roads.nearest_where(self.pos, |s| s.stop_allowed() && roads.node(s.a).pos.distance(roads.node(s.b).pos) >= 0.5)?;
        if q.distance(self.pos) > STOP_SNAP {
            return None;
        }
        let seg = roads.segment(segment);
        if seg.lanes_fwd == 0 || seg.lanes_bwd == 0 {
            return None;
        }
        let (a, b) = roads.segment_points(segment);
        let d = (b - a).normalize_or_zero();
        if d == Vec2::ZERO {
            return None;
        }
        let right = Vec2::new(-d.y, d.x);
        let side = if (self.pos - q).dot(right) >= 0.0 { 1.0 } else { -1.0 };
        let hw = seg.half_width();
        Some(BusStop { segment, pos: q - right * side * (hw - 1.4).max(1.5), stats: StopStats::default(), back: true })
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
    /// [r4] (K3) Of those, the legs a bus could drive the other way (from
    /// the next stop back to this one): a one-way street runs against the
    /// line there.
    #[serde(skip)]
    pub against: Vec<usize>,
    /// [pt-transit] The lanes of each leg as the buses drive them (lanes,
    /// `s` on the first, `s` on the last): worked out with the polylines,
    /// so a bus drives the line that is drawn. `None` for a broken leg.
    #[serde(skip)]
    pub leg_lanes: Vec<Option<(Vec<LaneId>, f32, f32)>>,
    /// [pt-transit] Tick before which no further bus of this line leaves a
    /// depot: buses go out a headway apart, not all at once.
    #[serde(default)]
    pub next_release: u64,
    /// [pt-transit] Why the last bus this line wanted was not sent, if it
    /// was not. Cleared when one goes out.
    #[serde(skip)]
    pub dispatch: Option<DispatchProblem>,
    /// [pt-transit] The line's books: yesterday's riders (boardings),
    /// fares and running costs, and today's fares so far.
    #[serde(default)]
    pub riders_yesterday: u32,
    #[serde(default)]
    pub fares_today: f32,
    #[serde(default)]
    pub fares_yesterday: f32,
    /// Bus-minutes run today (one bus out for one tick is one), from which
    /// the line's share of the fleet's upkeep is charged at midnight.
    #[serde(default)]
    pub bus_minutes_today: u32,
    #[serde(default)]
    pub upkeep_yesterday: f32,
    /// [w1] Runs out along its stops and back along the far kerbs of the
    /// same streets (the way back's stops are [`BusStop::back`]), instead
    /// of round a loop one way.
    #[serde(default)]
    pub two_way: bool,
    /// [r4-timetable] Hours and buses out part by part; `None` (an older
    /// save, or never set) runs `buses` all through the standard hours
    /// (`BusRoute::schedule`).
    #[serde(default)]
    pub timetable: Option<Timetable>,
    /// [r4-timetable] The line's day hour by hour, today and yesterday.
    #[serde(default)]
    pub timeline: Timeline,
    /// [r4-timetable] Buses out beyond what the timetable wants now: that
    /// many go home from their next stop (`World::step_transit`).
    #[serde(skip)]
    pub retire: u32,
}

/// [pt-transit] Why a line that wants a bus did not get one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DispatchProblem {
    /// No working depot in town.
    NoDepot,
    /// Every depot already has its fleet out.
    DepotsFull,
    /// No depot with room has a road to the first stop.
    Unreachable,
}

/// [pt-transit] Something wrong with a line, as the Lines window and the
/// remote API report it (`World::line_problems`, `Transit::check_stops`).
/// Leg `i` runs from stop `i` to stop `i + 1` (the last back to the first).
#[derive(Clone, Debug, PartialEq)]
pub enum LineProblem {
    /// Fewer than two stops: nothing to run.
    TooFewStops,
    /// No road from stop `leg` to the next: the line cannot run.
    NoRoad { leg: usize },
    /// [r4] (K3) No road from stop `leg` to the next going this way, though
    /// there is the other way: a one-way street runs against the line.
    OneWayAgainst { leg: usize },
    /// The bus has to go a long way round between these stops.
    LongWayRound { leg: usize, metres: f32, direct: f32 },
    /// The line is set to run no buses.
    NoBuses,
    /// No depot to send buses from.
    NoDepot,
    /// No depot with room can reach the first stop by road.
    DepotUnreachable,
    /// More buses are wanted across the lines than the depots hold.
    FleetShort { wanted: u32, capacity: u32 },
    /// Every depot has its fleet out: this line waits for a bus.
    DepotsFull,
    /// Night: the buses are home until the line's timetable starts them
    /// (`start`; they stopped at `end`).
    OutOfService { start: u32, end: u32 },
}

impl LineProblem {
    /// Stops a line from running at all (as opposed to a warning).
    pub fn blocks(&self) -> bool {
        matches!(self, LineProblem::TooFewStops | LineProblem::NoRoad { .. } | LineProblem::OneWayAgainst { .. } | LineProblem::NoBuses | LineProblem::NoDepot | LineProblem::DepotUnreachable)
    }

    /// The leg it is about, for marking on the map or the strip.
    pub fn leg(&self) -> Option<usize> {
        match self {
            LineProblem::NoRoad { leg } | LineProblem::OneWayAgainst { leg } | LineProblem::LongWayRound { leg, .. } => Some(*leg),
            _ => None,
        }
    }

    /// A short machine-readable name (the remote API's `kind`).
    pub fn kind(&self) -> &'static str {
        match self {
            LineProblem::TooFewStops => "too_few_stops",
            LineProblem::NoRoad { .. } => "no_road",
            LineProblem::OneWayAgainst { .. } => "one_way_against",
            LineProblem::LongWayRound { .. } => "long_way_round",
            LineProblem::NoBuses => "no_buses",
            LineProblem::NoDepot => "no_depot",
            LineProblem::DepotUnreachable => "depot_unreachable",
            LineProblem::FleetShort { .. } => "fleet_short",
            LineProblem::DepotsFull => "depots_full",
            LineProblem::OutOfService { .. } => "out_of_service",
        }
    }
}

impl std::fmt::Display for LineProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let hm = |m: u32| format!("{:02}:{:02}", m / 60, m % 60);
        match self {
            LineProblem::TooFewStops => write!(f, "needs at least two stops"),
            LineProblem::NoRoad { leg } => write!(f, "no road from stop {} to the next: move one of them, or join the streets", leg + 1),
            LineProblem::OneWayAgainst { leg } => write!(f, "stop {} to the next runs against a one-way street (buses only drive one-way streets and ramps the way they go): swap the two stops round, or add a stop on a street that leads back", leg + 1),
            LineProblem::LongWayRound { leg, metres, direct } => write!(f, "stop {} to the next is a {:.0} m drive for {:.0} m apart (a one-way street, or a stop on the far kerb?)", leg + 1, metres, direct),
            LineProblem::NoBuses => write!(f, "set to run no buses"),
            LineProblem::NoDepot => write!(f, "no bus depot: build one, buses start and end the day there"),
            LineProblem::DepotUnreachable => write!(f, "no depot can reach the first stop by road"),
            LineProblem::FleetShort { wanted, capacity } => write!(f, "the lines want {wanted} buses but the depots hold {capacity}: build another depot or run fewer buses"),
            LineProblem::DepotsFull => write!(f, "every depot has its buses out: this line waits for one"),
            LineProblem::OutOfService { start, end } => write!(f, "night: buses run {} to {}", hm(*start), hm(*end)),
        }
    }
}

impl BusRoute {
    /// An empty line with one bus at the standard fare.
    pub fn new(name: String, color: u8) -> BusRoute {
        BusRoute {
            name,
            color,
            stops: Vec::new(),
            buses: 1,
            fare: BUS_FARE,
            riders_today: 0,
            legs: Vec::new(),
            broken: Vec::new(),
            against: Vec::new(),
            leg_lanes: Vec::new(),
            next_release: 0,
            dispatch: None,
            riders_yesterday: 0,
            fares_today: 0.0,
            fares_yesterday: 0.0,
            bus_minutes_today: 0,
            upkeep_yesterday: 0.0,
            two_way: false,
            timetable: None,
            timeline: Timeline::default(),
            retire: 0,
        }
    }

    /// [r4-timetable] The line's timetable: its own, else `buses` all
    /// through the standard hours (as every line ran before timetables).
    pub fn schedule(&self) -> Timetable {
        self.timetable.unwrap_or_else(|| Timetable::flat(Kind::Bus, self.buses as u32))
    }

    /// [r4-timetable] Buses the line wants out at minute `m` of the day.
    pub fn buses_at(&self, m: u32) -> u32 {
        match &self.timetable {
            Some(t) => t.value_at(m).unwrap_or(0),
            None if Transit::in_service(m % 1440) => self.buses as u32,
            None => 0,
        }
    }

    /// [r4-timetable] The most buses the line runs at any time of day.
    pub fn peak_buses(&self) -> u32 {
        self.timetable.map_or(self.buses as u32, |t| t.peak(Kind::Bus))
    }

    /// [r4-timetable] Ticks between buses with `n` of them out.
    pub fn headway_with(&self, traffic_seconds_per_tick: f32, n: u32) -> u64 {
        ((self.loop_minutes(traffic_seconds_per_tick) / n.max(1) as f32).round() as u64).max(DISPATCH_GAP)
    }

    /// [w1] Stops on the way out: all but a two-way line's way back (which
    /// comes last).
    pub fn outbound_len(&self) -> usize {
        self.stops.iter().take_while(|s| !s.back).count()
    }

    /// [pt-transit] Sim minutes a bus takes round the whole loop (timed
    /// legs as timed, the rest estimated).
    pub fn loop_minutes(&self, traffic_seconds_per_tick: f32) -> f32 {
        (0..self.stops.len()).map(|i| self.leg_minutes(i, traffic_seconds_per_tick).0).sum()
    }

    /// [pt-transit] Ticks between this line's buses when all of them are
    /// out: the loop over the fleet. Buses leave the depot this far apart
    /// and are held at stops to keep it.
    pub fn headway(&self, traffic_seconds_per_tick: f32) -> u64 {
        self.headway_with(traffic_seconds_per_tick, self.buses as u32)
    }

    /// [pt-transit] Ticks between two of this line's buses leaving the
    /// depot: the headway, up to [`MAX_RELEASE_GAP`] (each joins the loop
    /// at its own stop, spread round it, so a long loop need not wait
    /// hours for its last bus).
    pub fn release_gap(&self, traffic_seconds_per_tick: f32) -> u64 {
        self.headway(traffic_seconds_per_tick).min(MAX_RELEASE_GAP)
    }

    /// [pt-transit] Problems with the line's route itself: too few stops,
    /// legs with no road, legs that go a long way round.
    pub fn route_problems(&self) -> Vec<LineProblem> {
        let n = self.stops.len();
        if n < 2 {
            return vec![LineProblem::TooFewStops];
        }
        let mut out = Vec::new();
        for i in 0..n {
            if self.broken.contains(&i) {
                out.push(if self.against.contains(&i) { LineProblem::OneWayAgainst { leg: i } } else { LineProblem::NoRoad { leg: i } });
                continue;
            }
            let direct = self.stops[i].pos.distance(self.stops[(i + 1) % n].pos);
            let metres = self.leg_length(i);
            if self.legs.len() == n && metres > direct * LONG_WAY_FACTOR + LONG_WAY_SLACK {
                out.push(LineProblem::LongWayRound { leg: i, metres, direct });
            }
        }
        out
    }

    /// [pt-transit] Midnight for the line's books: today's figures become
    /// yesterday's, with `upkeep` the running cost charged for today.
    pub fn roll_books(&mut self, upkeep: f32) {
        self.riders_yesterday = self.riders_today;
        self.riders_today = 0;
        self.fares_yesterday = self.fares_today;
        self.fares_today = 0.0;
        self.upkeep_yesterday = upkeep;
        self.bus_minutes_today = 0;
    }

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
        if self.stops.len() < 2 {
            return 0.0;
        }
        (0..self.hops(a, b)).map(|k| self.leg_length((a + k) % self.stops.len())).sum()
    }

    /// Metres of the leg from stop `i` to the next one round the loop:
    /// along its lane polyline, or as the crow flies before the legs are
    /// computed (right after a load).
    pub fn leg_length(&self, i: usize) -> f32 {
        let n = self.stops.len();
        if n < 2 || i >= n {
            return 0.0;
        }
        match self.legs.get(i) {
            Some(leg) if self.legs.len() == n => crate::geom::path_length(leg),
            _ => self.stops[i].pos.distance(self.stops[(i + 1) % n].pos),
        }
    }

    /// Sim minutes the leg from stop `i` takes: as timed by the buses, or,
    /// before any has run it, an estimate at [`BUS_ESTIMATE_SPEED`] plus
    /// the dwell. The flag says whether it was timed.
    pub fn leg_minutes(&self, i: usize, traffic_seconds_per_tick: f32) -> (f32, bool) {
        match self.stops.get(i) {
            Some(s) if s.stats.leg_runs > 0 => (s.stats.leg_minutes, true),
            _ => ((self.leg_length(i) / BUS_ESTIMATE_SPEED + DWELL_SECONDS) / traffic_seconds_per_tick.max(0.1), false),
        }
    }

    /// Boardings across the line yesterday (riders are counted when they
    /// board, so this is the line's ridership).
    pub fn boardings_yesterday(&self) -> u32 {
        self.stops.iter().map(|s| s.stats.on_yesterday).sum()
    }

    /// Where a bus on this line is along the loop, in stops: `2.4` is 40%
    /// of the way from the third stop to the fourth. From the stop it heads
    /// for and how far along the leg into it the point `pos` lies. `None`
    /// when it is not running the loop (going home, or the line changed).
    pub fn bus_progress(&self, next_stop: usize, pos: Vec2) -> Option<f32> {
        let n = self.stops.len();
        if n < 2 || next_stop >= n {
            return None;
        }
        let prev = (next_stop + n - 1) % n;
        let leg: Vec<Vec2> = match self.legs.get(prev) {
            Some(l) if self.legs.len() == n && l.len() >= 2 => l.clone(),
            _ => vec![self.stops[prev].pos, self.stops[next_stop].pos],
        };
        let total = crate::geom::path_length(&leg).max(1e-3);
        // Nearest point on the polyline, as distance along it.
        let (mut best, mut along, mut walked) = (f32::MAX, 0.0, 0.0);
        for w in leg.windows(2) {
            let (a, b) = (w[0], w[1]);
            let ab = b - a;
            let len = ab.length();
            let t = if len > 1e-6 { ((pos - a).dot(ab) / (len * len)).clamp(0.0, 1.0) } else { 0.0 };
            let d = pos.distance(a + ab * t);
            if d < best {
                best = d;
                along = walked + len * t;
            }
            walked += len;
        }
        Some(prev as f32 + (along / total).clamp(0.0, 1.0))
    }

    /// Midnight: every stop's counts move to yesterday.
    pub fn roll_day(&mut self) {
        for s in &mut self.stops {
            s.stats.roll_day();
        }
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
    /// Each bus's last stop and the tick it pulled in there, to time the
    /// leg to the next one ([`StopStats::leg_minutes`]). A bus missing here
    /// has not served a stop yet: it is still on its way from the depot.
    /// [review] Saved (ordered, so the save is stable), so a loaded game
    /// times the leg under way as the running one does.
    #[serde(default)]
    pub departures: std::collections::BTreeMap<CarId, (usize, u64)>,
    /// [bike-racks] Cyclists who rode a bus with their bike on its rack,
    /// and the times one was left at the stop because the rack was full,
    /// ever.
    #[serde(default)]
    pub bike_rack_rides_total: u64,
    #[serde(default)]
    pub bike_rack_full_total: u64,
    /// [r4-timetable] The city's default timetable: new lines of every
    /// kind start on it (`None`: the standard hours, one frequency all day).
    #[serde(default)]
    pub default_preset: Option<crate::timetable::Preset>,
}

impl Transit {
    pub fn new_route(&mut self) -> RouteId {
        self.next_line += 1;
        let n = self.next_line;
        self.version += 1;
        let mut r = BusRoute::new(format!("Line {n}"), (n % 8) as u8);
        // [r4-timetable] On the city's default timetable.
        r.timetable = self.default_preset.map(|p| p.timetable(Kind::Bus, r.buses as u32));
        self.routes.insert(r)
    }

    /// Recompute every leg's lane polyline after stops or roads change.
    pub fn refresh_legs(&mut self, net: &LaneNetwork) {
        self.version += 1;
        for route in self.routes.values_mut() {
            route.legs.clear();
            route.broken.clear();
            route.against.clear();
            route.leg_lanes.clear();
            let n = route.stops.len();
            if n < 2 {
                continue;
            }
            for (i, leg) in Self::plan_legs(net, &route.stops).into_iter().enumerate() {
                let a = route.stops[i];
                let b = route.stops[(i + 1) % n];
                match leg {
                    Some((lanes, s0, s1)) => {
                        route.legs.push(net.polyline(&lanes, s0, s1));
                        route.leg_lanes.push(Some((lanes, s0, s1)));
                    }
                    None => {
                        route.legs.push(vec![a.pos, b.pos]);
                        route.broken.push(i);
                        if net.find_route_round(b.segment, b.pos, a.segment, a.pos).is_some() {
                            route.against.push(i);
                        }
                        route.leg_lanes.push(None);
                    }
                }
            }
        }
    }

    /// [pt-transit] The lanes of each leg of a loop through `stops` (stop
    /// `i` to `i + 1`, the last back to the first), `None` where no road
    /// joins them. Each leg starts on the lane its stop stands by, so a bus
    /// that has driven one leg is on the lane the next begins with.
    pub fn plan_legs(net: &LaneNetwork, stops: &[BusStop]) -> Vec<Option<(Vec<LaneId>, f32, f32)>> {
        let n = stops.len();
        if n < 2 {
            return Vec::new();
        }
        (0..n)
            .map(|i| {
                let (a, b) = (stops[i], stops[(i + 1) % n]);
                net.find_route_round(a.segment, a.pos, b.segment, b.pos)
            })
            .collect()
    }

    /// [pt-transit] Check a line before it is committed: the problems a
    /// loop through `stops` would have (too few stops, no road between two,
    /// a leg the long way round), as `BusRoute::route_problems` reports
    /// them for a line. For the Lines window to show while a line is being
    /// edited, before Finish.
    pub fn check_stops(net: &LaneNetwork, stops: &[BusStop]) -> Vec<LineProblem> {
        let mut r = BusRoute::new(String::new(), 0);
        r.stops = stops.to_vec();
        for (i, leg) in Self::plan_legs(net, stops).into_iter().enumerate() {
            match leg {
                Some((lanes, s0, s1)) => r.legs.push(net.polyline(&lanes, s0, s1)),
                None => {
                    let (a, b) = (stops[i], stops[(i + 1) % stops.len()]);
                    r.legs.push(vec![a.pos, b.pos]);
                    r.broken.push(i);
                    if net.find_route_round(b.segment, b.pos, a.segment, a.pos).is_some() {
                        r.against.push(i);
                    }
                }
            }
        }
        r.route_problems()
    }

    /// [pt-transit] A stop for a click at `p`: an existing stop of any line
    /// within [`SHARE_STOP`] (lines share the kerb), else a new one snapped
    /// to the nearest street.
    pub fn stop_for_click(&self, roads: &RoadNetwork, p: Vec2) -> Option<BusStop> {
        let shared = self.routes.values().flat_map(|r| r.stops.iter()).filter(|s| s.pos.distance(p) <= SHARE_STOP).min_by(|a, b| a.pos.distance(p).total_cmp(&b.pos.distance(p)));
        match shared {
            Some(s) => Some(BusStop { segment: s.segment, pos: s.pos, stats: StopStats::default(), back: false }),
            None => BusStop::snap(roads, p),
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
        self.plan_trip_when(from, to, &|_, _| true)
    }

    /// [r4-timetable] `plan_trip` among the lines `runs` accepts: given a
    /// line and the metres walked to its boarding stop, does it run when
    /// the rider gets there?
    pub fn plan_trip_when(&self, from: Vec2, to: Vec2, runs: &dyn Fn(&BusRoute, f32) -> bool) -> Option<(RouteId, usize, usize)> {
        let direct = from.distance(to);
        if direct < BUS_TRIP_MIN {
            return None;
        }
        let mut best: Option<(f32, RouteId, usize, usize)> = None;
        for (id, route) in &self.routes {
            // [pt-transit] Nor lines no bus can be sent to: riders stood
            // there for hours and learnt to hate the bus.
            if !route.runnable() || route.peak_buses() == 0 || matches!(route.dispatch, Some(DispatchProblem::NoDepot | DispatchProblem::Unreachable)) {
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
                    if route.ride_length(a, b) > direct * RIDE_DETOUR + RIDE_SLACK || !runs(route, da) {
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

    /// [pt-transit] A trip on two lines, changing between a stop of the
    /// first and a stop of the second within [`TRANSFER_WALK`]: `(first
    /// line, board, alight, second line, board, alight)`, the one with the
    /// least walking whose rides each pass the same detour rule as
    /// `plan_trip`. Riders take the first leg; at its end they decide
    /// again, and the second line is the direct ride from there.
    pub fn plan_transfer(&self, from: Vec2, to: Vec2) -> Option<(RouteId, usize, usize, RouteId, usize, usize)> {
        self.plan_transfer_when(from, to, &|_, _| true)
    }

    /// [r4-timetable] `plan_transfer` among the lines `runs` accepts (as
    /// for `plan_trip_when`; the second line is asked with the walk to
    /// the change counted from the start).
    pub fn plan_transfer_when(&self, from: Vec2, to: Vec2, runs: &dyn Fn(&BusRoute, f32) -> bool) -> Option<(RouteId, usize, usize, RouteId, usize, usize)> {
        let direct = from.distance(to);
        if direct < BUS_TRIP_MIN {
            return None;
        }
        let lines: Vec<(RouteId, &BusRoute)> = self.routes.iter().filter(|(_, r)| r.runnable() && r.peak_buses() > 0 && !matches!(r.dispatch, Some(DispatchProblem::NoDepot | DispatchProblem::Unreachable))).collect();
        let mut best: Option<(f32, (RouteId, usize, usize, RouteId, usize, usize))> = None;
        for &(ia, a) in &lines {
            let boards: Vec<(usize, f32)> = a.stops.iter().enumerate().map(|(i, s)| (i, s.pos.distance(from))).filter(|(_, d)| *d <= STOP_REACH).collect();
            if boards.is_empty() {
                continue;
            }
            for &(ib, b) in &lines {
                if ib == ia {
                    continue;
                }
                let alights: Vec<(usize, f32)> = b.stops.iter().enumerate().map(|(i, s)| (i, s.pos.distance(to))).filter(|(_, d)| *d <= STOP_REACH).collect();
                if alights.is_empty() {
                    continue;
                }
                for (x, sx) in a.stops.iter().enumerate() {
                    for (y, sy) in b.stops.iter().enumerate() {
                        let change = sx.pos.distance(sy.pos);
                        if change > TRANSFER_WALK {
                            continue;
                        }
                        for &(p, dp) in &boards {
                            if p == x || a.ride_length(p, x) > from.distance(sx.pos) * RIDE_DETOUR + RIDE_SLACK || !runs(a, dp) {
                                continue;
                            }
                            for &(q, dq) in &alights {
                                if q == y || b.ride_length(y, q) > sy.pos.distance(to) * RIDE_DETOUR + RIDE_SLACK {
                                    continue;
                                }
                                let walking = dp + change + dq;
                                if walking + 60.0 > direct {
                                    continue;
                                }
                                if best.is_none_or(|(w, _)| walking < w) {
                                    best = Some((walking, (ia, p, x, ib, y, q)));
                                }
                            }
                        }
                    }
                }
            }
        }
        best.map(|b| b.1)
    }

    /// [pt-transit] Why `plan_trip` finds no bus for a trip, for the
    /// ridership report: the first reason that rules every line out.
    pub fn why_no_bus(&self, from: Vec2, to: Vec2) -> Option<NoBus> {
        if self.plan_trip(from, to).is_some() || self.plan_transfer(from, to).is_some() {
            return None;
        }
        let direct = from.distance(to);
        if direct < BUS_TRIP_MIN {
            return Some(NoBus::TooShort);
        }
        let lines: Vec<&BusRoute> = self.routes.values().filter(|r| r.runnable() && r.peak_buses() > 0).collect();
        if lines.is_empty() {
            return Some(NoBus::NoLine);
        }
        let near = |r: &BusRoute, p: Vec2| r.stops.iter().any(|s| s.pos.distance(p) <= STOP_REACH);
        let (start, end) = (lines.iter().any(|r| near(r, from)), lines.iter().any(|r| near(r, to)));
        Some(match (start, end) {
            (false, false) => NoBus::NoStopEitherEnd,
            (false, true) => NoBus::NoStopAtStart,
            (true, false) => NoBus::NoStopAtEnd,
            _ if !lines.iter().any(|r| near(r, from) && near(r, to)) => NoBus::NoLineServesBoth,
            // A line serves both ends: the ride round the loop is too long,
            // or the stops are as far as walking it.
            _ => NoBus::LongWayRound,
        })
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
        r.stops = pts.iter().map(|&p| BusStop { segment: SegmentId(0), pos: p, stats: StopStats::default(), back: false }).collect();
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
    fn two_lines_with_a_shared_corner_make_a_trip_with_a_change() {
        // East along y = 0, then north along x = 600: an L the size of two
        // lines, meeting at the corner.
        let mut t = Transit::default();
        let east = line(&mut t, &[Vec2::new(0.0, 0.0), Vec2::new(300.0, 0.0), Vec2::new(600.0, 0.0)], 1);
        let north = line(&mut t, &[Vec2::new(620.0, 20.0), Vec2::new(620.0, 320.0), Vec2::new(620.0, 620.0)], 1);
        let (from, to) = (Vec2::new(-20.0, 10.0), Vec2::new(640.0, 630.0));
        assert_eq!(t.plan_trip(from, to), None, "no one line serves both ends");
        assert_eq!(t.plan_transfer(from, to), Some((east, 0, 2, north, 0, 2)), "east to the corner, change, north");
        assert_eq!(t.why_no_bus(from, to), None);
        // Too far apart to change: no trip.
        t.routes[north].stops[0].pos = Vec2::new(620.0, 200.0);
        assert_eq!(t.plan_transfer(from, to), None);
        assert_eq!(t.why_no_bus(from, to), Some(NoBus::NoLineServesBoth));
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
    fn stop_stats_time_legs_roll_over_and_forget() {
        let mut s = StopStats::default();
        s.time_leg(10.0);
        assert_eq!(s.leg_minutes, 10.0, "the first run is the time");
        s.time_leg(20.0);
        assert!(s.leg_minutes > 10.0 && s.leg_minutes < 15.0, "one slow run moves it a little: {}", s.leg_minutes);
        s.on_today = 4;
        s.off_today = 2;
        s.roll_day();
        assert_eq!((s.on_today, s.off_today, s.on_yesterday, s.off_yesterday), (0, 0, 4, 2));
        s.forget_leg();
        assert_eq!((s.leg_minutes, s.leg_runs), (0.0, 0));
    }

    #[test]
    fn bus_progress_and_leg_estimates_follow_the_loop() {
        let mut t = Transit::default();
        let id = line(&mut t, &block(), 1);
        let r = &t.routes[id];
        // Heading for stop 1, a quarter of the way along the 400 m leg.
        assert!((r.bus_progress(1, Vec2::new(-100.0, 3.0)).unwrap() - 0.25).abs() < 1e-3);
        // Heading for stop 0: on the last leg, from stop 5.
        assert!(r.bus_progress(0, Vec2::new(-300.0, 300.0)).unwrap() >= 5.0);
        assert!(r.bus_progress(usize::MAX, Vec2::ZERO).is_none(), "going home");
        // Untimed legs are estimated from their length; timed ones are read.
        let (est, timed) = r.leg_minutes(0, 4.0);
        assert!(!timed && (est - (400.0 / BUS_ESTIMATE_SPEED + DWELL_SECONDS) / 4.0).abs() < 1e-3);
        t.routes[id].stops[0].stats.time_leg(7.0);
        assert_eq!(t.routes[id].leg_minutes(0, 4.0), (7.0, true));
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

/// [pt-transit] Why a trip has no bus (`Transit::why_no_bus`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NoBus {
    /// Short enough to walk, bus or no bus.
    TooShort,
    /// No line runs (none drawn, none runnable, none with buses).
    NoLine,
    NoStopEitherEnd,
    NoStopAtStart,
    NoStopAtEnd,
    /// Stops near both ends, but on different lines with no change
    /// between them within a short walk.
    NoLineServesBoth,
    /// One line serves both ends but rides the long way round the loop
    /// (lines run one way), or its stops are no nearer than walking.
    LongWayRound,
}

impl NoBus {
    pub const ALL: [NoBus; 7] = [NoBus::TooShort, NoBus::NoLine, NoBus::NoStopEitherEnd, NoBus::NoStopAtStart, NoBus::NoStopAtEnd, NoBus::NoLineServesBoth, NoBus::LongWayRound];

    pub fn key(self) -> &'static str {
        match self {
            NoBus::TooShort => "too_short",
            NoBus::NoLine => "no_line",
            NoBus::NoStopEitherEnd => "no_stop_either_end",
            NoBus::NoStopAtStart => "no_stop_at_start",
            NoBus::NoStopAtEnd => "no_stop_at_end",
            NoBus::NoLineServesBoth => "no_line_serves_both",
            NoBus::LongWayRound => "long_way_round",
        }
    }
}

/// The ride in the middle of a trip.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum TransitLeg {
    Bus { route: RouteId, board: usize, alight: usize },
    Rail { board: crate::buildings::BuildingId, alight: crate::buildings::BuildingId },
    /// A drone taxi from one pad to another (`crate::air`).
    Air { from: crate::buildings::BuildingId, to: crate::buildings::BuildingId },
    /// [subway] A metro ride on one line between two of its stations.
    Subway { line: crate::subway::MetroLineId, board: crate::buildings::BuildingId, alight: crate::buildings::BuildingId },
    /// [taxis] A cab from the kerb to the destination's kerb
    /// (`crate::taxi`); the cab is found by the dispatcher.
    Taxi,
    /// [ferry-islands] A ferry from terminal `board`'s pier to `alight`'s,
    /// both in town (`world/islands.rs`).
    Ferry { board: crate::buildings::BuildingId, alight: crate::buildings::BuildingId },
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
    /// [bike-racks] A cyclist taking the bus with their bike: it rides on
    /// the bus's front rack (`BUS_BIKE_RACK` places), so they board only
    /// while there is a place free and otherwise wait for the next bus.
    #[serde(default)]
    pub bike: bool,
}
