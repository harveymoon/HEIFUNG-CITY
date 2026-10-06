//! Lane-level traffic: lanes derived from the road graph, generated
//! intersections with connector lanes and signal phases, lane-graph
//! pathfinding, and car-following (intelligent driver model) microtraffic.
//!
//! Junction approaches carry lane arrows (`Lane::arrows`, a mask of
//! `LEFT | STRAIGHT | RIGHT`): by default a two-lane approach has a left-only
//! lane and a straight-and-right lane, wider ones a left lane (two when left
//! demand is high), straight lanes and a straight-and-right lane; a quiet
//! left becomes a shared left-and-straight lane. The defaults follow the
//! turning counts and can be overridden per approach (`Traffic::lane_use`).
//! Connectors exist for every movement and are `permitted` by the arrows,
//! so arrows change without renumbering lanes. Routes include lane changes as steps between parallel lanes
//! of a segment, so a car moves to the lane its next turn needs as early as
//! a gap allows. A change is taken only when it is safe by the IDM for the
//! car and for the follower in the target lane; the car then belongs to
//! the new lane at once and drifts across sideways over a few seconds
//! (`Car::lateral`). Cars also overtake vehicles that hold them up (a bus
//! at a stop, a slower truck) when a parallel lane has room.
//!
//! The lane network is rebuilt from the road network whenever roads change
//! and is never serialised; cars are.

use crate::buildings::BuildingId;
use crate::citizens::CitizenId;
use crate::market::Resource;
use crate::roads::{project_on_segment, NodeId, RoadNetwork, SegmentId, LANE_WIDTH, SHOULDER};
use glam::Vec2;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap, VecDeque};

new_key_type! {
    pub struct CarId;
}

pub type LaneId = u32;

pub const CAR_LENGTH: f32 = 4.5;
pub const BUS_LENGTH: f32 = 12.0;
const ROAD_SPEED: f32 = 13.0;
/// Highway carriageways (one-way, three lanes or more).
pub const HIGHWAY_SPEED: f32 = 25.0;
const CONNECTOR_SPEED: f32 = 6.0;
const IDM_ACCEL: f32 = 1.4;
const IDM_BRAKE: f32 = 2.0;
const IDM_MIN_GAP: f32 = 2.0;
const IDM_HEADWAY: f32 = 1.2;
const GREEN_SECONDS: f32 = 14.0;
const CLEARANCE_SECONDS: f32 = 3.0;
const LOOKAHEAD: f32 = 90.0;
/// Lane arrow bits.
pub const LEFT: u8 = 1;
pub const STRAIGHT: u8 = 2;
pub const RIGHT: u8 = 4;
/// A protected left arrow at the start of a green: its length, and how long
/// it runs at least before it ends for want of left turners.
const ARROW_SECONDS: f32 = 7.0;
const ARROW_MIN: f32 = 3.0;
/// A left turner that has yielded to oncoming traffic this long at the head
/// of its lane gets an arrow next time; a queue of left turners in a lane
/// of their own gets one once its head has yielded `LEFT_YIELDING`.
const LEFT_STARVED: f32 = 20.0;
const LEFT_YIELDING: f32 = 6.0;
/// How often (traffic seconds) default lane arrows are revised from the
/// turning counts.
const ADAPT_SECONDS: f32 = 600.0;
/// Passing a waiting left turner on the right: only below this speed, on
/// the shoulder, with a car this wide, stopping this far short of the lane
/// end so the crossing stays clear.
const PASS_MAX_SPEED: f32 = 8.0;
const CAR_WIDTH: f32 = 1.8;
const PASS_LATERAL_SPEED: f32 = 1.4;
const CROSSING_SETBACK: f32 = 3.0;
/// Left turn waits remembered for the report.
const LEFT_WAIT_WINDOW: usize = 500;
/// Shortest green before an actuated signal may cut to a waiting approach.
const MIN_GREEN: f32 = 5.0;
/// A car this close to the stop line (or in the junction) asks for a green.
const DEMAND_REACH: f32 = 40.0;
/// Routing penalty (metres) for a lane change and per vehicle on a lane,
/// so routes spread over parallel lanes and around queues.
const LANE_CHANGE_COST: f32 = 8.0;
const QUEUE_COST: f32 = 6.0;
/// Hardest braking (m/s per s) a lane change may ask of the changer or of its
/// new follower; a car stopped at the end of its lane accepts a little more.
const LC_SAFE_BRAKE: f32 = 3.0;
const LC_FORCED_BRAKE: f32 = 4.5;
/// Acceleration gain (m/s per s) that makes an overtake worth it.
const OVERTAKE_GAIN: f32 = 0.6;
/// Seconds after a lane change before the next discretionary one.
const LC_COOLDOWN: f32 = 4.0;
/// Sideways speed (m/s) of the drift into the new lane.
const LC_LATERAL_SPEED: f32 = 0.9;
/// Seconds a car waits on a full exit (a gridlock ring, a jammed street)
/// before it looks for another way; a lane change it cannot make (often two
/// cars that each want the other's full lane) is given up sooner.
const REROUTE_AFTER: f32 = 40.0;
const REROUTE_LANE_CHANGE_AFTER: f32 = 10.0;
/// Seconds a car waits at the kerb before the next driver along lets it
/// in, if that driver is slow enough to stop comfortably.
const COURTESY_AFTER: f32 = 15.0;
/// Trucks cruise a little below the limit, which is what gets them overtaken.
const TRUCK_SPEED_FACTOR: f32 = 0.85;
/// Trips older than this (traffic seconds) end where the car stands. A bus
/// is never ended this way (its riders would be teleported with it): it is
/// re-planned from where it stands instead.
/// [services] Emergency vehicles drive this much over the limit; cars this
/// far ahead of one on its lane pull this far to the right.
const SIREN_SPEED_FACTOR: f32 = 1.25;
const SIREN_CLEAR: f32 = 40.0;
const SIREN_PULL_ASIDE: f32 = 1.6;
const TRIP_TIMEOUT: f32 = 1800.0;

#[derive(Clone, Debug)]
pub enum LaneKind {
    Road { segment: SegmentId, forward: bool, index: u8 },
    Connector { node: NodeId, approach: usize },
}

#[derive(Clone, Debug)]
pub struct Lane {
    pub points: Vec<Vec2>,
    cum: Vec<f32>,
    pub length: f32,
    pub next: Vec<LaneId>,
    pub kind: LaneKind,
    /// Cars on this lane ordered by increasing position.
    pub cars: Vec<CarId>,
    pub green: bool,
    pub speed_limit: f32,
    pub end_node: NodeId,
    /// Parallel lanes of the same segment and direction, if any.
    pub left: Option<LaneId>,
    pub right: Option<LaneId>,
    /// Lanes that lead into this one.
    pub prev: Vec<LaneId>,
    /// For connectors: -1 a left turn, 1 a right turn, 0 straight on.
    pub turn: i8,
    /// For road lanes ending at a junction: the movements allowed from it
    /// (`LEFT | STRAIGHT | RIGHT`); 0 elsewhere.
    pub arrows: u8,
    /// For connectors: the arrows of the lane feeding it allow this turn.
    /// Routes only use permitted connectors; a car already routed keeps going.
    pub permitted: bool,
    /// Index into `LaneNetwork::approaches` for road lanes ending at a junction.
    pub approach: u32,
}

impl Lane {
    fn new(points: Vec<Vec2>, kind: LaneKind, end_node: NodeId, speed_limit: f32) -> Self {
        let mut cum = vec![0.0];
        for w in points.windows(2) {
            cum.push(cum.last().unwrap() + w[0].distance(w[1]));
        }
        let length = *cum.last().unwrap();
        Self { points, cum, length, next: Vec::new(), kind, cars: Vec::new(), green: true, speed_limit, end_node, left: None, right: None, prev: Vec::new(), turn: 0, arrows: 0, permitted: true, approach: u32::MAX }
    }

    pub fn is_connector(&self) -> bool {
        matches!(self.kind, LaneKind::Connector { .. })
    }

    /// True when `other` is the lane beside this one (a lane change).
    pub fn is_beside(&self, other: LaneId) -> bool {
        self.left == Some(other) || self.right == Some(other)
    }

    /// Position and unit direction at distance `s` along the lane.
    pub fn point_at(&self, s: f32) -> (Vec2, Vec2) {
        if self.points.len() < 2 {
            return (self.points.first().copied().unwrap_or(Vec2::ZERO), Vec2::X);
        }
        let s = s.clamp(0.0, self.length);
        let mut i = 0;
        while i + 2 < self.points.len() && self.cum[i + 1] < s {
            i += 1;
        }
        let seg_len = (self.cum[i + 1] - self.cum[i]).max(1e-4);
        let t = ((s - self.cum[i]) / seg_len).clamp(0.0, 1.0);
        let dir = (self.points[i + 1] - self.points[i]).normalize_or_zero();
        (self.points[i].lerp(self.points[i + 1], t), if dir == Vec2::ZERO { Vec2::X } else { dir })
    }

    /// Closest point on the lane to `p`: `(distance along, distance from)`.
    /// Points of the lane from `s0` to `s1`, the ends interpolated.
    pub fn points_between(&self, s0: f32, s1: f32) -> Vec<Vec2> {
        let (s0, s1) = (s0.clamp(0.0, self.length), s1.clamp(0.0, self.length));
        let mut out = vec![self.point_at(s0).0];
        for (i, p) in self.points.iter().enumerate() {
            if i < self.cum.len() && self.cum[i] > s0 && self.cum[i] < s1 {
                out.push(*p);
            }
        }
        out.push(self.point_at(s1).0);
        out
    }

    pub fn project(&self, p: Vec2) -> (f32, f32) {
        let mut best = (0.0, f32::MAX);
        for (i, w) in self.points.windows(2).enumerate() {
            let (t, q) = project_on_segment(p, w[0], w[1]);
            let d = p.distance(q);
            if d < best.1 {
                best = (self.cum[i] + t * (self.cum[i + 1] - self.cum[i]), d);
            }
        }
        best
    }

    pub fn start(&self) -> Vec2 {
        self.points[0]
    }

    pub fn end(&self) -> Vec2 {
        *self.points.last().unwrap()
    }

    fn start_dir(&self) -> Vec2 {
        (self.points[1] - self.points[0]).normalize_or_zero()
    }

    fn end_dir(&self) -> Vec2 {
        let n = self.points.len();
        (self.points[n - 1] - self.points[n - 2]).normalize_or_zero()
    }
}

#[derive(Clone, Debug)]
pub struct Intersection {
    pub node: NodeId,
    pub radius: f32,
    /// Streets meeting here, exits included.
    pub approaches: usize,
    /// Signal phases, each the approaches (with incoming lanes) that share
    /// a green: opposite approaches go together, left turns yielding to
    /// oncoming traffic. A one-way exit never gets a phase of its own in
    /// which nothing can move.
    pub phases: Vec<Vec<usize>>,
    /// Index into `phases` of the group that has the green.
    pub phase: usize,
    /// The approach straight across, per approach, if it feeds the junction.
    pub opposite: Vec<Option<usize>>,
    /// Seconds of protected left arrow at the start of the current green
    /// (0 for none): only left turns go, oncoming traffic has red.
    pub arrow: f32,
    /// Left turns made here and the time their drivers stood waiting.
    pub left_turns: u32,
    pub left_wait_sum: f32,
    pub left_wait_max: f32,
    pub timer: f32,
    pub connectors: Vec<(LaneId, usize)>,
    /// Road lanes ending here, with their approach index.
    pub incoming: Vec<(LaneId, usize)>,
    pub signalized: bool,
}

/// The lanes of one street arriving at a junction, with their arrows.
#[derive(Clone, Debug)]
pub struct Approach {
    pub node: NodeId,
    pub segment: SegmentId,
    /// Leftmost first.
    pub lanes: Vec<LaneId>,
    /// Movements this junction offers from here.
    pub exits: u8,
    pub arrows: Vec<u8>,
    /// Set by the player (`Traffic::lane_use`): not revised from demand.
    pub fixed: bool,
    /// Recent left, straight and right turns, decaying.
    pub counts: [f32; 3],
}

#[derive(Clone, Debug, Default)]
pub struct LaneNetwork {
    pub lanes: Vec<Lane>,
    pub intersections: Vec<Intersection>,
    pub segment_lanes: HashMap<SegmentId, Vec<LaneId>>,
    pub node_radius: Vec<f32>,
    /// Index into `intersections` per node, `usize::MAX` for none.
    pub node_intersection: Vec<usize>,
    pub approaches: Vec<Approach>,
    pub version: u64,
    /// Bumped whenever lane arrows change (for drawing them).
    pub arrows_version: u64,
}

/// Arrow bit for a connector's turn.
pub fn turn_bit(turn: i8) -> u8 {
    match turn {
        t if t < 0 => LEFT,
        0 => STRAIGHT,
        _ => RIGHT,
    }
}

/// "L", "S+R", "L+S+R" ...
pub fn arrows_label(a: u8) -> String {
    let parts: Vec<&str> = [(LEFT, "L"), (STRAIGHT, "S"), (RIGHT, "R")].iter().filter(|(b, _)| a & b != 0).map(|(_, n)| *n).collect();
    if parts.is_empty() { "-".into() } else { parts.join("+") }
}

/// Parse "L", "S+R", "LS", "left+straight" ... into arrow bits.
pub fn parse_arrows(text: &str) -> Option<u8> {
    let mut a = 0;
    for part in text.split(['+', ',', ' ', '|']).filter(|p| !p.is_empty()) {
        let p = part.to_ascii_lowercase();
        match p.as_str() {
            "l" | "left" => a |= LEFT,
            "s" | "straight" | "t" | "through" => a |= STRAIGHT,
            "r" | "right" => a |= RIGHT,
            _ if p.chars().all(|c| "lsr".contains(c)) => {
                for c in p.chars() {
                    a |= match c { 'l' => LEFT, 's' => STRAIGHT, _ => RIGHT };
                }
            }
            _ => return None,
        }
    }
    (a != 0).then_some(a)
}

