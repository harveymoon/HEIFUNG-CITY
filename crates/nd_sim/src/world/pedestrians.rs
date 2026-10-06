//! [pedestrians] Walkers on the move: along the path's right-hand side,
//! keeping their distance, waiting at the kerb for the walk light or a gap,
//! crossing (or jaywalking), and what happens when a car does not stop.
//! The rules are in `crate::pedestrians`; the lane side (walk lights,
//! drivers giving way) in `traffic/crosswalk.rs`.
//!
//! Every tick `step_citizens` calls `feet_begin`, then `step_walker` for each
//! walker, then `feet_end`, which hands the crossings in use and the walk
//! calls to the traffic model for its next step. Decisions draw from
//! `hash01`, never the world's random stream.

use super::*;
use crate::citizens::{driver_traits, DRUNK};
use crate::pedestrians::{gap_needed, hash01, jaywalk_chance, CrossingInfo, PedIncident, Walker, CROWD, CROWD_SPACE, KEEP_INCIDENTS, KEEP_RIGHT_MAX, KEEP_RIGHT_MIN, LAWFUL_GAP, PERSONAL_SPACE, SIDESTEP_MAX, HURT_SPEED};
use crate::traffic::{Hold, LaneId};
use crate::walk::{CrossingKind, CROSSWALK_HALF};
use std::collections::BTreeMap;

/// Cell of the walkers' spatial grid, metres.
const CELL: f32 = 2.0;
/// Depth of a crossing's hold along a lane.
const HOLD_DEPTH: f32 = CROSSWALK_HALF * 2.0 + 0.6;
/// Waiting walkers stand in rows of this many at the kerb.
const ROW: u32 = 3;
/// [services] Walkers on a crossing with lights and sirens coming hurry clear
/// at this many times their pace.
const SIREN_HURRY: f32 = 2.5;

/// Scratch for one tick of walking.
pub(super) struct Feet {
    /// Walkers by grid cell of where they stand now.
    grid: HashMap<(i32, i32), Vec<CitizenId>>,
    /// Walkers already waiting at each end of each crossing this tick.
    queue: HashMap<(u32, u32), u32>,
    /// Crossings someone is on, stepping onto, or waiting at (a zebra).
    occupied: Vec<(u32, CitizenId)>,
    /// Walk calls per intersection and approach.
    calls: Vec<Vec<bool>>,
    waiting: u32,
}

fn cell(p: Vec2) -> (i32, i32) {
    ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
}

fn path_key(path: &[Vec2]) -> (usize, u32, u32) {
    let first = path.first().map_or(0, |p| p.x.to_bits() ^ p.y.to_bits().rotate_left(16));
    let last = path.last().map_or(0, |p| p.x.to_bits() ^ p.y.to_bits().rotate_left(16));
    (path.len(), first, last)
}

fn walking(c: &Citizen) -> bool {
    c.mode == TravelMode::Walk && matches!(c.activity, Activity::Traveling { .. })
}

impl World {
    /// Before the walkers move: the day's counts, crossings matched to the
    /// lane network, last tick's conflicts, and the spatial grid.
    pub(super) fn feet_begin(&mut self) -> Feet {
        if self.clock.minute_of_day() == 0 {
            let p = &mut self.peds;
            p.jaywalkers_today = 0;
            p.near_misses_today = 0;
            p.collisions_today = 0;
            let citizens = &self.citizens;
            p.walkers.retain(|id, _| citizens.contains_key(*id));
        }
        self.refresh_crossings();
        self.handle_ped_conflicts();
        let mut grid: HashMap<(i32, i32), Vec<CitizenId>> = HashMap::new();
        for (id, c) in self.citizens.iter() {
            if walking(c) {
                grid.entry(cell(c.pos)).or_default().push(id);
            }
        }
        let calls = self.traffic.net.intersections.iter().map(|i| vec![false; i.approaches]).collect();
        Feet { grid, queue: HashMap::new(), occupied: Vec::new(), calls, waiting: 0 }
    }

