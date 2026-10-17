//! [insight] Recording the city's memory (`crate::insight`) once a tick,
//! after everything has moved: the traffic model's notes (street pieces
//! driven onto, re-plans) go into each vehicle's trip log, and the rest is
//! read off the state by noticing what changed since last tick: a stall
//! begun or ended and what held it, anger, a crash, the errand (the parking
//! hunt's steps); each citizen's activity (the legs of a trip), home, job,
//! car and household; each building's site, owner, business and households;
//! new and settled incidents. Trips choosing a mode and routes
//! that failed are noted where they are decided (`note_trip`,
//! `note_no_route`).
//!
//! Recording only: nothing reads it back into the simulation; no random
//! numbers.

use super::*;
use crate::insight::{ffi, Act, BuildingSeen, Ev, Happened, Life, Note, PersonSeen, Rec, Trip, TripKind, Why};
use crate::traffic::{Car, Wait};
use crate::insight::{BUILDING_BYTES, DONE_TRIPS, DONE_TRIP_BYTES, LIFE_BYTES, PERSON_BYTES, STALL_MIN, TRIP_BYTES, ANGER_STEP};
use crate::vehicle::Errand;

/// The `Activity` as recorded (`insight::Act`): its variant and what it names.
pub fn act_of(c: &Citizen) -> Act {
    let k = |b: Option<BuildingId>| ffi(b);
    let (tag, with) = match &c.activity {
        Activity::Arriving => (0, 0),
        // Home and work are the citizen's own: not repeated in every record.
        Activity::AtHome => (1, 0),
        Activity::Working => (2, 0),
        Activity::Visiting(b) => (3, k(Some(*b))),
        Activity::Idle => (4, 0),
        Activity::Traveling { to, .. } => (5, k(*to)),
        Activity::Driving { car, .. } => (6, ffi(Some(*car))),
        Activity::Sitting { park, .. } => (7, k(Some(*park))),
        Activity::Waiting { .. } => (8, 0),
        Activity::Riding { bus } => (9, ffi(Some(*bus))),
        Activity::OnTrain { train } => (10, ffi(Some(*train))),
        Activity::Flying { drone } => (11, ffi(Some(*drone))),
        Activity::OnMetro { train } => (12, ffi(Some(*train))),
        Activity::Passenger { driver, .. } => (13, ffi(Some(*driver))),
        Activity::Abroad { terminal, .. } => (14, k(Some(*terminal))),
    };
    let mode = if tag == 5 && c.mode == TravelMode::Bike { 1 } else { 0 };
    Act { tag, with, mode }
}

/// An errand as recorded: its tag (`insight::ERRAND_NAMES`), the hunt's
/// tries and the building it names.
pub fn errand_of(e: Errand) -> (u8, u8, u64) {
    match e {
        Errand::Trip => (0, 0, 0),
        Errand::Search { tries } => (1, tries, 0),
        Errand::ToLot(b) => (2, 0, ffi(Some(b))),
        Errand::ToKerb => (3, 0, 0),
        Errand::Sale(b) => (4, 0, ffi(Some(b))),
        Errand::Deliver => (5, 0, 0),
        Errand::Leave => (6, 0, 0),
        Errand::Haul { base } => (7, 0, ffi(base)),
        Errand::Return(b) => (8, 0, ffi(Some(b))),
        Errand::Patrol(b) => (9, 0, ffi(Some(b))),
        Errand::Tow(_) => (10, 0, 0),
        Errand::Recover(_) => (11, 0, 0),
        Errand::Taxi(_) => (12, 0, 0),
        Errand::Through => (13, 0, 0),
        Errand::Out => (14, 0, 0),
    }
}

/// One point of a trip replayed: where, and when (a fractional tick).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReplayPoint {
    pub at: Vec2,
    pub tick: f64,
    /// The street piece it was on (as recorded), if known.
    pub segment: Option<u32>,
}

impl World {
    /// [insight] The once-a-tick look (see the module notes); run by
    /// `tick`, public for `examples/insight_probe`.
    pub fn step_insight(&mut self) {
        let synced = self.insight.synced;
        self.insight_vehicles();
        self.insight_people(synced);
        self.insight_incidents();
        self.insight_buildings(synced);
        self.insight.synced = true;
    }

