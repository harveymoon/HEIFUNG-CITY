//! Incidents opening: fires, crimes, crashes and illness reported to 911.

use super::*;

impl World {
    pub(super) fn open_incident(&mut self, kind: IncidentKind, building: BuildingId, citizen: Option<CitizenId>, suspect: Option<CitizenId>, report_delay: u64) -> u32 {
        let tick = self.clock.tick;
        let id = self.services.next_id;
        self.services.next_id += 1;
        let pos = self.building_lot(building).map_or(Vec2::ZERO, |l| l.center);
        let work_left = match kind {
            IncidentKind::Fire => 0.0,
            IncidentKind::Crime => CRIME_WORK,
            IncidentKind::Medical => MEDICAL_WORK,
            IncidentKind::Crash => CRASH_WORK,
        };
        self.services.incidents.push(Incident {
            id,
            kind,
            building,
            pos,
            citizen,
            suspect,
            caller: None,
            dispatcher: None,
            stage: Stage::Unreported,
            started: tick,
            report_at: tick + report_delay,
            reported: None,
            dispatch_at: None,
            dispatched: None,
            arrived: None,
            resolved: None,
            unit: None,
            intensity: if kind == IncidentKind::Fire { 0.15 } else { 0.0 },
            work_left,
            outcome: None,
            place: self.buildings.get(building).map(|b| b.name.clone()).unwrap_or_default(),
            crime: None,
            outside: false,
            arrive_at: None,
            fee: 0.0,
            unit_name: None,
            people: Vec::new(),
            damage: Vec::new(),
            on_road: false,
            chase: None,
            second: None,
            second_alarm: None,
            known_suspect: false,
        });
        // [review] By `slot()`: a crash (index 3) fell off the end of the
        // three counters; it counts with the police, who clear it.
        if let Some(n) = self.services.stats.incidents_today.get_mut(kind.slot()) {
            *n += 1;
        }
        self.events.push(SimEvent::IncidentStarted(id));
        id
    }

    /// Set a building alight. None if it cannot burn or already does.
    pub fn start_fire(&mut self, bid: BuildingId) -> Option<u32> {
        let b = self.buildings.get(bid)?;
        if !b.operating() || b.land_use == LandUse::Park || b.rail.is_some() || self.services.is_burning(bid) {
            return None;
        }
        let occupied = !b.residents.is_empty() || !b.workers.is_empty() || b.visitors > 0;
        let name = b.name.clone();
        let people: Vec<CitizenId> = b.residents.iter().chain(b.workers.iter()).copied().collect();
        let delay = if occupied { self.srng().random_range(2..8) } else { self.srng().random_range(10..40) };
        let tick = self.clock.tick;
        for cid in people {
            if let Some(c) = self.citizens.get_mut(cid) {
                c.remember(tick, format!("{name} caught fire."));
                self.concerns.heard(cid, c, crate::concerns::Concern::Safety, 0.6);
                c.mood = (c.mood - 0.08).max(0.0);
            }
        }
        Some(self.open_incident(IncidentKind::Fire, bid, None, None, delay))
    }

    /// Hook for a crime that something else decided on (a criminal citizen,
    /// a gang): what, where, who did it and to whom. Somebody calls it in a
    /// few minutes later and the police answer like any other job. Returns
    /// the incident id; arrests show as `Outcome::Arrest`, `SimEvent::Arrested`
    /// and `Services::arrests_of`.
    pub fn report_crime(&mut self, kind: CrimeKind, at: BuildingId, suspect: Option<CitizenId>, victim: Option<CitizenId>) -> Option<u32> {
        if !self.buildings.contains_key(at) {
            return None;
        }
        let delay = self.srng().random_range(3..20);
        let id = self.open_incident(IncidentKind::Crime, at, victim, suspect, delay);
        let names: Vec<(CitizenId, String, &str)> = [(victim, "victim"), (suspect, "suspect")].into_iter().filter_map(|(c, role)| c.and_then(|c| self.citizens.get(c).map(|x| (c, x.name.clone(), role)))).collect();
        if let Some(i) = self.services.incident_mut(id) {
            i.crime = Some(kind);
            if kind == CrimeKind::Homicide {
                i.work_left = HOMICIDE_WORK;
            }
            for (c, n, role) in names {
                i.involve(c, &n, role);
            }
        }
        Some(id)
    }

