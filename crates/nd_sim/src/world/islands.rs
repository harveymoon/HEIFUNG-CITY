//! [ferry-islands] Islands reached only by ferry.
//!
//! A part of the city with no road or path to the rest (an island, a spit
//! across a bay) is not stranded when one of its ferry terminals has boats
//! to a terminal on the main network: it is an **island** (`Island`, worked
//! out with the stranded areas in `refresh_connectivity`). Then:
//!
//! - **Ferries are a trip mode** (`TripMode::Ferry`, `TransitLeg::Ferry`):
//!   a resident going between two walk networks walks to the terminal,
//!   waits on the pier, boards the next boat that sails for the other
//!   terminal (one from either end: a boat from the far terminal calls
//!   here and sails back), rides out of sight, steps off and goes on as
//!   after any ride. The planner weighs the walk, the wait (by both
//!   terminals' timetables, `ferry_wait`) and the sailing. Out of hours
//!   the wait is until the first boat of the morning: the last boat home
//!   matters, and a rider who missed it waits on the pier all night.
//! - **Freight crosses by water** (`queue_ferry_load`): a load for an
//!   island with no road from the city waits at the mainland terminal; a
//!   car ferry drives the lorry aboard and it drives off on the island
//!   (`LORRY_LANES` of its deck, up to a quarter of it), a passenger boat
//!   takes `HOLD_LOADS` pallets in its hold and porters carry them on.
//!   Building sites there build (`island_supplied`).
//! - **Island life is a choice**: a household offered an island home
//!   takes it by its liking for quiet, how its earners would get to work
//!   and whether the boats run when their shifts end (`island_appeal`);
//!   the land has its own price (quiet and views, less the crossing:
//!   `splat_islands`); islanders out of work look for work on the island
//!   first; earners worn down by the crossing move to the mainland
//!   (`island_move`).
//!
//! Nothing here draws from the world's stream: choices are hashes of the
//! household, the day and the place.

use super::*;
use crate::hash::{mix, unit};
use crate::ports::{self, FerryLink, FerryLoad, PortKind, ShipKind};
use crate::world::connectivity::Island;
use crate::world::trips::{ModeOption, ModePlan, TripQuery};
use crate::citizens::{LifeStage, TripMode};
use slotmap::Key;

/// [ferry-islands] Longest a rider waits on the pier before giving up
/// (they wait out the night for the first boat).
pub const FERRY_GIVE_UP: u64 = 14 * 60;
/// [ferry-islands] Least and most the planner expects to wait for a boat
/// while they sail.
pub const MIN_FERRY_WAIT: f32 = 4.0;
pub const MAX_FERRY_WAIT: f32 = 120.0;
/// [ferry-islands] Ferry minutes against what a trip "should" take before
/// it hurts: islanders know the crossing takes its time.
pub const FERRY_GRACE: f32 = 40.0;
/// [ferry-islands] The land value of an island: the quiet and the views,
/// and what each minute of the crossing (wait and sail) beyond
/// `CALM_MINUTES` takes off.
pub const ISLAND_CALM: f32 = 0.08;
pub const CALM_MINUTES: f32 = 20.0;
pub const PER_FERRY_MINUTE: f32 = 0.004;
/// [ferry-islands] Pain of the ferry (bad half-hours, remembered) past
/// which an islander's household thinks of moving to the mainland, and
/// the chance a day that it does.
pub const FERRY_MOVE_PAIN: f32 = 1.6;
const FERRY_MOVE_CHANCE: f32 = 0.25;

/// [ferry-islands] Residents waiting on the piers for one crossing (both
/// ways counted) that bring the next boat from either end there instead of
/// its turn.
pub const CALL_DEMAND: u32 = 8;
/// [ferry-islands] Minutes before the boats stop that islanders out on the
/// mainland set off for the pier.
pub const LAST_BOAT_LEAD: u32 = 120;
/// [ferry-islands] Farther than this from the pier, a rider gets to the
/// terminal as a trip of its own (a bus, the car) before walking out on it.
pub const PIER_WALK: f32 = 350.0;
/// [ferry-islands] Residents on the piers for one crossing (both ways) that
/// bring an extra sailing, and the least gap between sailings then.
pub const EXTRA_SAILING_CROWD: u32 = 40;
pub const EXTRA_SAILING_GAP: u64 = 25;
/// [ferry-islands] Minutes before the boats stop when a terminal sweeps up
/// whoever is left on the piers (a boat for a single rider).
pub const LAST_SWEEP: u32 = 45;
/// [ferry-islands] Why a trip across the water found no way: the boats.
pub const NO_FERRY: &str = "no ferry is running across now";

/// [ferry-islands] A crossing as the planner sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FerryPlan {
    pub board: BuildingId,
    pub alight: BuildingId,
    /// Where to wait on `board`'s pier, and `alight`'s gate.
    pub stop: Vec2,
    pub exit: Vec2,
    pub wait: f32,
    pub sail: f32,
    /// Minutes to the pier and on from the far one.
    pub ends: f32,
    /// Walking at both ends, metres as the crow flies.
    pub walk: f32,
}

/// [ferry-islands] One island for the panels and the remote API.
#[derive(Clone, Debug, PartialEq)]
pub struct IslandInfo {
    pub center: Vec2,
    pub name: String,
    pub terminals: Vec<BuildingId>,
    pub mainland: Vec<BuildingId>,
    pub car_ferry: bool,
    pub buildings: u32,
    pub residents: u32,
    pub workers: u32,
    /// Shortest sailing to the mainland, and the wait for a boat now.
    pub sail_minutes: f32,
    pub wait_minutes: Option<f32>,
    /// Services with a station on the island (police, fire, health).
    pub police: bool,
    pub fire: bool,
    pub health: bool,
    pub loads_waiting: u32,
}

