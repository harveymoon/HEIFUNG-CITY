//! [passengers] Riding along in someone else's car (types and the overview
//! in `crate::rides`).
//!
//! `join_ride` seats a passenger with a driver who is setting off by car
//! from the same place: walking to the parked car, or already pulling out.
//! Every tick `step_passengers` keeps passengers with their driver: beside
//! them on the walk to the car, inside it on the road (out of sight, at the
//! car's position, listed in `Car::occupants`). When the driver is out of
//! the car again the passengers get out where it stopped and walk on to
//! their own destination; if the drive never happened (no route, the car
//! clamped), they go their own way. A driver dropping someone off stops at
//! their door first (`drop_off`), then drives on.

use super::*;
use crate::citizens::TripMode;
use crate::rides::{self, OnwardLeg, RideKind};

/// Walking the last stretch from the car below this is just getting out.
const GET_OUT: f32 = 20.0;

impl World {
    /// Seats free for passengers with `driver` (none unless they are setting
    /// off by car).
    fn seats_left(&self, driver: CitizenId) -> usize {
        let Some(d) = self.citizens.get(driver) else { return 0 };
        let driving = match &d.activity {
            Activity::Driving { .. } => true,
            Activity::Traveling { .. } => d.to_car.is_some(),
            _ => false,
        };
        if !driving {
            return 0;
        }
        let model = d.car.and_then(|v| self.vehicles.cars.get(v)).map(|c| c.model).or(d.vehicle).unwrap_or_default();
        rides::seats(&model).saturating_sub(self.passengers_of(driver).len())
    }

    /// The citizens riding with `driver`.
    pub fn passengers_of(&self, driver: CitizenId) -> Vec<CitizenId> {
        self.citizens.iter().filter(|(_, c)| matches!(c.activity, Activity::Passenger { driver: d, .. } if d == driver)).map(|(id, _)| id).collect()
    }

    /// [passengers] Everyone in a car on the road besides the driver.
    pub fn car_occupants(&self, car: CarId) -> Vec<CitizenId> {
        self.traffic.cars.get(car).map_or(Vec::new(), |c| c.occupants.clone())
    }

    /// Where a citizen is, or the door of the building they are in.
    fn standing_at(&self, id: CitizenId) -> Option<Vec2> {
        let c = self.citizens.get(id)?;
        match c.inside {
            Some(b) => self.building_entrance(b),
            None => Some(c.pos),
        }
    }

    /// `p` rides with `driver` to `to`: the driver is setting off by car
    /// from where `p` is, with a seat free. False (nothing changed) when not.
    /// `begin` starts the passenger's trip record (the mode choice does it
    /// itself).
    pub(super) fn join_ride(&mut self, p: CitizenId, driver: CitizenId, to: BuildingId, purpose: Purpose, kind: RideKind, begin: bool) -> bool {
        if p == driver || self.seats_left(driver) == 0 || self.citizens.get(p).is_none_or(|c| c.activity.is_traveling()) {
            return false;
        }
        let (Some(here), Some(dpos)) = (self.standing_at(p), self.citizens.get(driver).map(|d| d.pos)) else { return false };
        if here.distance(dpos) > rides::TOGETHER {
            return false;
        }
        let Some(entrance) = self.building_entrance(to) else { return false };
        self.leave_building(p);
        let seated = matches!(self.citizens[driver].activity, Activity::Driving { .. });
        let c = &mut self.citizens[p];
        c.transit = None;
        c.to_car = None;
        c.pending_footfall.clear();
        c.prev_pos = c.pos;
        c.pos = dpos;
        c.mode = TravelMode::Walk;
        c.activity = Activity::Passenger { driver, to: Some(to), purpose, seated };
        if begin {
            self.pack_meal(p);
            self.begin_trip(p, TripMode::Car, dpos.distance(entrance));
        }
        self.rides.note(kind);
        true
    }

