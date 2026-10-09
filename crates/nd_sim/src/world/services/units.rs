//! Units on the road: patrols, returning to the station, units lost or
//! leaving town, arriving on scene, driving and packing up.

use super::*;

impl World {
    /// Take a unit's car off the road, leaving the unit where the car was.
    pub(in crate::world) fn pull_off_road(&mut self, uid: UnitId) {
        let Some(c) = self.services.units.get_mut(uid).and_then(|u| u.car.take()) else { return };
        if let Some(car) = self.traffic.remove_car(c) {
            let u = &mut self.services.units[uid];
            u.pos = car.pos;
            u.heading = car.heading;
        }
        self.services.by_car.remove(&c);
    }

    /// Police cars patrol and ambulances post at standby points while free;
    /// a parked unit moves to its next beat when its stay is up. A unit
    /// whose station has lost the crew for it goes home.
    pub(super) fn step_patrols(&mut self) {
        let tick = self.clock.tick;
        let ids: Vec<UnitId> = self.services.units.iter().filter(|(_, u)| matches!(u.kind, UnitKind::PoliceCar | UnitKind::Ambulance)).map(|(id, _)| id).collect();
        for uid in ids {
            let out = self.units_out();
            let u = &self.services.units[uid];
            let (station, state, parked, until) = (u.station, u.state, u.car.is_none(), u.patrol_until);
            let crewed = self.crewed_units(station);
            let n_out = out.get(&station).copied().unwrap_or(0);
            match state {
                UnitState::Idle if n_out < crewed => {
                    self.start_patrol(uid);
                }
                UnitState::Patrol if n_out > crewed => self.return_to_station(uid),
                UnitState::Patrol if parked && tick >= until => {
                    if !self.start_patrol(uid) {
                        self.return_to_station(uid);
                    }
                }
                _ => {}
            }
        }
    }

    /// Drive to a new beat within the station's district (police: crime
    /// hot spots and shopping streets; ambulances: where people live), away
    /// from the kind's other units on patrol. False if there is nowhere to go.
    pub(super) fn start_patrol(&mut self, uid: UnitId) -> bool {
        let Some(u) = self.services.units.get(uid) else { return false };
        let (station, kind) = (u.station, u.kind);
        let here = u.car.and_then(|c| self.traffic.cars.get(c)).map_or(u.pos, |c| c.pos);
        let Some((centre, radius)) = self.buildings.get(station).and_then(|b| Some((self.lots.get(b.lot)?.center, b.service.map(|s| s.kind.radius(s.tier))?))) else { return false };
        let reach = radius * PATROL_REACH;
        let others: Vec<Vec2> = self
            .services
            .units
            .iter()
            .filter(|(id, o)| *id != uid && o.kind == kind && o.state == UnitState::Patrol)
            .map(|(_, o)| o.car.and_then(|c| self.traffic.cars.get(c)).map_or(o.pos, |c| c.pos))
            .collect();
        // Where this kind of call came from over the last two days.
        let since = self.clock.tick.saturating_sub(2 * 1440);
        let recent: Vec<Vec2> = self.services.incidents.iter().filter(|i| i.kind.unit() == kind && i.started >= since).map(|i| i.pos).collect();
        let mut cands: Vec<(f32, LotId)> = Vec::new();
        for b in self.buildings.values() {
            if !b.operating() || b.land_use == LandUse::Park {
                continue;
            }
            let Some(l) = self.lots.get(b.lot) else { continue };
            if l.center.distance(centre) > reach || l.center.distance(here) < 60.0 {
                continue;
            }
            let calls = recent.iter().filter(|p| p.distance(l.center) < 160.0).count() as f32;
            let base = match kind {
                UnitKind::PoliceCar => 1.0 + 4.0 * self.society.crime.heat_at(l.center, 120.0) + 2.0 * calls + if matches!(b.land_use, LandUse::Commercial | LandUse::Entertainment) { 1.0 } else { 0.0 },
                _ => 0.2 + b.residents.len() as f32 / 10.0 + 2.0 * calls,
            };
            let crowd = others.iter().filter(|p| p.distance(l.center) < 200.0).count() as f32;
            cands.push((base / (1.0 + 3.0 * crowd), b.lot));
        }
        let total: f32 = cands.iter().map(|c| c.0).sum();
        if cands.is_empty() || total <= 0.0 {
            return false;
        }
        let mut roll = self.srng().random::<f32>() * total;
        let mut pick = cands[cands.len() - 1].1;
        for (w, lot) in &cands {
            roll -= w;
            if roll <= 0.0 {
                pick = *lot;
                break;
            }
        }
        let Some(to) = self.lots.get(pick).and_then(|l| self.lot_access(l)) else { return false };
        self.pull_off_road(uid);
        if !self.drive_unit(uid, to, false) {
            return false;
        }
        let u = &mut self.services.units[uid];
        u.state = UnitState::Patrol;
        u.incident = None;
        u.hospital = None;
        true
    }