    fn trip_kind(&self, id: CarId, c: &Car) -> TripKind {
        if c.bus.is_some() {
            TripKind::Bus
        } else if c.siren || c.drive.amber || self.services.unit_of_car(id).is_some() {
            TripKind::Service
        } else if matches!(c.errand, Errand::Taxi(_)) {
            TripKind::Taxi
        } else if c.errand == Errand::Through {
            TripKind::Through
        } else if c.cargo.is_some() {
            TripKind::Lorry
        } else if matches!(c.errand, Errand::Haul { .. } | Errand::Return(_) | Errand::Patrol(_) | Errand::Tow(_) | Errand::Recover(_) | Errand::Out) {
            TripKind::Fleet
        } else if c.owned.is_some() || self.citizens.contains_key(c.citizen) {
            TripKind::Private
        } else {
            TripKind::Other
        }
    }

    /// Vehicles: notes in, trips begun and ended, and what changed.
    fn insight_vehicles(&mut self) {
        let tick = self.clock.tick;
        let notes = std::mem::take(&mut self.traffic.notes);
        // Notes for trips already kept first (a car gone this tick still
        // gets its last), the rest once new trips are open.
        let mut later = Vec::new();
        for (id, n) in notes {
            if self.insight.live.contains_key(id) {
                self.apply_note(id, n);
            } else {
                later.push((id, n));
            }
        }
        // Gone from the road: the trip is over.
        let gone: Vec<CarId> = self.insight.live.keys().filter(|&id| !self.traffic.cars.contains_key(id)).collect();
        for id in gone {
            if let Some(mut t) = self.insight.live.remove(id) {
                let at = if t.watch.pos != Vec2::ZERO { t.watch.pos } else { t.log.records().last().map_or(Vec2::ZERO, |r| r.at) };
                t.log.push(Rec { tick, at, ev: Ev::End(crate::insight::End::Left) }, TRIP_BYTES);
                t.end = Some(tick);
                t.log.trim(DONE_TRIP_BYTES);
                match t.driver.and_then(|d| self.insight.people.get_mut(d)) {
                    Some(p) => p.last_drive = Some(Box::new(t)),
                    None => {
                        self.insight.done.push_back(t);
                        while self.insight.done.len() > DONE_TRIPS {
                            self.insight.done.pop_front();
                        }
                    }
                }
            }
        }
        // New on the road.
        let fresh: Vec<CarId> = self.traffic.cars.keys().filter(|&id| !self.insight.live.contains_key(id)).collect();
        for id in fresh {
            let c = &self.traffic.cars[id];
            let driver = self.citizens.contains_key(c.citizen).then_some(c.citizen);
            let to = match driver.and_then(|d| self.citizens.get(d)).map(|d| &d.activity) {
                Some(Activity::Driving { to, .. }) => *to,
                _ => c.cargo.map(|g| g.to).filter(|b| self.buildings.contains_key(*b)),
            };
            let mut t = Trip { car: id, kind: self.trip_kind(id, c), vehicle: c.owned, driver, to, start: tick, ..Default::default() };
            t.watch.errand = errand_of(c.errand);
            t.log.push(Rec { tick, at: c.pos, ev: Ev::Start }, TRIP_BYTES);
            self.insight.live.insert(id, t);
        }
        for (id, n) in later {
            if self.insight.live.contains_key(id) {
                self.apply_note(id, n);
            }
        }
        // What changed on each car.
        let secs = self.config.traffic_seconds_per_tick;
        let ins = &mut self.insight;
        for (id, c) in self.traffic.cars.iter() {
            let Some(t) = ins.live.get_mut(id) else { continue };
            let w = &mut t.watch;
            w.pos = c.pos;
            match w.stall {
                None if c.stalled > 0.0 => w.stall = Some((tick, c.wait, c.blocked_by, c.pos)),
                Some((since, cause, blocker, at)) if c.stalled > 0.0 => {
                    // The first real reason, and the latest car in the way.
                    let cause = if cause == Wait::Free { c.wait } else { cause };
                    w.stall = Some((since, cause, c.blocked_by.or(blocker), at));
                }
                Some((since, cause, blocker, at)) => {
                    let seconds = (tick - since) as f32 * secs;
                    if seconds >= STALL_MIN {
                        t.log.push(Rec { tick, at, ev: Ev::Stall { seconds: seconds as u32, cause, blocker } }, TRIP_BYTES);
                    }
                    w.stall = None;
                }
                None => {}
            }
            let w = &mut t.watch;
            let pct = (c.drive.anger.clamp(0.0, 1.0) * 100.0).round() as u8;
            if pct.abs_diff(w.anger) >= ANGER_STEP || (pct == 0 && w.anger > 0) {
                w.anger = pct;
                t.log.push(Rec { tick, at: c.pos, ev: Ev::Anger { pct } }, TRIP_BYTES);
            }
            let w = &mut t.watch;
            if c.crashed() != w.crashed {
                w.crashed = c.crashed();
                if w.crashed {
                    t.log.push(Rec { tick, at: c.pos, ev: Ev::Crash }, TRIP_BYTES);
                    if let Some(p) = t.driver.and_then(|d| ins.people.get_mut(d)) {
                        p.life.push(Rec { tick, at: c.pos, ev: Ev::Life { what: Life::Crash, other: ffi(Some(id)), text: String::new() } }, LIFE_BYTES);
                    }
                }
            }
            let e = errand_of(c.errand);
            if e != t.watch.errand {
                t.watch.errand = e;
                t.log.push(Rec { tick, at: c.pos, ev: Ev::Errand { what: e.0, tries: e.1, building: crate::insight::unffi(e.2) } }, TRIP_BYTES);
            }
        }
    }

