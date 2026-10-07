//! [emergency] Car chases. When the police come to pick up a suspect who
//! can get to a car or a motorbike (theirs, parked within `CHASE_REACH` of
//! the scene, or the one they are driving), the suspect may run for it
//! (`CHASE_CHANCE`) instead of being taken or slipping away on foot. They
//! drive recklessly (`DriverProfile::fleeing`: fast, through every light,
//! round anything slower) towards somewhere away from the police car,
//! choosing again every few minutes and whenever they get there. The unit
//! gives chase with lights and sirens, re-routed every minute to where the
//! fugitive is now. Tag rules: the police car touching the fugitive's car
//! (`TAG_REACH`, centre to centre, at any substep of the minute) ends it,
//! the suspect stops, gets out and is arrested. Out of touch
//! (`CHASE_LOSE_DIST`) for `CHASE_LOSE_TICKS` minutes in a row, or
//! `CHASE_MAX` minutes in, the police lose them and the fugitive goes home.
//! Every chase is kept on its incident (`Incident::chase`) and counted in
//! `ServiceStats`. Decisions draw from the services' own stream.

use super::*;
use crate::services::*;
use crate::traffic::DriverProfile;
use crate::vehicle::{Errand, ParkKind, VehicleId};
use rand::Rng;

/// How a suspect gets away.
enum Getaway {
    /// Already at the wheel of their own car.
    Driving(CarId),
    /// Their car (or motorbike) is parked within reach.
    Parked(VehicleId),
}

impl World {
    /// The vehicle `s` can flee the police at `at` in, if any.
    fn getaway(&self, s: CitizenId, at: Vec2) -> Option<Getaway> {
        let c = self.citizens.get(s)?;
        if self.services.in_custody(s) || self.is_patient(s) || self.services.chases.iter().any(|x| x.suspect == s) {
            return None;
        }
        match c.activity {
            Activity::Driving { car, .. } => {
                let tc = self.traffic.cars.get(car)?;
                (tc.owned.is_some() && !tc.crashed() && !tc.in_stunt() && tc.drive.gang.is_none() && !tc.drive.fleeing).then_some(Getaway::Driving(car))
            }
            Activity::AtHome | Activity::Working | Activity::Visiting(_) | Activity::Idle | Activity::Traveling { .. } | Activity::Sitting { .. } | Activity::Waiting { .. } => {
                let vid = c.car.filter(|_| c.owns_car)?;
                let spot = self.vehicles.cars.get(vid)?.spot()?;
                (!matches!(spot.kind, ParkKind::Dealer | ParkKind::Hooked | ParkKind::Pound) && spot.pos.distance(at) <= CHASE_REACH).then_some(Getaway::Parked(vid))
            }
            _ => None,
        }
    }

    /// Somewhere for a fugitive at `from` to run to, away from the police
    /// at `police`: a lot's car access `FLEE_RANGE` off, the furthest from
    /// the police for the least driving, with a little chance in it. Best
    /// first.
    fn flee_targets(&mut self, from: Vec2, police: Vec2) -> Vec<(SegmentId, Vec2)> {
        let mut c: Vec<(f32, SegmentId, Vec2)> = self
            .lots
            .values()
            .filter_map(|l| l.car_segment.map(|s| (s, l.car_point)))
            .filter(|(_, p)| (FLEE_RANGE.0..=FLEE_RANGE.1).contains(&p.distance(from)))
            .map(|(s, p)| (p.distance(police) - 0.5 * p.distance(from), s, p))
            .collect();
        for x in c.iter_mut() {
            x.0 += self.srng().random_range(0.0..120.0f32);
        }
        c.sort_by(|a, b| b.0.total_cmp(&a.0));
        c.into_iter().take(4).map(|(_, s, p)| (s, p)).collect()
    }

