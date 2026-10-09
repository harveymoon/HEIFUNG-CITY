//! Pedestrians in the lane network: walk lights, drivers giving way at
//! crossings, and cars that went over one with someone on it.
//!
//! - Walk lights (`PedTraffic::walk`): the crosswalk across an arm of a
//!   signalised junction shows walk while the vehicles running parallel to
//!   it have their green (any phase that does not include that arm), after
//!   any protected left arrow, until `PED_CLEARANCE` seconds before the
//!   green ends (when another phase is waiting for it). Turning cars that
//!   cross that crosswalk give way to whoever is on it.
//! - Walkers waiting at (or on) a crosswalk call its walk phase
//!   (`PedTraffic::calls`), so a signal never skips it and does not cut it
//!   short while someone is crossing.
//! - Holds (`PedTraffic::holds`): the world marks every crossing someone is
//!   on (or stepping onto) as a stop point on the lanes that pass over it;
//!   a driver who can still stop comfortably stops short of it. At a
//!   signalised crosswalk the stop line (`PedTraffic::stop_lines`) also
//!   keeps cars waiting at a red off the zebra.
//! - Conflicts (`PedTraffic::conflicts`): a car that drives over an
//!   occupied crossing at speed, for the world to turn into a near miss or,
//!   rarely, someone hurt.
//!
//! Kept apart from the car-following code in `traffic.rs`: `step` calls
//! `ped_demand`, `update_walk`, `crosswalk_gap` and `crosswalk_conflicts`.

use super::*;
use std::collections::BTreeMap;

/// Last seconds of a vehicle green in which nobody starts across.
pub const PED_CLEARANCE: f32 = 4.0;
/// Hardest a driver brakes to give way to a walker, m/s per s.
const PED_BRAKE: f32 = 5.0;
/// An emergency stop for someone in the road where there is no crosswalk:
/// only a driver who can still stop this hard does.
const PED_EMERGENCY_BRAKE: f32 = 8.0;
/// A car still going this fast over an occupied crossing is a conflict.
pub const CONFLICT_SPEED: f32 = 3.0;
/// How far ahead (metres) a driver looks for someone on a crossing.
const PED_LOOKAHEAD: f32 = 60.0;
/// [services] Walkers hear and see lights and sirens this far up the road
/// (along the lanes leading to the crossing) and keep the crossing clear.
pub const SIREN_WARNING: f32 = 90.0;
/// Past this far beyond the start of a crossing a vehicle is over it.
const HOLD_DEPTH_MAX: f32 = 8.0;

/// A stop point on a lane in front of a crossing.
#[derive(Clone, Copy, Debug)]
pub struct Hold {
    /// Distance along the lane where the crossing begins (less a margin).
    pub s: f32,
    /// Depth of the crossing along the lane.
    pub depth: f32,
    /// Index into `WalkGraph::crossings`.
    pub crossing: u32,
}

/// Pedestrian state of the lane network, refreshed by the world every tick.
#[derive(Clone, Debug, Default)]
pub struct PedTraffic {
    /// Per intersection, per approach: the walk light on the crosswalk
    /// across that arm.
    pub walk: Vec<Vec<bool>>,
    /// Per intersection, per approach: someone waiting at or crossing it.
    pub calls: Vec<Vec<bool>>,
    /// Occupied crossings, per lane passing over them.
    pub holds: BTreeMap<LaneId, Vec<Hold>>,
    /// Unmarked crossings someone is on (a roundabout's lanes, a dead end,
    /// jaywalking): drivers do not stop for them, but a car that goes over
    /// one with someone on it is still a near miss or worse.
    pub watch: BTreeMap<LaneId, Vec<Hold>>,
    /// Stop lines before signalised crosswalks, per incoming lane: where
    /// a car waiting at a red stops.
    pub stop_lines: HashMap<LaneId, f32>,
    /// Cars that went over an occupied crossing at speed since the world
    /// last drained this: (car, crossing, speed).
    pub conflicts: Vec<(CarId, u32, f32)>,
    /// (car, crossing) reported this tick and last, not to report twice.
    seen: Vec<(CarId, u32, u8)>,
}