/// Default arrows for an approach of `n` lanes offering `exits`, given the
/// share of recent traffic turning left (None before any is counted) and
/// the arrows it has now (for hysteresis).
pub fn default_arrows(n: usize, exits: u8, left_share: Option<f32>, now: Option<&[u8]>) -> Vec<u8> {
    if n <= 1 || exits.count_ones() <= 1 {
        return vec![exits; n];
    }
    let shared_now = now.is_some_and(|a| a.len() == 2 && a[0] & STRAIGHT != 0);
    let dual_now = now.is_some_and(|a| a.len() >= 3 && a[1] & LEFT != 0);
    // Before anything is counted: the plain default (a left-only lane on a
    // two-lane approach, one left lane on wider ones).
    let share = left_share.unwrap_or(0.35);
    let quiet_left = if shared_now { share < 0.4 } else { share < 0.3 };
    let busy_left = if dual_now { share > 0.3 } else { share > 0.4 };
    let mut a: Vec<u8> = match n {
        2 if quiet_left => vec![LEFT | STRAIGHT, STRAIGHT | RIGHT],
        2 => vec![LEFT, STRAIGHT | RIGHT],
        3 if busy_left => vec![LEFT, LEFT | STRAIGHT, STRAIGHT | RIGHT],
        _ => {
            let mut v = vec![STRAIGHT; n];
            v[0] = LEFT;
            if busy_left && n >= 4 {
                v[1] = LEFT;
            }
            v[n - 1] = STRAIGHT | RIGHT;
            v
        }
    };
    // Fit to the exits there are: a lane left with nothing takes the side
    // it is on; every exit is reachable from some lane.
    for (i, x) in a.iter_mut().enumerate() {
        *x &= exits;
        if *x == 0 {
            let side = if exits & STRAIGHT != 0 { STRAIGHT } else if i < n / 2 { LEFT } else { RIGHT };
            *x = if exits & side != 0 { side } else { exits };
        }
    }
    if exits & LEFT != 0 && a.iter().all(|x| x & LEFT == 0) {
        a[0] |= LEFT;
    }
    if exits & RIGHT != 0 && a.iter().all(|x| x & RIGHT == 0) {
        a[n - 1] |= RIGHT;
    }
    if exits & STRAIGHT != 0 && a.iter().all(|x| x & STRAIGHT == 0) {
        a[n - 1] |= STRAIGHT;
    }
    a
}

/// Freight carried by a truck between two businesses.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Cargo {
    /// The seller; a null key for goods imported from out of town.
    pub from: BuildingId,
    pub to: BuildingId,
    pub resource: Resource,
    pub amount: f32,
    /// Money the buyer owes on delivery.
    pub price: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Car {
    /// Driver for private cars; a null key for trucks.
    pub citizen: CitizenId,
    pub cargo: Option<Cargo>,
    pub route: Vec<LaneId>,
    pub idx: usize,
    pub s: f32,
    pub s_end: f32,
    pub v: f32,
    pub pos: Vec2,
    pub heading: f32,
    pub prev_pos: Vec2,
    pub prev_heading: f32,
    pub color: u8,
    /// Seconds spent on the trip; stalled trips time out.
    #[serde(default)]
    pub age: f32,
    /// A bus on a line.
    #[serde(default)]
    pub bus: Option<Bus>,
    /// Seconds left standing still (a bus at a stop).
    #[serde(default)]
    pub hold: f32,
    /// Sideways offset (metres, + to the right) from the lane centre while
    /// drifting into a new lane; decays to zero.
    #[serde(default)]
    pub lateral: f32,
    /// Seconds the car has stood still without a reason of its own (a bus
    /// dwelling at a stop is not counted).
    #[serde(default)]
    pub stalled: f32,
    /// What held the car back on the last physics step.
    #[serde(default)]
    pub wait: Wait,
    /// Seconds before the car may change lanes by choice or replan again.
    #[serde(default)]
    pub cooldown: f32,
    /// Waiting at the kerb for a gap to pull out into its first lane; not
    /// yet in the lane's list, so nobody queues behind it.
    #[serde(default)]
    pub entering: bool,
    /// Squeezing past a left turner waiting at the head of a one-lane
    /// approach, on the shoulder to its right.
    #[serde(default)]
    pub passing: bool,
    /// Seconds stood waiting before the left turn ahead (reset once made).
    #[serde(default)]
    pub turn_wait: f32,
    /// Of those, seconds at the stop line yielding to oncoming traffic.
    #[serde(default)]
    pub yielded: f32,
    /// [services] An emergency vehicle with lights and sirens: it runs red
    /// lights and passes queues, and the cars ahead of it pull aside.
    #[serde(default)]
    pub siren: bool,
    /// What it looks like (body, class, wear, livery); drawing only.
    #[serde(default)]
    pub model: crate::vehicle::VehicleModel,
}

/// Why a car is not driving freely.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Wait {
    #[default]
    Free,
    /// Following (or queued behind) another vehicle.
    Leader,
    /// At a red light.
    Signal,
    /// Yielding at an unsignalled junction.
    Yield,
    /// The lane beyond the junction is full.
    ExitFull,
    /// At the end of its lane, waiting for a gap to change lanes.
    LaneChange,
    /// A bus standing at a stop.
    Dwell,
    /// At the kerb, waiting for a gap to pull out.
    Entering,
    /// Pulling up at the destination.
    Arriving,
}

impl Car {
    pub fn length(&self) -> f32 {
        if self.bus.is_some() { BUS_LENGTH } else { CAR_LENGTH }
    }

    /// Desired speed on a lane with this limit.
    fn desired_speed(&self, limit: f32) -> f32 {
        if self.siren {
            return limit * SIREN_SPEED_FACTOR;
        }
        if self.cargo.is_some() { limit * TRUCK_SPEED_FACTOR } else { limit }
    }

    /// The route asks for a change into the lane beside this one next.
    fn change_pending(&self, net: &LaneNetwork) -> Option<LaneId> {
        let next = *self.route.get(self.idx + 1)?;
        net.lanes[self.route[self.idx] as usize].is_beside(next).then_some(next)
    }
}

/// A ring of full lanes whose leading vehicles each wait for the next lane
/// in the ring: nobody can move until somebody takes another way.
#[derive(Clone, Debug)]
pub struct Gridlock {
    pub lanes: Vec<LaneId>,
    /// The junctions the ring passes through (where its heads stand).
    pub junctions: Vec<NodeId>,
    pub points: Vec<Vec2>,
    pub vehicles: usize,
}

/// What a bus is doing on its line.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Bus {
    pub route: crate::transit::RouteId,
    /// Stop the bus is driving to; `usize::MAX` when heading for the depot.
    pub next_stop: usize,
    pub passengers: Vec<CitizenId>,
    /// Finish the loop at the next stop and go home.
    pub to_depot: bool,
    pub depot: crate::buildings::BuildingId,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Traffic {
    #[serde(skip)]
    pub net: LaneNetwork,
    pub cars: SlotMap<CarId, Car>,
    /// Lane arrows set by the player, per approach; reapplied whenever the
    /// lane network is rebuilt (a rule whose lane count no longer matches is
    /// ignored).
    #[serde(default)]
    pub lane_use: Vec<LaneUseRule>,
    /// The most recent left turns' waits, seconds.
    #[serde(skip)]
    pub left_waits: VecDeque<f32>,
    #[serde(skip)]
    adapt_clock: f32,
}

/// Arrows for the lanes (leftmost first) of `segment` arriving at `node`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LaneUseRule {
    pub segment: SegmentId,
    pub node: NodeId,
    pub arrows: Vec<u8>,
}

/// What a signal knows about one approach of its junction.
#[derive(Clone, Debug, Default)]
pub struct SignalDemand {
    /// Someone at (or near) the stop line.
    pub any: Vec<bool>,
    /// Left turners near the stop line.
    pub left_queue: Vec<u8>,
    /// A left turner at the head of its lane has waited `LEFT_STARVED`.
    pub left_starved: Vec<bool>,
    /// The approach has a left-only lane.
    pub dedicated: Vec<bool>,
    /// The left turner at the head has been yielding to oncoming traffic.
    pub left_yielding: Vec<bool>,
}

fn quad_bezier(p0: Vec2, c: Vec2, p1: Vec2, samples: usize) -> Vec<Vec2> {
    (0..=samples)
        .map(|i| {
            let t = i as f32 / samples as f32;
            let u = 1.0 - t;
            p0 * (u * u) + c * (2.0 * u * t) + p1 * (t * t)
        })
        .collect()
}

/// Curve joining the end of one lane to the start of another inside an
/// intersection. Uses the intersection of the two lane directions as the
/// bezier control point when it is sensible, so through-connectors stay
/// straight and turns bend.
fn connector_curve(p_in: Vec2, d_in: Vec2, p_out: Vec2, d_out: Vec2, radius: f32) -> Vec<Vec2> {
    if p_in.distance(p_out) < 0.5 {
        return vec![p_in, p_out];
    }
    let denom = d_in.perp_dot(d_out);
    let mut control = (p_in + p_out) * 0.5;
    if denom.abs() > 1e-3 {
        let qp = p_out - p_in;
        let t = qp.perp_dot(d_out) / denom;
        let u = qp.perp_dot(d_in) / denom;
        // Keep the control point inside the junction so turns never swing
        // out over the kerb, even at shallow angles.
        let limit = radius * 1.3 + 1.0;
        if t > 0.0 && u > 0.0 {
            let t = t.min(limit);
            control = p_in + d_in * t;
        }
    }
    let straight = d_in.dot(d_out) > 0.98;
    quad_bezier(p_in, control, p_out, if straight { 1 } else { 6 })
}

impl LaneNetwork {
    pub fn rebuild(&mut self, roads: &RoadNetwork) {
        self.lanes.clear();
        self.intersections.clear();
        self.segment_lanes.clear();
        self.node_intersection = vec![usize::MAX; roads.nodes.len()];
        self.approaches.clear();
        self.version += 1;
        self.arrows_version += 1;

        // Intersection radius per node: enough to fit the widest road.
        self.node_radius = roads
            .nodes
            .iter()
            .map(|n| {
                let alive: Vec<_> = n.segments.iter().filter(|&&s| roads.segment(s).alive && roads.segment(s).is_street()).collect();
                let w = alive.iter().map(|&&s| roads.segment(s).half_width()).fold(0.0, f32::max);
                match alive.len() {
                    0 => 0.0,
                    1 => w * 0.8,
                    2 => {
                        // A bend: the sharper it is, the more room the turn needs.
                        let (sa, sb) = (*alive[0], *alive[1]);
                        let (a0, a1) = roads.segment_points(sa);
                        let (b0, b1) = roads.segment_points(sb);
                        let here = n.pos;
                        let da = (if a0.distance(here) < a1.distance(here) { a1 - a0 } else { a0 - a1 }).normalize_or_zero();
                        let db = (if b0.distance(here) < b1.distance(here) { b1 - b0 } else { b0 - b1 }).normalize_or_zero();
                        // da and db both point away from the node; straight through means da . db = -1.
                        let bend = (1.0 + da.dot(db)).clamp(0.0, 2.0) * 0.5; // 0 straight, 1 hairpin
                        (0.6 + (w + 1.0) * bend).min(w + 1.0)
                    }
                    _ => w + 1.0,
                }
            })
            .collect();

        for (sid, seg) in roads.alive_segments() {
            if !seg.is_street() {
                continue;
            }
            let (pa, pb) = roads.segment_points(sid);
            let len = pa.distance(pb);
            if len < 0.5 {
                continue;
            }
            let dir = (pb - pa) / len;
            let mut ra = self.node_radius[seg.a.0 as usize];
            let mut rb = self.node_radius[seg.b.0 as usize];
            if len - ra - rb < 2.0 {
                let k = ((len - 2.0) / (ra + rb).max(1e-3)).clamp(0.0, 1.0);
                ra *= k;
                rb *= k;
            }
            let p0 = pa + dir * ra;
            let p1 = pb - dir * rb;
            // Right-hand traffic. The sim plane maps to world XZ with y up, so the
            // right of a car heading `dir` is (-dir.y, dir.x). The whole lane set is
            // centred on the segment line: backward lanes on the left half, forward
            // lanes on the right half, boundary k at (k - total/2) lane widths.
            let right = Vec2::new(-dir.y, dir.x);
            let total = seg.lanes() as f32;
            let left_edge = -total * 0.5 * LANE_WIDTH;
            let mut ids = Vec::new();
            let first = self.lanes.len() as LaneId;
            let speed = if seg.is_highway() { HIGHWAY_SPEED } else { ROAD_SPEED };
            for i in 0..seg.lanes_fwd {
                let off = right * (left_edge + (seg.lanes_bwd as f32 + i as f32 + 0.5) * LANE_WIDTH);
                let id = self.lanes.len() as LaneId;
                self.lanes.push(Lane::new(vec![p0 + off, p1 + off], LaneKind::Road { segment: sid, forward: true, index: i }, seg.b, speed));
                ids.push(id);
            }
            for j in 0..seg.lanes_bwd {
                let off = right * (left_edge + (seg.lanes_bwd as f32 - j as f32 - 0.5) * LANE_WIDTH);
                let id = self.lanes.len() as LaneId;
                self.lanes.push(Lane::new(vec![p1 + off, p0 + off], LaneKind::Road { segment: sid, forward: false, index: j }, seg.a, speed));
                ids.push(id);
            }
            // Index 0 is the leftmost lane in both directions.
            let (nf, nb) = (seg.lanes_fwd as LaneId, seg.lanes_bwd as LaneId);
            for (start, n) in [(first, nf), (first + nf, nb)] {
                for k in 0..n {
                    let lane = &mut self.lanes[(start + k) as usize];
                    lane.left = (k > 0).then(|| start + k - 1);
                    lane.right = (k + 1 < n).then(|| start + k + 1);
                }
            }
            self.segment_lanes.insert(sid, ids);
        }

        let road_lane_count = self.lanes.len();
        for (ni, node) in roads.nodes.iter().enumerate() {
            let nid = NodeId(ni as u32);
            let approaches: Vec<SegmentId> = node.segments.iter().copied().filter(|&s| roads.segment(s).alive && roads.segment(s).is_street()).collect();
            if approaches.is_empty() {
                continue;
            }
            let mut incoming: Vec<(LaneId, SegmentId)> = Vec::new();
            let mut outgoing: Vec<(LaneId, SegmentId)> = Vec::new();
            for &sid in &approaches {
                let seg = roads.segment(sid);
                for &lid in &self.segment_lanes[&sid] {
                    if let LaneKind::Road { forward, .. } = self.lanes[lid as usize].kind {
                        let ends_here = if forward { seg.b == nid } else { seg.a == nid };
                        if ends_here {
                            incoming.push((lid, sid));
                        } else {
                            outgoing.push((lid, sid));
                        }
                    }
                }
            }
            let radius = self.node_radius[ni];
            let mut connectors = Vec::new();
            let lane_index = |l: &Lane| match l.kind {
                LaneKind::Road { index, .. } => index as usize,
                _ => 0,
            };
            let width = |set: &[(LaneId, SegmentId)], seg: SegmentId| set.iter().filter(|&&(_, s)| s == seg).count();
            for &(inc, inc_seg) in &incoming {
                let approach = approaches.iter().position(|&s| s == inc_seg).unwrap_or(0);
                let (i, n) = (lane_index(&self.lanes[inc as usize]), width(&incoming, inc_seg));
                let d_in = self.lanes[inc as usize].end_dir();
                // Exits from this approach, leftmost first, with their turn.
                let mut exits: Vec<(SegmentId, f32)> = Vec::new();
                for &(out, out_seg) in &outgoing {
                    if (out_seg == inc_seg && approaches.len() > 1) || exits.iter().any(|&(s, _)| s == out_seg) {
                        continue; // U-turns only at dead ends
                    }
                    let d_out = self.lanes[out as usize].start_dir();
                    exits.push((out_seg, d_in.perp_dot(d_out).atan2(d_in.dot(d_out))));
                }
                exits.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
                let turn = |angle: f32| if approaches.len() <= 2 || angle.abs() < 0.5 { 0 } else if angle < 0.0 { -1 } else { 1 };
                // A connector for every movement into one lane of each exit:
                // a left turn keeps its place counted from the left, a right
                // turn from the right, straight on fans n lanes over m. The
                // lane arrows then permit some of them.
                for &(out, out_seg) in &outgoing {
                    let Some(&(_, angle)) = exits.iter().find(|&&(s, _)| s == out_seg) else { continue };
                    let (j, m) = (lane_index(&self.lanes[out as usize]), width(&outgoing, out_seg));
                    let ok = match turn(angle) {
                        -1 => j == i.min(m - 1),
                        1 => j == m - 1 - (n - 1 - i).min(m - 1),
                        _ => {
                            let lo = i * m / n.max(1);
                            let hi = ((i + 1) * m).div_ceil(n.max(1)).saturating_sub(1).max(lo);
                            (lo..=hi).contains(&j)
                        }
                    };
                    if ok {
                        let id = self.push_connector(inc, out, nid, approach, radius, approaches.len());
                        self.lanes[id as usize].turn = turn(angle);
                        connectors.push((id, approach));
                    }
                }
            }
            // Lane arrows for every street arriving at a real junction (not
            // highway ramps: merges and diverges keep every lane open).
            if approaches.len() >= 3 && !approaches.iter().any(|&s| roads.segment(s).is_highway()) {
                for &sid in &approaches {
                    let mut lanes: Vec<LaneId> = incoming.iter().filter(|&&(_, s)| s == sid).map(|&(l, _)| l).collect();
                    if lanes.is_empty() {
                        continue;
                    }
                    lanes.sort_by_key(|&l| lane_index(&self.lanes[l as usize]));
                    let exits = lanes.iter().flat_map(|&l| self.lanes[l as usize].next.iter()).fold(0u8, |acc, &c| acc | turn_bit(self.lanes[c as usize].turn));
                    let arrows = default_arrows(lanes.len(), exits, None, None);
                    let idx = self.approaches.len() as u32;
                    for &l in &lanes {
                        self.lanes[l as usize].approach = idx;
                    }
                    self.approaches.push(Approach { node: nid, segment: sid, lanes, exits, arrows: Vec::new(), fixed: false, counts: [0.0; 3] });
                    self.set_arrows(idx as usize, arrows);
                }
            }
            let two_way = approaches.iter().filter(|&&s| roads.segment(s).lanes_fwd > 0 && roads.segment(s).lanes_bwd > 0).count();
            let feeding: Vec<usize> = (0..approaches.len()).filter(|&ap| incoming.iter().any(|&(_, s)| s == approaches[ap])).collect();
            // Direction of each approach away from the junction.
            let here = node.pos;
            let away: Vec<Vec2> = approaches
                .iter()
                .map(|&sid| {
                    let (a, b) = roads.segment_points(sid);
                    (if a.distance(here) < b.distance(here) { b - a } else { a - b }).normalize_or_zero()
                })
                .collect();
            let mut opposite: Vec<Option<usize>> = vec![None; approaches.len()];
            for &a in &feeding {
                let best = feeding.iter().copied().filter(|&b| b != a).min_by(|&x, &y| away[a].dot(away[x]).total_cmp(&away[a].dot(away[y])));
                if let Some(b) = best.filter(|&b| away[a].dot(away[b]) < -0.85) {
                    opposite[a] = Some(b);
                }
            }
            for a in 0..approaches.len() {
                if opposite[a].is_some_and(|b| opposite[b] != Some(a)) {
                    opposite[a] = None;
                }
            }
            // Mutual opposites share a phase; the rest go alone.
            let mut phases: Vec<Vec<usize>> = Vec::new();
            for &a in &feeding {
                if phases.iter().any(|g| g.contains(&a)) {
                    continue;
                }
                match opposite[a] {
                    Some(b) => phases.push(vec![a, b]),
                    None => phases.push(vec![a]),
                }
            }
            // One incoming approach (or one opposed pair) has nobody to yield to.
            let signalized = ((approaches.len() >= 3 && two_way >= 3) || approaches.len() >= 4) && phases.len() >= 2;
            self.node_intersection.resize(roads.nodes.len().max(self.node_intersection.len()), usize::MAX);
            self.node_intersection[ni] = self.intersections.len();
            let incoming = incoming.iter().map(|&(l, s)| (l, approaches.iter().position(|&a| a == s).unwrap_or(0))).collect();
            self.intersections.push(Intersection { node: nid, radius, approaches: approaches.len(), phases, phase: 0, opposite, arrow: 0.0, left_turns: 0, left_wait_sum: 0.0, left_wait_max: 0.0, timer: 0.0, connectors, incoming, signalized });
        }
        let _ = road_lane_count;
        for id in 0..self.lanes.len() {
            for k in 0..self.lanes[id].next.len() {
                let next = self.lanes[id].next[k] as usize;
                self.lanes[next].prev.push(id as LaneId);
            }
        }
        self.apply_signals();
    }