    /// Off patrol and back to the station.
    pub(super) fn return_to_station(&mut self, uid: UnitId) {
        if self.services.units.get(uid).is_some_and(|u| !self.station_alive(u.station)) {
            self.unit_leaves_town(uid);
            return;
        }
        self.pull_off_road(uid);
        let Some(u) = self.services.units.get_mut(uid) else { return };
        u.incident = None;
        u.hospital = None;
        let station = u.station;
        let to = self.building_lot(station).and_then(|l| self.lot_access(l));
        match to {
            Some(to) if self.drive_unit(uid, to, false) => self.services.units[uid].state = UnitState::Returning,
            _ => self.park_at_station(uid),
        }
    }

    /// A unit whose car vanished (roads rebuilt, a save loaded) carries on
    /// as best it can: a responder is sent again, a returning unit is home,
    /// a patient on the way is delivered.
    pub(super) fn recover_lost_units(&mut self) {
        let lost: Vec<UnitId> = self.services.units.iter().filter(|(_, u)| u.car.is_some_and(|c| !self.traffic.cars.contains_key(c))).map(|(id, _)| id).collect();
        for uid in lost {
            // Leaving town with no station: its car was taken off the road
            // (a rebuild), and with it the unit.
            if self.services.units[uid].leaving {
                self.services.units.remove(uid);
                continue;
            }
            let state = {
                let u = &mut self.services.units[uid];
                u.car = None;
                u.state
            };
            match state {
                UnitState::Responding => {
                    let inc = self.services.units[uid].incident.take();
                    self.park_at_station(uid);
                    // [fire-scene] A second engine lost on the way: the fire
                    // carries on with the first.
                    if let Some(i) = inc.and_then(|i| self.services.incident_mut(i)).filter(|i| i.second == Some(uid)) {
                        i.second = None;
                        continue;
                    }
                    if let Some(i) = inc.and_then(|i| self.services.incident_mut(i)) {
                        i.stage = Stage::Reported;
                        i.unit = None;
                        i.dispatch_at = Some(self.clock.tick);
                    }
                }
                UnitState::Transporting => {
                    let at = self.services.units[uid].hospital.and_then(|h| self.building_entrance(h));
                    if let Some(p) = at {
                        self.services.units[uid].pos = p;
                    }
                    self.deliver_patient(uid);
                }
                _ => self.park_at_station(uid),
            }
        }
    }

    /// The station still stands as a service building.
    pub(super) fn station_alive(&self, station: BuildingId) -> bool {
        self.buildings.get(station).is_some_and(|b| b.service.is_some())
    }

    /// [services] Unit `uid`'s station is gone (bulldozed, or replaced). A
    /// crew on scene or carrying a patient finishes the job first (`send_home`
    /// then sees no station and sends it off); one still on its way hands
    /// the call back to the queue for another unit; the rest leave town.
    /// They used to be deleted with their car, and an incident still naming
    /// the unit crashed the tick (`invalid SlotMap key`) when its scene work
    /// finished.
    pub(super) fn station_lost(&mut self, uid: UnitId) {
        let Some(u) = self.services.units.get(uid) else { return };
        if u.leaving {
            // Still parked where it could find no road out: try again.
            if u.car.is_none() {
                self.unit_leaves_town(uid);
            }
            return;
        }
        let job = u.incident.filter(|&i| self.services.incident(i).is_some_and(|x| x.active() && x.unit == Some(uid)));
        match (u.state, job) {
            (UnitState::OnScene | UnitState::Transporting, Some(_)) => {}
            (UnitState::Responding, Some(i)) => {
                self.requeue(i);
                self.unit_leaves_town(uid);
            }
            _ => self.unit_leaves_town(uid),
        }
    }