impl PedTraffic {
    /// Is the walk light on across `approach` of intersection `inter`?
    pub fn walk_shown(&self, inter: usize, approach: usize) -> bool {
        self.walk.get(inter).and_then(|w| w.get(approach)).copied().unwrap_or(false)
    }
}

impl Traffic {
    /// The world's pedestrians for the next step: occupied crossings and
    /// walk calls. Conflicts already reported last tick are forgotten.
    pub fn set_pedestrians(&mut self, holds: BTreeMap<LaneId, Vec<Hold>>, calls: Vec<Vec<bool>>) {
        self.peds.holds = holds;
        self.peds.watch.clear();
        self.peds.calls = calls;
        self.peds.seen.retain_mut(|x| {
            x.2 += 1;
            x.2 < 2
        });
    }

    /// The unmarked crossings in use this step (see `PedTraffic::watch`);
    /// set after `set_pedestrians`.
    pub fn set_unmarked(&mut self, watch: BTreeMap<LaneId, Vec<Hold>>) {
        self.peds.watch = watch;
    }

    /// Walkers waiting at a crosswalk call every phase that would give
    /// them the walk light.
    pub(super) fn ped_demand(&self, demand: &mut [SignalDemand]) {
        for (k, calls) in self.peds.calls.iter().enumerate() {
            let (Some(inter), Some(d)) = (self.net.intersections.get(k), demand.get_mut(k)) else { continue };
            if !inter.signalized || d.any.len() < inter.approaches {
                continue;
            }
            for (a, _) in calls.iter().enumerate().filter(|(_, &c)| c) {
                for g in inter.phases.iter().filter(|g| !g.contains(&a)) {
                    for &ap in g {
                        if let Some(x) = d.any.get_mut(ap) {
                            *x = true;
                        }
                    }
                }
            }
        }
    }

    /// The walk lights, from the signal state after this substep.
    pub(super) fn update_walk(&mut self, demand: &[SignalDemand]) {
        let n = self.net.intersections.len();
        self.peds.walk.resize(n, Vec::new());
        for (k, inter) in self.net.intersections.iter().enumerate() {
            let w = &mut self.peds.walk[k];
            w.clear();
            w.resize(inter.approaches, false);
            if !inter.signalized || inter.phases.is_empty() {
                continue;
            }
            let green_end = inter.arrow + inter.green();
            if inter.timer < inter.arrow || inter.timer >= green_end {
                continue;
            }
            // Will this green end soon? Only if another phase wants it.
            let others = demand.get(k).filter(|d| d.any.len() >= inter.approaches).is_none_or(|d| inter.phases.iter().enumerate().any(|(p, g)| p != inter.phase && g.iter().any(|&ap| d.any[ap])));
            if others && inter.timer >= green_end - PED_CLEARANCE {
                continue;
            }
            let group = &inter.phases[inter.phase];
            for (ap, x) in w.iter_mut().enumerate() {
                *x = !group.contains(&ap);
            }
        }
    }

    /// The gap (for the car-following model) to the nearest crossing ahead
    /// that someone is on, if the driver can still stop for it; and to a
    /// signalised crosswalk's stop line when the light ahead is red.
    pub(super) fn crosswalk_gap(&self, car: &Car) -> Option<f32> {
        if self.peds.holds.is_empty() && self.peds.watch.is_empty() && self.peds.stop_lines.is_empty() {
            return None;
        }
        let can_stop = |dist: f32, brake: f32| car.v * car.v <= 2.0 * brake * dist.max(0.0) + 0.25;
        let mut base = -car.s;
        let mut best: Option<f32> = None;
        let mut take = |dist: f32, brake: f32| {
            if dist >= -0.3 && dist < PED_LOOKAHEAD && can_stop(dist, brake) {
                let g = dist.max(0.0) + IDM_MIN_GAP;
                best = Some(best.map_or(g, |b: f32| b.min(g)));
            }
        };
        for k in car.idx..(car.idx + 3).min(car.route.len()) {
            let l = car.route[k];
            if k > car.idx && self.net.lanes[car.route[k - 1] as usize].is_beside(l) {
                break;
            }
            if let Some(hs) = self.peds.holds.get(&l) {
                for h in hs {
                    take(base + h.s, PED_BRAKE);
                }
            }
            // Someone actually in the road off a crosswalk: an emergency stop
            // if there is still room for one, else they are run into.
            if let Some(hs) = self.peds.watch.get(&l) {
                for h in hs {
                    take(base + h.s, PED_EMERGENCY_BRAKE);
                }
            }
            // Lights and sirens go through the red, but not through people.
            if let Some(&s) = self.peds.stop_lines.get(&l).filter(|_| !car.siren) {
                // Only while the way on is red: a car on green drives over.
                let red = car.route.get(k + 1).is_some_and(|&n| {
                    let nl = &self.net.lanes[n as usize];
                    nl.is_connector() && !nl.green
                });
                if red && car.drive.running != car.route.get(k + 1).copied() {
                    take(base + s, PED_BRAKE);
                }
            }
            base += self.net.lanes[l as usize].length;
            if base > PED_LOOKAHEAD {
                break;
            }
        }
        best
    }