impl World {
    // ── the links ────────────────────────────────────────────────────────

    /// Every pair of ferry terminals in town a boat can sail between, with
    /// the length of the sailing. Worked out with the port links.
    pub(in crate::world) fn refresh_ferry_links(&mut self) {
        let terms: Vec<(BuildingId, Vec2, PortKind)> = self.ports.ports.values().filter(|p| p.kind.is_ferry() && p.sails()).filter_map(|p| p.building.map(|b| (b, p.berth, p.kind))).collect();
        let mut pairs = Vec::new();
        let mut ends = Vec::new();
        for i in 0..terms.len() {
            for j in i + 1..terms.len() {
                pairs.push((terms[i].1, Some(terms[j].1)));
                ends.push((i, j));
            }
        }
        let (routes, _) = self.landscape.water_routes_batch(&pairs, &[]);
        let mut links = Vec::new();
        for ((i, j), r) in ends.into_iter().zip(routes) {
            let Some(r) = r else { continue };
            let large = |k: PortKind| k.car_deck() > 0;
            links.push(FerryLink { a: terms[i].0, b: terms[j].0, metres: crate::geom::path_length(&r), cars: large(terms[i].2) && large(terms[j].2) });
        }
        self.ports.links = links;
    }

    /// [ferry-islands] The links at terminal `b`.
    pub fn ferry_links_at(&self, b: BuildingId) -> Vec<FerryLink> {
        self.ports.links.iter().filter(|l| l.other(b).is_some()).copied().collect()
    }

    /// The walk network (component) at `p`, by the node a walk from there
    /// would start at.
    pub(in crate::world) fn walk_comp_at(&self, p: Vec2) -> Option<u32> {
        self.walk.snap_end(p).and_then(|n| self.walk.component(n))
    }

    /// Minutes a ferry takes over `metres`, with its time alongside.
    fn sail_minutes(&self, metres: f32) -> f32 {
        metres / self.config.metres_per_tick(ports::FERRY_SPEED).max(0.01) + ports::FERRY_DWELL as f32 * 0.5
    }

    /// [ferry-islands] Does terminal `b` serve an island (it stands on one,
    /// or one of the terminals it sails to does)? Such a terminal keeps its
    /// boats for the crossing in the rush hours instead of runs out of town.
    pub(in crate::world) fn island_duty(&self, b: BuildingId) -> bool {
        self.connectivity.islands.iter().any(|i| i.terminals.contains(&b) || i.mainland.contains(&b))
    }

    /// [ferry-islands] Is the sailing number `turn` from `port` (with other
    /// terminals to go to unless `alone`) a run out of town? Turn about,
    /// except that a terminal serving an island sails across in the rush.
    pub(in crate::world) fn remote_turn(&self, port: &crate::ports::Port, alone: bool, turn: usize, minute: u32) -> bool {
        if port.remote.is_empty() {
            return false;
        }
        if alone {
            return true;
        }
        let rush = |(a, b): (u32, u32)| (a..b).contains(&(minute % 1440));
        if (rush(crate::timetable::AM_RUSH) || rush(crate::timetable::PM_RUSH)) && port.building.is_some_and(|b| self.island_duty(b)) {
            return false;
        }
        turn % 2 == 0
    }

    /// [ferry-islands] How many boats an hour sail from terminal `x` for
    /// terminal `y` at minute `m` of the day: `x`'s own for `y` and `y`'s,
    /// which call at `x` on the way back. 0 while neither sails.
    fn boats_per_minute(&self, x: BuildingId, y: BuildingId, m: u32) -> f32 {
        let mut rate = 0.0;
        for (t, other) in [(x, y), (y, x)] {
            let Some(p) = self.ports.port_of(t) else { continue };
            let Some(h) = p.ferry_headway_at(m) else { continue };
            let _ = other;
            let others = self.ports.links.iter().filter(|l| l.other(t).is_some()).count().max(1) as f32;
            // The share of its runs that go to the other end: all but the
            // runs out of town, which take every other turn.
            let share = if self.remote_turn(p, false, 0, m) { 0.5 / others } else { 1.0 / others };
            rate += share / h.max(1) as f32;
        }
        rate
    }

    /// [ferry-islands] Minutes a rider reaching `x`'s pier at minute `m`
    /// of the day would wait for a boat to `y`: half the gap between
    /// boats while they sail; out of hours, until the first boat of the
    /// morning (None when no boat sails within the day).
    pub fn ferry_wait(&self, x: BuildingId, y: BuildingId, m: u32) -> Option<f32> {
        let rate = self.boats_per_minute(x, y, m);
        if rate > 0.0 {
            return Some((0.5 / rate).clamp(MIN_FERRY_WAIT, MAX_FERRY_WAIT));
        }
        // Out of hours: step on to the first boat.
        (1..=24 * 4).map(|k| m + k * 15).find(|&t| self.boats_per_minute(x, y, t) > 0.0).map(|t| (t - m) as f32 + (0.5 / self.boats_per_minute(x, y, t)).clamp(MIN_FERRY_WAIT, MAX_FERRY_WAIT))
    }

    /// [ferry-islands] Residents waiting on the piers, by (board, alight).
    pub fn pier_queues(&self) -> std::collections::BTreeMap<(BuildingId, BuildingId), u32> {
        let mut out = std::collections::BTreeMap::new();
        if self.ports.links.is_empty() {
            return out;
        }
        for c in self.citizens.values() {
            if let (Activity::Waiting { .. }, Some(TransitLeg::Ferry { board, alight })) = (&c.activity, c.transit.map(|t| t.leg)) {
                *out.entry((board, alight)).or_insert(0) += 1;
            }
        }
        out
    }