    /// Send the fugitive's car on to somewhere away from the police.
    fn flee_on(&mut self, car: CarId, police: Vec2) -> bool {
        let Some(from) = self.traffic.cars.get(car).map(|c| c.pos) else { return false };
        for (seg, p) in self.flee_targets(from, police) {
            if let Some((goal, s_end, _)) = self.traffic.net.nearest_lane_on(seg, p) {
                if self.traffic.redirect(car, goal, s_end) {
                    return true;
                }
            }
        }
        false
    }

    /// The police are here to pick up the suspect of crime `id`: if they
    /// can get to a vehicle, they may run for it. True when a chase began
    /// (the incident is then in `Stage::Pursuit`).
    pub(super) fn try_chase(&mut self, id: u32) -> bool {
        let Some(inc) = self.services.incident(id) else { return false };
        if inc.outside || inc.kind != IncidentKind::Crime {
            return false;
        }
        let (Some(s), Some(uid), at) = (inc.suspect, inc.unit, inc.pos) else { return false };
        if self.services.units.get(uid).is_none_or(|u| u.kind != UnitKind::PoliceCar || u.car.is_some()) {
            return false;
        }
        let Some(g) = self.getaway(s, at) else { return false };
        if self.srng().random::<f32>() >= CHASE_CHANCE {
            return false;
        }
        self.start_chase(id, uid, s, g)
    }

    /// Put the suspect at the wheel, fleeing, and the unit after them.
    fn start_chase(&mut self, id: u32, uid: UnitId, s: CitizenId, g: Getaway) -> bool {
        let tick = self.clock.tick;
        let police_at = self.services.units[uid].pos;
        // Where the chase starts: the car on the road, or the parked one.
        let start = match g {
            Getaway::Driving(car) => self.traffic.cars[car].pos,
            Getaway::Parked(vid) => match self.vehicles.cars[vid].spot() {
                Some(spot) => spot.pos,
                None => return false,
            },
        };
        // The police car first: no road to them, no chase.
        let Some(to) = self.roads.nearest_street(start).map(|(seg, _, q)| (seg, q)) else { return false };
        if !self.drive_unit(uid, to, true) {
            return false;
        }
        let car = match g {
            Getaway::Driving(car) => {
                if !self.flee_on(car, police_at) {
                    self.pull_off_road(uid);
                    return false;
                }
                car
            }
            Getaway::Parked(vid) => {
                let first = self.flee_targets(start, police_at).into_iter().find_map(|(seg, p)| self.drive_from_spot(vid, seg, p, s, Errand::Trip));
                let Some(car) = first else {
                    self.pull_off_road(uid);
                    return false;
                };
                self.leave_building(s);
                car
            }
        };
        let pcar = self.services.units[uid].car.expect("just put on the road");
        // The fugitive's driving: their own, made reckless.
        let (pos, heading, model, owned) = {
            let age = self.citizens[s].age;
            let (caution, hedonism) = crate::citizens::driver_traits(self.citizens[s].seed);
            let drunk = self.citizens[s].drunk;
            let tc = &mut self.traffic.cars[car];
            let phase = crate::pedestrians::hash01(slotmap::Key::data(&car).as_ffi(), 0xF1EE, 1);
            let base = if tc.drive.profile.assigned { tc.drive.calm.unwrap_or(tc.drive.profile) } else { DriverProfile::for_driver(age.max(17), caution, hedonism, drunk, 0.5, phase) };
            tc.drive.profile = base.fleeing();
            tc.drive.calm = None;
            tc.drive.anger = 0.0;
            tc.drive.fleeing = true;
            tc.drive.expected_min = tc.drive.expected_min.max(1.0);
            tc.errand = Errand::Trip;
            (tc.pos, tc.heading, tc.model, tc.owned)
        };
        self.traffic.cars[pcar].drive.chasing = Some(car);
        {
            let c = &mut self.citizens[s];
            c.to_car = None;
            c.pos = pos;
            c.prev_pos = pos;
            c.heading = heading;
            c.vehicle = Some(model);
            c.commute.drive_since = c.commute.drive_since.max(1);
            c.activity = Activity::Driving { car, to: None, purpose: Purpose::Wander };
            c.next_decision_tick = c.next_decision_tick.max(tick + CHASE_MAX + 30);
            c.thought = "Go, go, go!".to_string();
            c.remember(tick, format!("Saw the cops coming and ran for my {}.", super::cars::model_label(&model)));
        }
        self.services.units[uid].state = UnitState::Pursuing;
        let vehicle = super::cars::model_label(&model);
        if let Some(i) = self.services.incident_mut(id) {
            i.stage = Stage::Pursuit;
            i.chase = Some(ChaseRecord { started: tick, vehicle, closest: pos.distance(police_at), ..Default::default() });
        }
        self.services.chases.push(Chase { incident: id, unit: uid, suspect: s, car, vehicle: owned, started: tick, out_of_touch: 0, planned: tick, last: pos });
        let st = &mut self.services.stats;
        st.chases_today += 1;
        st.chases_total += 1;
        true
    }