    fn push_connector(&mut self, inc: LaneId, out: LaneId, nid: NodeId, approach: usize, radius: f32, approaches: usize) -> LaneId {
        let (p_in, d_in) = (self.lanes[inc as usize].end(), self.lanes[inc as usize].end_dir());
        let (p_out, d_out) = (self.lanes[out as usize].start(), self.lanes[out as usize].start_dir());
        let pts = connector_curve(p_in, d_in, p_out, d_out, radius);
        let id = self.lanes.len() as LaneId;
        let through = self.lanes[inc as usize].speed_limit.min(self.lanes[out as usize].speed_limit);
        // Along a highway (past a ramp too) traffic keeps its speed.
        let speed = if through > ROAD_SPEED && d_in.dot(d_out) > 0.9 {
            through
        } else if approaches <= 2 && d_in.dot(d_out) > 0.7 {
            ROAD_SPEED
        } else {
            CONNECTOR_SPEED
        };
        self.lanes.push(Lane::new(pts, LaneKind::Connector { node: nid, approach }, nid, speed));
        self.lanes[id as usize].next.push(out);
        self.lanes[inc as usize].next.push(id);
        id
    }

    /// Give an approach new lane arrows: its lanes' `arrows` and which of
    /// their connectors are permitted.
    pub fn set_arrows(&mut self, approach: usize, arrows: Vec<u8>) {
        let Some(ap) = self.approaches.get_mut(approach) else { return };
        if ap.arrows == arrows {
            return;
        }
        ap.arrows = arrows.clone();
        let lanes = ap.lanes.clone();
        for (k, &l) in lanes.iter().enumerate() {
            let a = arrows.get(k).copied().unwrap_or(0);
            self.lanes[l as usize].arrows = a;
            for c in self.lanes[l as usize].next.clone() {
                let bit = turn_bit(self.lanes[c as usize].turn);
                self.lanes[c as usize].permitted = a & bit != 0;
            }
        }
        self.arrows_version += 1;
    }

    fn apply_signals(&mut self) {
        for inter in &self.intersections {
            let group = inter.phases.get(inter.phase);
            for &(lane, approach) in &inter.connectors {
                let mine = group.is_some_and(|g| g.contains(&approach));
                let open = if inter.timer < inter.arrow {
                    // Protected arrow: left turns only.
                    self.lanes[lane as usize].turn < 0
                } else {
                    inter.timer < inter.arrow + GREEN_SECONDS
                };
                self.lanes[lane as usize].green = !inter.signalized || (mine && open);
            }
        }
    }

    /// Fixed-cycle signal step (no knowledge of traffic).
    #[cfg(test)]
    fn update_signals(&mut self, dt: f32) {
        self.step_signals(dt, None);
    }

    /// Advance the signals. With `demand` (per intersection, per approach:
    /// is anyone waiting at the stop line?) they are actuated: a green with
    /// nobody left to serve is cut short after `MIN_GREEN` when another
    /// approach is waiting, a green is held while only its own approach
    /// wants it, and phases nobody waits for are skipped. Without demand
    /// anywhere the plain fixed cycle runs.
    fn step_signals(&mut self, dt: f32, demand: Option<&[SignalDemand]>) {
        for (k, inter) in self.intersections.iter_mut().enumerate() {
            if !inter.signalized || inter.phases.is_empty() {
                continue;
            }
            inter.timer += dt;
            let d = demand.and_then(|d| d.get(k)).filter(|d| d.any.len() >= inter.approaches);
            if let Some(d) = d {
                let wants = |g: &Vec<usize>| g.iter().any(|&ap| d.any[ap]);
                let mine = wants(&inter.phases[inter.phase]);
                let others = inter.phases.iter().enumerate().any(|(k, g)| k != inter.phase && wants(g));
                // The arrow ends early once nobody is left to turn.
                if inter.timer < inter.arrow && inter.timer >= ARROW_MIN && inter.phases[inter.phase].iter().all(|&ap| d.left_queue[ap] == 0) {
                    inter.arrow = inter.timer;
                }
                let green_end = inter.arrow + GREEN_SECONDS;
                // A left turner starving in an endless oncoming stream: end
                // this green so the next one can open with an arrow.
                let starving = inter.phases[inter.phase].iter().any(|&ap| d.left_starved[ap]);
                if inter.timer >= inter.arrow && inter.timer < green_end {
                    if inter.timer >= inter.arrow + MIN_GREEN && ((!mine && others) || starving) {
                        inter.timer = green_end;
                    } else if mine && !others && inter.timer > green_end - 1.0 {
                        inter.timer = green_end - 1.0;
                    }
                }
            }
            if inter.timer >= inter.arrow + GREEN_SECONDS + CLEARANCE_SECONDS {
                inter.timer = 0.0;
                let n = inter.phases.len();
                let mut next = (inter.phase + 1) % n;
                if let Some(d) = d {
                    if let Some(step) = (1..=n).find(|&st| inter.phases[(inter.phase + st) % n].iter().any(|&ap| d.any[ap])) {
                        next = (inter.phase + step) % n;
                    }
                }
                inter.phase = next;
                // A protected left where opposed lefts queue in a lane of
                // their own, or a left turner has waited too long.
                let g = &inter.phases[next];
                inter.arrow = match d {
                    Some(d) if g.len() > 1 && g.iter().any(|&ap| (d.dedicated[ap] && d.left_queue[ap] >= 3 && d.left_yielding[ap]) || d.left_starved[ap]) => ARROW_SECONDS,
                    _ => 0.0,
                };
            }
        }
        self.apply_signals();
    }

    /// Closest lane of a segment to a point: `(lane, s, distance)`.
    pub fn nearest_lane_on(&self, segment: SegmentId, p: Vec2) -> Option<(LaneId, f32, f32)> {
        let mut best: Option<(LaneId, f32, f32)> = None;
        for &lid in self.segment_lanes.get(&segment)? {
            let (s, d) = self.lanes[lid as usize].project(p);
            if best.map_or(true, |b| d < b.2) {
                best = Some((lid, s, d));
            }
        }
        best
    }

    /// The polyline a route drives, for drawing.
    pub fn polyline(&self, lanes: &[LaneId], s0: f32, s1: f32) -> Vec<Vec2> {
        let mut out: Vec<Vec2> = Vec::new();
        // Where the drive enters each lane: a lane change is drawn a few
        // metres after the car joined the lane it leaves.
        let mut a = s0;
        for (k, &l) in lanes.iter().enumerate() {
            let lane = &self.lanes[l as usize];
            let changes = lanes.get(k + 1).is_some_and(|&n| lane.is_beside(n));
            let b = if k + 1 == lanes.len() {
                s1
            } else if changes {
                (a + 10.0).min(lane.length)
            } else {
                lane.length
            };
            for p in lane.points_between(a, b) {
                if out.last().is_none_or(|q| q.distance(p) > 0.05) {
                    out.push(p);
                }
            }
            a = if changes { b } else { 0.0 };
        }
        out
    }

    /// A* over lanes from a point on one segment to a point on another.
    pub fn find_route(&self, from_seg: SegmentId, from: Vec2, to_seg: SegmentId, to: Vec2) -> Option<(Vec<LaneId>, f32, f32)> {
        let (start, s_start, _) = self.nearest_lane_on(from_seg, from)?;
        let (goal, s_end, _) = self.nearest_lane_on(to_seg, to)?;
        self.route_lanes(start, s_start, goal, s_end)
    }

    /// A* over lanes from `s_start` along `start` to `s_end` along `goal`;
    /// the route begins with `start`. A goal behind the start on the same
    /// lane has no route: lanes are one way and the start is never re-entered.
    pub fn route_lanes(&self, start: LaneId, s_start: f32, goal: LaneId, s_end: f32) -> Option<(Vec<LaneId>, f32, f32)> {
        self.route_avoiding(start, s_start, goal, s_end, None, None)
    }

