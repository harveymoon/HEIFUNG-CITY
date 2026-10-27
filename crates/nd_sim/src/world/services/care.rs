//! After the call: patients treated or taken to hospital, suspects taken
//! into custody, and incidents resolved.

use super::*;

impl World {
    /// Work on scene is done (crimes and patients; fires end when out).
    pub(super) fn finish_scene(&mut self, id: u32) {
        let tick = self.clock.tick;
        let Some(inc) = self.services.incident(id).cloned() else { return };
        match inc.kind {
            IncidentKind::Crime => {
                // [emergency] A suspect who can get to a car may run for it.
                if self.try_chase(id) {
                    return;
                }
                let quick = inc.arrived.unwrap_or(tick).saturating_sub(inc.started) <= QUICK_RESPONSE;
                let homicide = inc.crime == Some(CrimeKind::Homicide);
                // [murder] A killing is investigated, not just answered: the
                // odds follow whether the killer is known, not the response time.
                let odds = match (homicide, inc.known_suspect, quick) {
                    (true, true, _) => HOMICIDE_ARREST_KNOWN,
                    (true, false, _) => HOMICIDE_ARREST_UNKNOWN,
                    (false, _, true) => ARREST_QUICK,
                    (false, _, false) => ARREST_SLOW,
                };
                let caught = inc.suspect.is_some() && self.srng().random::<f32>() < odds;
                let bname = self.buildings.get(inc.building).map(|b| b.name.clone()).unwrap_or_default();
                if caught {
                    if let Some(s) = inc.suspect.and_then(|s| self.citizens.get_mut(s)) {
                        s.remember(tick, format!("Got picked up by the cops over the job at {bname}."));
                        s.mood = (s.mood - 0.2).max(0.0);
                        s.thought = "Busted.".to_string();
                    }
                    if let Some(v) = inc.citizen.and_then(|v| self.citizens.get_mut(v)) {
                        v.remember(tick, "The police caught whoever robbed me.");
                    }
                    self.services.stats.arrests_today += 1;
                    if let Some(s) = inc.suspect {
                        self.take_into_custody(id, s);
                    }
                    if homicide {
                        self.homicide_solved(&inc);
                    }
                    self.resolve(id, Outcome::Arrest);
                } else {
                    self.resolve(id, Outcome::Escaped);
                }
            }
            IncidentKind::Medical => {
                let hospital = self.hospital_with_bed(inc.pos);
                let uid = inc.unit;
                match (hospital, uid) {
                    (Some(h), Some(uid)) => {
                        let to = self.building_lot(h).and_then(|l| self.lot_access(l));
                        if to.is_some_and(|to| self.drive_unit(uid, to, true)) {
                            let u = &mut self.services.units[uid];
                            u.state = UnitState::Transporting;
                            u.hospital = Some(h);
                            self.services.incident_mut(id).unwrap().stage = Stage::Transporting;
                        } else {
                            self.treat_on_site(id);
                        }
                    }
                    (Some(h), None) if inc.outside => {
                        if let Some(p) = inc.citizen {
                            self.admit(id, p, h);
                        }
                    }
                    _ => self.treat_on_site(id),
                }
            }
            IncidentKind::Fire => {}
            IncidentKind::Crash => {
                // A drunk or a red-light runner who is not in an ambulance
                // may be taken in; the wrecks are towed (`World::step_driving`).
                let suspect = inc.suspect.filter(|&s| self.citizens.contains_key(s) && !self.is_patient(s) && !self.services.in_custody(s));
                if suspect.is_some() && self.srng().random::<f32>() < DUI_ARREST {
                    let s = suspect.unwrap();
                    let c = &mut self.citizens[s];
                    c.remember(tick, "Arrested at the scene of my crash.");
                    c.mood = (c.mood - 0.2).max(0.0);
                    c.thought = "Should never have got behind the wheel.".to_string();
                    self.services.stats.arrests_today += 1;
                    self.take_into_custody(id, s);
                    self.resolve(id, Outcome::Arrest);
                } else {
                    self.resolve(id, Outcome::Cleared);
                }
            }
        }
    }