    /// Every tick: each chase is either over (a tag, lost, called off) or
    /// the police re-target the fugitive and the fugitive maybe picks
    /// somewhere else to run.
    pub(super) fn step_chases(&mut self) {
        if self.services.chases.is_empty() {
            return;
        }
        let chases = std::mem::take(&mut self.services.chases);
        let mut keep = Vec::new();
        let mut done = Vec::new();
        for mut ch in chases {
            match self.step_chase(&mut ch) {
                None => keep.push(ch),
                Some(r) => done.push((ch, r)),
            }
        }
        self.services.chases = keep;
        for (ch, r) in done {
            self.end_chase(ch, r);
        }
    }

    fn step_chase(&mut self, ch: &mut Chase) -> Option<ChaseResult> {
        let tick = self.clock.tick;
        let pcar = self.services.units.get(ch.unit).filter(|u| u.state == UnitState::Pursuing).and_then(|u| u.car).filter(|c| self.traffic.cars.contains_key(*c));
        let (Some(pcar), Some(fc)) = (pcar, self.traffic.cars.get(ch.car)) else { return Some(ChaseResult::CalledOff) };
        let at_wheel = matches!(self.citizens.get(ch.suspect).map(|c| &c.activity), Some(Activity::Driving { car, .. }) if *car == ch.car);
        if !at_wheel {
            return Some(ChaseResult::CalledOff);
        }
        let pc = &self.traffic.cars[pcar];
        let (fp, pp) = (fc.pos, pc.pos);
        // Contact at any substep of the minute: the tag.
        let touch = fc.drive.poses.iter().zip(pc.drive.poses.iter()).map(|(a, b)| a.0.distance(b.0)).fold(fp.distance(pp), f32::min);
        let moved = fp.distance(ch.last);
        ch.last = fp;
        if let Some(r) = self.services.incident_mut(ch.incident).and_then(|i| i.chase.as_mut()) {
            r.metres += moved;
            r.closest = r.closest.min(touch);
        }
        if touch <= TAG_REACH {
            return Some(ChaseResult::Tagged);
        }
        ch.out_of_touch = if fp.distance(pp) > CHASE_LOSE_DIST { ch.out_of_touch + 1 } else { 0 };
        if ch.out_of_touch >= CHASE_LOSE_TICKS || tick.saturating_sub(ch.started) >= CHASE_MAX {
            return Some(ChaseResult::Lost);
        }
        self.pursue(pcar, ch.car);
        if tick.saturating_sub(ch.planned) >= FLEE_REPLAN {
            ch.planned = tick;
            self.flee_on(ch.car, pp);
        }
        None
    }