    /// Cars driving over an occupied crossing at speed, after a substep.
    pub(super) fn crosswalk_conflicts(&mut self) {
        if self.peds.holds.is_empty() && self.peds.watch.is_empty() {
            return;
        }
        let mut found: Vec<(CarId, u32, f32)> = Vec::new();
        for (&lane, hs) in self.peds.holds.iter().chain(self.peds.watch.iter()) {
            let Some(l) = self.net.lanes.get(lane as usize) else { continue };
            for h in hs {
                for &cid in &l.cars {
                    let c = &self.cars[cid];
                    if c.s > h.s + 0.3 && c.s < h.s + h.depth + CAR_LENGTH * 0.5 && c.v > CONFLICT_SPEED && !c.crashed() {
                        found.push((cid, h.crossing, c.v));
                    }
                }
            }
        }
        for (cid, cr, v) in found {
            if self.peds.seen.iter().any(|x| x.0 == cid && x.1 == cr) {
                continue;
            }
            self.peds.seen.push((cid, cr, 0));
            self.peds.conflicts.push((cid, cr, v));
        }
    }

    /// [services] A vehicle with lights and sirens on `lane` short of the
    /// crossing at `s`, or on the lanes leading to it, within
    /// `SIREN_WARNING` metres (one already over the crossing does not count).
    pub fn siren_near(&self, lane: LaneId, s: f32) -> bool {
        let Some(l) = self.net.lanes.get(lane as usize) else { return false };
        if l.cars.iter().any(|&c| self.cars[c].siren && self.cars[c].s <= s + HOLD_DEPTH_MAX && s - self.cars[c].s < SIREN_WARNING) {
            return true;
        }
        let mut frontier: Vec<(LaneId, f32)> = l.prev.iter().map(|&p| (p, s)).collect();
        for _ in 0..3 {
            let mut next = Vec::new();
            for (p, ahead) in frontier {
                let pl = &self.net.lanes[p as usize];
                if pl.cars.iter().any(|&c| self.cars[c].siren && pl.length - self.cars[c].s + ahead < SIREN_WARNING) {
                    return true;
                }
                if pl.length + ahead < SIREN_WARNING {
                    next.extend(pl.prev.iter().map(|&q| (q, pl.length + ahead)));
                }
            }
            frontier = next;
        }
        false
    }

    /// Seconds before the first moving vehicle reaches the stretch
    /// `[s, s + depth]` of `lane`, looking back up to two lanes along the
    /// routes that lead onto it; 0 when one is on it. Standing vehicles
    /// (at a red, giving way) do not count. Lights and sirens count as four
    /// times nearer: walkers keep out of their way.
    pub fn time_to_crossing(&self, lane: LaneId, s: f32, depth: f32) -> f32 {
        self.time_to_crossing_above(lane, s, depth, 0.5)
    }