    /// After the walkers moved: the crossings in use become stop points for
    /// drivers, and the walk calls go to the signals.
    pub(super) fn feet_end(&mut self, feet: Feet) {
        let mut occ = feet.occupied;
        occ.sort();
        occ.dedup();
        let mut holds: BTreeMap<LaneId, Vec<Hold>> = BTreeMap::new();
        let mut last = u32::MAX;
        for &(cr, _) in &occ {
            if cr == last {
                continue;
            }
            last = cr;
            if let Some(info) = self.peds.crossings.get(cr as usize) {
                for &(lane, s) in &info.lanes {
                    holds.entry(lane).or_default().push(Hold { s, depth: HOLD_DEPTH, crossing: cr });
                }
            }
        }
        self.peds.occupants = occ;
        self.peds.waiting_now = feet.waiting;
        self.traffic.set_pedestrians(holds, feet.calls);
    }

    /// Match the walk graph's crossings to the lane network: which are
    /// signalised (and across which approach), and where each lane meets
    /// them. Also the stop lines before signalised crosswalks.
    fn refresh_crossings(&mut self) {
        let key = (self.walk.version, self.traffic.net.version);
        if self.peds.key == key && self.peds.crossings.len() == self.walk.crossings.len() {
            return;
        }
        self.peds.key = key;
        let net = &self.traffic.net;
        let roads = &self.roads;
        let mut stop_lines = HashMap::new();
        let infos: Vec<CrossingInfo> = self
            .walk
            .crossings
            .iter()
            .map(|c| {
                let signal = c.node.filter(|_| c.kind == CrossingKind::Junction).and_then(|n| {
                    let k = *net.node_intersection.get(n.0 as usize)?;
                    let inter = net.intersections.get(k)?;
                    if !inter.signalized || roads.nodes.len() <= n.0 as usize {
                        return None;
                    }
                    let ap = roads.node(n).segments.iter().copied().filter(|&s| roads.segment(s).alive && roads.segment(s).is_street()).position(|s| s == c.segment)?;
                    (ap < inter.approaches).then_some((k as u32, ap as u32))
                });
                let lanes: Vec<(LaneId, f32)> = net
                    .segment_lanes
                    .get(&c.segment)
                    .map(|ls| ls.iter().map(|&l| (l, (net.lanes[l as usize].project(c.mid).0 - CROSSWALK_HALF - 0.3).max(0.0))).collect())
                    .unwrap_or_default();
                if let (Some(_), Some(n)) = (signal, c.node) {
                    for &(l, s) in &lanes {
                        if net.lanes[l as usize].end_node == n {
                            stop_lines.insert(l, s);
                        }
                    }
                }
                CrossingInfo { signal, lanes }
            })
            .collect();
        self.peds.crossings = infos;
        self.traffic.peds.stop_lines = stop_lines;
    }

    /// [services] A siren vehicle is coming up to crossing `cr`: walkers
    /// keep off it and those on it hurry clear.
    fn siren_at_crossing(&self, cr: u32) -> bool {
        self.peds.crossings.get(cr as usize).is_some_and(|info| info.lanes.iter().any(|&(l, s)| self.traffic.siren_near(l, s)))
    }

    /// Seconds before the first moving vehicle reaches crossing `cr`.
    fn crossing_gap(&self, cr: u32) -> f32 {
        let Some(info) = self.peds.crossings.get(cr as usize) else { return f32::INFINITY };
        info.lanes.iter().map(|&(l, s)| self.traffic.time_to_crossing(l, s, HOLD_DEPTH)).fold(f32::INFINITY, f32::min)
    }

    /// On the way to a shift that has already started.
    pub(super) fn late_for_work(&self, id: CitizenId, purpose: Purpose) -> bool {
        if purpose != Purpose::Work {
            return false;
        }
        let c = &self.citizens[id];
        let hour = self.clock.hour() as u8;
        c.work.and_then(|w| self.buildings.get(w)).is_some_and(|b| b.shift_for(c.seed).contains(hour))
    }