    /// Point the police car at the fugitive: along the way it is going,
    /// a couple of lanes past where it is now, so the police car never
    /// plans to pull up short of it (a route ending just past the quarry
    /// had it braking for its own stop every minute, and it never caught
    /// anyone). Failing that, just past where it is.
    fn pursue(&mut self, pcar: CarId, target: CarId) {
        let Some(t) = self.traffic.cars.get(target) else { return };
        let net = &self.traffic.net;
        let ahead = (t.idx + 2).min(t.route.len() - 1);
        let goal = t.route[ahead];
        let s_end = if ahead + 1 == t.route.len() { t.s_end } else { net.lanes[goal as usize].length - 0.3 };
        let (lane, s) = (t.route[t.idx], t.s);
        let len = net.lanes[lane as usize].length;
        if !self.traffic.redirect(pcar, goal, s_end.max(0.3)) {
            self.traffic.redirect(pcar, lane, (s + 6.0).min(len - 0.3).max(0.3));
        }
    }

    /// The fugitive's car reached where it was running to: on to the next
    /// place (the chase goes on). True when `car` is a fugitive's.
    pub(super) fn chase_car_arrived(&mut self, car: CarId) -> bool {
        let Some(ch) = self.services.chases.iter().find(|c| c.car == car) else { return false };
        let police = self.services.units.get(ch.unit).and_then(|u| u.car).and_then(|c| self.traffic.cars.get(c)).map_or(ch.last, |c| c.pos);
        if let Some(ch) = self.services.chases.iter_mut().find(|c| c.car == car) {
            ch.planned = self.clock.tick;
        }
        self.flee_on(car, police);
        true
    }

    /// A police car in pursuit got to the end of its route (where the
    /// fugitive was): it stays on the road and is pointed at them again.
    /// True when `car` is a pursuing unit's.
    pub(super) fn pursuit_arrived(&mut self, car: CarId) -> bool {
        let Some(uid) = self.services.by_car.get(&car).copied() else { return false };
        if self.services.units.get(uid).is_none_or(|u| u.state != UnitState::Pursuing) {
            return false;
        }
        if let Some(target) = self.traffic.cars.get(car).and_then(|c| c.drive.chasing) {
            self.pursue(car, target);
        }
        true
    }

    fn end_chase(&mut self, ch: Chase, result: ChaseResult) {
        let tick = self.clock.tick;
        let id = ch.incident;
        let s = ch.suspect;
        if let Some(r) = self.services.incident_mut(id).and_then(|i| i.chase.as_mut()) {
            r.ended = Some(tick);
            r.result = Some(result);
        }
        let place = self.services.incident(id).map(|i| i.place.clone()).unwrap_or_default();
        let at_wheel = matches!(self.citizens.get(s).map(|c| &c.activity), Some(Activity::Driving { car, .. }) if *car == ch.car);
        // The police car comes off the road where it is.
        self.pull_off_road(ch.unit);
        let outcome = if result == ChaseResult::Tagged && at_wheel {
            // Tag: the fugitive stops and gets out; the car stays at the kerb.
            let pos = self.traffic.remove_car(ch.car).map(|c| c.pos);
            if let (Some(vid), Some(p)) = (ch.vehicle.filter(|v| self.vehicles.cars.contains_key(*v)), pos) {
                self.park_stranded(vid, p);
            }
            if let Some(c) = self.citizens.get_mut(s) {
                if let Some(p) = pos {
                    c.pos = p;
                    c.prev_pos = p;
                }
                c.activity = Activity::Idle;
                c.mood = (c.mood - 0.25).max(0.0);
                c.thought = "Boxed in. Busted.".to_string();
                c.remember(tick, format!("The cops ran me down after the job at {place}. Nowhere left to go."));
            }
            if let Some(v) = self.services.incident(id).and_then(|i| i.citizen).and_then(|v| self.citizens.get_mut(v)) {
                v.remember(tick, "The police chased down whoever robbed me.");
            }
            let st = &mut self.services.stats;
            st.arrests_today += 1;
            st.chases_tagged_today += 1;
            st.chases_tagged_total += 1;
            self.take_into_custody(id, s);
            Outcome::Arrest
        } else {
            // Lost (or called off): the fugitive calms down and drives home.
            if let Some(tc) = self.traffic.cars.get_mut(ch.car) {
                tc.drive.fleeing = false;
                tc.drive.profile.assigned = false;
                tc.drive.calm = None;
                tc.drive.expected_min = 0.0;
                tc.errand = Errand::Trip;
            }
            let home = self.citizens.get(s).and_then(|c| c.home);
            if at_wheel && self.traffic.cars.contains_key(ch.car) {
                let to = home.and_then(|h| self.building_lot(h)).and_then(|l| l.car_segment.map(|seg| (seg, l.car_point)));
                if let Some((seg, p)) = to {
                    if let Some((goal, s_end, _)) = self.traffic.net.nearest_lane_on(seg, p) {
                        self.traffic.redirect(ch.car, goal, s_end);
                    }
                }
                if let Some(c) = self.citizens.get_mut(s) {
                    c.activity = Activity::Driving { car: ch.car, to: home, purpose: Purpose::Home };
                }
            } else if let Some(c) = self.citizens.get_mut(s).filter(|c| matches!(c.activity, Activity::Driving { car, .. } if car == ch.car)) {
                // The car is gone (towed off a wreck, a rebuild): on foot.
                c.activity = Activity::Idle;
                c.next_decision_tick = tick + 1;
            }
            if let Some(c) = self.citizens.get_mut(s) {
                c.thought = "Lost them. Heart's still going.".to_string();
                c.remember(tick, "Shook off a police car. Lying low for a while.");
            }
            let st = &mut self.services.stats;
            st.chases_lost_today += 1;
            Outcome::Escaped
        };
        if let Some(u) = self.services.units.get_mut(ch.unit) {
            u.state = UnitState::OnScene;
        }
        self.resolve(id, outcome);
        // Resolved some other way meanwhile: the unit still goes free.
        if self.services.units.get(ch.unit).is_some_and(|u| u.state == UnitState::OnScene) {
            self.send_home(ch.unit);
        }
    }