    pub(in crate::world) fn treat_on_site(&mut self, id: u32) {
        let tick = self.clock.tick;
        if let Some(c) = self.services.incident(id).and_then(|i| i.citizen).and_then(|c| self.citizens.get_mut(c)) {
            c.remember(tick, "Patched up by paramedics where I fell. No bed free anywhere.");
            c.next_decision_tick = tick + 60;
        }
        if let Some(cid) = self.services.incident(id).and_then(|i| i.citizen) {
            if let Some(c) = self.citizens.get(cid) {
                self.concerns.heard(cid, c, crate::concerns::Concern::Services, 0.5);
            }
        }
        self.resolve(id, Outcome::TreatedOnSite);
    }

    /// The nearest hospital with a free bed.
    pub(super) fn hospital_with_bed(&self, p: Vec2) -> Option<BuildingId> {
        self.buildings
            .iter()
            .filter_map(|(id, b)| b.service.filter(|s| s.kind == ServiceKind::Health).map(|s| (id, b, s)))
            .filter(|(_, b, _)| !b.is_derelict() && b.operating())
            .filter(|(id, _, s)| (self.services.admissions.iter().filter(|a| a.hospital == *id).count() as u32) < s.kind.capacity(s.tier))
            .filter_map(|(id, b, _)| self.lots.get(b.lot).map(|l| (id, l.center.distance(p))))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// The ambulance is at the hospital door: the patient goes to a bed.
    pub(super) fn deliver_patient(&mut self, uid: UnitId) {
        let Some((inc, hospital)) = self.services.units.get(uid).map(|u| (u.incident, u.hospital)) else { return };
        let patient = inc.and_then(|i| self.services.incident(i)).and_then(|i| i.citizen);
        match (inc, hospital, patient) {
            (Some(id), Some(h), Some(p)) if self.buildings.contains_key(h) && self.citizens.contains_key(p) => self.admit(id, p, h),
            (Some(id), _, _) => self.resolve(id, Outcome::Gone),
            _ => {}
        }
        self.send_home(uid);
    }

    /// A patient into a hospital bed; the household is billed.
    pub(in crate::world) fn admit(&mut self, id: u32, p: CitizenId, h: BuildingId) {
        let tick = self.clock.tick;
        {
            {
                self.leave_building(p);
                let entrance = self.building_entrance(h).unwrap_or(Vec2::ZERO);
                let hours = self.srng().random_range(STAY_HOURS.0..=STAY_HOURS.1);
                let until = tick + hours * 60;
                let hname = self.buildings[h].name.clone();
                self.buildings[h].visitors += 1;
                let c = &mut self.citizens[p];
                c.inside = Some(h);
                c.activity = Activity::Visiting(h);
                c.pos = entrance;
                c.prev_pos = entrance;
                c.next_decision_tick = until;
                c.thought = "Hooked up to a drip. At least it's warm.".to_string();
                c.remember(tick, format!("The ambulance took me to {hname}."));
                self.services.admissions.push(Admission { citizen: p, hospital: h, until });
                // The household pays what it can; the city covers the rest.
                let paid = match self.citizens[p].household.filter(|hh| self.households.contains_key(*hh)) {
                    Some(hh) => {
                        let pay = self.households[hh].money.clamp(0.0, HOSPITAL_BILL);
                        self.pay_city(Account::Household(hh), Flow::ServiceUpkeep, pay, Some(Item::Named("hospital bills paid by patients".into())))
                    }
                    None => 0.0,
                };
                self.services.stats.bills_paid_today += paid;
                self.services.stats.bills_covered_today += HOSPITAL_BILL - paid;
                if let Some(i) = self.services.incident_mut(id) {
                    i.taken(p, &hname);
                }
                self.resolve(id, Outcome::Admitted);
            }
        }
    }

    /// An arrested suspect goes to a jail with room for a sentence, or is
    /// held at the nearest police station for a few hours and let go.
    pub(in crate::world) fn take_into_custody(&mut self, id: u32, s: CitizenId) {
        let tick = self.clock.tick;
        if !self.citizens.contains_key(s) {
            return;
        }
        let at = self.services.incident(id).map_or(Vec2::ZERO, |i| i.pos);
        let occupancy = self.services.jail_occupancy();
        let nearest = |w: &World, kind: ServiceKind, room: bool| -> Option<BuildingId> {
            w.buildings
                .iter()
                .filter_map(|(bid, b)| b.service.filter(|x| x.kind == kind).map(|x| (bid, b, x)))
                .filter(|(_, b, _)| !b.is_derelict() && b.operating())
                .filter(|(bid, _, x)| !room || occupancy.iter().find(|o| o.0 == *bid).map_or(0, |o| o.1) < x.kind.capacity(x.tier))
                .filter_map(|(bid, b, _)| w.lots.get(b.lot).map(|l| (bid, l.center.distance(at))))
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(bid, _)| bid)
        };
        let (place, jailed, until) = match nearest(self, ServiceKind::Jail, true) {
            Some(j) => {
                let range = if self.services.incident(id).is_some_and(|i| i.crime == Some(CrimeKind::Homicide)) { HOMICIDE_SENTENCE_DAYS } else { SENTENCE_DAYS };
                let days = self.srng().random_range(range.0..=range.1);
                (Some(j), true, tick + days * 1440)
            }
            None => (nearest(self, ServiceKind::Police, false), false, tick + HELD_HOURS * 60),
        };
        match self.services.record.iter_mut().find(|r| r.0 == s) {
            Some(r) => r.1 += 1,
            None => self.services.record.push((s, 1)),
        }
        self.events.push(SimEvent::Arrested(id, s));
        let Some(place) = place else {
            // No station to hold them at: a caution on the spot.
            if let Some(i) = self.services.incident_mut(id) {
                i.taken(s, "let go with a caution");
            }
            return;
        };
        self.leave_building(s);
        let entrance = self.building_entrance(place).unwrap_or(at);
        let pname = self.buildings[place].name.clone();
        self.buildings[place].visitors += 1;
        let c = &mut self.citizens[s];
        c.inside = Some(place);
        c.activity = Activity::Visiting(place);
        c.pos = entrance;
        c.prev_pos = entrance;
        c.next_decision_tick = until;
        c.thought = if jailed { "Four walls and a bunk.".to_string() } else { "They'll have to let me go soon.".to_string() };
        c.remember(tick, if jailed { format!("Locked up in {pname}.") } else { format!("Held at {pname}. No jail to send me to.") });
        self.services.custody.push(Custody { citizen: s, at: place, jailed, until });
        if let Some(i) = self.services.incident_mut(id) {
            i.taken(s, &if jailed { format!("{pname} (jail)") } else { format!("{pname} (held)") });
        }
    }

