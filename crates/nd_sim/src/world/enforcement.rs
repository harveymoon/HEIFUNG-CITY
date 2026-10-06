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

/// What a tow truck looks like (as the driving agent's wreck trucks do).
pub(super) fn tow_model() -> VehicleModel {
    VehicleModel { class: VehicleClass::Mid, body: BodyType::Pickup, wear: 1, tuned: false, livery: Livery::Utility, paint: 3 }
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
        }
        if tick % 10 == 3 {
            self.staff_patrols();
        }
        if tick % 2 == 0 {
            self.patrols_look();
        }
        self.step_tow_jobs();
        if tick % 60 == 47 {
            self.hourly_pound();
        }
    }

    // ── offices and patrols ────────────────────────────────────────────────

    /// Working parking enforcement offices.
    pub fn enforcement_offices(&self) -> Vec<BuildingId> {
        self.buildings.iter().filter(|(_, b)| b.service.is_some_and(|s| s.kind == ServiceKind::ParkingEnforcement) && b.construction.is_none() && !b.is_derelict()).map(|(id, _)| id).collect()
    }

    /// Patrol cars an office puts out now: by tier, two officers a car,
    /// during patrol hours.
    pub fn enforcement_cars(&self, office: BuildingId) -> usize {
        let Some(b) = self.buildings.get(office) else { return 0 };
        let Some(s) = b.service else { return 0 };
        if !PATROL_HOURS.contains(&(self.clock.hour() as u8)) || b.is_derelict() || b.construction.is_some() {
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
        let gone: Vec<CarId> = self.enforcement.patrols.iter().filter(|p| !self.buildings.contains_key(p.office)).map(|p| p.car).collect();
        for car in gone {
            self.traffic.remove_car(car);
        }
        let traffic = &self.traffic;
        self.enforcement.patrols.retain(|p| traffic.cars.get(p.car).is_some_and(|c| c.errand == Errand::Patrol(p.office)));
    }

    fn send_patrol_home(&mut self, car: CarId, office: BuildingId) {
        let home = self.truck_stop(office).and_then(|(s, p)| self.traffic.net.nearest_lane_on(s, p));
        match home {
            Some((goal, s_end, _)) if self.traffic.redirect(car, goal, s_end) => self.traffic.cars[car].errand = Errand::Return(office),
            _ => {
                self.traffic.remove_car(car);
            }
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
    /// or parked illegally; the tow truck once the grace has run out, or at
    /// once for an illegal one.
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
            self.call_tow(vid, TowReason::Overstay);
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
        let ok = bld.construction.is_none() && !bld.is_derelict() && bld.business.as_ref().is_some_and(|z| z.kind == BusinessKind::TowYard && !z.closed);
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
                busy < self.tow_trucks(*id)
            })
            .filter_map(|(id, b)| self.lots.get(b.lot).filter(|l| l.car_segment.is_some()).map(|l| (id, l.center.distance(p))))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// Call a tow truck for a parked car.
    pub(super) fn call_tow(&mut self, vid: VehicleId, reason: TowReason) -> Option<u32> {
        if self.enforcement.job_for(vid).is_some() {
            return None;
        }
        let pos = self.vehicles.cars.get(vid)?.spot()?.pos;
        let company = self.tow_company_near(pos);
        let delay = if company.is_some() { 3 } else { OUT_OF_TOWN_DELAY };
        let id = self.enforcement.next_job;
        self.enforcement.next_job += 1;
        let tick = self.clock.tick;
        self.enforcement.jobs.push(TowJob { id, car: vid, company, truck: None, state: TowState::Waiting { until: tick + delay }, reason, since: tick });
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
        let company = job.company.filter(|c| self.tow_trucks(*c) > 0).or_else(|| self.tow_company_near(spot.pos));
        let route = match company {
            Some(c) => self.truck_stop(c).and_then(|(fs, fp)| self.route_any_lane(fs, fp, seg, pt)),
            None => self.route_from_edge(seg, pt),
        };
        let Some((route, s0, s1)) = route else {
            if let Some(j) = self.enforcement.jobs.iter_mut().find(|j| j.id == job_id) {
                j.state = TowState::Waiting { until: tick + 30 };
            }
            return;
        };
        let truck = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, 3);
        let t = &mut self.traffic.cars[truck];
        t.model = tow_model();
        t.errand = Errand::Tow(job_id);
        if let Some(j) = self.enforcement.jobs.iter_mut().find(|j| j.id == job_id) {
            j.company = company;
            j.truck = Some(truck);
            j.state = TowState::ToCar;
        }
    }

    /// A tow truck reached the car (hook it, off to the pound) or the
    /// pound (in it goes).
    pub(super) fn tow_truck_arrived(&mut self, truck: CarId, job_id: u32) {
        let tick = self.clock.tick;
        let Some(job) = self.enforcement.job(job_id).copied() else {
            self.tow_truck_home(truck, None);
            return;
        };
        match job.state {
            TowState::ToCar => {
                let spot = self.vehicles.cars.get(job.car).and_then(|c| c.spot().copied()).filter(|s| s.kind.on_street());
                let Some(spot) = spot else {
                    // Gone before the truck came.
                    self.enforcement.jobs.retain(|j| j.id != job_id);
                    self.tow_truck_home(truck, job.company);
                    return;
                };
                let (pos, heading) = {
                    let t = &self.traffic.cars[truck];
                    (t.pos, t.heading)
                };
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
                let _ = spot;
                // On to the pound.
                let to_pound = match job.company {
                    Some(c) => self.truck_stop(c).and_then(|(s, p)| self.traffic.net.nearest_lane_on(s, p)).is_some_and(|(goal, s_end, _)| self.traffic.redirect(truck, goal, s_end)),
                    None => self.redirect_out(truck),
                };
                if let Some(j) = self.enforcement.jobs.iter_mut().find(|j| j.id == job_id) {
                    j.state = TowState::ToPound;
                }
                if !to_pound {
                    self.traffic.remove_car(truck);
                    self.impound(job.car, job.company);
                    self.enforcement.jobs.retain(|j| j.id != job_id);
                }
            }
            TowState::ToPound => {
                self.traffic.remove_car(truck);
                self.impound(job.car, job.company);
                self.enforcement.jobs.retain(|j| j.id != job_id);
            }
            TowState::Waiting { .. } => self.tow_truck_home(truck, job.company),
        }
    }

    /// An idle tow truck goes back to its yard, or off the map.
    fn tow_truck_home(&mut self, truck: CarId, company: Option<BuildingId>) {
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
        self.traffic.remove_car(truck);
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
                let Some((seg, pt)) = self.building_lot(home).and_then(|l| l.car_segment.map(|s| (s, l.car_point))) else { return };
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
        let upkeep: f32 = self.vehicles.city_lots.values().map(|l| l.size.upkeep_per_day()).sum();
        if upkeep > 0.0 {
            self.economy.debit(Flow::ServiceUpkeep, upkeep);
        }
    }
}

fn car_model(w: &World, vid: VehicleId) -> VehicleModel {
    w.vehicles.cars.get(vid).map_or_else(VehicleModel::default, |c| c.model)
}