    /// One traffic note into its car's trip.
    fn apply_note(&mut self, id: CarId, n: Note) {
        let tick = self.clock.tick;
        let Some(c) = self.traffic.cars.get(id) else {
            // Gone already: only where it went counts.
            if let (Note::Entered { segment, forward, at }, Some(t)) = (n, self.insight.live.get_mut(id)) {
                t.log.push(Rec { tick, at, ev: Ev::Enter { segment, forward } }, TRIP_BYTES);
            }
            return;
        };
        let (pos, errand, emergency) = (c.pos, c.errand, c.siren || c.drive.fleeing);
        let rec = match n {
            Note::Entered { segment, forward, at } => {
                let Some(t) = self.insight.live.get_mut(id) else { return };
                // Back along the street it just drove: a U-turn, at a dead
                // end if the node it turned at has no other way on.
                if t.watch.last == Some((segment, !forward)) {
                    let dead_end = self.roads.try_segment(SegmentId(segment)).is_some_and(|s| {
                        let node = if forward { s.a } else { s.b };
                        self.roads.neighbors(node).count() <= 1
                    });
                    t.log.push(Rec { tick, at, ev: Ev::UTurn { dead_end } }, TRIP_BYTES);
                }
                t.watch.last = Some((segment, forward));
                Rec { tick, at, ev: Ev::Enter { segment, forward } }
            }
            Note::Replan { why, avoided, before_m, after_m } => {
                // The world names a redirect by the errand behind it.
                let why = match why {
                    _ if emergency && matches!(why, Why::Redirect | Why::NewRoute) => Why::Emergency,
                    Why::Redirect => match errand {
                        Errand::Search { .. } | Errand::ToLot(_) | Errand::ToKerb => Why::ParkingHunt,
                        Errand::Trip => Why::Redirect,
                        _ => Why::Errand,
                    },
                    w => w,
                };
                Rec { tick, at: pos, ev: Ev::Replan { why, avoided, before_m, after_m } }
            }
            Note::RageUTurn => Rec { tick, at: pos, ev: Ev::RageUTurn },
        };
        // The same re-plan as one of the last few records (same reason,
        // round the same place): counted, not written again.
        if let (Ev::Replan { why, avoided, .. }, Some(t)) = (&rec.ev, self.insight.live.get_mut(id)) {
            let recent = t.log.records();
            let same = recent.iter().rev().take(6).any(|r| matches!(&r.ev, Ev::Replan { why: w, avoided: a, .. } if w == why && r.at.distance(rec.at) < 3.0 && match (a, avoided) { (Some(a), Some(b)) => a.distance(*b) < 3.0, (None, None) => true, _ => false }));
            if same {
                t.watch.repeats += 1;
                return;
            }
        }
        if let Some(t) = self.insight.live.get_mut(id) {
            t.log.push(rec, TRIP_BYTES);
        }
    }

