//! Buildings coming down: the city taking a lot, demolition, and the people
//! and vehicles it leaves stranded.

use super::*;

impl World {
    /// One of the city's own structures ([`Building::is_city_structure`]):
    /// never taken over by another placement; the player bulldozes it first.
    pub(in crate::world) fn is_civic_structure(b: &Building) -> bool {
        b.is_city_structure()
    }

    /// The city takes a lot for one of its own structures: whatever grew
    /// there (homes, a shop, a site still going up) is bought out and pulled
    /// down, its residents and staff move on with a memory of it. Another
    /// civic structure is not replaced: that is an error the player sees.
    pub(in crate::world) fn take_over_lot(&mut self, lot_id: LotId, for_what: &str) -> Result<(), String> {
        let Some(bid) = self.lots.get(lot_id).and_then(|l| l.building) else { return Ok(()) };
        let Some(b) = self.buildings.get(bid) else { return Ok(()) };
        if Self::is_civic_structure(b) {
            return Err(format!("{} stands there; bulldoze it first", b.name));
        }
        let memory = format!("The city bought out {} to build a {for_what}. Had to move on.", b.name);
        // [relocate] The business looks for a new place.
        self.pack_business(bid, &format!("the city's new {for_what}"));
        self.buy_out(bid, lot_id, None, &memory);
        Ok(())
    }

    /// [review] The city buys a grown building out and pulls it down (its
    /// business already packed up to move): the `Expropriated` event, then
    /// the demolition, the residents remembering `memory`. `lot` is the key
    /// the event names; `at` where it stood when its lot is already gone
    /// (`demolish_with`). The one way both a take-over and the road works'
    /// eminent domain end.
    pub(in crate::world) fn buy_out(&mut self, bid: BuildingId, lot: LotId, at: Option<Vec2>, memory: &str) {
        if self.buildings.contains_key(bid) {
            self.events.push(SimEvent::Expropriated(bid, lot));
        }
        self.demolish_with(bid, at, Some(memory));
    }

    pub(in crate::world) fn demolish(&mut self, bid: BuildingId) {
        self.demolish_with(bid, None, None);
    }

