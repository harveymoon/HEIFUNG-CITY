//! [taxis] Taxi companies and their cabs, the systems (`crate::taxi` holds
//! the data and the rules): the mode choice's taxi option and the demand
//! nobody served, a company opening on that demand, the dispatcher, cabs
//! picking riders up at the kerb and setting them down at the far kerb,
//! fares, ranks, shifts, the yard at night, and fleets growing or shrinking
//! with trade.
//!
//! Draws from its own random stream (`trng`) for anything it decides, so a
//! city with no taxi company runs as it did before; the one shared draw is
//! the mode choice's whim for the taxi option, which only exists once a
//! company does.

use super::*;
use crate::citizens::TripMode;
use crate::taxi::{self, Cab, CabId, CabJob, CabPlace, TaxiRequest};
use crate::traffic::LaneKind;
use crate::vehicle::Errand;
use super::trips::{ModeOption, ModePlan, TripQuery};

/// A place cabs queue for fares: a station forecourt, a venue in the
/// evening, a company's own gate.
#[derive(Clone, Copy, Debug)]
struct Rank {
    building: BuildingId,
    segment: SegmentId,
    at: Vec2,
}

impl World {
    fn trng(&mut self) -> SimRng {
        // A fresh generator per draw site keyed by the tick: deterministic,
        // and never shifts the world's own stream.
        crate::rng_for(self.config.seed ^ 0x7A_C1_CA_B5, self.clock.tick)
    }

    // ── companies ──────────────────────────────────────────────────────────

    /// Is `b` a working taxi company (built, open, not derelict)?
    pub fn taxi_company_works(&self, b: BuildingId) -> bool {
        self.buildings.get(b).is_some_and(|bld| bld.construction.is_none() && bld.demolition.is_none() && !bld.is_derelict() && bld.business.as_ref().is_some_and(|z| z.kind == BusinessKind::TaxiCompany && !z.closed))
    }

    /// Every working taxi company.
    pub fn taxi_companies(&self) -> Vec<BuildingId> {
        self.buildings.keys().filter(|b| self.taxi_company_works(*b)).collect()
    }

    /// A cab of some company is on duty: the taxi option exists.
    pub fn taxis_in_service(&self) -> bool {
        self.taxis.cabs.values().any(|c| c.on_duty && c.job != CabJob::Leaving)
    }

    /// Open a taxi company on a vacant lot with a street: a yard of bays
    /// with a few cabs to start. `None` when the lot will not do.
    pub fn build_taxi_company(&mut self, lot_id: LotId, name: String) -> Option<BuildingId> {
        let lot = self.lots.get(lot_id)?;
        if lot.building.is_some() || lot.landlocked || lot.car_segment.is_none() || self.under_facility(lot.center) || lot.area() < taxi::YARD_MIN_AREA {
            return None;
        }
        let jobs = taxi::COMPANY_JOBS;
        let mut bz = Business::new(BusinessKind::TaxiCompany, jobs);
        // The founder's stake buys the first cabs; this is the float.
        bz.money = 300.0;
        let id = self.place_special(lot_id, name, jobs, 0, |b| {
            b.floors = 0;
            b.tier = Tier::Low;
            b.business = Some(bz);
        });
        for _ in 0..taxi::START_CABS {
            self.add_cab(id);
        }
        self.taxis.stats.companies_founded_total += 1;
        Some(id)
    }

    /// A new cab in `company`'s yard.
    fn add_cab(&mut self, company: BuildingId) -> CabId {
        let number = self.taxis.fleet(company).map(|(_, c)| c.number).max().unwrap_or(0) + 1;
        let pos = self.building_lot(company).map_or(Vec2::ZERO, |l| l.center);
        let tick = self.clock.tick;
        self.taxis.cabs.insert(Cab { company, number, model: taxi::cab_model(number), place: CabPlace::Yard, job: CabJob::Idle, on_duty: false, last_pos: pos, since: tick })
    }

    /// What a citizen could put into a business: their own savings and
    /// their household's.
    fn wealth_of(&self, c: &crate::citizens::Citizen) -> f32 {
        c.savings.max(0.0) + c.household.and_then(|h| self.households.get(h)).map_or(0.0, |h| h.money.max(0.0))
    }