    /// Every tick, after everyone has moved: passengers keep with their
    /// driver, and get out when the driver does.
    pub(super) fn step_passengers(&mut self) {
        for car in std::mem::take(&mut self.rides.cars) {
            if let Some(c) = self.traffic.cars.get_mut(car) {
                c.occupants.clear();
            }
        }
        let riders: Vec<(CitizenId, CitizenId, bool)> = self.citizens.iter().filter_map(|(id, c)| match c.activity {
            Activity::Passenger { driver, seated, .. } => Some((id, driver, seated)),
            _ => None,
        }).collect();
        for (p, driver, seated) in riders {
            enum Next {
                Ride(CarId, Vec2, f32),
                Walk(Vec2, f32),
                GetOut(Vec2),
                Alone,
            }
            let next = match self.citizens.get(driver) {
                Some(d) => match &d.activity {
                    Activity::Driving { car, .. } => match self.traffic.cars.get(*car) {
                        Some(rc) => Next::Ride(*car, rc.pos, rc.heading),
                        None if seated => Next::GetOut(d.pos),
                        None => Next::Alone,
                    },
                    Activity::Traveling { .. } if d.to_car.is_some() && !seated => {
                        // Beside them, a step to the right.
                        let side = Vec2::new(-d.heading.sin(), d.heading.cos()) * -0.7;
                        Next::Walk(d.pos + side, d.heading)
                    }
                    _ if seated => Next::GetOut(d.pos),
                    _ => Next::Alone,
                },
                None if seated => Next::GetOut(self.citizens[p].pos),
                None => Next::Alone,
            };
            match next {
                Next::Ride(car, pos, heading) => {
                    let c = &mut self.citizens[p];
                    c.prev_pos = if seated { c.pos } else { pos };
                    c.prev_heading = heading;
                    c.pos = pos;
                    c.heading = heading;
                    if let Activity::Passenger { seated, .. } = &mut c.activity {
                        *seated = true;
                    }
                    if let Some(rc) = self.traffic.cars.get_mut(car) {
                        rc.occupants.push(p);
                        let n = rc.occupants.len() as u32 + 1;
                        if rc.occupants.len() == 1 {
                            self.rides.cars.push(car);
                        }
                        self.rides.stats.fullest_car = self.rides.stats.fullest_car.max(n);
                    }
                }
                Next::Walk(pos, heading) => {
                    let c = &mut self.citizens[p];
                    c.prev_pos = c.pos;
                    c.prev_heading = c.heading;
                    c.pos = pos;
                    c.heading = heading;
                }
                Next::GetOut(at) => self.get_out(p, at),
                Next::Alone => self.go_alone(p),
            }
        }
    }

    /// Out of the car at `at`: walk on to where they were going, or be there.
    fn get_out(&mut self, p: CitizenId, at: Vec2) {
        let Some(c) = self.citizens.get(p) else { return };
        let Activity::Passenger { to, purpose, .. } = c.activity else { return };
        let Some(entrance) = to.and_then(|b| self.building_entrance(b)) else {
            let c = &mut self.citizens[p];
            c.pos = at;
            c.prev_pos = at;
            c.activity = Activity::Idle;
            c.commute.mode = None;
            return;
        };
        let near = at.distance(entrance) < GET_OUT;
        let mut path = if near {
            Vec::new()
        } else {
            match self.walk.route(at, entrance) {
                Some((pts, _)) => pts,
                None => vec![at],
            }
        };
        path.push(entrance);
        let c = &mut self.citizens[p];
        c.pos = at;
        c.prev_pos = at;
        c.mode = TravelMode::Walk;
        c.commute.walk_after = at.distance(entrance);
        c.activity = Activity::Traveling { to, purpose, path, idx: 0 };
        if near {
            self.citizens[p].pos = entrance;
            self.arrive(p);
        }
    }

    /// The drive fell through before they got in: they make their own way.
    fn go_alone(&mut self, p: CitizenId) {
        let Some(c) = self.citizens.get(p) else { return };
        let Activity::Passenger { to, purpose, .. } = c.activity else { return };
        let c = &mut self.citizens[p];
        c.activity = Activity::Idle;
        c.commute.mode = None;
        match to {
            Some(b) if self.buildings.contains_key(b) => self.travel_to(p, Some(b), purpose),
            _ => {}
        }
    }

    // ── who rides with whom ─────────────────────────────────────────────

    /// [passengers] For the mode choice: family or a coworker walking to
    /// their car from right where this trip starts, going to within
    /// `LIFT_REACH` of where it goes, with a seat. The driver and the door
    /// they are going to.
    pub(super) fn lift_on_offer(&self, id: CitizenId, q: &super::trips::TripQuery) -> Option<(CitizenId, Vec2)> {
        let c = self.citizens.get(id)?;
        let mut pool: Vec<CitizenId> = c.household.and_then(|h| self.households.get(h)).map_or(Vec::new(), |h| h.members.clone());
        if let Some(w) = c.work.and_then(|w| self.buildings.get(w)) {
            pool.extend(w.workers.iter().take(40).copied());
        }
        for m in pool {
            if m == id {
                continue;
            }
            let Some(x) = self.citizens.get(m) else { continue };
            let Some(tc) = x.to_car.filter(|_| matches!(x.activity, Activity::Traveling { .. })) else { continue };
            if x.pos.distance(q.from) > rides::TOGETHER {
                continue;
            }
            let Some(door) = self.building_entrance(tc.to) else { continue };
            if door.distance(q.to) <= rides::LIFT_REACH && self.seats_left(m) > 0 {
                return Some((m, door));
            }
        }
        None
    }