    /// [services] A unit with no station left drives off the map by the
    /// nearest exit (`unit_car_arrived` takes it out of the world there),
    /// without lights, taking no calls. A unit standing still (parked in the
    /// station, on scene, on a beat) is put on the street beside it. With
    /// no road out it waits and tries again (`station_lost`), except a unit
    /// that was parked inside its station: that goes with the building.
    pub(super) fn unit_leaves_town(&mut self, uid: UnitId) {
        let Some(u) = self.services.units.get_mut(uid) else { return };
        let was_idle = u.state == UnitState::Idle && u.car.is_none();
        u.leaving = true;
        u.state = UnitState::Returning;
        u.incident = None;
        u.hospital = None;
        let (car, pos) = (u.car, u.pos);
        match car.filter(|c| self.traffic.cars.contains_key(*c)) {
            Some(car) => {
                self.traffic.cars[car].siren = false;
                if !self.redirect_out(car) {
                    self.traffic.pull_in(car, super::cars::RETIRE_RETRY_SECONDS);
                }
            }
            None => {
                self.services.units[uid].car = None;
                match self.route_out_from(pos) {
                    Some((lanes, s0, s1)) => {
                        let car = self.traffic.spawn_car(CitizenId::default(), None, lanes, s0, s1, 0);
                        self.services.by_car.insert(car, uid);
                        self.services.units[uid].car = Some(car);
                    }
                    None if was_idle => {
                        self.services.units.remove(uid);
                    }
                    None => {}
                }
            }
        }
    }

    /// [services] Back in the dispatch queue: the unit that had the call
    /// is gone or was called off. A scene the air crew had (its work set to
    /// "until the crew is done") gets the ground crew's time again.
    pub(in crate::world) fn requeue(&mut self, id: u32) {
        let tick = self.clock.tick;
        let Some(i) = self.services.incident_mut(id) else { return };
        i.stage = Stage::Reported;
        i.unit = None;
        i.dispatched = None;
        i.arrived = None;
        i.unit_name = None;
        i.dispatch_at = Some(tick);
        if i.work_left == f32::MAX {
            i.work_left = match i.kind {
                IncidentKind::Medical => MEDICAL_WORK,
                IncidentKind::Crime => CRIME_WORK,
                IncidentKind::Crash => CRASH_WORK,
                IncidentKind::Fire => 0.0,
            };
        }
    }

    /// [services] Someone is on this job: the unit it names is out on it, or
    /// an air ambulance has it.
    pub(super) fn incident_served(&self, inc: &Incident) -> bool {
        let by_unit = inc.unit.and_then(|u| self.services.units.get(u)).is_some_and(|u| u.incident == Some(inc.id) && !u.leaving);
        by_unit || self.air.drones.values().any(|d| matches!(d.job, crate::air::Job::Medic { incident, .. } if incident == inc.id))
    }

    /// [fire-scene] Fire `id` lost its first engine: its second (on scene
    /// or on the way) becomes its unit. False when there is none.
    pub(super) fn promote_second(&mut self, id: u32) -> bool {
        let Some(sec) = self.services.incident(id).and_then(|i| i.second) else { return false };
        let state = self.services.units.get(sec).filter(|u| u.incident == Some(id) && !u.leaving).map(|u| u.state);
        let stage = match state {
            Some(UnitState::OnScene) => Stage::OnScene,
            Some(UnitState::Responding) => Stage::Dispatched,
            _ => {
                if let Some(i) = self.services.incident_mut(id) {
                    i.second = None;
                }
                return false;
            }
        };
        let i = self.services.incident_mut(id).unwrap();
        i.unit = Some(sec);
        i.second = None;
        i.stage = stage;
        true
    }

    /// [services] An active call whose unit or aircraft is gone: a patient
    /// who was being driven to hospital is delivered to the nearest free bed
    /// (or treated where they are); anything else goes back in the queue.
    pub(super) fn orphaned(&mut self, id: u32) {
        let Some(inc) = self.services.incident(id).cloned() else { return };
        if inc.stage == Stage::Transporting {
            match (inc.citizen.filter(|c| self.citizens.contains_key(*c)), self.hospital_with_bed(inc.pos)) {
                (Some(p), Some(h)) => self.admit(id, p, h),
                _ => self.treat_on_site(id),
            }
            return;
        }
        self.requeue(id);
    }

