//! [rail-crossing] Level crossings: where a street or a footpath crosses the
//! railway at grade. Each one runs a small state machine driven by the
//! trains coming: the red lights start flashing [`WARN_LEAD`] seconds before
//! a train reaches it, the barrier arms come down [`WARN_ONLY`] seconds later
//! (taking [`LOWER_SECONDS`]), stay down while a train is over it and for
//! [`CLEAR_DELAY`] after the last vehicle of the last train has cleared, then
//! rise (taking [`RAISE_SECONDS`]) and the lights go out.
//!
//! - Streets with more than two lanes in all ([`BARRIER_MIN_LANES`]) get
//!   half-barriers: an arm on the kerb side of each approach covering the
//!   lanes coming in, so a car still on the crossing can always drive off.
//!   Smaller streets get the flashing lights, a bell and the crossbuck only
//!   (as most two-lane rural crossings do); footpaths get lights and small
//!   gates.
//! - Road traffic: while a crossing is anything but open its connector
//!   lanes are closed to vehicles (`Lane::rail_closed`): a driver stops at the
//!   stop line unless they are too close to stop (they go on and clear it);
//!   anyone already on it drives off. Walkers and cyclists stop at the edge
//!   of the track's band ([`LevelCrossing::entry`]) and those on it walk off.
//! - Trains: a train never runs onto a crossing that is not closed with
//!   its box clear of vehicles; it brakes and waits at it instead (the
//!   timing makes that rare: the arms are down before it gets there).
//!
//! The geometry is derived from the road network (rebuilt when the roads
//! change); the phase, timers and the counts are saved, every field with a
//! default so old saves load. Trains are not saved, so after a load the
//! crossings find none coming and open.

use crate::roads::{NodeId, RoadKind, RoadNetwork, SegmentId, PATH_HALF_WIDTH, RAIL_HALF_WIDTH};
use crate::zoning::SIDEWALK;
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Seconds before a train reaches the crossing that the lights start.
pub const WARN_LEAD: f32 = 15.0;
/// Seconds of lights alone before the arms start down.
pub const WARN_ONLY: f32 = 3.0;
/// Seconds the arms take to come down, and to go back up.
pub const LOWER_SECONDS: f32 = 5.0;
pub const RAISE_SECONDS: f32 = 5.0;
/// Seconds the arms stay down after the last train has cleared.
pub const CLEAR_DELAY: f32 = 2.0;
/// Streets with at least this many lanes in all get barrier arms.
pub const BARRIER_MIN_LANES: u8 = 3;
/// From the edge of the track's band to the stop line, metres.
pub const STOP_BACK: f32 = 3.2;
/// From the stop line towards the track to the barrier's pivot, metres.
pub const BARRIER_IN: f32 = 1.2;
/// Half the width of the band walkers and riders keep off while the
/// crossing is closed, across the track.
pub const WALK_BAND: f32 = RAIL_HALF_WIDTH + 0.8;
/// A train held at a block signal is assumed to stand this much longer
/// (seconds): a held train near a crossing does not keep it shut.
pub const HELD_EXTRA: f32 = 20.0;
/// Passes remembered for the trains-per-hour count (sim minutes).
pub const PASS_WINDOW: u64 = 24 * 60;

/// Where a crossing is in its cycle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrossingPhase {
    /// Lights out, arms up: traffic passes.
    #[default]
    Open,
    /// A train is coming: lights flashing, arms still up.
    Warning,
    /// Lights flashing, arms coming down.
    Lowering,
    /// Arms down: the train may pass.
    Closed,
    /// The train has gone: arms going up, lights still flashing.
    Raising,
}

impl CrossingPhase {
    pub fn label(self) -> &'static str {
        match self {
            CrossingPhase::Open => "open",
            CrossingPhase::Warning => "warning",
            CrossingPhase::Lowering => "lowering",
            CrossingPhase::Closed => "closed",
            CrossingPhase::Raising => "raising",
        }
    }
}

/// What protects a crossing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CrossingGuard {
    /// Half-barriers, lights, a bell and crossbucks (a street of three
    /// lanes or more).
    Barriers,
    /// Lights, a bell and crossbucks (a one- or two-lane street).
    #[default]
    Lights,
    /// A footpath: lights and small swing-down gates.
    Footpath,
}