    /// Free to come along now: at home or idle, awake, not held or in a ward.
    fn free_to_ride(&self, id: CitizenId) -> bool {
        self.citizens.get(id).is_some_and(|c| matches!(c.activity, Activity::AtHome | Activity::Idle | Activity::Working | Activity::Visiting(_)) && !c.asleep && c.wakefulness > crate::citizens::TIRED) && !self.services.in_custody(id) && !self.is_patient(id)
    }

    /// [passengers] A driver just set off to work or home: coworkers who
    /// set off from the same place for the same part of town come along.
    /// To work: coworkers living at the driver's door whose shift is about
    /// to start. Home: coworkers whose shift is over and who live near the
    /// driver, and family out at the same place.
    pub(super) fn pick_up(&mut self, driver: CitizenId, origin: Option<BuildingId>, target: BuildingId, purpose: Purpose) {
        let Some(d) = self.citizens.get(driver) else { return };
        let hour = self.clock.hour() as u8;
        let (work, household) = (d.work, d.household);
        let to_near = |w: &World, b: Option<BuildingId>| -> bool {
            let (Some(a), Some(t)) = (b.and_then(|b| w.building_entrance(b)), w.building_entrance(target)) else { return false };
            a.distance(t) <= rides::LIFT_REACH
        };
        let mut pool: Vec<(CitizenId, BuildingId, Purpose, RideKind)> = Vec::new();
        match purpose {
            Purpose::Work => {
                let Some(w) = work.and_then(|w| self.buildings.get(w)) else { return };
                for &m in w.workers.iter().filter(|m| **m != driver).take(40) {
                    let Some(x) = self.citizens.get(m) else { continue };
                    let soon = w.shift_for(x.seed).contains(hour) || w.shift_for(x.seed).contains((hour + 1) % 24);
                    let at_home = x.home.is_some() && x.inside == x.home && matches!(x.activity, Activity::AtHome);
                    if soon && at_home {
                        pool.push((m, target, Purpose::Work, RideKind::Carpool));
                    }
                }
            }
            Purpose::Home => {
                let Some(o) = origin else { return };
                for (m, x) in self.citizens.iter() {
                    if m == driver || x.inside != Some(o) {
                        continue;
                    }
                    let family = household.is_some() && x.household == household && matches!(x.activity, Activity::Visiting(_) | Activity::Working);
                    let mate = work == Some(o) && x.work == Some(o) && matches!(x.activity, Activity::Working) && self.buildings.get(o).is_some_and(|b| !b.shift_for(x.seed).contains(hour)) && to_near(self, x.home);
                    if let (true, Some(h)) = (family || mate, x.home) {
                        pool.push((m, h, Purpose::Home, if family { RideKind::Outing } else { RideKind::Carpool }));
                    }
                }
            }
            _ => return,
        }
        let tick = self.clock.tick;
        let name = self.citizens[driver].name.clone();
        for (m, to, why, kind) in pool {
            if self.seats_left(driver) == 0 {
                break;
            }
            if !self.free_to_ride(m) {
                continue;
            }
            if self.join_ride(m, driver, to, why, kind, true) {
                let c = &mut self.citizens[m];
                c.thought = format!("Getting a lift with {name}.");
                c.next_decision_tick = tick + 60;
            }
        }
    }