    /// `route_lanes` that never enters `avoid_lane` and never takes the
    /// step `avoid_step` (from, to): a car looking for another way round a
    /// full exit or a lane change it cannot make.
    ///
    /// Lane changes are steps between parallel lanes, costed at
    /// `LANE_CHANGE_COST`; every vehicle already on a lane adds
    /// `QUEUE_COST`, so routes spread over lanes and around queues.
    pub fn route_avoiding(&self, start: LaneId, s_start: f32, goal: LaneId, s_end: f32, avoid_lane: Option<LaneId>, avoid_step: Option<(LaneId, LaneId)>) -> Option<(Vec<LaneId>, f32, f32)> {
        if start == goal && s_end >= s_start {
            return Some((vec![start], s_start, s_end));
        }
        let to = self.lanes.get(goal as usize)?.point_at(s_end).0;
        let mut open = BinaryHeap::new();
        let mut came: HashMap<LaneId, LaneId> = HashMap::new();
        let mut g: HashMap<LaneId, f32> = HashMap::new();
        let start_cost = self.lanes[start as usize].length - s_start;
        g.insert(start, start_cost);
        open.push(Open { f: start_cost + self.lanes[start as usize].end().distance(to), lane: start, expanded: 0 });
        let mut expansions = 0usize;
        while let Some(Open { lane, expanded, .. }) = open.pop() {
            if lane == goal && expanded > 0 {
                let mut route = vec![goal];
                let mut cur = goal;
                while let Some(&prev) = came.get(&cur) {
                    route.push(prev);
                    if prev == start {
                        break;
                    }
                    cur = prev;
                }
                route.reverse();
                return Some((route, s_start, s_end));
            }
            expansions += 1;
            if expansions > 20_000 {
                return None;
            }
            let gl = g[&lane];
            let here = &self.lanes[lane as usize];
            let ahead = here.next.iter().filter(|&&n| self.lanes[n as usize].permitted).map(|&n| (n, self.lanes[n as usize].length));
            let beside = [here.left, here.right].into_iter().flatten().map(|n| (n, LANE_CHANGE_COST));
            for (next, step) in ahead.chain(beside) {
                if next == start || Some(next) == avoid_lane || Some((lane, next)) == avoid_step {
                    continue;
                }
                // Changing onto the goal lane from the start puts the car
                // level with where it is: the goal must not be behind that.
                if lane == start && next == goal && here.is_beside(next) && s_end < s_start + 1.0 {
                    continue;
                }
                let cost = gl + step + QUEUE_COST * self.lanes[next as usize].cars.len() as f32;
                if g.get(&next).map_or(true, |&old| cost < old) {
                    g.insert(next, cost);
                    came.insert(next, lane);
                    open.push(Open { f: cost + self.lanes[next as usize].end().distance(to), lane: next, expanded: expanded + 1 });
                }
            }
        }
        None
    }

    /// Metres left to drive on `route` from `s` along its first lane.
    fn route_length(&self, route: &[LaneId], s: f32) -> f32 {
        let mut total = 0.0;
        for (k, &l) in route.iter().enumerate() {
            let lane = &self.lanes[l as usize];
            total += if k == 0 {
                lane.length - s
            } else if self.lanes[route[k - 1] as usize].is_beside(l) {
                LANE_CHANGE_COST
            } else {
                lane.length
            };
        }
        total
    }
}

#[derive(PartialEq)]
struct Open {
    f: f32,
    lane: LaneId,
    expanded: u32,
}

impl Eq for Open {}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        other.f.total_cmp(&self.f).then_with(|| other.lane.cmp(&self.lane))
    }
}

fn idm(v: f32, v0: f32, gap: f32, dv: f32) -> f32 {
    let gap = gap.max(0.1);
    let s_star = IDM_MIN_GAP + (v * IDM_HEADWAY + v * dv / (2.0 * (IDM_ACCEL * IDM_BRAKE).sqrt())).max(0.0);
    IDM_ACCEL * (1.0 - (v / v0).powi(4) - (s_star / gap).powi(2))
}

impl Traffic {
    pub fn rebuild(&mut self, roads: &RoadNetwork) {
        self.net.rebuild(roads);
        for r in self.lane_use.clone() {
            if let Some(ai) = self.net.approaches.iter().position(|a| a.segment == r.segment && a.node == r.node && a.lanes.len() == r.arrows.len()) {
                self.net.approaches[ai].fixed = true;
                self.net.set_arrows(ai, r.arrows.clone());
            }
        }
    }

    /// Drop every car; used when the road network changes shape.
    pub fn clear_cars(&mut self) -> Vec<(CarId, Car)> {
        let out: Vec<(CarId, Car)> = self.cars.drain().collect();
        for lane in &mut self.net.lanes {
            lane.cars.clear();
        }
        out
    }

    /// After loading a save the lane network is rebuilt; put cars back on
    /// their lanes and drop any whose route no longer exists.
    pub fn reattach_cars(&mut self) -> Vec<(CarId, Car)> {
        let lane_count = self.net.lanes.len() as LaneId;
        let invalid: Vec<CarId> = self
            .cars
            .iter()
            .filter(|(_, c)| c.route.iter().any(|&l| l >= lane_count) || c.idx >= c.route.len())
            .map(|(id, _)| id)
            .collect();
        let dropped: Vec<(CarId, Car)> = invalid.into_iter().filter_map(|id| self.cars.remove(id).map(|c| (id, c))).collect();
        for lane in &mut self.net.lanes {
            lane.cars.clear();
        }
        let ids: Vec<CarId> = self.cars.keys().filter(|&id| !self.cars[id].entering).collect();
        for id in ids {
            let lane = self.cars[id].route[self.cars[id].idx];
            self.insert_sorted(lane, id);
        }
        dropped
    }

    fn insert_sorted(&mut self, lane: LaneId, car: CarId) {
        let s = self.cars[car].s;
        let list = &mut self.net.lanes[lane as usize].cars;
        let pos = list.iter().position(|&other| self.cars[other].s > s).unwrap_or(list.len());
        list.insert(pos, car);
    }

    pub fn spawn_car(&mut self, citizen: CitizenId, cargo: Option<Cargo>, route: Vec<LaneId>, s_start: f32, s_end: f32, color: u8) -> CarId {
        let lane = route[0];
        let (pos, dir) = self.net.lanes[lane as usize].point_at(s_start);
        let heading = dir.y.atan2(dir.x);
        let id = self.cars.insert(Car { citizen, cargo, route, idx: 0, s: s_start, s_end, v: 0.0, pos, heading, prev_pos: pos, prev_heading: heading, color, age: 0.0, bus: None, hold: 0.0, lateral: 0.0, stalled: 0.0, wait: Wait::Free, cooldown: 0.0, entering: false, passing: false, turn_wait: 0.0, yielded: 0.0, siren: false, model: Default::default() });
        self.pull_out(id);
        id
    }

    /// A new car joins its first lane only where there is room (no overlap
    /// with the vehicles there, nobody forced to brake hard); otherwise it
    /// waits at the kerb and tries again every step.
    fn pull_out(&mut self, id: CarId) -> bool {
        let lane = self.cars[id].route[self.cars[id].idx];
        let room = self.change_ok(id, lane, LC_SAFE_BRAKE).is_some();
        let kerb = self.net.lanes[lane as usize].right.is_none();
        let c = &mut self.cars[id];
        if room {
            if c.entering {
                c.entering = false;
                c.cooldown = LC_COOLDOWN;
            }
            self.insert_sorted(lane, id);
        } else if !c.entering {
            c.entering = true;
            c.wait = Wait::Entering;
            // Drawn pulled in at the kerb until it can go.
            if kerb {
                c.lateral = LANE_WIDTH * 0.6;
            }
        }
        room
    }

    /// Send a car on a new route from where it stands: a bus leaving a stop.
    /// If the new route starts on the car's current lane it keeps its place;
    /// otherwise it is moved to the start of the new route.
    pub fn reroute(&mut self, id: CarId, route: Vec<LaneId>, s_start: f32, s_end: f32) {
        if route.is_empty() {
            return;
        }
        let old_lane = self.cars[id].route[self.cars[id].idx];
        if route[0] == old_lane {
            let car = &mut self.cars[id];
            car.route = route;
            car.idx = 0;
            car.s_end = s_end;
            car.age = 0.0;
            return;
        }
        if let Some(l) = self.net.lanes.get_mut(old_lane as usize) {
            l.cars.retain(|&c| c != id);
        }
        let first = route[0];
        {
            let car = &mut self.cars[id];
            car.route = route;
            car.idx = 0;
            car.s = s_start;
            car.s_end = s_end;
            car.age = 0.0;
        }
        self.insert_sorted(first, id);
    }

    pub fn remove_car(&mut self, id: CarId) -> Option<Car> {
        let car = self.cars.remove(id)?;
        if let Some(&lane) = car.route.get(car.idx) {
            if let Some(l) = self.net.lanes.get_mut(lane as usize) {
                l.cars.retain(|&c| c != id);
            }
        }
        Some(car)
    }

    /// Route a stalled car afresh from the lane it stands on to the same
    /// destination, keeping its place in the lane. False when no route
    /// exists (the destination is behind it on its own lane, say).
    fn replan_from_here(&mut self, id: CarId) -> bool {
        let (here, s, goal, s_end) = {
            let c = &self.cars[id];
            (c.route[c.idx], c.s, *c.route.last().unwrap(), c.s_end)
        };
        let Some((route, _, _)) = self.net.route_lanes(here, s, goal, s_end) else { return false };
        let car = &mut self.cars[id];
        car.route = route;
        car.idx = 0;
        car.age = 0.0;
        true
    }

    /// Would a car entering the connector `conn` find no room on `exit`?
    /// Room is measured with the real vehicles on the exit lane: a bus
    /// takes nearly three cars' worth. `strict` (a real junction, where a
    /// car left in the box blocks cross traffic) also counts a stopped
    /// tail right at the start of the exit as no room.
    fn exit_blocked(&self, conn: &Lane, exit: LaneId, strict: bool) -> bool {
        let el = &self.net.lanes[exit as usize];
        let queue: f32 = el.cars.iter().map(|&c| self.cars[c].length() + IDM_MIN_GAP).sum();
        let last = el.cars.first().map(|&c| &self.cars[c]);
        // Its tail is at the very start and the lane is packed, or that
        // tail is not moving off.
        let blocked = last.is_some_and(|c| c.s < c.length() + IDM_MIN_GAP + 1.0 && (queue > el.length - conn.length || (strict && c.v < 0.5)));
        let tail_near = last.is_some_and(|c| c.s < c.length() + 0.5);
        blocked || (tail_near && !conn.cars.is_empty())
    }

    /// A connector inside a junction of three or more streets (not a bend).
    fn at_junction(&self, conn: &Lane) -> bool {
        let LaneKind::Connector { node, .. } = conn.kind else { return false };
        self.net.node_intersection.get(node.0 as usize).and_then(|&i| self.net.intersections.get(i)).is_some_and(|i| i.approaches >= 3)
    }

    /// Gap, leader speed and the reason for a car with no leader on its
    /// own lane: look down the route for the next car or a red light.
    fn lookahead(&self, car: &Car, lane: &Lane) -> (f32, f32, Wait) {
        let mut dist = lane.length - car.s;
        let route = &car.route;
        let mut prev = route[car.idx];
        for (k, &next) in route.iter().enumerate().skip(car.idx + 1).take(4) {
            if self.net.lanes[prev as usize].is_beside(next) {
                // A lane change further on; the car deals with it there.
                break;
            }
            let nl = &self.net.lanes[next as usize];
            if nl.is_connector() && !nl.green {
                // Stop line just before the lane ends; the min gap keeps the nose clear.
                return (dist - 1.0 + IDM_MIN_GAP, 0.0, Wait::Signal);
            }
            // Do not enter a junction unless the exit lane has room: this is
            // what keeps a saturated grid from locking up.
            if nl.is_connector() && !lane.is_connector() && k == car.idx + 1 {
                if let LaneKind::Connector { node, approach } = nl.kind {
                    if let Some(inter) = self.net.node_intersection.get(node.0 as usize).and_then(|&i| self.net.intersections.get(i)) {
                        // Merging traffic yields; the highway mainline does not.
                        if !inter.signalized && inter.approaches >= 3 && lane.speed_limit <= ROAD_SPEED {
                            let busy = inter.connectors.iter().any(|&(lid, ap)| ap != approach && !self.net.lanes[lid as usize].cars.is_empty());
                            if busy {
                                return (dist - 1.0 + IDM_MIN_GAP, 0.0, Wait::Yield);
                            }
                        }
                        if inter.signalized && nl.turn < 0 && self.oncoming(inter, approach) {
                            return (dist - 1.0 + IDM_MIN_GAP, 0.0, Wait::Yield);
                        }
                    }
                }
                if let Some(&exit) = route.get(k + 1) {
                    if self.exit_blocked(nl, exit, self.at_junction(nl)) {
                        return (dist - 1.0 + IDM_MIN_GAP, 0.0, Wait::ExitFull);
                    }
                }
            }
            if let Some(&first) = nl.cars.first() {
                let lead = &self.cars[first];
                return (dist + lead.s - lead.length(), lead.v, Wait::Leader);
            }
            dist += nl.length;
            if dist > LOOKAHEAD {
                break;
            }
            prev = next;
        }
        (LOOKAHEAD * 2.0, ROAD_SPEED, Wait::Free)
    }

    /// A left turn on a shared green yields to oncoming traffic going
    /// straight or right: vehicles in the junction from the opposite
    /// approach, or about to enter it at speed. Opposing left turns pass
    /// each other and do not yield.
    fn oncoming(&self, inter: &Intersection, approach: usize) -> bool {
        let Some(opp) = inter.opposite.get(approach).copied().flatten() else { return false };
        let lanes = &self.net.lanes;
        let in_junction = inter.connectors.iter().any(|&(lid, ap)| ap == opp && lanes[lid as usize].turn >= 0 && !lanes[lid as usize].cars.is_empty());
        in_junction
            || inter.incoming.iter().any(|&(lid, ap)| {
                let lane = &lanes[lid as usize];
                ap == opp
                    && lane.cars.last().is_some_and(|&h| {
                        let c = &self.cars[h];
                        let through = c.route.get(c.idx + 1).is_some_and(|&n| {
                            let nl = &lanes[n as usize];
                            nl.is_connector() && nl.green && nl.turn >= 0
                        });
                        through && c.v > 1.0 && lane.length - c.s < 25.0
                    })
            })
    }

    /// Per signalised intersection and approach: is a car waiting at (or
    /// close to) the stop line?
    fn signal_demand(&self) -> Vec<SignalDemand> {
        self.net
            .intersections
            .iter()
            .map(|inter| {
                let n = if inter.signalized { inter.approaches } else { 0 };
                let mut d = SignalDemand { any: vec![false; n], left_queue: vec![0; n], left_starved: vec![false; n], dedicated: vec![false; n], left_yielding: vec![false; n] };
                if inter.signalized {
                    for &(lid, ap) in &inter.incoming {
                        let lane = &self.net.lanes[lid as usize];
                        if lane.arrows == LEFT {
                            d.dedicated[ap] = true;
                        }
                        if lane.cars.last().is_some_and(|&c| self.cars[c].s > lane.length - DEMAND_REACH) {
                            d.any[ap] = true;
                        }
                        // Left turners count only behind a head that could
                        // actually go: an arrow is wasted on a full exit.
                        let head_can_turn = lane.cars.last().is_some_and(|&h| {
                            let car = &self.cars[h];
                            self.next_turn(car) == Some(-1) && match (car.route.get(car.idx + 1), car.route.get(car.idx + 2)) {
                                (Some(&conn), Some(&exit)) => !self.exit_blocked(&self.net.lanes[conn as usize], exit, true),
                                _ => false,
                            }
                        });
                        if head_can_turn {
                            for (k, &c) in lane.cars.iter().enumerate().rev() {
                                let car = &self.cars[c];
                                if car.s < lane.length - DEMAND_REACH {
                                    break;
                                }
                                if self.next_turn(car) == Some(-1) {
                                    d.left_queue[ap] = d.left_queue[ap].saturating_add(1);
                                    if k + 1 == lane.cars.len() {
                                        d.left_starved[ap] |= car.yielded > LEFT_STARVED;
                                        d.left_yielding[ap] |= car.yielded > LEFT_YIELDING;
                                    }
                                }
                            }
                        }
                    }
                }
                d
            })
            .collect()
    }

