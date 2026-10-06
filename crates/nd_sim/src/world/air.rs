//! Air transport, the systems (`crate::air` holds the data): pads from the
//! buildings, rooftop pads on dear towers, the taxi fleet and its
//! dispatcher, the rider's side of a drone trip (`consider_air`, called from
//! the mode choice in `trips.rs`), flight (climb, cruise, hold, land),
//! batteries, air ambulances, the police helicopter, noise and grumbling.
//!
//! Everything here draws from its own random stream (`arng`), so a city
//! without a vertiport runs exactly as it did before air transport.

use super::*;
use crate::air::{self, Craft, Drone, DroneId, Job, Pad, PadKind, Phase};
use crate::land_value::LandValueBuilder;
use crate::services::{IncidentKind, ServiceKind, Stage};

/// A drone trip the mode choice settled on.
#[derive(Clone, Copy, Debug)]
pub(super) struct AirPlan {
    pub from: BuildingId,
    pub to: BuildingId,
    pub from_pos: Vec2,
}

/// Air ambulances: is this patient critical? The old, and a share of the
/// rest by the incident number (the sim has no triage).
fn critical(age: u8, incident: u32) -> bool {
    age >= 65 || incident % 3 == 0
}

impl World {
    fn arng(&mut self) -> &mut SimRng {
        let (seed, tick) = (self.config.seed, self.clock.tick);
        self.air.rng.get_or_insert_with(|| crate::rng_for(seed ^ 0xA1_2D_0E, tick))
    }

    fn air_service_on(&self) -> bool {
        let m = self.clock.minute_of_day() as u32;
        !(air::SERVICE_END..air::SERVICE_START).contains(&m)
    }

    // ── pads and fleets ────────────────────────────────────────────────────

    /// What the pads depend on: the stations with helipads, the vertiports
    /// and the rooftop list.
    fn pad_signature(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for (id, b) in self.buildings.iter() {
            if let Some(s) = b.service {
                (id, s.kind as u8, s.tier as u8, b.construction.is_some(), b.demolition.is_some(), b.is_derelict(), b.lot).hash(&mut h);
            }
        }
        self.air.rooftops.hash(&mut h);
        h.finish()
    }

    /// Rebuild the pads from the buildings and match the fleets to them.
    pub fn refresh_pads(&mut self) {
        let mut pads: Vec<Pad> = Vec::new();
        for (id, b) in self.buildings.iter() {
            if b.construction.is_some() || b.demolition.is_some() || b.is_derelict() {
                continue;
            }
            let Some(lot) = self.lots.get(b.lot) else { continue };
            let pad = match b.service {
                Some(s) if s.kind == ServiceKind::Vertiport => Some((PadKind::Vertiport, air::vertiport_deck(s.tier), air::vertiport_slots(s.tier))),
                Some(s) if s.tier == Tier::High && s.kind == ServiceKind::Health => Some((PadKind::Hospital, b.floors as f32 * 4.2 + 0.5, 2)),
                Some(s) if s.tier == Tier::High && s.kind == ServiceKind::Fire => Some((PadKind::Fire, b.floors as f32 * 4.5 + 0.5, 1)),
                Some(s) if s.tier == Tier::High && s.kind == ServiceKind::Police => Some((PadKind::Police, b.floors as f32 * 4.2 + 0.5, 1)),
                None if self.air.rooftops.contains(&id) => Some((PadKind::Rooftop, b.floors as f32 * 3.3 + 2.0, 1)),
                _ => None,
            };
            if let Some((kind, height, slots)) = pad {
                pads.push(Pad { building: id, kind, pos: lot.center, height, slots });
            }
        }
        self.air.pads = pads;
        self.air.pads_key = self.pad_signature();
        // Aircraft whose base is gone: anyone aboard is set down where it is.
        let lost: Vec<DroneId> = self.air.drones.iter().filter(|(_, d)| !self.air.pads.iter().any(|p| p.building == d.base)).map(|(id, _)| id).collect();
        for id in lost {
            self.scrap_drone(id);
        }
        // Fleets: vertiports keep one taxi per slot but one; the big
        // hospitals and the fire HQ an air ambulance; the police HQ a helicopter.
        let pads = self.air.pads.clone();
        for pad in &pads {
            let (craft, want) = match pad.kind {
                PadKind::Vertiport => (Craft::Taxi, pad.slots.saturating_sub(1).max(1) as usize),
                PadKind::Hospital | PadKind::Fire => (Craft::AirAmbulance, 1),
                PadKind::Police => (Craft::PoliceHeli, 1),
                PadKind::Rooftop => continue,
            };
            let have = self.air.drones.values().filter(|d| d.base == pad.building).count();
            for k in have..want {
                let mut d = Drone::new(craft, k as u16 + 1, pad.building, pad);
                match self.air.free_slot(pad.building) {
                    Some(s) => d.slot = s,
                    None => {
                        // Every slot is taken by visitors: start in the air above it.
                        d.at = None;
                        d.alt = craft.cruise();
                        d.prev_alt = d.alt;
                        d.phase = Phase::Cruising;
                        d.job = Job::Home;
                    }
                }
                self.air.drones.insert(d);
            }
        }
        // A pad that went away cannot hold anybody's slot.
        let ids: Vec<DroneId> = self.air.drones.keys().collect();
        for id in ids {
            let lost_slot = self.air.drones[id].at.is_some_and(|b| self.air.pad(b).is_none());
            let lost_dest = self.air.drones[id].dest.is_some_and(|b| self.air.pad(b).is_none());
            if lost_slot || lost_dest {
                let d = &mut self.air.drones[id];
                d.at = None;
                if d.phase.airborne() || lost_dest {
                    self.air_go_home(id);
                }
            }
        }
    }

    /// Take an aircraft out of service: a passenger is set down where it is.
    fn scrap_drone(&mut self, id: DroneId) {
        let Some(d) = self.air.drones.remove(id) else { return };
        if let Some(c) = d.aboard() {
            self.strand_flyer(c, d.pos);
        }
        if let Job::Medic { incident, .. } = d.job {
            // [services] On scene too: the air crew's work never runs down
            // (`work_left` is f32::MAX while they have it), so a patient left
            // OnScene was pinned for ever. Back in the queue with the ground
            // crew's time (`requeue`).
            if self.services.incident(incident).is_some_and(|i| matches!(i.stage, Stage::Dispatched | Stage::OnScene | Stage::Transporting)) {
                self.requeue(incident);
            }
        }
    }

    /// Someone in the air when their aircraft went away: on the ground
    /// where it was, free to decide again.
    fn strand_flyer(&mut self, c: CitizenId, at: Vec2) {
        let tick = self.clock.tick;
        if let Some(c) = self.citizens.get_mut(c) {
            c.activity = Activity::Idle;
            c.transit = None;
            c.inside = None;
            c.pos = at;
            c.prev_pos = at;
            c.next_decision_tick = tick + 1;
        }
    }