    /// Dev and remote: stage a chase now. A free police unit, from where it
    /// is, comes for someone whose car is parked near home, as if about a
    /// robbery there, and the suspect runs for the car. The incident id, or
    /// why not.
    pub fn stage_chase(&mut self) -> Result<u32, String> {
        let uid = self.services.units.iter().find(|(_, u)| u.kind == UnitKind::PoliceCar && !u.busy()).map(|(id, _)| id).ok_or("no free police car")?;
        let suspect = self
            .citizens
            .iter()
            .filter(|(id, c)| c.owns_car && c.home.is_some() && !self.services.in_custody(*id) && !self.is_patient(*id))
            .filter_map(|(id, c)| {
                let spot = c.car.and_then(|v| self.vehicles.cars.get(v)).and_then(|v| v.spot().copied())?;
                let home = self.building_entrance(c.home?)?;
                (spot.pos.distance(home) < CHASE_REACH * 0.5 && !matches!(spot.kind, ParkKind::Dealer | ParkKind::Hooked | ParkKind::Pound)).then_some(id)
            })
            .next()
            .ok_or("nobody with a car parked near home")?;
        let home = self.citizens[suspect].home.unwrap();
        let at = self.building_entrance(home).unwrap_or_default();
        let id = self.report_crime(CrimeKind::Robbery, home, Some(suspect), None).ok_or("could not report the crime")?;
        self.pull_off_road(uid);
        let tick = self.clock.tick;
        {
            let u = &mut self.services.units[uid];
            u.state = UnitState::OnScene;
            u.incident = Some(id);
        }
        if let Some(i) = self.services.incident_mut(id) {
            i.unit = Some(uid);
            i.stage = Stage::OnScene;
            i.reported = Some(tick);
            i.dispatched = Some(tick);
            i.arrived = Some(tick);
        }
        let g = self.getaway(suspect, at).ok_or("the suspect cannot get to the car")?;
        if self.start_chase(id, uid, suspect, g) {
            Ok(id)
        } else {
            self.resolve(id, Outcome::Escaped);
            Err("no road for the chase".into())
        }
    }

