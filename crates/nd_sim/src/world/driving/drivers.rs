//! [driving] Drivers: drink, profiles, rage, and what the road shows of them.

use super::*;

impl World {
    /// Drinks at venues; sobering up everywhere else.
    pub(super) fn step_drink(&mut self, ticks: u32) {
        let buildings = &self.buildings;
        for c in self.citizens.values_mut() {
            let at_venue = match c.activity {
                Activity::Visiting(b) => buildings.get(b).is_some_and(|b| b.land_use == LandUse::Entertainment || b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Venue)),
                _ => false,
            };
            if at_venue && c.age >= 18 {
                let (_, hedonism) = driver_traits(c.seed);
                c.drunk = (c.drunk + DRINK_RATE * (hedonism - 0.3).max(0.0) * ticks as f32).min(1.0);
            } else if c.drunk > 0.0 {
                c.drunk = (c.drunk - SOBER_RATE * ticks as f32).max(0.0);
            }
        }
    }

    /// Give every car that set off since the last tick its driver's profile.
    pub(super) fn assign_drivers(&mut self) {
        let hour = self.clock.hour();
        let tick = self.clock.tick;
        let new: Vec<CarId> = self.traffic.cars.iter().filter(|(_, c)| !c.drive.profile.assigned).map(|(id, _)| id).collect();
        for id in new {
            let phase = car_hash(id, 1);
            let car = &self.traffic.cars[id];
            let gang = car.drive.gang.is_some();
            let profile = if car.siren || car.bus.is_some() || car.cargo.is_some() || self.services.unit_of_car(id).is_some() {
                DriverProfile::professional(phase)
            } else if let Some(c) = self.citizens.get(car.citizen) {
                let (caution, hedonism) = driver_traits(c.seed);
                let knack = ((c.seed.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) % 1000) as f32 / 1000.0;
                let p = DriverProfile::for_driver(c.age.max(17), caution, hedonism, c.drunk, knack, phase);
                if c.drunk >= DRUNK && !gang {
                    self.driving.drunk_trips_today += 1;
                    let cid = car.citizen;
                    let c = &mut self.citizens[cid];
                    c.remember(tick, "Drove after a few too many. Felt fine. Mostly.");
                }
                p
            } else {
                // A stranger driving through: anyone, now and then drunk at night.
                let age = 18 + (car_hash(id, 2) * 62.0) as u8;
                let night = !(5..22).contains(&hour);
                let drunk = if night && car_hash(id, 5) < 0.04 { 0.4 + 0.5 * car_hash(id, 6) } else { 0.0 };
                DriverProfile::for_driver(age, car_hash(id, 3), car_hash(id, 4), drunk, car_hash(id, 7), phase)
            };
            let car = &mut self.traffic.cars[id];
            car.drive.profile = if gang { profile.pack() } else { profile };
            car.drive.seen_lead_v = car.v;
        }
    }

    /// [rage] Time every drive from the tick it appears (expected minutes
    /// from its route at departure), and let drivers stuck far longer than
    /// that lose their temper: anger (`traffic::road_rage`, by patience and
    /// temper) is laid over their calm profile, so they tailgate, floor it,
    /// switch lanes and run ambers. It only builds during a trip and goes
    /// with the car when it arrives (into the trip's pain: `end_trip`).
    /// Professionals and takeover packs keep their heads.
    pub(in crate::world) fn step_rage(&mut self) {
        let tick = self.clock.tick;
        let secs = self.config.traffic_seconds_per_tick.max(0.1);
        let ids: Vec<CarId> = self.traffic.cars.keys().collect();
        for id in ids {
            let car = &self.traffic.cars[id];
            if !car.drive.profile.assigned {
                continue;
            }
            if car.drive.expected_min <= 0.0 {
                let expected = car.expected_seconds(&self.traffic.net) / secs;
                let pro = car.siren || car.bus.is_some() || car.cargo.is_some() || car.drive.gang.is_some() || self.services.unit_of_car(id).is_some() || !matches!(car.errand, crate::vehicle::Errand::Trip | crate::vehicle::Errand::Search { .. } | crate::vehicle::Errand::ToLot(_) | crate::vehicle::Errand::ToKerb | crate::vehicle::Errand::Leave | crate::vehicle::Errand::Through);
                let (patience, temper) = match self.citizens.get(car.citizen) {
                    Some(c) => {
                        let (caution, hedonism) = driver_traits(c.seed);
                        (crate::citizens::patience(c.seed), rebelliousness(caution, hedonism))
                    }
                    None => (car_hash(id, 8), rebelliousness(car_hash(id, 3), car_hash(id, 4))),
                };
                let car = &mut self.traffic.cars[id];
                car.drive.trip_start = tick;
                car.drive.expected_min = expected.max(1.0);
                car.drive.patience = patience;
                car.drive.temper = temper;
                car.drive.calm = (!pro).then_some(car.drive.profile);
                continue;
            }
            let Some(calm) = car.drive.calm else { continue };
            if car.crashed() || car.drive.gang.is_some() {
                continue;
            }
            let elapsed = tick.saturating_sub(car.drive.trip_start) as f32;
            let target = crate::traffic::road_rage(elapsed, car.drive.expected_min, car.drive.patience, car.drive.temper);
            // Anger builds over a few minutes rather than in one jump.
            let anger = if target > car.drive.anger { (car.drive.anger + 0.1).min(target) } else { car.drive.anger };
            if anger != car.drive.anger {
                let car = &mut self.traffic.cars[id];
                if car.drive.anger < 0.5 && anger >= 0.5 {
                    self.driving.road_rage_today += 1;
                }
                car.drive.anger = anger;
                car.drive.anger_peak = car.drive.anger_peak.max(anger);
                car.drive.profile = calm.angry(anger);
            }
        }
    }

    /// [clarity] A stop on a multi-stop drive (a child dropped at school, a
    /// delivery made, a cab's fare picked up): the next leg is timed afresh
    /// from here against its own route, and the driver, having had a
    /// breather at the kerb, calms down. A long round of stops is not one
    /// endless late trip, so it never boils over into road rage.
    pub(in crate::world) fn begin_leg(&mut self, id: CarId) {
        let tick = self.clock.tick;
        let secs = self.config.traffic_seconds_per_tick.max(0.1);
        let net = &self.traffic.net;
        let Some(car) = self.traffic.cars.get_mut(id) else { return };
        if car.drive.expected_min <= 0.0 {
            // Not timed yet: `step_rage` times it from here anyway.
            return;
        }
        car.drive.trip_start = tick;
        car.drive.expected_min = (car.expected_seconds(net) / secs).max(1.0);
        car.drive.anger = 0.0;
        if let Some(calm) = car.drive.calm {
            car.drive.profile = calm;
        }
    }

    /// [rage] The drive under way: minutes so far and minutes it should
    /// have taken (judged from the route when it set off). `None` before
    /// the first tick has timed it.
    pub fn car_trip_minutes(&self, id: CarId) -> Option<(f32, f32)> {
        let c = self.traffic.cars.get(id)?;
        (c.drive.expected_min > 0.0).then(|| (self.clock.tick.saturating_sub(c.drive.trip_start) as f32, c.drive.expected_min))
    }

    /// A vehicle in a few words, for the inspector and the remote API:
    /// "Mara Voss's hatchback", "bus on Line 2", "delivery truck".
    pub fn car_label(&self, id: CarId) -> String {
        let Some(c) = self.traffic.cars.get(id) else { return "a vehicle that has gone".into() };
        if let Some(bus) = &c.bus {
            return match self.transit.routes.get(bus.route) {
                Some(r) => format!("bus on {}", r.name),
                None => "a bus".into(),
            };
        }
        if c.cargo.is_some() {
            return "delivery truck".into();
        }
        let body = c.model.body.label().replace('_', " ");
        match self.citizens.get(c.citizen) {
            Some(p) => format!("{}'s {body}", p.name),
            None if c.siren || self.services.unit_of_car(id).is_some() => format!("emergency {body}"),
            // [moto] A rider from out of town in a moto pack.
            None if c.drive.gang.is_some_and(|g| g.pack) => format!("a {body} with the moto pack"),
            None => {
                let an = if body.starts_with(['a', 'e', 'i', 'o', 'u']) { "an" } else { "a" };
                match c.errand {
                    // [carlife] Sold, on its way to the dealer or the trade.
                    crate::vehicle::Errand::Sale(_) => format!("a used {body} being sold"),
                    crate::vehicle::Errand::Deliver => format!("{an} {body} being delivered"),
                    crate::vehicle::Errand::Through => format!("{an} {body} passing through"),
                    _ if c.owned.is_some() => format!("{an} {body}"),
                    _ => format!("{an} {body} passing through"),
                }
            }
        }
    }

    /// Drivers seething in traffic right now (anger at least a half).
    pub fn angry_drivers_now(&self) -> usize {
        self.traffic.cars.values().filter(|c| c.drive.anger >= 0.5).count()
    }

    /// Drunk drivers on the road right now.
    pub fn drunk_drivers_now(&self) -> usize {
        self.traffic.cars.values().filter(|c| c.drive.profile.drunk >= DRUNK && !c.crashed()).count()
    }
}