impl CrossingGuard {
    pub fn label(self) -> &'static str {
        match self {
            CrossingGuard::Barriers => "barriers",
            CrossingGuard::Lights => "lights",
            CrossingGuard::Footpath => "footpath gates",
        }
    }

    /// Arms that come down (on the road or the path).
    pub fn has_arms(self) -> bool {
        self != CrossingGuard::Lights
    }
}

/// One way meeting the crossing: a street or path arm leaving the node.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CrossingArm {
    pub segment: u32,
    /// Unit direction from the crossing out along the arm. Traffic arriving
    /// on this arm heads `-away`.
    pub away: Vec2,
    /// Lanes arriving on this arm (towards the track) and leaving on it.
    pub lanes_in: u8,
    pub lanes_out: u8,
    /// Half the carriageway (or the path).
    pub half_width: f32,
    /// Distance from the crossing's centre along the arm to the stop line.
    pub stop: f32,
    pub path: bool,
}

impl CrossingArm {
    /// The arriving traffic's heading.
    pub fn heading(&self) -> Vec2 {
        -self.away
    }

    /// Where the barrier pivots: on the verge just off the kerb on the
    /// arriving traffic's right, [`BARRIER_IN`] in from the stop line.
    pub fn pivot(&self, at: Vec2) -> Vec2 {
        let h = self.heading();
        let right = Vec2::new(-h.y, h.x);
        let edge = if self.path { self.half_width + 0.35 } else { self.half_width + 0.55 };
        at + self.away * (self.stop - BARRIER_IN).max(1.0) + right * edge
    }

    /// The arm's length when down: over the arriving lanes to the centre
    /// line (all of a one-way street; all of a path).
    pub fn arm_length(&self) -> f32 {
        let total = (self.lanes_in + self.lanes_out).max(1) as f32;
        let share = if self.path || self.lanes_out == 0 { 1.0 } else { self.lanes_in as f32 / total };
        let edge = if self.path { 0.35 } else { 0.55 };
        (self.half_width * 2.0 * share + edge - 0.15).max(1.0)
    }
}

/// A level crossing.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct LevelCrossing {
    /// The road node on the track.
    pub node: NodeId,
    pub pos: Vec2,
    /// Unit direction along the track.
    pub rail_dir: Vec2,
    pub guard: CrossingGuard,
    pub arms: Vec<CrossingArm>,
    /// Lanes of the widest street over it, both ways together.
    pub lanes: u8,
    /// Half the length of the box along the track: how far either side of
    /// the centre the crossing reaches (the streets and their pavements).
    pub reach: f32,
    pub phase: CrossingPhase,
    /// Seconds into the phase (Warning), or since the last train cleared
    /// (Closed).
    pub timer: f32,
    /// How far down the arms are, 0 up to 1 down, now and last tick (the
    /// renderer blends between).
    pub arm: f32,
    pub prev_arm: f32,
    /// Seconds until the next train reaches it, as last seen (none coming:
    /// `None`).
    pub eta: Option<f32>,
    /// A train over it now.
    pub occupied: bool,
    /// The sim ticks trains went over it, the last [`PASS_WINDOW`].
    pub passes: Vec<u64>,
    pub trains_total: u32,
    /// Times it closed, and the seconds it was not open in all.
    pub closures: u32,
    pub shut_seconds: f32,
    /// Seconds the current (or last) closure has lasted.
    pub closure_seconds: f32,
    /// Vehicles stopped by it and the seconds they stood there in all.
    pub vehicles_held: u32,
    pub held_seconds: f32,
    /// Walkers and riders standing at it this tick.
    #[serde(skip)]
    pub people_waiting: u32,
    /// Vehicles standing at its stop lines this tick.
    #[serde(skip)]
    pub vehicles_waiting: u32,
    /// Times a train had to wait at it (the arms were not down in time or
    /// a vehicle was on it).
    pub trains_held: u32,
    /// The vehicles held in the current closure (counted once each).
    #[serde(skip)]
    pub waiting: Vec<crate::traffic::CarId>,
    /// The trains seen over it in the current closure.
    #[serde(skip)]
    pub seen: Vec<crate::rail::TrainId>,
}