    /// How likely this citizen is to jaywalk right now.
    fn jaywalk_chance_of(&self, id: CitizenId, late: bool) -> f32 {
        let c = &self.citizens[id];
        let (caution, hedonism) = self.peds.walkers.get(&id).map(|w| (w.caution, w.hedonism)).unwrap_or_else(|| driver_traits(c.seed));
        jaywalk_chance(c.age, caution, hedonism, c.drunk, late)
    }

    /// A walking route for this citizen: a jaywalker may plan to cut
    /// straight over the street mid-block.
    pub(super) fn walk_route_for(&self, id: CitizenId, from: Vec2, to: Vec2, purpose: Purpose) -> Option<(Vec<Vec2>, Vec<u32>)> {
        let p = self.jaywalk_chance_of(id, self.late_for_work(id, purpose));
        let jay = p > 0.0 && hash01(self.citizens[id].seed, self.clock.tick, 0x7a17) < p;
        self.walk.route_with(from, to, jay)
    }

    /// May this walker step onto crossing `cr` now? `Some(against)` to go
    /// (`against`: against the light, or mid-block), `None` to wait.
    fn may_cross(&self, id: CitizenId, w: &Walker, cr: u32, since: u64, late: bool) -> Option<bool> {
        let crossing = &self.walk.crossings[cr as usize];
        let signal = self.peds.crossings.get(cr as usize).and_then(|i| i.signal);
        let lawful = match (crossing.kind, signal) {
            (CrossingKind::Jay, _) => false,
            (_, Some((k, ap))) => self.traffic.peds.walk_shown(k as usize, ap as usize),
            _ => true,
        };
        let gap = self.crossing_gap(cr);
        if lawful {
            // Look first: nobody too close to stop.
            return (gap >= LAWFUL_GAP).then_some(false);
        }
        if crossing.kind != CrossingKind::Jay && !w.bold {
            return None;
        }
        let c = &self.citizens[id];
        // Drink makes a gap look bigger than it is.
        let seen = gap * (1.0 + (c.drunk - DRUNK * 0.5).max(0.0) * 3.0 * hash01(c.seed, since, cr as u64));
        (seen >= gap_needed(w.caution, late)).then_some(true)
    }