    /// Towers of the high tier on very dear land get a rooftop pad; they
    /// keep it while the land stays dear. Each change redraws the tower.
    pub fn refresh_rooftops(&mut self) {
        // Demand: rich adults living in each building, who would fly.
        let mut rich: HashMap<BuildingId, f32> = HashMap::new();
        for c in self.citizens.values() {
            if c.class() == Tier::High && c.stage() != crate::citizens::LifeStage::Child {
                if let Some(h) = c.home {
                    *rich.entry(h).or_default() += 1.0;
                }
            }
        }
        let homes: Vec<(Vec2, f32)> = rich.iter().filter_map(|(b, n)| self.building_lot(*b).map(|l| (l.center, *n))).collect();
        let mut towers: Vec<(BuildingId, f32, bool)> = Vec::new();
        for (id, b) in self.buildings.iter() {
            let tower = matches!(b.land_use, LandUse::Residential | LandUse::Commercial)
                && b.tier == Tier::High
                && b.floors >= air::ROOFTOP_MIN_FLOORS
                && b.construction.is_none()
                && b.demolition.is_none()
                && b.service.is_none()
                && b.plant.is_none()
                && !b.depot
                && b.rail.is_none()
                && b.port.is_none()
                && !b.is_parking()
                && !b.is_derelict();
            if !tower {
                continue;
            }
            let Some(lot) = self.lots.get(b.lot) else { continue };
            let value = self.land_value.at(lot.center);
            let had = self.air.rooftops.contains(&id);
            let dear = value >= air::ROOFTOP_LAND_VALUE || (had && value >= air::ROOFTOP_KEEP);
            // Its own rich residents count in full, the neighbours' at half.
            let own = rich.get(&id).copied().unwrap_or(0.0);
            let near: f32 = homes.iter().filter(|(p, _)| p.distance(lot.center) <= air::ROOFTOP_DEMAND_REACH).map(|(_, n)| *n).sum::<f32>() - own;
            let demand = own + 0.5 * near;
            towers.push((id, if dear { demand } else { -1.0 }, had));
        }
        // Rare: the richest towers only, about one in nine, a pad once
        // granted kept while demand holds at 60% of the bar.
        let cap = ((towers.len() as f32 / air::ROOFTOP_TOWERS_PER_PAD).round() as usize).max(2);
        let mut ranked: Vec<(BuildingId, f32, bool)> = towers.into_iter().filter(|(_, d, had)| *d >= air::ROOFTOP_DEMAND * if *had { 0.6 } else { 1.0 }).map(|(id, d, had)| (id, if had { d * 1.25 } else { d }, had)).collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut next: Vec<BuildingId> = ranked.into_iter().take(cap).map(|t| t.0).collect();
        next.sort();
        if next == self.air.rooftops {
            return;
        }
        for &b in next.iter().filter(|b| !self.air.rooftops.contains(b)) {
            self.events.push(SimEvent::BuildingChanged(b));
        }
        let gone: Vec<BuildingId> = self.air.rooftops.iter().copied().filter(|b| !next.contains(b)).collect();
        for b in gone {
            if self.buildings.contains_key(b) {
                self.events.push(SimEvent::BuildingChanged(b));
            }
        }
        self.air.rooftops = next;
        self.refresh_pads();
    }

    // ── the rider ──────────────────────────────────────────────────────────