    /// [ferry-islands] Where to wait on a terminal's pier: along it, a
    /// little to one side, spread by the rider.
    pub(in crate::world) fn pier_spot(&self, b: BuildingId, who: u64) -> Option<Vec2> {
        let p = self.ports.port_of(b)?;
        let h = mix(who, 0x91E5);
        let t = 0.15 + 0.55 * unit(h);
        let side = (unit(h >> 20) - 0.5) * p.kind.pier().1 * 0.9;
        Some(p.pier[0].lerp(p.pier[1], t) + p.along * side)
    }

    /// [ferry-islands] The best crossing from `from` to `to` when they lie
    /// on different walk networks joined by a ferry: board where the walk
    /// there, the wait and the sailing are least. `arrive` gives the
    /// minute of the day a walk of so many metres reaches the pier.
    pub(in crate::world) fn plan_ferry_trip(&self, id: CitizenId, from: Vec2, to: Vec2, arrive: &dyn Fn(f32) -> u32) -> Option<FerryPlan> {
        if self.ports.links.is_empty() {
            return None;
        }
        let cf = self.walk_comp_at(from)?;
        let ct = self.walk_comp_at(to)?;
        if cf == ct {
            return None;
        }
        let walk = self.config.walk_speed.max(0.1);
        let comp_of = |b: BuildingId| self.building_entrance(b).and_then(|p| self.walk_comp_at(p));
        let works = |b: BuildingId| self.ports.port_of(b).is_some_and(|p| p.works());
        let who = self.citizens.get(id).map_or(0, |c| c.seed);
        let mut best: Option<(f32, FerryPlan)> = None;
        for l in &self.ports.links {
            for (x, y) in [(l.a, l.b), (l.b, l.a)] {
                if !works(x) || !works(y) || comp_of(x) != Some(cf) || comp_of(y) != Some(ct) {
                    continue;
                }
                let (Some(stop), Some(exit)) = (self.pier_spot(x, who), self.building_entrance(y)) else { continue };
                let metres = from.distance(stop) + exit.distance(to);
                // Either end may be a ride (a bus, a car) when it is far.
                let secs = self.config.traffic_seconds_per_tick;
                let leg = |d: f32| if d <= PIER_WALK { d * 1.25 / walk } else { (d * 1.25 / walk).min(12.0 + d * 1.3 / (7.0 * secs)) };
                let first = leg(from.distance(stop));
                let Some(wait) = self.ferry_wait(x, y, self.clock.minute_of_day() as u32 + first.ceil() as u32) else { continue };
                let _ = arrive;
                let sail = self.sail_minutes(l.metres);
                let minutes = first + wait + sail + leg(exit.distance(to));
                if best.as_ref().is_none_or(|(m, _)| minutes < *m) {
                    best = Some((minutes, FerryPlan { board: x, alight: y, stop, exit, wait, sail, walk: metres, ends: minutes - wait - sail }));
                }
            }
        }
        best.map(|b| b.1)
    }

    /// [ferry-islands] The ferry as a mode: the walk to the pier, the wait,
    /// the crossing and the fare; a calm ride, the deck open to the air.
    pub(in crate::world) fn ferry_option(&self, id: CitizenId, q: &TripQuery, arrive: &dyn Fn(f32) -> u32) -> Option<ModeOption> {
        let p = self.plan_ferry_trip(id, q.from, q.to, arrive)?;
        // Out at night for nothing that matters: not if it means the pier
        // till morning.
        // Nor out across the water for an evening with no boat back after.
        if !matches!(q.purpose, Purpose::Home | Purpose::Work) {
            if p.wait > 90.0 {
                return None;
            }
            let back = self.clock.minute_of_day() as f32 + p.ends + p.wait + p.sail + 210.0;
            let _ = arrive;
            if self.boats_per_minute(p.alight, p.board, back as u32 % 1440) <= 0.0 {
                return None;
            }
        }
        let c = self.citizens.get(id)?;
        let walk = self.config.walk_speed.max(0.1);
        let [sociable, ambition, _, _, curiosity] = crate::vehicle::traits_of(c.seed);
        let minutes = p.ends + p.wait + p.sail;
        let _ = walk;
        let comfort = 3.0 - 6.0 * sociable * 0.5 - 4.0 * curiosity;
        let pain = c.commute.pain_of(TripMode::Ferry);
        let cost = minutes * (0.7 + 0.6 * ambition) + ports::FERRY_FARE * self.money_weight(id) + comfort + trips::PAIN_WEIGHT * pain;
        Some(ModeOption { mode: TripMode::Ferry, minutes, money: ports::FERRY_FARE, comfort, pain, cost, plan: ModePlan::Ferry { board: p.board, alight: p.alight, stop: p.stop } })
    }

    // ── on the pier and aboard ───────────────────────────────────────────

