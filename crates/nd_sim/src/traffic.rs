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

mod arrows;
mod build;
mod crash;
mod crosswalk;
mod driver;
mod follow;
mod gridlock;
mod hazard;
mod junction;
mod kerb;
mod lane;
mod lane_change;
mod place;
mod ramps;
mod remap;
mod roundabout;
mod routing;
mod scene;
mod signals;
mod siren;
mod stunt;
mod substep;
mod tolls;
mod vehicle;
mod why;

pub use crosswalk::{Hold, PedTraffic, PED_CLEARANCE};
pub use remap::CarryReport;
pub use roundabout::{RingLanes, RingNode, RING_CRITICAL_GAP, RING_SPEED};
pub use scene::SceneZone;
pub use siren::{pulls_over, OffLaw, OffLawMove, PURSUIT_SPEED_FACTOR};
pub use why::{JamChain, StopReason};
pub use arrows::*;
pub use build::*;
pub use driver::*;
use follow::idm_with;
pub use gridlock::*;
pub use lane::*;
pub use signals::*;
use stunt::step_stunt;
pub use vehicle::*;

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
/// Deceleration (m/s²) a driver plans with to meet a lower speed limit
/// ahead (a ramp's eased limit, a turn), braking early instead of at the
/// line.
const ANTICIPATE_BRAKE: f32 = 1.5;
/// How far ahead (m) a driver reads the limits of the lanes on its route.
const ANTICIPATE_REACH: f32 = 220.0;
/// Hardest deceleration (m/s²) the free-road term of the car-following
/// model asks for when a car is over its desired speed. Plain IDM gives
/// `a (1 - (v/v0)^4)`, which for a car at 25 m/s entering a 6 m/s lane is
/// -250 m/s²: it stopped dead in one step and pulled away again (the
/// stop-go "jitter" on ramps). Drivers anticipate limits, so this only
/// trims what is left over.
const FREE_BRAKE: f32 = 3.0;
/// Sideways acceleration (m/s²) drivers accept on a ramp's bends: what
/// caps a curved ramp's eased limit (`v = sqrt(a r)`).
const RAMP_LATERAL: f32 = 3.0;
const IDM_ACCEL: f32 = 1.4;
const IDM_BRAKE: f32 = 2.0;
const IDM_MIN_GAP: f32 = 2.0;
/// Metres a car would have to be shoved back past the start of its lane
/// beyond which it stands on the car ahead (a pile), not just close behind.
const PILE_OVERLAP: f32 = 2.5;
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
/// [rail-crossing] Hardest a driver brakes to stop for a level crossing's
/// lights; closer than that they go on over.
const RAIL_STOP_BRAKE: f32 = 4.5;
/// [rail-crossing] How far short of the end of its lane a vehicle's front
/// stops for a level crossing: its body (drawn about its front point) stays
/// behind the painted stop line.
const RAIL_STOP_SHORT: f32 = 3.5;
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
/// Extra routing cost of a lane change on a piece of lane with less than
/// `SHORT_CHANGE_LEN` before the solid line of the junction ahead (planned
/// as long as it can be, `SOLID_LINE_MAX`): drivers pick their lane
/// well before the junction, and a change planned into the last metres is
/// the one that is never made.
const SHORT_CHANGE_COST: f32 = 60.0;
const SHORT_CHANGE_LEN: f32 = 40.0;
/// Routing cost of a lane change planned on a piece that lies wholly in
/// the solid-line zone before a junction: possible only as a last resort.
const SOLID_CHANGE_COST: f32 = 300.0;
/// The solid line before a junction (or a roundabout entry): no lane
/// changes in the last `SOLID_LINE_MIN` metres, more at speed (as far as a
/// car covers in `SOLID_LINE_SECONDS`, up to `SOLID_LINE_MAX`), nor inside
/// the junction. A car not yet in the lane its turn needs by then carries
/// on in its own lane and finds another way (it misses the turn).
const SOLID_LINE_MIN: f32 = 20.0;
const SOLID_LINE_SECONDS: f32 = 2.0;
const SOLID_LINE_MAX: f32 = 32.0;
/// A lane change glides over this many seconds of driving (at least
/// `GLIDE_MIN` metres, at most `GLIDE_MAX`).
const GLIDE_SECONDS: f32 = 3.0;
const GLIDE_MIN: f32 = 8.0;
const GLIDE_MAX: f32 = 45.0;
/// Metres a second a stopped car mid-change still creeps across by.
const GLIDE_CREEP: f32 = 0.8;
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
/// Seconds two cars stand side by side, each wanting the other's lane,
/// before they swap places (`Traffic::swap_lanes`).
const SWAP_AFTER: f32 = 8.0;
/// Seconds before a car with no other way than the lane change it could
/// not make looks for one again.
const SOLID_RETRY: f32 = 2.0;
/// A vehicle at the kerb whose spot stays covered by standing traffic this
/// long pulls out into the nearest real gap behind it instead.
const KERB_SLIDE_AFTER: f32 = 90.0;
const KERB_SLIDE_REACH: f32 = 80.0;
/// A stalled car tries another way at most this often, doubling the wait
/// after every try that leaves it standing (from `REROUTE_LANE_CHANGE_AFTER`).
const REROUTE_BACKOFF_MAX: f32 = 320.0;
/// Seconds standing still before a vehicle counts in the wait-for graph.
const WAIT_GRAPH_STALL: f32 = 60.0;
/// Seconds a car stands at the end of a lane leading off the map, unable to
/// make the last lane change of its route, before it leaves in its own lane.
const DEAD_END_GIVE_UP: f32 = 5.0;
/// Trucks cruise a little below the limit, which is what gets them overtaken.
const TRUCK_SPEED_FACTOR: f32 = 0.85;
/// [services] Emergency vehicles drive this much over the limit; cars
/// pulled over for one stand this far to the right (`traffic/siren.rs`).
const SIREN_SPEED_FACTOR: f32 = 1.25;
const SIREN_PULL_ASIDE: f32 = 1.6;
/// [carlife] A vehicle this long on one route (traffic seconds) looks for
/// another way from where it stands; it is never taken off the road.
const REPLAN_AFTER: f32 = 1800.0;