impl Default for LevelCrossing {
    fn default() -> Self {
        Self {
            node: NodeId(u32::MAX),
            pos: Vec2::ZERO,
            rail_dir: Vec2::X,
            guard: CrossingGuard::Lights,
            arms: Vec::new(),
            lanes: 0,
            reach: 0.0,
            phase: CrossingPhase::Open,
            timer: 0.0,
            arm: 0.0,
            prev_arm: 0.0,
            eta: None,
            occupied: false,
            passes: Vec::new(),
            trains_total: 0,
            closures: 0,
            shut_seconds: 0.0,
            closure_seconds: 0.0,
            vehicles_held: 0,
            held_seconds: 0.0,
            people_waiting: 0,
            vehicles_waiting: 0,
            trains_held: 0,
            waiting: Vec::new(),
            seen: Vec::new(),
        }
    }
}

impl LevelCrossing {
    /// Closed to road traffic and walkers: anything but open.
    pub fn shut(&self) -> bool {
        self.phase != CrossingPhase::Open
    }

    /// The lights are flashing.
    pub fn flashing(&self) -> bool {
        self.phase != CrossingPhase::Open
    }

    /// A train may run over it (with the box clear of vehicles, which the
    /// world checks).
    pub fn down(&self) -> bool {
        self.phase == CrossingPhase::Closed
    }

    /// Trains over it in the last hour, as of tick `now`.
    pub fn trains_last_hour(&self, now: u64) -> usize {
        self.passes.iter().filter(|&&t| now.saturating_sub(t) < 60).count()
    }

    /// Trains an hour over the last day (or as long as it has existed).
    pub fn trains_per_hour(&self, now: u64) -> f32 {
        let n = self.passes.iter().filter(|&&t| now.saturating_sub(t) < PASS_WINDOW).count();
        let span = self.passes.first().map_or(60, |&t| now.saturating_sub(t).clamp(60, PASS_WINDOW));
        n as f32 * 60.0 / span as f32
    }

    /// Mean seconds a held vehicle stood at it.
    pub fn average_wait(&self) -> f32 {
        if self.vehicles_held == 0 { 0.0 } else { self.held_seconds / self.vehicles_held as f32 }
    }

    /// Mean seconds it stays shut per closure.
    pub fn average_closure(&self) -> f32 {
        if self.closures == 0 { 0.0 } else { self.shut_seconds / self.closures as f32 }
    }

    /// Advance the cycle by `dt` seconds, with the next train `eta` seconds
    /// away (none: `None`) and whether one is over it now. Phase changes
    /// inside the step carry the time left over, so the cycle runs the same
    /// whatever the tick length. Returns true when a closure ended.
    pub fn step(&mut self, dt: f32, eta: Option<f32>, occupied: bool) -> bool {
        self.prev_arm = self.arm;
        self.eta = eta;
        self.occupied = occupied;
        let wanted = occupied || eta.is_some_and(|e| e <= WARN_LEAD);
        if self.shut() {
            self.shut_seconds += dt;
            self.closure_seconds += dt;
        }
        let mut left = dt;
        let mut ended = false;
        for _ in 0..8 {
            match self.phase {
                CrossingPhase::Open => {
                    if !wanted {
                        break;
                    }
                    self.phase = CrossingPhase::Warning;
                    self.timer = 0.0;
                    self.closures += 1;
                    self.closure_seconds = 0.0;
                    self.shut_seconds += left;
                    self.closure_seconds += left;
                }
                CrossingPhase::Warning => {
                    if !wanted {
                        // The train stopped short (a station, a signal).
                        self.phase = CrossingPhase::Raising;
                        continue;
                    }
                    let need = WARN_ONLY - self.timer;
                    if left < need {
                        self.timer += left;
                        left = 0.0;
                    } else {
                        left -= need;
                        self.phase = CrossingPhase::Lowering;
                        self.timer = 0.0;
                    }
                }
                CrossingPhase::Lowering => {
                    if !wanted {
                        self.phase = CrossingPhase::Raising;
                        continue;
                    }
                    let need = (1.0 - self.arm) * LOWER_SECONDS;
                    if left < need {
                        self.arm += left / LOWER_SECONDS;
                        left = 0.0;
                    } else {
                        left -= need;
                        self.arm = 1.0;
                        self.phase = CrossingPhase::Closed;
                        self.timer = 0.0;
                    }
                }
                CrossingPhase::Closed => {
                    if wanted {
                        self.timer = 0.0;
                        left = 0.0;
                    } else {
                        let need = CLEAR_DELAY - self.timer;
                        if left < need {
                            self.timer += left;
                            left = 0.0;
                        } else {
                            left -= need;
                            self.phase = CrossingPhase::Raising;
                            self.timer = 0.0;
                        }
                    }
                }
                CrossingPhase::Raising => {
                    if wanted {
                        self.phase = CrossingPhase::Lowering;
                        continue;
                    }
                    let need = self.arm * RAISE_SECONDS;
                    if left < need {
                        self.arm -= left / RAISE_SECONDS;
                        left = 0.0;
                    } else {
                        left -= need;
                        self.arm = 0.0;
                        self.phase = CrossingPhase::Open;
                        self.timer = 0.0;
                        // Not shut for what is left of the step.
                        self.shut_seconds -= left;
                        self.closure_seconds -= left;
                        self.waiting.clear();
                        self.seen.clear();
                        ended = true;
                        break;
                    }
                }
            }
            if left <= 0.0 {
                break;
            }
        }
        self.arm = self.arm.clamp(0.0, 1.0);
        ended
    }