    /// [ferry-islands] Set off for a crossing: from near the pier, walk
    /// out onto it; from farther, get to the terminal any way (bus, car,
    /// on foot: a trip of its own) and walk out from there (`reach_pier`).
    #[allow(clippy::too_many_arguments)]
    pub(in crate::world) fn set_off_for_pier(&mut self, id: CitizenId, from: Vec2, board: BuildingId, alight: BuildingId, stop: Vec2, target: BuildingId, purpose: Purpose, fresh: bool) {
        let leg = TransitLeg::Ferry { board, alight };
        if from.distance(stop) <= PIER_WALK {
            self.walk_to_transit(id, stop, leg, target, purpose, fresh);
            return;
        }
        let since = self.clock.tick;
        self.ports.to_pier.retain(|t| t.citizen != id);
        self.ports.to_pier.push(crate::ports::ToPier { citizen: id, board, alight, target, purpose, since });
        self.travel_with(id, Some(board), Purpose::ToStop, true, false, true);
        let going = self.citizens.get(id).is_some_and(|c| c.activity.is_traveling() || c.to_car.is_some() || c.transit.is_some());
        if !going {
            self.ports.to_pier.retain(|t| t.citizen != id);
            self.walk_to_transit(id, stop, leg, target, purpose, fresh);
        }
    }

    /// [ferry-islands] At terminal `b` on the way to a crossing: out onto
    /// the pier to wait. False when this is not such a trip.
    pub(in crate::world) fn reach_pier(&mut self, id: CitizenId, b: BuildingId) -> bool {
        if self.citizens.get(id).is_none_or(|c| c.transit.is_some()) {
            return false;
        }
        let Some(k) = self.ports.to_pier.iter().position(|t| t.citizen == id && t.board == b) else { return false };
        let t = self.ports.to_pier.remove(k);
        if !self.buildings.contains_key(t.target) || !self.buildings.contains_key(t.alight) {
            return false;
        }
        let who = self.citizens[id].seed;
        let stop = self.pier_spot(b, who).or_else(|| self.building_entrance(b)).unwrap_or(self.citizens[id].pos);
        let c = &mut self.citizens[id];
        c.inside = None;
        c.activity = Activity::Idle;
        self.walk_to_transit(id, stop, TransitLeg::Ferry { board: b, alight: t.alight }, t.target, t.purpose, false);
        true
    }

    /// Take on riders waiting at `at` for `to` into ship `sid`, as many as
    /// it has room for; they pay the fare at the gate.
    pub(in crate::world) fn ferry_board(&mut self, sid: crate::ports::ShipId, at: BuildingId, to: BuildingId) -> u32 {
        let Some(ship) = self.ports.ships.get(sid) else { return 0 };
        let cap = self.ports.port_of(ship.port).map_or(ports::FERRY_CAPACITY, |p| p.kind.ferry_capacity());
        let room = cap.saturating_sub(ship.passengers) as usize;
        if room == 0 {
            return 0;
        }
        let name = self.buildings.get(at).map(|b| b.name.clone()).unwrap_or_default();
        let boarding = self.board_waiting(|t| matches!(t.leg, TransitLeg::Ferry { board, alight } if board == at && alight == to), room, ports::FERRY_FARE, Flow::BusFares, Item::Building(at), &format!("Took the ferry from {name}."));
        if boarding.is_empty() {
            return 0;
        }
        let tick = self.clock.tick;
        let hour = self.clock.minute_of_day() as u32 / 60;
        let n = boarding.len() as u32;
        let mut waits = Vec::with_capacity(boarding.len());
        for &p in &boarding {
            let waited = self.citizens[p].transit.map_or(0, |t| if t.since > 0 { tick.saturating_sub(t.since) } else { 0 });
            waits.push(waited as u32);
            let c = &mut self.citizens[p];
            c.activity = Activity::OnFerry { from: at, to };
            c.inside = None;
            self.ports.missed.remove(&p);
        }
        if let Some(port) = self.ports.ports.values_mut().find(|p| p.building == Some(at)) {
            for w in waits {
                port.timeline.board(hour, 1, w);
            }
        }
        let ship = &mut self.ports.ships[sid];
        ship.riders.extend(boarding);
        ship.passengers += n;
        let (from, dest) = (ship.port, ship.to);
        let s = &mut self.ports.stats;
        s.own_trips_today += n;
        s.ferry_riders_today += n;
        s.ferry_fares_today += n as f32 * ports::FERRY_FARE;
        // Riders boarding on the way back count on the same route.
        if dest.is_some() && at != from {
            self.ports.count_line_riders(from, dest, None, n);
        }
        n
    }

    /// Riders aboard ship `sid` whose crossing ends at `at` step off and go
    /// on (any way they like, as off a bus).
    pub(in crate::world) fn ferry_set_down(&mut self, sid: crate::ports::ShipId, at: BuildingId) {
        let Some(ship) = self.ports.ships.get_mut(sid) else { return };
        let citizens = &self.citizens;
        let (off, keep): (Vec<CitizenId>, Vec<CitizenId>) = std::mem::take(&mut ship.riders).into_iter().partition(|&p| citizens.get(p).is_none_or(|c| matches!(c.activity, Activity::OnFerry { to, .. } if to == at)));
        ship.passengers = ship.passengers.saturating_sub(off.len() as u32);
        ship.riders = keep;
        for p in off {
            self.step_off_ferry(p, at);
        }
    }

    /// One rider off the boat at `at`'s gate, on to where they were going.
    pub(in crate::world) fn step_off_ferry(&mut self, p: CitizenId, at: BuildingId) {
        let Some(c) = self.citizens.get(p) else { return };
        let pos = self.building_entrance(at).unwrap_or(c.pos);
        let c = &mut self.citizens[p];
        c.pos = pos;
        c.prev_pos = pos;
        c.activity = Activity::Idle;
        c.inside = None;
        let Some(trip) = c.transit.take() else { return };
        self.travel_to_impl(p, Some(trip.target), trip.purpose, true, false);
    }