/// Angle `a` to `b` by `t`, the short way round.
pub use crate::geom::lerp_angle;

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
    /// [driving] Crashes decided since the world last drained them.
    #[serde(skip)]
    pub crashes: Vec<CrashReport>,
    #[serde(default)]
    pub next_crash: u32,
    /// [driving] The traffic model's random stream (crashes only).
    #[serde(default)]
    pub rng: u64,
    /// [driving] Red lights run (counted by the world per day).
    #[serde(default)]
    pub red_runs: u32,
    /// [pedestrians] Walk lights, crossings in use and who drove over one
    /// (`traffic/crosswalk.rs`).
    #[serde(skip)]
    pub peds: PedTraffic,
    /// [tolls] Vehicles that drove through a toll booth since the world
    /// last drained them.
    #[serde(skip)]
    pub toll_passes: Vec<TollPass>,
    /// [toll-inspect] Vehicles set off this tick while some road is tolled:
    /// the world checks whether their route went round a booth. With where
    /// along its first lane each set off.
    #[serde(skip)]
    pub toll_checks: Vec<(CarId, f32)>,
    /// [road-inspect] Street pieces (segment index) vehicles drove onto this
    /// tick, one entry a vehicle; the world drains them into the hourly
    /// counts (`crate::road_traffic`).
    #[serde(skip)]
    pub segment_entries: Vec<u32>,
    /// [tools] The player's junction settings (signals, phase greens, the
    /// left arrow, banned left turns), reapplied on every rebuild by
    /// position (`traffic/junction.rs`).
    #[serde(default)]
    pub junction_rules: Vec<crate::junctions::JunctionRule>,
    /// [scene] What stands at the roadside this tick for traffic to slow
    /// for and steer round (fire engines at a fire), set by the world
    /// before every step (`traffic/scene.rs`).
    #[serde(skip)]
    pub scenes: Vec<SceneZone>,
}

impl Traffic {
    pub fn rebuild(&mut self, roads: &RoadNetwork) {
        self.net.rebuild(roads);
        // [tools] The player's junction settings first: a banned left
        // takes the movement away before the arrows are fitted.
        self.apply_junction_rules(roads);
        // Player arrows go back on their approaches by geometry
        // (`traffic/remap.rs`): ids are renumbered by every rebuild.
        self.resolve_lane_use(roads);
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
        let ids: Vec<CarId> = self.cars.keys().filter(|&id| !self.cars[id].entering && !self.cars[id].in_stunt()).collect();
        for id in ids {
            let lane = self.cars[id].route[self.cars[id].idx];
            self.insert_sorted(lane, id);
        }
        dropped
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

    /// Bookkeeping as a car moves from `from` onto `to`: turning counts for
    /// the approach's arrows, and the wait before a left turn.
    fn entered(&mut self, from: LaneId, to: LaneId, id: CarId) {
        // [road-inspect] Counted onto the street piece it drives onto.
        if let LaneKind::Road { segment, .. } = self.net.lanes[to as usize].kind {
            self.segment_entries.push(segment.0);
        }
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

    /// Advance all cars by `seconds` in `substeps` steps. Returns cars that
    /// reached their destination (they stay in `cars` until removed).
    pub fn step(&mut self, seconds: f32, substeps: usize) -> Vec<CarId> {
        let dt = seconds / substeps.max(1) as f32;
        self.drop_dead_end_changes();
        for car in self.cars.values_mut() {
            car.prev_pos = car.pos;
            car.prev_heading = car.heading;
            car.age += seconds;
            car.drive.poses.clear();
            car.drive.poses.push((car.pos, car.heading));
        }
        let mut arrived = Vec::new();
        for _ in 0..substeps {
            let mut demand = self.signal_demand();
            self.ped_demand(&mut demand);
            self.net.step_signals(dt, Some(&demand));
            self.update_walk(&demand);
            for lane in self.net.lanes.iter_mut().filter(|l| l.is_connector()) {
                lane.red_for = if lane.green { 0.0 } else { lane.red_for + dt };
            }

            let claims = self.kerb_claims();
            // [emergency] Sirens and fugitives plan their way round.
            self.plan_offlaw(dt);
            // [scene] Fire engines at the kerb, tow trucks at work.
            let zones = self.scene_zones();
            let accs = self.plan_substep(dt, &claims, &zones);
            self.apply_steps(accs, dt);
            self.yield_to_sirens(dt);
            self.pull_out_waiting(dt);
            self.lane_changes();
            self.advance_lanes(&mut arrived);
            if self.net.tolls {
                self.toll_gates();
            }
            self.reorder_passers();
            self.offlaw_reorder();
            self.collisions();
            self.crosswalk_conflicts();
            self.hazards(dt);
            self.place_cars(dt);
        }
        self.reroute_stalled();
        self.adapt_clock += seconds;
        if self.adapt_clock >= ADAPT_SECONDS {
            self.adapt_clock = 0.0;
            self.adapt_arrows();
        }
        arrived
    }
}

#[cfg(test)]
mod tests;
