//! [parking] Parking enforcement, towing and the pound (types and rules in
//! `crate::enforcement`).
//!
//! Officers from each parking enforcement office drive their rounds in
//! small cars through the streets within the office's reach, from 07:00 to
//! 21:00, and look at every car parked at a kerb they pass. A car over its
//! limit is ticketed; still there after the grace, or parked where parking
//! is banned, it is towed: the nearest tow company in town sends a truck
//! (one from out of town comes from beyond the map edge when there is
//! none, later and dearer), which hooks the car where it stands and carries
//! it to its pound. The owner pays the fee and storage and fetches the car
//! (walking or riding there and driving it home; a pound out of town
//! delivers it), or it is auctioned after three days.

use super::*;
use crate::citizens::ToCar;
use crate::enforcement::*;
use crate::services::ServiceKind;
use crate::vehicle::{BodyType, CarEvent, CarPlace, Errand, Livery, ParkKind, ParkSpot, VehicleClass, VehicleId, VehicleModel};
use rand::seq::IndexedRandom;

/// [services] How near the tow company's driveway a truck must stand to be
/// at the pound (metres).
const POUND_REACH: f32 = 25.0;

/// What a tow truck looks like (as the driving agent's wreck trucks do): a
/// wrecker with a boom and a wheel lift.
pub(super) fn tow_model() -> VehicleModel {
    VehicleModel { class: VehicleClass::Mid, body: BodyType::TowTruck, wear: 1, tuned: false, livery: Livery::Utility, paint: 3 }
}

/// [tow] The tow truck sent for a vehicle with this body: a heavy wrecker
/// for a lorry, a bus or an RV, else the ordinary one.
pub(super) fn tow_model_for(towed: BodyType) -> VehicleModel {
    let mut m = tow_model();
    if towed.needs_heavy_tow() {
        m.body = BodyType::HeavyTow;
    }
    m
}

/// A parking officer's small car.
fn patrol_model() -> VehicleModel {
    VehicleModel { class: VehicleClass::Mid, body: BodyType::Hatchback, wear: 0, tuned: false, livery: Livery::Utility, paint: 6 }
}

impl World {
    /// Every tick, after the cars have moved.
    pub(super) fn step_enforcement(&mut self) {
        let tick = self.clock.tick;
        if self.clock.minute_of_day() == 0 {
            let s = &mut self.enforcement.stats;
            s.tickets_today = 0;
            s.fines_today = 0.0;
            s.tows_today = 0;
            s.tows_out_of_town_today = 0;
            s.released_today = 0;
            s.release_fees_today = 0.0;
            s.auctioned_today = 0;
            s.city_lot_fees_today = 0.0;
            s.boots_today = 0;
            s.clamps_today = 0;
            s.clamps_paid_today = 0;
            s.clamp_fines_today = 0.0;
        }
        if tick % 10 == 3 {
            self.staff_patrols();
        }
        if tick % 2 == 0 {
            self.patrols_look();
        }
        self.step_tow_jobs();
        // [tow-moves] Trucks against a one-way or turning round.
        self.step_tow_moves();
        if tick % 60 == 47 {
            self.hourly_pound();
        }
    }

    // ── offices and patrols ────────────────────────────────────────────────

    /// Working parking enforcement offices.
    pub fn enforcement_offices(&self) -> Vec<BuildingId> {
        self.buildings.iter().filter(|(_, b)| b.service.is_some_and(|s| s.kind == ServiceKind::ParkingEnforcement) && b.operating() && !b.is_derelict()).map(|(id, _)| id).collect()
    }

    /// Patrol cars an office puts out now: by tier, two officers a car,
    /// during patrol hours.
    pub fn enforcement_cars(&self, office: BuildingId) -> usize {
        let Some(b) = self.buildings.get(office) else { return 0 };
        let Some(s) = b.service else { return 0 };
        if !PATROL_HOURS.contains(&(self.clock.hour() as u8)) || b.is_derelict() || !b.operating() {
            return 0;
        }
        [1usize, 2, 4][s.tier.index()].min(b.workers.len() / 2)
    }

    fn office_reach(&self, office: BuildingId) -> (Vec2, f32) {
        let b = &self.buildings[office];
        let r = b.service.map_or(0.0, |s| s.kind.radius(s.tier));
        (self.lots.get(b.lot).map_or(Vec2::ZERO, |l| l.center), r)
    }

    /// A street within the office's reach to drive along next.
    fn patrol_target(&mut self, office: BuildingId) -> Option<(SegmentId, Vec2)> {
        let (c, r) = self.office_reach(office);
        let mut segs: Vec<SegmentId> = self.roads.segments_in_box(c - Vec2::splat(r), c + Vec2::splat(r)).filter(|&s| self.slot_count(s) > 0).collect();
        segs.sort_by_key(|s| s.0);
        let sid = *segs.choose(&mut self.rng)?;
        let (a, b) = self.roads.segment_points(sid);
        Some((sid, a.lerp(b, 0.5)))
    }