    /// Development's taxi branch: when enough trips today wanted a cab and
    /// found none, a citizen with savings opens a company on a vacant lot
    /// (commercial land first) near where those trips began. True if one
    /// opened.
    pub(super) fn maybe_found_taxi_company(&mut self, vacant: &[LotId]) -> bool {
        let companies = self.taxi_companies().len() as u32;
        if companies as usize >= (self.citizens.len() / taxi::PEOPLE_PER_COMPANY).max(1) {
            return false;
        }
        let need = taxi::FOUND_DEMAND * (1 + companies);
        if self.taxis.latent_today.max(self.taxis.latent_yesterday) < need || self.taxis.latent_today == 0 {
            return false;
        }
        let at = self.taxis.latent_sum / self.taxis.latent_today as f32;
        let mut candidates: Vec<(f32, LotId)> = vacant
            .iter()
            .copied()
            .filter(|&l| self.lots.get(l).is_some_and(|lot| matches!(lot.land_use, LandUse::Commercial | LandUse::Industrial) && lot.car_segment.is_some() && lot.area() >= taxi::YARD_MIN_AREA))
            .map(|l| {
                let lot = &self.lots[l];
                // Commercial land is where a cab firm belongs; industrial will do.
                let k = if lot.land_use == LandUse::Commercial { 1.0 } else { 1.6 };
                (lot.center.distance(at) * k, l)
            })
            .collect();
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let Some(&(d, lot_id)) = candidates.first() else { return false };
        if d > 900.0 {
            return false;
        }
        // The founder: a grown-up with savings living near the demand.
        let lot_pos = self.lots[lot_id].center;
        let wealth = |c: &crate::citizens::Citizen| self.wealth_of(c);
        let founder = self
            .citizens
            .iter()
            .filter(|(_, c)| c.stage() == crate::citizens::LifeStage::Adult && wealth(c) >= 200.0)
            .filter_map(|(id, c)| c.home.and_then(|h| self.building_lot(h)).map(|l| (id, l.center.distance(lot_pos), wealth(c))))
            // The most savings, a local preferred: a kilometre away counts
            // as 200 less.
            .max_by(|a, b| (a.2 - a.1 * 0.2).total_cmp(&(b.2 - b.1 * 0.2)).then(b.0.cmp(&a.0)))
            .map(|(id, _, _)| id);
        let mut name = match founder.and_then(|f| self.citizens.get(f)) {
            Some(c) => format!("{} Cabs", c.name.split_whitespace().last().unwrap_or("Neon")),
            None => {
                let mut rng = self.trng();
                ["Neon Cab Co.", "Night Owl Taxis", "Glowline Cabs", "Chrome Cab Co.", "Kerbside Taxis"].choose(&mut rng).map(|s| s.to_string()).unwrap_or_default()
            }
        };
        if self.buildings.values().any(|b| b.name == name) {
            name = format!("{name} {}", self.taxi_companies().len() + 1);
        }
        let Some(company) = self.build_taxi_company(lot_id, name.clone()) else { return false };
        let tick = self.clock.tick;
        if let Some(f) = founder {
            let stake = (self.wealth_of(&self.citizens[f]) * 0.4).min(1500.0);
            match self.citizens[f].household.and_then(|h| self.households.get_mut(h)) {
                Some(h) => h.money -= stake,
                None => self.citizens[f].savings -= stake,
            }
            if let Some(z) = self.buildings[company].business.as_mut() {
                z.money += stake;
            }
            self.citizens[f].remember(tick, format!("Put my savings into a taxi firm: {name}. People here need cabs."));
        }
        self.taxis.latent_today = 0;
        self.taxis.latent_yesterday = 0;
        self.taxis.latent_sum = Vec2::ZERO;
        true
    }

    // ── the rider ──────────────────────────────────────────────────────────

    /// Where a citizen would wait for a cab and where it would stop: the
    /// street their building opens on (with car access), else the nearest
    /// street that is not a highway. `(segment, stop point, wait point)`.
    fn taxi_pickup(&self, id: CitizenId, from: Vec2) -> Option<(SegmentId, Vec2, Vec2)> {
        let c = self.citizens.get(id)?;
        if let Some(lot) = c.inside.and_then(|b| self.building_lot(b)) {
            if let Some(s) = lot.car_segment {
                return Some((s, lot.car_point, lot.entrance()));
            }
        }
        let (s, _, q) = self.roads.nearest_street(from)?;
        if self.roads.segment(s).is_highway() || q.distance(from) > 120.0 {
            return None;
        }
        Some((s, q, from + (q - from) * 0.85))
    }