    /// [passengers] A child off to school is driven there by a parent at
    /// home with the car: on the way to work, or (now and then) a parent
    /// who is not working, who drives home again. True when it is arranged
    /// (the child is then a passenger).
    pub(super) fn school_run(&mut self, kid: CitizenId, school: BuildingId, purpose: Purpose) -> bool {
        let tick = self.clock.tick;
        let hour = self.clock.hour() as u8;
        let Some(k) = self.citizens.get(kid) else { return false };
        let (Some(home), Some(hh)) = (k.home, k.household) else { return false };
        if k.inside != Some(home) {
            return false;
        }
        let (Some(door), Some(gate)) = (self.building_entrance(home), self.building_entrance(school)) else { return false };
        if door.distance(gate) < self.config.drive_threshold.max(350.0) {
            return false;
        }
        let members = self.households.get(hh).map_or(Vec::new(), |h| h.members.clone());
        let mut best: Option<(CitizenId, VehicleIdOf, BuildingId, Purpose)> = None;
        for m in members {
            let Some(x) = self.citizens.get(m) else { continue };
            if m == kid || x.stage() == crate::citizens::LifeStage::Child || x.inside != Some(home) || !matches!(x.activity, Activity::AtHome) || x.asleep || x.drunk >= crate::citizens::DRUNK {
                continue;
            }
            let Some((car, at)) = self.car_ready(m, door) else { continue };
            let job = x.work.filter(|w| self.buildings.get(*w).is_some_and(|b| b.shift_for(x.seed).contains(hour) || b.shift_for(x.seed).contains((hour + 1) % 24)));
            let shift_later = x.work.and_then(|w| self.buildings.get(w)).is_some_and(|b| (hour..hour.saturating_add(4).min(23)).any(|h| b.shift_for(x.seed).contains(h)));
            match job {
                Some(w) => {
                    best = Some((m, (car, at), w, Purpose::Work));
                    break;
                }
                None if !shift_later && best.is_none() => {
                    let mut rng = crate::rng_for(x.seed ^ tick, 0x5C40_0151);
                    if rng.random_bool(0.4) {
                        best = Some((m, (car, at), home, Purpose::Home));
                    }
                }
                None => {}
            }
        }
        let Some((parent, (car, at), then, then_purpose)) = best else { return false };
        // The parent drives to the school, the child beside them.
        let from = door;
        let lu = self.buildings.get(school).map_or(LandUse::Civic, |b| b.land_use);
        if at.distance(from) > 25.0 {
            self.walk_to_car(parent, car, at, school, Purpose::Visit(lu), from);
        } else if !self.start_drive(parent, car, school, Purpose::Visit(lu)) {
            return false;
        }
        self.begin_trip(parent, TripMode::Car, from.distance(gate));
        self.rides.onward.push(OnwardLeg { driver: parent, stop: school, to: then, purpose: then_purpose });
        if !self.join_ride(kid, parent, school, purpose, RideKind::SchoolRun, true) {
            return false;
        }
        let (pname, kname, sname) = (self.citizens[parent].name.clone(), self.citizens[kid].name.clone(), self.buildings[school].name.clone());
        let first = |n: &str| n.split_whitespace().next().unwrap_or(n).to_string();
        self.citizens[parent].thought = format!("Dropping {} at {sname}.", first(&kname));
        self.citizens[kid].thought = format!("{} is driving me to school.", first(&pname));
        true
    }

    /// [passengers] A car reached a stop on the way (a child's school):
    /// those going there get out, and it drives on. True when handled.
    pub(super) fn drop_off(&mut self, car_id: CarId) -> bool {
        let Some(car) = self.traffic.cars.get(car_id) else { return false };
        let (driver, pos) = (car.citizen, car.pos);
        let Some(Activity::Driving { to: Some(stop), .. }) = self.citizens.get(driver).map(|c| c.activity.clone()) else { return false };
        if !self.rides.onward.iter().any(|l| l.driver == driver && l.stop == stop) {
            return false;
        }
        let Some(leg) = self.rides.take_onward(driver) else { return false };
        let tick = self.clock.tick;
        for p in self.passengers_of(driver) {
            if matches!(self.citizens[p].activity, Activity::Passenger { to: Some(t), .. } if t == stop) {
                self.get_out(p, pos);
            }
        }
        if let Some(c) = self.citizens.get_mut(driver) {
            c.activity = Activity::Driving { car: car_id, to: Some(leg.to), purpose: leg.purpose };
            if let Some(b) = self.buildings.get(stop) {
                if (tick + id_hash_of(driver)) % 3 == 0 {
                    c.remember(tick, format!("Dropped the kids at {}.", b.name));
                }
            }
        }
        // On to the next place, after a moment at the kerb.
        let next = self.building_lot(leg.to).and_then(|l| l.car_segment.map(|s| (s, l.car_point))).and_then(|(s, p)| self.traffic.net.nearest_lane_on(s, p));
        if let Some((goal, s_end, _)) = next {
            if self.traffic.redirect(car_id, goal, s_end) {
                self.traffic.cars[car_id].errand = crate::vehicle::Errand::Trip;
                self.traffic.pull_in(car_id, 20.0);
                return true;
            }
        }
        // No way on from here: park nearby and walk.
        false
    }

    /// [passengers] The car a driver meant to take could not be driven:
    /// the stop on the way is off; where they meant to go after it, if any.
    pub(super) fn onward_after_failed_drive(&mut self, driver: CitizenId) -> Option<(BuildingId, Purpose)> {
        self.rides.take_onward(driver).map(|l| (l.to, l.purpose))
    }
}

type VehicleIdOf = (crate::vehicle::VehicleId, Vec2);

fn id_hash_of(id: CitizenId) -> u64 {
    slotmap::Key::data(&id).as_ffi()
}
