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
use crate::pedestrians::{ButtonLight, gap_needed, hash01, jaywalk_chance, CrossingInfo, PedIncident, Walker, CROWD, CROWD_SPACE, CRUSH_SPILL, JAM_CRAWL, KEEP_INCIDENTS, KEEP_SPREAD, KERB_MARGIN, LAWFUL_GAP, OPEN_ROOM, PACE, PERSONAL_SPACE, SIDESTEP_MAX, HURT_SPEED};
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
/// [walk-crossings] A jaywalker waiting this many ticks at the kerb for a
/// gap in the traffic gives up and goes round by a crossing.
const JAY_GIVE_UP: u64 = 5;

/// Scratch for one tick of walking.
pub(super) struct Feet {
    /// Walkers by grid cell of where they stand now.
    grid: HashMap<(i32, i32), Vec<CitizenId>>,
    /// [cyclists] Riders stopped at a light or for a walker.
    pub(super) riders_waiting: u32,
    /// Walkers already waiting at each end of each crossing this tick.
    queue: HashMap<(u32, u32), u32>,
    /// Crossings someone is on, stepping onto, or waiting at (a zebra).
    pub(super) occupied: Vec<(u32, CitizenId)>,
    /// Walk calls per intersection and approach.
    pub(super) calls: Vec<Vec<bool>>,
    waiting: u32,
    /// [tools] Push-button crossings someone pressed the button of.
    presses: Vec<u32>,
}

fn cell(p: Vec2) -> (i32, i32) {
    ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
}

fn path_key(path: &[Vec2]) -> (usize, u32, u32) {
    let first = path.first().map_or(0, |p| p.x.to_bits() ^ p.y.to_bits().rotate_left(16));
    let last = path.last().map_or(0, |p| p.x.to_bits() ^ p.y.to_bits().rotate_left(16));
    (path.len(), first, last)
}

/// Where the leg ending at path index `idx` starts.
fn leg_start_of(path: &[Vec2], idx: usize, base: Vec2) -> Vec2 {
    if idx > 0 { path[idx - 1] } else { base }
}

fn walking(c: &Citizen) -> bool {
    c.mode == TravelMode::Walk && matches!(c.activity, Activity::Traveling { .. })
}