    /// Where the step of a walker or rider from `a` to `b` would enter the
    /// band along the track this crossing keeps clear (`0..=1` along it),
    /// or `None`: it stays out, or starts inside (it is crossing: let it
    /// clear).
    pub fn entry(&self, a: Vec2, b: Vec2) -> Option<f32> {
        let n = Vec2::new(-self.rail_dir.y, self.rail_dir.x);
        let to = |p: Vec2| ((p - self.pos).dot(self.rail_dir), (p - self.pos).dot(n));
        let (au, av) = to(a);
        let (bu, bv) = to(b);
        let inside = |u: f32, v: f32| u.abs() <= self.reach && v.abs() <= WALK_BAND;
        if inside(au, av) {
            return None;
        }
        // Clip against the box (Liang-Barsky).
        let (du, dv) = (bu - au, bv - av);
        let (mut t0, mut t1) = (0.0f32, 1.0f32);
        for (p, q) in [(-du, au + self.reach), (du, self.reach - au), (-dv, av + WALK_BAND), (dv, WALK_BAND - av)] {
            if p.abs() < 1e-6 {
                if q < 0.0 {
                    return None;
                }
                continue;
            }
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
            if t0 > t1 {
                return None;
            }
        }
        Some(t0)
    }
}

/// The crossings of a road network, with their cycles and counts.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LevelCrossings {
    pub list: Vec<LevelCrossing>,
    /// The roads version the geometry was taken from (`None` after a load:
    /// it is taken again, the cycles and counts kept).
    #[serde(skip)]
    pub built_for: Option<u64>,
    /// Trains standing at a crossing this tick, waiting for it to close.
    #[serde(skip)]
    pub trains_waiting: Vec<crate::rail::TrainId>,
    /// Some crossing is shut this tick.
    #[serde(skip)]
    pub any_shut: bool,
}

impl LevelCrossings {
    /// Take the crossings from the roads again, keeping each one's cycle
    /// and counts where a crossing still stands at about the same place.
    pub fn rebuild(&mut self, roads: &RoadNetwork) {
        let old = std::mem::take(&mut self.list);
        let mut list = find(roads);
        for c in &mut list {
            if let Some(o) = old.iter().filter(|o| o.pos.distance(c.pos) < 3.0).min_by(|a, b| a.pos.distance(c.pos).total_cmp(&b.pos.distance(c.pos))) {
                let geo = (c.node, c.pos, c.rail_dir, c.guard, std::mem::take(&mut c.arms), c.lanes, c.reach);
                *c = o.clone();
                (c.node, c.pos, c.rail_dir, c.guard, c.arms, c.lanes, c.reach) = geo;
            }
        }
        self.list = list;
        self.built_for = Some(roads.version);
    }