    /// Chases under way: (incident, fugitive's car, police car).
    pub fn chases_now(&self) -> Vec<(u32, CarId, Option<CarId>)> {
        self.services.chases.iter().map(|c| (c.incident, c.car, self.services.units.get(c.unit).and_then(|u| u.car))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mind::IdleMind;

    fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
        vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
    }

    fn run(w: &mut World, n: usize) {
        let mut m = IdleMind;
        for _ in 0..n {
            w.tick(&mut m);
        }
    }

    /// A grid of streets with civic land and housing along them, lived
    /// in, and a crewed police station.
    fn town() -> (World, UnitId) {
        let mut w = World::new(WorldConfig { emergencies: false, society: false, landscape: crate::landscape::LandscapeParams::flat(), ..WorldConfig::test_town() });
        w.economy.treasury = 1_000_000.0;
        for k in -2..=2 {
            let x = k as f32 * 200.0;
            w.add_road(&[Vec2::new(x, -420.0), Vec2::new(x, 420.0)], 1, 1);
            w.add_road(&[Vec2::new(-420.0, x), Vec2::new(420.0, x)], 1, 1);
        }
        w.add_zone(square(-390.0, 8.0, 780.0, 50.0), LandUse::Civic);
        for y in [-260.0, -60.0, 208.0] {
            w.add_zone(square(-390.0, y, 780.0, 52.0), LandUse::Residential);
        }
        let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Residential && l.usable()).map(|(id, _)| id).take(16).collect();
        for l in lots {
            w.build_on(l);
            if let Some(b) = w.lots[l].building {
                w.finish_construction(b);
            }
        }
        for _ in 0..40 {
            if w.citizens.len() >= 20 {
                break;
            }
            w.immigrate();
        }
        assert!(w.citizens.len() >= 12, "people moved in: {}", w.citizens.len());
        let lot = w
            .lots
            .iter()
            .filter(|(_, l)| l.land_use == LandUse::Civic && l.usable() && l.building.is_none() && l.car_access() && l.area() >= 420.0)
            .min_by(|a, b| a.1.center.x.abs().total_cmp(&b.1.center.x.abs()))
            .map(|(id, _)| id)
            .expect("a civic lot");
        let station = w.build_service(lot, ServiceKind::Police, Tier::Mid).unwrap();
        let crew: Vec<CitizenId> = w.citizens.keys().take(2).collect();
        for c in crew {
            w.citizens[c].work = Some(station);
            w.buildings[station].workers.push(c);
        }
        let mut guard = 0;
        while !w.services.units.values().any(|u| u.station == station) && guard < 20 {
            run(&mut w, 1);
            guard += 1;
        }
        let uid = w.services.units.iter().find(|(_, u)| u.station == station).map(|(id, _)| id).expect("a unit");
        (w, uid)
    }

    /// A chase set up by hand: the unit parked at a scene, a suspect whose
    /// car is parked nearby, and the chase started from there.
    fn chase_case(seed_shift: u64) -> (World, u32, CitizenId) {
        let (mut w, uid) = town();
        w.pull_off_road(uid);
        let suspect = w.citizens.keys().nth(4 + seed_shift as usize).unwrap();
        let shop = w.buildings.iter().find(|(_, b)| b.service.is_none() && b.construction.is_none()).map(|(id, _)| id).unwrap();
        let at = w.building_entrance(shop).unwrap();
        if let Some(old) = w.citizens[suspect].car {
            w.drop_car(old, crate::vehicle::CarEvent::Sold);
        }
        w.give_car_parked(suspect, at + Vec2::new(30.0, 0.0));
        let id = w.report_crime(CrimeKind::Robbery, shop, Some(suspect), None).unwrap();
        {
            let u = &mut w.services.units[uid];
            u.state = UnitState::OnScene;
            u.incident = Some(id);
            u.pos = at;
        }
        let i = w.services.incident_mut(id).unwrap();
        i.unit = Some(uid);
        i.stage = Stage::OnScene;
        let g = w.getaway(suspect, at).expect("their car is within reach");
        assert!(w.start_chase(id, uid, suspect, g), "the chase begins");
        (w, id, suspect)
    }