    /// Custody runs out, or the person is somewhere else: they walk free.
    /// Prisoners are fed.
    pub(super) fn step_custody(&mut self) {
        let tick = self.clock.tick;
        let mut out = Vec::new();
        for (k, c) in self.services.custody.iter().enumerate() {
            let here = self.citizens.get(c.citizen).is_some_and(|x| x.inside == Some(c.at));
            if !here || tick >= c.until {
                out.push(k);
            }
        }
        for k in out.into_iter().rev() {
            let c = self.services.custody.remove(k);
            if !c.jailed {
                self.services.stats.released_early_total += 1;
            }
            if let Some(x) = self.citizens.get_mut(c.citizen) {
                if x.inside == Some(c.at) {
                    x.remember(tick, if c.jailed { "Did my time. Out." } else { "Let go. No jail in this town." });
                    x.next_decision_tick = tick + 1;
                }
            }
            self.events.push(SimEvent::Released(c.citizen));
        }
        for c in &self.services.custody {
            if let Some(x) = self.citizens.get_mut(c.citizen) {
                x.satiety = x.satiety.max(0.6);
                x.next_decision_tick = x.next_decision_tick.max(c.until);
            }
        }
        let s = &mut self.services.stats;
        s.jailed = self.services.custody.iter().filter(|c| c.jailed).count() as u32;
        s.held_at_stations = self.services.custody.iter().filter(|c| !c.jailed).count() as u32;
    }