    /// Put officers on the road (one car per call) and call them in when
    /// the shift ends.
    fn staff_patrols(&mut self) {
        let traffic = &self.traffic;
        self.enforcement.patrols.retain(|p| traffic.cars.get(p.car).is_some_and(|c| c.errand == Errand::Patrol(p.office)));
        for office in self.enforcement_offices() {
            let want = self.enforcement_cars(office);
            let mine: Vec<CarId> = self.enforcement.patrols.iter().filter(|p| p.office == office).map(|p| p.car).collect();
            if mine.len() < want {
                let Some((fs, fp)) = self.truck_stop(office) else { continue };
                let Some((ts, tp)) = self.patrol_target(office) else { continue };
                let Some((route, s0, s1)) = self.traffic.net.find_route(fs, fp, ts, tp) else { continue };
                let car = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, 6);
                let c = &mut self.traffic.cars[car];
                c.model = patrol_model();
                c.errand = Errand::Patrol(office);
                self.enforcement.patrols.push(Patrol { car, office });
            } else if mine.len() > want {
                for &car in &mine[want..] {
                    self.send_patrol_home(car, office);
                }
            }
        }
        // [services] An office pulled down: its officers drive off the map
        // rather than vanish mid-street.
        let gone: Vec<CarId> = self.enforcement.patrols.iter().filter(|p| !self.buildings.contains_key(p.office)).map(|p| p.car).collect();
        for car in gone {
            self.retire_vehicle(car);
        }
        let traffic = &self.traffic;
        self.enforcement.patrols.retain(|p| traffic.cars.get(p.car).is_some_and(|c| c.errand == Errand::Patrol(p.office)));
    }

    fn send_patrol_home(&mut self, car: CarId, office: BuildingId) {
        let home = self.truck_stop(office).and_then(|(s, p)| self.traffic.net.nearest_lane_on(s, p));
        match home {
            Some((goal, s_end, _)) if self.traffic.redirect(car, goal, s_end) => self.traffic.cars[car].errand = Errand::Return(office),
            // [services] No road home: off the map, not gone on the spot.
            _ => self.retire_vehicle(car),
        }
    }

    /// An officer reached the end of a street: on to the next, or home.
    pub(super) fn patrol_arrived(&mut self, car: CarId, office: BuildingId) {
        let on_shift = self.enforcement.patrols.iter().filter(|p| p.office == office).position(|p| p.car == car).is_some_and(|k| k < self.enforcement_cars(office));
        if on_shift {
            for _ in 0..3 {
                let Some((sid, mid)) = self.patrol_target(office) else { break };
                if let Some((goal, s_end, _)) = self.traffic.net.nearest_lane_on(sid, mid) {
                    if self.traffic.redirect(car, goal, s_end) {
                        return;
                    }
                }
            }
        }
        self.send_patrol_home(car, office);
        self.enforcement.patrols.retain(|p| p.car != car);
    }

    /// Officers look at the cars parked along the street they pass.
    fn patrols_look(&mut self) {
        if self.enforcement.patrols.is_empty() {
            return;
        }
        let eyes: Vec<Vec2> = self.enforcement.patrols.iter().filter_map(|p| self.traffic.cars.get(p.car).map(|c| c.pos)).collect();
        let seen: Vec<VehicleId> = self
            .vehicles
            .cars
            .iter()
            .filter(|(_, c)| c.spot().is_some_and(|s| s.kind.on_street() && eyes.iter().any(|e| e.distance(s.pos) < SPOT_RANGE)))
            .map(|(id, _)| id)
            .collect();
        for vid in seen {
            self.check_parked(vid);
        }
    }

    // ── the rules ──────────────────────────────────────────────────────────

    /// Minutes a car may stand where it is; `None`: no limit (a resident's
    /// permit near home, or a street whose rule says so).
    pub fn kerb_limit(&self, vid: VehicleId) -> Option<u64> {
        let car = self.vehicles.cars.get(vid)?;
        let spot = car.spot()?;
        let home = self.citizens.get(car.owner).and_then(|c| c.home).and_then(|h| self.building_entrance(h));
        if home.is_some_and(|h| h.distance(spot.pos) <= PERMIT_RADIUS) {
            return None;
        }
        match spot.segment.and_then(|s| self.vehicles.kerb_rules.get(&s)) {
            Some(0) => None,
            Some(&m) => Some(m as u64),
            None => Some(KERB_LIMIT),
        }
    }

    /// Parked where parking is banned (a shop front, a clearway, a ring).
    pub fn parked_illegally(&self, spot: &ParkSpot) -> bool {
        spot.kind == ParkKind::Overflow || (spot.kind == ParkKind::Kerb && !spot.segment.is_some_and(|s| self.kerb_allowed(s)))
    }

    /// An officer checks a parked car: a ticket when it is over its limit
    /// or parked illegally; [clamp] the clamp once the grace has run out
    /// (towed if the fine goes unpaid), or the tow truck at once for an
    /// illegal one.
    pub(super) fn check_parked(&mut self, vid: VehicleId) {
        let tick = self.clock.tick;
        if self.enforcement.job_for(vid).is_some() {
            return;
        }
        let Some(spot) = self.vehicles.cars.get(vid).and_then(|c| c.spot().copied()).filter(|s| s.kind.on_street()) else { return };
        let illegal = self.parked_illegally(&spot);
        let over = self.kerb_limit(vid).is_some_and(|l| tick >= spot.since + l);
        if !illegal && !over {
            return;
        }
        if spot.ticketed == 0 {
            self.ticket(vid, illegal);
            if illegal {
                self.call_tow(vid, TowReason::Illegal);
            }
        } else if illegal {
            self.call_tow(vid, TowReason::Illegal);
        } else if tick >= spot.ticketed + GRACE {
            self.clamp(vid);
        }
    }

    /// A parking ticket: the fine from the household's purse to the city.
    fn ticket(&mut self, vid: VehicleId, illegal: bool) {
        let tick = self.clock.tick;
        let owner = {
            let car = &mut self.vehicles.cars[vid];
            if let CarPlace::Parked(s) = &mut car.place {
                s.ticketed = tick.max(1);
            }
            car.owner
        };
        self.charge(owner, FINE);
        self.economy.credit(Flow::ParkingFines, FINE);
        let s = &mut self.enforcement.stats;
        s.tickets_today += 1;
        s.tickets_total += 1;
        s.fines_today += FINE;
        s.fines_total += FINE;
        if let Some(c) = self.citizens.get_mut(owner) {
            c.remember(tick, if illegal { format!("Parking ticket, {FINE:.0}: parked where you can't.") } else { format!("Parking ticket, {FINE:.0}: over the two hours.") });
            c.mood = (c.mood - 0.03).max(0.0);
            c.commute.parking_pain += 0.5;
        }
    }

    /// Money out of a citizen's household purse (their pocket without one).
    fn charge(&mut self, id: CitizenId, amount: f32) {
        let Some(c) = self.citizens.get_mut(id) else { return };
        match c.household.and_then(|h| self.households.get_mut(h)) {
            Some(h) => h.money -= amount,
            None => c.savings -= amount,
        }
    }

    // ── towing ─────────────────────────────────────────────────────────────

    /// Tow trucks a tow company runs: one per four staff, at least one
    /// with anyone there; none when it is shut.
    pub fn tow_trucks(&self, b: BuildingId) -> usize {
        let Some(bld) = self.buildings.get(b) else { return 0 };
        let ok = bld.operating() && !bld.is_derelict() && bld.business.as_ref().is_some_and(|z| z.kind == BusinessKind::TowYard && !z.closed);
        if !ok || bld.workers.is_empty() {
            return 0;
        }
        (bld.workers.len() / TRUCKS_PER_STAFF).max(1)
    }

    /// The nearest tow company in town with a truck free.
    pub fn tow_company_near(&self, p: Vec2) -> Option<BuildingId> {
        self.buildings
            .iter()
            .filter(|(id, _)| {
                let busy = self.enforcement.jobs.iter().filter(|j| j.company == Some(*id) && j.truck.is_some()).count();
                // [services] And its trucks out to wrecks.
                let wrecks = self.driving.wrecks.iter().filter(|w| w.company == Some(*id) && w.truck.is_some()).count();
                busy + wrecks < self.tow_trucks(*id)
            })
            .filter_map(|(id, b)| self.lots.get(b.lot).filter(|l| l.car_segment.is_some()).map(|l| (id, l.center.distance(p))))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// Call a tow truck for a parked car.
    pub(super) fn call_tow(&mut self, vid: VehicleId, reason: TowReason) -> Option<u32> {
        self.call_tow_after(vid, reason, 0)
    }

    /// Call a tow truck that sets off no sooner than `wait` minutes from
    /// now ([clamp] the clamp fine's time limit).
    fn call_tow_after(&mut self, vid: VehicleId, reason: TowReason, wait: u64) -> Option<u32> {
        if self.enforcement.job_for(vid).is_some() {
            return None;
        }
        let pos = self.vehicles.cars.get(vid)?.spot()?.pos;
        let company = self.tow_company_near(pos);
        let delay = wait.max(if company.is_some() { 3 } else { OUT_OF_TOWN_DELAY });
        let id = self.enforcement.next_job;
        self.enforcement.next_job += 1;
        let tick = self.clock.tick;
        self.enforcement.jobs.push(TowJob { id, car: vid, company, truck: None, state: TowState::Waiting { until: tick + delay }, reason, since: tick, contra: None });
        Some(id)
    }

    fn step_tow_jobs(&mut self) {
        let tick = self.clock.tick;
        let jobs: Vec<TowJob> = self.enforcement.jobs.clone();
        for job in jobs {
            // The car is gone from where it was (the owner drove off, sold
            // it, left town): nothing to tow.
            let parked = self.vehicles.cars.get(job.car).and_then(|c| c.spot().copied());
            match job.state {
                TowState::Waiting { until } => {
                    if !parked.is_some_and(|s| s.kind.on_street()) {
                        self.enforcement.jobs.retain(|j| j.id != job.id);
                        continue;
                    }
                    if tick >= until {
                        self.dispatch_tow(job.id);
                    }
                }
                TowState::ToCar | TowState::ToPound => {
                    let truck = job.truck.and_then(|t| self.traffic.cars.get(t).map(|c| (t, c.pos, c.heading)));
                    match truck {
                        Some((_, pos, heading)) if job.state == TowState::ToPound => {
                            // The car rides on the truck.
                            if let Some(CarPlace::Parked(s)) = self.vehicles.cars.get_mut(job.car).map(|c| &mut c.place) {
                                s.pos = pos;
                                s.heading = heading;
                            }
                        }
                        Some(_) => {}
                        None => {
                            // The truck was taken off the road (roads rebuilt).
                            if job.state == TowState::ToPound {
                                self.impound(job.car, job.company);
                                self.enforcement.jobs.retain(|j| j.id != job.id);
                            } else if let Some(j) = self.enforcement.jobs.iter_mut().find(|j| j.id == job.id) {
                                j.truck = None;
                                j.state = TowState::Waiting { until: tick + 5 };
                            }
                        }
                    }
                }
            }
        }
    }

    /// Send the truck: from the company's yard, or in from the map edge.
    fn dispatch_tow(&mut self, job_id: u32) {
        let tick = self.clock.tick;
        let Some(job) = self.enforcement.job(job_id).copied() else { return };
        let Some(spot) = self.vehicles.cars.get(job.car).and_then(|c| c.spot().copied()) else { return };
        let Some((seg, pt)) = self.spot_exit(&spot) else { return };
        // A company busy elsewhere meanwhile: look again.
        // (A company counts its trucks out on other jobs: the one it had
        // free when the call came may have gone since.)
        let free = |c: BuildingId| {
            let out = self.enforcement.jobs.iter().filter(|j| j.id != job_id && j.company == Some(c) && j.truck.is_some()).count();
            // [services] And those out at wrecks.
            let wrecks = self.driving.wrecks.iter().filter(|w| w.company == Some(c) && w.truck.is_some()).count();
            self.tow_trucks(c) > out + wrecks
        };
        let company = job.company.filter(|&c| free(c)).or_else(|| self.tow_company_near(spot.pos));
        // The company from out of town has a fleet of its own, not one
        // lorry per call: with every truck out, the job waits. Uncapped, a
        // city short of parking (every overflow car a tow call) had 170 tow
        // trucks on its roads, two thirds of them stuck in the jam they
        // were feeding.
        if company.is_none() {
            let out = self.enforcement.jobs.iter().filter(|j| j.company.is_none() && j.truck.is_some_and(|t| self.traffic.cars.contains_key(t))).count();
            // [services] Its trucks out at wrecks count too, but a wreck is
            // never kept waiting on this cap (`World::step_wrecks`): a crash
            // blocking a road comes before a parking ticket.
            let wrecks = self.driving.wrecks.iter().filter(|w| w.company.is_none() && w.truck.is_some_and(|t| self.traffic.cars.contains_key(t))).count();
            if out + wrecks >= OUT_OF_TOWN_TOW_TRUCKS {
                if let Some(j) = self.enforcement.jobs.iter_mut().find(|j| j.id == job_id) {
                    j.state = TowState::Waiting { until: tick + 10 };
                }
                return;
            }
        }
        let route = match company {
            Some(c) => self.truck_stop(c).and_then(|(fs, fp)| self.route_any_lane(fs, fp, seg, pt)),
            None => self.route_from_edge(seg, pt),
        };
        // [tow-moves] On a one-way street with a long way round: in from
        // the far end, the last stretch against the traffic.
        let start = match (&route, company) {
            (Some(r), _) => Some((r.0[0], r.1)),
            (None, Some(c)) => self.truck_stop(c).and_then(|(s, p)| self.traffic.net.nearest_lane_on(s, p)).map(|(l, s, _)| (l, s)),
            _ => None,
        };
        let contra = start.and_then(|(l0, s0)| {
            let net = &self.traffic.net;
            self.plan_contraflow(seg, spot.pos, route.as_ref(), &|l, s| net.route_lanes(l0, s0, l, s))
        });
        let (route, contra) = match contra {
            Some((alt, c)) => (Some(alt), Some(c)),
            None => (route, None),
        };
        let Some((route, s0, s1)) = route else {
            if let Some(j) = self.enforcement.jobs.iter_mut().find(|j| j.id == job_id) {
                j.state = TowState::Waiting { until: tick + 30 };
            }
            return;
        };
        let truck = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, 3);
        let towed = self.vehicles.cars.get(job.car).map_or(BodyType::Sedan, |c| c.model.body);
        let t = &mut self.traffic.cars[truck];
        t.model = tow_model_for(towed);
        t.errand = Errand::Tow(job_id);
        if let Some(j) = self.enforcement.jobs.iter_mut().find(|j| j.id == job_id) {
            j.company = company;
            j.truck = Some(truck);
            j.state = TowState::ToCar;
            j.contra = contra;
        }
    }

    /// A tow truck reached the car (hook it, off to the pound) or the
    /// pound (in it goes).
    pub(super) fn tow_truck_arrived(&mut self, truck: CarId, job_id: u32) {
        let Some(job) = self.enforcement.job(job_id).copied() else {
            self.tow_truck_home(truck, None);
            return;
        };
        match job.state {
            TowState::ToCar => {
                // [tow-moves] In from the far end of a one-way: up the
                // street to the car first, against the traffic.
                if let Some(c) = job.contra.filter(|_| !self.tow_moving(truck)) {
                    if self.traffic.cars.get(truck).is_some_and(|t| t.pos.distance(c.stop) > 15.0) && self.vehicles.cars.get(job.car).and_then(|c| c.spot()).is_some_and(|s| s.kind.on_street()) {
                        self.begin_contraflow(truck, job_id, c);
                        return;
                    }
                }
                if !self.hook_parked(truck, job_id) {
                    self.tow_truck_home(truck, job.company);
                    return;
                }
                // [tow-moves] Turned round in the street when the way to
                // the pound lies back the other way.
                if let Some(goal) = self.pound_goal(truck, job_id) {
                    if self.maybe_u_turn(truck, goal, super::tow_moves::AfterMove::ToPound { job: job_id }) {
                        return;
                    }
                }
                // [services] With no road there yet it waits at the kerb with
                // the car on the hook and tries again (it used to vanish, the
                // car teleported into the pound).
                if !self.route_to_pound(truck, job_id) {
                    self.traffic.pull_in(truck, super::cars::RETIRE_RETRY_SECONDS);
                }
            }
            TowState::ToPound => {
                // [services] Only at the pound (the yard's driveway, or the
                // map edge for one out of town) does the car go in; a truck
                // that held for want of a road, or whose yard was pulled
                // down on the way, drives on.
                if !self.at_pound(truck, job_id) {
                    if !self.route_to_pound(truck, job_id) {
                        self.traffic.pull_in(truck, super::cars::RETIRE_RETRY_SECONDS);
                    }
                    return;
                }
                let company = self.enforcement.job(job_id).and_then(|j| j.company);
                self.traffic.remove_car(truck);
                self.impound(job.car, company);
                // [clamp] The unpaid clamp fine is owed at the pound too.
                if job.reason == TowReason::Unpaid {
                    if let Some(CarPlace::Parked(s)) = self.vehicles.cars.get_mut(job.car).map(|c| &mut c.place) {
                        s.fee += CLAMP_FINE;
                    }
                    self.enforcement.stats.clamps_towed_total += 1;
                }
                self.enforcement.jobs.retain(|j| j.id != job_id);
            }
            TowState::Waiting { .. } => self.tow_truck_home(truck, job.company),
        }
    }

    /// The truck at the car of job `job_id`: hook it on (the car rides on
    /// the truck to the pound). False, and the job dropped, when the car
    /// went before the truck came.
    pub(super) fn hook_parked(&mut self, truck: CarId, job_id: u32) -> bool {
        let tick = self.clock.tick;
        let Some(job) = self.enforcement.job(job_id).copied() else { return false };
        if job.state != TowState::ToCar {
            return false;
        }
        let spot = self.vehicles.cars.get(job.car).and_then(|c| c.spot().copied()).filter(|s| s.kind.on_street());
        let (Some(_), Some(t)) = (spot, self.traffic.cars.get(truck)) else {
            // Gone before the truck came.
            self.enforcement.jobs.retain(|j| j.id != job_id);
            return false;
        };
        let (pos, heading) = (t.pos, t.heading);
        self.unpark(job.car);
        let model = self.vehicles.cars[job.car].model;
        self.vehicles.cars[job.car].place = CarPlace::Parked(ParkSpot { kind: ParkKind::Hooked, building: job.company, pos, heading, since: tick, ..Default::default() });
        // Drawn on the truck, as the wreck trucks carry theirs.
        self.driving.tows.push(super::driving::Tow { truck, model, yard: job.company });
        let owner = self.vehicles.cars[job.car].owner;
        if let Some(c) = self.citizens.get_mut(owner) {
            c.remember(tick, format!("My car was towed ({}).", job.reason.label()));
            c.mood = (c.mood - 0.05).max(0.0);
            c.commute.parking_pain += 1.0;
        }
        // On to the pound.
        if let Some(j) = self.enforcement.jobs.iter_mut().find(|j| j.id == job_id) {
            j.state = TowState::ToPound;
        }
        true
    }

    /// An idle tow truck goes back to its yard, or off the map.
    pub(super) fn tow_truck_home(&mut self, truck: CarId, company: Option<BuildingId>) {
        let home = company.and_then(|c| self.truck_stop(c)).and_then(|(s, p)| self.traffic.net.nearest_lane_on(s, p));
        if let (Some(c), Some((goal, s_end, _))) = (company, home) {
            if self.traffic.redirect(truck, goal, s_end) {
                self.traffic.cars[truck].errand = Errand::Return(c);
                return;
            }
        }
        if self.redirect_out(truck) {
            self.traffic.cars[truck].errand = Errand::Leave;
            return;
        }
        // [services] No road out either: it holds and tries again.
        self.retire_vehicle(truck);
    }

    /// [services] Is the tow truck of job `job_id` at its pound: the
    /// company's driveway (while the company stands), else the map edge?
    fn at_pound(&self, truck: CarId, job_id: u32) -> bool {
        if !self.traffic.cars.contains_key(truck) {
            return true;
        }
        match self.enforcement.job(job_id).and_then(|j| j.company).filter(|c| self.buildings.contains_key(*c)) {
            Some(c) => self.truck_stop(c).is_some_and(|(seg, p)| self.car_at_stop(truck, seg, p, POUND_REACH)),
            None => self.at_map_edge(truck),
        }
    }

    /// [services] Send the tow truck of job `job_id` on to its pound: the
    /// company's yard, or (the company gone, or none in town) the pound
    /// out of town past the map edge. False when no road leads there.
    pub(super) fn route_to_pound(&mut self, truck: CarId, job_id: u32) -> bool {
        let company = self.enforcement.job(job_id).and_then(|j| j.company);
        if let Some(c) = company {
            if self.buildings.contains_key(c) {
                let stop = self.truck_stop(c).and_then(|(s, p)| self.traffic.net.nearest_lane_on(s, p));
                return stop.is_some_and(|(goal, s_end, _)| self.traffic.redirect(truck, goal, s_end));
            }
            // The yard was pulled down: the pound out of town takes it.
            if let Some(j) = self.enforcement.jobs.iter_mut().find(|j| j.id == job_id) {
                j.company = None;
            }
            if let Some(t) = self.driving.tows.iter_mut().find(|t| t.truck == truck) {
                t.yard = None;
            }
        }
        self.redirect_out(truck)
    }

    /// In the pound: the company's yard, or one out of town.
    fn impound(&mut self, vid: VehicleId, company: Option<BuildingId>) {
        let tick = self.clock.tick;
        let company = company.filter(|c| self.buildings.contains_key(*c));
        let pos = match company {
            Some(c) => self.building_entrance(c).unwrap_or_default(),
            None => self.edge_exits(Vec2::ZERO).first().map(|&(l, s)| self.traffic.net.lanes[l as usize].point_at(s).0).unwrap_or_default(),
        };
        let fee = if company.is_some() { TOW_FEE } else { OUT_OF_TOWN_TOW_FEE };
        let Some(car) = self.vehicles.cars.get_mut(vid) else { return };
        car.place = CarPlace::Parked(ParkSpot { kind: ParkKind::Pound, building: company, pos, since: tick, fee, ..Default::default() });
        let owner = car.owner;
        let s = &mut self.enforcement.stats;
        s.tows_today += 1;
        s.tows_total += 1;
        if company.is_none() {
            s.tows_out_of_town_today += 1;
            s.tows_out_of_town_total += 1;
        }
        let name = company.map_or("a pound out of town".to_string(), |c| self.buildings[c].name.clone());
        if let Some(c) = self.citizens.get_mut(owner) {
            c.parked_at = None;
            c.remember(tick, format!("The car's in the pound at {name}. {fee:.0} to get it back."));
        }
    }

    /// What the owner pays to get a car out: the tow fee and storage for
    /// every day begun.
    pub fn release_fee(&self, spot: &ParkSpot) -> f32 {
        spot.fee + STORAGE_PER_DAY * (self.clock.tick.saturating_sub(spot.since) / 1440 + 1) as f32
    }

    /// The owner pays to take the car out of the pound.
    pub(super) fn pay_release(&mut self, owner: CitizenId, spot: ParkSpot) {
        let tick = self.clock.tick;
        let fee = self.release_fee(&spot);
        self.charge(owner, fee);
        if let Some(bz) = spot.building.and_then(|b| self.buildings.get_mut(b)).and_then(|b| b.business.as_mut()) {
            bz.money += fee;
            bz.revenue_today += fee;
            bz.deals += 1;
        }
        let s = &mut self.enforcement.stats;
        s.released_today += 1;
        s.released_total += 1;
        s.release_fees_today += fee;
        if let Some(c) = self.citizens.get_mut(owner) {
            c.remember(tick, format!("Paid {fee:.0} to get the car out of the pound."));
            c.commute.parking_pain += 0.5;
        }
    }

    /// Hourly: cars in the pound are fetched (when the owner can pay and
    /// has the time) or, after `AUCTION_AFTER`, auctioned.
    pub(super) fn hourly_pound(&mut self) {
        let tick = self.clock.tick;
        self.settle_clamps();
        self.rebook_booted();
        let impounded: Vec<(VehicleId, ParkSpot)> = self.vehicles.cars.iter().filter_map(|(id, c)| c.spot().filter(|s| s.kind == ParkKind::Pound).map(|s| (id, *s))).collect();
        for (vid, spot) in impounded {
            // A yard that closed: its pound went to a company out of town.
            if spot.building.is_some_and(|b| !self.buildings.contains_key(b)) {
                if let Some(CarPlace::Parked(s)) = self.vehicles.cars.get_mut(vid).map(|c| &mut c.place) {
                    s.building = None;
                }
            }
            if tick >= spot.since + AUCTION_AFTER {
                self.auction(vid);
            } else {
                self.maybe_collect(vid);
            }
        }
    }

    /// The owner goes to fetch the car: to the pound in town (the walk or
    /// ride there, then the drive home), or pays a pound out of town to
    /// deliver it.
    fn maybe_collect(&mut self, vid: VehicleId) {
        let hour = self.clock.hour() as u8;
        let Some(car) = self.vehicles.cars.get(vid) else { return };
        let Some(spot) = car.spot().copied() else { return };
        let owner = car.owner;
        let Some(c) = self.citizens.get(owner) else { return };
        if c.car != Some(vid) || c.asleep || c.to_car.is_some() || c.activity.is_traveling() || !(8..20).contains(&hour) {
            return;
        }
        let Some(home) = c.home else { return };
        let purse = c.household.and_then(|h| self.households.get(h)).map_or(c.savings, |h| h.money);
        let fee = self.release_fee(&spot);
        if purse < fee * 0.5 {
            return;
        }
        match spot.building {
            Some(pound) => {
                self.travel_with(owner, Some(pound), Purpose::Wander, true, true, false);
                if self.citizens[owner].activity.is_traveling() {
                    let c = &mut self.citizens[owner];
                    c.to_car = Some(ToCar { car: vid, to: home, purpose: Purpose::Home });
                    c.thought = "Off to get the car out of the pound.".to_string();
                }
            }
            None => {
                // Delivered from out of town, for the fee.
                let Some((seg, pt)) = self.car_stop(home) else { return };
                let Some((route, s0, s1)) = self.route_from_edge(seg, pt) else { return };
                self.pay_release(owner, spot);
                let (model, color) = (car_model(self, vid), self.vehicles.cars[vid].color);
                let t = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, color);
                let tc = &mut self.traffic.cars[t];
                tc.model = model;
                tc.owned = Some(vid);
                tc.errand = Errand::Deliver;
                self.vehicles.cars[vid].place = CarPlace::Driving(t);
            }
        }
    }

    /// Unclaimed: sold at auction (the money to the tow company); the car
    /// drives off to its buyer out of town.
    fn auction(&mut self, vid: VehicleId) {
        let tick = self.clock.tick;
        let Some(car) = self.vehicles.cars.get(vid) else { return };
        let Some(spot) = car.spot().copied() else { return };
        let (owner, price) = (car.owner, car.model.price() * 0.3);
        if let Some(bz) = spot.building.and_then(|b| self.buildings.get_mut(b)).and_then(|b| b.business.as_mut()) {
            bz.money += price;
            bz.revenue_today += price;
        }
        if let Some(c) = self.citizens.get_mut(owner).filter(|c| c.car == Some(vid)) {
            c.car = None;
            c.owns_car = false;
            c.vehicle = None;
            c.parked_at = None;
            c.remember(tick, "Never fetched the car from the pound. They auctioned it off.");
        }
        let s = &mut self.enforcement.stats;
        s.auctioned_today += 1;
        s.auctioned_total += 1;
        let driven = spot.building.is_some() && self.drive_out_of_town(vid, Errand::Sale(BuildingId::default())).is_some();
        if !driven {
            self.drop_car(vid, CarEvent::Sold);
        }
    }

    // ── [boot] clamped on the player's orders ──────────────────────────────

    /// [boot] Have a private car clamped and towed: `vid` is its record.
    /// On the road it pulls over first (`order_tow_car`). Returns the tow job.
    pub fn order_tow(&mut self, vid: VehicleId) -> Result<u32, String> {
        let car = self.vehicles.cars.get(vid).ok_or("no such car")?;
        let spot = match car.place {
            CarPlace::Driving(cid) => return self.order_tow_car(cid),
            CarPlace::Parked(s) => s,
        };
        match spot.kind {
            ParkKind::Hooked | ParkKind::Pound => return Err("it has already been towed".into()),
            ParkKind::Dealer => return Err("it is on a dealer's forecourt".into()),
            k if !k.on_street() => return Err(format!("it is parked off the street ({}); only cars at the kerb can be clamped", k.label())),
            _ => {}
        }
        Ok(self.boot(vid))
    }

    /// [boot] The same for a car on the road: it pulls over to the kerb
    /// (a free slot close by, else the edge of the street where it is),
    /// off the lanes so traffic goes round it, and everyone gets out and
    /// walks on.
    pub fn order_tow_car(&mut self, car_id: CarId) -> Result<u32, String> {
        let car = self.traffic.cars.get(car_id).ok_or("no such car")?;
        if car.bus.is_some() || car.cargo.is_some() || car.siren || self.services.unit_of_car(car_id).is_some() {
            return Err("only private cars can be towed".into());
        }
        let Some(vid) = car.owned.filter(|v| self.vehicles.cars.contains_key(*v)) else {
            return Err("only residents' cars can be towed; this one is passing through".into());
        };
        match car.errand {
            Errand::Trip | Errand::Search { .. } | Errand::ToLot(_) | Errand::ToKerb | Errand::Deliver => {}
            Errand::Leave | Errand::Sale(_) => return Err("it is on its way out of town".into()),
            _ => return Err("only private cars can be towed".into()),
        }
        let (pos, heading, driver) = (car.pos, car.heading, car.citizen);
        // The kerb to the right of the lane, if no slot is free.
        let dir = Vec2::new(heading.cos(), heading.sin());
        let edge = pos + Vec2::new(dir.y, -dir.x) * 2.6;
        let tick = self.clock.tick;
        let spot = self.boot_spot(pos).unwrap_or_else(|| ParkSpot {
            kind: ParkKind::Overflow,
            segment: self.roads.nearest_street(pos).map(|(s, _, _)| s),
            slot: u16::MAX,
            pos: edge,
            heading,
            since: tick,
            ..Default::default()
        });
        self.traffic.remove_car(car_id);
        self.park_at(vid, spot);
        // The driver gets out and walks the rest; passengers follow.
        if self.citizens.get(driver).is_some_and(|c| matches!(c.activity, Activity::Driving { car, .. } if car == car_id)) {
            let c = &mut self.citizens[driver];
            c.commute.drive_since = 0;
            c.commute.search_since = 0;
            self.walk_from_car(driver, spot.pos);
        }
        Ok(self.boot(vid))
    }

    /// Clamp a car standing at the kerb and call the tow truck. The owner
    /// is not pleased.
    fn boot(&mut self, vid: VehicleId) -> u32 {
        let tick = self.clock.tick;
        let (owner, already) = {
            let car = &mut self.vehicles.cars[vid];
            let mut already = false;
            if let CarPlace::Parked(s) = &mut car.place {
                already = s.booted;
                s.booted = true;
            }
            (car.owner, already)
        };
        let job = match self.enforcement.job_for(vid) {
            Some(j) => j.id,
            None => self.call_tow(vid, TowReason::Ordered).unwrap_or(u32::MAX),
        };
        // Clamped once: a second order changes nothing.
        if already {
            return job;
        }
        let s = &mut self.enforcement.stats;
        s.boots_today += 1;
        s.boots_total += 1;
        if let Some(c) = self.citizens.get_mut(owner) {
            c.remember(tick, "Somebody had my car clamped. Now it's getting towed and I'll have to pay to get it back.");
            c.thought = "They clamped my car. Unbelievable.".to_string();
            c.mood = (c.mood - 0.08).max(0.0);
            c.commute.parking_pain += 1.0;
            self.concerns.said(tick, owner, c, crate::concerns::Concern::Parking, 0.6, "They clamped my car and towed it.");
        }
        job
    }

    /// [clamp] An officer clamps a car still over its limit after the
    /// ticket's grace: a clamp fine, and the tow truck booked for when
    /// `CLAMP_LIMIT` runs out unless the owner pays first (`settle_clamps`).
    pub(super) fn clamp(&mut self, vid: VehicleId) -> Option<u32> {
        let tick = self.clock.tick;
        let owner = {
            let car = self.vehicles.cars.get_mut(vid)?;
            let CarPlace::Parked(s) = &mut car.place else { return None };
            if s.booted || !s.kind.on_street() {
                return None;
            }
            s.booted = true;
            car.owner
        };
        let job = self.call_tow_after(vid, TowReason::Unpaid, CLAMP_LIMIT);
        let s = &mut self.enforcement.stats;
        s.clamps_today += 1;
        s.clamps_total += 1;
        if let Some(c) = self.citizens.get_mut(owner) {
            c.remember(tick, format!("My car got clamped. {CLAMP_FINE:.0} to get it off, or they tow it."));
            c.thought = "Clamped. Of course it's clamped.".to_string();
            c.mood = (c.mood - 0.05).max(0.0);
            c.commute.parking_pain += 0.8;
            self.concerns.said(tick, owner, c, crate::concerns::Concern::Parking, 0.4, "They clamped my car for overstaying.");
        }
        job
    }

    /// [clamp] Hourly: owners of clamped cars who can pay may settle the
    /// fine (awake, by day, the money in the household purse, and getting
    /// round to it: `CLAMP_PAY_CHANCE` an hour by a hash of car and hour).
    /// The clamp comes off and the tow is cancelled. The rest are towed
    /// when their time runs out.
    fn settle_clamps(&mut self) {
        let tick = self.clock.tick;
        let hour = self.clock.hour() as u8;
        if !(7..23).contains(&hour) {
            return;
        }
        let waiting: Vec<(u32, VehicleId)> = self.enforcement.jobs.iter().filter(|j| j.reason == TowReason::Unpaid && matches!(j.state, TowState::Waiting { .. })).map(|j| (j.id, j.car)).collect();
        for (job, vid) in waiting {
            let Some(car) = self.vehicles.cars.get(vid) else { continue };
            if !car.spot().is_some_and(|s| s.booted) {
                continue;
            }
            let owner = car.owner;
            let Some(c) = self.citizens.get(owner) else { continue };
            if c.asleep || self.services.in_custody(owner) {
                continue;
            }
            let purse = c.household.and_then(|h| self.households.get(h)).map_or(c.savings, |h| h.money);
            let key = slotmap::Key::data(&vid).as_ffi();
            if purse < CLAMP_FINE || crate::pedestrians::hash01(key, tick / 60, 0xC1A3) >= CLAMP_PAY_CHANCE {
                continue;
            }
            self.charge(owner, CLAMP_FINE);
            self.economy.credit(Flow::ParkingFines, CLAMP_FINE);
            if let Some(CarPlace::Parked(s)) = self.vehicles.cars.get_mut(vid).map(|c| &mut c.place) {
                s.booted = false;
                // A fresh start at the kerb: the ticket is settled.
                s.ticketed = 0;
                s.since = tick;
            }
            self.enforcement.jobs.retain(|j| j.id != job);
            let s = &mut self.enforcement.stats;
            s.clamps_paid_today += 1;
            s.clamps_paid_total += 1;
            s.clamp_fines_today += CLAMP_FINE;
            s.fines_today += CLAMP_FINE;
            s.fines_total += CLAMP_FINE;
            if let Some(c) = self.citizens.get_mut(owner) {
                c.remember(tick, format!("Paid {CLAMP_FINE:.0} to get the clamp off the car."));
                c.commute.parking_pain += 0.3;
            }
        }
    }

    /// [boot] Clamped cars whose tow job was lost (a truck taken off the
    /// road, roads rebuilt) get a fresh one.
    fn rebook_booted(&mut self) {
        let lost: Vec<VehicleId> = self.vehicles.cars.iter().filter(|(id, c)| c.spot().is_some_and(|s| s.booted) && self.enforcement.job_for(*id).is_none()).map(|(id, _)| id).collect();
        for vid in lost {
            self.call_tow(vid, TowReason::Ordered);
        }
    }

    // ── parking lots the city runs ─────────────────────────────────────────

    /// Set the fee at a lot the city runs.
    pub fn set_parking_price(&mut self, b: BuildingId, price: f32) -> Result<f32, String> {
        let price = price.clamp(0.0, 50.0);
        let lot = self.vehicles.city_lots.get_mut(&b).ok_or("not a parking lot the city runs")?;
        lot.price = price;
        if let Some(bld) = self.buildings.get_mut(b) {
            bld.parking_price = price;
        }
        Ok(price)
    }

    /// Midnight: the city pays for its lots.
    pub(super) fn daily_city_lots(&mut self) {
        let buildings = &self.buildings;
        self.vehicles.city_lots.retain(|b, _| buildings.contains_key(*b));
        let lots: Vec<(BuildingId, f32)> = self.vehicles.city_lots.iter().map(|(b, l)| (*b, l.size.upkeep_per_day())).collect();
        for (b, upkeep) in lots {
            self.economy.debit_item(Flow::ServiceUpkeep, upkeep, Item::Building(b));
        }
    }
}

fn car_model(w: &World, vid: VehicleId) -> VehicleModel {
    w.vehicles.cars.get(vid).map_or_else(VehicleModel::default, |c| c.model)
}