    /// The turn of the connector a car on a road lane takes next, if any.
    fn next_turn(&self, car: &Car) -> Option<i8> {
        let here = &self.net.lanes[car.route[car.idx] as usize];
        if here.is_connector() {
            return None;
        }
        let next = &self.net.lanes[*car.route.get(car.idx + 1)? as usize];
        next.is_connector().then_some(next.turn)
    }

    /// A left turner at the head of its lane, close to the stop line and
    /// slow: someone the car behind may pass on the right.
    fn waiting_left(&self, lane: &Lane, id: CarId) -> bool {
        let c = &self.cars[id];
        lane.cars.last() == Some(&id) && c.v < 3.0 && lane.length - c.s < 25.0 && self.next_turn(c) == Some(-1)
    }

    /// How far right of its lane centre a car can squeeze past another on
    /// this lane without touching the kerb, if there is room at all.
    fn pass_offset(lane: &Lane) -> Option<f32> {
        let room = LANE_WIDTH * 0.5 + SHOULDER - CAR_WIDTH * 0.5 - 0.15;
        let need = CAR_WIDTH + 0.25;
        (lane.right.is_none() && room >= need).then_some(need)
    }

    /// Mean and maximum of the recent left turn waits, and how many.
    pub fn left_wait_stats(&self) -> (f32, f32, usize) {
        let n = self.left_waits.len();
        if n == 0 {
            return (0.0, 0.0, 0);
        }
        (self.left_waits.iter().sum::<f32>() / n as f32, self.left_waits.iter().copied().fold(0.0, f32::max), n)
    }

    /// Set (or with `None` clear) the player's arrows for the lanes of
    /// `segment` arriving at `node`, leftmost first.
    pub fn set_lane_use(&mut self, segment: SegmentId, node: NodeId, arrows: Option<Vec<u8>>) -> Result<(), String> {
        let Some(ai) = self.net.approaches.iter().position(|a| a.segment == segment && a.node == node) else {
            return Err("that street does not arrive at a junction there".into());
        };
        self.lane_use.retain(|r| !(r.segment == segment && r.node == node));
        match arrows {
            Some(a) => {
                let ap = &self.net.approaches[ai];
                if a.len() != ap.lanes.len() {
                    return Err(format!("the approach has {} lanes, {} arrows given", ap.lanes.len(), a.len()));
                }
                if a.iter().any(|&x| x & ap.exits == 0) {
                    return Err("every lane needs a movement the junction offers".into());
                }
                if a.iter().fold(0, |acc, &x| acc | x) & ap.exits != ap.exits {
                    return Err("some exit would be reachable from no lane".into());
                }
                let a: Vec<u8> = a.iter().map(|&x| x & ap.exits).collect();
                self.lane_use.push(LaneUseRule { segment, node, arrows: a.clone() });
                self.net.approaches[ai].fixed = true;
                self.net.set_arrows(ai, a);
            }
            None => {
                let ap = &self.net.approaches[ai];
                let a = default_arrows(ap.lanes.len(), ap.exits, None, None);
                self.net.approaches[ai].fixed = false;
                self.net.set_arrows(ai, a);
            }
        }
        Ok(())
    }

    /// Revise the default arrows of approaches the player has not set from
    /// their recent turning counts.
    fn adapt_arrows(&mut self) {
        for ai in 0..self.net.approaches.len() {
            let ap = &self.net.approaches[ai];
            let total: f32 = ap.counts.iter().sum();
            if !ap.fixed && total >= 20.0 {
                let a = default_arrows(ap.lanes.len(), ap.exits, Some(ap.counts[0] / total), Some(&ap.arrows));
                self.net.set_arrows(ai, a);
            }
            let ap = &mut self.net.approaches[ai];
            for c in &mut ap.counts {
                *c *= 0.5;
            }
        }
    }