    /// Citizens: each change of activity (a leg, arriving somewhere), and
    /// life events read off the state (new home, job, car, household).
    fn insight_people(&mut self, synced: bool) {
        let tick = self.clock.tick;
        let ins = &mut self.insight;
        for (id, c) in self.citizens.iter() {
            let Some(p) = ins.people.entry(id).map(|e| e.or_default()) else { continue };
            let now = PersonSeen { act: act_of(c), home: ffi(c.home), work: ffi(c.work), car: ffi(c.car), household: ffi(c.household) };
            let Some(seen) = p.seen else {
                if synced {
                    p.life.push(Rec { tick, at: c.pos, ev: Ev::Life { what: Life::Arrived, other: now.home, text: String::new() } }, LIFE_BYTES);
                }
                p.seen = Some(now);
                continue;
            };
            if seen == now {
                continue;
            }
            if seen.act != now.act {
                p.trips.push(Rec { tick, at: c.pos, ev: Ev::Act(now.act) }, PERSON_BYTES);
            }
            for (what, a, b) in [(Life::Home, seen.home, now.home), (Life::Job, seen.work, now.work), (Life::Car, seen.car, now.car), (Life::Household, seen.household, now.household)] {
                if a != b {
                    p.life.push(Rec { tick, at: c.pos, ev: Ev::Life { what, other: b, text: String::new() } }, LIFE_BYTES);
                }
            }
            p.seen = Some(now);
        }
        if tick % 60 == 0 {
            let citizens = &self.citizens;
            ins.people.retain(|k, _| citizens.contains_key(k));
        }
    }

    /// [insight] A trip set off: the mode taken (`modes[k]`, of the ranked
    /// `(mode, minutes)`; those before it could not be done), the runner-up,
    /// what for and where. `fresh` false: a later leg of the same trip.
    pub(super) fn note_trip(&mut self, id: CitizenId, modes: &[(u8, f32)], k: usize, purpose: Purpose, target: BuildingId, fresh: bool) {
        let Some(&(mode, minutes)) = modes.get(k) else { return };
        let Some(c) = self.citizens.get(id) else { return };
        let failed = modes[..k].iter().fold(0u8, |m, (md, _)| m | 1u8.checked_shl(*md as u32).unwrap_or(0));
        let alt = modes.iter().skip(k + 1).find(|(m, _)| *m != mode).copied();
        let rec = Rec { tick: self.clock.tick, at: c.pos, ev: Ev::Trip { mode, purpose, to: Some(target), minutes, alt, failed, leg: !fresh } };
        if let Some(p) = self.insight.people.entry(id).map(|e| e.or_default()) {
            p.trips.push(rec, PERSON_BYTES);
        }
    }

    /// [insight] No way to `target` (as the no-route alert says why).
    pub(super) fn note_no_route(&mut self, id: CitizenId, from: Vec2, target: BuildingId, why: &str) {
        let rec = Rec { tick: self.clock.tick, at: from, ev: Ev::NoRoute { to: Some(target), why: why.to_string() } };
        if let Some(p) = self.insight.people.entry(id).map(|e| e.or_default()) {
            p.trips.push(rec, PERSON_BYTES);
        }
    }

    /// [insight] Something happened to a building.
    pub fn note_building(&mut self, id: BuildingId, what: Happened, other: u64, text: String) {
        let at = self.building_lot(id).map_or(Vec2::ZERO, |l| l.center);
        let rec = Rec { tick: self.clock.tick, at, ev: Ev::Happened { what, other, text } };
        if let Some(p) = self.insight.buildings.entry(id).map(|e| e.or_default()) {
            p.log.push(rec, BUILDING_BYTES);
        }
    }