    pub(super) fn park_at_station(&mut self, uid: UnitId) {
        let Some(u) = self.services.units.get(uid) else { return };
        if !self.station_alive(u.station) {
            self.unit_leaves_town(uid);
            return;
        }
        let home = self.buildings.get(u.station).and_then(|b| self.lots.get(b.lot)).map(|l| l.car_point);
        let u = &mut self.services.units[uid];
        u.state = UnitState::Idle;
        u.incident = None;
        u.hospital = None;
        if let Some(p) = home {
            u.pos = p;
        }
    }

    /// An emergency vehicle reached the end of its route (called from
    /// `step_traffic`).
    pub(in crate::world) fn unit_car_arrived(&mut self, car_id: CarId) {
        // [services] Leaving town: gone at the map edge, else (it held for
        // want of a road out) it tries again from here.
        if let Some(uid) = self.services.by_car.get(&car_id).copied().filter(|u| self.services.units.get(*u).is_some_and(|u| u.leaving)) {
            if self.at_map_edge(car_id) {
                self.services.by_car.remove(&car_id);
                self.traffic.remove_car(car_id);
                self.services.units.remove(uid);
            } else if !self.redirect_out(car_id) {
                self.traffic.pull_in(car_id, super::cars::RETIRE_RETRY_SECONDS);
            }
            return;
        }
        // [emergency] In pursuit: it stays on the road after its quarry.
        if self.pursuit_arrived(car_id) {
            return;
        }
        let Some(uid) = self.services.by_car.remove(&car_id) else { return };
        let Some(car) = self.traffic.remove_car(car_id) else { return };
        let Some(u) = self.services.units.get_mut(uid) else { return };
        u.car = None;
        u.pos = car.pos;
        u.heading = car.heading;
        let state = u.state;
        match state {
            UnitState::Responding => {
                let inc = u.incident;
                // [fire-scene] At a fire either engine (the first or the
                // second alarm's) may get there first; the other joins it.
                let fire = inc.and_then(|i| self.services.incident(i)).filter(|i| i.kind == IncidentKind::Fire && (i.unit == Some(uid) || i.second == Some(uid))).map(|i| (i.stage, i.building, i.unit, i.second));
                let live = inc.and_then(|i| self.services.incident(i)).is_some_and(|i| i.stage == Stage::Dispatched) || fire.is_some_and(|f| f.0 == Stage::OnScene);
                if !live {
                    self.send_home(uid);
                    return;
                }
                self.services.units[uid].state = UnitState::OnScene;
                if let Some((stage, b, first, second)) = fire {
                    // Pulled in to the kerb, behind an engine already there.
                    let other = [first, second].into_iter().flatten().find(|&o| o != uid).and_then(|o| self.services.units.get(o)).filter(|o| o.state == UnitState::OnScene && o.car.is_none() && o.incident == inc).map(|o| (o.pos, o.heading));
                    self.park_engine(uid, b, other);
                    if stage == Stage::OnScene {
                        return;
                    }
                }
                self.on_scene(inc.unwrap());
            }
            UnitState::Transporting => self.deliver_patient(uid),
            UnitState::Returning => self.park_at_station(uid),
            UnitState::Patrol => {
                // On the beat: park a while.
                let stay = if self.services.units[uid].kind == UnitKind::PoliceCar { PATROL_STAY } else { STANDBY_STAY };
                let until = self.clock.tick + self.srng().random_range(stay.0..=stay.1);
                self.services.units[uid].patrol_until = until;
            }
            _ => self.send_home(uid),
        }
    }