    /// Leader on `lane` ahead of position `s` and the follower behind it
    /// (looking back onto the lanes feeding it), as `(id, gap)` measured for
    /// a vehicle of length `len` standing at `s`.
    fn neighbours_at(&self, lane: LaneId, s: f32, len: f32, skip: CarId) -> (Option<(CarId, f32)>, Option<(CarId, f32)>) {
        let tl = &self.net.lanes[lane as usize];
        let cars: Vec<CarId> = tl.cars.iter().copied().filter(|&c| c != skip).collect();
        let pos = cars.iter().position(|&o| self.cars[o].s > s).unwrap_or(cars.len());
        let leader = cars.get(pos).map(|&l| (l, self.cars[l].s - s - self.cars[l].length()));
        let follower = if pos > 0 {
            let f = cars[pos - 1];
            Some((f, s - self.cars[f].s - len))
        } else {
            tl.prev
                .iter()
                .filter_map(|&p| {
                    let pl = &self.net.lanes[p as usize];
                    pl.cars.last().map(|&f| (f, s + pl.length - self.cars[f].s - len))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
        };
        (leader, follower)
    }

    /// Can the car move across into `target` (beside its lane) right now
    /// without itself or its new follower braking harder than `brake`?
    /// Returns its acceleration there.
    fn change_ok(&self, id: CarId, target: LaneId, brake: f32) -> Option<f32> {
        let c = &self.cars[id];
        let tl = &self.net.lanes[target as usize];
        let s = c.s.min(tl.length);
        let (leader, follower) = self.neighbours_at(target, s, c.length(), id);
        let acc = match leader {
            Some((l, gap)) => {
                if gap < IDM_MIN_GAP * 0.5 {
                    return None;
                }
                idm(c.v, c.desired_speed(tl.speed_limit), gap, c.v - self.cars[l].v)
            }
            None => idm(c.v, c.desired_speed(tl.speed_limit), tl.length - s + LOOKAHEAD, 0.0),
        };
        if acc < -brake {
            return None;
        }
        if let Some((f, gap)) = follower {
            let fc = &self.cars[f];
            if gap < IDM_MIN_GAP * 0.5 {
                return None;
            }
            if idm(fc.v, fc.desired_speed(tl.speed_limit), gap, fc.v - c.v) < -brake {
                return None;
            }
        }
        Some(acc)
    }

    /// Move a car into the lane beside it, on its route's next step or on a
    /// new route that starts there.
    fn change_lane(&mut self, id: CarId, target: LaneId, new_route: Option<Vec<LaneId>>) {
        let old = self.cars[id].route[self.cars[id].idx];
        self.net.lanes[old as usize].cars.retain(|&c| c != id);
        let shift = if self.net.lanes[old as usize].right == Some(target) { -LANE_WIDTH } else { LANE_WIDTH };
        let len = self.net.lanes[target as usize].length;
        let c = &mut self.cars[id];
        match new_route {
            Some(r) => {
                c.route = r;
                c.idx = 0;
            }
            None => c.idx += 1,
        }
        c.s = c.s.min(len);
        c.lateral = (c.lateral + shift).clamp(-1.5 * LANE_WIDTH, 1.5 * LANE_WIDTH);
        c.cooldown = LC_COOLDOWN;
        self.insert_sorted(target, id);
    }

    /// Lane changes for one physics step: cars whose route steps into the
    /// lane beside them change as soon as it is safe; others overtake a
    /// vehicle holding them up when a parallel lane is better and the
    /// route from there is no real detour.
    fn lane_changes(&mut self) {
        let ids: Vec<CarId> = self.cars.keys().collect();
        for id in ids {
            let c = &self.cars[id];
            if c.hold > 0.0 || c.entering {
                continue;
            }
            let lane = &self.net.lanes[c.route[c.idx] as usize];
            if lane.is_connector() {
                continue;
            }
            if lane.left.is_none() && lane.right.is_none() {
                // One lane: squeeze past a waiting left turner on the right.
                let k = lane.cars.iter().position(|&o| o == id);
                let lead = k.and_then(|k| lane.cars.get(k + 1)).copied();
                if c.passing {
                    if !lead.is_some_and(|l| self.next_turn(&self.cars[l]) == Some(-1)) {
                        self.cars[id].passing = false;
                    }
                } else if let (Some(l), Some(_)) = (lead, Self::pass_offset(lane)) {
                    let gap = self.cars[l].s - c.s - self.cars[l].length();
                    let through = self.next_turn(c).is_some_and(|t| t >= 0) && self.net.lanes[c.route[c.idx + 1] as usize].permitted;
                    let at_junction = self.net.node_intersection.get(lane.end_node.0 as usize).and_then(|&i| self.net.intersections.get(i)).is_some_and(|i| i.approaches >= 3);
                    if c.bus.is_none() && through && at_junction && c.v < PASS_MAX_SPEED && gap < 30.0 && c.s < lane.length - CROSSING_SETBACK - 6.0 && self.waiting_left(lane, l) {
                        self.cars[id].passing = true;
                    }
                }
                continue;
            }
            if let Some(target) = c.change_pending(&self.net) {
                let forced = lane.length - c.s < 8.0 && c.v < 1.0;
                if self.change_ok(id, target, if forced { LC_FORCED_BRAKE } else { LC_SAFE_BRAKE }).is_some() {
                    self.change_lane(id, target, None);
                }
                continue;
            }
            if c.cooldown > 0.0 || lane.length - c.s < 12.0 || (c.idx + 1 == c.route.len() && c.s_end - c.s < 40.0) {
                continue;
            }
            // Held up by a vehicle that is slow on its own account?
            let Some(k) = lane.cars.iter().position(|&o| o == id) else { continue };
            let Some(&lead) = lane.cars.get(k + 1) else { continue };
            let l = &self.cars[lead];
            let gap = l.s - c.s - l.length();
            let v0 = c.desired_speed(lane.speed_limit);
            // A bus at a stop, a slower truck, or a left turner about to wait
            // for a gap in the oncoming traffic.
            let slow = l.hold > 0.0 || (l.wait == Wait::Free && l.v < 0.9 * v0) || (self.waiting_left(lane, lead) && self.next_turn(c) != Some(-1));
            if gap > 35.0 || !slow {
                continue;
            }
            let acc_here = idm(c.v, v0, gap, c.v - l.v);
            let (goal, s_end, s) = (*c.route.last().unwrap(), c.s_end, c.s);
            let old_len = self.net.route_length(&c.route[c.idx..], s);
            for target in [lane.left, lane.right].into_iter().flatten() {
                let Some(acc) = self.change_ok(id, target, LC_SAFE_BRAKE) else { continue };
                if acc - acc_here < OVERTAKE_GAIN {
                    continue;
                }
                let Some((route, _, _)) = self.net.route_lanes(target, s, goal, s_end) else { continue };
                if self.net.route_length(&route, s) > old_len + 3.0 * LANE_CHANGE_COST + 20.0 {
                    continue;
                }
                self.change_lane(id, target, Some(route));
                break;
            }
        }
    }

    /// Bookkeeping as a car moves from `from` onto `to`: turning counts for
    /// the approach's arrows, and the wait before a left turn.
    fn entered(&mut self, from: LaneId, to: LaneId, id: CarId) {
        let (ap, turn) = (self.net.lanes[from as usize].approach, self.net.lanes[to as usize].turn);
        if !self.net.lanes[to as usize].is_connector() {
            return;
        }
        if let Some(a) = self.net.approaches.get_mut(ap as usize) {
            a.counts[(turn + 1) as usize] += 1.0;
            // counts are [left, straight, right]
        }
        if turn < 0 {
            let wait = std::mem::take(&mut self.cars[id].turn_wait);
            self.cars[id].yielded = 0.0;
            let node = self.net.lanes[to as usize].end_node;
            if let Some(inter) = self.net.node_intersection.get(node.0 as usize).copied().and_then(|i| self.net.intersections.get_mut(i)) {
                inter.left_turns += 1;
                inter.left_wait_sum += wait;
                inter.left_wait_max = inter.left_wait_max.max(wait);
            }
            self.left_waits.push_back(wait);
            if self.left_waits.len() > LEFT_WAIT_WINDOW {
                self.left_waits.pop_front();
            }
        } else {
            self.cars[id].turn_wait = 0.0;
            self.cars[id].yielded = 0.0;
        }
    }

    /// Cars stalled at the head of their lane for `REROUTE_AFTER` seconds
    /// because their exit is full (a gridlock ring, a jammed street) or
    /// because they cannot get into the lane their turn needs look for
    /// another way from where they stand.
    fn reroute_stalled(&mut self) {
        let ids: Vec<CarId> = self.cars.keys().collect();
        for id in ids {
            let c = &self.cars[id];
            if c.stalled < REROUTE_LANE_CHANGE_AFTER || c.cooldown > 0.0 || c.hold > 0.0 || c.entering || c.idx + 1 >= c.route.len() {
                continue;
            }
            let here = c.route[c.idx];
            let lane = &self.net.lanes[here as usize];
            if lane.is_connector() || lane.cars.last() != Some(&id) || lane.length - c.s > 6.0 {
                continue;
            }
            let next = c.route[c.idx + 1];
            let (avoid_lane, avoid_step) = if lane.is_beside(next) {
                (None, Some((here, next)))
            } else if c.stalled < REROUTE_AFTER {
                continue;
            } else {
                match c.route.get(c.idx + 2) {
                    Some(&exit) if self.exit_blocked(&self.net.lanes[next as usize], exit, true) => (Some(exit), None),
                    _ => continue,
                }
            };
            let (goal, s_end, s) = (*c.route.last().unwrap(), c.s_end, c.s);
            let found = self.net.route_avoiding(here, s, goal, s_end, avoid_lane, avoid_step);
            let c = &mut self.cars[id];
            c.cooldown = REROUTE_LANE_CHANGE_AFTER;
            if let Some((route, _, _)) = found {
                c.route = route;
                c.idx = 0;
            }
        }
    }

    /// Rings of full lanes whose leading vehicles each wait for the next
    /// lane of the ring: true gridlocks, not queues at a light.
    pub fn gridlocks(&self) -> Vec<Gridlock> {
        let waits_on = |lane: usize| -> Option<usize> {
            let l = &self.net.lanes[lane];
            if l.is_connector() {
                return None;
            }
            let c = &self.cars[*l.cars.last()?];
            if c.v >= 0.5 || l.length - c.s > 6.0 {
                return None;
            }
            let conn = *c.route.get(c.idx + 1)?;
            let exit = *c.route.get(c.idx + 2)?;
            let cl = &self.net.lanes[conn as usize];
            (cl.is_connector() && self.exit_blocked(cl, exit, true)).then_some(exit as usize)
        };
        let mut state = vec![0u8; self.net.lanes.len()];
        let mut out = Vec::new();
        for start in 0..self.net.lanes.len() {
            if state[start] != 0 {
                continue;
            }
            let mut path = Vec::new();
            let mut cur = start;
            loop {
                if state[cur] == 1 {
                    let i = path.iter().position(|&x| x == cur).unwrap_or(0);
                    let lanes: Vec<LaneId> = path[i..].iter().map(|&l| l as LaneId).collect();
                    out.push(Gridlock {
                        junctions: lanes.iter().map(|&l| self.net.lanes[l as usize].end_node).collect(),
                        points: lanes.iter().map(|&l| self.net.lanes[l as usize].end()).collect(),
                        vehicles: lanes.iter().map(|&l| self.net.lanes[l as usize].cars.len()).sum(),
                        lanes,
                    });
                    break;
                }
                if state[cur] == 2 {
                    break;
                }
                state[cur] = 1;
                path.push(cur);
                match waits_on(cur) {
                    Some(n) => cur = n,
                    None => break,
                }
            }
            for p in path {
                state[p] = 2;
            }
        }
        out
    }

    /// Advance all cars by `seconds` in `substeps` steps. Returns cars that
    /// reached their destination (they stay in `cars` until removed).
    /// [services] Cars a little ahead of an emergency vehicle on its lane
    /// pull to the right and slow to a crawl so it can pass.
    fn yield_to_sirens(&mut self) {
        let mut pull: Vec<CarId> = Vec::new();
        for lane in &self.net.lanes {
            for (k, &cid) in lane.cars.iter().enumerate() {
                if !self.cars[cid].siren {
                    continue;
                }
                let s = self.cars[cid].s;
                pull.extend(lane.cars[k + 1..].iter().copied().filter(|&o| !self.cars[o].siren && self.cars[o].s - s < SIREN_CLEAR));
            }
        }
        for id in pull {
            let c = &mut self.cars[id];
            c.lateral = c.lateral.max(SIREN_PULL_ASIDE);
            c.v = c.v.min(2.0);
        }
    }

    pub fn step(&mut self, seconds: f32, substeps: usize) -> Vec<CarId> {
        let dt = seconds / substeps.max(1) as f32;
        for car in self.cars.values_mut() {
            car.prev_pos = car.pos;
            car.prev_heading = car.heading;
            car.age += seconds;
        }
        let mut arrived = Vec::new();
        for _ in 0..substeps {
            let demand = self.signal_demand();
            self.net.step_signals(dt, Some(&demand));

            // Kerb positions where a car has waited long enough to be let in.
            let mut claims: HashMap<LaneId, Vec<f32>> = HashMap::new();
            for c in self.cars.values().filter(|c| c.entering && c.stalled > COURTESY_AFTER) {
                claims.entry(c.route[c.idx]).or_default().push(c.s);
            }
            let mut accs: Vec<(CarId, f32, Wait, bool)> = Vec::with_capacity(self.cars.len());
            for (lane_id, lane) in self.net.lanes.iter().enumerate() {
                let lane_claims = claims.get(&(lane_id as LaneId));
                for (k, &cid) in lane.cars.iter().enumerate() {
                    let car = &self.cars[cid];
                    if car.hold > 0.0 {
                        accs.push((cid, -1000.0, Wait::Dwell, false));
                        continue;
                    }
                    let pending = car.change_pending(&self.net).is_some();
                    // Passing a left turner: once out on the shoulder, the
                    // turner is no longer in the way.
                    let beside = car.passing
                        && Self::pass_offset(lane).is_some_and(|o| car.lateral >= 0.8 * o)
                        && lane.cars.get(k + 1).is_some_and(|&l| self.next_turn(&self.cars[l]) == Some(-1));
                    let lead = lane.cars.get(if beside { k + 2 } else { k + 1 });
                    let (mut gap, mut lead_v, mut why) = match lead {
                        Some(&lead) => (self.cars[lead].s - car.s - self.cars[lead].length(), self.cars[lead].v, Wait::Leader),
                        None if pending => (LOOKAHEAD * 2.0, ROAD_SPEED, Wait::Free),
                        None => self.lookahead(car, lane),
                    };
                    if car.passing && matches!(why, Wait::Signal | Wait::ExitFull | Wait::Yield) {
                        // On the shoulder: stop short of the crossing.
                        gap -= CROSSING_SETBACK;
                    }
                    // [services] Lights and sirens: no leader, no red light.
                    if car.siren {
                        (gap, lead_v, why) = (LOOKAHEAD * 2.0, ROAD_SPEED, Wait::Free);
                    }
                    if pending {
                        // Stop at the end of the lane until the change is made.
                        let to_end = lane.length - car.s - 1.0 + IDM_MIN_GAP;
                        if to_end < gap {
                            (gap, lead_v, why) = (to_end, 0.0, Wait::LaneChange);
                        }
                    } else if car.idx + 1 == car.route.len() {
                        let to_stop = car.s_end + IDM_MIN_GAP - car.s;
                        if to_stop < gap {
                            (gap, lead_v, why) = (to_stop, 0.0, Wait::Arriving);
                        }
                    }
                    // The first driver behind a waiting car stops short to let it out.
                    if let Some(cl) = lane_claims {
                        let ahead = lane.cars.get(k + 1).map_or(f32::MAX, |&l| self.cars[l].s);
                        for &at in cl.iter().filter(|&&at| at > car.s + 1.0 && at < ahead - 1.0) {
                            let room = at - CAR_LENGTH - IDM_MIN_GAP - car.s;
                            if room < gap && car.v * car.v < 2.0 * IDM_BRAKE * room.max(0.0) {
                                (gap, lead_v, why) = (room + IDM_MIN_GAP, 0.0, Wait::Leader);
                            }
                        }
                    }
                    if why == Wait::Leader && gap > 50.0 {
                        why = Wait::Free;
                    }
                    let left_wait = car.v < 0.5 && self.next_turn(car) == Some(-1);
                    accs.push((cid, idm(car.v, car.desired_speed(lane.speed_limit), gap, car.v - lead_v), why, left_wait));
                }
            }
            for (cid, acc, why, left_wait) in accs {
                let car = &mut self.cars[cid];
                car.wait = why;
                if left_wait {
                    car.turn_wait += dt;
                    if why == Wait::Yield {
                        car.yielded += dt;
                    }
                }
                car.cooldown = (car.cooldown - dt).max(0.0);
                if car.hold > 0.0 {
                    car.hold -= dt;
                    car.v = 0.0;
                    car.stalled = 0.0;
                    continue;
                }
                car.v = (car.v + acc * dt).max(0.0);
                car.s += car.v * dt;
                car.stalled = if car.v < 0.5 { car.stalled + dt } else { 0.0 };
                if car.passing {
                    // Out onto the shoulder (the offset was checked against
                    // the kerb when the pass began).
                    let target = CAR_WIDTH + 0.25;
                    let rate = PASS_LATERAL_SPEED * (car.v / 3.0).clamp(0.5, 1.0) * dt;
                    car.lateral = (car.lateral + rate).min(target.max(car.lateral));
                } else if car.lateral != 0.0 {
                    let rate = LC_LATERAL_SPEED * (car.v / 3.0).clamp(0.3, 1.0) * dt;
                    car.lateral -= car.lateral.signum() * car.lateral.abs().min(rate);
                }
            }

            self.yield_to_sirens();

            let waiting: Vec<CarId> = self.cars.iter().filter(|(_, c)| c.entering).map(|(id, _)| id).collect();
            for id in waiting {
                if !self.pull_out(id) {
                    let c = &mut self.cars[id];
                    c.stalled += dt;
                    c.wait = Wait::Entering;
                }
            }

            self.lane_changes();

            let ids: Vec<CarId> = self.cars.keys().collect();
            for cid in ids {
                let (lane_id, idx, len, s, s_end) = {
                    let c = &self.cars[cid];
                    let lane_id = c.route[c.idx];
                    (lane_id, c.idx, self.net.lanes[lane_id as usize].length, c.s, c.s_end)
                };
                if self.cars[cid].age > TRIP_TIMEOUT {
                    if self.cars[cid].bus.is_some() && self.replan_from_here(cid) {
                        continue;
                    }
                    if !arrived.contains(&cid) {
                        arrived.push(cid);
                    }
                    continue;
                }
                if self.cars[cid].entering {
                    continue;
                }
                if idx + 1 == self.cars[cid].route.len() {
                    if s >= s_end - 0.5 {
                        let c = &mut self.cars[cid];
                        c.s = s_end.min(len);
                        c.v = 0.0;
                        if !arrived.contains(&cid) {
                            arrived.push(cid);
                        }
                    }
                } else if s >= len {
                    if self.cars[cid].change_pending(&self.net).is_some() {
                        // Never roll off the end into the lane beside.
                        let c = &mut self.cars[cid];
                        c.s = len - 0.05;
                        c.v = 0.0;
                        continue;
                    }
                    self.net.lanes[lane_id as usize].cars.retain(|&c| c != cid);
                    let next = {
                        let c = &mut self.cars[cid];
                        c.s -= len;
                        c.idx += 1;
                        c.passing = false;
                        c.route[c.idx]
                    };
                    self.entered(lane_id, next, cid);
                    self.insert_sorted(next, cid);
                }
            }
            // A car that drew level with the turner it passes goes ahead of
            // it in the lane's order.
            let passers: Vec<CarId> = self.cars.iter().filter(|(_, c)| c.passing).map(|(id, _)| id).collect();
            for id in passers {
                let lane = self.cars[id].route[self.cars[id].idx];
                let cars = &mut self.net.lanes[lane as usize].cars;
                if let Some(k) = cars.iter().position(|&c| c == id) {
                    if k + 1 < cars.len() && self.cars[cars[k + 1]].s < self.cars[id].s {
                        cars.swap(k, k + 1);
                    }
                }
            }
        }
        self.reroute_stalled();
        self.adapt_clock += seconds;
        if self.adapt_clock >= ADAPT_SECONDS {
            self.adapt_clock = 0.0;
            self.adapt_arrows();
        }
        for car in self.cars.values_mut() {
            let lane = &self.net.lanes[car.route[car.idx] as usize];
            let (p, d) = lane.point_at(car.s);
            let right = Vec2::new(-d.y, d.x);
            car.pos = p + right * car.lateral;
            let mut heading = d.y.atan2(d.x);
            if car.lateral.abs() > 0.05 && car.v > 0.5 {
                // Nose turned toward the lane it is drifting into.
                heading += (-car.lateral.signum() * LC_LATERAL_SPEED).atan2(car.v.max(2.0));
            }
            car.heading = heading;
        }
        arrived
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cross() -> RoadNetwork {
        let mut net = RoadNetwork::default();
        net.add_polyline(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
        net.add_polyline(&[Vec2::new(0.0, -200.0), Vec2::new(0.0, 200.0)], 1, 1);
        net
    }

    #[test]
    fn lanes_and_connectors_are_generated() {
        let roads = cross();
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let road_lanes = t.net.lanes.iter().filter(|l| !l.is_connector()).count();
        let connectors = t.net.lanes.iter().filter(|l| l.is_connector()).count();
        assert_eq!(road_lanes, 8);
        // Centre: 4 incoming lanes x 3 outgoing (no U-turn) = 12; dead ends: 4 U-turns.
        assert_eq!(connectors, 16);
        assert!(t.net.intersections.iter().any(|i| i.signalized));
    }

    #[test]
    fn lanes_keep_right_and_turns_stay_inside_junctions() {
        let roads = cross();
        let mut t = Traffic::default();
        t.rebuild(&roads);
        // Eastbound lane (a -> b with +x direction) must sit on the +y side in
        // sim coordinates, which is the driver's right once y maps to world z.
        for lane in t.net.lanes.iter().filter(|l| !l.is_connector()) {
            let d = (lane.end() - lane.start()).normalize_or_zero();
            let mid = (lane.start() + lane.end()) * 0.5;
            // Perpendicular offset of the lane from the segment centreline.
            let centre_offset = if d.x.abs() > 0.5 { mid.y } else { mid.x };
            let right = Vec2::new(-d.y, d.x);
            let side = if d.x.abs() > 0.5 { right.y } else { right.x };
            assert!(centre_offset * side > 0.0, "lane {:?} is on the left of its direction", lane.kind);
        }
        for lane in t.net.lanes.iter().filter(|l| l.is_connector()) {
            let node = roads.node(lane.end_node).pos;
            let r = t.net.node_radius[lane.end_node.0 as usize];
            for p in &lane.points {
                assert!(p.distance(node) <= r * 1.6 + 0.5, "connector swings {:.1} m out of a {:.1} m junction", p.distance(node), r);
            }
        }
    }

    #[test]
    fn a_car_drives_across_the_intersection() {
        let roads = cross();
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let west = roads.nearest_on_network(Vec2::new(-150.0, 0.0)).unwrap().0;
        let north = roads.nearest_on_network(Vec2::new(0.0, 150.0)).unwrap().0;
        let (route, s0, s1) = t.net.find_route(west, Vec2::new(-150.0, -2.0), north, Vec2::new(2.0, 150.0)).expect("route");
        assert!(route.len() >= 3);
        let citizen = CitizenId::default();
        let car = t.spawn_car(citizen, None, route, s0, s1, 0);
        let mut arrived = false;
        for _ in 0..240 {
            if t.step(1.0, 2).contains(&car) {
                arrived = true;
                break;
            }
        }
        assert!(arrived, "car should arrive within four minutes");
        assert!(t.cars[car].pos.distance(Vec2::new(2.0, 150.0)) < 6.0);
    }

    #[test]
    fn ramps_merge_without_signals_and_bends_keep_speed() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 2, 2);
        // A one-way ramp joining at a shallow angle.
        roads.add_polyline(&[Vec2::new(-150.0, 60.0), Vec2::new(-20.0, 2.0)], 1, 0);
        // A two-way side street.
        roads.add_polyline(&[Vec2::new(150.0, 200.0), Vec2::new(150.0, 3.0)], 1, 1);
        let mut net = LaneNetwork::default();
        net.rebuild(&roads);
        let merge = net.intersections.iter().find(|i| roads.node(i.node).pos.distance(Vec2::new(-20.0, 0.0)) < 1.0).expect("merge junction");
        assert_eq!(merge.approaches, 3);
        assert!(!merge.signalized, "a one-way ramp yields instead of getting a signal");
        let t = net.intersections.iter().find(|i| roads.node(i.node).pos.distance(Vec2::new(150.0, 0.0)) < 1.0).expect("T junction");
        assert!(t.signalized, "three two-way roads get signals");

        // A curved road is a chain of short straight pieces; its bends run at road speed.
        let curve = crate::plan::sample_curve(&[Vec2::new(0.0, -300.0), Vec2::new(60.0, -200.0), Vec2::new(0.0, -100.0)]);
        assert!(curve.len() > 10);
        let mut roads2 = RoadNetwork::default();
        roads2.add_polyline(&curve, 1, 1);
        let mut net2 = LaneNetwork::default();
        net2.rebuild(&roads2);
        // Only the bends: dead ends keep their slow U-turn connectors.
        let bend_connectors: Vec<&Lane> = net2.lanes.iter().filter(|l| matches!(l.kind, LaneKind::Connector { node, .. } if roads2.node(node).segments.len() == 2)).collect();
        assert!(!bend_connectors.is_empty());
        assert!(bend_connectors.iter().all(|l| l.speed_limit >= ROAD_SPEED - 0.01), "bends do not slow traffic to junction speed");
    }

    #[test]
    fn a_one_way_exit_gets_no_green_phase() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
        roads.add_polyline(&[Vec2::new(0.0, -200.0), Vec2::new(0.0, 0.0)], 1, 1);
        // The north arm leaves the junction one way: nothing ever arrives from it.
        roads.add_polyline(&[Vec2::new(0.0, 0.0), Vec2::new(0.0, 200.0)], 1, 0);
        let mut net = LaneNetwork::default();
        net.rebuild(&roads);
        let centre = net.intersections.iter().position(|i| roads.node(i.node).pos.distance(Vec2::ZERO) < 1.0).expect("centre");
        let inter = &net.intersections[centre];
        assert_eq!(inter.approaches, 4);
        assert!(inter.signalized);
        assert_eq!(inter.phases.len(), 2, "east and west share a phase, south has its own: {:?}", inter.phases);
        // Over a whole cycle every incoming approach gets a green, and at
        // no time is the junction red for everyone during a green window.
        let cycle = GREEN_SECONDS + CLEARANCE_SECONDS;
        let mut greens_seen = std::collections::HashSet::new();
        for step in 0..(2.0 * cycle) as usize {
            net.update_signals(1.0);
            let inter = &net.intersections[centre];
            if inter.timer < GREEN_SECONDS - 1.0 {
                let green: Vec<usize> = inter.connectors.iter().filter(|(l, _)| net.lanes[*l as usize].green).map(|(_, ap)| *ap).collect();
                assert!(!green.is_empty(), "nobody has the green at {step}s");
                greens_seen.extend(green);
            }
        }
        assert_eq!(greens_seen.len(), 3);
        // The cycle is two phases long, not four.
        let inter = &net.intersections[centre];
        assert_eq!(inter.phase, 0, "back at the first phase after two greens");
    }

    #[test]
    fn the_lookahead_sees_a_bus_as_long_as_it_is() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-200.0, 0.0), Vec2::new(0.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let west = roads.nearest_street(Vec2::new(-100.0, 0.0)).unwrap().0;
        let east = roads.nearest_street(Vec2::new(100.0, 0.0)).unwrap().0;
        // Both points on the eastbound (right-hand, +y) side.
        let (route, s0, s1) = t.net.find_route(west, Vec2::new(-30.0, 2.0), east, Vec2::new(150.0, 2.0)).expect("route");
        assert_eq!(route.len(), 3, "lane, bend connector, lane");
        // A bus standing just past the start of the far lane.
        let bus_route = vec![route[2]];
        let bus = t.spawn_car(CitizenId::default(), None, bus_route, 6.0, 150.0, 0);
        t.cars[bus].bus = Some(Bus { route: crate::transit::RouteId::default(), next_stop: 0, passengers: Vec::new(), to_depot: false, depot: BuildingId::default() });
        let car = t.spawn_car(CitizenId::default(), None, route.clone(), s0, s1, 0);
        let lane = &t.net.lanes[route[0] as usize];
        let (gap, lead_v, _) = t.lookahead(&t.cars[car], lane);
        let expected = (lane.length - s0) + t.net.lanes[route[1] as usize].length + 6.0 - BUS_LENGTH;
        assert!((gap - expected).abs() < 1e-3, "gap {gap} vs {expected}: the bus is {BUS_LENGTH} m, not a car");
        assert_eq!(lead_v, 0.0);
    }