impl World {
    /// Before the walkers move: the day's counts, crossings matched to the
    /// lane network, last tick's conflicts, and the spatial grid.
    pub(super) fn feet_begin(&mut self) -> Feet {
        crate::prof_scope!("feet_begin");
        if self.clock.minute_of_day() == 0 {
            let p = &mut self.peds;
            p.jaywalkers_today = 0;
            p.near_misses_today = 0;
            p.collisions_today = 0;
            p.red_lights_run_today = 0;
            let citizens = &self.citizens;
            p.walkers.retain(|id, _| citizens.contains_key(*id));
            p.riders.retain(|id, _| citizens.contains_key(*id));
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
        if self.peds.buttons_key != self.walk.version {
            self.peds.buttons.clear();
            self.peds.buttons_key = self.walk.version;
        }
        // [walk-crossings] The survey counts by crossing index: a new graph
        // starts it afresh.
        if self.peds.uses_key != self.walk.version {
            self.peds.uses.clear();
            self.peds.uses_key = self.walk.version;
        }
        Feet { grid, riders_waiting: 0, queue: HashMap::new(), occupied: Vec::new(), calls, waiting: 0, presses: Vec::new() }
    }

    /// After the walkers moved: the crossings in use become stop points for
    /// drivers, and the walk calls go to the signals.
    pub(super) fn feet_end(&mut self, feet: Feet) {
        crate::prof_scope!("feet_end");
        let mut occ = feet.occupied;
        occ.sort();
        occ.dedup();
        // Drivers stop only for painted crossings; unmarked ones in use are
        // watched for cars going over someone.
        let mut holds: BTreeMap<LaneId, Vec<Hold>> = BTreeMap::new();
        let mut watch: BTreeMap<LaneId, Vec<Hold>> = BTreeMap::new();
        let mut last = u32::MAX;
        for &(cr, _) in &occ {
            if cr == last {
                continue;
            }
            last = cr;
            let marked = self.walk.crossings.get(cr as usize).is_some_and(|c| c.marked());
            if let Some(info) = self.peds.crossings.get(cr as usize) {
                let into = if marked { &mut holds } else { &mut watch };
                for &(lane, s) in &info.lanes {
                    into.entry(lane).or_default().push(Hold { s, depth: HOLD_DEPTH, crossing: cr });
                }
            }
        }
        // [tools] Push-button crossings: a press starts the cycle; while
        // the light is red, traffic stops before the crossing whether or
        // not anyone is on it yet.
        let dt = self.config.traffic_seconds_per_tick;
        for &cr in &feet.presses {
            let light = self.peds.buttons.entry(cr).or_default();
            let was = *light;
            *light = light.press();
            if was == ButtonLight::Idle {
                self.peds.button_presses_total += 1;
            }
        }
        let mut reds = 0;
        for (&cr, light) in self.peds.buttons.iter_mut() {
            if light.step(dt) {
                reds += 1;
            }
            if light.red() {
                if let Some(info) = self.peds.crossings.get(cr as usize) {
                    for &(lane, s) in &info.lanes {
                        let v = holds.entry(lane).or_default();
                        if !v.iter().any(|h| h.crossing == cr) {
                            v.push(Hold { s, depth: HOLD_DEPTH, crossing: cr });
                        }
                    }
                }
            }
        }
        self.peds.buttons.retain(|_, l| *l != ButtonLight::Idle);
        self.peds.button_reds_total += reds;
        self.peds.occupants = occ;
        self.peds.waiting_now = feet.waiting;
        self.peds.riders_waiting_now = feet.riders_waiting;
        self.traffic.set_pedestrians(holds, feet.calls);
        self.traffic.set_unmarked(watch);
    }

    /// Match the walk graph's crossings to the lane network: which are
    /// signalised (and across which approach), and where each lane meets
    /// them. Also the stop lines before signalised crosswalks.
    /// [tools] `refresh_crossings` now, after junction signals or the
    /// crossings changed without a rebuild of either network.
    pub(super) fn refresh_crossings_now(&mut self) {
        self.peds.key = (u64::MAX, u64::MAX);
        self.refresh_crossings();
    }

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
                // [footbridge] A crosswalk its footbridge replaced has no
                // walk light (nobody calls the phase) and no stop line.
                // [walk-crossings] A slip road's mouth has walk lights when
                // its junction has signals (and no button of its own), and
                // only an off-ramp's: its traffic waits at the red while the
                // walkers cross. Traffic turns into an on-ramp from every
                // approach, on every green, so its mouth has no safe walk
                // light (G11: walkers there held up the whole junction).
                let off_ramp_only = |n: NodeId| {
                    roads.try_segment(c.segment).is_some_and(|sg| {
                        let (arriving, leaving) = if sg.b == n { (sg.lanes_fwd, sg.lanes_bwd) } else { (sg.lanes_bwd, sg.lanes_fwd) };
                        arriving > 0 && leaving == 0
                    })
                };
                let lit = c.kind == CrossingKind::Junction || (c.ramp && !c.button && c.node.is_some_and(off_ramp_only));
                let signal = c.node.filter(|_| lit && !c.closed).and_then(|n| {
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
        // [walk-crossings] A slip road's mouth is open to walkers only as a
        // proper crossing: with the junction's walk lights (then marked, so
        // drivers turning in give way), a push button or a footbridge.
        for (c, info) in self.walk.crossings.iter_mut().zip(&infos) {
            if c.ramp && !c.closed && !c.button {
                c.shut = info.signal.is_none();
                c.kind = if c.shut { CrossingKind::Informal } else { CrossingKind::Junction };
            }
        }
        self.peds.crossings = infos;
        let nodes = &self.walk.nodes;
        self.peds.cross_grid = crate::geom::Grid::new(
            16.0,
            self.walk.crossings.iter().map(|c| {
                let (a, b) = (nodes[c.a as usize].pos, nodes[c.b as usize].pos);
                (a.min(b), a.max(b))
            }),
        );
        self.traffic.peds.stop_lines = stop_lines;
    }

    /// [tools] The light of push-button crossing `cr` for traffic.
    pub fn button_light(&self, cr: u32) -> ButtonLight {
        self.peds.buttons.get(&cr).copied().unwrap_or_default()
    }

    /// [services] A siren vehicle is coming up to crossing `cr`: walkers
    /// keep off it and those on it hurry clear.
    fn siren_at_crossing(&self, cr: u32) -> bool {
        self.peds.crossings.get(cr as usize).is_some_and(|info| info.lanes.iter().any(|&(l, s)| self.traffic.siren_near(l, s)))
    }

    /// Seconds before the first vehicle moving at `min_v` or more reaches
    /// crossing `cr`.
    pub(super) fn crossing_gap(&self, cr: u32, min_v: f32) -> f32 {
        let Some(info) = self.peds.crossings.get(cr as usize) else { return f32::INFINITY };
        info.lanes.iter().map(|&(l, s)| self.traffic.time_to_crossing_above(l, s, HOLD_DEPTH, min_v)).fold(f32::INFINITY, f32::min)
    }

    /// [w1-roads] Has a driver stood at crossing `cr` (just before it, on
    /// one of its lanes) for `ZEBRA_PATIENCE` or more?
    pub(super) fn driver_kept_waiting(&self, cr: u32) -> bool {
        self.driver_waiting(cr, crate::pedestrians::ZEBRA_PATIENCE)
    }

    /// [w1-roads] Has a driver stood at crossing `cr` (just before it, on
    /// one of its lanes) for `secs` or more?
    pub(super) fn driver_waiting(&self, cr: u32, secs: f32) -> bool {
        let Some(info) = self.peds.crossings.get(cr as usize) else { return false };
        let t = &self.traffic;
        let reach = crate::pedestrians::ZEBRA_WAIT_REACH;
        let standing = |c: &crate::traffic::Car| c.v < 0.5 && c.stalled >= secs;
        info.lanes.iter().any(|&(l, s)| {
            t.net.lanes.get(l as usize).is_some_and(|lane| {
                lane.cars.iter().filter_map(|&c| t.cars.get(c)).any(|c| c.s <= s + 0.5 && s - c.s < reach && standing(c))
                    // [r2-traffic] Or at the end of a lane leading onto it
                    // (a crossing over a junction's or a dead end's turn).
                    || (s < reach && lane.prev.iter().filter_map(|&p| t.net.lanes.get(p as usize)).any(|pl| pl.cars.last().and_then(|&c| t.cars.get(c)).is_some_and(|c| pl.length - c.s + s < reach && standing(c))))
            })
        })
    }

    /// [emergency] The room a walker has either side of the line from `a`
    /// to `b` (m, - left, + right): on a pavement, from the building line
    /// to the kerb; on a crossing, the zebra's depth; anywhere else (lot
    /// paths, parks, plazas) a good width.
    pub(super) fn walk_room(&self, a: Vec2, b: Vec2) -> (f32, f32) {
        let open = (-OPEN_ROOM, OPEN_ROOM);
        let len = a.distance(b);
        if len < 0.5 {
            return open;
        }
        if self.walk.crossing_between(a, b).is_some() {
            return (-CROSSWALK_HALF - 0.4, CROSSWALK_HALF + 0.4);
        }
        let dir = (b - a) / len;
        let right = Vec2::new(-dir.y, dir.x);
        let mid = (a + b) * 0.5;
        // [footbridge] Up a footbridge's stairs or on its deck: within its
        // width, not out over the street beside it.
        if self.walk.level_at_node(a) > 0.0 || self.walk.level_at_node(b) > 0.0 {
            let half = crate::junctions::FOOTBRIDGE_WIDTH * 0.5 - 0.3;
            return (-half, half);
        }
        // [footbridge] Along a raised path (on a viaduct, a bridge): on its
        // deck, not in the air (or on the water) beside it.
        for sid in self.roads.segments_near(mid, crate::roads::PATH_HALF_WIDTH) {
            let s = self.roads.segment(sid);
            if s.kind != crate::roads::RoadKind::Path || !(s.bridge || s.viaduct) {
                continue;
            }
            let (pa, pb) = self.roads.segment_points(sid);
            let q = crate::geom::project_on_segment(mid, pa, pb).1;
            if q.distance(mid) < crate::roads::PATH_HALF_WIDTH && (pb - pa).normalize_or_zero().dot(dir).abs() > 0.7 {
                let half = crate::roads::PATH_HALF_WIDTH - 0.4;
                return (-half, half);
            }
        }
        let Some((sid, _, q)) = self.roads.nearest_street(mid) else { return open };
        let hw = self.roads.segment(sid).half_width();
        let to = q - mid;
        let d = to.length();
        // A pavement: alongside the street, within its width of the kerb.
        if d < hw + crate::zoning::SIDEWALK + 0.6 && d > hw && (to / d.max(1e-3)).dot(dir).abs() < 0.5 {
            let kerb = (d - hw - KERB_MARGIN).max(0.2);
            let wall = (hw + crate::zoning::SIDEWALK - d - KERB_MARGIN).max(0.2);
            return if to.dot(right) > 0.0 { (-wall, kerb) } else { (-kerb, wall) };
        }
        open
    }

    /// [emergency] On a crossing: step round any car standing (or crawling)
    /// on it, out past its nearer end, rather than through it.
    fn weave_cars(&self, cr: u32, p: Vec2) -> Vec2 {
        let Some(info) = self.peds.crossings.get(cr as usize) else { return p };
        let mut p = p;
        for &(lane, s) in &info.lanes {
            let Some(l) = self.traffic.net.lanes.get(lane as usize) else { continue };
            for &cid in &l.cars {
                let c = &self.traffic.cars[cid];
                let len = c.length();
                if c.s < s - 1.0 || c.s - len > s + HOLD_DEPTH + 1.0 {
                    continue;
                }
                let fwd = Vec2::from_angle(c.heading);
                let rel = p - (c.pos - fwd * len * 0.5);
                let (along, across) = (rel.dot(fwd), rel.dot(Vec2::new(-fwd.y, fwd.x)));
                if along.abs() < len * 0.5 + 0.45 && across.abs() < 1.35 {
                    let push = if along >= 0.0 { len * 0.5 + 0.5 - along } else { -(len * 0.5 + 0.5 + along) };
                    p += fwd * push;
                }
            }
        }
        p
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
        crate::prof_scope!("walk_route_for");
        let p = self.jaywalk_chance_of(id, self.late_for_work(id, purpose));
        let jay = p > 0.0 && hash01(self.citizens[id].seed, self.clock.tick, 0x7a17) < p;
        self.walk.route_with(from, to, jay)
    }

    /// [walk-crossings] Plan the rest of a walk again, lawfully, from the
    /// kerb it stands at (`path[idx - 1]`) to where it was going: the
    /// crossing ahead was closed under it (a footbridge or the player took
    /// it away) or a jaywalker tired of waiting for a gap. True when a new
    /// way was found; `path` then starts at that kerb (`idx` 0).
    fn replan_walk(&self, path: &mut Vec<Vec2>, idx: &mut usize) -> bool {
        if *idx == 0 {
            return false;
        }
        let from = path[*idx - 1];
        let Some(&dest) = path.last() else { return false };
        let Some((mut pts, _)) = self.walk.route(from, dest) else { return false };
        if pts.last().is_none_or(|p| p.distance(dest) > 1e-3) {
            pts.push(dest);
        }
        *path = pts;
        *idx = 0;
        true
    }

    /// May this walker step onto crossing `cr` now? `Some(against)` to go
    /// (`against`: against the light, or mid-block), `None` to wait.
    fn may_cross(&self, id: CitizenId, w: &Walker, cr: u32, since: u64, late: bool) -> Option<bool> {
        let crossing = &self.walk.crossings[cr as usize];
        let signal = self.peds.crossings.get(cr as usize).and_then(|i| i.signal);
        let button = crossing.button && signal.is_none();
        let lawful = match (crossing.kind, signal) {
            (CrossingKind::Jay, _) => false,
            (_, Some((k, ap))) => self.traffic.peds.walk_shown(k as usize, ap as usize),
            // [tools] A push-button crossing: across on the walk light.
            _ if button => self.button_light(cr).walk(),
            _ => true,
        };
        // With the walk light on, a jam standing or crawling across the
        // crossing is walked through (between the cars), not waited for.
        let gap = self.crossing_gap(cr, if lawful && (signal.is_some() || button) { JAM_CRAWL } else { 0.5 });
        if lawful && crossing.marked() {
            // [w1-roads] At a zebra with no lights, a driver kept waiting
            // gets to go: the walkers coming up hold back.
            if signal.is_none() && !button && self.driver_kept_waiting(cr) {
                return None;
            }
            // Look first: nobody too close to stop.
            return (gap >= LAWFUL_GAP).then_some(false);
        }
        if lawful {
            // [r2-traffic] Unmarked, nobody steps out in front of a car
            // standing at it either: it is about to go. Only moving cars
            // made the gap, so a stream of walkers round the end of a dead
            // end kept the cars turning there standing for minutes,
            // "braking for someone crossing the road" (K37, K38, K47).
            if self.driver_waiting(cr, crate::pedestrians::GREEN_WAIT) {
                return None;
            }
            // Unmarked (a dead end, a roundabout's lanes): nobody gives way,
            // so a gap long enough to get across, plus the walker's margin.
            let across = self.walk.nodes.get(crossing.a as usize).zip(self.walk.nodes.get(crossing.b as usize)).map_or(8.0, |(a, b)| a.pos.distance(b.pos));
            let per_second = self.config.walk_speed / self.config.traffic_seconds_per_tick.max(0.1);
            let needed = across / per_second.max(0.5) + gap_needed(w.caution, late);
            return (gap >= needed).then_some(false);
        }
        if crossing.kind != CrossingKind::Jay && !w.bold {
            return None;
        }
        // [w1-roads] Against the light, nobody steps out in front of a car
        // standing at the line: it is about to go. Its
        // gap read as open road (only moving cars count), so a stream of
        // walkers against the red kept the cars with the green standing at
        // the stop line for minutes (Harvey's city, a signalled junction
        // at (-186, -427): nine on the crosswalk, cars stood 500 s).
        if signal.is_some() && self.driver_waiting(cr, crate::pedestrians::GREEN_WAIT) {
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
        crate::prof_scope!("step_walker");
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
                let keep = KEEP_SPREAD.0 + (KEEP_SPREAD.1 - KEEP_SPREAD.0) * hash01(seed, 0x5eed, 1);
                let pace = PACE.0 + (PACE.1 - PACE.0) * hash01(seed, 0x9ace, 2);
                Walker { key: (usize::MAX, 0, 0), base: start, side: 0.0, keep, crossing: None, waiting: None, bold: false, against: false, caution, hedonism, pace, room: (-SIDESTEP_MAX, SIDESTEP_MAX), room_leg: usize::MAX }
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
            w.room_leg = usize::MAX;
        }
        // [emergency] Everyone at their own pace.
        // [social] Walking with a friend: the slower one sets the pace.
        let speed = speed * self.social_pace(id, w.pace);
        let mut heading = self.citizens[id].heading;
        let mut dir = Vec2::from_angle(heading);
        let mut budget = speed;
        let mut hurried = false;
        let mut wait_at: Option<(u32, Vec2, Vec2)> = None;
        let mut rail_wait: Option<usize> = None;
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
                            // [walk-crossings] The crossing ahead is gone (a
                            // footbridge replaced it, the player removed it,
                            // a slip road's mouth lost its walk lights), or a
                            // jaywalker has stood long enough waiting for a
                            // gap: plan the rest of the way again, over the
                            // bridge or round by a crossing (Harvey's flag:
                            // people with routes from before the footbridge
                            // bunched at the kerb and ran across together).
                            let c = &self.walk.crossings[cr as usize];
                            let gone = c.closed || c.shut;
                            let tired = c.kind == CrossingKind::Jay && tick.saturating_sub(since) >= JAY_GIVE_UP;
                            if (gone || tired) && self.replan_walk(path, idx) {
                                if gone {
                                    self.peds.replans_total += 1;
                                    self.peds.uses.entry(cr).or_default().replans += 1;
                                } else {
                                    self.peds.jay_give_ups_total += 1;
                                }
                                w.key = path_key(path);
                                w.crossing = None;
                                w.waiting = None;
                                w.against = false;
                                w.room_leg = usize::MAX;
                                leg_start = w.base;
                                continue;
                            }
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
                                    let u = self.peds.uses.entry(cr).or_default();
                                    u.at_grade += 1;
                                    u.against += against as u32;
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
            // [rail-crossing] A level crossing with its lights on: wait at
            // the edge of the track (whoever is on it walks off).
            // ([walk-networks] not down in a tunnel under the track)
            if let Some((k, stop)) = self.rail_stop(w.base, w.base + dir * d.min(budget)).filter(|_| self.walk.level_at_node(target) >= 0.0) {
                w.base = stop;
                budget = 0.0;
                rail_wait = Some(k);
                break;
            }
            if d <= budget {
                // [walk-crossings] Over a footbridge's deck: counted.
                if *idx > 0 {
                    if let Some(k) = self.walk.footbridge_deck(path[*idx - 1], target) {
                        self.peds.uses.entry(k).or_default().bridge += 1;
                    }
                }
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
        if let Some(k) = rail_wait {
            self.note_rail_wait(k);
        }
        // The leg now being walked, for its width.
        let (leg_idx, leg) = (*idx, (*idx < path.len()).then(|| (leg_start_of(path, *idx, w.base), path[*idx])));
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
            } else if crossing.button {
                // [tools] Press the button and wait for the light: drivers
                // do not give way to someone waiting here.
                feet.presses.push(cr);
            } else if crossing.marked() && !self.siren_at_crossing(cr) && !(crossing.kind != CrossingKind::Jay && self.driver_kept_waiting(cr)) {
                // About to step onto a zebra: drivers give way (but not to
                // someone standing back for lights and sirens, nor [w1-roads]
                // to someone holding back for a driver kept waiting).
                feet.occupied.push((cr, id));
            }
            kerb + u * (0.9 - 0.7 * row) + v * col * 0.75
        } else {
            heading = dir.y.atan2(dir.x);
            let right = Vec2::new(-dir.y, dir.x);
            // [emergency] The width this leg offers, and where in it this
            // walker likes to be (further out on a wide path or a plaza).
            if let Some((a, b)) = leg.filter(|_| w.room_leg != leg_idx) {
                w.room = self.walk_room(a, b);
                w.room_leg = leg_idx;
            }
            let (mut lo, mut hi) = w.room;
            let wide = hi - lo > 4.0;
            let keep = (if wide { w.keep * 1.8 } else { w.keep }).clamp(lo + 0.2, (hi - 0.2).max(lo + 0.2));
            w.side += (keep - w.side) * 0.25;
            let mut p = w.base + right * w.side;
            // Personal space: fall in behind someone going the same way
            // (or standing), step aside for anyone else.
            let (cx, cy) = cell(p);
            let mut near: Vec<(Vec2, Vec2, bool, f32)> = Vec::new();
            for dx in -1..=1 {
                for dy in -1..=1 {
                    let Some(list) = feet.grid.get(&(cx + dx, cy + dy)) else { continue };
                    for &o in list {
                        if o == id {
                            continue;
                        }
                        let Some(oc) = self.citizens.get(o) else { continue };
                        let standing = self.peds.walkers.get(&o).is_some_and(|ow| ow.waiting.is_some());
                        near.push((oc.pos, Vec2::from_angle(oc.heading), standing, oc.pos.distance(oc.prev_pos)));
                    }
                }
            }
            if !near.is_empty() && !arrived {
                let crowd = near.iter().filter(|x| x.0.distance(p) < 3.0).count() > CROWD;
                let r = if crowd { CROWD_SPACE } else { PERSONAL_SPACE };
                // In a crush the rules are a suggestion: step off the line
                // onto whatever is beside it.
                if crowd {
                    (lo, hi) = (lo - CRUSH_SPILL, hi + CRUSH_SPILL);
                }
                let mine = speed;
                let mut back_room = moved.min(w.base.distance(leg_start));
                // A few passes: stepping clear of one may bring another close.
                for _ in 0..3 {
                    let mut clear = true;
                    for &(op, odir, standing, omoved) in &near {
                        let d = op.distance(p);
                        if d >= r {
                            continue;
                        }
                        clear = false;
                        let along = (op - p).dot(dir);
                        let lateral = (op - p).dot(right);
                        let same_way = standing || odir.dot(dir) > 0.3;
                        // Where they are across the path, and the two places
                        // beside them that clear them.
                        let theirs = w.side + lateral;
                        let (left_of, right_of) = (theirs - r - 0.05, theirs + r + 0.05);
                        let fits = |x: f32| x >= lo && x <= hi;
                        if same_way && along > 0.05 {
                            // Someone ahead going my way: pass on the left (or
                            // the right) when they are slower and there is
                            // room; abreast if we are only bunched up; fall in
                            // behind only when boxed in.
                            let slower = standing || omoved < mine * 0.92;
                            let to = if fits(left_of) { Some(left_of) } else if fits(right_of) { Some(right_of) } else { None };
                            match to {
                                Some(x) if slower || d < r * 0.8 => w.side = x,
                                _ if back_room > 0.0 => {
                                    let back = ((r * r - lateral * lateral).max(0.0).sqrt() + along + 0.02).min(back_room);
                                    w.base -= dir * back;
                                    back_room -= back;
                                }
                                _ => {}
                            }
                        } else {
                            // Beside, behind or coming the other way: step
                            // away from them, within the room there is.
                            let away = if lateral > 0.05 { -1.0 } else { 1.0 };
                            let to = w.side + away * (r - d);
                            w.side = if fits(to) {
                                to
                            } else if fits(w.side - away * (r - d)) && !same_way {
                                w.side - away * (r - d)
                            } else {
                                to.clamp(lo, hi.max(lo))
                            };
                        }
                        p = w.base + right * w.side;
                    }
                    if clear {
                        break;
                    }
                }
            }
            // [emergency] Between the cars of a jam standing on the crossing.
            if let Some((cr, _)) = w.crossing {
                let q = self.weave_cars(cr, p);
                if q != p {
                    w.side += (q - p).dot(right);
                    p = q;
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
                // [r2-traffic] The driver pulls over to the kerb (out of
                // the traffic, which goes on past) unless it is in the
                // middle of a junction: it used to stop dead in its lane
                // for minutes with everyone stuck behind it (K29).
                let in_junction = self.traffic.cars.get(car).and_then(|c| c.route.get(c.idx)).is_some_and(|&l| self.traffic.net.lanes[l as usize].is_connector());
                if in_junction {
                    if let Some(c) = self.traffic.cars.get_mut(car) {
                        c.v = 0.0;
                        c.hold = c.hold.max(150.0);
                    }
                } else if self.traffic.cars.contains_key(car) {
                    self.traffic.pull_in(car, 150.0);
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
            .filter(|(id, c)| !except.contains(id) && !c.asleep && c.age >= 12 && c.inside.is_none() && !matches!(c.activity, Activity::OnTrain { .. } | Activity::OnMetro { .. } | Activity::Arriving))
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

    /// [emergency] A dozen setting off together along one pavement spread
    /// across it and pass each other at their own pace, rather than filing
    /// along one line.
    #[test]
    fn a_crowd_spreads_out_and_overtakes_instead_of_walking_single_file() {
        let mut w = town();
        quiet(&mut w);
        let route = w.walk.route(Vec2::new(-250.0, -8.0), Vec2::new(-30.0, -8.0)).expect("a pavement route").0;
        let ids: Vec<CitizenId> = w.citizens.keys().take(12).collect();
        for &id in &ids {
            let seed = w.citizens[id].seed;
            walker(&mut w, id, seed, route.clone());
        }
        let order = |w: &World| -> Vec<(CitizenId, f32)> { ids.iter().filter(|&&id| matches!(w.citizens[id].activity, Activity::Traveling { .. })).map(|&id| (id, w.citizens[id].pos.x)).collect() };
        run(&mut w, 12);
        let early = order(&w);
        // Spread across the pavement: the walkers' offsets from its line.
        let ys: Vec<f32> = early.iter().map(|&(id, _)| w.citizens[id].pos.y).collect();
        let mean = ys.iter().sum::<f32>() / ys.len().max(1) as f32;
        let sd = (ys.iter().map(|y| (y - mean) * (y - mean)).sum::<f32>() / ys.len().max(1) as f32).sqrt();
        assert!(sd > 0.3, "spread across the pavement, not one line: sd {sd:.2} m");
        run(&mut w, 40);
        let late = order(&w);
        let ahead = |list: &[(CitizenId, f32)], a: CitizenId, b: CitizenId| {
            let (pa, pb) = (list.iter().find(|x| x.0 == a).map(|x| x.1), list.iter().find(|x| x.0 == b).map(|x| x.1));
            pa.zip(pb).map(|(x, y)| x > y)
        };
        let mut passed = 0;
        for &(a, _) in &early {
            for &(b, _) in &early {
                if let (Some(before), Some(after)) = (ahead(&early, a, b), ahead(&late, a, b)) {
                    passed += (!before && after) as u32;
                }
            }
        }
        assert!(passed > 0, "faster walkers overtook slower ones");
    }

    /// [emergency] With the walk light on and a car standing across the
    /// crossing, a walker goes round its end, not through it.
    #[test]
    fn walkers_weave_round_a_car_standing_on_the_crossing() {
        let mut w = town();
        quiet(&mut w);
        run(&mut w, 1);
        let (cr, ..) = crosswalk(&w);
        let (lane, s) = w.peds.crossings[cr as usize].lanes[0];
        let car = w.traffic.spawn_car(CitizenId::default(), None, vec![lane], s + 3.0, s + 3.5, 0);
        w.traffic.step(0.1, 1);
        let c = &w.traffic.cars[car];
        let fwd = Vec2::from_angle(c.heading);
        let middle = c.pos - fwd * c.length() * 0.5;
        let q = w.weave_cars(cr, middle + Vec2::new(-fwd.y, fwd.x) * 0.3);
        assert!((q - middle).dot(fwd).abs() >= c.length() * 0.5 + 0.4, "stepped out past the car's end: {q} from {middle}");
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