    /// [insight] A life event for a citizen.
    pub fn note_life(&mut self, id: CitizenId, what: Life, other: u64, text: String) {
        let Some(at) = self.citizens.get(id).map(|c| c.pos) else { return };
        let rec = Rec { tick: self.clock.tick, at, ev: Ev::Life { what, other, text } };
        if let Some(p) = self.insight.people.entry(id).map(|e| e.or_default()) {
            p.life.push(rec, LIFE_BYTES);
        }
    }

    /// New incidents into their building's log (and the people in them),
    /// settled ones with how they ended.
    fn insight_incidents(&mut self) {
        let last = self.insight.last_incident;
        let mut new: Vec<(u32, BuildingId, String, Option<CitizenId>, Option<CitizenId>, crate::services::IncidentKind)> = Vec::new();
        let mut settled: Vec<(u32, BuildingId, String)> = Vec::new();
        for i in &self.services.incidents {
            if i.id > last {
                new.push((i.id, i.building, format!("{:?}", i.kind).to_lowercase(), i.citizen, i.suspect, i.kind));
            }
        }
        for &oid in &self.insight.open_incidents {
            match self.services.incidents.iter().find(|i| i.id == oid) {
                Some(i) if i.resolved.is_some() => settled.push((oid, i.building, i.outcome.map_or("over".to_string(), |o| format!("{o:?}").to_lowercase()))),
                _ => {}
            }
        }
        for (id, b, kind, who, suspect, k) in new {
            self.insight.last_incident = self.insight.last_incident.max(id);
            self.note_building(b, Happened::Incident, id as u64, kind.clone());
            let text = format!("{kind} at {}", self.buildings.get(b).map_or("a building", |b| b.name.as_str()));
            if let Some(c) = who {
                let what = match k {
                    crate::services::IncidentKind::Crime => Life::CrimeVictim,
                    _ => Life::Patient,
                };
                self.note_life(c, what, id as u64, text.clone());
            }
            if let Some(s) = suspect {
                self.note_life(s, Life::Suspect, id as u64, text);
            }
            if self.services.incidents.iter().any(|i| i.id == id && i.resolved.is_none()) {
                self.insight.open_incidents.push(id);
            }
        }
        for (id, b, how) in settled {
            self.note_building(b, Happened::Resolved, id as u64, how);
        }
        let incidents = &self.services.incidents;
        self.insight.open_incidents.retain(|&oid| incidents.iter().any(|i| i.id == oid && i.resolved.is_none()));
    }

    /// Buildings: what changed (see `Happened`). Every tick, so a loaded
    /// game notices the same changes at the same tick as one left running.
    fn insight_buildings(&mut self, synced: bool) {
        let mut events: Vec<(BuildingId, Happened, u64, String)> = Vec::new();
        let ins = &mut self.insight;
        for (id, b) in self.buildings.iter() {
            let Some(p) = ins.buildings.entry(id).map(|e| e.or_default()) else { continue };
            let business = b.business.as_ref().map(|z| (z.kind.label(), z.closed));
            let changed = |s: &BuildingSeen| {
                s.site != b.construction.is_some() || s.demolishing != b.demolition.is_some() || s.business != business || s.owner != ffi(b.plot.owner) || s.tier != b.tier.index() as u8 || s.floors != b.floors || s.households != b.households.len() as u16 || s.name != b.name
            };
            let fresh = p.seen.is_none();
            if p.seen.as_ref().is_some_and(|s| !changed(s)) {
                continue;
            }
            let now = BuildingSeen { site: b.construction.is_some(), demolishing: b.demolition.is_some(), business, owner: ffi(b.plot.owner), tier: b.tier.index() as u8, floors: b.floors, households: b.households.len() as u16, name: b.name.clone() };
            let seen = p.seen.replace(now.clone());
            let Some(s) = seen else {
                if fresh && synced {
                    events.push((id, Happened::Started, 0, if now.site { "site".into() } else { "placed".into() }));
                }
                continue;
            };
            let mut ev = |what, other, text: String| events.push((id, what, other, text));
            if s.site && !now.site && !now.demolishing {
                ev(Happened::Built, 0, String::new());
            }
            if !s.demolishing && now.demolishing {
                ev(Happened::Demolition, 0, b.demolition.as_ref().map_or(String::new(), |d| d.reason.label().to_string()));
            }
            match (s.business, now.business) {
                (None, Some((k, _))) => ev(Happened::Opened, 0, k.to_string()),
                (Some(_), None) => ev(Happened::Closed, 0, "business gone".into()),
                (Some((a, _)), Some((k, _))) if a != k => ev(Happened::Opened, 0, k.to_string()),
                (Some((_, false)), Some((_, true))) => ev(Happened::Closed, 0, String::new()),
                (Some((_, true)), Some((_, false))) => ev(Happened::Reopened, 0, String::new()),
                _ => {}
            }
            if s.owner != now.owner {
                ev(Happened::Owner, now.owner, String::new());
            }
            if s.tier != now.tier || s.floors != now.floors {
                ev(Happened::Rebuilt, 0, format!("{} tier, {} floors", b.tier.label(), b.floors));
            }
            if now.households > s.households {
                ev(Happened::MovedIn, (now.households - s.households) as u64, String::new());
            } else if now.households < s.households {
                ev(Happened::MovedOut, (s.households - now.households) as u64, String::new());
            }
            if s.name != now.name {
                ev(Happened::Renamed, 0, now.name.clone());
            }
        }
        for (id, what, other, text) in events {
            self.note_building(id, what, other, text);
        }
        if self.clock.tick % 60 == 0 {
            let buildings = &self.buildings;
            self.insight.buildings.retain(|k, _| buildings.contains_key(k));
        }
    }