    /// [emergency] `time_to_crossing`, counting only vehicles moving at
    /// `min_v` or more: with the walk light on, a jam crawling across the
    /// crossing is something to weave through, not to wait for.
    pub fn time_to_crossing_above(&self, lane: LaneId, s: f32, depth: f32, min_v: f32) -> f32 {
        let siren = |c: &Car| if c.siren { 0.25 } else { 1.0 };
        let mut best = f32::INFINITY;
        let Some(l) = self.net.lanes.get(lane as usize) else { return best };
        for &cid in &l.cars {
            let c = &self.cars[cid];
            if c.v < min_v || c.s > s + depth + CAR_LENGTH {
                continue;
            }
            let t = if c.s >= s - 0.3 { 0.0 } else { (s - c.s) / c.v };
            best = best.min(t * siren(c));
        }
        // Feeders: cars whose route runs onto this lane next (or the one after).
        for &p in &l.prev {
            let pl = &self.net.lanes[p as usize];
            for &cid in &pl.cars {
                let c = &self.cars[cid];
                if c.v < min_v || c.route.get(c.idx + 1) != Some(&lane) {
                    continue;
                }
                best = best.min((pl.length - c.s + s) / c.v * siren(c));
            }
            if pl.length > PED_LOOKAHEAD {
                continue;
            }
            for &pp in &pl.prev {
                let ppl = &self.net.lanes[pp as usize];
                for &cid in &ppl.cars {
                    let c = &self.cars[cid];
                    if c.v < min_v || c.route.get(c.idx + 1) != Some(&p) || c.route.get(c.idx + 2) != Some(&lane) {
                        continue;
                    }
                    let d = ppl.length - c.s + pl.length + s;
                    if d < PED_LOOKAHEAD * 1.5 {
                        best = best.min(d / c.v * siren(c));
                    }
                }
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roads(cross: bool) -> RoadNetwork {
        let mut net = RoadNetwork::default();
        net.add_polyline(&[Vec2::new(-400.0, 0.0), Vec2::new(400.0, 0.0)], 1, 1);
        if cross {
            net.add_polyline(&[Vec2::new(0.0, -400.0), Vec2::new(0.0, 400.0)], 1, 1);
        }
        net
    }

    fn send(t: &mut Traffic, roads: &RoadNetwork, from: Vec2, to: Vec2) -> CarId {
        let (fs, _, _) = roads.nearest_street(from).unwrap();
        let (ts, _, _) = roads.nearest_street(to).unwrap();
        let (route, s0, s1) = t.net.find_route(fs, from, ts, to).expect("route");
        t.spawn_car(CitizenId::default(), None, route, s0, s1, 0)
    }

    #[test]
    fn off_a_crosswalk_drivers_only_brake_if_they_still_can_and_running_someone_down_counts() {
        let roads = roads(false);
        let w = LANE_WIDTH;
        let setup = || {
            let mut t = Traffic::default();
            t.rebuild(&roads);
            let car = send(&mut t, &roads, Vec2::new(-300.0, 0.5 * w), Vec2::new(300.0, 0.5 * w));
            let lane = t.cars[car].route[t.cars[car].idx];
            let hold = t.net.lanes[lane as usize].project(Vec2::new(0.0, 0.5 * w)).0 - 1.5;
            let watch: BTreeMap<LaneId, Vec<Hold>> = [(lane, vec![Hold { s: hold, depth: 3.0, crossing: 7 }])].into_iter().collect();
            (t, car, lane, hold, watch)
        };
        // Far off: an emergency stop short of the person in the road, and the
        // reason says there was no crosswalk.
        let (mut t, car, lane, hold, watch) = setup();
        for _ in 0..60 {
            t.set_pedestrians(BTreeMap::new(), Vec::new());
            t.set_unmarked(watch.clone());
            t.step(1.0, 4);
        }
        let c = &t.cars[car];
        assert!(c.route[c.idx] == lane && c.s <= hold + 0.05 && c.v < 0.1, "braked short of them: s {} hold {hold}", c.s);
        assert_eq!(t.stop_reason(car).key(), "jaywalker");
        // Too close and too fast to stop: through, and it counts.
        let (mut t, car, _, hold, watch) = setup();
        t.cars[car].s = hold - 6.0;
        t.cars[car].v = 13.0;
        for _ in 0..4 {
            t.set_pedestrians(BTreeMap::new(), Vec::new());
            t.set_unmarked(watch.clone());
            t.step(1.0, 4);
        }
        assert!(t.peds.conflicts.iter().any(|c| c.0 == car && c.1 == 7), "ran into them: {:?}", t.peds.conflicts);
    }

    #[test]
    fn a_driver_stops_for_someone_on_a_crossing_and_drives_on_after() {
        let roads = roads(false);
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let w = LANE_WIDTH;
        let car = send(&mut t, &roads, Vec2::new(-300.0, 0.5 * w), Vec2::new(300.0, 0.5 * w));
        let lane = t.cars[car].route[t.cars[car].idx];
        let hold = t.net.lanes[lane as usize].project(Vec2::new(0.0, 0.5 * w)).0 - 1.5;
        let holds: BTreeMap<LaneId, Vec<Hold>> = [(lane, vec![Hold { s: hold, depth: 3.0, crossing: 0 }])].into_iter().collect();
        for _ in 0..60 {
            t.set_pedestrians(holds.clone(), Vec::new());
            t.step(1.0, 4);
        }
        let c = &t.cars[car];
        assert!(c.route[c.idx] == lane && c.s <= hold + 0.05 && c.v < 0.1, "stopped short of the crossing: s {} v {} hold {hold}", c.s, c.v);
        assert!(t.peds.conflicts.is_empty(), "nobody was run over");
        t.set_pedestrians(BTreeMap::new(), Vec::new());
        let mut gone = false;
        for _ in 0..40 {
            gone |= !t.step(1.0, 4).is_empty() || t.cars[car].s > hold + 5.0;
        }
        assert!(gone, "drives on once the crossing is clear");
    }

    #[test]
    fn a_turning_car_gives_way_on_the_crosswalk_it_turns_into() {
        let roads = roads(true);
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let w = LANE_WIDTH;
        // Eastbound, turning right into the street going +y.
        let car = send(&mut t, &roads, Vec2::new(-200.0, 0.5 * w), Vec2::new(-0.5 * w, 200.0));
        let route = t.cars[car].route.clone();
        let k = route.iter().position(|&l| t.net.lanes[l as usize].is_connector()).expect("a turn");
        assert_eq!(t.net.lanes[route[k] as usize].turn, 1, "a right turn");
        let exit = route[k + 1];
        let holds: BTreeMap<LaneId, Vec<Hold>> = [(exit, vec![Hold { s: 0.8, depth: 3.0, crossing: 0 }])].into_iter().collect();
        let mut waited = false;
        for _ in 0..90 {
            t.set_pedestrians(holds.clone(), Vec::new());
            t.step(1.0, 4);
            let c = &t.cars[car];
            let on = c.route[c.idx];
            assert!(on != exit || c.s <= 0.85, "onto the crosswalk with someone on it: s {}", c.s);
            waited |= c.v < 0.1 && c.idx >= k;
        }
        assert!(waited, "the car waited for the walker");
        assert!(t.peds.conflicts.is_empty());
    }

    #[test]
    fn the_walk_light_shows_only_while_parallel_traffic_has_the_green() {
        let roads = roads(true);
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let w = LANE_WIDTH;
        let k = t.net.intersections.iter().position(|i| i.signalized).expect("a signalised junction");
        let n = t.net.intersections[k].approaches;
        let mut shown = vec![false; n];
        for step in 0..240 {
            if step % 5 == 0 {
                send(&mut t, &roads, Vec2::new(-300.0, 0.5 * w), Vec2::new(300.0, 0.5 * w));
                send(&mut t, &roads, Vec2::new(0.5 * w, 300.0), Vec2::new(0.5 * w, -300.0));
            }
            // Someone waits at every crosswalk.
            let calls: Vec<Vec<bool>> = t.net.intersections.iter().map(|i| vec![true; i.approaches]).collect();
            t.set_pedestrians(BTreeMap::new(), calls);
            for id in t.step(1.0, 4) {
                t.remove_car(id);
            }
            let inter = &t.net.intersections[k];
            for ap in 0..n {
                if t.peds.walk_shown(k, ap) {
                    shown[ap] = true;
                    // Nothing from this arm may drive over its crosswalk.
                    for &(l, a) in &inter.connectors {
                        assert!(a != ap || !t.net.lanes[l as usize].green, "walk across approach {ap} while it has a green");
                    }
                }
            }
        }
        assert!(shown.iter().all(|&s| s), "every crosswalk got its walk light: {shown:?}");
    }
}