    #[test]
    fn a_stalled_bus_is_replanned_instead_of_teleported() {
        let roads = cross();
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let west = roads.nearest_street(Vec2::new(-150.0, 0.0)).unwrap().0;
        let north = roads.nearest_street(Vec2::new(0.0, 150.0)).unwrap().0;
        let (route, s0, s1) = t.net.find_route(west, Vec2::new(-150.0, -2.0), north, Vec2::new(2.0, 150.0)).expect("route");
        let goal = *route.last().unwrap();
        let bus = t.spawn_car(CitizenId::default(), None, route.clone(), s0, s1, 0);
        t.cars[bus].bus = Some(Bus { route: crate::transit::RouteId::default(), next_stop: 1, passengers: Vec::new(), to_depot: false, depot: BuildingId::default() });
        let car = t.spawn_car(CitizenId::default(), None, route, s0 + 20.0, s1, 0);
        // Both have been out far too long; neither has moved.
        t.cars[bus].age = TRIP_TIMEOUT + 1.0;
        t.cars[car].age = TRIP_TIMEOUT + 1.0;
        let arrived = t.step(1.0, 1);
        assert!(arrived.contains(&car), "a private car gives up where it stands");
        assert!(!arrived.contains(&bus), "the bus does not");
        let b = &t.cars[bus];
        assert!(b.age < 2.0, "its clock restarted");
        assert_eq!(b.idx, 0);
        assert_eq!(*b.route.last().unwrap(), goal, "still bound for the same stop");
        assert!((b.s_end - s1).abs() < 1e-4);
        assert!(t.net.lanes[b.route[0] as usize].cars.contains(&bus), "and still on its lane");
        // A bus whose stop is behind it on its own lane cannot be replanned
        // (lanes are one way) and times out like a car.
        let east = roads.nearest_street(Vec2::new(150.0, 0.0)).unwrap().0;
        let (lane, s, _) = t.net.nearest_lane_on(east, Vec2::new(150.0, 2.0)).unwrap();
        let stuck = t.spawn_car(CitizenId::default(), None, vec![lane], s, s - 20.0, 0);
        t.cars[stuck].bus = Some(Bus { route: crate::transit::RouteId::default(), next_stop: 1, passengers: Vec::new(), to_depot: false, depot: BuildingId::default() });
        t.cars[stuck].age = TRIP_TIMEOUT + 1.0;
        assert!(t.step(1.0, 1).contains(&stuck));
    }