    /// [driving] A bad crash on the road at `at`: the police come to clear
    /// it (and arrest `at_fault`, if given, when they can), and every one of
    /// `injured` waits there for an ambulance. `near` names the place.
    /// `caller` is whoever saw it and calls 911. Returns the crash incident
    /// and the medical ones.
    pub fn report_crash(&mut self, at: Vec2, near: BuildingId, drivers: &[CitizenId], injured: &[CitizenId], at_fault: Option<CitizenId>, caller: Option<CitizenId>) -> Option<(u32, Vec<u32>)> {
        if !self.buildings.contains_key(near) {
            return None;
        }
        // Out past the edge of the map (the highway runs on for kilometres)
        // a crash is the next town's business: their police and ambulances
        // take it, and it is never a call of ours. The hurt are patched up
        // and go on; the wreck is towed in its own time.
        if self.landscape.beyond_map_edge(at, 0.0) {
            let tick = self.clock.tick;
            for &p in injured {
                if let Some(c) = self.citizens.get_mut(p) {
                    c.remember(tick, "Crashed out past the city. The next town's paramedics patched me up.");
                    c.next_decision_tick = c.next_decision_tick.min(tick + 60);
                }
            }
            return None;
        }
        // A crash where a crash scene is already open (the same junction, a
        // pile-up) joins that scene: one police car clears both wrecks.
        let joined = self.services.active().filter(|i| i.kind == IncidentKind::Crash && i.pos.distance(at) < CRASH_SCENE_JOIN).map(|i| i.id).next();
        let delay = self.srng().random_range(1..5);
        let id = match joined {
            Some(id) => id,
            None => self.open_incident(IncidentKind::Crash, near, None, at_fault, delay),
        };
        let work = if at_fault.is_some() {
            CRASH_WORK_ARREST
        } else if !injured.is_empty() {
            CRASH_WORK
        } else {
            CRASH_WORK_CLEAR
        };
        if let Some(i) = self.services.incident_mut(id) {
            // A joined scene takes as long as its longest case.
            i.work_left = if joined.is_some() { i.work_left.max(work) } else { work };
            if joined.is_some() && i.suspect.is_none() {
                i.suspect = at_fault;
            }
        }
        let names: Vec<(CitizenId, String, &str)> = drivers
            .iter()
            .map(|&c| (c, if Some(c) == at_fault { "driver at fault" } else { "driver" }))
            .chain(injured.iter().map(|&c| (c, "injured")))
            .filter_map(|(c, role)| self.citizens.get(c).map(|x| (c, x.name.clone(), role)))
            .collect();
        if let Some(i) = self.services.incident_mut(id) {
            if joined.is_none() {
                i.pos = at;
                i.on_road = true;
                i.caller = caller;
            }
            for (c, n, role) in names {
                i.involve(c, &n, role);
            }
        }
        let mut medical = Vec::new();
        for &p in injured {
            let Some(name) = self.citizens.get(p).map(|c| c.name.clone()) else { continue };
            let m = self.open_incident(IncidentKind::Medical, near, Some(p), None, delay);
            if let Some(i) = self.services.incident_mut(m) {
                i.pos = at;
                i.on_road = true;
                i.caller = caller;
                i.involve(p, &name, "patient");
            }
            medical.push(m);
        }
        Some((id, medical))
    }

    /// A crime at a building: a victim from whoever is there, a suspect
    /// from the town's idle and unhappy, if any.
    pub fn start_crime(&mut self, bid: BuildingId) -> Option<u32> {
        let b = self.buildings.get(bid)?;
        if self.services.active().any(|i| i.kind == IncidentKind::Crime && i.building == bid) {
            return None;
        }
        let name = b.name.clone();
        let regulars: Vec<CitizenId> = b.residents.iter().chain(b.workers.iter()).copied().collect();
        let present: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| c.inside == Some(bid)).map(|(id, _)| id).collect();
        let pool: Vec<CitizenId> = if present.is_empty() { regulars } else { present };
        let victim = pool.choose(self.srng()).copied();
        let suspects: Vec<CitizenId> = self.citizens.iter().filter(|(id, c)| c.work.is_none() && c.can_work() && c.mood < 0.5 && Some(*id) != victim && !self.services.in_custody(*id)).map(|(id, _)| id).collect();
        let suspect = if self.srng().random_bool(0.7) { suspects.choose(self.srng()).copied() } else { None };
        let tick = self.clock.tick;
        if let Some(v) = victim.and_then(|v| self.citizens.get_mut(v)) {
            v.remember(tick, format!("Got robbed at {name}."));
            self.concerns.heard(victim.unwrap_or_default(), v, crate::concerns::Concern::Safety, 0.6);
            v.mood = (v.mood - 0.1).max(0.0);
            v.thought = "Somebody just robbed me.".to_string();
        }
        let kind = *[CrimeKind::Robbery, CrimeKind::Theft, CrimeKind::Assault, CrimeKind::Vandalism].choose(self.srng()).unwrap();
        self.report_crime(kind, bid, suspect, victim)
    }

    /// A citizen indoors is taken ill and stays put until help comes.
    pub fn fall_ill(&mut self, cid: CitizenId) -> Option<u32> {
        let c = self.citizens.get(cid)?;
        let bid = c.inside?;
        if !self.buildings.contains_key(bid) || self.is_patient(cid) {
            return None;
        }
        let tick = self.clock.tick;
        let delay = self.srng().random_range(1..6);
        let c = &mut self.citizens[cid];
        c.thought = "I don't feel well at all.".to_string();
        c.next_decision_tick = c.next_decision_tick.max(tick + MEDICAL_GIVE_UP + 120);
        c.mood = (c.mood - 0.05).max(0.0);
        let name = self.citizens[cid].name.clone();
        let id = self.open_incident(IncidentKind::Medical, bid, Some(cid), None, delay);
        if let Some(i) = self.services.incident_mut(id) {
            i.involve(cid, &name, "patient");
        }
        Some(id)
    }

    /// Ill and waiting, or in a hospital bed.
    pub fn is_patient(&self, cid: CitizenId) -> bool {
        self.services.active().any(|i| i.kind == IncidentKind::Medical && i.citizen == Some(cid)) || self.services.admissions.iter().any(|a| a.citizen == cid)
    }
}
