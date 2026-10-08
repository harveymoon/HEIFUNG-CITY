//! [events] Street races and warehouse raves, run night by night (the data
//! and geometry are in `crate::street_events`). Decisions hash the seed,
//! the day and the ids (`mix`); the few rolls that need a stream draw
//! from their own (`rng_for`), never the world's, so nothing else shifts.

use super::*;
use crate::citizens::{driver_traits, rebelliousness};
use crate::services::{CrimeKind, Outcome, Stage as IncidentStage};
use crate::street_events::*;
use crate::traffic::{DriverProfile, GangPhase, GangRole, LaneId, StuntKind};
use crate::vehicle::{BodyType, VehicleClass, VehicleModel};

/// A stable 64-bit mix of two numbers.
fn mix(a: u64, b: u64) -> u64 {
    let mut x = a ^ b.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn unit(h: u64) -> f32 {
    (h >> 40) as f32 / (1u64 << 24) as f32
}

fn key_of<K: slotmap::Key>(k: K) -> u64 {
    k.data().as_ffi()
}

/// A car fast enough to race: tuned, a coupé or a superbike, or an
/// expensive saloon or wedge.
pub fn fast_car(m: &VehicleModel) -> bool {
    m.tuned || matches!(m.body, BodyType::Coupe | BodyType::AkiraBike | BodyType::Motorcycle) || (m.class == VehicleClass::Expensive && matches!(m.body, BodyType::Sedan | BodyType::CyberTruck | BodyType::Hatchback))
}

/// Young night owls: the people a rave is for.
fn night_owl(c: &Citizen) -> bool {
    let (_, hedonism) = driver_traits(c.seed);
    (18..=34).contains(&c.age) && hedonism >= 0.55
}

impl World {
    /// Everything the nights do in a tick.
    pub(super) fn step_nights(&mut self) {
        let tick = self.clock.tick;
        let day = self.clock.day();
        let minute = self.clock.minute_of_day();
        // ── races ──
        if minute == RACE_ROLL_MINUTE as u64 && self.street_events.race_rolled_day != day && self.street_events.race.is_none() {
            self.street_events.race_rolled_day = day;
            let h = mix(self.config.seed ^ 0x5ACE_D0C5, day);
            let crew = self.race_crew().len();
            // Not on a takeover night: the same crowd is out at that.
            let takeover = self.driving.takeovers.active.is_some() || self.driving.takeovers.planned.is_some();
            if day >= SCENE_FROM_DAY && crew >= MIN_RACERS && self.citizens.len() >= 60 && !takeover && unit(h) < RACE_NIGHT_CHANCE as f32 {
                let window = RACE_START_WINDOW.1 - RACE_START_WINDOW.0;
                self.street_events.race_planned = Some(tick + RACE_START_WINDOW.0 + mix(h, 1) % window);
            }
        }
        if self.street_events.race_planned.is_some_and(|t| tick >= t) {
            self.street_events.race_planned = None;
            let _ = self.start_race(None);
        }
        if self.street_events.race.is_some() {
            self.run_race();
        }
        // ── raves ──
        if minute == RAVE_ROLL_MINUTE as u64 && self.street_events.rave_rolled_day != day && self.street_events.rave.is_none() {
            self.street_events.rave_rolled_day = day;
            self.roll_rave();
        }
        if let Some((b, at)) = self.street_events.rave_planned {
            if tick >= at {
                self.street_events.rave_planned = None;
                let _ = self.start_rave(b);
            }
        }
        if self.street_events.rave.is_some() {
            self.run_rave();
        }
        // ── [moto] packs ──
        self.step_moto();
    }

    pub(super) fn news(&mut self, kind: EventKind, headline: String, pos: Vec2) {
        let seq = self.street_events.push_news(self.clock.tick, kind, headline, pos);
        self.events.push(SimEvent::News(seq));
    }

    /// Take a fine from a citizen's household (or their own savings) into
    /// the treasury. Returns what was taken.
    fn fine(&mut self, cid: CitizenId, amount: f32) -> f32 {
        let Some(c) = self.citizens.get_mut(cid) else { return 0.0 };
        match c.household.filter(|h| self.households.contains_key(*h)) {
            Some(h) => self.households[h].money -= amount,
            None => c.savings -= amount,
        }
        self.economy.credit(Flow::Fines, amount);
        self.street_events.fines_total += amount;
        amount
    }

    // ── street races ───────────────────────────────────────────────────────

    /// Locals who would race tonight: young enough, a bit wild, with a fast
    /// car of their own and free now. Flashiest first.
    pub fn race_crew(&self) -> Vec<CitizenId> {
        let busy: Vec<CitizenId> = self.driving.takeovers.active.as_ref().map(|t| t.residents.clone()).unwrap_or_default();
        let mut out: Vec<(CitizenId, f32)> = self
            .citizens
            .iter()
            .filter(|(id, c)| (18..=42).contains(&c.age) && c.owns_car && c.car.is_some() && !busy.contains(id) && !self.services.in_custody(*id) && !self.is_patient(*id))
            .filter(|(_, c)| matches!(c.activity, Activity::AtHome | Activity::Idle))
            .filter_map(|(id, c)| {
                let model = c.car.and_then(|v| self.vehicles.cars.get(v)).map(|o| o.model)?;
                if !fast_car(&model) {
                    return None;
                }
                let (caution, hedonism) = driver_traits(c.seed);
                let r = rebelliousness(caution, hedonism);
                (r >= 0.45).then_some((id, r + if model.tuned { 0.3 } else { 0.0 }))
            })
            .collect();
        out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        out.into_iter().map(|(id, _)| id).collect()
    }

    /// Lots a race can meet or finish at: surface car parks and empty lots
    /// a car can reach, with room for a row or two.
    fn race_lots(&self) -> Vec<(LotId, String)> {
        let mut out: Vec<(LotId, String)> = self
            .lots
            .iter()
            .filter(|(_, l)| l.car_segment.is_some() && !l.landlocked && l.land_use != LandUse::Park && l.area() >= 350.0)
            .filter_map(|(id, l)| match l.building.and_then(|b| self.buildings.get(b)) {
                None => Some((id, "the empty lot".to_string())),
                Some(b) if b.floors == 0 && b.operating() && b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Parking) => Some((id, b.name.clone())),
                _ => None,
            })
            .filter(|(id, _)| !self.under_facility(self.lots[*id].center))
            .map(|(id, name)| {
                let name = if name == "the empty lot" {
                    let near = self.building_near(self.lots[id].center).and_then(|b| self.buildings.get(b)).map(|b| b.name.clone());
                    near.map_or(name, |n| format!("the empty lot by {n}"))
                } else {
                    name
                };
                (id, name)
            })
            .collect();
        out.sort_by_key(|(id, _)| key_of(*id));
        out
    }

    /// The lane a car reaches a spot by, and where along it.
    fn spot_lane(&self, s: &Spot) -> Option<(LaneId, f32)> {
        let seg = self.lots.get(s.lot)?.car_segment?;
        self.traffic.net.nearest_lane_on(seg, s.kerb).map(|(l, at, _)| (l, at))
    }

    /// The race's route from the meet's kerb to the finish's.
    fn race_route(&self, meet: &Spot, finish: &Spot) -> Option<(Vec<LaneId>, f32, f32, f32)> {
        let (a, sa) = self.spot_lane(meet)?;
        let (b, sb) = self.spot_lane(finish)?;
        let (route, s0, s1) = self.traffic.net.route_lanes(a, sa, b, sb)?;
        let lanes = &self.traffic.net.lanes;
        let len: f32 = route.iter().map(|&l| lanes[l as usize].length).sum::<f32>() - s0 - (lanes[*route.last()? as usize].length - s1);
        Some((route, s0, s1, len))
    }

    /// Start a race now: the meet is the lot nearest `near` (or the best
    /// one for tonight's crew). Returns its id.
    pub fn start_race(&mut self, near: Option<Vec2>) -> Result<u32, String> {
        if self.street_events.race.is_some() {
            return Err("a race is already on".into());
        }
        let tick = self.clock.tick;
        let crew: Vec<CitizenId> = self.race_crew().into_iter().take(MAX_RACERS).collect();
        if crew.len() < 2 {
            return Err("fewer than two locals with a fast car are free to race".into());
        }
        let lots = self.race_lots();
        if lots.len() < 2 {
            return Err("no empty lot or surface car park to meet at".into());
        }
        let homes: Vec<Vec2> = crew.iter().filter_map(|&c| self.citizens[c].home.and_then(|h| self.building_entrance(h))).collect();
        let crew_at = if homes.is_empty() { Vec2::ZERO } else { homes.iter().copied().sum::<Vec2>() / homes.len() as f32 };
        let day = self.clock.day();
        // The meet: nearest `near`, else near the crew, on cheap land,
        // with a little of the night's luck.
        let mut meets: Vec<(f32, LotId, String)> = lots
            .iter()
            .map(|(id, name)| {
                let l = &self.lots[*id];
                let score = match near {
                    Some(q) => l.center.distance(q),
                    None => l.center.distance(crew_at) * 0.5 + 400.0 * self.land_value.at(l.center) + 300.0 * unit(mix(key_of(*id), day)),
                };
                (score, *id, name.clone())
            })
            .collect();
        meets.sort_by(|a, b| a.0.total_cmp(&b.0).then(key_of(a.1).cmp(&key_of(b.1))));
        for (_, meet_lot, meet_name) in meets.into_iter().take(6) {
            let meet = Spot::of(meet_lot, &self.lots[meet_lot], meet_name);
            // The finish: another lot a good run away, preferring the
            // middle of the range, ties by the night.
            let mut finishes: Vec<(f32, LotId, String)> = lots
                .iter()
                .filter(|(id, _)| *id != meet_lot)
                .filter_map(|(id, name)| {
                    let d = self.lots[*id].center.distance(meet.centre);
                    (RACE_LENGTH.0..=RACE_LENGTH.1).contains(&d).then(|| ((d - 1200.0).abs() + 200.0 * unit(mix(key_of(*id), day ^ 7)), *id, name.clone()))
                })
                .collect();
            finishes.sort_by(|a, b| a.0.total_cmp(&b.0).then(key_of(a.1).cmp(&key_of(b.1))));
            for (_, fin_lot, fin_name) in finishes.into_iter().take(6) {
                let finish = Spot::of(fin_lot, &self.lots[fin_lot], fin_name);
                let Some((route, _, _, len)) = self.race_route(&meet, &finish) else { continue };
                if len < RACE_MIN_ROUTE {
                    continue;
                }
                return self.launch_race(meet, finish, route, len, crew, tick);
            }
        }
        Err(format!("no pair of the {} empty lots and surface car parks a race could run between ({:.0} m to {:.1} km apart, {RACE_MIN_ROUTE:.0} m by road)", lots.len(), RACE_LENGTH.0, RACE_LENGTH.1 / 1000.0))
    }

    fn launch_race(&mut self, meet: Spot, finish: Spot, route: Vec<LaneId>, route_m: f32, crew: Vec<CitizenId>, tick: u64) -> Result<u32, String> {
        let id = self.street_events.next_id;
        self.street_events.next_id += 1;
        let mut race = Race { id, stage: RaceStage::Gathering, meet, finish, started: tick, go_at: None, ends_at: None, racers: Vec::new(), cars: Vec::new(), route, route_m, finished: Vec::new(), bays: 0, crowd: Vec::new(), incident: None, arrests: 0, fined: 0, fines: 0.0, chase: None };
        for cid in crew {
            if let Some(car) = self.drive_to_spot(cid, &race.meet, id) {
                race.cars.push(car);
                race.racers.push(cid);
                let place = race.meet.name.clone();
                let c = &mut self.citizens[cid];
                c.asleep = false;
                c.remember(tick, format!("Got the message: cars meeting at {place} tonight. Took the good wheels."));
                c.thought = "Race night.".to_string();
            }
        }
        if race.cars.len() < 2 {
            // Nobody could get there: call it off quietly.
            for &car in &race.cars {
                self.release_racer(car, None);
            }
            return Err("nobody could drive to the meet".into());
        }
        let (n, place, at) = (race.cars.len(), race.meet.name.clone(), race.meet.centre);
        self.street_events.race = Some(race);
        self.street_events.races_total += 1;
        self.news(EventKind::Race, format!("Fast cars gathering at {place}: {n} drivers, engines warm"), at);
        Ok(id)
    }

    /// Put a racer in their own car, bound for a spot's kerb.
    fn drive_to_spot(&mut self, cid: CitizenId, s: &Spot, race: u32) -> Option<CarId> {
        let c = self.citizens.get(cid)?;
        let here = c.inside.and_then(|b| self.building_entrance(b)).unwrap_or(c.pos);
        let home = c.home;
        let (vid, _) = self.car_ready(cid, here)?;
        let seg = self.lots.get(s.lot)?.car_segment?;
        let car = self.drive_from_spot(vid, seg, s.kerb, cid, crate::vehicle::Errand::Trip)?;
        self.leave_building(cid);
        let tc = &mut self.traffic.cars[car];
        tc.drive.gang = Some(GangRole { takeover: race, phase: GangPhase::Joining, race: true, pack: false });
        let (pos, model) = (tc.pos, tc.model);
        let c = &mut self.citizens[cid];
        c.pos = pos;
        c.prev_pos = pos;
        c.vehicle = Some(model);
        c.to_car = None;
        c.activity = Activity::Driving { car, to: home, purpose: Purpose::Wander };
        Some(car)
    }

    /// One tick of the race under way.
    fn run_race(&mut self) {
        let Some(mut r) = self.street_events.race.take() else { return };
        let tick = self.clock.tick;
        // Cars gone (towed off a wreck, a rebuild) drop out.
        let mut k = 0;
        while k < r.cars.len() {
            if self.traffic.cars.contains_key(r.cars[k]) {
                k += 1;
            } else {
                r.cars.remove(k);
                r.racers.remove(k);
            }
        }
        r.finished.retain(|c| self.traffic.cars.contains_key(*c));
        match r.stage {
            RaceStage::Gathering => {
                self.pull_in(&mut r, false);
                let in_lot = r.cars.iter().filter(|&&c| self.traffic.cars[c].in_stunt()).count();
                let waited = tick.saturating_sub(r.started);
                if tick % 3 == 0 {
                    self.gather_onlookers(&mut r, false);
                }
                // Ready when everyone is in, or most after a wait, or a
                // pair after a long one; then a few minutes showing off
                // the cars (`ends_at` holds the flag until then).
                let ready = (in_lot >= 3 && (in_lot == r.cars.len() || (waited >= RACE_GATHER && in_lot * 10 >= r.cars.len() * 6))) || (in_lot >= 2 && waited >= RACE_GATHER * 2);
                if ready && r.ends_at.is_none() {
                    r.ends_at = Some(tick + SHOW_OFF);
                }
                if r.ends_at.is_some_and(|t| tick >= t) {
                    r.ends_at = None;
                    self.go(&mut r);
                } else if waited > RACE_GATHER * 3 || r.cars.is_empty() {
                    self.end_race(r, false);
                    return;
                }
            }
            RaceStage::Racing | RaceStage::Party => {
                self.pull_in(&mut r, true);
                if r.stage == RaceStage::Racing {
                    let running = r.cars.iter().filter(|&&c| self.traffic.cars[c].drive.gang.is_some_and(|g| g.phase == GangPhase::Racing) && !self.traffic.cars[c].crashed()).count();
                    let took = tick.saturating_sub(r.go_at.unwrap_or(tick));
                    if (running == 0 && !r.finished.is_empty()) || took >= RACE_MAX || r.cars.is_empty() {
                        r.stage = RaceStage::Party;
                        let party = PARTY_TICKS.0 + mix(r.id as u64, tick) % (PARTY_TICKS.1 - PARTY_TICKS.0);
                        r.ends_at = Some(tick + party);
                        let winner = r.finished.first().and_then(|&c| r.racer_of(c)).and_then(|c| self.citizens.get(c)).map(|c| c.name.clone());
                        let line = match winner {
                            Some(w) => format!("{w} won the street race to {}; the party's on", r.finish.name),
                            None => format!("Nobody finished the street race to {}", r.finish.name),
                        };
                        self.news(EventKind::Race, line, r.finish.centre);
                    }
                } else if tick % 3 == 0 {
                    self.gather_onlookers(&mut r, true);
                }
                // Police at the finish wait for the first car in, then move.
                let police = r.incident.and_then(|i| self.services.incident(i)).is_some_and(|i| i.stage == IncidentStage::OnScene);
                if police && (r.stage == RaceStage::Party || !r.finished.is_empty()) {
                    self.bust_race(&mut r);
                    self.end_race(r, true);
                    return;
                }
                if r.cars.is_empty() || r.ends_at.is_some_and(|e| tick >= e) {
                    self.end_race(r, false);
                    return;
                }
            }
        }
        self.street_events.race = Some(r);
    }

    /// Cars that reached the lot they were heading for park up in a bay.
    fn pull_in(&mut self, r: &mut Race, at_finish: bool) {
        let spot = if at_finish { r.finish.clone() } else { r.meet.clone() };
        let want = if at_finish { GangPhase::Racing } else { GangPhase::Joining };
        let n = r.cars.len().max(4);
        let arriving: Vec<CarId> = r
            .cars
            .iter()
            .copied()
            .filter(|&c| {
                let car = &self.traffic.cars[c];
                let near = car.pos.distance(spot.kerb) < LOT_REACH && (car.v < 0.6 || car.pos.distance(spot.kerb) < 12.0);
                let done = car.idx + 1 >= car.route.len() && car.s >= car.s_end - 1.0;
                car.drive.gang.is_some_and(|g| g.phase == want) && !car.crashed() && !car.in_stunt() && (near || done && car.pos.distance(spot.kerb) < LOT_REACH * 2.0)
            })
            .collect();
        for c in arriving {
            let (at, face) = spot.bay(r.bays, n);
            r.bays += 1;
            self.traffic.begin_stunt(c, StuntKind::Ring { at, face });
            self.traffic.cars[c].v = 0.0;
            if at_finish && !r.finished.contains(&c) {
                r.finished.push(c);
                if let Some(cid) = r.racer_of(c) {
                    let place = r.finished.len();
                    let tick = self.clock.tick;
                    if let Some(p) = self.citizens.get_mut(cid) {
                        p.thought = if place == 1 { "First! Nobody touches me.".to_string() } else { format!("Came in {}. Next time.", ordinal(place)) };
                        p.remember(tick, format!("Raced to {}. Came in {}.", r.finish.name, ordinal(place)));
                        p.mood = (p.mood + if place == 1 { 0.15 } else { 0.05 }).min(1.0);
                    }
                }
            }
        }
    }

    /// The flag drops: everyone in the lot pulls out flat out along the
    /// route; stragglers go home. A neighbour may call the police.
    fn go(&mut self, r: &mut Race) {
        let tick = self.clock.tick;
        // The route again: road works may have changed the lanes.
        let Some((route, s0, s1, len)) = self.race_route(&r.meet, &r.finish) else {
            r.stage = RaceStage::Party;
            r.ends_at = Some(tick);
            return;
        };
        r.route = route.clone();
        r.route_m = len;
        for &car in &r.cars.clone() {
            if !self.traffic.cars[car].in_stunt() {
                self.release_racer(car, None);
                continue;
            }
            self.traffic.end_stunt(car, route.clone(), s0, s1);
            let tc = &mut self.traffic.cars[car];
            if let Some(g) = tc.drive.gang.as_mut() {
                g.phase = GangPhase::Racing;
            }
            let base = if tc.drive.profile.assigned { tc.drive.profile } else { DriverProfile::professional(0.5) };
            tc.drive.profile = base.racing();
            tc.drive.calm = None;
            tc.drive.anger = 0.0;
        }
        // Drop the stragglers from the race's lists.
        let keep: Vec<bool> = r.cars.iter().map(|&c| self.traffic.cars.get(c).is_some_and(|c| c.drive.gang.is_some_and(|g| g.phase == GangPhase::Racing))).collect();
        retain_pairs(r, &keep);
        r.bays = 0;
        r.stage = RaceStage::Racing;
        r.go_at = Some(tick);
        self.news(EventKind::Race, format!("Street race! {} cars tearing from {} to {} ({:.1} km)", r.cars.len(), r.meet.name, r.finish.name, r.route_m / 1000.0), r.meet.centre);
        // Engines at full revs at midnight: someone calls it in (most
        // nights; the odds by the night, not the world's dice).
        if unit(mix(self.config.seed ^ 0x0CA1, r.id as u64)) < 0.8 {
            let caller = self.witness(r.meet.centre, 450.0, &r.racers);
            if let Some(near) = self.building_near(r.finish.centre) {
                if let Some(id) = self.report_crime(CrimeKind::StreetRace, near, r.racers.first().copied(), None) {
                    if let Some(i) = self.services.incident_mut(id) {
                        i.pos = r.finish.kerb;
                        i.on_road = true;
                        i.caller = caller;
                    }
                    r.incident = Some(id);
                }
            }
            if let Some((who, c)) = caller.and_then(|c| self.citizens.get_mut(c).map(|x| (c, x))) {
                c.remember(tick, format!("Called the police: cars racing past {} at midnight.", r.meet.name));
                self.concerns.heard(who, c, crate::concerns::Concern::Noise, 0.4);
            }
        }
    }

    /// Onlookers walk over: the young and the reckless near the lot.
    fn gather_onlookers(&mut self, r: &mut Race, at_finish: bool) {
        if r.crowd.len() >= RACE_CROWD {
            return;
        }
        let spot = if at_finish { r.finish.clone() } else { r.meet.clone() };
        let mut pool: Vec<(CitizenId, f32)> = self
            .citizens
            .iter()
            .filter(|(id, c)| !r.crowd.contains(id) && !r.racers.contains(id) && (16..=40).contains(&c.age) && !c.asleep)
            .filter(|(_, c)| matches!(c.activity, Activity::AtHome | Activity::Idle))
            .filter(|(id, _)| !self.services.in_custody(*id) && !self.is_patient(*id))
            .filter_map(|(id, c)| {
                let d = c.pos.distance(spot.centre);
                let (caution, hedonism) = driver_traits(c.seed);
                let keen = 0.6 * hedonism + 0.4 * (1.0 - caution);
                (d < 500.0 && keen > 0.5).then_some((id, d))
            })
            .collect();
        pool.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        let until = r.ends_at.unwrap_or(self.clock.tick + 120);
        for (cid, _) in pool.into_iter().take(2) {
            let k = r.crowd.len();
            if self.walk_to_stand(cid, spot.onlooker(k), until, "Cars lined up, music up. Gotta see this.") {
                r.crowd.push(cid);
            }
        }
    }

    /// Walk someone out to stand at `spot` until `until`.
    fn walk_to_stand(&mut self, cid: CitizenId, spot: Vec2, until: u64, thought: &str) -> bool {
        let from = match self.citizens[cid].inside.and_then(|b| self.building_entrance(b)) {
            Some(e) => e,
            None => self.citizens[cid].pos,
        };
        let Some((mut path, _)) = self.walk.route(from, spot) else { return false };
        path.push(spot);
        let exit = self.leave_building(cid);
        let c = &mut self.citizens[cid];
        if let Some(p) = exit {
            c.pos = p;
            c.prev_pos = p;
        }
        c.asleep = false;
        c.mode = crate::citizens::TravelMode::Walk;
        c.transit = None;
        c.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path, idx: 0 };
        c.next_decision_tick = until;
        c.thought = thought.to_string();
        true
    }

    /// The police are on scene: racers still at the party may be arrested
    /// (car impounded), the rest are fined; one may run for it.
    fn bust_race(&mut self, r: &mut Race) {
        let tick = self.clock.tick;
        let Some(inc) = r.incident else { return };
        let mut rng = crate::rng_for(self.config.seed ^ 0x5ACE_B057, tick);
        let place = r.finish.name.clone();
        let mut runner: Option<(CitizenId, CarId)> = None;
        for k in 0..r.cars.len() {
            let (car, cid) = (r.cars[k], r.racers[k]);
            let Some(tc) = self.traffic.cars.get(car) else { continue };
            let here = tc.in_stunt() || tc.pos.distance(r.finish.kerb) < 120.0;
            if !here || !matches!(self.citizens.get(cid).map(|c| &c.activity), Some(Activity::Driving { car: x, .. }) if *x == car) {
                continue;
            }
            if r.arrests < RACE_MAX_ARRESTS && rng.random::<f32>() < RACE_ARREST_CHANCE {
                // Arrested; the car is impounded (towed off like a wreck).
                self.traffic.cars[car].citizen = CitizenId::default();
                if let Some(vid) = self.traffic.cars[car].owned.take() {
                    self.drop_car(vid, crate::vehicle::CarEvent::Scrapped);
                }
                self.tow_or_queue(car);
                r.fines += self.fine(cid, RACE_FINE);
                let c = &mut self.citizens[cid];
                c.activity = Activity::Idle;
                c.inside = None;
                c.remember(tick, format!("Busted at the street race at {place}. Fined and they impounded my car."));
                c.thought = "Busted.".to_string();
                c.mood = (c.mood - 0.25).max(0.0);
                self.services.stats.arrests_today += 1;
                self.take_into_custody(inc, cid);
                r.arrests += 1;
            } else if runner.is_none() && tc.in_stunt() && rng.random::<f32>() < 0.5 {
                runner = Some((cid, car));
            } else {
                r.fines += self.fine(cid, RACE_FINE);
                r.fined += 1;
                if let Some(c) = self.citizens.get_mut(cid) {
                    c.remember(tick, format!("The police broke up the street race at {place}. {RACE_FINE:.0} fine."));
                    c.mood = (c.mood - 0.1).max(0.0);
                }
            }
        }
        // The impounded cars are the tow's now.
        let keep: Vec<bool> = r.cars.iter().map(|c| self.traffic.cars.get(*c).is_some_and(|x| x.citizen != CitizenId::default())).collect();
        retain_pairs(r, &keep);
        // One floors it: the chase system takes it from here.
        if let Some((cid, car)) = runner {
            if self.release_racer(car, Some(cid)) {
                if let Some(i) = self.services.incident_mut(inc) {
                    i.suspect = Some(cid);
                }
                if self.try_chase(inc) {
                    r.chase = Some(inc);
                    let keep: Vec<bool> = r.cars.iter().map(|&c| c != car).collect();
                    retain_pairs(r, &keep);
                    self.news(EventKind::Race, format!("A racer ran from the police at {place}: chase under way"), r.finish.centre);
                    return;
                }
            }
            r.fines += self.fine(cid, RACE_FINE);
            r.fined += 1;
        }
        self.resolve(inc, if r.arrests > 0 || r.fined > 0 { Outcome::Arrest } else { Outcome::Escaped });
    }

    /// Back to normal driving, bound for home: out of the lot onto the
    /// lanes, or turned round on the road. With `keep_role` the gang role
    /// stays off (a chase wants the car as an ordinary one). True when the
    /// car is on its way.
    fn release_racer(&mut self, car: CarId, _for_chase: Option<CitizenId>) -> bool {
        let Some(c) = self.traffic.cars.get(car) else { return false };
        let driver = c.citizen;
        let home = self.citizens.get(driver).and_then(|p| p.home);
        let dest = home.and_then(|h| self.building_lot(h)).and_then(|l| l.car_segment.map(|s| (s, l.car_point)));
        let goal = dest.and_then(|(seg, p)| self.traffic.net.nearest_lane_on(seg, p));
        let mut ok = false;
        if c.in_stunt() {
            // Out the way it came in: the lot's street.
            let pos = c.pos;
            let lot_seg = self.lots.values().filter(|l| l.car_segment.is_some() && l.center.distance(pos) < 60.0).min_by(|a, b| a.center.distance(pos).total_cmp(&b.center.distance(pos))).and_then(|l| l.car_segment.map(|s| (s, l.car_point)));
            let start = lot_seg.and_then(|(s, p)| self.traffic.net.nearest_lane_on(s, p)).or_else(|| self.roads.nearest_street(pos).and_then(|(s, _, q)| self.traffic.net.nearest_lane_on(s, q)));
            if let (Some((l0, s0, _)), Some((g, gs, _))) = (start, goal) {
                if let Some((route, s0, s1)) = self.traffic.net.route_lanes(l0, s0, g, gs) {
                    self.traffic.end_stunt(car, route, s0, s1);
                    ok = true;
                }
            }
            if !ok {
                // Boxed in with nowhere to drive: towed off like a wreck.
                self.traffic.cars[car].citizen = CitizenId::default();
                if let Some(vid) = self.traffic.cars[car].owned.take() {
                    self.drop_car(vid, crate::vehicle::CarEvent::Scrapped);
                }
                self.tow_or_queue(car);
                if let Some(p) = self.citizens.get_mut(driver) {
                    p.activity = Activity::Idle;
                    p.next_decision_tick = self.clock.tick + 1;
                }
                return false;
            }
        } else if let Some((g, gs, _)) = goal {
            let (here, s) = (c.route[c.idx], c.s);
            if let Some((route, _, s1)) = self.traffic.net.route_lanes(here, s, g, gs) {
                self.traffic.reroute(car, route, s, s1);
            }
            ok = true;
        }
        let tc = &mut self.traffic.cars[car];
        tc.drive.gang = None;
        tc.drive.profile.assigned = false;
        tc.drive.calm = None;
        if let Some(p) = self.citizens.get_mut(driver) {
            if let Activity::Driving { purpose, to, .. } = &mut p.activity {
                *purpose = Purpose::Home;
                *to = home;
            }
        }
        ok
    }

    /// Everyone goes home; the record and the news.
    fn end_race(&mut self, r: Race, police: bool) {
        let tick = self.clock.tick;
        for &car in &r.cars {
            if self.traffic.cars.get(car).is_some_and(|c| !c.drive.fleeing) {
                self.release_racer(car, None);
            }
        }
        for &cid in &r.racers {
            if let Some(p) = self.citizens.get_mut(cid) {
                if !police && r.go_at.is_some() {
                    p.remember(tick, format!("Raced {} to {} and partied after. What a night.", r.meet.name, r.finish.name));
                    p.mood = (p.mood + 0.08).min(1.0);
                }
            }
        }
        let crowd_n = r.crowd.iter().filter(|&&c| self.citizens.get(c).is_some_and(|c| c.pos.distance(r.finish.centre) < 60.0 || c.pos.distance(r.meet.centre) < 60.0)).count() as u32;
        for &cid in &r.crowd {
            if let Some(c) = self.citizens.get_mut(cid) {
                if matches!(c.activity, Activity::Idle | Activity::Traveling { .. }) {
                    c.next_decision_tick = tick + 1 + mix(key_of(cid), tick) % 12;
                }
                if c.pos.distance(r.finish.centre) < 60.0 || c.pos.distance(r.meet.centre) < 60.0 {
                    c.remember(tick, format!("Watched the street racers at {}. Loud, fast, stupid, brilliant.", if r.go_at.is_some() { &r.finish.name } else { &r.meet.name }));
                    c.mood = (c.mood + 0.04).min(1.0);
                }
            }
        }
        if let Some(i) = r.incident {
            if !police && self.services.incident(i).is_some_and(|i| i.active() && i.stage != IncidentStage::OnScene && i.stage != IncidentStage::Pursuit) {
                self.resolve(i, Outcome::Escaped);
            }
        }
        let winner = r.finished.first().and_then(|&c| r.racer_of(c)).and_then(|c| self.citizens.get(c)).map(|c| c.name.clone());
        if police {
            self.street_events.busts_total += 1;
            let chase = if r.chase.is_some() { ", one fled" } else { "" };
            self.news(EventKind::Race, format!("Police busted the street race at {}: {} arrested, {} fined{chase}", r.finish.name, r.arrests, r.fined), r.finish.centre);
        } else if r.go_at.is_none() {
            self.news(EventKind::Race, format!("The street race at {} fizzled out", r.meet.name), r.meet.centre);
        }
        let people = r.racers.len() as u32 + r.arrests + u32::from(r.chase.is_some());
        self.street_events.record(EventRecord { id: r.id, kind: EventKind::Race, place: r.meet.name.clone(), pos: r.meet.centre, finish: Some(r.finish.name.clone()), started: r.started, ended: tick, people, crowd: crowd_n, winner, police, arrests: r.arrests, fines: r.fines, complaints: u32::from(r.incident.is_some()), chase: r.chase.is_some(), culture: 0.0 });
    }

    /// The race under way, if any.
    pub fn race(&self) -> Option<&Race> {
        self.street_events.race.as_ref()
    }

    // ── warehouse raves ───────────────────────────────────────────────────

    /// The old warehouses (cheap industrial buildings standing) and the
    /// culture round each, strongest first.
    pub fn rave_spots(&self) -> Vec<RaveSpot> {
        let warehouses: Vec<(BuildingId, String, Vec2)> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.land_use == LandUse::Industrial && b.tier == Tier::Low && b.operating() && b.port.is_none() && !b.is_power() && b.service.is_none() && b.rail.is_none())
            .filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (id, b.name.clone(), l.center)))
            .collect();
        if warehouses.is_empty() {
            return Vec::new();
        }
        // Who lives where: night owls and artists, by home.
        let people: Vec<(Vec2, bool, bool)> = self
            .citizens
            .values()
            .filter_map(|c| {
                let (owl, artist) = (night_owl(c), super::pressure::is_artist(c));
                if !owl && !artist {
                    return None;
                }
                let at = c.home.and_then(|h| self.building_lot(h)).map(|l| l.center)?;
                Some((at, owl, artist))
            })
            .collect();
        let homes: Vec<Vec2> = self.buildings.values().filter(|b| b.homes > 0 && !b.residents.is_empty()).filter_map(|b| self.lots.get(b.lot).map(|l| l.center)).collect();
        let art_scene = self.pressure.signals.art_scene();
        let mut out: Vec<RaveSpot> = warehouses
            .into_iter()
            .map(|(b, name, pos)| {
                let mut pull = RavePull { art_scene, ..Default::default() };
                for &(at, owl, artist) in &people {
                    if at.distance(pos) <= RAVE_REACH {
                        pull.youth += u32::from(owl);
                        pull.artists += u32::from(artist);
                    }
                }
                pull.neighbours = homes.iter().filter(|h| h.distance(pos) <= NOISE_REACH).count() as u32;
                RaveSpot { building: b, name, pos, pull, last_rave_day: self.street_events.last_rave_day(b) }
            })
            .collect();
        out.sort_by(|a, b| b.pull.culture().total_cmp(&a.pull.culture()).then(key_of(a.building).cmp(&key_of(b.building))));
        out
    }

    /// 22:00: weigh the warehouses; the best with a scene round it and no
    /// rave lately may open its doors at 23:00.
    fn roll_rave(&mut self) {
        let day = self.clock.day();
        let tick = self.clock.tick;
        let spots = self.rave_spots();
        let pick = spots.iter().find(|s| s.pull.has_scene() && s.last_rave_day.is_none_or(|d| day >= d + RAVE_REST_DAYS)).cloned();
        self.street_events.rave_spots = spots.into_iter().take(12).collect();
        let Some(spot) = pick else { return };
        if day < SCENE_FROM_DAY {
            return;
        }
        // Not every night somewhere: a gap after the last one in town.
        if self.street_events.rave_days.iter().any(|&(_, d)| day < d + RAVE_GAP_DAYS) {
            return;
        }
        if unit(mix(self.config.seed ^ 0x8A7E, day)) < spot.pull.night_chance() {
            let doors = tick + (RAVE_DOORS_MINUTE - RAVE_ROLL_MINUTE) as u64;
            self.street_events.rave_planned = Some((spot.building, doors));
        }
    }

    /// Open a rave at warehouse `b` now. Returns its id.
    pub fn start_rave(&mut self, b: BuildingId) -> Result<u32, String> {
        if self.street_events.rave.is_some() {
            return Err("a rave is already on".into());
        }
        let tick = self.clock.tick;
        let Some(bld) = self.buildings.get(b) else { return Err("no such building".into()) };
        let Some(lot) = self.lots.get(bld.lot) else { return Err("the building has no lot".into()) };
        let place = bld.name.clone();
        let centre = lot.center;
        let front = if lot.front_dir.length_squared() > 0.0 { lot.front_dir.normalize() } else { Vec2::Y };
        let door = lot.entrance() + front * 1.2;
        let along = Vec2::new(-front.y, front.x);
        let pull = self.rave_spots().into_iter().find(|s| s.building == b).map(|s| s.pull).unwrap_or_default();
        let id = self.street_events.next_id;
        self.street_events.next_id += 1;
        let length = RAVE_LENGTH.0 + mix(key_of(b), tick) % (RAVE_LENGTH.1 - RAVE_LENGTH.0);
        let mut rave = Rave { id, building: b, place: place.clone(), centre, door, queue_dir: along, started: tick, ends_at: tick + length, promoter: None, guests: Vec::new(), queue: Vec::new(), inside: Vec::new(), peak: 0, complaints: 0, incident: None, arrests: 0, fines: 0.0, takings: 0.0, pull };
        self.invite_ravers(&mut rave, RAVE_GUESTS * 2 / 3);
        rave.promoter = rave.guests.first().copied();
        if rave.guests.is_empty() {
            return Err("nobody in reach wants to come".into());
        }
        if let Some((p, c)) = rave.promoter.and_then(|p| self.citizens.get_mut(p).map(|c| (p, c))) {
            let _ = p;
            c.remember(tick, format!("Opened up {place} for a night. Sound system, lights, the lot."));
        }
        let n = rave.guests.len();
        let headline = if pull.artists as f32 / ARTISTS_PER >= pull.youth as f32 / YOUTH_PER { format!("Rave at {place}: the art crowd's warehouse party, {n} on the way") } else { format!("Rave at {place}: lights up, {n} heading for the door") };
        self.street_events.rave = Some(rave);
        self.street_events.raves_total += 1;
        self.street_events.rave_days.retain(|x| x.0 != b);
        self.street_events.rave_days.push((b, self.clock.day()));
        self.news(EventKind::Rave, headline, centre);
        Ok(id)
    }

    /// Ask more people to come: night owls and artists in reach who are
    /// free now, nearest first, up to `n` more.
    fn invite_ravers(&mut self, rave: &mut Rave, n: usize) {
        let room = RAVE_GUESTS.saturating_sub(rave.guests.len()).min(n);
        if room == 0 {
            return;
        }
        let hour = self.clock.hour() as u8;
        let mut pool: Vec<(CitizenId, f32)> = self
            .citizens
            .iter()
            .filter(|(id, c)| !rave.guests.contains(id) && (night_owl(c) || (super::pressure::is_artist(c) && c.age <= 50)))
            .filter(|(_, c)| matches!(c.activity, Activity::AtHome | Activity::Idle))
            .filter(|(_, c)| c.work.and_then(|w| self.buildings.get(w)).is_none_or(|b| !b.shift_for(c.seed).contains(hour)))
            .filter(|(id, _)| !self.services.in_custody(*id) && !self.is_patient(*id))
            .filter_map(|(id, c)| {
                let d = c.pos.distance(rave.centre);
                (d < RAVE_GUEST_REACH).then_some((id, d))
            })
            .collect();
        pool.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        let tick = self.clock.tick;
        let mut added = 0;
        for (cid, _) in pool {
            if added >= room {
                break;
            }
            // Not everyone wants a warehouse at midnight: about two in three.
            if mix(key_of(cid), rave.id as u64) % 3 == 0 {
                continue;
            }
            let k = rave.queue.len();
            if self.walk_to_stand(cid, rave.queue_spot(k), rave.ends_at, "Heard there's a rave tonight. Going.") {
                rave.guests.push(cid);
                rave.queue.push(cid);
                let place = rave.place.clone();
                self.citizens[cid].remember(tick, format!("Word went round about a rave at {place}."));
                added += 1;
            }
        }
    }

    /// One tick of the rave: the queue shuffles up, the door lets people
    /// in, the neighbours hear it, the police may come.
    fn run_rave(&mut self) {
        let Some(mut r) = self.street_events.rave.take() else { return };
        let tick = self.clock.tick;
        if !self.buildings.contains_key(r.building) {
            self.end_rave(r, false);
            return;
        }
        // Who gave up and went (plans changed, picked up, gone).
        r.queue.retain(|&c| self.citizens.get(c).is_some_and(|c| matches!(c.activity, Activity::Traveling { to: None, .. } | Activity::Idle)));
        r.inside.retain(|&c| self.citizens.get(c).is_some_and(|c| c.inside == Some(r.building)));
        // More word goes round until 01:00.
        if tick % 20 == 0 && tick < r.started + 120 {
            self.invite_ravers(&mut r, 8);
        }
        // The queue is in the order people got there: whoever has arrived
        // (standing within sight of the door) is ahead of anyone still
        // walking over, so one slow walker never holds the door.
        let arrived = |w: &World, c: CitizenId| w.citizens.get(c).is_some_and(|c| matches!(c.activity, Activity::Idle) || (matches!(c.activity, Activity::Traveling { .. }) && c.pos.distance(r.door) < 45.0));
        let (mut here, away): (Vec<CitizenId>, Vec<CitizenId>) = r.queue.iter().partition(|&&c| arrived(self, c));
        here.extend(away);
        r.queue = here;
        // The door: the head of the queue, standing there, goes in.
        if tick % 2 == 0 {
            let mut let_in = 0;
            while let_in < DOOR_RATE && !r.queue.is_empty() {
                let cid = r.queue[0];
                let at_door = self.citizens.get(cid).is_some_and(|c| matches!(c.activity, Activity::Idle) && c.pos.distance(r.door) < 4.0);
                if !at_door {
                    break;
                }
                r.queue.remove(0);
                let b = r.building;
                let place = r.place.clone();
                // On the door: the promoter's cut, and the hire of the
                // place to whoever runs the warehouse.
                let fee = RAVE_DOOR_FEE.min(self.citizens[cid].savings.max(0.0));
                self.citizens[cid].savings -= fee;
                if let Some(z) = self.buildings.get_mut(b).and_then(|x| x.business.as_mut()) {
                    z.money += fee * 0.5;
                    z.revenue_today += fee * 0.5;
                }
                r.takings += fee;
                let c = &mut self.citizens[cid];
                c.activity = Activity::Visiting(b);
                c.inside = Some(b);
                c.next_decision_tick = r.ends_at;
                c.thought = "Bass in the chest. Perfect.".to_string();
                c.remember(tick, format!("Danced at a warehouse rave at {place}."));
                c.mood = (c.mood + 0.08).min(1.0);
                if let Some(bld) = self.buildings.get_mut(b) {
                    bld.visitors += 1;
                }
                r.inside.push(cid);
                let_in += 1;
            }
        }
        r.peak = r.peak.max(r.inside.len() as u32);
        // The queue shuffles up: anyone standing away from their place
        // walks to it.
        for k in 0..r.queue.len() {
            let cid = r.queue[k];
            let spot = r.queue_spot(k);
            if let Some(c) = self.citizens.get_mut(cid) {
                if matches!(c.activity, Activity::Idle) && c.pos.distance(spot) > 0.6 {
                    c.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path: vec![c.pos, spot], idx: 0 };
                    c.next_decision_tick = r.ends_at;
                } else if matches!(c.activity, Activity::Idle) {
                    // Facing the door.
                    let d = r.door - c.pos;
                    c.heading = d.y.atan2(d.x);
                    c.prev_heading = c.heading;
                    c.next_decision_tick = c.next_decision_tick.max(r.ends_at);
                }
            }
        }
        // The neighbours hear it every half hour, and one may call.
        if tick >= r.started + NOISE_GRACE && (tick - r.started) % 30 == 15 {
            self.rave_noise(&mut r);
        }
        let police = r.incident.and_then(|i| self.services.incident(i)).is_some_and(|i| i.stage == IncidentStage::OnScene);
        if police {
            self.shut_down_rave(&mut r);
            self.end_rave(r, true);
            return;
        }
        if tick >= r.ends_at {
            self.end_rave(r, false);
            return;
        }
        self.street_events.rave = Some(r);
    }

    /// Bass through the walls: neighbours at home near the warehouse lose
    /// sleep and mood, and (by the night's odds, more the more of them are
    /// woken) one calls the police.
    fn rave_noise(&mut self, r: &mut Rave) {
        let tick = self.clock.tick;
        let homes: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| !b.residents.is_empty() && self.lots.get(b.lot).is_some_and(|l| l.center.distance(r.centre) < NOISE_REACH)).map(|(id, _)| id).collect();
        let mut woken: Vec<CitizenId> = Vec::new();
        for b in homes {
            for &cid in &self.buildings[b].residents.clone() {
                if woken.len() >= 40 || r.guests.contains(&cid) {
                    continue;
                }
                if let Some(c) = self.citizens.get_mut(cid) {
                    if c.inside == Some(b) {
                        c.mood = (c.mood - 0.02).max(0.0);
                        if c.asleep && mix(key_of(cid), tick) % 2 == 0 {
                            c.asleep = false;
                        }
                        self.concerns.heard(cid, c, crate::concerns::Concern::Noise, 0.35);
                        if (tick - r.started) < NOISE_GRACE + 30 {
                            c.remember(tick, format!("Bass from {} rattling the windows all night.", r.place));
                        }
                        woken.push(cid);
                    }
                }
            }
        }
        r.complaints += (woken.len() as u32).min(12) / 3;
        if r.incident.is_some() || woken.is_empty() {
            return;
        }
        let odds = (0.15 + 0.04 * woken.len() as f32).min(0.85);
        if unit(mix(key_of(r.building) ^ 0x0015E, tick)) >= odds {
            return;
        }
        let caller = woken[(mix(r.id as u64, tick) % woken.len() as u64) as usize];
        if let Some(id) = self.report_crime(CrimeKind::Rave, r.building, r.promoter, None) {
            if let Some(i) = self.services.incident_mut(id) {
                i.pos = r.door;
                i.caller = Some(caller);
            }
            r.incident = Some(id);
            r.complaints += 1;
            if let Some(c) = self.citizens.get_mut(caller) {
                c.remember(tick, format!("Called the police about the rave at {}. Some of us work in the morning.", r.place));
            }
            self.news(EventKind::Rave, format!("Noise complaint: a neighbour called the police on the rave at {}", r.place), r.centre);
        }
    }

    /// The police are at the door: the music stops, the promoter is fined
    /// (and now and then taken in).
    fn shut_down_rave(&mut self, r: &mut Rave) {
        let tick = self.clock.tick;
        let Some(inc) = r.incident else { return };
        if let Some(p) = r.promoter.filter(|p| self.citizens.contains_key(*p)) {
            r.fines += self.fine(p, RAVE_FINE);
            if unit(mix(key_of(p), tick)) < 0.25 {
                if self.citizens[p].inside.is_some() {
                    self.leave_building(p);
                }
                let c = &mut self.citizens[p];
                c.activity = Activity::Idle;
                c.remember(tick, format!("Taken in over the rave at {}. Fined {RAVE_FINE:.0}.", r.place));
                self.services.stats.arrests_today += 1;
                self.take_into_custody(inc, p);
                r.arrests += 1;
            } else {
                self.citizens[p].remember(tick, format!("The police shut the rave at {} down. Fined {RAVE_FINE:.0}.", r.place));
            }
        }
        self.resolve(inc, if r.arrests > 0 { Outcome::Arrest } else { Outcome::Escaped });
    }

    /// Lights up, everyone out; the record and the news.
    fn end_rave(&mut self, r: Rave, police: bool) {
        let tick = self.clock.tick;
        for &cid in r.inside.iter().chain(r.queue.iter()) {
            let inside = self.citizens.get(cid).is_some_and(|c| c.inside == Some(r.building));
            if inside {
                let exit = self.leave_building(cid);
                if let Some(c) = self.citizens.get_mut(cid) {
                    if let Some(p) = exit {
                        c.pos = p;
                        c.prev_pos = p;
                    }
                }
            }
            if let Some(c) = self.citizens.get_mut(cid) {
                if matches!(c.activity, Activity::Idle | Activity::Visiting(_) | Activity::Traveling { to: None, .. }) {
                    c.activity = Activity::Idle;
                    c.next_decision_tick = tick + 1 + mix(key_of(cid), tick) % 10;
                }
                if police {
                    c.thought = "Cops killed the music.".to_string();
                }
            }
        }
        if let Some(i) = r.incident {
            if !police && self.services.incident(i).is_some_and(|i| i.active() && i.stage != IncidentStage::OnScene) {
                self.resolve(i, Outcome::Escaped);
            }
        }
        let people = r.guests.len() as u32;
        if police {
            self.street_events.busts_total += 1;
            self.news(EventKind::Rave, format!("Police shut down the rave at {}; promoter fined {:.0}", r.place, r.fines), r.centre);
        } else {
            self.news(EventKind::Rave, format!("The rave at {} wound down at dawn ({} danced)", r.place, r.peak.max(r.inside.len() as u32)), r.centre);
        }
        self.street_events.record(EventRecord { id: r.id, kind: EventKind::Rave, place: r.place.clone(), pos: r.centre, finish: None, started: r.started, ended: tick, people, crowd: r.peak, winner: None, police, arrests: r.arrests, fines: r.fines, complaints: r.complaints, chase: false, culture: r.pull.culture() });
    }

    /// The rave under way, if any.
    pub fn rave(&self) -> Option<&Rave> {
        self.street_events.rave.as_ref()
    }
}

/// Keep the racers and their cars where `keep` says, in step.
fn retain_pairs(r: &mut Race, keep: &[bool]) {
    for k in (0..r.cars.len().min(keep.len())).rev() {
        if !keep[k] {
            r.cars.remove(k);
            r.racers.remove(k);
        }
    }
}

fn ordinal(n: usize) -> String {
    let suffix = match (n % 10, n % 100) {
        (1, 11) | (2, 12) | (3, 13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}