    /// Riders on a boat that is gone (pulled out of service, or a save
    /// that does not keep ships): set down at the pier they were sailing to.
    pub(in crate::world) fn ferries_after_load(&mut self) {
        let aboard: Vec<(CitizenId, BuildingId)> = self.citizens.iter().filter_map(|(id, c)| match c.activity {
            Activity::OnFerry { to, .. } => Some((id, to)),
            _ => None,
        }).filter(|(id, _)| !self.ports.ships.values().any(|s| s.riders.contains(id))).collect();
        for (id, to) in aboard {
            if self.buildings.contains_key(to) {
                self.step_off_ferry(id, to);
            } else {
                let pos = self.citizens[id].pos;
                self.citizens[id].transit = None;
                self.strand_driver(id, pos);
            }
        }
    }

    /// [ferry-islands] Every ten minutes while ferries run: anyone still
    /// waiting on the pier when the last boat has gone settles in for the
    /// night (said once); and nobody is left aboard a boat that is gone.
    pub(in crate::world) fn step_ferry_piers(&mut self) {
        let tick = self.clock.tick;
        if tick % 10 != 7 || self.ports.links.is_empty() {
            return;
        }
        if tick % 30 == 7 {
            self.ferries_after_load();
            // Trips to a pier that never got there.
            let citizens = &self.citizens;
            self.ports.to_pier.retain(|t| citizens.contains_key(t.citizen) && tick < t.since + 8 * 60);
        }
        let m = self.clock.minute_of_day() as u32;
        self.catch_the_last_boat(m);
        let waiting: Vec<(CitizenId, BuildingId, BuildingId)> = self.citizens.iter().filter(|(id, c)| matches!(c.activity, Activity::Waiting { .. }) && !self.ports.missed.contains(id)).filter_map(|(id, c)| match c.transit.map(|t| t.leg) {
            Some(TransitLeg::Ferry { board, alight }) => Some((id, board, alight)),
            _ => None,
        }).collect();
        for (id, board, alight) in waiting {
            if self.boats_per_minute(board, alight, m) > 0.0 {
                continue;
            }
            self.ports.missed.insert(id);
            self.ports.stats.missed_last_boat_today += 1;
            if let Some(p) = self.ports.ports.values_mut().find(|p| p.building == Some(board)) {
                p.timeline.miss(m / 60);
            }
            let first = self.ports.port_of(board).map(|p| p.schedule().next_running(tick, crate::timetable::Kind::Ferry)).unwrap_or(tick);
            let text = format!("Missed the last boat. First one's at {}. Long night on the pier.", crate::timetable::hm((first % 1440) as u32));
            let c = &mut self.citizens[id];
            c.thought = text.clone();
            c.remember(tick, text.clone());
            c.commute.feel(TripMode::Ferry, 3.0);
            self.concerns.said(tick, id, &self.citizens[id], crate::concerns::Concern::Commute, 0.6, &text);
        }
    }

    /// [ferry-islands] Islanders out on the mainland (at a shop, a night
    /// out, a bench, or at work on a late shift) set off home when the last
    /// boats for their island stop within two hours (a terminal sends a boat every hour and a half or so).
    fn catch_the_last_boat(&mut self, m: u32) {
        if self.connectivity.islands.is_empty() {
            return;
        }
        // Islands whose boats from the mainland stop within the hour.
        let closing: Vec<usize> = (0..self.connectivity.islands.len())
            .filter(|&k| {
                let i = &self.connectivity.islands[k];
                let runs = |t: u32| i.mainland.iter().any(|&o| i.terminals.iter().any(|&x| self.boats_per_minute(o, x, t % 1440) > 0.0));
                runs(m) && !runs(m + LAST_BOAT_LEAD)
            })
            .collect();
        if closing.is_empty() {
            return;
        }
        let tick = self.clock.tick;
        let going: Vec<(CitizenId, BuildingId)> = self
            .citizens
            .iter()
            .filter(|(_, c)| matches!(c.activity, Activity::Visiting(_) | Activity::Sitting { .. } | Activity::Working | Activity::Idle) && !c.asleep)
            .filter_map(|(id, c)| {
                let home = c.home?;
                let k = self.island_of(home)?;
                if !closing.contains(&k) || c.inside.is_some_and(|b| self.island_of(b) == Some(k)) {
                    return None;
                }
                // Out on the mainland, not on the island's own ground.
                let here = c.inside.and_then(|b| self.building_entrance(b)).unwrap_or(c.pos);
                (self.walk_comp_at(here) != Some(self.connectivity.islands[k].comp)).then_some((id, home))
            })
            .collect();
        for (id, home) in going {
            let c = &mut self.citizens[id];
            let leaving_work = matches!(c.activity, Activity::Working);
            c.thought = if leaving_work { "Off early to make the last boat home.".to_string() } else { "Last boat soon. Better head for the pier.".to_string() };
            c.next_decision_tick = tick + 1;
            self.travel_to(id, Some(home), Purpose::Home);
        }
    }

    // ── freight ──────────────────────────────────────────────────────────

    /// [ferry-islands] The island (index) a building's lot is on.
    pub fn island_of(&self, b: BuildingId) -> Option<usize> {
        let lot = self.buildings.get(b)?.lot;
        self.connectivity.island_lots.get(&lot).copied()
    }