    /// Close an incident. The unit on scene, if any, heads home.
    pub(in crate::world) fn resolve(&mut self, id: u32, outcome: Outcome) {
        let tick = self.clock.tick;
        let Some(inc) = self.services.incident_mut(id) else { return };
        if !inc.active() {
            return;
        }
        inc.stage = Stage::Resolved;
        inc.resolved = Some(tick);
        inc.outcome = Some(outcome);
        let (kind, unit, citizen, bid, answered, second) = (inc.kind, inc.unit, inc.citizen, inc.building, inc.arrived.is_some(), inc.second);
        if let Some(n) = self.services.stats.resolved_today.get_mut(kind.slot()) {
            *n += 1;
        }
        if !answered && !matches!(outcome, Outcome::Gone) {
            self.services.stats.unanswered_today += 1;
        }
        match outcome {
            Outcome::Unsolved => {
                if let Some(c) = citizen.and_then(|c| self.citizens.get_mut(c)) {
                    c.remember(tick, "Nobody came about the robbery. Nobody ever does.");
                    self.concerns.heard(citizen.unwrap_or_default(), c, crate::concerns::Concern::Services, 0.6);
                    c.mood = (c.mood - 0.08).max(0.0);
                }
            }
            Outcome::Recovered => {
                if let Some(c) = citizen.and_then(|c| self.citizens.get_mut(c)) {
                    c.remember(tick, "Waited hours for an ambulance that never came. Pulled through on my own.");
                    self.concerns.heard(citizen.unwrap_or_default(), c, crate::concerns::Concern::Services, 0.8);
                    c.mood = (c.mood - 0.15).max(0.0);
                    c.next_decision_tick = tick + 30;
                }
            }
            Outcome::Gone => {
                if let Some(c) = citizen.and_then(|c| self.citizens.get_mut(c)) {
                    c.next_decision_tick = c.next_decision_tick.min(tick + 30);
                }
            }
            Outcome::Extinguished => {
                let name = self.buildings.get(bid).map(|b| b.name.clone()).unwrap_or_default();
                let people: Vec<CitizenId> = self.buildings.get(bid).map(|b| b.residents.iter().chain(b.workers.iter()).copied().collect()).unwrap_or_default();
                for p in people {
                    if let Some(c) = self.citizens.get_mut(p) {
                        c.remember(tick, format!("They put the fire out at {name}."));
                    }
                }
            }
            _ => {}
        }
        for uid in [unit, second.filter(|s| Some(*s) != unit)].into_iter().flatten() {
            let u = self.services.units.get(uid).filter(|u| u.incident == Some(id)).map(|u| u.state);
            if u == Some(UnitState::OnScene) && kind == IncidentKind::Fire {
                // [fire-scene] The crew packs up first (`step_packing`).
                self.services.units[uid].packing_until = tick + PACK_UP_TICKS;
            } else if u == Some(UnitState::OnScene) {
                self.send_home(uid);
            } else if u == Some(UnitState::Responding) {
                // Called off on the way: free at once, from where it is.
                self.pull_off_road(uid);
                self.send_home(uid);
            }
        }
        self.services.route_failed.remove(&id);
        self.events.push(SimEvent::IncidentResolved(id));
    }
}