    /// Take a building down. `at` is where it stood when its lot is already
    /// gone; `memory` replaces the residents' default note about it.
    pub(in crate::world) fn demolish_with(&mut self, bid: BuildingId, at: Option<Vec2>, memory: Option<&str>) {
        // Nothing vanishes: it becomes a demolition site and is cleared over
        // the next days (see `construction.rs`), unless it must go at once.
        if self.begin_demolition(bid, at, memory) {
            return;
        }
        let city = self.is_city_building(bid);
        let Some(b) = self.buildings.remove(bid) else { return };
        if at.is_none() && self.lots.get(b.lot).is_some_and(|l| l.building == Some(bid)) {
            self.lots[b.lot].building = None;
            // [free-placement] Its ground goes back to the plan.
            if city {
                self.release_city_lot(b.lot, true);
            }
        }
        let tick = self.clock.tick;
        // [review] During a rebuild `b.lot` is an old key naming some other
        // lot: where it stood (`at`) comes first.
        let center = at.or_else(|| self.lots.get(b.lot).map(|l| l.center)).unwrap_or(Vec2::ZERO);
        for cid in b.residents.iter().copied() {
            if let Some(c) = self.citizens.get_mut(cid) {
                if c.home == Some(bid) {
                    c.home = None;
                    match memory {
                        Some(m) => c.remember(tick, m.to_string()),
                        None => c.remember(tick, format!("{} was demolished. Homeless again.", b.name)),
                    }
                    self.concerns.heard(cid, c, crate::concerns::Concern::Displacement, 0.8);
                }
            }
        }
        // Households too, not only their members: the household's home
        // would otherwise dangle until the hourly `sync_households`.
        for hid in b.households.iter().copied() {
            if let Some(h) = self.households.get_mut(hid) {
                if h.home == Some(bid) {
                    h.home = None;
                }
            }
        }
        for cid in b.workers.iter().copied() {
            if self.citizens.get(cid).is_some_and(|c| c.work == Some(bid)) {
                self.lay_off(cid, None);
            }
        }
        let inside: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| c.inside == Some(bid)).map(|(id, _)| id).collect();
        for cid in inside {
            let c = &mut self.citizens[cid];
            c.inside = None;
            c.pos = center;
            c.activity = Activity::Idle;
        }
        if b.rail.is_some() {
            for f in self.facilities.values_mut() {
                if f.building == Some(bid) {
                    f.building = None;
                }
            }
            self.refresh_facility_cache();
        }
        // [land] Extra plots freed, a bought plot handed over. Its old key
        // names nothing (or another lot) once the lots are recut.
        let lot = if at.is_none() { b.lot } else { LotId::default() };
        self.events.push(SimEvent::BuildingRemoved(bid, lot));
        self.estate_building_gone(bid, &b.plot, lot);
    }

    /// Someone loses their job: back to gig money, with a note if there is
    /// a story to it.
    pub(in crate::world) fn lay_off(&mut self, cid: CitizenId, memory: Option<String>) {
        let tick = self.clock.tick;
        if let Some(c) = self.citizens.get_mut(cid) {
            c.work = None;
            c.income = GIG_INCOME;
            if let Some(m) = memory {
                // [concerns] A shop going under hurts; a site crew moving on is routine.
                let s = if m.contains("shut down") { 0.6 } else { 0.15 };
                c.remember(tick, m);
                self.concerns.heard(cid, c, crate::concerns::Concern::Jobs, s);
            }
        }
    }

    /// Personal skill factor, fixed per person, spreading incomes within a job.
    pub(in crate::world) fn skill_of(seed: u64) -> f32 {
        0.75 + (seed % 1000) as f32 / 1000.0 * 0.6
    }

    /// [review] What a job at `work` pays someone with this seed: the wage
    /// times their skill (before any school bonus, `daily_services`).
    pub(in crate::world) fn base_income(work: &Building, seed: u64) -> f32 {
        work.wage() * Self::skill_of(seed)
    }

    /// A car taken off the road: its driver, or a bus's riders, stand there.
    pub(in crate::world) fn strand_car(&mut self, car: crate::traffic::Car) {
        use crate::vehicle::{CarEvent, Errand};
        // A truck taken off the road still delivers: the buyer already counts
        // the load as inbound and would never reorder it otherwise.
        if let Some(cargo) = car.cargo {
            self.deliver(cargo);
        }
        // [carlife] Trucks and vans are back at their base; private cars
        // park where they stood (cars on their way to be sold or out of
        // town are gone, as they were about to be).
        match car.errand {
            Errand::Haul { base: Some(b) } | Errand::Return(b) if self.buildings.contains_key(b) => {
                *self.vehicles.fleet_parked.entry(b).or_insert(0) += 1;
            }
            _ => {}
        }
        if car.cargo.is_some() {
            return;
        }
        if let Some(vid) = car.owned.filter(|v| self.vehicles.cars.contains_key(*v)) {
            match car.errand {
                Errand::Sale(_) => self.drop_car(vid, CarEvent::Sold),
                Errand::Leave => self.drop_car(vid, CarEvent::Emigrated),
                _ => self.park_stranded(vid, car.pos),
            }
        }
        if let Some(bus) = &car.bus {
            for &p in &bus.passengers {
                if let Some(c) = self.citizens.get_mut(p) {
                    c.transit = None;
                }
                self.strand_driver(p, car.pos);
            }
        } else {
            self.strand_driver(car.citizen, car.pos);
        }
    }

    /// Whoever was in a vehicle that vanished (network rebuilt, bad save)
    /// stands where it was and decides again.
    pub(in crate::world) fn strand_driver(&mut self, cid: CitizenId, pos: Vec2) {
        // [ped-highway] Out of the vehicle onto the pavement, never the
        // carriageway of a highway or ramp.
        let pos = self.kerbside(pos);
        if let Some(c) = self.citizens.get_mut(cid) {
            c.pos = pos;
            c.inside = None;
            c.activity = Activity::Idle;
            c.pending_footfall.clear();
            c.next_decision_tick = self.clock.tick + 1;
        }
    }
}