    /// [ferry-islands] A load for `cargo.to` that no road reaches from
    /// where it sets off: when `cargo.to` is on an island, it waits at the
    /// mainland terminal of the crossing (true); false when no ferry helps.
    pub(in crate::world) fn queue_ferry_load(&mut self, from_building: Option<BuildingId>, cargo: crate::traffic::Cargo) -> bool {
        let Some(k) = self.island_of(cargo.to) else { return false };
        let island = &self.connectivity.islands[k];
        // From a business on an island (this one or another): not by boat.
        if from_building.is_some_and(|b| self.island_of(b).is_some()) {
            return false;
        }
        // The mainland end with the shortest crossing, a car ferry first.
        let mut best: Option<(bool, f32, BuildingId, BuildingId)> = None;
        for &t in &island.terminals {
            for l in self.ferry_links_at(t) {
                let Some(m) = l.other(t) else { continue };
                if !island.mainland.contains(&m) {
                    continue;
                }
                let key = (!l.cars, l.metres);
                if best.is_none_or(|(c, d, _, _)| key < (c, d)) {
                    best = Some((!l.cars, l.metres, m, t));
                }
            }
        }
        let Some((_, _, at, to)) = best else { return false };
        let since = self.clock.tick;
        self.ports.ferry_freight.push(FerryLoad { at, to, cargo, since });
        true
    }

    /// [ferry-islands] Loads at `at` for `to` go aboard ship `sid`: lorries
    /// onto a car ferry's deck, pallets into a passenger boat's hold.
    pub(in crate::world) fn ferry_load_freight(&mut self, sid: crate::ports::ShipId, at: BuildingId, to: BuildingId) {
        if self.ports.ferry_freight.is_empty() {
            return;
        }
        let Some(ship) = self.ports.ships.get(sid) else { return };
        let car = ship.kind == ShipKind::CarFerry;
        let deck = self.ports.port_of(ship.port).map_or(0, |p| p.kind.car_deck());
        let room = if car { (deck / ports::LORRY_LANES).max(1) as usize } else { ports::HOLD_LOADS };
        let room = room.saturating_sub(ship.cargo.len());
        if room == 0 {
            return;
        }
        let mut taken = Vec::new();
        let mut keep = Vec::new();
        for l in std::mem::take(&mut self.ports.ferry_freight) {
            if l.at == at && l.to == to && taken.len() < room {
                taken.push(l.cargo);
            } else {
                keep.push(l);
            }
        }
        self.ports.ferry_freight = keep;
        let ship = &mut self.ports.ships[sid];
        if car {
            ship.cars += taken.len() as u32;
        }
        ship.cargo.extend(taken);
    }

    /// [ferry-islands] Ship `sid` alongside at `at`: the loads for there come
    /// off; off a car ferry each lorry drives on to its door, off a passenger
    /// boat porters carry the pallets.
    pub(in crate::world) fn ferry_unload_freight(&mut self, sid: crate::ports::ShipId, at: BuildingId) {
        let Some(ship) = self.ports.ships.get_mut(sid) else { return };
        if ship.cargo.is_empty() || !ship.kind.is_ferry() {
            return;
        }
        let car = ship.kind == ShipKind::CarFerry;
        let loads = std::mem::take(&mut ship.cargo);
        if car {
            ship.cars = ship.cars.saturating_sub(loads.len() as u32);
        }
        let n = loads.len() as u32;
        self.ports.stats.loads_ferried_today += n;
        for cargo in loads {
            if car && self.lorry_off_ferry(at, cargo) {
                self.ports.stats.lorries_ferried_today += 1;
                continue;
            }
            self.deliver(cargo);
        }
    }

    /// A lorry off the car deck at terminal `at`, driving on to `cargo.to`;
    /// false when no street on the island leads there.
    fn lorry_off_ferry(&mut self, at: BuildingId, cargo: crate::traffic::Cargo) -> bool {
        let Some((s0, p0)) = self.truck_stop(at) else { return false };
        let Some((s1, p1)) = self.truck_stop(cargo.to) else { return false };
        let Some((route, a, b)) = self.traffic.net.find_route(s0, p0, s1, p1) else { return false };
        let to_site = self.buildings.get(cargo.to).is_some_and(|b| b.construction.is_some());
        let id = self.traffic.spawn_car(CitizenId::default(), Some(cargo), route, a, b, 7);
        let mut rng = crate::vehicle::model_rng(self.clock.tick, id.data().as_ffi());
        self.traffic.cars[id].model = crate::vehicle::VehicleModel::for_cargo(cargo.resource, None, to_site, true, &mut rng);
        if to_site {
            if let Some(c) = self.builder_colour(cargo.to) {
                self.traffic.cars[id].model.livery = crate::vehicle::Livery::fleet(crate::vehicle::Livery::Construction, c);
            }
        }
        true
    }

    /// [ferry-islands] The terminal a lorry on the island of building `at`
    /// drives back to for the car ferry off it (None off the islands, or
    /// with no car ferry: then it is not on the island at all).
    pub(in crate::world) fn island_pier_for(&self, at: BuildingId) -> Option<BuildingId> {
        let k = self.island_of(at)?;
        let i = &self.connectivity.islands[k];
        i.terminals.iter().copied().find(|t| self.ports.kind_of(*t).is_some_and(|k| k.car_deck() > 0))
    }

    /// [ferry-islands] Can a building site's crews and materials get there
    /// by ferry (its lot is on an island with boats to the city)?
    pub(in crate::world) fn island_supplied(&self, bid: BuildingId) -> bool {
        self.island_of(bid).is_some_and(|k| !self.connectivity.islands[k].mainland.is_empty())
    }

    /// [ferry-islands] A load that waited at the pier for days (its boat
    /// stopped calling, the terminal went): delivered as it would have been
    /// with no way there at all.
    pub(in crate::world) fn daily_ferry_freight(&mut self) {
        let tick = self.clock.tick;
        let (stale, keep): (Vec<FerryLoad>, Vec<FerryLoad>) = std::mem::take(&mut self.ports.ferry_freight).into_iter().partition(|l| tick > l.since + 2 * 1440 || !self.buildings.contains_key(l.at) || !self.buildings.contains_key(l.to));
        self.ports.ferry_freight = keep;
        for l in stale {
            self.deliver(l.cargo);
        }
    }