    /// One tick of walking for `id` (on foot, `Activity::Traveling`): true
    /// once the path is done.
    pub(super) fn step_walker(&mut self, id: CitizenId, feet: &mut Feet) -> bool {
        let tick = self.clock.tick;
        let speed = self.config.walk_speed;
        let (seed, start) = {
            let c = &mut self.citizens[id];
            c.prev_pos = c.pos;
            c.prev_heading = c.heading;
            (c.seed, c.pos)
        };
        let late = match &self.citizens[id].activity {
            Activity::Traveling { purpose, .. } => self.late_for_work(id, *purpose),
            _ => false,
        };
        let mut act = std::mem::replace(&mut self.citizens[id].activity, Activity::Idle);
        let Activity::Traveling { path, idx, .. } = &mut act else {
            self.citizens[id].activity = act;
            return false;
        };
        let key = path_key(path);
        let mut w = match self.peds.walkers.get(&id) {
            Some(w) => *w,
            None => {
                let (caution, hedonism) = driver_traits(seed);
                let keep = KEEP_RIGHT_MIN + (KEEP_RIGHT_MAX - KEEP_RIGHT_MIN) * hash01(seed, 0x5eed, 1);
                Walker { key: (usize::MAX, 0, 0), base: start, side: 0.0, keep, crossing: None, waiting: None, bold: false, against: false, caution, hedonism }
            }
        };
        if w.key != key {
            // A new journey: start from where they stand.
            w.key = key;
            w.base = start;
            w.side = 0.0;
            w.crossing = None;
            w.waiting = None;
            w.against = false;
        }
        let mut heading = self.citizens[id].heading;
        let mut dir = Vec2::from_angle(heading);
        let mut budget = speed;
        let mut hurried = false;
        let mut wait_at: Option<(u32, Vec2, Vec2)> = None;
        let mut leg_start = if *idx > 0 { path[*idx - 1] } else { w.base };
        let mut guard = 0;
        while budget > 1e-4 && *idx < path.len() && guard < 256 {
            guard += 1;
            if *idx > 0 {
                if let Some(cr) = self.walk.crossing_between(path[*idx - 1], path[*idx]) {
                    if w.crossing != Some((cr, *idx)) {
                        if w.base.distance(path[*idx - 1]) > 0.5 {
                            // Already on the way over (after a reload).
                            w.crossing = Some((cr, *idx));
                        } else {
                            let since = match w.waiting {
                                Some((c, i, t)) if c == cr && i == *idx => t,
                                _ => {
                                    let p = self.jaywalk_chance_of(id, late);
                                    w.bold = p > 0.0 && hash01(seed, tick, cr as u64) < p;
                                    tick
                                }
                            };
                            // Lights and sirens coming: everyone stays on the kerb.
                            let cross = if self.siren_at_crossing(cr) { None } else { self.may_cross(id, &w, cr, since, late) };
                            match cross {
                                Some(against) => {
                                    w.crossing = Some((cr, *idx));
                                    w.waiting = None;
                                    w.against = against;
                                    if against {
                                        self.peds.jaywalkers_today += 1;
                                        self.peds.jaywalks_total += 1;
                                    }
                                }
                                None => {
                                    w.waiting = Some((cr, *idx, since));
                                    wait_at = Some((cr, path[*idx - 1], path[*idx]));
                                    break;
                                }
                            }
                        }
                    }
                    // On the crossing with a siren coming: hurry clear.
                    if self.siren_at_crossing(cr) && !hurried {
                        budget *= SIREN_HURRY;
                        hurried = true;
                    }
                    feet.occupied.push((cr, id));
                    if let Some((k, ap)) = self.peds.crossings.get(cr as usize).and_then(|i| i.signal) {
                        if let Some(x) = feet.calls.get_mut(k as usize).and_then(|v| v.get_mut(ap as usize)) {
                            *x = true;
                        }
                    }
                }
            }
            let target = path[*idx];
            let d = w.base.distance(target);
            if d > 1e-4 {
                dir = (target - w.base) / d;
            }
            if d <= budget {
                w.base = target;
                leg_start = target;
                *idx += 1;
                budget -= d;
                if w.crossing.is_some_and(|(_, i)| i + 1 == *idx) {
                    w.crossing = None;
                    w.against = false;
                }
            } else {
                w.base += dir * budget;
                budget = 0.0;
            }
        }
        let arrived = *idx >= path.len();
        let moved = speed - budget;
        self.citizens[id].activity = act;

        let pos = if let Some((cr, kerb, far)) = wait_at {
            // At the kerb, in a small cluster facing the street.
            let u = (far - kerb).normalize_or_zero();
            let v = Vec2::new(-u.y, u.x);
            let slot = feet.queue.entry((cr, kerb.x.to_bits() ^ kerb.y.to_bits().rotate_left(16))).or_insert(0);
            let n = *slot;
            *slot += 1;
            feet.waiting += 1;
            let (row, col) = ((n / ROW) as f32, (n % ROW) as f32 - 1.0);
            heading = u.y.atan2(u.x);
            let crossing = &self.walk.crossings[cr as usize];
            let signal = self.peds.crossings.get(cr as usize).and_then(|i| i.signal);
            if let Some((k, ap)) = signal {
                if let Some(x) = feet.calls.get_mut(k as usize).and_then(|v| v.get_mut(ap as usize)) {
                    *x = true;
                }
            } else if crossing.lawful() && !self.siren_at_crossing(cr) {
                // About to step onto a zebra: drivers give way (but not to
                // someone standing back for lights and sirens).
                feet.occupied.push((cr, id));
            }
            kerb + u * (0.9 - 0.7 * row) + v * col * 0.75
        } else {
            heading = dir.y.atan2(dir.x);
            let right = Vec2::new(-dir.y, dir.x);
            w.side += (w.keep - w.side) * 0.35;
            let mut p = w.base + right * w.side;
            // Personal space: fall in behind someone going the same way
            // (or standing), step aside for anyone else.
            let (cx, cy) = cell(p);
            let mut near: Vec<(Vec2, Vec2, bool)> = Vec::new();
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let Some(list) = feet.grid.get(&(cx + dx, cy + dy)) else { continue };
                    for &o in list {
                        if o == id {
                            continue;
                        }
                        let Some(oc) = self.citizens.get(o) else { continue };
                        let standing = self.peds.walkers.get(&o).is_some_and(|ow| ow.waiting.is_some());
                        near.push((oc.pos, Vec2::from_angle(oc.heading), standing));
                    }
                }
            }
            if !near.is_empty() && !arrived {
                let crowd = near.iter().filter(|x| x.0.distance(p) < 3.0).count() > CROWD;
                let r = if crowd { CROWD_SPACE } else { PERSONAL_SPACE };
                let mut back_room = moved.min(w.base.distance(leg_start));
                // A few passes: stepping clear of one may bring another close.
                for _ in 0..3 {
                    let mut clear = true;
                    for &(op, odir, standing) in &near {
                        let d = op.distance(p);
                        if d >= r {
                            continue;
                        }
                        clear = false;
                        let along = (op - p).dot(dir);
                        let lateral = (op - p).dot(right);
                        let same_way = standing || odir.dot(dir) > 0.3;
                        // No room to step aside (abreast already): fall in behind.
                        let boxed = w.side + (r - d) > SIDESTEP_MAX && along > -0.05;
                        if (along > 0.05 || boxed) && same_way && back_room > 0.0 {
                            let back = ((r * r - lateral * lateral).max(0.0).sqrt() + along + 0.02).min(back_room);
                            w.base -= dir * back;
                            back_room -= back;
                        } else {
                            // Step away from them: right unless they are on our right.
                            let away = if lateral > 0.05 { -1.0 } else { 1.0 };
                            let to = w.side + away * (r - d);
                            w.side = if to.abs() <= SIDESTEP_MAX { to } else { w.side - away * (r - d) }.clamp(-SIDESTEP_MAX, SIDESTEP_MAX);
                        }
                        p = w.base + right * w.side;
                    }
                    if clear {
                        break;
                    }
                }
            }
            p
        };
        {
            let c = &mut self.citizens[id];
            c.pos = pos;
            c.heading = heading;
        }
        // Keep the grid current for those who move after.
        let (from, to) = (cell(start), cell(pos));
        if from != to || arrived {
            if let Some(list) = feet.grid.get_mut(&from) {
                list.retain(|&o| o != id);
            }
            if !arrived {
                feet.grid.entry(to).or_default().push(id);
            }
        }
        if arrived {
            self.peds.walkers.remove(&id);
        } else {
            self.peds.walkers.insert(id, w);
        }
        arrived
    }

    /// Cars that went over a crossing with someone on it: a near miss, or
    /// (faster, or unluckier) someone hurt, the driver stopped, 911 called
    /// and an ambulance on its way.
    fn handle_ped_conflicts(&mut self) {
        let conflicts = std::mem::take(&mut self.traffic.peds.conflicts);
        let tick = self.clock.tick;
        for (car, cr, speed) in conflicts {
            let ped = self.peds.occupants.iter().filter(|x| x.0 == cr).map(|x| x.1).find(|p| self.citizens.get(*p).is_some_and(walking) && self.peds.walkers.get(p).is_some_and(|w| w.crossing.is_some_and(|x| x.0 == cr)));
            let Some(ped) = ped else { continue };
            let Some(driver) = self.traffic.cars.get(car).map(|c| c.citizen) else { continue };
            let against = self.peds.walkers.get(&ped).is_some_and(|w| w.against);
            let (pseed, pdrunk, pos) = {
                let c = &self.citizens[ped];
                (c.seed, c.drunk, c.pos)
            };
            let chance = if speed < HURT_SPEED { 0.0 } else { ((speed - HURT_SPEED) / 10.0).clamp(0.0, 1.0) * 0.3 + if pdrunk >= DRUNK { 0.1 } else { 0.0 } };
            let hurt = hash01(pseed, tick, cr as u64 ^ 0x4817) < chance;
            let place = self.walk_place(pos);
            if hurt {
                self.peds.collisions_today += 1;
                self.peds.collisions_total += 1;
                if let Some(c) = self.traffic.cars.get_mut(car) {
                    c.v = 0.0;
                    c.hold = c.hold.max(150.0);
                }
                {
                    let c = &mut self.citizens[ped];
                    c.activity = Activity::Idle;
                    c.inside = None;
                    c.prev_pos = c.pos;
                    c.mood = (c.mood - 0.2).max(0.0);
                    c.next_decision_tick = tick + crate::services::MEDICAL_GIVE_UP + 120;
                    c.thought = "Hit by a car. Everything hurts.".to_string();
                    c.remember(tick, format!("Hit by a car crossing the street by {place}."));
                }
                self.peds.walkers.remove(&ped);
                let drivers: Vec<CitizenId> = [driver].into_iter().filter(|d| self.citizens.contains_key(*d)).collect();
                if let Some(d) = drivers.first().copied() {
                    let c = &mut self.citizens[d];
                    c.remember(tick, format!("Hit someone crossing the street by {place}."));
                    c.mood = (c.mood - 0.15).max(0.0);
                }
                let at_fault = if against { None } else { drivers.first().copied() };
                let caller = self.ped_witness(pos, &[ped, driver]);
                if let Some(near) = self.ped_building_near(pos) {
                    self.report_crash(pos, near, &drivers, &[ped], at_fault, caller);
                }
            } else {
                self.peds.near_misses_today += 1;
                self.peds.near_misses_total += 1;
                let c = &mut self.citizens[ped];
                c.remember(tick, format!("A car nearly hit me crossing by {place}."));
                c.mood = (c.mood - 0.02).max(0.0);
            }
            self.peds.recent.push(PedIncident { tick, pos, citizen: ped, speed, hurt, jaywalking: against });
            if self.peds.recent.len() > KEEP_INCIDENTS {
                self.peds.recent.remove(0);
            }
        }
    }

    fn ped_building_near(&self, p: Vec2) -> Option<BuildingId> {
        self.buildings.iter().filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (id, l.center.distance_squared(p)))).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(id, _)| id)
    }

    fn walk_place(&self, p: Vec2) -> String {
        self.ped_building_near(p).and_then(|b| self.buildings.get(b)).map(|b| b.name.clone()).unwrap_or_else(|| "the street".to_string())
    }

    /// Somebody close enough to see it and call it in.
    fn ped_witness(&self, p: Vec2, except: &[CitizenId]) -> Option<CitizenId> {
        self.citizens
            .iter()
            .filter(|(id, c)| !except.contains(id) && !c.asleep && c.age >= 12 && c.inside.is_none() && !matches!(c.activity, Activity::OnTrain { .. } | Activity::Arriving))
            .map(|(id, c)| (id, c.pos.distance(p)))
            .filter(|(_, d)| *d < 300.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mind::IdleMind;
    use crate::pedestrians::CAUTIOUS;

    fn square(x0: f32, y0: f32, size: f32) -> Vec<Vec2> {
        vec![Vec2::new(x0, y0), Vec2::new(x0 + size, y0), Vec2::new(x0 + size, y0 + size), Vec2::new(x0, y0 + size)]
    }

    fn run(w: &mut World, ticks: usize) {
        let mut minds = IdleMind;
        for _ in 0..ticks {
            w.tick(&mut minds);
        }
    }

    /// A signalised crossroads with homes in two corners, lived in.
    fn town() -> World {
        let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
        w.add_road(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        w.add_road(&[Vec2::new(0.0, -300.0), Vec2::new(0.0, 300.0)], 1, 1);
        w.add_zone(square(10.0, 10.0, 180.0), LandUse::Residential);
        w.add_zone(square(-190.0, 10.0, 180.0), LandUse::Residential);
        run(&mut w, 8 * 60);
        let sites: Vec<BuildingId> = w.buildings.iter().filter(|(_, b)| b.construction.is_some()).map(|(id, _)| id).collect();
        for id in sites {
            w.finish_construction(id);
        }
        let mut guard = 0;
        while w.citizens.len() < 24 && guard < 24 {
            run(&mut w, 60);
            guard += 1;
        }
        assert!(w.citizens.len() >= 24, "people moved in: {}", w.citizens.len());
        w
    }

    fn seeds(f: impl Fn(f32, f32) -> bool, n: usize) -> Vec<u64> {
        (1u64..)
            .filter(|&s| {
                let (c, h) = driver_traits(s);
                f(c, h)
            })
            .take(n)
            .collect()
    }

    /// The crossroads' first signalised crosswalk: crossing, intersection,
    /// approach, and its two kerbs.
    fn crosswalk(w: &World) -> (u32, usize, usize, Vec2, Vec2) {
        let (cr, info) = w.peds.crossings.iter().enumerate().find(|(_, i)| i.signal.is_some()).expect("a signalised crosswalk");
        let (k, ap) = info.signal.unwrap();
        let c = &w.walk.crossings[cr];
        (cr as u32, k as usize, ap as usize, w.walk.nodes[c.a as usize].pos, w.walk.nodes[c.b as usize].pos)
    }

    /// Set a citizen walking `path` from its start, with this seed's traits.
    fn walker(w: &mut World, id: CitizenId, seed: u64, path: Vec<Vec2>) {
        w.peds.walkers.remove(&id);
        let c = &mut w.citizens[id];
        c.seed = seed;
        c.age = 30;
        c.drunk = 0.0;
        c.inside = None;
        c.mode = TravelMode::Walk;
        c.pos = path[0];
        c.prev_pos = path[0];
        c.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path, idx: 0 };
        c.next_decision_tick = u64::MAX;
    }

    /// Everyone stays indoors and decides nothing.
    fn quiet(w: &mut World) {
        for c in w.citizens.values_mut() {
            if let Some(h) = c.home {
                c.activity = Activity::AtHome;
                c.inside = Some(h);
            }
            c.next_decision_tick = u64::MAX;
        }
    }

    fn until_dont_walk(w: &mut World, k: usize, ap: usize) {
        let mut guard = 0;
        while w.traffic.peds.walk_shown(k, ap) && guard < 200 {
            run(w, 1);
            guard += 1;
        }
        assert!(!w.traffic.peds.walk_shown(k, ap));
    }

    #[test]
    fn a_walker_waits_at_the_kerb_for_the_walk_light() {
        let mut w = town();
        quiet(&mut w);
        run(&mut w, 1);
        let (cr, k, ap, a, b) = crosswalk(&w);
        let sid = w.walk.crossings[cr as usize].segment;
        let hw = w.roads.segment(sid).half_width();
        let (s0, s1) = w.roads.segment_points(sid);
        until_dont_walk(&mut w, k, ap);
        let id = w.citizens.keys().next().unwrap();
        walker(&mut w, id, seeds(|c, _| c > 0.85, 1)[0], vec![a, b]);
        let (mut waited, mut crossed) = (0, false);
        for _ in 0..200 {
            run(&mut w, 1);
            let Some(st) = w.peds.walkers.get(&id).copied() else { break };
            let pos = w.citizens[id].pos;
            if st.waiting.is_some() {
                waited += 1;
                let off = crate::geom::project_on_segment(pos, s0, s1).1.distance(pos);
                assert!(off > hw + 0.3, "waits on the pavement, not in the road: {off} from the centre line, half width {hw}");
            }
            if st.crossing.is_some() && !crossed {
                crossed = true;
                assert!(w.traffic.peds.walk_shown(k, ap), "stepped out on the walk light");
                assert!(!st.against);
            }
        }
        assert!(waited >= 1, "had to wait");
        assert!(crossed, "got across");
        assert!(!matches!(w.citizens[id].activity, Activity::Traveling { .. }), "arrived");
        assert_eq!(w.peds.jaywalkers_today, 0);
    }

    /// Walkers with these seeds put at the kerb on don't walk, one after
    /// another, on an empty road: how many went against the light.
    fn red_light_trials(seeds: &[u64]) -> u32 {
        let mut w = town();
        quiet(&mut w);
        run(&mut w, 1);
        let (_, k, ap, a, b) = crosswalk(&w);
        let id = w.citizens.keys().next().unwrap();
        let mut against = 0;
        for &seed in seeds {
            until_dont_walk(&mut w, k, ap);
            walker(&mut w, id, seed, vec![a, b]);
            run(&mut w, 1);
            if w.peds.walkers.get(&id).is_some_and(|s| s.crossing.is_some() && s.against) {
                against += 1;
                assert!(!w.traffic.peds.walk_shown(k, ap));
            }
            w.citizens[id].activity = Activity::Idle;
            w.peds.walkers.remove(&id);
            run(&mut w, 1);
        }
        against
    }

    #[test]
    fn cautious_people_never_jaywalk_and_rebels_sometimes_do() {
        assert_eq!(red_light_trials(&seeds(|c, _| c > CAUTIOUS, 30)), 0, "the cautious wait for the light");
        let bold = red_light_trials(&seeds(|c, h| c < 0.15 && h > 0.8, 30));
        // Each trial's boldness is rolled from the tick it starts at, which
        // follows the signal timing and so the town's traffic: a sample, so
        // a band rather than a rate.
        assert!(bold >= 3 && bold < 30, "rebels sometimes cross against the light on an empty road: {bold} of 30");
    }

    #[test]
    fn walkers_keep_their_distance() {
        let mut w = town();
        quiet(&mut w);
        // Two groups along one pavement, one each way, all setting off together.
        let route = w.walk.route(Vec2::new(-250.0, -8.0), Vec2::new(-30.0, -8.0)).expect("a pavement route").0;
        let back: Vec<Vec2> = route.iter().rev().copied().collect();
        let ids: Vec<CitizenId> = w.citizens.keys().take(24).collect();
        for (n, &id) in ids.iter().enumerate() {
            let seed = w.citizens[id].seed;
            walker(&mut w, id, seed, if n % 2 == 0 { route.clone() } else { back.clone() });
        }
        let mut close: HashMap<(usize, usize), u32> = HashMap::new();
        let (mut worst, mut late_close) = (0, 0);
        for tick in 0..150 {
            run(&mut w, 1);
            let pos: Vec<Option<Vec2>> = ids.iter().map(|&id| matches!(w.citizens[id].activity, Activity::Traveling { .. }).then(|| w.citizens[id].pos)).collect();
            for i in 0..ids.len() {
                for j in i + 1..ids.len() {
                    let near = matches!((pos[i], pos[j]), (Some(p), Some(q)) if p.distance(q) < 0.5);
                    let e = close.entry((i, j)).or_insert(0);
                    *e = if near { *e + 1 } else { 0 };
                    worst = worst.max(*e);
                    if tick > 10 && *e > 2 {
                        late_close += 1;
                    }
                }
            }
        }
        // A dozen leaving one doorway at once take a few ticks to string out.
        assert!(worst <= 4, "two walkers stayed within 0.5 m of each other for {worst} ticks");
        assert!(late_close == 0, "walkers already strung out came within 0.5 m again for long: {late_close}");
        assert!(ids.iter().all(|&id| !matches!(w.citizens[id].activity, Activity::Traveling { .. })), "everyone got there");
    }

    #[test]
    fn walking_is_deterministic() {
        let trace = || {
            let mut w = town();
            run(&mut w, 300);
            let mut h: Vec<(u32, u32)> = w.citizens.values().map(|c| (c.pos.x.to_bits(), c.pos.y.to_bits())).collect();
            h.push((w.peds.jaywalks_total as u32, w.peds.near_misses_total as u32));
            h
        };
        assert_eq!(trace(), trace());
    }
}