    /// The nearest cab free for a fare near `p`, within reach: on duty and
    /// not busy (at a rank, at a kerb, heading to a rank or the yard, or
    /// standing in the yard while the rota wants more cabs out).
    fn nearest_free_cab(&self, p: Vec2) -> Option<(CabId, f32)> {
        self.taxis
            .cabs
            .iter()
            .filter(|(_, c)| c.on_duty && c.job.free())
            .filter(|(_, c)| self.taxi_company_works(c.company))
            .map(|(id, c)| (id, self.cab_pos(c).distance(p)))
            .filter(|(_, d)| *d <= taxi::PICKUP_REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
    }

    /// Where a cab is now.
    fn cab_pos(&self, c: &Cab) -> Vec2 {
        match c.place {
            CabPlace::Yard => self.building_lot(c.company).map_or(c.last_pos, |l| l.center),
            CabPlace::Rank { pos, .. } | CabPlace::Kerb { pos, .. } => pos,
            CabPlace::Road(car) => self.traffic.cars.get(car).map_or(c.last_pos, |x| x.pos),
        }
    }

    /// The taxi way to make a trip, priced like the other modes: the walk
    /// to the kerb, the wait for the nearest free cab, the ride, the fare,
    /// and the comfort of being driven (best at night, and for anyone
    /// drunk). `hypothetical` prices it as if a company existed (an
    /// average wait), for counting the demand nobody serves.
    pub(super) fn taxi_option(&self, id: CitizenId, q: &TripQuery, hypothetical: bool) -> Option<ModeOption> {
        if matches!(q.purpose, Purpose::Wander | Purpose::Sit | Purpose::ToStop | Purpose::Visit(LandUse::Park)) {
            return None;
        }
        let c = self.citizens.get(id)?;
        if c.stage() == crate::citizens::LifeStage::Child {
            return None;
        }
        let dist = q.from.distance(q.to);
        if dist < taxi::MIN_TRIP {
            return None;
        }
        let lot = self.building_lot(q.target)?;
        lot.car_segment?;
        let (segment, at, wait_at) = self.taxi_pickup(id, q.from)?;
        let secs = self.config.traffic_seconds_per_tick;
        let wait = if hypothetical {
            8.0
        } else {
            if self.taxis.cabs.is_empty() {
                return None;
            }
            // Riders already waiting with no cab sent take the free cabs
            // first (everyone deciding this minute sees the same free cab).
            let near = |p: Vec2| p.distance(at) <= taxi::PICKUP_REACH;
            let queued = self.taxis.requests.iter().filter(|r| r.cab.is_none() && near(r.at)).count();
            let free = self.taxis.cabs.values().filter(|x| x.on_duty && x.job.free() && near(self.cab_pos(x))).count();
            let wait = match self.nearest_free_cab(at) {
                Some((_, d)) if free > queued => d * 1.3 / (9.0 * secs) + 2.0,
                // Every cab is taken: a company within reach still takes the
                // booking, and the rider waits for a cab to come free, longer
                // the more riders each cab on duty already has queued.
                _ => {
                    let on_duty = self.taxis.cabs.values().filter(|x| x.on_duty && near(self.cab_pos(x))).count();
                    if on_duty == 0 {
                        return None;
                    }
                    8.0 + 15.0 * (queued + 1 - free.min(queued)) as f32 / on_duty as f32
                }
            };
            // Nobody waits half an hour for a cab: that rider is demand the
            // fleet could not meet (counted by `note_taxi_demand`).
            if wait > taxi::MAX_EXPECTED_WAIT {
                return None;
            }
            wait
        };
        let metres = dist * 1.3;
        let fare = taxi::fare(metres);
        let purse = c.savings + c.household.and_then(|h| self.households.get(h)).map_or(0.0, |h| h.money.max(0.0));
        if purse < fare * 2.0 {
            return None;
        }
        let walk = self.config.walk_speed.max(0.1);
        let minutes = wait_at.distance(q.from) / walk + wait + metres / (9.0 * secs) + 1.0;
        let [_, ambition, hedonism, _, _] = crate::vehicle::traits_of(c.seed);
        let frail = matches!(c.stage(), crate::citizens::LifeStage::Retiree);
        let drunk = c.drunk >= crate::citizens::DRUNK;
        let comfort = taxi::comfort(hedonism, drunk, taxi::late_night(self.clock.hour() as u8), frail);
        let time_w = 0.7 + 0.6 * ambition;
        let money_w = self.money_weight(id);
        let pain = c.commute.pain_of(TripMode::Taxi);
        let cost = minutes * time_w + fare * money_w + comfort + super::trips::PAIN_WEIGHT * pain;
        Some(ModeOption { mode: TripMode::Taxi, minutes, money: fare, comfort, pain, cost, plan: ModePlan::Taxi { segment, at, wait_at } })
    }

    /// A trip that chose something else because no cab was in reach, yet
    /// a cab would have beaten the best way it had: demand nobody served.
    pub(super) fn note_taxi_demand(&mut self, id: CitizenId, q: &TripQuery, ranked: &[ModeOption]) {
        if !q.allow_transit || ranked.iter().any(|o| o.mode == TripMode::Taxi) {
            return;
        }
        let Some(best) = ranked.first() else { return };
        if let Some(h) = self.taxi_option(id, q, true) {
            if h.cost < best.cost * 0.9 {
                // Near a company: a rider it had no cab for (it buys more).
                // Far from any: the demand a new company opens on.
                let near = self
                    .taxi_companies()
                    .into_iter()
                    .filter_map(|c| self.building_lot(c).map(|l| (c, l.center.distance(q.from))))
                    .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)))
                    .filter(|(_, d)| *d < taxi::PICKUP_REACH * 1.5);
                match near {
                    Some((c, _)) => self.taxis.note_missed(c),
                    None => {
                        self.taxis.latent_today += 1;
                        self.taxis.latent_sum += q.from;
                    }
                }
            }
        }
    }

    /// Walk to the kerb and wait; the dispatcher sends a cab.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn hail_taxi(&mut self, id: CitizenId, segment: SegmentId, at: Vec2, wait_at: Vec2, target: BuildingId, purpose: Purpose, fresh: bool) {
        self.walk_to_transit(id, wait_at, TransitLeg::Taxi, target, purpose, fresh);
        let tick = self.clock.tick;
        // Booked: no second thoughts until the cab comes or they give up.
        let c = &mut self.citizens[id];
        c.next_decision_tick = c.next_decision_tick.max(tick + taxi::WAIT_GIVE_UP * 3);
        self.taxis.requests.retain(|r| r.rider != id);
        self.taxis.requests.push(TaxiRequest { rider: id, segment, at, since: tick, cab: None });
    }

    fn on_taxi_leg(&self, c: CitizenId) -> bool {
        self.citizens.get(c).is_some_and(|x| matches!(x.transit.map(|t| t.leg), Some(TransitLeg::Taxi)))
    }

    fn waiting_for_taxi(&self, c: CitizenId) -> bool {
        self.on_taxi_leg(c) && self.citizens.get(c).is_some_and(|x| matches!(x.activity, Activity::Waiting { .. }))
    }

    // ── the tick ───────────────────────────────────────────────────────────

    pub(super) fn step_taxis(&mut self) {
        let tick = self.clock.tick;
        if self.clock.minute_of_day() == 0 {
            self.daily_taxis();
        }
        if self.taxis.cabs.is_empty() && self.taxis.requests.is_empty() {
            return;
        }
        self.track_cabs();
        self.serve_requests();
        self.board_waiting_riders();
        if tick % 5 == 3 {
            self.run_rota();
        }
        if tick % 60 == 17 {
            self.retire_lost_fleets();
        }
    }

    /// Cabs on the road: riders ride along; a cab whose car went (road
    /// works rebuilt the lanes) is put back in its yard, setting down
    /// anyone aboard where it was.
    fn track_cabs(&mut self) {
        let ids: Vec<CabId> = self.taxis.cabs.keys().collect();
        for id in ids {
            let cab = self.taxis.cabs[id];
            let CabPlace::Road(car) = cab.place else { continue };
            let alive = self.traffic.cars.get(car).is_some_and(|c| c.errand == Errand::Taxi(id));
            if !alive {
                if let CabJob::Fare { rider, .. } = cab.job {
                    self.set_down(rider, cab.last_pos, false);
                }
                let c = &mut self.taxis.cabs[id];
                c.place = CabPlace::Yard;
                c.job = CabJob::Idle;
                c.since = self.clock.tick;
                self.taxis.stats.cabs_recovered_total += 1;
                continue;
            }
            let (pos, heading) = {
                let c = &self.traffic.cars[car];
                (c.pos, c.heading)
            };
            self.taxis.cabs[id].last_pos = pos;
            if let CabJob::Fare { rider, .. } = cab.job {
                if let Some(c) = self.citizens.get_mut(rider).filter(|c| matches!(c.activity, Activity::Riding { bus } if bus == car)) {
                    c.pos = pos;
                    c.prev_pos = pos;
                    c.heading = heading;
                }
            }
        }
    }

    /// Requests: drop those whose rider went another way, send the
    /// nearest free cab to those without one, give up after the wait.
    fn serve_requests(&mut self) {
        let tick = self.clock.tick;
        let reqs = std::mem::take(&mut self.taxis.requests);
        let mut keep = Vec::with_capacity(reqs.len());
        for mut r in reqs {
            if !self.on_taxi_leg(r.rider) || self.taxis.cab_for(r.rider).is_some_and(|c| matches!(self.taxis.cabs[c].job, CabJob::Fare { .. })) {
                // Gone another way, or already aboard.
                if let Some(cab) = r.cab.filter(|c| self.taxis.cabs.get(*c).is_some_and(|x| x.job == CabJob::Pickup { rider: r.rider })) {
                    self.cab_free(cab);
                }
                continue;
            }
            // The cab sent was taken off the road or given up.
            if r.cab.is_some_and(|c| self.taxis.cabs.get(c).is_none_or(|x| x.job != CabJob::Pickup { rider: r.rider })) {
                r.cab = None;
            }
            if r.cab.is_none() {
                if let Some((cab, _)) = self.nearest_free_cab(r.at) {
                    if self.send_cab(cab, r.segment, r.at, CabJob::Pickup { rider: r.rider }) {
                        r.cab = Some(cab);
                    }
                }
            }
            // Waiting at the kerb too long: walk (or whatever is left).
            let waited = self.citizens.get(r.rider).and_then(|c| c.transit).map_or(0, |t| if t.since > 0 { tick.saturating_sub(t.since) } else { 0 });
            let limit = if r.cab.is_some() { taxi::WAIT_GIVE_UP * 2 } else { taxi::WAIT_GIVE_UP };
            if self.waiting_for_taxi(r.rider) && waited > limit {
                if let Some(cab) = r.cab {
                    self.cab_free(cab);
                }
                self.give_up_on_cab(r.rider, r.at);
                continue;
            }
            keep.push(r);
        }
        // Requests added meanwhile (none: hailing is not done from here).
        keep.append(&mut self.taxis.requests);
        self.taxis.requests = keep;
    }

    /// No cab came: remembered, counted against the nearest company, and
    /// the trip goes on another way.
    fn give_up_on_cab(&mut self, rider: CitizenId, at: Vec2) {
        let tick = self.clock.tick;
        self.taxis.stats.gave_up_today += 1;
        let near = self.taxi_companies().into_iter().min_by(|a, b| {
            let da = self.building_lot(*a).map_or(f32::MAX, |l| l.center.distance(at));
            let db = self.building_lot(*b).map_or(f32::MAX, |l| l.center.distance(at));
            da.total_cmp(&db).then(a.cmp(b))
        });
        if let Some(c) = near {
            self.taxis.note_missed(c);
        }
        let Some(trip) = self.citizens.get_mut(rider).and_then(|c| c.transit.take()) else { return };
        if let Some(c) = self.citizens.get_mut(rider) {
            c.thought = "No cab is coming. Walking it.".to_string();
            c.commute.feel(TripMode::Taxi, 2.5);
            c.remember(tick, "Stood at the kerb waiting for a cab that never came.");
            self.concerns.heard(rider, c, crate::concerns::Concern::Commute, 0.4);
        }
        self.travel_to_impl(rider, Some(trip.target), trip.purpose, false, false);
    }

    /// Cabs stopped at the kerb for a rider: in they get once the rider is
    /// there (they may still be walking to it).
    fn board_waiting_riders(&mut self) {
        let ids: Vec<CabId> = self.taxis.cabs.iter().filter(|(_, c)| matches!(c.place, CabPlace::Kerb { .. }) && matches!(c.job, CabJob::Pickup { .. })).map(|(id, _)| id).collect();
        for id in ids {
            let cab = self.taxis.cabs[id];
            let CabJob::Pickup { rider } = cab.job else { continue };
            if !self.on_taxi_leg(rider) {
                self.cab_free(id);
                continue;
            }
            let at = self.cab_pos(&cab);
            let here = self.citizens.get(rider).is_some_and(|c| matches!(c.activity, Activity::Waiting { .. }) && c.pos.distance(at) < 80.0);
            if here {
                self.board(id, rider);
            }
        }
    }

    /// The rider gets in and the cab drives to their destination's kerb.
    fn board(&mut self, id: CabId, rider: CitizenId) {
        let tick = self.clock.tick;
        let Some(trip) = self.citizens.get(rider).and_then(|c| c.transit) else { return };
        let Some((seg, pt)) = self.building_lot(trip.target).and_then(|l| l.car_segment.map(|s| (s, l.car_point))) else {
            self.cab_free(id);
            return;
        };
        let from = self.cab_pos(&self.taxis.cabs[id]);
        if !self.send_cab(id, seg, pt, CabJob::Fare { rider, from }) {
            // No way there by road: the rider walks after all.
            self.cab_free(id);
            self.give_up_on_cab(rider, from);
            return;
        }
        let CabPlace::Road(car) = self.taxis.cabs[id].place else { return };
        let waited = tick.saturating_sub(trip.since.max(1)) as f32;
        self.taxis.stats.wait_minutes_today += waited;
        let night = taxi::late_night(self.clock.hour() as u8);
        let company = self.taxis.cabs[id].company;
        let name = self.buildings.get(company).map(|b| b.name.clone()).unwrap_or_default();
        let c = &mut self.citizens[rider];
        let drunk = c.drunk >= crate::citizens::DRUNK;
        c.activity = Activity::Riding { bus: car };
        c.inside = None;
        if drunk || night {
            c.remember(tick, if drunk { format!("Too far gone to drive. Took a {name} cab home.") } else { format!("Late. Hailed a {name} cab.") });
        }
        if drunk {
            self.taxis.stats.drunk_rides_today += 1;
        }
        if night {
            self.taxis.stats.night_rides_today += 1;
        }
        self.taxis.requests.retain(|r| r.rider != rider);
    }

    /// A cab drives to `pt` on `seg` for `job`: from where it is parked
    /// (pulling out of its bay or off the kerb) or rerouted on the road.
    fn send_cab(&mut self, id: CabId, seg: SegmentId, pt: Vec2, job: CabJob) -> bool {
        let Some(cab) = self.taxis.cabs.get(id).copied() else { return false };
        let tick = self.clock.tick;
        match cab.place {
            CabPlace::Road(car) => {
                let Some((goal, s_end, _)) = self.traffic.net.nearest_lane_on(seg, pt) else { return false };
                if !self.traffic.cars.contains_key(car) || !self.traffic.redirect(car, goal, s_end) {
                    return false;
                }
            }
            parked => {
                let from = match parked {
                    CabPlace::Rank { segment, pos, .. } | CabPlace::Kerb { segment, pos, .. } => Some((segment, pos)),
                    _ => self.truck_stop(cab.company),
                };
                let Some((fseg, fpt)) = from else { return false };
                let Some((route, s0, s1)) = self.route_any_lane(fseg, fpt, seg, pt) else { return false };
                let car = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, cab.model.paint);
                let c = &mut self.traffic.cars[car];
                c.model = cab.model;
                c.errand = Errand::Taxi(id);
                // Drawn pulling out from the kerb or the gate.
                let off = parked.street_pose().map_or(2.0, |(p, _)| p.distance(c.pos).min(2.6));
                c.lateral = c.lateral.max(off);
                self.taxis.cabs[id].place = CabPlace::Road(car);
            }
        }
        let c = &mut self.taxis.cabs[id];
        c.job = job;
        c.since = tick;
        true
    }

    /// A cab reached the end of its drive.
    pub(super) fn cab_arrived(&mut self, car: CarId, id: CabId) {
        let Some(cab) = self.taxis.cabs.get(id).copied() else {
            // A cab with no record (should not happen): off the road.
            self.traffic.remove_car(car);
            return;
        };
        match cab.job {
            CabJob::Pickup { rider } => {
                self.park_cab_at_kerb(id, car);
                if self.waiting_for_taxi(rider) {
                    self.board_waiting_riders();
                }
            }
            CabJob::Fare { rider, from } => {
                let pos = self.traffic.cars.get(car).map_or(cab.last_pos, |c| c.pos);
                self.take_fare(id, rider, from.distance(pos) * 1.25);
                self.set_down(rider, pos, true);
                self.taxis.cabs[id].job = CabJob::Idle;
                // On to the next fare, a rank, or the yard; still on the road.
                if !self.cab_onwards(id) {
                    self.park_cab_at_kerb(id, car);
                }
            }
            CabJob::ToRank { rank } => self.park_cab_at_rank(id, car, rank),
            CabJob::ToYard => {
                self.traffic.remove_car(car);
                let c = &mut self.taxis.cabs[id];
                c.place = CabPlace::Yard;
                c.job = CabJob::Idle;
                c.since = self.clock.tick;
            }
            CabJob::Leaving => {
                self.traffic.remove_car(car);
                self.taxis.cabs.remove(id);
                self.taxis.stats.cabs_sold_total += 1;
            }
            CabJob::Idle => self.park_cab_at_kerb(id, car),
        }
    }

    /// The meter stops: the rider's household pays, the company takes it.
    fn take_fare(&mut self, id: CabId, rider: CitizenId, metres: f32) {
        let fare = taxi::fare(metres);
        let company = self.taxis.cabs[id].company;
        if let Some(c) = self.citizens.get_mut(rider) {
            match c.household.and_then(|h| self.households.get_mut(h)) {
                Some(h) => h.money -= fare,
                None => c.savings -= fare,
            }
            c.commute.fee += fare;
        }
        if let Some(z) = self.buildings.get_mut(company).and_then(|b| b.business.as_mut()) {
            z.money += fare;
            z.revenue_today += fare;
            z.deals += 1;
        }
        let s = &mut self.taxis.stats;
        s.rides_today += 1;
        s.fares_today += fare;
        s.fare_metres_today += metres;
        self.taxis.rides_by_company.entry(company).or_default().0 += 1;
    }

    /// Out of the cab at `at`: the rider walks the rest of the way (or,
    /// when the cab never got there, is left free to decide again).
    fn set_down(&mut self, rider: CitizenId, at: Vec2, arrived: bool) {
        let tick = self.clock.tick;
        let trip = self.citizens.get_mut(rider).and_then(|c| c.transit.take());
        let entrance = trip.and_then(|t| self.building_entrance(t.target));
        let Some(c) = self.citizens.get(rider) else { return };
        if !matches!(c.activity, Activity::Riding { .. } | Activity::Waiting { .. }) {
            return;
        }
        match (trip, entrance, arrived) {
            (Some(t), Some(e), true) => {
                let mut path = match self.walk.route(at, e) {
                    Some((pts, _)) if at.distance(e) > 3.0 => pts,
                    _ => Vec::new(),
                };
                path.push(e);
                let x = &mut self.citizens[rider];
                x.pos = at;
                x.prev_pos = at;
                x.inside = None;
                x.mode = TravelMode::Walk;
                x.activity = Activity::Traveling { to: Some(t.target), purpose: t.purpose, path, idx: 0 };
            }
            _ => {
                let x = &mut self.citizens[rider];
                x.activity = Activity::Idle;
                x.inside = None;
                x.pos = at;
                x.prev_pos = at;
                x.next_decision_tick = tick + 1;
            }
        }
    }

    /// A free cab on the road decides where to go: a rider waiting nearby,
    /// else a rank with room, else (off duty, or every rank full) the
    /// yard. False when it can go nowhere from here.
    fn cab_onwards(&mut self, id: CabId) -> bool {
        let Some(cab) = self.taxis.cabs.get(id).copied() else { return false };
        // A rider still without a cab: take them first.
        if cab.on_duty {
            let pos = self.cab_pos(&cab);
            let waiting = self.taxis.requests.iter().filter(|r| r.cab.is_none() && r.at.distance(pos) <= taxi::PICKUP_REACH).min_by(|a, b| a.at.distance(pos).total_cmp(&b.at.distance(pos))).copied();
            if let Some(r) = waiting {
                if self.send_cab(id, r.segment, r.at, CabJob::Pickup { rider: r.rider }) {
                    if let Some(q) = self.taxis.requests.iter_mut().find(|q| q.rider == r.rider) {
                        q.cab = Some(id);
                    }
                    return true;
                }
            }
            if let Some(rank) = self.pick_rank(id) {
                if self.send_cab(id, rank.segment, rank.at, CabJob::ToRank { rank: rank.building }) {
                    return true;
                }
            }
        }
        let Some((seg, pt)) = self.truck_stop(cab.company) else { return false };
        self.send_cab(id, seg, pt, CabJob::ToYard)
    }

    /// A cab that was sent somewhere is free again where it is.
    fn cab_free(&mut self, id: CabId) {
        let Some(cab) = self.taxis.cabs.get(id).copied() else { return };
        self.taxis.cabs[id].job = CabJob::Idle;
        if matches!(cab.place, CabPlace::Road(_)) && !self.cab_onwards(id) {
            if let CabPlace::Road(car) = cab.place {
                self.park_cab_at_kerb(id, car);
            }
        }
    }

    /// Where a car stands, pulled over to the kerb of its street.
    fn cab_kerb_pose(&self, car: CarId) -> Option<(SegmentId, Vec2, f32)> {
        let c = self.traffic.cars.get(car)?;
        let lane = c.route.get(c.idx).and_then(|l| self.traffic.net.lanes.get(*l as usize));
        let seg = match lane.map(|l| &l.kind) {
            Some(LaneKind::Road { segment, .. }) => *segment,
            _ => self.roads.nearest_street(c.pos)?.0,
        };
        let dir = Vec2::new(c.heading.cos(), c.heading.sin());
        let kerb = lane.is_some_and(|l| l.right.is_none());
        let pos = c.pos + Vec2::new(-dir.y, dir.x) * if kerb { crate::roads::LANE_WIDTH * 0.6 } else { 0.0 };
        Some((seg, pos, c.heading))
    }

    fn park_cab_at_kerb(&mut self, id: CabId, car: CarId) {
        let pose = self.cab_kerb_pose(car);
        self.traffic.remove_car(car);
        let tick = self.clock.tick;
        let c = &mut self.taxis.cabs[id];
        c.since = tick;
        c.place = match pose {
            Some((segment, pos, heading)) => {
                c.last_pos = pos;
                CabPlace::Kerb { segment, pos, heading }
            }
            None => CabPlace::Yard,
        };
    }

    /// Into the rank's queue: nose to tail behind the cabs already there.
    fn park_cab_at_rank(&mut self, id: CabId, car: CarId, rank: BuildingId) {
        let taken: Vec<u8> = self.taxis.cabs.values().filter_map(|c| match c.place {
            CabPlace::Rank { rank: r, slot, .. } if r == rank => Some(slot),
            _ => None,
        }).collect();
        let Some(slot) = (0..taxi::RANK_SLOTS as u8).find(|s| !taken.contains(s)) else {
            // Full: wait at the kerb here instead.
            self.park_cab_at_kerb(id, car);
            return;
        };
        let Some((segment, head, heading)) = self.cab_kerb_pose(car) else {
            self.park_cab_at_kerb(id, car);
            return;
        };
        let back = Vec2::new(heading.cos(), heading.sin()) * (-6.5 * slot as f32);
        self.traffic.remove_car(car);
        let tick = self.clock.tick;
        let c = &mut self.taxis.cabs[id];
        c.place = CabPlace::Rank { rank, slot, segment, pos: head + back, heading };
        c.job = CabJob::Idle;
        c.since = tick;
        c.last_pos = head + back;
    }

    /// Ranks cabs wait at: the stations' forecourts, venues that are open
    /// (the evening crowd), and every company's own gate.
    fn ranks(&self) -> Vec<Rank> {
        let hour = self.clock.hour() as u8;
        let mut out: Vec<Rank> = Vec::new();
        let mut push = |b: BuildingId, w: &World| {
            if let Some((segment, at)) = w.truck_stop(b) {
                if w.building_lot(b).is_some_and(|l| l.car_segment.is_some()) {
                    out.push(Rank { building: b, segment, at });
                }
            }
        };
        for &s in &self.stations {
            push(s, self);
        }
        let mut venues: Vec<(BuildingId, u16)> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Venue && !z.closed) && b.construction.is_none() && b.serves_at(hour))
            .map(|(id, b)| (id, b.visitors))
            .collect();
        venues.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        for (v, _) in venues.into_iter().take(6) {
            push(v, self);
        }
        for c in self.taxi_companies() {
            push(c, self);
        }
        out
    }

    /// The rank a free cab should wait at: the nearest with room, a busy
    /// rank (station, venue) preferred over a company gate.
    fn pick_rank(&self, id: CabId) -> Option<Rank> {
        let cab = self.taxis.cabs.get(id)?;
        let pos = self.cab_pos(cab);
        let home = self.building_lot(cab.company).map_or(pos, |l| l.center);
        self.ranks()
            .into_iter()
            .filter(|r| self.taxis.at_rank(r.building) < taxi::RANK_SLOTS)
            .filter(|r| r.at.distance(home) <= taxi::PICKUP_REACH)
            .filter(|r| !self.taxi_company_works(r.building) || r.building == cab.company)
            .map(|r| {
                let gate = if self.taxi_company_works(r.building) { 400.0 } else { 0.0 };
                (r, r.at.distance(pos) + gate + 150.0 * self.taxis.at_rank(r.building) as f32)
            })
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.building.cmp(&b.0.building)))
            .map(|(r, _)| r)
    }

    /// The rota: as many cabs on duty as the hour wants, off-duty ones
    /// home to the yard; free cabs on duty go out to the ranks.
    fn run_rota(&mut self) {
        let hour = self.clock.hour() as u8;
        for company in self.taxi_companies() {
            let fleet: Vec<CabId> = self.taxis.fleet(company).filter(|(_, c)| c.job != CabJob::Leaving).map(|(id, _)| id).collect();
            let want = ((fleet.len() as f32 * taxi::on_duty_share(hour)).ceil() as usize).clamp(1, fleet.len().max(1));
            let mut on: Vec<CabId> = fleet.iter().copied().filter(|id| self.taxis.cabs[*id].on_duty).collect();
            // Off duty: the free ones longest out first; busy ones finish their fare.
            while on.len() > want {
                let Some(pos) = on.iter().rposition(|id| self.taxis.cabs[*id].job.free() || self.taxis.cabs[*id].job == CabJob::Idle) else { break };
                let id = on.remove(pos);
                self.taxis.cabs[id].on_duty = false;
            }
            for id in fleet.iter().copied() {
                if on.len() >= want {
                    break;
                }
                if !self.taxis.cabs[id].on_duty {
                    self.taxis.cabs[id].on_duty = true;
                    on.push(id);
                }
            }
            for id in fleet {
                let cab = self.taxis.cabs[id];
                match (cab.on_duty, cab.place, cab.job) {
                    // Off duty and free: back to the yard.
                    (false, CabPlace::Rank { .. } | CabPlace::Kerb { .. }, CabJob::Idle) | (false, CabPlace::Road(_), CabJob::Idle | CabJob::ToRank { .. }) => {
                        if let Some((seg, pt)) = self.truck_stop(company) {
                            self.send_cab(id, seg, pt, CabJob::ToYard);
                        }
                    }
                    // On duty and idle in the yard or at a kerb: out to a rank.
                    (true, CabPlace::Yard | CabPlace::Kerb { .. }, CabJob::Idle) => match self.pick_rank(id) {
                        Some(rank) => {
                            if rank.building != company || !matches!(cab.place, CabPlace::Yard) {
                                self.send_cab(id, rank.segment, rank.at, CabJob::ToRank { rank: rank.building });
                            }
                        }
                        // Every rank full: wait in the yard, still on call.
                        None if matches!(cab.place, CabPlace::Kerb { .. }) => {
                            if let Some((seg, pt)) = self.truck_stop(company) {
                                self.send_cab(id, seg, pt, CabJob::ToYard);
                            }
                        }
                        None => {}
                    },
                    _ => {}
                }
            }
        }
    }

    /// Fleets whose company closed or went: sold, driven out of town (a
    /// rider aboard is set down first). Nothing just vanishes.
    fn retire_lost_fleets(&mut self) {
        let lost: Vec<CabId> = self.taxis.cabs.iter().filter(|(_, c)| c.job != CabJob::Leaving && !self.taxi_company_works(c.company)).map(|(id, _)| id).collect();
        for id in lost {
            self.sell_cab(id);
        }
    }

    /// Sell a cab: it drives off the map and its record goes when it is
    /// gone. With no road off the map it is sold where it stands.
    fn sell_cab(&mut self, id: CabId) {
        let Some(cab) = self.taxis.cabs.get(id).copied() else { return };
        if let CabJob::Fare { rider, .. } = cab.job {
            self.set_down(rider, self.cab_pos(&cab), false);
        }
        let car = match cab.place {
            CabPlace::Road(car) if self.traffic.cars.contains_key(car) => Some(car),
            parked => {
                let from = match parked {
                    CabPlace::Rank { segment, pos, .. } | CabPlace::Kerb { segment, pos, .. } => Some((segment, pos)),
                    _ => self.truck_stop(cab.company).or_else(|| self.roads.nearest_street(cab.last_pos).map(|(s, _, q)| (s, q))),
                };
                from.and_then(|(fseg, fpt)| {
                    let (start, s0, _) = self.traffic.net.nearest_lane_on(fseg, fpt)?;
                    let (route, s0, s1) = self.edge_exits(fpt).into_iter().find_map(|(goal, s_end)| self.traffic.net.route_lanes(start, s0, goal, s_end))?;
                    let car = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, cab.model.paint);
                    self.traffic.cars[car].model = cab.model;
                    Some(car)
                })
            }
        };
        match car {
            Some(car) if matches!(cab.place, CabPlace::Road(_)) && !self.redirect_out(car) => {
                self.traffic.remove_car(car);
                self.taxis.cabs.remove(id);
                self.taxis.stats.cabs_sold_total += 1;
            }
            Some(car) => {
                self.traffic.cars[car].errand = Errand::Taxi(id);
                let c = &mut self.taxis.cabs[id];
                c.place = CabPlace::Road(car);
                c.job = CabJob::Leaving;
                c.on_duty = false;
            }
            None => {
                self.taxis.cabs.remove(id);
                self.taxis.stats.cabs_sold_total += 1;
            }
        }
    }

    /// Midnight: the day's numbers roll over; a company that left riders
    /// waiting buys a cab if it can, one with cabs standing idle sells one.
    fn daily_taxis(&mut self) {
        let t = &mut self.taxis;
        t.stats.rides_yesterday = std::mem::take(&mut t.stats.rides_today);
        t.stats.fares_yesterday = std::mem::take(&mut t.stats.fares_today);
        t.stats.gave_up_yesterday = std::mem::take(&mut t.stats.gave_up_today);
        t.stats.drunk_rides_today = 0;
        t.stats.night_rides_today = 0;
        t.stats.wait_minutes_today = 0.0;
        t.stats.fare_metres_today = 0.0;
        t.latent_yesterday = std::mem::take(&mut t.latent_today);
        t.latent_sum = Vec2::ZERO;
        for v in t.missed.values_mut() {
            *v = (0, v.0);
        }
        for v in t.rides_by_company.values_mut() {
            *v = (0, v.0);
        }
        let companies = self.taxi_companies();
        self.taxis.missed.retain(|b, _| companies.contains(b));
        self.taxis.rides_by_company.retain(|b, _| companies.contains(b));
        for company in companies {
            let fleet = self.taxis.fleet_size(company);
            let missed = self.taxis.missed.get(&company).map_or(0, |m| m.1);
            let rides = self.taxis.rides_by_company.get(&company).map_or(0, |m| m.1);
            let money = self.buildings[company].business.as_ref().map_or(0.0, |z| z.money);
            if missed >= taxi::GROW_MISSED && fleet < taxi::MAX_CABS && money >= taxi::CAB_PRICE + 150.0 {
                // One cab for every few riders left waiting, as many as the
                // books allow, a few a night.
                let want = (missed.div_ceil(taxi::MISSED_PER_CAB) as usize).clamp(1, taxi::MAX_BUY_PER_DAY).min(taxi::MAX_CABS - fleet);
                let afford = ((money - 150.0) / taxi::CAB_PRICE).floor().max(0.0) as usize;
                for _ in 0..want.min(afford).max(1) {
                    if let Some(z) = self.buildings[company].business.as_mut() {
                        z.money -= taxi::CAB_PRICE;
                        z.spend_today += taxi::CAB_PRICE;
                    }
                    self.add_cab(company);
                    self.taxis.stats.cabs_bought_total += 1;
                }
            } else if missed == 0 && fleet > taxi::START_CABS && (rides as f32) < fleet as f32 * 1.5 {
                // The idlest cab in the yard goes; half its price comes back.
                if let Some(id) = self.taxis.fleet(company).filter(|(_, c)| c.place == CabPlace::Yard && c.job == CabJob::Idle).map(|(id, _)| id).last() {
                    if let Some(z) = self.buildings[company].business.as_mut() {
                        z.money += taxi::CAB_PRICE * 0.5;
                    }
                    self.sell_cab(id);
                }
            }
        }
    }

    /// The parked cabs drawn on the street (at ranks and kerbs): pose and
    /// model, for the renderer.
    pub fn parked_cabs(&self) -> impl Iterator<Item = (CabId, Vec2, f32, crate::vehicle::VehicleModel)> + '_ {
        self.taxis.cabs.iter().filter_map(|(id, c)| c.place.street_pose().map(|(p, h)| (id, p, h, c.model)))
    }

    /// Cabs standing in `company`'s yard, in fleet-number order.
    pub fn cabs_in_yard(&self, company: BuildingId) -> Vec<(CabId, crate::vehicle::VehicleModel)> {
        let mut v: Vec<(u16, CabId, crate::vehicle::VehicleModel)> = self.taxis.fleet(company).filter(|(_, c)| c.place == CabPlace::Yard).map(|(id, c)| (c.number, id, c.model)).collect();
        v.sort_by_key(|x| x.0);
        v.into_iter().map(|(_, id, m)| (id, m)).collect()
    }
}