    // ── island life ──────────────────────────────────────────────────────

    /// [ferry-islands] Minutes of a crossing from island `k` now: the
    /// shortest sailing and the wait for it.
    pub(in crate::world) fn island_crossing(&self, k: usize, m: u32) -> Option<(f32, f32)> {
        let island = self.connectivity.islands.get(k)?;
        let mut best: Option<(f32, f32)> = None;
        for &t in &island.terminals {
            for l in self.ferry_links_at(t) {
                let Some(o) = l.other(t).filter(|o| island.mainland.contains(o)) else { continue };
                let Some(wait) = self.ferry_wait(t, o, m) else { continue };
                let sail = self.sail_minutes(l.metres);
                if best.is_none_or(|(s, w)| sail + wait < s + w) {
                    best = Some((sail, wait));
                }
            }
        }
        best
    }

    /// [ferry-islands] How much a household of `people` (age, seed, job)
    /// would like an island home `home`: 0..1. Retirees and the quiet
    /// like it; jobs on the island suit; a long crossing to work, or boats
    /// that stop before a shift ends (or start after it begins), do not.
    pub(in crate::world) fn island_appeal(&self, people: &[(u8, u64, Option<BuildingId>, f32)], home: BuildingId) -> f32 {
        let Some(k) = self.island_of(home) else { return 1.0 };
        let mut appeal: f32 = 0.45;
        let adults: Vec<&(u8, u64, Option<BuildingId>, f32)> = people.iter().filter(|p| LifeStage::of_age(p.0) != LifeStage::Child).collect();
        if adults.iter().all(|p| LifeStage::of_age(p.0) == LifeStage::Retiree) {
            appeal += 0.3;
        }
        // The quiet: the less sociable and the more cautious.
        if let Some(p) = adults.first() {
            let [sociable, _, _, caution, _] = crate::vehicle::traits_of(p.1);
            appeal += 0.25 * (caution - sociable);
        }
        let (sail, wait) = self.island_crossing(k, 8 * 60).unwrap_or((60.0, 60.0));
        for p in &adults {
            let Some(job) = p.2 else { continue };
            if self.island_of(job) == Some(k) {
                appeal += 0.15;
                continue;
            }
            appeal -= ((sail + wait - 15.0) / 60.0).clamp(0.0, 0.5);
            // The boats there and back round the shift.
            let Some(b) = self.buildings.get(job) else { continue };
            let shift = b.shift_for(p.1);
            if shift == crate::clock::Hours::ALWAYS {
                continue;
            }
            let (start, end) = (shift.open as u32 * 60, shift.close as u32 * 60);
            let t = &self.connectivity.islands[k];
            let runs = |m: u32| t.terminals.iter().any(|&x| t.mainland.iter().any(|&y| self.boats_per_minute(x, y, m % 1440) > 0.0));
            if !runs(start.saturating_sub(60)) || !runs(end + 30) {
                appeal -= 0.45;
            }
        }
        appeal.clamp(0.0, 1.0)
    }

    /// [ferry-islands] Does the household take island home `home`? By its
    /// liking (`island_appeal`) against a hash of who they are and where.
    pub(in crate::world) fn takes_island_home(&self, people: &[(u8, u64, Option<BuildingId>, f32)], home: BuildingId) -> bool {
        if self.island_of(home).is_none() {
            return true;
        }
        let who = people.first().map_or(0, |p| p.1);
        unit(mix(who ^ 0x15_1A4D, home.data().as_ffi())) < self.island_appeal(people, home)
    }

    /// [ferry-islands] Once a day: a household on an island whose earners
    /// the crossing has worn down moves to the mainland, if a home there
    /// suits (a hash of the household and the day decides).
    pub(in crate::world) fn island_move(&mut self, hid: HouseholdId, income: f32) -> bool {
        let Some(home) = self.households.get(hid).and_then(|h| h.home) else { return false };
        let Some(k) = self.island_of(home) else { return false };
        let members = self.households[hid].members.clone();
        let worn = members.iter().filter_map(|m| self.citizens.get(*m)).any(|c| c.work.is_some_and(|w| self.island_of(w) != Some(k)) && c.commute.pain_of(TripMode::Ferry) > FERRY_MOVE_PAIN);
        if !worn || unit(mix(hid.data().as_ffi() ^ 0xFE_4417, self.clock.day())) >= FERRY_MOVE_CHANCE {
            return false;
        }
        let tier = self.buildings.get(home).map_or(Tier::Low, |b| b.tier);
        let budget = income * trips::AFFORDABLE_SHARE;
        let to = self
            .buildings
            .iter()
            .filter(|(id, b)| *id != home && b.land_use == LandUse::Residential && b.operating() && b.free_homes() > 0 && b.annex.is_none() && b.tier >= tier && b.rent <= budget)
            .filter(|(id, _)| self.island_of(*id).is_none())
            .min_by(|a, b| a.1.rent.total_cmp(&b.1.rent).then(a.0.cmp(&b.0)))
            .map(|(id, _)| id);
        let Some(to) = to else { return false };
        self.move_household(hid, to, "Sick of the ferry twice a day. Moved to {} on the mainland.");
        self.ports.stats.islanders_moved_off_today += 1;
        true
    }