    /// The crossing at `node`, if it is one.
    pub fn at_node(&self, node: NodeId) -> Option<&LevelCrossing> {
        self.list.iter().find(|c| c.node == node)
    }
}

/// The unit direction of `sid` away from `node`.
fn away_from(roads: &RoadNetwork, node: NodeId, sid: SegmentId) -> Vec2 {
    let s = roads.segment(sid);
    let (a, b) = roads.segment_points(sid);
    if s.a == node { (b - a).normalize_or_zero() } else { (a - b).normalize_or_zero() }
}

/// The track's direction at `node` (an alive rail piece there), if any.
fn rail_dir_at(roads: &RoadNetwork, node: NodeId) -> Option<Vec2> {
    let n = roads.node(node);
    let rail = n.segments.iter().copied().find(|&s| roads.segment(s).alive && roads.segment(s).is_rail())?;
    let (a, b) = roads.segment_points(rail);
    Some((b - a).normalize_or_zero()).filter(|d| *d != Vec2::ZERO)
}

/// Distance from the centre of a crossing along an arm leaving it at
/// `away` (half-width `hw`) to its stop line, with the track along `rail`.
fn stop_distance(away: Vec2, rail: Vec2, hw: f32) -> f32 {
    let sin = away.perp_dot(rail).abs().max(0.35);
    let cos = away.dot(rail).abs();
    (RAIL_HALF_WIDTH + STOP_BACK + hw * cos) / sin
}

/// How far from a level crossing's centre the lanes of its streets end
/// (at the stop line), or `None` where `node` is no level crossing. The
/// lane network uses it for the node's radius so cars stop clear of the
/// track, and the street mesh paints the stop lines there.
pub fn stop_radius(roads: &RoadNetwork, node: NodeId) -> Option<f32> {
    let rail = rail_dir_at(roads, node)?;
    let n = roads.node(node);
    n.segments
        .iter()
        .copied()
        .filter(|&s| roads.segment(s).alive && roads.segment(s).is_street())
        .map(|s| stop_distance(away_from(roads, node, s), rail, roads.segment(s).half_width()))
        .reduce(f32::max)
}

/// Every level crossing of the network: a node where an alive rail piece
/// meets a street or a footpath (they meet at grade, or there would be no
/// shared node).
pub fn find(roads: &RoadNetwork) -> Vec<LevelCrossing> {
    (0..roads.nodes.len()).filter_map(|i| at(roads, NodeId(i as u32))).collect()
}