    #[test]
    fn a_suspect_flees_by_car_and_the_chase_ends_in_a_tag_or_is_lost() {
        let (mut w, id, suspect) = chase_case(0);
        assert_eq!(w.services.incident(id).unwrap().stage, Stage::Pursuit);
        assert!(matches!(w.citizens[suspect].activity, Activity::Driving { .. }), "at the wheel");
        let car = w.services.chases[0].car;
        assert!(w.traffic.cars[car].drive.fleeing && w.traffic.cars[car].drive.profile.ignores_signals, "driving recklessly");
        let mut guard = 0;
        while w.services.incident(id).unwrap().active() && guard < (CHASE_MAX + 10) as usize {
            run(&mut w, 1);
            guard += 1;
        }
        let inc = w.services.incident(id).unwrap();
        assert!(!inc.active(), "the chase ended within its time");
        let rec = inc.chase.as_ref().expect("on the record");
        assert!(rec.ended.is_some() && rec.result.is_some());
        match rec.result.unwrap() {
            ChaseResult::Tagged => {
                assert_eq!(inc.outcome, Some(Outcome::Arrest));
                assert!(w.services.in_custody(suspect), "arrested");
                assert!(!w.traffic.cars.contains_key(car), "the car stopped and was left");
            }
            r => {
                assert_eq!(inc.outcome, Some(Outcome::Escaped), "{r:?}");
                assert!(!w.services.in_custody(suspect));
            }
        }
        assert!(w.services.chases.is_empty());
        assert!(w.services.units.values().all(|u| u.state != UnitState::Pursuing), "the unit is free again");
        assert_eq!(w.services.stats.chases_total, 1);
    }

    /// How chases go, for tuning: `cargo test chase_outcomes -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn chase_outcomes() {
        for k in 0..6 {
            let (mut w, id, _) = chase_case(k);
            let mut n = 0;
            while w.services.incident(id).unwrap().active() && n < 80 {
                run(&mut w, 1);
                n += 1;
            }
            let i = w.services.incident(id).unwrap();
            println!("case {k}: {:?} {:?}", i.outcome, i.chase);
        }
    }

    #[test]
    fn touching_the_fugitive_is_a_tag() {
        let (mut w, id, suspect) = chase_case(1);
        // Under way, then the police car right on the fugitive's tail and
        // closing.
        run(&mut w, 3);
        let ch = w.services.chases[0].clone();
        let pcar = w.services.units[ch.unit].car.unwrap();
        let (lane, s, v) = {
            let f = &w.traffic.cars[ch.car];
            (f.route[f.idx], f.s, f.v)
        };
        let len = w.traffic.net.lanes[lane as usize].length;
        w.traffic.reroute(pcar, vec![lane], (s - 3.5).max(0.0), len - 0.3);
        if let Some(c) = w.traffic.cars.get_mut(pcar) {
            c.entering = false;
            c.lateral = 0.0;
            c.s = (s - 3.5).max(0.0);
            c.v = v + 3.0;
        }
        run(&mut w, 2);
        let inc = w.services.incident(id).unwrap();
        assert_eq!(inc.chase.as_ref().and_then(|c| c.result), Some(ChaseResult::Tagged), "a touch is a tag");
        assert!(w.services.in_custody(suspect));
        assert_eq!(w.services.stats.chases_tagged_total, 1);
    }

    #[test]
    fn chases_are_deterministic() {
        let trace = || {
            let (mut w, id, _) = chase_case(2);
            run(&mut w, 40);
            let i = w.services.incident(id).unwrap();
            (i.stage, i.chase.as_ref().map(|c| (c.metres.to_bits(), c.result)))
        };
        assert_eq!(trace(), trace());
    }
}