    /// [insight] A trip's way as points with times: where it took to the
    /// road, each street piece it drove onto (along the piece's lane while
    /// the roads are as they were), and, for a car still on the road, where
    /// it is now.
    pub fn trip_replay(&self, t: &Trip) -> Vec<ReplayPoint> {
        let recs = t.log.records();
        let mut out: Vec<ReplayPoint> = Vec::new();
        let push = |out: &mut Vec<ReplayPoint>, at: Vec2, tick: f64, segment: Option<u32>| {
            if out.last().is_none_or(|p| p.at.distance(at) > 0.25) {
                out.push(ReplayPoint { at, tick, segment });
            }
        };
        // Street piece entries with their lanes' shapes, timed later.
        let mut path: Vec<(Vec2, u64, Option<u32>, Vec<Vec2>)> = Vec::new();
        for r in &recs {
            match r.ev {
                Ev::Start | Ev::End(_) => path.push((r.at, r.tick, None, Vec::new())),
                Ev::Enter { segment, forward } => {
                    let lane = self.traffic.net.segment_lanes.get(&SegmentId(segment)).into_iter().flatten().map(|&l| &self.traffic.net.lanes[l as usize]).find(|l| matches!(l.kind, crate::traffic::LaneKind::Road { forward: f, .. } if f == forward) && l.points.first().is_some_and(|p| p.distance(r.at) < 2.5));
                    path.push((r.at, r.tick, Some(segment), lane.map(|l| l.points[1..].to_vec()).unwrap_or_default()));
                }
                _ => {}
            }
        }
        let live = t.end.is_none();
        for (k, (at, tick, seg, shape)) in path.iter().enumerate() {
            let next = path.get(k + 1).map(|n| n.1).unwrap_or(*tick);
            push(&mut out, *at, *tick as f64, *seg);
            // The lane under a car still on it: only as far as the car.
            if live && k + 1 == path.len() {
                continue;
            }
            // The lane's shape, timed by its share of the way to the next entry.
            let len: f32 = std::iter::once(*at).chain(shape.iter().copied()).collect::<Vec<_>>().windows(2).map(|w| w[0].distance(w[1])).sum::<f32>().max(0.01);
            let mut run = 0.0;
            let mut prev = *at;
            for &p in shape {
                run += prev.distance(p);
                prev = p;
                let f = (run / len) as f64 * 0.9;
                push(&mut out, p, *tick as f64 + f * (next - *tick) as f64, *seg);
            }
        }
        if t.end.is_none() {
            if let Some(c) = self.traffic.cars.get(t.car) {
                push(&mut out, c.pos, self.clock.tick as f64, None);
            }
        }
        out
    }
}