    /// The mode choice's air branch: a rich adult on a long trip flies
    /// when a taxi pad is within walking reach of both ends, the fare is in
    /// the purse and the flight beats the road.
    pub(super) fn consider_air(&mut self, id: CitizenId, target: BuildingId, purpose: Purpose, from: Vec2, to: Vec2) -> Option<AirPlan> {
        if self.air.taxis() == 0 || !self.air_service_on() || matches!(purpose, Purpose::Wander | Purpose::Sit | Purpose::ToStop) {
            return None;
        }
        let c = self.citizens.get(id)?;
        if c.class() != Tier::High || c.stage() == crate::citizens::LifeStage::Child {
            return None;
        }
        let dist = from.distance(to);
        if dist < air::MIN_TRIP {
            return None;
        }
        let near = |p: Vec2| self.air.pads.iter().filter(|q| q.kind.for_taxis()).map(|q| (q, q.pos.distance(p))).filter(|(_, d)| *d <= air::REACH).min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.building.cmp(&b.0.building)));
        let target_pad = self.air.pads.iter().find(|q| q.building == target && q.kind.for_taxis()).map(|q| (q, 0.0));
        let (a, da) = near(from)?;
        let (b, db) = target_pad.or_else(|| near(to))?;
        if a.building == b.building {
            return None;
        }
        let metres = a.pos.distance(b.pos);
        let fare = air::fare(metres);
        let purse = c.savings + c.household.and_then(|h| self.households.get(h)).map_or(0.0, |h| h.money.max(0.0));
        if purse < fare {
            return None;
        }
        let tsp = self.config.traffic_seconds_per_tick;
        // Door to door against a city drive (7 m/s on average, a detour,
        // finding a space); the rich pay for the view, so a little slower is fine.
        let fly = (da + db) / self.config.walk_speed.max(0.1) + air::flight_ticks(metres, a.height, b.height, air::TAXI_ALT, air::TAXI_MPS, tsp) + 2.0;
        let road = dist * 1.35 / (7.0 * tsp) + 6.0;
        if fly > road * 1.25 {
            return None;
        }
        let plan = AirPlan { from: a.building, to: b.building, from_pos: a.pos };
        // Most of the rich take it; some like their own car.
        (self.arng().random::<f32>() < 0.85).then_some(plan)
    }

    /// Set off for the pad; the dispatcher sends a drone to meet them.
    pub(super) fn start_air_trip(&mut self, id: CitizenId, plan: AirPlan, target: BuildingId, purpose: Purpose, fresh: bool) {
        self.walk_to_transit(id, plan.from_pos, TransitLeg::Air { from: plan.from, to: plan.to }, target, purpose, fresh);
        if !self.air.pending.contains(&id) {
            self.air.pending.push(id);
        }
    }

    /// The rider's air leg, if they are on one.
    fn air_leg(&self, c: CitizenId) -> Option<(BuildingId, BuildingId)> {
        match self.citizens.get(c)?.transit?.leg {
            TransitLeg::Air { from, to } => Some((from, to)),
            _ => None,
        }
    }

    /// Is `c` standing at pad `pad` waiting for their drone?
    fn waiting_at(&self, c: CitizenId, pad: BuildingId) -> bool {
        self.citizens.get(c).is_some_and(|x| matches!(x.activity, Activity::Waiting { .. })) && self.air_leg(c).is_some_and(|(from, _)| from == pad)
    }

    /// Off the drone at its pad: walk the rest of the way.
    fn walk_from_pad(&mut self, c: CitizenId, at: Vec2) {
        let Some(trip) = self.citizens.get_mut(c).and_then(|x| x.transit.take()) else { return };
        let Some(entrance) = self.building_entrance(trip.target) else {
            self.strand_flyer(c, at);
            return;
        };
        let mut path = match self.walk.route(at, entrance) {
            Some((pts, _)) if at.distance(entrance) > 3.0 => pts,
            _ => Vec::new(),
        };
        path.push(entrance);
        let x = &mut self.citizens[c];
        x.pos = at;
        x.prev_pos = at;
        x.inside = None;
        x.mode = TravelMode::Walk;
        x.activity = Activity::Traveling { to: Some(trip.target), purpose: trip.purpose, path, idx: 0 };
    }

    // ── the tick ───────────────────────────────────────────────────────────

    pub(super) fn step_air(&mut self) {
        let tick = self.clock.tick;
        if self.clock.minute_of_day() == 0 {
            self.daily_air();
        }
        if tick % 120 == 100 {
            self.refresh_rooftops();
        }
        if tick % 10 == 4 || (self.air.pads.is_empty() && self.air.pads_key == 0) {
            if self.air.pads_key != self.pad_signature() {
                self.refresh_pads();
            }
        }
        if self.air.drones.is_empty() && self.air.pending.is_empty() {
            return;
        }
        self.dispatch_air_medics();
        if tick % 5 == 2 {
            self.dispatch_police_heli();
        }
        if tick % 10 == 6 {
            // Riders whose drone gave up on them queue again.
            let orphans: Vec<CitizenId> = self.citizens.iter().filter(|(cid, c)| matches!(c.transit.map(|t| t.leg), Some(TransitLeg::Air { .. })) && !self.air.pending.contains(cid) && self.air.drone_for(*cid).is_none()).map(|(cid, _)| cid).collect();
            self.air.pending.extend(orphans);
        }
        self.dispatch_taxis();
        let ids: Vec<DroneId> = self.air.drones.keys().collect();
        for id in ids {
            self.fly(id);
        }
        if tick % 15 == 9 {
            self.grumble();
        }
    }

    fn daily_air(&mut self) {
        let a = &mut self.air;
        a.stats.flights_yesterday = std::mem::take(&mut a.stats.flights_today);
        a.stats.riders_yesterday = std::mem::take(&mut a.stats.riders_today);
        a.stats.fares_yesterday = std::mem::take(&mut a.stats.fares_today);
        a.stats.gave_up_today = 0;
        a.stats.holds_today = 0;
        a.stats.medic_flights_today = 0;
        a.stats.police_flights_today = 0;
        a.stats.grumbles_today = 0;
        let taxis = a.drones.values().filter(|d| d.craft == Craft::Taxi).count() as f32;
        let costs = taxis * air::DRONE_COST_PER_DAY;
        a.company.money += a.company.revenue_today - costs;
        a.company.revenue_yesterday = std::mem::take(&mut a.company.revenue_today);
        a.company.costs_today = costs;
        for d in a.drones.values_mut() {
            d.flights_today = 0;
            d.km_today = 0.0;
        }
        for n in a.noise.values_mut() {
            *n *= 0.5;
        }
        a.noise.retain(|_, n| *n > 0.5);
        a.grumbled.clear();
    }

    // ── dispatch ───────────────────────────────────────────────────────────

    /// Battery a taxi needs to fetch a rider from pad `from`, fly them to
    /// `to` and get home, with a climb at each take-off.
    fn battery_needed(&self, d: &Drone, from: Vec2, to: Vec2) -> f32 {
        let home = self.air.pad(d.base).map_or(to, |p| p.pos);
        let km = (d.pos.distance(from) + from.distance(to) + to.distance(home)) / 1000.0;
        km * air::BATTERY_PER_KM + 3.0 * air::TAXI_ALT * air::BATTERY_PER_CLIMB_M
    }

    /// Riders without a drone get the nearest free one with the charge for
    /// the job; riders who waited too long give up and go by road.
    fn dispatch_taxis(&mut self) {
        let tick = self.clock.tick;
        let pending = std::mem::take(&mut self.air.pending);
        let mut keep: Vec<CitizenId> = Vec::new();
        for c in pending {
            let Some((from, to)) = self.air_leg(c) else { continue };
            if self.air.drone_for(c).is_some() {
                continue;
            }
            let (Some(a), Some(b)) = (self.air.pad(from).copied(), self.air.pad(to).copied()) else {
                self.give_up_air(c, "The pad's shut. Taking the car.");
                continue;
            };
            let best = self
                .air
                .drones
                .iter()
                .filter(|(_, d)| d.craft == Craft::Taxi && matches!(d.job, Job::Idle | Job::Home) && !(d.phase == Phase::Parked && d.until > tick))
                .filter(|(_, d)| d.battery - self.battery_needed(d, a.pos, b.pos) >= air::RESERVE * 0.5)
                .map(|(id, d)| (d.pos.distance(a.pos), id))
                .min_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
            match best {
                Some((_, d)) => {
                    self.air.drones[d].job = Job::Taxi { rider: c, from, to, aboard: false };
                    self.air_retarget(d, from);
                }
                None => {
                    let waited = self.citizens.get(c).and_then(|x| x.transit).map_or(0, |t| tick.saturating_sub(t.since));
                    let at_pad = self.citizens.get(c).is_some_and(|x| matches!(x.activity, Activity::Waiting { .. }));
                    if at_pad && waited > air::WAIT_GIVE_UP {
                        self.give_up_air(c, "No drone free. Had to take the car like everyone else.");
                    } else {
                        keep.push(c);
                    }
                }
            }
        }
        // Riders who set off while this ran keep their place in the queue.
        keep.append(&mut self.air.pending);
        self.air.pending = keep;
    }

    fn give_up_air(&mut self, c: CitizenId, thought: &str) {
        self.air.stats.gave_up_today += 1;
        self.air.pending.retain(|p| *p != c);
        let waiting = self.citizens.get(c).is_some_and(|x| matches!(x.activity, Activity::Waiting { .. }));
        let Some(trip) = self.citizens.get_mut(c).and_then(|x| x.transit.take()) else { return };
        self.citizens[c].thought = thought.to_string();
        if waiting {
            self.travel_to_impl(c, Some(trip.target), trip.purpose, false, false);
        }
    }

    /// Critical patients far from a hospital: the nearest idle air
    /// ambulance takes the job before the ground units are sent.
    fn dispatch_air_medics(&mut self) {
        let tick = self.clock.tick;
        let jobs: Vec<(u32, Vec2, Option<CitizenId>)> = self
            .services
            .active()
            .filter(|i| i.kind == IncidentKind::Medical && i.stage == Stage::Reported && i.unit.is_none() && !i.outside && i.dispatch_at.is_some_and(|t| tick >= t))
            .map(|i| (i.id, i.pos, i.citizen))
            .collect();
        if jobs.is_empty() {
            return;
        }
        for (id, pos, patient) in jobs {
            let Some(patient) = patient else { continue };
            let age = self.citizens.get(patient).map_or(0, |c| c.age);
            if !critical(age, id) {
                continue;
            }
            let nearest = self.buildings.values().filter(|b| b.service.is_some_and(|s| s.kind == ServiceKind::Health) && !b.is_derelict()).filter_map(|b| self.lots.get(b.lot)).map(|l| l.center.distance(pos)).fold(f32::MAX, f32::min);
            if nearest <= air::AMBULANCE_MIN_DIST {
                continue;
            }
            let heli = self.air.drones.iter().filter(|(_, d)| d.craft == Craft::AirAmbulance && d.job == Job::Idle && d.phase == Phase::Parked && d.until <= tick).map(|(hid, d)| (d.pos.distance(pos), hid)).min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            let Some((_, hid)) = heli else { continue };
            let Some(inc) = self.services.incident(id).cloned() else { continue };
            let base = self.air.drones[hid].base;
            let call = format!("{} {}, {}", Craft::AirAmbulance.call_sign(), self.air.drones[hid].number, self.buildings.get(base).map_or(String::new(), |b| b.name.clone()));
            {
                let i = self.services.incident_mut(id).unwrap();
                i.stage = Stage::Dispatched;
                i.dispatched = Some(tick);
                i.unit_name = Some(call.clone());
            }
            if let Some(d) = inc.dispatcher.and_then(|d| self.citizens.get_mut(d)) {
                d.remember(tick, format!("Critical patient at {}, too far for the road. Sent {call}.", inc.place));
            }
            self.events.push(SimEvent::UnitDispatched(id));
            self.air.stats.medic_flights_today += 1;
            self.air.stats.medic_flights_total += 1;
            self.air.drones[hid].job = Job::Medic { incident: id, patient, hospital: None, aboard: false, work_until: 0 };
            // On the roof if it has a pad, else in the street out front.
            match self.air.pad(inc.building).copied() {
                Some(p) => self.air_take_off(hid, Some(p.building), p.pos, p.height),
                None => {
                    let spot = self.building_lot(inc.building).map_or(pos, |l| if l.car_access() { l.car_point } else { l.entrance() });
                    self.air_take_off(hid, None, spot, 0.0);
                }
            }
        }
    }

    /// At night the police helicopter circles a crime scene with its
    /// searchlight until the job is over.
    fn dispatch_police_heli(&mut self) {
        let tick = self.clock.tick;
        let hour = self.clock.hour();
        if (5..20).contains(&hour) {
            return;
        }
        let watched: Vec<u32> = self.air.drones.values().filter_map(|d| if let Job::Patrol { incident } = d.job { Some(incident) } else { None }).collect();
        let crimes: Vec<(u32, Vec2)> = self.services.active().filter(|i| i.kind == IncidentKind::Crime && matches!(i.stage, Stage::Reported | Stage::Dispatched | Stage::OnScene) && !watched.contains(&i.id)).map(|i| (i.id, i.pos)).collect();
        for (id, pos) in crimes {
            let heli = self.air.drones.iter().filter(|(_, d)| d.craft == Craft::PoliceHeli && d.job == Job::Idle && d.phase == Phase::Parked && d.until <= tick).map(|(hid, d)| (d.pos.distance(pos), hid)).min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            let Some((_, hid)) = heli else { return };
            self.air.drones[hid].job = Job::Patrol { incident: id };
            self.air.stats.police_flights_today += 1;
            self.air_take_off(hid, None, pos, 0.0);
        }
    }

    // ── flight ─────────────────────────────────────────────────────────────

    /// Head for pad `dest` from wherever the aircraft is.
    fn air_retarget(&mut self, id: DroneId, dest: BuildingId) {
        let Some(pad) = self.air.pad(dest).copied() else { return };
        let d = &self.air.drones[id];
        match d.phase {
            Phase::Parked | Phase::Boarding if d.at == Some(dest) => {
                // Already on the pad: wait for the rider there.
                let d = &mut self.air.drones[id];
                d.phase = Phase::Parked;
                d.dest = Some(dest);
                d.until = 0;
            }
            Phase::Parked | Phase::Boarding => self.air_take_off(id, Some(dest), pad.pos, pad.height),
            Phase::Descending if d.at == Some(dest) => {}
            _ => {
                let d = &mut self.air.drones[id];
                if d.phase == Phase::Descending {
                    d.at = None;
                    d.phase = Phase::Climbing;
                } else if d.phase == Phase::Holding {
                    d.phase = Phase::Cruising;
                }
                d.dest = Some(dest);
                d.goal = pad.pos;
                d.goal_alt = pad.height;
            }
        }
    }

    /// Lift off for `goal` (a pad, or a street spot with `dest` None).
    fn air_take_off(&mut self, id: DroneId, dest: Option<BuildingId>, goal: Vec2, goal_alt: f32) {
        let d = &mut self.air.drones[id];
        d.dest = dest;
        d.goal = goal;
        d.goal_alt = goal_alt;
        d.until = 0;
        if !d.phase.airborne() {
            d.phase = Phase::Climbing;
            d.flights_today += 1;
            d.flights_total += 1;
            if d.craft == Craft::Taxi {
                self.air.stats.flights_today += 1;
            }
        } else if d.phase == Phase::Holding || d.phase == Phase::Orbiting {
            d.phase = Phase::Cruising;
        }
    }

    /// Back to base, to park and charge.
    fn air_go_home(&mut self, id: DroneId) {
        let base = self.air.drones[id].base;
        self.air.drones[id].job = Job::Home;
        match self.air.pad(base).copied() {
            Some(p) => {
                if self.air.drones[id].at == Some(base) && !self.air.drones[id].phase.airborne() {
                    let d = &mut self.air.drones[id];
                    d.job = Job::Idle;
                    d.phase = Phase::Parked;
                    d.dest = Some(base);
                } else {
                    self.air_take_off(id, Some(base), p.pos, p.height);
                }
            }
            None => self.scrap_drone(id),
        }
    }

    /// One tick of one aircraft.
    fn fly(&mut self, id: DroneId) {
        let tick = self.clock.tick;
        let dt = self.config.traffic_seconds_per_tick;
        {
            let d = &mut self.air.drones[id];
            d.prev_pos = d.pos;
            d.prev_alt = d.alt;
            d.prev_heading = d.heading;
        }
        let phase = self.air.drones[id].phase;
        match phase {
            Phase::Parked => self.parked(id),
            Phase::Boarding => {
                if tick >= self.air.drones[id].until {
                    self.boarded(id);
                }
            }
            Phase::Climbing => {
                let d = &mut self.air.drones[id];
                let cruise = d.craft.cruise();
                let rise = (air::VERTICAL_MPS * dt).min((cruise - d.alt).max(0.0));
                d.alt += rise;
                if d.craft == Craft::Taxi {
                    d.battery -= rise * air::BATTERY_PER_CLIMB_M;
                }
                // Clear of the roof, start edging towards the goal.
                if d.alt > cruise * 0.6 {
                    let step = d.craft.mps() * dt * 0.35;
                    step_towards(d, step);
                }
                if d.alt >= cruise - 0.01 {
                    d.alt = cruise;
                    d.at = None;
                    d.phase = Phase::Cruising;
                }
            }
            Phase::Cruising => {
                let (step, dist) = {
                    let d = &self.air.drones[id];
                    (d.craft.mps() * dt, d.pos.distance(d.goal))
                };
                let patrol = matches!(self.air.drones[id].job, Job::Patrol { .. });
                if dist <= air::APPROACH.max(step * 0.5) {
                    if patrol {
                        self.air.drones[id].phase = Phase::Orbiting;
                    } else {
                        self.request_landing(id);
                    }
                } else {
                    let d = &mut self.air.drones[id];
                    let moved = step.min(dist - air::APPROACH * 0.5);
                    step_towards(d, moved);
                    if d.craft == Craft::Taxi {
                        d.battery -= moved / 1000.0 * air::BATTERY_PER_KM;
                        d.km_today += moved / 1000.0;
                    }
                }
            }
            Phase::Holding => {
                {
                    let d = &mut self.air.drones[id];
                    circle(d, air::HOLD_RADIUS, d.craft.mps() * dt * 0.5, d.craft.cruise() - 20.0);
                    if d.craft == Craft::Taxi {
                        d.battery -= d.craft.mps() * dt * 0.5 / 1000.0 * air::BATTERY_PER_KM;
                    }
                }
                self.request_landing(id);
            }
            Phase::Orbiting => {
                {
                    let d = &mut self.air.drones[id];
                    circle(d, 60.0, d.craft.mps() * dt * 0.4, d.craft.cruise() * 0.65);
                }
                let over = match self.air.drones[id].job {
                    Job::Patrol { incident } => self.services.incident(incident).is_none_or(|i| !i.active()) || (5..20).contains(&self.clock.hour()),
                    _ => true,
                };
                if over {
                    self.air_go_home(id);
                }
            }
            Phase::Descending => {
                let d = &mut self.air.drones[id];
                let dist = d.pos.distance(d.goal);
                step_towards(d, (d.craft.mps() * dt * 0.5).min(dist));
                let fall = (air::VERTICAL_MPS * dt).min((d.alt - d.goal_alt).max(0.0));
                // Come down over the pad, not on the way to it.
                if dist < 25.0 {
                    d.alt -= fall;
                }
                if d.pos.distance(d.goal) < 0.5 && d.alt <= d.goal_alt + 0.01 {
                    d.alt = d.goal_alt;
                    d.pos = d.goal;
                    self.landed(id);
                }
            }
        }
        // Noise under it, louder low down.
        let d = &self.air.drones[id];
        if d.phase.airborne() {
            let w = if d.alt < 60.0 { 1.0 } else { 0.3 };
            *self.air.noise.entry(air::noise_cell(d.pos)).or_insert(0.0) += w;
        }
        // Anyone aboard moves with it.
        if let Some(c) = d.aboard() {
            let (pos, prev, heading) = (d.pos, d.prev_pos, d.heading);
            if let Some(x) = self.citizens.get_mut(c) {
                if matches!(x.activity, Activity::Flying { .. }) {
                    x.pos = pos;
                    x.prev_pos = prev;
                    x.heading = heading;
                }
            }
        }
    }

    /// Near the pad: land if a slot is free and nobody has been holding
    /// longer, else hold.
    fn request_landing(&mut self, id: DroneId) {
        let tick = self.clock.tick;
        let Some(dest) = self.air.drones[id].dest else {
            self.air.drones[id].phase = Phase::Descending;
            return;
        };
        let mine = {
            let d = &self.air.drones[id];
            if d.phase == Phase::Holding { (d.hold_since, id) } else { (tick, id) }
        };
        let ahead = self.air.drones.iter().any(|(oid, o)| oid != id && o.phase == Phase::Holding && o.dest == Some(dest) && (o.hold_since, oid) < mine);
        let slot = if ahead { None } else { self.air.free_slot(dest) };
        let d = &mut self.air.drones[id];
        match slot {
            Some(s) => {
                d.at = Some(dest);
                d.slot = s;
                d.phase = Phase::Descending;
            }
            None if d.phase != Phase::Holding => {
                d.phase = Phase::Holding;
                d.hold_since = tick;
                self.air.stats.holds_today += 1;
            }
            None => {}
        }
    }

    /// Wheels down.
    fn landed(&mut self, id: DroneId) {
        let tick = self.clock.tick;
        let (job, dest, pos, base) = {
            let d = &mut self.air.drones[id];
            d.phase = Phase::Parked;
            (d.job, d.dest, d.pos, d.base)
        };
        match job {
            Job::Taxi { rider, from, aboard: false, .. } if dest == Some(from) => self.try_board(id, rider, from),
            Job::Taxi { rider, to, aboard: true, .. } if dest == Some(to) => {
                let fare_pad = self.air.pad(to).map(|p| p.pos).unwrap_or(pos);
                self.walk_from_pad(rider, fare_pad);
                if let Some(x) = self.citizens.get_mut(rider) {
                    x.thought = "Above the smog in four minutes. Worth every cent.".to_string();
                }
                let d = &mut self.air.drones[id];
                d.job = Job::Idle;
                d.phase = Phase::Boarding;
                d.until = tick + air::BOARD_TICKS;
            }
            Job::Medic { incident, aboard: false, .. } => {
                let live = self.services.incident(incident).is_some_and(|i| i.stage == Stage::Dispatched);
                if !live {
                    self.air_go_home(id);
                    return;
                }
                self.on_scene(incident);
                if let Some(i) = self.services.incident_mut(incident) {
                    // The air crew finishes the job, not the ground timer.
                    i.work_left = f32::MAX;
                }
                if let Job::Medic { work_until, .. } = &mut self.air.drones[id].job {
                    *work_until = tick + air::MEDIC_WORK;
                }
            }
            Job::Medic { incident, patient, hospital: Some(h), aboard: true, .. } => {
                if let Some(x) = self.citizens.get_mut(patient) {
                    x.activity = Activity::Idle;
                }
                if self.buildings.contains_key(h) && self.citizens.contains_key(patient) {
                    self.admit(incident, patient, h);
                    if let Some(x) = self.citizens.get_mut(patient) {
                        x.remember(tick, "Flown to hospital by helicopter. Don't remember much of it.");
                    }
                } else {
                    self.strand_flyer(patient, pos);
                    self.resolve(incident, crate::services::Outcome::Gone);
                }
                self.air_go_home(id);
            }
            _ if dest == Some(base) => {
                let d = &mut self.air.drones[id];
                d.job = Job::Idle;
                d.until = tick + air::TURNAROUND;
            }
            _ => self.air_go_home(id),
        }
    }

    /// The rider gets in, if they are standing at the pad; they pay.
    fn try_board(&mut self, id: DroneId, rider: CitizenId, pad: BuildingId) {
        let tick = self.clock.tick;
        if !self.waiting_at(rider, pad) {
            // Still walking up: wait on the pad. Gone elsewhere: go home.
            let coming = self.air_leg(rider).is_some_and(|(from, _)| from == pad);
            if !coming {
                self.air_go_home(id);
            }
            return;
        }
        let Some((from, to)) = self.air_leg(rider) else { return };
        let metres = self.air.pad(from).zip(self.air.pad(to)).map_or(0.0, |(a, b)| a.pos.distance(b.pos));
        let fare = air::fare(metres);
        let to_name = self.buildings.get(to).map_or(String::new(), |b| b.name.clone());
        {
            let c = &mut self.citizens[rider];
            let from_savings = fare.min(c.savings.max(0.0));
            c.savings -= from_savings;
            let rest = fare - from_savings;
            if let Some(h) = c.household.and_then(|h| self.households.get_mut(h)) {
                h.money -= rest;
            } else {
                c.savings -= rest;
            }
            c.activity = Activity::Flying { drone: id };
            c.inside = None;
            c.remember(tick, format!("Took a drone taxi to {to_name}. {fare:.0} well spent."));
        }
        self.economy.credit_item(Flow::BusinessTax, fare * air::LEVY, Item::Named("air taxi levy".into()));
        let a = &mut self.air;
        a.company.revenue_today += fare * (1.0 - air::LEVY);
        a.stats.riders_today += 1;
        a.stats.riders_total += 1;
        a.stats.fares_today += fare;
        a.pending.retain(|p| *p != rider);
        let d = &mut a.drones[id];
        d.job = Job::Taxi { rider, from, to, aboard: true };
        d.phase = Phase::Boarding;
        d.until = tick + air::BOARD_TICKS;
    }

    /// Doors shut: off to the next leg.
    fn boarded(&mut self, id: DroneId) {
        let job = self.air.drones[id].job;
        match job {
            Job::Taxi { to, aboard: true, .. } => {
                let Some(p) = self.air.pad(to).copied() else {
                    if let Some(c) = self.air.drones[id].aboard() {
                        let pos = self.air.drones[id].pos;
                        self.strand_flyer(c, pos);
                    }
                    self.air_go_home(id);
                    return;
                };
                self.air_take_off(id, Some(to), p.pos, p.height);
            }
            Job::Taxi { aboard: false, .. } => self.air.drones[id].phase = Phase::Parked,
            _ => {
                let d = &self.air.drones[id];
                if d.at == Some(d.base) {
                    self.air.drones[id].phase = Phase::Parked;
                } else {
                    self.air_go_home(id);
                }
            }
        }
    }

    /// On the ground: charging at base, waiting for a rider at a pickup,
    /// or the medics working on scene.
    fn parked(&mut self, id: DroneId) {
        let tick = self.clock.tick;
        let (job, at, base) = {
            let d = &self.air.drones[id];
            (d.job, d.at, d.base)
        };
        if at == Some(base) && self.air.drones[id].craft == Craft::Taxi {
            let d = &mut self.air.drones[id];
            d.battery = (d.battery + air::CHARGE_PER_TICK).min(1.0);
        }
        match job {
            Job::Taxi { rider, from, aboard: false, .. } if at == Some(from) => self.try_board(id, rider, from),
            Job::Medic { incident, patient, aboard: false, work_until, .. } if work_until > 0 && tick >= work_until => {
                let pos = self.air.drones[id].pos;
                let live = self.services.incident(incident).is_some_and(|i| i.active() && i.stage == Stage::OnScene);
                let here = self.services.incident(incident).is_some_and(|i| self.citizens.get(patient).is_some_and(|c| c.inside == Some(i.building)));
                if !live || !here {
                    if live {
                        self.resolve(incident, crate::services::Outcome::Gone);
                    }
                    self.air_go_home(id);
                    return;
                }
                // The nearest hospital with a bed, a helipad one first.
                let beds = |w: &World, h: BuildingId, s: crate::services::ServiceSite| (w.services.admissions.iter().filter(|a| a.hospital == h).count() as u32) < s.kind.capacity(s.tier);
                let mut options: Vec<(bool, f32, BuildingId)> = self
                    .buildings
                    .iter()
                    .filter_map(|(hid, b)| b.service.filter(|s| s.kind == ServiceKind::Health && !b.is_derelict() && beds(self, hid, *s)).map(|_| (hid, b)))
                    .filter_map(|(hid, b)| self.lots.get(b.lot).map(|l| (self.air.pad(hid).is_none(), l.center.distance(pos), hid)))
                    .collect();
                options.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)).then(a.2.cmp(&b.2)));
                let Some(&(_, _, h)) = options.first() else {
                    self.treat_on_site(incident);
                    self.air_go_home(id);
                    return;
                };
                if let Some(i) = self.services.incident_mut(incident) {
                    i.stage = Stage::Transporting;
                }
                self.leave_building(patient);
                if let Some(x) = self.citizens.get_mut(patient) {
                    x.activity = Activity::Flying { drone: id };
                    x.inside = None;
                    x.thought = "The rotors. So loud.".to_string();
                }
                self.air.drones[id].job = Job::Medic { incident, patient, hospital: Some(h), aboard: true, work_until };
                match self.air.pad(h).copied() {
                    Some(p) => self.air_take_off(id, Some(h), p.pos, p.height),
                    None => {
                        let spot = self.building_lot(h).map_or(pos, |l| if l.car_access() { l.car_point } else { l.entrance() });
                        self.air_take_off(id, None, spot, 0.0);
                    }
                }
            }
            // Dropped off somewhere that is not home: make room.
            Job::Idle if at != Some(base) => self.air_go_home(id),
            _ => {}
        }
    }

    // ── the street's view ──────────────────────────────────────────────────

    /// People out on the street who are not rich see the drones come and
    /// go over their heads, and resent it, once a day at most.
    fn grumble(&mut self) {
        let tick = self.clock.tick;
        let low: Vec<(Vec2, f32)> = self.air.drones.values().filter(|d| d.craft == Craft::Taxi && d.phase.airborne()).map(|d| (d.pos, if d.alt < 80.0 { 70.0 } else { 35.0 })).collect();
        if low.is_empty() {
            return;
        }
        let near: Vec<CitizenId> = self
            .citizens
            .iter()
            .filter(|(_, c)| c.is_visible() && c.class() != Tier::High && low.iter().any(|(p, r)| p.distance(c.pos) < *r))
            .map(|(id, _)| id)
            .collect();
        const LINES: [&str; 4] = [
            "Another drone taxi screamed over. The rich don't even touch the ground any more.",
            "Drones overhead all day. Must be nice up there.",
            "A sky taxi rattled the windows. We get the noise, they get the view.",
            "Watched a drone land on some tower roof. Not for the likes of us.",
        ];
        for c in near {
            if self.air.grumbled.get(&c).is_some_and(|t| tick < t + 1440) {
                continue;
            }
            if self.arng().random::<f32>() > 0.3 {
                continue;
            }
            let line = LINES[self.arng().random_range(0..LINES.len())];
            self.air.grumbled.insert(c, tick);
            self.air.stats.grumbles_today += 1;
            let x = &mut self.citizens[c];
            x.remember(tick, line.to_string());
            self.concerns.heard(c, x, crate::concerns::Concern::Noise, 0.2);
            x.thought = "Bloody drones.".to_string();
            x.mood = (x.mood - 0.02).max(0.0);
        }
    }

    /// Land value: busy flight paths and rooftop pads are a nuisance (the
    /// land value grid's nuisance layer, which homes feel most).
    pub(super) fn splat_air_noise(&self, b: &mut LandValueBuilder) {
        for (cell, n) in self.air.noise.iter() {
            let p = air::noise_penalty(*n);
            if p > 0.0 {
                b.splat_nuisance(air::noise_cell_center(*cell), 90.0, p);
            }
        }
        for pad in self.air.pads.iter().filter(|p| p.kind == PadKind::Rooftop) {
            b.splat_nuisance(pad.pos, 70.0, 0.01);
        }
    }
}