    /// A unit (city or out of town) got there: the clock on the response
    /// stops and the people involved see it arrive.
    pub(in crate::world) fn on_scene(&mut self, id: u32) {
        let tick = self.clock.tick;
        let (kind, bid, response) = {
            let i = self.services.incident_mut(id).unwrap();
            i.stage = Stage::OnScene;
            i.arrived = Some(tick);
            (i.kind, i.building, i.response_ticks())
        };
        if let Some(r) = response {
            self.services.stats.response_ticks_today += r;
            self.services.stats.responses_today += 1;
            self.services.stats.response_min_today[kind.slot()].push(r.min(u16::MAX as u64) as u16);
        }
        let bname = self.buildings.get(bid).map(|b| b.name.clone()).unwrap_or_default();
        let note = match kind {
            IncidentKind::Fire => format!("The fire brigade arrived at {bname}, sirens going."),
            IncidentKind::Crime => format!("The police showed up at {bname}."),
            IncidentKind::Medical => format!("Paramedics came for me at {bname}."),
            IncidentKind::Crash => format!("The police came to the crash by {bname}."),
        };
        let who: Vec<CitizenId> = match kind {
            IncidentKind::Medical => self.services.incident(id).and_then(|i| i.citizen).into_iter().collect(),
            IncidentKind::Crash => self.services.incident(id).map(|i| i.people.iter().map(|p| p.citizen).collect()).unwrap_or_default(),
            _ => self.buildings.get(bid).map(|b| b.residents.iter().chain(b.workers.iter()).copied().collect()).unwrap_or_default(),
        };
        // [concerns] Help that took this long is a grievance.
        let slow = response.filter(|r| *r >= crate::concerns::SLOW_RESPONSE_TICKS).map(|r| {
            let unit = match kind {
                IncidentKind::Fire => "the fire brigade",
                IncidentKind::Medical => "the ambulance",
                _ => "the police",
            };
            format!("Took {r} minutes for {unit} to get to {bname}.")
        });
        for (i, cid) in who.into_iter().enumerate() {
            if let Some(c) = self.citizens.get_mut(cid) {
                c.remember(tick, note.clone());
                if let Some(text) = slow.as_ref().filter(|_| i < 8) {
                    self.concerns.said(tick, cid, c, crate::concerns::Concern::Services, 0.5, text.clone());
                }
            }
        }
        self.events.push(SimEvent::UnitOnScene(id));
    }

    /// Drive a car for `uid` from where it stands to `to`. False if no road.
    pub(in crate::world) fn drive_unit(&mut self, uid: UnitId, to: (SegmentId, Vec2), siren: bool) -> bool {
        let Some(from) = self.services.units.get(uid).map(|u| u.pos) else { return false };
        let Some((seg, _, q)) = self.roads.nearest_street(from) else { return false };
        // A goal just behind on the same street has no route (lanes are one
        // way): set off from the nearer end of the street instead.
        let route = self.traffic.net.find_route(seg, q, to.0, to.1).or_else(|| {
            let (a, b) = self.roads.segment_points(seg);
            let mut ends = [a, b];
            ends.sort_by(|x, y| x.distance(q).total_cmp(&y.distance(q)));
            ends.iter().find_map(|e| self.traffic.net.find_route(seg, *e, to.0, to.1))
        });
        let Some((lanes, s0, s1)) = route else { return false };
        let car = self.traffic.spawn_car(CitizenId::default(), None, lanes, s0, s1, 0);
        self.traffic.cars[car].siren = siren;
        self.services.by_car.insert(car, uid);
        self.services.units[uid].car = Some(car);
        true
    }

    /// Back to the station, or teleported home when no road leads there.
    /// Police cars and ambulances with a crew go straight back on patrol or
    /// standby instead.
    pub(in crate::world) fn send_home(&mut self, uid: UnitId) {
        let Some(u) = self.services.units.get(uid) else { return };
        if !self.station_alive(u.station) {
            self.unit_leaves_town(uid);
            return;
        }
        if matches!(u.kind, UnitKind::PoliceCar | UnitKind::Ambulance) && u.car.is_none() {
            let out = self.units_out();
            if out.get(&u.station).copied().unwrap_or(0) <= self.crewed_units(u.station) && self.start_patrol(uid) {
                return;
            }
        }
        let Some(u) = self.services.units.get_mut(uid) else { return };
        u.incident = None;
        u.hospital = None;
        let station = u.station;
        let to = self.building_lot(station).and_then(|l| self.lot_access(l));
        match to {
            Some(to) if self.drive_unit(uid, to, false) => self.services.units[uid].state = UnitState::Returning,
            _ => self.park_at_station(uid),
        }
    }

    /// [fire-scene] Fire engines whose crew has packed up drive home.
    pub(super) fn step_packing(&mut self) {
        let tick = self.clock.tick;
        let done: Vec<UnitId> = self.services.units.iter().filter(|(_, u)| u.packing_until > 0 && (tick >= u.packing_until || u.state != UnitState::OnScene)).map(|(id, _)| id).collect();
        for uid in done {
            let u = &mut self.services.units[uid];
            u.packing_until = 0;
            if u.state == UnitState::OnScene {
                self.send_home(uid);
            }
        }
    }
}