    #[test]
    fn paths_carry_no_car_lanes_and_do_not_make_junctions() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        let path = roads.add_polyline_kind(&[Vec2::new(0.0, 150.0), Vec2::new(0.0, 3.0)], 0, 0, crate::roads::RoadKind::Path);
        let mut net = LaneNetwork::default();
        net.rebuild(&roads);
        assert!(path.iter().all(|s| !net.segment_lanes.contains_key(s)), "paths have no lanes");
        let node = roads.nodes.iter().position(|n| n.pos.distance(Vec2::new(0.0, 0.0)) < 0.5).unwrap();
        let inter = net.intersections.iter().find(|i| i.node.0 as usize == node).unwrap();
        assert_eq!(inter.approaches, 2, "a path joining a street is not a junction for cars");
        assert!(!inter.signalized);
        assert!(roads.path(crate::roads::NodeId(node as u32), roads.segment(path[0]).a).is_some(), "walkers can use the path");
    }

    /// A two-by-two lane avenue east-west crossing a one-lane street.
    fn avenue_cross() -> RoadNetwork {
        let mut net = RoadNetwork::default();
        net.add_polyline(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 2, 2);
        net.add_polyline(&[Vec2::new(0.0, -400.0), Vec2::new(0.0, 400.0)], 1, 1);
        net
    }

    /// The road lane of `segment` running along `dir` with this index.
    fn lane_of(t: &Traffic, segment: SegmentId, dir: Vec2, index: u8) -> LaneId {
        *t.net.segment_lanes[&segment]
            .iter()
            .find(|&&l| {
                let lane = &t.net.lanes[l as usize];
                matches!(lane.kind, LaneKind::Road { index: i, .. } if i == index) && (lane.end() - lane.start()).normalize().dot(dir) > 0.9
            })
            .expect("lane")
    }

    #[test]
    fn lane_changes_wait_for_a_safe_gap() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 2, 2);
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let seg = roads.nearest_street(Vec2::ZERO).unwrap().0;
        let (left, right) = (lane_of(&t, seg, Vec2::X, 0), lane_of(&t, seg, Vec2::X, 1));
        assert_eq!(t.net.lanes[right as usize].left, Some(left));
        assert_eq!(t.net.lanes[left as usize].right, Some(right));
        let car = t.spawn_car(CitizenId::default(), None, vec![right, left], 100.0, 200.0, 0);
        t.cars[car].v = 10.0;
        let other = t.spawn_car(CitizenId::default(), None, vec![left], 100.0, 250.0, 0);
        let mut check = |s: f32, v: f32| {
            t.net.lanes[left as usize].cars.retain(|&c| c != other);
            t.cars[other].s = s;
            t.cars[other].v = v;
            t.insert_sorted(left, other);
            t.change_ok(car, left, LC_SAFE_BRAKE).is_some()
        };
        assert!(!check(100.0, 10.0), "alongside: no room");
        assert!(!check(88.0, 13.0), "a faster car 12 m behind would have to brake hard");
        assert!(check(30.0, 13.0), "70 m behind it has time");
        assert!(!check(108.0, 0.0), "a stopped car 3.5 m ahead: the changer would have to brake hard");
        assert!(check(160.0, 10.0), "a leader well ahead at the same speed is fine");
        // With a safe gap the change happens on the next step and the car
        // drifts across instead of jumping.
        check(250.0 - 1.0, 10.0);
        let p0 = t.cars[car].pos;
        t.step(0.5, 1);
        let c = &t.cars[car];
        assert_eq!(c.route[c.idx], left, "changed lanes");
        assert!(t.net.lanes[left as usize].cars.contains(&car) && !t.net.lanes[right as usize].cars.contains(&car));
        assert!(c.lateral > 2.0, "still drawn near the old lane: {}", c.lateral);
        assert!(c.pos.distance(p0) < 10.0 * 0.5 + 1.0, "no sideways jump");
    }

    /// Drive one car on its own until it arrives; returns the lanes it used
    /// and the largest lateral offset seen.
    fn drive(t: &mut Traffic, car: CarId) -> (Vec<LaneId>, f32, bool) {
        let mut used = vec![t.cars[car].route[0]];
        let mut lateral: f32 = 0.0;
        for _ in 0..600 {
            let arrived = t.step(1.0, 4);
            let c = &t.cars[car];
            lateral = lateral.max(c.lateral.abs());
            assert!(c.pos.distance(c.prev_pos) < ROAD_SPEED * 1.0 + 1.0, "positions move smoothly");
            if used.last() != Some(&c.route[c.idx]) {
                used.push(c.route[c.idx]);
            }
            if arrived.contains(&car) {
                return (used, lateral, c.age < TRIP_TIMEOUT);
            }
        }
        (used, lateral, false)
    }

    #[test]
    fn cars_move_to_the_turn_lane_their_turn_needs() {
        let roads = avenue_cross();
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let w = LANE_WIDTH;
        let west = roads.nearest_street(Vec2::new(-200.0, 0.0)).unwrap().0;
        let (inner, outer) = (lane_of(&t, west, Vec2::X, 0), lane_of(&t, west, Vec2::X, 1));
        // Lane discipline at the junction: the outer (right) lane never
        // turns left, the inner (left) lane never turns right; both go on.
        let turns = |l: LaneId| t.net.lanes[l as usize].next.iter().filter(|&&c| t.net.lanes[c as usize].permitted).map(|&c| t.net.lanes[c as usize].turn).collect::<Vec<i8>>();
        // The default two-lane approach: a left-only lane and a straight-and-right lane.
        assert_eq!(t.net.lanes[inner as usize].arrows, LEFT);
        assert_eq!(t.net.lanes[outer as usize].arrows, STRAIGHT | RIGHT);
        assert!(!turns(outer).contains(&-1) && turns(outer).contains(&1) && turns(outer).contains(&0), "{:?}", turns(outer));
        assert_eq!(turns(inner), vec![-1], "the inner lane only turns left");

        // A left turn (east to south: -y is the driver's left here) from the outer lane.
        let south = roads.nearest_street(Vec2::new(0.0, -200.0)).unwrap().0;
        let (route, s0, s1) = t.net.find_route(west, Vec2::new(-300.0, 1.5 * w), south, Vec2::new(0.5 * w, -150.0)).expect("route");
        assert_eq!(route[0], outer);
        assert!(route.windows(2).any(|p| p == [outer, inner]), "the route moves to the inner lane: {route:?}");
        let car = t.spawn_car(CitizenId::default(), None, route, s0, s1, 0);
        let (used, lateral, arrived) = drive(&mut t, car);
        assert!(arrived, "arrived before timing out: {used:?}");
        let conn = used.iter().position(|&l| t.net.lanes[l as usize].is_connector() && t.net.lanes[l as usize].turn != 0).expect("turned");
        assert_eq!(t.net.lanes[used[conn] as usize].turn, -1, "a left turn");
        assert_eq!(used[conn - 1], inner, "taken from the inner lane");
        assert!(lateral > 2.0, "drifted across visibly ({lateral})");
        t.remove_car(car);

        // A right turn (east to north) starting in the inner lane.
        let north = roads.nearest_street(Vec2::new(0.0, 200.0)).unwrap().0;
        let (route, s0, s1) = t.net.find_route(west, Vec2::new(-300.0, 0.5 * w), north, Vec2::new(-0.5 * w, 150.0)).expect("route");
        assert_eq!(route[0], inner);
        let car = t.spawn_car(CitizenId::default(), None, route, s0, s1, 0);
        let (used, _, arrived) = drive(&mut t, car);
        assert!(arrived);
        let conn = used.iter().position(|&l| t.net.lanes[l as usize].is_connector() && t.net.lanes[l as usize].turn != 0).expect("turned");
        assert_eq!(t.net.lanes[used[conn] as usize].turn, 1, "a right turn");
        assert_eq!(used[conn - 1], outer, "taken from the outer lane");
        assert!(t.cars[car].lateral.abs() < 0.05, "settled in its lane");
    }

    /// Load a four-way signal from every arm with mixed turns for ten
    /// minutes. Returns (spawned, arrived in time, arrival order).
    fn busy_junction() -> (usize, usize, Vec<CarId>) {
        let roads = avenue_cross();
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let w = LANE_WIDTH;
        // (start point on an inbound lane, end point on the outbound lane) per arm.
        let arms = [
            (Vec2::new(-350.0, 1.5 * w), Vec2::new(-300.0, -1.5 * w)),
            (Vec2::new(350.0, -1.5 * w), Vec2::new(300.0, 1.5 * w)),
            (Vec2::new(-0.5 * w, -350.0), Vec2::new(0.5 * w, -300.0)),
            (Vec2::new(0.5 * w, 350.0), Vec2::new(-0.5 * w, 300.0)),
        ];
        let inner = [Vec2::new(0.0, -w), Vec2::new(0.0, w), Vec2::ZERO, Vec2::ZERO];
        let mut spawned = 0;
        let mut on_time = 0;
        let mut order = Vec::new();
        let mut max_stall: f32 = 0.0;
        for tick in 0..2400 {
            if tick < 300 && tick % 4 == 0 {
                for (a, &(from, _)) in arms.iter().enumerate() {
                    let b = (a + 1 + (tick / 4 + a) % 3) % 4;
                    let from = if (tick / 4) % 2 == 0 { from } else { from + inner[a] };
                    let to = arms[b].1;
                    let (fs, _, _) = roads.nearest_street(from).unwrap();
                    let (ts, _, _) = roads.nearest_street(to).unwrap();
                    let (route, s0, s1) = t.net.find_route(fs, from, ts, to).expect("route");
                    t.spawn_car(CitizenId::default(), None, route, s0, s1, 0);
                    spawned += 1;
                }
            }
            for id in t.step(2.0, 4) {
                if t.cars[id].age < TRIP_TIMEOUT {
                    on_time += 1;
                }
                order.push(id);
                t.remove_car(id);
            }
            assert!(t.gridlocks().is_empty(), "gridlock at {}s", tick * 2);
            max_stall = t.cars.values().filter(|c| !c.entering).map(|c| c.stalled).fold(max_stall, f32::max);
            if tick > 300 && t.cars.is_empty() {
                break;
            }
        }
        assert!(max_stall < 180.0, "nobody in traffic stands still for minutes ({max_stall:.0}s)");
        (spawned, on_time, order)
    }

    #[test]
    fn a_busy_four_way_signal_never_deadlocks() {
        let (spawned, on_time, order) = busy_junction();
        assert_eq!(spawned, 300);
        assert_eq!(on_time, spawned, "every car arrives, none times out");
        // Deterministic: the same arrivals in the same order.
        let (_, _, again) = busy_junction();
        assert_eq!(order, again);
    }

    #[test]
    fn a_gridlock_ring_is_found_and_broken() {
        // A two-by-two grid of blocks; fill the four lanes round the
        // north-east block with cars that each want the next lane round.
        let mut roads = RoadNetwork::default();
        for c in [-100.0, 0.0, 100.0] {
            roads.add_polyline(&[Vec2::new(-100.0, c), Vec2::new(100.0, c)], 1, 1);
            roads.add_polyline(&[Vec2::new(c, -100.0), Vec2::new(c, 100.0)], 1, 1);
        }
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let corners = [Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(100.0, 100.0), Vec2::new(0.0, 100.0)];
        let ring: Vec<LaneId> = (0..4)
            .map(|i| {
                let (a, b) = (corners[i], corners[(i + 1) % 4]);
                let seg = roads.nearest_street((a + b) * 0.5).unwrap().0;
                lane_of(&t, seg, (b - a).normalize(), 0)
            })
            .collect();
        let conn = |a: LaneId, b: LaneId| *t.net.lanes[a as usize].next.iter().find(|&&c| t.net.lanes[c as usize].next[0] == b).expect("connector");
        let mut placed: Vec<(Vec<LaneId>, f32, f32)> = Vec::new();
        for i in 0..4 {
            let (l0, l1, l2) = (ring[i], ring[(i + 1) % 4], ring[(i + 2) % 4]);
            let route = vec![l0, conn(l0, l1), l1, conn(l1, l2), l2];
            let len = t.net.lanes[l0 as usize].length;
            let n = ((len - 6.5) / 6.5).floor() as usize + 1;
            let step = (len - 6.5) / (n - 1) as f32;
            for k in 0..n {
                placed.push((route.clone(), 5.0 + k as f32 * step, t.net.lanes[l2 as usize].length * 0.5));
            }
        }
        let cars: Vec<CarId> = placed.into_iter().map(|(route, s, s_end)| t.spawn_car(CitizenId::default(), None, route, s, s_end, 0)).collect();
        assert!(cars.iter().all(|&c| !t.cars[c].entering), "all placed on the road");
        let locks = t.gridlocks();
        assert_eq!(locks.len(), 1, "one ring");
        assert_eq!(locks[0].junctions.len(), 4);
        assert_eq!(locks[0].vehicles, cars.len());
        let mut on_time = 0;
        for _ in 0..1200 {
            for id in t.step(2.0, 4) {
                on_time += (t.cars[id].age < TRIP_TIMEOUT) as usize;
                t.remove_car(id);
            }
            if t.cars.is_empty() {
                break;
            }
        }
        assert_eq!(on_time, cars.len(), "the ring was broken by drivers taking another way, nobody timed out");
    }

    /// One-lane streets crossing at a signal, long enough for queues.
    fn street_cross() -> RoadNetwork {
        let mut net = RoadNetwork::default();
        net.add_polyline(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        net.add_polyline(&[Vec2::new(0.0, -400.0), Vec2::new(0.0, 400.0)], 1, 1);
        net
    }

    /// Spawn a car between two points (each on the lane it should use).
    fn send(t: &mut Traffic, roads: &RoadNetwork, from: Vec2, to: Vec2) -> CarId {
        let (fs, _, _) = roads.nearest_street(from).unwrap();
        let (ts, _, _) = roads.nearest_street(to).unwrap();
        let (route, s0, s1) = t.net.find_route(fs, from, ts, to).expect("route");
        t.spawn_car(CitizenId::default(), None, route, s0, s1, 0)
    }

    /// Forty seconds of the westbound stream (and cross traffic) first, so
    /// the oncoming lane is busy when the left turner arrives.
    fn warm_up(t: &mut Traffic, roads: &RoadNetwork, cross: bool) {
        let w = LANE_WIDTH;
        for tick in 0..40 {
            if tick % 2 == 0 {
                send(t, roads, Vec2::new(300.0, -0.5 * w), Vec2::new(-300.0, -0.5 * w));
            }
            if cross && tick % 6 == 0 {
                send(t, roads, Vec2::new(-0.5 * w, -300.0), Vec2::new(-0.5 * w, 300.0));
            }
            for id in t.step(1.0, 4) {
                t.remove_car(id);
            }
        }
    }

    /// An eastbound left turner waits for a gap in a constant westbound
    /// stream; the straight-on car behind it pulls onto the shoulder,
    /// passes and merges back. Returns a trace for the determinism check.
    fn pass_scene() -> Vec<(f32, f32)> {
        let roads = street_cross();
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let w = LANE_WIDTH;
        let east_in = roads.nearest_street(Vec2::new(-200.0, 0.0)).unwrap().0;
        let lane = lane_of(&t, east_in, Vec2::X, 0);
        let room = Traffic::pass_offset(&t.net.lanes[lane as usize]).expect("a shoulder to pass on");
        assert!(room + CAR_WIDTH * 0.5 <= LANE_WIDTH * 0.5 + SHOULDER, "stays off the kerb");
        warm_up(&mut t, &roads, false);
        let turner = send(&mut t, &roads, Vec2::new(-60.0, 0.5 * w), Vec2::new(0.5 * w, -150.0));
        let follower = send(&mut t, &roads, Vec2::new(-100.0, 0.5 * w), Vec2::new(150.0, 0.5 * w));
        let mut trace = Vec::new();
        let (mut passed_while_waiting, mut max_lateral, mut was_passing): (bool, f32, bool) = (false, 0.0, false);
        for tick in 0..300 {
            // The oncoming stream that keeps the turner waiting.
            if tick % 2 == 0 {
                send(&mut t, &roads, Vec2::new(300.0, -0.5 * w), Vec2::new(-300.0, -0.5 * w));
            }
            for id in t.step(1.0, 4) {
                if id != follower && id != turner {
                    t.remove_car(id);
                }
            }
            let Some(f) = t.cars.get(follower) else { break };
            was_passing |= f.passing;
            max_lateral = max_lateral.max(f.lateral);
            let on_lane = f.route[f.idx] == lane;
            if on_lane && f.v < 0.5 && f.passing {
                assert!(f.s <= t.net.lanes[lane as usize].length - CROSSING_SETBACK + 0.5, "waits short of the crossing ({:.1})", f.s);
            }
            let tc = &t.cars[turner];
            if !on_lane && f.idx > 0 && tc.route[tc.idx] == lane {
                passed_while_waiting = true;
            }
            trace.push((f.pos.x, f.pos.y));
            if f.idx >= 2 && f.lateral.abs() < 0.05 {
                break;
            }
        }
        assert!(was_passing, "the follower pulled out to pass");
        assert!(max_lateral >= CAR_WIDTH && max_lateral <= room + 0.01, "onto the shoulder, not the kerb: {max_lateral:.2}");
        assert!(passed_while_waiting, "it got by while the turner was still waiting");
        let f = &t.cars[follower];
        assert!(f.idx >= 2 && f.lateral.abs() < 0.05, "and merged back into its lane");
        trace
    }

    #[test]
    fn a_car_passes_a_waiting_left_turner_on_the_right() {
        let a = pass_scene();
        assert_eq!(a, pass_scene(), "deterministic");
    }

    #[test]
    fn lane_arrows_default_from_demand_and_can_be_set() {
        let roads = avenue_cross();
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let w = LANE_WIDTH;
        let west = roads.nearest_street(Vec2::new(-200.0, 0.0)).unwrap().0;
        let (inner, outer) = (lane_of(&t, west, Vec2::X, 0), lane_of(&t, west, Vec2::X, 1));
        let node = t.net.lanes[outer as usize].end_node;
        // The shared straight-and-right lane takes both movements without a change.
        for to in [Vec2::new(300.0, 1.5 * w), Vec2::new(-0.5 * w, 150.0)] {
            let (route, _, _) = t.net.find_route(west, Vec2::new(-300.0, 1.5 * w), roads.nearest_street(to).unwrap().0, to).unwrap();
            assert_eq!(route[0], outer);
            assert!(!t.net.lanes[outer as usize].is_beside(route[1]), "no lane change from the straight-and-right lane: {route:?}");
        }
        // Straight on from the left-only lane moves over first.
        let (route, _, _) = t.net.find_route(west, Vec2::new(-300.0, 0.5 * w), roads.nearest_street(Vec2::new(300.0, 0.0)).unwrap().0, Vec2::new(300.0, 1.5 * w)).unwrap();
        assert_eq!(&route[..2], &[inner, outer]);

        // The player makes it left-and-straight plus right only.
        assert!(t.set_lane_use(west, node, Some(vec![LEFT, LEFT])).is_err(), "nothing would go straight or right");
        assert!(t.set_lane_use(west, node, Some(vec![LEFT])).is_err(), "two lanes need two arrows");
        t.set_lane_use(west, node, Some(vec![LEFT | STRAIGHT, RIGHT])).unwrap();
        let permitted = |l: LaneId| t.net.lanes[l as usize].next.iter().filter(|&&c| t.net.lanes[c as usize].permitted).map(|&c| t.net.lanes[c as usize].turn).collect::<Vec<i8>>();
        assert_eq!(permitted(outer), vec![1]);
        assert!(permitted(inner).contains(&0) && permitted(inner).contains(&-1));
        // It survives a rebuild of the network and can be cleared.
        t.rebuild(&roads);
        assert_eq!(t.net.lanes[inner as usize].arrows, LEFT | STRAIGHT);
        t.set_lane_use(west, node, None).unwrap();
        assert_eq!(t.net.lanes[inner as usize].arrows, LEFT);

        // Demand: a quiet left shares its lane with straight traffic, a
        // busy one on a wide avenue gets two lanes.
        let all = LEFT | STRAIGHT | RIGHT;
        assert_eq!(default_arrows(2, all, Some(0.1), None), vec![LEFT | STRAIGHT, STRAIGHT | RIGHT]);
        assert_eq!(default_arrows(2, all, Some(0.5), None), vec![LEFT, STRAIGHT | RIGHT]);
        assert_eq!(default_arrows(4, all, Some(0.5), None), vec![LEFT, LEFT, STRAIGHT, STRAIGHT | RIGHT]);
        assert_eq!(default_arrows(4, all, Some(0.1), None), vec![LEFT, STRAIGHT, STRAIGHT, STRAIGHT | RIGHT]);
        assert_eq!(default_arrows(2, LEFT | RIGHT, None, None), vec![LEFT, RIGHT], "a T junction");
        // Counted traffic revises the default.
        let ai = t.net.lanes[outer as usize].approach as usize;
        t.net.approaches[ai].counts = [2.0, 40.0, 8.0];
        t.adapt_arrows();
        assert_eq!(t.net.lanes[inner as usize].arrows, LEFT | STRAIGHT, "few lefts: the left lane is shared");
    }

    #[test]
    fn a_protected_left_arrow_ends_starvation() {
        let roads = street_cross();
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let w = LANE_WIDTH;
        warm_up(&mut t, &roads, false);
        let turner = send(&mut t, &roads, Vec2::new(-40.0, 0.5 * w), Vec2::new(0.5 * w, -150.0));
        let node = t.net.lanes[t.cars[turner].route[0] as usize].end_node;
        let inter = t.net.node_intersection[node.0 as usize];
        let mut arrows_seen = 0;
        let mut turned_at = None;
        for tick in 0..600 {
            // Constant oncoming traffic and nobody on the cross street: the
            // green never has to end, the case that used to starve lefts.
            if tick % 2 == 0 {
                send(&mut t, &roads, Vec2::new(300.0, -0.5 * w), Vec2::new(-300.0, -0.5 * w));
            }
            for id in t.step(1.0, 4) {
                if id != turner {
                    t.remove_car(id);
                }
            }
            let i = &t.net.intersections[inter];
            if i.arrow > 0.0 && i.timer < i.arrow {
                arrows_seen += 1;
            }
            let c = &t.cars[turner];
            if t.net.lanes[c.route[c.idx] as usize].is_connector() && turned_at.is_none() {
                turned_at = Some(tick);
                break;
            }
        }
        let turned_at = turned_at.expect("the left turner got through");
        assert!(arrows_seen > 0, "on a protected arrow");
        assert!(turned_at < 60, "soon after starving, not at the trip timeout ({turned_at}s)");
        let (mean, max, n) = t.left_wait_stats();
        assert_eq!(n, 1);
        assert!(mean > 0.0 && max == mean);
        assert!(t.net.intersections[inter].left_turns == 1 && t.net.intersections[inter].left_wait_max > 0.0);
    }
}