/// Move a drone `step` metres towards its goal (never past it), turning to face it.
fn step_towards(d: &mut Drone, step: f32) {
    let to = d.goal - d.pos;
    let dist = to.length();
    if dist < 1e-3 {
        return;
    }
    let s = step.min(dist);
    d.pos += to / dist * s;
    d.heading = to.y.atan2(to.x);
}

/// Fly a circle of `radius` round the goal at `alt`, `step` metres a tick.
fn circle(d: &mut Drone, radius: f32, step: f32, alt: f32) {
    let off = d.pos - d.goal;
    let a = if off.length() > 1.0 { off.y.atan2(off.x) } else { 0.0 } + step / radius;
    let next = d.goal + Vec2::new(a.cos(), a.sin()) * radius;
    let dir = next - d.pos;
    if dir.length() > 1e-3 {
        d.heading = dir.y.atan2(dir.x);
    }
    d.pos = d.pos.lerp(next, 0.5);
    d.alt += (alt - d.alt).clamp(-air::VERTICAL_MPS * 4.0, air::VERTICAL_MPS * 4.0);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mind::IdleMind;
    use crate::services::Outcome;

    fn rect(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
        vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
    }

    /// A long avenue: civic land on the north side, homes on the south,
    /// emergencies off so nothing random takes a building away.
    fn town(seed: u64) -> World {
        let mut w = World::new(WorldConfig { seed, emergencies: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
        w.economy.treasury = 1_000_000.0;
        w.add_road(&[Vec2::new(-880.0, 0.0), Vec2::new(880.0, 0.0)], 1, 1);
        w.add_road(&[Vec2::new(-880.0, 120.0), Vec2::new(880.0, 120.0)], 1, 1);
        for x in [-880.0, -600.0, -300.0, 0.0, 300.0, 600.0, 880.0] {
            w.add_road(&[Vec2::new(x, 0.0), Vec2::new(x, 120.0)], 1, 1);
        }
        w.add_zone(rect(-870.0, 8.0, 1740.0, 50.0), LandUse::Civic);
        w.add_zone(rect(-870.0, -60.0, 1740.0, 52.0), LandUse::Residential);
        w
    }

    fn lot_near(w: &World, x: f32, lu: LandUse) -> LotId {
        w.lots
            .iter()
            .filter(|(_, l)| l.land_use == lu && l.usable() && l.building.is_none() && l.car_access() && l.area() >= 380.0)
            .min_by(|a, b| (a.1.center.x - x).abs().total_cmp(&(b.1.center.x - x).abs()))
            .map(|(id, _)| id)
            .expect("a lot")
    }

    fn building_near(w: &mut World, x: f32, lu: LandUse) -> BuildingId {
        let lot = lot_near(w, x, lu);
        w.build_on(lot);
        let id = w.lots[lot].building.unwrap();
        w.finish_construction(id);
        id
    }

    /// A citizen of the given income at home in `home`, with a household.
    fn resident(w: &mut World, home: BuildingId, income: f32) -> CitizenId {
        let e = w.building_entrance(home).unwrap();
        let hid = w.households.insert(Household { members: Vec::new(), home: Some(home), money: 600.0, groceries: 10.0, rent_stress_days: 0, moved_day: 0 });
        let n = w.citizens.len();
        let c = w.citizens.insert(Citizen {
            name: format!("Test {n}"),
            seed: 1000 + n as u64,
            home: Some(home),
            work: None,
            pos: e,
            heading: 0.0,
            prev_pos: e,
            prev_heading: 0.0,
            mode: TravelMode::Walk,
            car: None,
            commute: Default::default(),
            look: None,
            to_car: None,
            drunk: Default::default(),
            owns_car: true,
            owns_bike: false,
            vehicle: None,
            parked_at: None,
            transit: None,
            activity: Activity::AtHome,
            memories: Vec::new(),
            places: Default::default(),
            thought: String::new(),
            spotlight: false,
            next_decision_tick: u64::MAX / 2,
            arrived_tick: 0,
            inside: Some(home),
            mood: 0.6,
            income,
            savings: 400.0,
            rent_stress_days: 0,
            satiety: 0.9,
            groceries: 0.0,
            hungry_days: 0,
            trail: Vec::new(),
            pending_footfall: Vec::new(),
            household: Some(hid),
            age: 40,
            wakefulness: 0.9,
            asleep: false,
            packed_meal: 1.0,
        });
        w.households[hid].members.push(c);
        w.buildings[home].residents.push(c);
        w.buildings[home].households.push(hid);
        c
    }

    fn vertiport(w: &mut World, x: f32, tier: Tier) -> BuildingId {
        let lot = lot_near(w, x, LandUse::Civic);
        let b = w.build_service(lot, ServiceKind::Vertiport, tier).expect("vertiport");
        w.refresh_pads();
        b
    }

    /// Plan a trip, a few times over (the rich sometimes take the car).
    fn plan(w: &mut World, c: CitizenId, to: BuildingId) -> Option<TransitLeg> {
        for _ in 0..12 {
            let home = w.citizens[c].home.unwrap();
            {
                let cz = &mut w.citizens[c];
                cz.transit = None;
                cz.activity = Activity::AtHome;
                cz.inside = Some(home);
            }
            w.air.pending.clear();
            w.travel_to(c, Some(to), Purpose::Work);
            if let Some(t) = w.citizens[c].transit {
                return Some(t.leg);
            }
            if let Activity::Driving { car, .. } = w.citizens[c].activity {
                w.traffic.remove_car(car);
            }
        }
        None
    }

    #[test]
    fn only_the_rich_fly_and_only_with_pads_at_both_ends() {
        let mut w = town(3);
        let home = building_near(&mut w, -820.0, LandUse::Residential);
        let office = building_near(&mut w, 820.0, LandUse::Civic);
        let west = vertiport(&mut w, -780.0, Tier::Mid);
        let rich = resident(&mut w, home, 220.0);
        assert!(w.air.taxis() >= 1, "a vertiport brings drones");
        // One pad, near home only: nobody flies.
        assert!(!matches!(plan(&mut w, rich, office), Some(TransitLeg::Air { .. })), "no pad at the far end");
        let east = vertiport(&mut w, 780.0, Tier::Low);
        assert!(matches!(plan(&mut w, rich, office), Some(TransitLeg::Air { from, to }) if from == west && to == east), "pads at both ends: the rich fly");
        // The middle class and the poor take the road.
        let mid = resident(&mut w, home, 110.0);
        assert!(!matches!(plan(&mut w, mid, office), Some(TransitLeg::Air { .. })), "middle income does not fly");
        let poor = resident(&mut w, home, 40.0);
        assert!(!matches!(plan(&mut w, poor, office), Some(TransitLeg::Air { .. })));
        // A short hop is walked or driven.
        let near = building_near(&mut w, -700.0, LandUse::Civic);
        assert!(!matches!(plan(&mut w, rich, near), Some(TransitLeg::Air { .. })), "too short to fly");
    }

    /// Two dear towers at either end of the avenue, each with a rooftop
    /// pad, and a vertiport in the middle for the drones.
    fn tower_town(seed: u64) -> (World, CitizenId, BuildingId, BuildingId) {
        let mut w = town(seed);
        let home = building_near(&mut w, -820.0, LandUse::Residential);
        let office = building_near(&mut w, 820.0, LandUse::Residential);
        for b in [home, office] {
            let bld = &mut w.buildings[b];
            bld.tier = Tier::High;
            bld.floors = 12;
        }
        w.land_value.fill(0.9);
        for b in [home, office] {
            for _ in 0..9 {
                resident(&mut w, b, 300.0);
            }
        }
        w.refresh_rooftops();
        assert!(w.air.has_rooftop(home) && w.air.has_rooftop(office), "dear towers full of the rich grow pads");
        vertiport(&mut w, 0.0, Tier::Mid);
        let rich = resident(&mut w, home, 260.0);
        (w, rich, home, office)
    }

    #[test]
    fn a_flight_goes_straight_and_beats_the_road() {
        let (mut w, rich, home, office) = tower_town(5);
        let road_world = {
            let mut r = w.clone();
            r.air = Default::default();
            r
        };
        assert!(matches!(plan(&mut w, rich, office), Some(TransitLeg::Air { from, to }) if from == home && to == office));
        let a = w.air.pad(home).unwrap().pos;
        let b = w.air.pad(office).unwrap().pos;
        let mut minds = IdleMind;
        let (mut aboard_at, mut off_at, mut peak, mut worst_off_line) = (None, None, 0.0f32, 0.0f32);
        for t in 0..400u32 {
            w.tick(&mut minds);
            if let Activity::Flying { drone } = w.citizens[rich].activity {
                aboard_at.get_or_insert(t);
                let d = &w.air.drones[drone];
                peak = peak.max(d.alt);
                // Distance from the straight line between the pads.
                let ab = b - a;
                let off = (d.pos - a).perp_dot(ab).abs() / ab.length();
                worst_off_line = worst_off_line.max(off);
            } else if aboard_at.is_some() && off_at.is_none() {
                off_at = Some(t);
            }
            if w.citizens[rich].inside == Some(office) {
                break;
            }
        }
        assert_eq!(w.citizens[rich].inside, Some(office), "arrived: {}", w.citizens[rich].activity.label());
        let air_ticks = off_at.unwrap() - aboard_at.unwrap();
        assert!(peak >= air::TAXI_ALT - 1.0, "climbs to cruise height ({peak})");
        assert!(worst_off_line < 5.0, "flies straight, no roads ({worst_off_line} m off the line)");
        assert!(w.air.stats.riders_today == 1 && w.air.stats.fares_today >= air::fare(a.distance(b)) - 0.01);
        assert!(w.citizens[rich].memories.iter().any(|m| m.text.contains("drone taxi")));
        // The same trip by car, in the same town without the air service.
        let mut r = road_world;
        // [carlife] Owning a car means a real car parked somewhere.
        let at = r.building_entrance(home).unwrap();
        r.give_car_parked(rich, at);
        r.travel_to(rich, Some(office), Purpose::Work);
        assert!(matches!(r.citizens[rich].activity, Activity::Driving { .. }), "drives without drones");
        let mut road_ticks = 0u32;
        while matches!(r.citizens[rich].activity, Activity::Driving { .. }) && road_ticks < 2000 {
            road_ticks += 1;
            r.tick(&mut minds);
        }
        eprintln!("pad to pad {:.0} m: {air_ticks} ticks in the air, {road_ticks} ticks by road", a.distance(b));
        assert!(air_ticks * 3 < road_ticks * 2, "air {air_ticks} ticks against the road's {road_ticks}");
    }

    #[test]
    fn a_busy_rooftop_pad_makes_drones_hold_and_land_in_turn() {
        let (mut w, _, home, _) = tower_town(7);
        let pad = *w.air.pad(home).unwrap();
        let taxis: Vec<DroneId> = w.air.drones.iter().filter(|(_, d)| d.craft == Craft::Taxi).map(|(id, _)| id).take(2).collect();
        assert_eq!(taxis.len(), 2);
        for (k, &id) in taxis.iter().enumerate() {
            let d = &mut w.air.drones[id];
            d.at = None;
            d.phase = Phase::Cruising;
            d.alt = air::TAXI_ALT;
            d.pos = pad.pos + Vec2::new(300.0 + k as f32 * 60.0, 0.0);
            d.dest = Some(home);
            d.goal = pad.pos;
            d.goal_alt = pad.height;
            d.job = Job::Home;
        }
        let mut minds = IdleMind;
        let mut landed: Vec<DroneId> = Vec::new();
        for _ in 0..120 {
            w.tick(&mut minds);
            assert!(w.air.slots_used(home) <= 1, "one drone on a one-slot pad");
            for &id in &taxis {
                let d = &w.air.drones[id];
                if d.at == Some(home) && !landed.contains(&id) {
                    landed.push(id);
                }
            }
        }
        assert!(w.air.stats.holds_today >= 1, "the second drone held");
        assert_eq!(landed, taxis, "both landed, the nearer first");
    }

    #[test]
    fn rooftop_pads_come_with_dear_land_and_go_when_it_cheapens() {
        let mut w = town(9);
        let tower = building_near(&mut w, -500.0, LandUse::Residential);
        let low = building_near(&mut w, -300.0, LandUse::Residential);
        let mid = building_near(&mut w, -100.0, LandUse::Residential);
        w.buildings[tower].tier = Tier::High;
        w.buildings[tower].floors = 14;
        w.buildings[low].tier = Tier::High;
        w.buildings[low].floors = 3;
        w.buildings[mid].tier = Tier::Mid;
        w.buildings[mid].floors = 14;
        w.land_value.fill(0.9);
        w.refresh_rooftops();
        assert!(w.air.rooftops.is_empty(), "dear land but nobody rich to fly: no pads");
        for _ in 0..9 {
            resident(&mut w, tower, 300.0);
            resident(&mut w, mid, 300.0);
        }
        w.land_value.fill(0.6);
        w.refresh_rooftops();
        assert!(w.air.rooftops.is_empty(), "ordinary land: no pads");
        w.land_value.fill(0.9);
        w.drain_events();
        w.refresh_rooftops();
        assert_eq!(w.air.rooftops, vec![tower], "only the tall high-tier tower");
        assert!(w.drain_events().iter().any(|e| matches!(e, SimEvent::BuildingChanged(b) if *b == tower)), "the tower is redrawn");
        assert!(w.air.pad(tower).is_some_and(|p| p.kind == PadKind::Rooftop && p.slots == 1));
        // Kept through a small dip, lost when the land cheapens.
        w.land_value.fill(0.75);
        w.refresh_rooftops();
        assert_eq!(w.air.rooftops, vec![tower]);
        w.land_value.fill(0.5);
        w.refresh_rooftops();
        assert!(w.air.rooftops.is_empty() && w.air.pad(tower).is_none());
        // Rare: twenty dear towers with rich tenants share about two pads,
        // and the richest get them.
        w.land_value.fill(0.9);
        let mut towers = Vec::new();
        for k in 0..20 {
            let Some(lot) = w.lots.iter().filter(|(_, l)| l.usable() && l.building.is_none() && l.land_use == LandUse::Residential).map(|(id, _)| id).next() else { break };
            w.build_on(lot);
            let b = w.lots[lot].building.unwrap();
            w.finish_construction(b);
            w.buildings[b].tier = Tier::High;
            w.buildings[b].floors = 12;
            for _ in 0..(2 + k) {
                resident(&mut w, b, 300.0);
            }
            towers.push(b);
        }
        w.refresh_rooftops();
        let n = w.air.rooftops.len();
        assert!((1..=4).contains(&n), "{n} pads on {} towers", towers.len() + 1);
        assert!(w.air.rooftops.iter().all(|b| towers.iter().position(|t| t == b).is_none_or(|k| k >= towers.len() / 2)), "pads go to the richer towers");
    }

    #[test]
    fn critical_patients_far_from_a_hospital_are_flown_in() {
        let mut w = town(11);
        let lot = lot_near(&w, -800.0, LandUse::Civic);
        let hospital = w.build_service(lot, ServiceKind::Health, Tier::High).unwrap();
        w.refresh_pads();
        assert!(w.air.drones.values().any(|d| d.craft == Craft::AirAmbulance && d.base == hospital), "a high-tier hospital keeps a helicopter");
        let far_home = building_near(&mut w, 820.0, LandUse::Residential);
        let patient = resident(&mut w, far_home, 60.0);
        w.citizens[patient].age = 78;
        let id = w.fall_ill(patient).unwrap();
        let mut minds = IdleMind;
        let mut flew = false;
        for _ in 0..600 {
            w.tick(&mut minds);
            if matches!(w.citizens[patient].activity, Activity::Flying { .. }) {
                flew = true;
            }
            if w.services.incident(id).is_some_and(|i| !i.active()) {
                break;
            }
        }
        let inc = w.services.incident(id).unwrap();
        assert!(inc.unit_name.as_deref().is_some_and(|n| n.contains("Air Medic")), "{:?}", inc.unit_name);
        assert_eq!(inc.outcome, Some(Outcome::Admitted));
        assert!(flew, "carried in the helicopter");
        assert_eq!(w.citizens[patient].inside, Some(hospital));
        assert!(w.air.stats.medic_flights_today >= 1);
        // Someone ill next door to the hospital is not flown.
        let near_home = building_near(&mut w, -740.0, LandUse::Residential);
        let other = resident(&mut w, near_home, 60.0);
        w.citizens[other].age = 78;
        let id2 = w.fall_ill(other).unwrap();
        for _ in 0..80 {
            w.tick(&mut minds);
        }
        assert!(!w.services.incident(id2).and_then(|i| i.unit_name.clone()).is_some_and(|n| n.contains("Air")), "a short way is driven");
        assert_eq!(w.air.stats.medic_flights_total, 1);
    }

    #[test]
    fn air_traffic_is_deterministic_and_survives_a_save() {
        let run = |seed: u64| {
            let (mut w, rich, _, office) = tower_town(seed);
            assert!(plan(&mut w, rich, office).is_some());
            let mut minds = IdleMind;
            for _ in 0..12 {
                w.tick(&mut minds);
            }
            // Save and load mid-trip.
            let mut w = World::from_ron(&w.to_ron().unwrap()).unwrap();
            for _ in 0..200 {
                w.tick(&mut minds);
            }
            let poses: Vec<(f32, f32, f32)> = w.air.drones.values().map(|d| (d.pos.x, d.pos.y, d.alt)).collect();
            (poses, w.air.stats.riders_today, w.air.stats.flights_today, w.citizens[rich].inside == Some(office))
        };
        let a = run(21);
        let b = run(21);
        assert_eq!(a, b);
        assert!(a.3 && a.1 == 1, "the rider arrived after the load: {a:?}");
    }
}
