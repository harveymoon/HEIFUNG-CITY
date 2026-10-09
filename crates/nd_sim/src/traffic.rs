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
/// Trips older than this (traffic seconds) end where the car stands. A bus
/// is never ended this way (its riders would be teleported with it): it is
/// re-planned from where it stands instead.
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

            // Kerb positions where a car has waited long enough to be let in.
            // Each claim: where the kerb car's front will be, how long it is
            // and who it is. A spot near the start of a lane also holds up
            // traffic still on the lanes feeding it.
            let mut claims: HashMap<LaneId, Vec<(f32, f32, CarId)>> = HashMap::new();
            // A car stopped at the end of its lane for the lane change its
            // turn needs is let in the same way (the zip: the driver behind
            // in the next lane leaves it a gap as the queue moves up).
            // Without it a car in a slow queue waited for a gap that the
            // queue beside it never left, and blocked its own lane meanwhile.
            for (cid, c) in self.cars.iter().filter(|(_, c)| !c.entering && c.wait == Wait::LaneChange && c.stalled > COURTESY_AFTER) {
                if let Some(target) = c.change_pending(&self.net) {
                    claims.entry(target).or_default().push((c.s.min(self.net.lanes[target as usize].length), c.length(), cid));
                }
            }
            for (cid, c) in self.cars.iter().filter(|(_, c)| c.entering && c.stalled > COURTESY_AFTER) {
                let lane = c.route[c.idx];
                claims.entry(lane).or_default().push((c.s, c.length(), cid));
                if c.s < c.length() + IDM_MIN_GAP + 30.0 {
                    for &p in &self.net.lanes[lane as usize].prev {
                        claims.entry(p).or_default().push((c.s + self.net.lanes[p as usize].length, c.length(), cid));
                    }
                }
            }
            // [emergency] Sirens and fugitives plan their way round.
            self.plan_offlaw(dt);
            // [scene] Fire engines at the kerb, tow trucks at work.
            let zones = self.scene_zones();
            let mut accs: Vec<Step> = Vec::with_capacity(self.cars.len());
            for (lane_id, lane) in self.net.lanes.iter().enumerate() {
                let lane_claims = claims.get(&(lane_id as LaneId));
                for (k, &cid) in lane.cars.iter().enumerate() {
                    let car = &self.cars[cid];
                    if car.hold > 0.0 {
                        accs.push(Step { by: None, id: cid, acc: -1000.0, why: Wait::Dwell, left_wait: false, seen: 0.0, run: None, dodge: 0.0 });
                        continue;
                    }
                    if car.crashed() {
                        accs.push(Step { by: None, id: cid, acc: -1000.0, why: Wait::Crashed, left_wait: false, seen: 0.0, run: None, dodge: car.drive.dodge });
                        continue;
                    }
                    let pending = car.change_pending(&self.net).is_some();
                    // Passing a left turner: once out on the shoulder, the
                    // turner is no longer in the way.
                    let beside = car.passing
                        && Self::pass_offset(lane).is_some_and(|o| car.lateral >= 0.8 * o)
                        && lane.cars.get(k + 1).is_some_and(|&l| self.next_turn(&self.cars[l]) == Some(-1));
                    let lead = lane.cars.get(if beside { k + 2 } else { k + 1 });
                    let mut run = None;
                    let (mut gap, mut lead_v, mut why) = match lead {
                        Some(&lead) => (self.cars[lead].s - car.s - self.cars[lead].length(), self.cars[lead].v, Wait::Leader),
                        None if pending => (LOOKAHEAD * 2.0, ROAD_SPEED, Wait::Free),
                        None => {
                            let (g, v, w, r) = self.lookahead(car, lane);
                            run = r;
                            (g, v, w)
                        }
                    };
                    if car.passing && matches!(why, Wait::Signal | Wait::ExitFull | Wait::Yield) {
                        // On the shoulder: stop short of the crossing.
                        gap -= CROSSING_SETBACK;
                    }
                    // [emergency] Lights and sirens (and whoever flees them):
                    // past what their move takes them round, no red light.
                    if car.offlaw() || car.passing_hazard() {
                        (gap, lead_v, why) = self.offlaw_gap(car, gap, lead_v, why);
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
                    let mut by: Option<CarId> = None;
                    if let Some(cl) = lane_claims {
                        let ahead = lane.cars.get(k + 1).map_or(f32::MAX, |&l| self.cars[l].s);
                        // ([services] Lights and sirens do not stop to let a
                        // car change lanes in front of them.)
                        for &(at, len, owner) in cl.iter().filter(|&&(at, _, o)| at > car.s + 1.0 && at < ahead - 1.0 && !(car.siren && !self.cars[o].entering)) {
                            // Room for the whole vehicle (a bus is 12 m), not a car.
                            let room = at - len - IDM_MIN_GAP - car.s;
                            if room < gap && car.v * car.v < 2.0 * IDM_BRAKE * room.max(0.0) {
                                (gap, lead_v, why) = (room + IDM_MIN_GAP, 0.0, Wait::Leader);
                                by = Some(owner);
                            }
                        }
                    }
                    // [pedestrians] Someone on a crossing ahead, or the stop line
                    // (sirens included: they slow for people in the road).
                    if let Some(g) = self.crosswalk_gap(car).filter(|&g| g < gap) {
                        (gap, lead_v, why) = (g, 0.0, Wait::Yield);
                    }
                    // [rail-crossing] A level crossing ahead with its lights on.
                    if self.net.rail_closed_any {
                        if let Some(g) = self.rail_gap(car, lane).filter(|&g| g < gap) {
                            (gap, lead_v, why) = (g, 0.0, Wait::RailCrossing);
                        }
                    }
                    // [scene] Something at the roadside: slow, ease round
                    // it, or stop short where there is no room.
                    let ask = if zones.is_empty() { Default::default() } else { self.scene_ask(car, lane, &zones) };
                    if let Some(g) = ask.stop.filter(|&g| g < gap) {
                        (gap, lead_v, why) = (g, 0.0, Wait::Blocked);
                    }
                    if why == Wait::Leader && gap > 50.0 {
                        why = Wait::Free;
                    }
                    if by.is_none() {
                        by = match (why, lead) {
                            (Wait::Leader, Some(&l)) => Some(l),
                            (Wait::Leader | Wait::ExitFull, None) => self.blocker_ahead(car, why),
                            _ => None,
                        };
                    }
                    let left_wait = car.v < 0.5 && self.next_turn(car) == Some(-1);
                    // [driving] The driver sees the car ahead change speed
                    // only after their reaction time.
                    let reaction = car.drive.profile.reaction;
                    let seen = if reaction > 0.0 { car.drive.seen_lead_v + (lead_v - car.drive.seen_lead_v) * (dt / reaction).min(1.0) } else { lead_v };
                    // The limit here, or a lower one ahead the driver is
                    // already braking for; [tolls] slow through a toll booth.
                    let mut v0 = car.desired_speed(self.limit_ahead(car, lane));
                    if self.net.tolls && !car.siren {
                        if let Some(cap) = self.toll_cap(car) {
                            v0 = v0.min(cap);
                        }
                    }
                    if let Some(cap) = Self::offlaw_cap(car) {
                        v0 = v0.min(cap);
                    }
                    if let Some(cap) = ask.cap {
                        v0 = v0.min(cap);
                    }
                    accs.push(Step { by, id: cid, acc: car.idm(v0, gap, car.v - seen), why, left_wait, seen, run, dodge: ask.dodge });
                }
            }
            for st in accs {
                let car = &mut self.cars[st.id];
                Self::ease_dodge(car, st.dodge, dt);
                car.wait = st.why;
                car.blocked_by = st.by;
                car.drive.seen_lead_v = st.seen;
                if st.run.is_some() {
                    car.drive.running = st.run;
                }
                if st.left_wait {
                    car.turn_wait += dt;
                    if st.why == Wait::Yield {
                        car.yielded += dt;
                    }
                }
                car.cooldown = (car.cooldown - dt).max(0.0);
                if let Some(cr) = car.drive.crash.as_mut() {
                    car.v = 0.0;
                    car.stalled = 0.0;
                    if !cr.wrecked {
                        cr.hold -= dt;
                        if cr.hold <= 0.0 {
                            car.drive.crash = None;
                        }
                    }
                    continue;
                }
                if car.hold > 0.0 {
                    car.hold -= dt;
                    car.v = 0.0;
                    car.stalled = 0.0;
                    continue;
                }
                // [driving] A real driver cannot brake infinitely hard.
                let acc = if car.drive.profile.assigned { st.acc.max(-MAX_BRAKE) } else { st.acc };
                car.v = (car.v + acc * dt).max(0.0);
                car.s += car.v * dt;
                car.stalled = if car.v < 0.5 { car.stalled + dt } else { 0.0 };
                if car.v >= 0.5 {
                    car.replans = 0;
                }
                if car.v < 0.3 {
                    // Stopped after all: the next light is a new decision.
                    car.drive.running = None;
                }
                if car.passing {
                    // Out onto the shoulder (the offset was checked against
                    // the kerb when the pass began).
                    let target = CAR_WIDTH + 0.25;
                    let rate = PASS_LATERAL_SPEED * (car.v / 3.0).clamp(0.5, 1.0) * dt;
                    car.lateral = (car.lateral + rate).min(target.max(car.lateral));
                } else if car.glide_from != 0.0 && (car.lateral - car.glide_at()).abs() < 0.05 {
                    // Eased out and back in along a half cosine (a car
                    // standing mid-change still creeps across).
                    car.glide_done += car.v.max(GLIDE_CREEP) * dt;
                    car.lateral = car.glide_at();
                    if car.glide_done >= car.glide_len {
                        car.lateral = 0.0;
                        car.glide_from = 0.0;
                    }
                } else if car.lateral != 0.0 {
                    // Settling back from a nudge (pulled aside for a siren,
                    // back from the shoulder): no glide under way.
                    car.glide_from = 0.0;
                    let rate = LC_LATERAL_SPEED * (car.v / 3.0).clamp(0.3, 1.0) * dt;
                    car.lateral -= car.lateral.signum() * car.lateral.abs().min(rate);
                }
            }

            self.yield_to_sirens(dt);

            let waiting: Vec<CarId> = self.cars.iter().filter(|(_, c)| c.entering).map(|(id, _)| id).collect();
            for id in waiting {
                // Standing at the kerb on purpose (unloading): not waiting yet.
                if self.cars[id].hold > 0.0 {
                    let c = &mut self.cars[id];
                    c.hold -= dt;
                    c.v = 0.0;
                    c.stalled = 0.0;
                    c.wait = Wait::Dwell;
                    continue;
                }
                if !self.pull_out(id) {
                    let c = &mut self.cars[id];
                    c.stalled += dt;
                    c.wait = Wait::Entering;
                }
            }

            self.lane_changes();

            let ids: Vec<CarId> = self.cars.keys().collect();
            for cid in ids {
                if self.cars[cid].in_stunt() {
                    // [driving] In a takeover show, off the lanes.
                    self.cars[cid].age = 0.0;
                    continue;
                }
                // [carlife] A trip this long is stuck somewhere: look for
                // another way from where it stands. Nobody is taken off
                // the road; a jam is part of the city and its drivers feel it.
                if self.cars[cid].age > REPLAN_AFTER {
                    self.cars[cid].age = 0.0;
                    if !self.cars[cid].entering && self.cars[cid].idx + 1 < self.cars[cid].route.len() {
                        self.replan_from_here(cid);
                    }
                }
                if self.cars[cid].entering {
                    continue;
                }
                // Read after the replan: it swaps the route and resets `idx`,
                // so a snapshot taken before would test the old index against
                // the new route (and could "arrive" a car blocks from its goal).
                let (lane_id, idx, len, s, s_end) = {
                    let c = &self.cars[cid];
                    let lane_id = c.route[c.idx];
                    (lane_id, c.idx, self.net.lanes[lane_id as usize].length, c.s, c.s_end)
                };
                if idx + 1 == self.cars[cid].route.len() {
                    if s >= s_end - 0.5 {
                        let c = &mut self.cars[cid];
                        c.s = s_end.min(len);
                        c.v = 0.0;
                        // [driving] A takeover car waits at the meet for the show.
                        if c.drive.gang.is_some_and(|g| matches!(g.phase, GangPhase::Joining | GangPhase::Racing)) {
                            continue;
                        }
                        if !arrived.contains(&cid) {
                            arrived.push(cid);
                        }
                    }
                } else if s >= len {
                    // On through as many lanes as the step covered: a fast
                    // car crosses a short connector or curve piece within
                    // one step. Taking one lane a step left it standing at
                    // the end of each short lane (drawn there) and then
                    // springing ahead with the backlog: the stutter along
                    // curved ramps.
                    let (mut lane_id, mut len) = (lane_id, len);
                    loop {
                        if self.cars[cid].change_pending(&self.net).is_some() {
                            // Never roll off the end into the lane beside.
                            let c = &mut self.cars[cid];
                            c.s = len - 0.05;
                            c.v = 0.0;
                            break;
                        }
                        // Never land on (or ahead of) the tail of the last
                        // vehicle into the next lane: it would be shoved back
                        // there onto whoever stands behind, and with nowhere
                        // behind the lane start cars piled up on it (rings of
                        // short segments locked solid that way). Wait at the
                        // end of this lane until there is room. (Lights and
                        // sirens squeeze past as they always have.)
                        let (at, idx_now, siren) = (self.cars[cid].s, self.cars[cid].idx, self.cars[cid].siren);
                        if let Some(&next) = self.cars[cid].route.get(idx_now + 1).filter(|_| !siren) {
                            // [driving] Nor a crash on the shoulder it is
                            // easing past (`traffic/hazard.rs`).
                            let passing = self.cars[cid].passing_hazard();
                            if let Some(&tail) = self.net.lanes[next as usize].cars.first().filter(|&&t| !(passing && self.is_hazard(&self.cars[t]))) {
                                let room = self.cars[tail].s - self.cars[tail].length() - 0.1;
                                if at - len > room {
                                    let tv = self.cars[tail].v;
                                    let c = &mut self.cars[cid];
                                    c.s = len - 0.05;
                                    c.v = c.v.min(tv);
                                    break;
                                }
                            }
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
                        self.entered_red(next, cid);
                        // [tolls] Came onto a tolled lane from behind: it will
                        // pay once past the booth line (even if already past it
                        // after this substep's hop).
                        if let Some(t) = self.net.lanes[next as usize].toll {
                            let c = &mut self.cars[cid];
                            if c.toll_booth != t.booth {
                                c.toll_booth = t.booth;
                                c.toll_paid = false;
                            }
                        }
                        let c = &self.cars[cid];
                        let next_len = self.net.lanes[next as usize].length;
                        if c.idx + 1 >= c.route.len() || c.s < next_len {
                            break;
                        }
                        (lane_id, len) = (next, next_len);
                    }
                }
            }
            if self.net.tolls {
                self.toll_gates();
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

/// One car's physics for a substep, worked out before any car moves.
struct Step {
    id: CarId,
    /// The vehicle holding it back, if one is.
    by: Option<CarId>,
    acc: f32,
    why: Wait,
    left_wait: bool,
    /// The leader's speed as the driver now perceives it.
    seen: f32,
    /// Committed to running this red.
    run: Option<LaneId>,
    /// [scene] How far sideways to ease round a scene.
    dodge: f32,
}

#[cfg(test)]
mod tests;