/// The level crossing at `node` (its geometry; the cycle at rest), if it is one.
pub fn at(roads: &RoadNetwork, node: NodeId) -> Option<LevelCrossing> {
    let n = roads.node(node);
    let alive: Vec<SegmentId> = n.segments.iter().copied().filter(|&s| roads.segment(s).alive).collect();
    if !alive.iter().any(|&s| roads.segment(s).is_rail()) {
        return None;
    }
    let ways: Vec<SegmentId> = alive.iter().copied().filter(|&s| matches!(roads.segment(s).kind, RoadKind::Street | RoadKind::Path)).collect();
    if ways.is_empty() {
        return None;
    }
    let rail = rail_dir_at(roads, node)?;
    let streets = ways.iter().any(|&s| roads.segment(s).is_street());
    let mut arms = Vec::new();
    let mut lanes = 0u8;
    let mut reach = 0.0f32;
    for &sid in &ways {
        let s = roads.segment(sid);
        let path = s.kind == RoadKind::Path;
        // Paths where streets cross too: the street's gear serves them.
        if path && streets {
            continue;
        }
        let away = away_from(roads, node, sid);
        let hw = s.half_width();
        let (lanes_in, lanes_out) = if path { (0, 0) } else if s.b == node { (s.lanes_fwd, s.lanes_bwd) } else { (s.lanes_bwd, s.lanes_fwd) };
        lanes = lanes.max(s.lanes());
        let sin = away.perp_dot(rail).abs().max(0.35);
        let stop = if path { (WALK_BAND + 1.2 + hw * away.dot(rail).abs()) / sin } else { stop_distance(away, rail, hw) };
        let side = if path { hw + 1.5 } else { hw + SIDEWALK + 0.5 };
        reach = reach.max(side / sin + WALK_BAND * away.dot(rail).abs() / sin);
        arms.push(CrossingArm { segment: sid.0, away, lanes_in, lanes_out, half_width: hw, stop, path });
    }
    if arms.is_empty() {
        return None;
    }
    let guard = if !streets {
        CrossingGuard::Footpath
    } else if lanes >= BARRIER_MIN_LANES {
        CrossingGuard::Barriers
    } else {
        CrossingGuard::Lights
    };
    Some(LevelCrossing { node, pos: n.pos, rail_dir: rail, guard, arms, lanes, reach: reach.max(PATH_HALF_WIDTH + 1.5), ..Default::default() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(c: &mut LevelCrossing, secs: f32, eta: impl Fn(f32) -> Option<f32>, occ: impl Fn(f32) -> bool, t0: f32) -> Vec<(f32, CrossingPhase, f32)> {
        let mut out = Vec::new();
        let mut t = t0;
        while t < t0 + secs {
            c.step(1.0, eta(t), occ(t));
            t += 1.0;
            out.push((t, c.phase, c.arm));
        }
        out
    }

    #[test]
    fn the_cycle_warns_lowers_holds_and_raises() {
        let mut c = LevelCrossing::default();
        // A train reaches it at t = 30 and is over it until t = 40.
        let log = run(&mut c, 60.0, |t| (t < 30.0).then_some(30.0 - t), |t| (30.0..40.0).contains(&t), 0.0);
        let at = |t: f32| log.iter().find(|x| x.0 >= t).unwrap();
        assert_eq!(at(10.0).1, CrossingPhase::Open);
        assert_eq!(at(17.0).1, CrossingPhase::Warning, "lights from 15 s out");
        assert_eq!(at(20.0).1, CrossingPhase::Lowering);
        // Down before the train arrives.
        assert_eq!(at(30.0).1, CrossingPhase::Closed);
        assert_eq!(at(30.0).2, 1.0);
        assert_eq!(at(41.0).1, CrossingPhase::Closed, "down a moment after it clears");
        assert_eq!(at(44.0).1, CrossingPhase::Raising);
        assert_eq!(at(50.0).1, CrossingPhase::Open);
        assert_eq!(at(50.0).2, 0.0);
        assert_eq!(c.closures, 1);
    }

    #[test]
    fn arms_are_down_before_the_train_whatever_the_step_length() {
        for dt in [0.5f32, 1.0, 2.0, 4.0] {
            for arrive in [29.0f32, 30.0, 31.0, 32.5] {
                let mut c = LevelCrossing::default();
                let mut t = 0.0;
                let mut opened_at = None;
                while t < 80.0 {
                    let eta = (t < arrive).then_some(arrive - t);
                    c.step(dt, eta, (arrive..arrive + 10.0).contains(&t));
                    t += dt;
                    if (arrive..arrive + 10.0).contains(&t) {
                        assert_eq!(c.phase, CrossingPhase::Closed, "dt {dt} arrive {arrive} t {t}");
                    }
                    if t > arrive && c.phase == CrossingPhase::Open && opened_at.is_none() {
                        opened_at = Some(t);
                    }
                }
                let open = opened_at.expect("opens again");
                assert!(open - (arrive + 10.0) <= CLEAR_DELAY + RAISE_SECONDS + 2.0 * dt, "dt {dt}: open at {open}");
            }
        }
    }

    #[test]
    fn a_walker_is_stopped_at_the_band_and_one_on_it_walks_off() {
        let c = LevelCrossing { pos: Vec2::ZERO, rail_dir: Vec2::X, reach: 10.0, ..Default::default() };
        // Walking across the track (along y).
        let t = c.entry(Vec2::new(0.0, -10.0), Vec2::new(0.0, 0.0)).unwrap();
        assert!((t - (10.0 - WALK_BAND) / 10.0).abs() < 1e-4);
        assert!(c.entry(Vec2::new(0.0, -1.0), Vec2::new(0.0, 5.0)).is_none(), "on it: walks off");
        assert!(c.entry(Vec2::new(20.0, -10.0), Vec2::new(20.0, 10.0)).is_none(), "away along the track");
    }
}