    /// [ferry-islands] Once a day: someone on the mainland with a job on an
    /// island the crossing has worn down (the boats do not run round their
    /// shift) gives it up; a hash of them and the day decides.
    pub(in crate::world) fn island_job_quit(&mut self, m: CitizenId) {
        let Some(c) = self.citizens.get(m) else { return };
        let Some(job) = c.work else { return };
        let Some(k) = self.island_of(job) else { return };
        if c.home.is_some_and(|h| self.island_of(h) == Some(k)) || c.commute.pain_of(TripMode::Ferry) <= FERRY_MOVE_PAIN {
            return;
        }
        if unit(mix(c.seed ^ 0x0F_F15E, self.clock.day())) >= FERRY_MOVE_CHANCE {
            return;
        }
        let name = self.buildings.get(job).map(|b| b.name.clone()).unwrap_or_default();
        self.lay_off(m, Some(format!("Gave up the job at {name}. The ferry there and back was killing me.")));
        self.ports.stats.island_jobs_quit_today += 1;
    }

    /// [ferry-islands] A job on the citizen's own island, if one is open.
    pub(in crate::world) fn island_job_for(&mut self, id: CitizenId) -> Option<BuildingId> {
        let k = self.citizens.get(id)?.home.and_then(|h| self.island_of(h))?;
        let lots = &self.connectivity.islands.get(k)?.lots;
        let mut open: Vec<BuildingId> = lots.iter().filter_map(|l| self.lots.get(*l).and_then(|l| l.building)).filter(|b| self.buildings.get(*b).is_some_and(|x| x.free_jobs() > 0 && (x.operating() || x.construction.is_some()))).collect();
        open.sort();
        open.dedup();
        if open.is_empty() {
            return None;
        }
        let c = &self.citizens[id];
        Some(open[(mix(c.seed, self.clock.tick) % open.len() as u64) as usize])
    }

    /// [ferry-islands] The land on an island: quiet, with water all round,
    /// worth more, less what the crossing costs in time.
    pub(super) fn splat_islands(&self, b: &mut crate::land_value::LandValueBuilder) {
        for k in 0..self.connectivity.islands.len() {
            let (sail, wait) = self.island_crossing(k, 8 * 60).unwrap_or((60.0, 60.0));
            let w = ISLAND_CALM - (sail + wait - CALM_MINUTES).max(0.0) * PER_FERRY_MINUTE;
            for l in &self.connectivity.islands[k].lots {
                if let Some(lot) = self.lots.get(*l) {
                    b.splat(lot.center, 50.0, w * 0.5);
                }
            }
        }
    }

    /// [ferry-islands] Every island, for the panels and the remote API.
    pub fn islands(&self) -> Vec<IslandInfo> {
        let m = self.clock.minute_of_day() as u32;
        self.connectivity
            .islands
            .iter()
            .enumerate()
            .map(|(k, i)| {
                let has = |kind: crate::services::ServiceKind| i.buildings.iter().any(|b| self.buildings.get(*b).is_some_and(|x| x.service.is_some_and(|s| s.kind == kind)));
                let crossing = self.island_crossing(k, m);
                let name = i.terminals.first().and_then(|t| self.buildings.get(*t)).map_or_else(|| "An island".to_string(), |b| format!("The island by {}", b.name));
                IslandInfo {
                    center: i.center,
                    name,
                    terminals: i.terminals.clone(),
                    mainland: i.mainland.clone(),
                    car_ferry: i.car_ferry,
                    buildings: i.buildings.len() as u32,
                    residents: i.residents,
                    workers: i.workers,
                    sail_minutes: crossing.map_or(0.0, |c| c.0),
                    wait_minutes: crossing.map(|c| c.1),
                    police: has(crate::services::ServiceKind::Police),
                    fire: has(crate::services::ServiceKind::Fire),
                    health: has(crate::services::ServiceKind::Health),
                    loads_waiting: self.ports.ferry_freight.iter().filter(|l| i.terminals.contains(&l.to)).count() as u32,
                }
            })
            .collect()
    }

    /// [ferry-islands] Find the islands among the walk networks cut off
    /// from the main one: a network with a ferry terminal that has boats
    /// to a terminal on the main network. `comps` gives each lot's walk
    /// networks. Returns the islands (lots, buildings and people filled in
    /// by the caller's groups).
    pub(in crate::world) fn find_islands(&self, main: u32, comps: &HashMap<LotId, Vec<u32>>) -> Vec<Island> {
        let mut out: Vec<Island> = Vec::new();
        if self.ports.links.is_empty() {
            return out;
        }
        let comps_of = |b: BuildingId| self.buildings.get(b).and_then(|x| comps.get(&x.lot)).cloned().unwrap_or_default();
        let works = |b: BuildingId| self.ports.port_of(b).is_some_and(|p| p.works());
        for l in &self.ports.links {
            for (x, y) in [(l.a, l.b), (l.b, l.a)] {
                let (cx, cy) = (comps_of(x), comps_of(y));
                if cx.is_empty() || cx.contains(&main) || !cy.contains(&main) || !works(x) || !works(y) {
                    continue;
                }
                let comp = *cx.iter().min().unwrap();
                match out.iter_mut().find(|i| i.comp == comp) {
                    Some(i) => {
                        if !i.terminals.contains(&x) {
                            i.terminals.push(x);
                        }
                        if !i.mainland.contains(&y) {
                            i.mainland.push(y);
                        }
                        i.car_ferry |= l.cars;
                    }
                    None => out.push(Island { comp, terminals: vec![x], mainland: vec![y], car_ferry: l.cars, ..Default::default() }),
                }
            }
        }
        out
    }
}
