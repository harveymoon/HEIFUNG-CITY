//! [land] Convoys: a billionaire's cars on the road, with the escort.

use super::*;

impl World {
    /// Every tick: a billionaire who sets off in their car gets the convoy
    /// (black cars behind, a police car ahead for the richest); a convoy
    /// whose boss has parked leaves guards at the kerb for an hour and the
    /// escorts drive on to the end of the route and leave.
    pub(in crate::world) fn step_convoys(&mut self) {
        if self.estate.elites.is_empty() && self.estate.convoys.is_empty() {
            return;
        }
        for i in 0..self.estate.elites.len() {
            let cid = self.estate.elites[i].citizen;
            let Some(Activity::Driving { car, .. }) = self.citizens.get(cid).map(|c| &c.activity) else { continue };
            let car = *car;
            if self.traffic.cars.contains_key(car) && !self.estate.convoys.iter().any(|c| c.boss == car) {
                self.form_convoy(i, car);
            }
        }
        // The boss changed course (a new errand, a parking hunt): the
        // detail goes where it goes, from where each car stands.
        let mut turns: Vec<(CarId, crate::traffic::LaneId, f32)> = Vec::new();
        for conv in &self.estate.convoys {
            let Some(b) = self.traffic.cars.get(conv.boss) else { continue };
            let (Some(&goal), s_end) = (b.route.last(), b.s_end) else { continue };
            for id in conv.escorts.iter().chain(conv.police.iter()) {
                if self.traffic.cars.get(*id).is_some_and(|c| c.route.last() != Some(&goal)) {
                    turns.push((*id, goal, s_end));
                }
            }
        }
        for (id, goal, s_end) in turns {
            self.traffic.redirect(id, goal, s_end);
        }
        // Escorts drive like a trained detail: the boss's pace, a touch
        // quicker to close up, a short headway, and through an amber when
        // the supercar went.
        for conv in &self.estate.convoys {
            let Some((boss, at)) = self.traffic.cars.get(conv.boss).map(|b| (b.drive.profile, b.pos)) else { continue };
            if !boss.assigned {
                continue;
            }
            for id in conv.escorts.iter().chain(conv.police.iter()) {
                if let Some(car) = self.traffic.cars.get_mut(*id) {
                    let mut p = boss;
                    // A detail keeps together through the lights, and one
                    // left behind closes up fast.
                    let behind = car.pos.distance(at);
                    p.speed = boss.speed * if behind > 60.0 { 1.3 } else { 1.06 };
                    p.ignores_signals = true;
                    p.headway = (boss.headway * 0.7).max(0.8);
                    p.accel = boss.accel * 1.1;
                    p.reaction = 0.4;
                    p.compliance = boss.compliance.min(0.5);
                    p.skill = 1.0;
                    p.drunk = 0.0;
                    car.drive.profile = p;
                }
            }
        }
        let tick = self.clock.tick;
        let cars = &self.traffic.cars;
        let citizens = &self.citizens;
        for c in &mut self.estate.convoys {
            c.escorts.retain(|e| cars.contains_key(*e));
            c.police = c.police.filter(|p| cars.contains_key(*p));
            if c.stop.is_none() && !cars.contains_key(c.boss) {
                let at = citizens.get(c.citizen).map_or(Vec2::ZERO, |x| x.pos);
                c.stop = Some((at, tick + 60));
            }
        }
        self.estate.convoys.retain(|c| cars.contains_key(c.boss) || !c.escorts.is_empty() || c.police.is_some() || c.stop.is_some_and(|(_, t)| t > tick));
    }

    pub(super) fn form_convoy(&mut self, elite: usize, boss: CarId) {
        let Some(car) = self.traffic.cars.get(boss) else { return };
        if car.idx >= car.route.len() {
            return;
        }
        let route: Vec<crate::traffic::LaneId> = car.route[car.idx..].to_vec();
        let (s, s_end) = (car.s, car.s_end);
        let e = &self.estate.elites[elite];
        let (cid, n, police) = (e.citizen, e.escorts, e.police);
        // Tight behind the supercar, one car length and a gap apart, so
        // they pull out on its tail rather than waiting for a gap of their
        // own; the police car a little ahead.
        let lane_len = self.traffic.net.lanes.get(route[0] as usize).map_or(s, |l| l.length);
        let mut escorts = Vec::new();
        for k in 0..n {
            let at = (s - 7.5 * (k as f32 + 1.0)).max(0.0);
            let id = self.traffic.spawn_car(CitizenId::default(), None, route.clone(), at, s_end, 0);
            if let Some(c) = self.traffic.cars.get_mut(id) {
                c.model = escort_model();
            }
            escorts.push(id);
        }
        let police = police.then(|| {
            let ahead = if route.len() > 1 { (s + 9.0).min(lane_len - 0.5).max(s) } else { s };
            let id = self.traffic.spawn_car(CitizenId::default(), None, route.clone(), ahead, s_end, 3);
            if let Some(c) = self.traffic.cars.get_mut(id) {
                c.model = police_escort_model();
            }
            id
        });
        self.estate.convoys.push(Convoy { citizen: cid, boss, escorts, police, stop: None });
        self.estate.stats.convoys_total += 1;
    }

    /// An escort or police car of a convoy reached the end of its route:
    /// it leaves the road (it drives off to wherever such cars wait).
    pub(in crate::world) fn convoy_car_arrived(&mut self, car: CarId) -> bool {
        let Some(conv) = self.estate.convoys.iter().find(|c| c.escorts.contains(&car) || c.police == Some(car)) else { return false };
        // The boss drives on (a parking hunt, a new errand): so does the
        // detail.
        if let Some(goal) = self.traffic.cars.get(conv.boss).and_then(|b| b.route.last().map(|g| (*g, b.s_end))) {
            if self.traffic.redirect(car, goal.0, goal.1) && self.traffic.cars.get(car).is_some_and(|c| c.idx + 1 < c.route.len() || c.s + 1.0 < c.s_end) {
                return true;
            }
        }
        for c in &mut self.estate.convoys {
            c.escorts.retain(|e| *e != car);
            if c.police == Some(car) {
                c.police = None;
            }
        }
        self.traffic.remove_car(car);
        true
    }

    /// The driver brings a billionaire's supercar round to `near` when it
    /// stands parked far off (they never walk to the car).
    pub(in crate::world) fn bring_car(&mut self, cid: CitizenId, near: Vec2) {
        let Some(vid) = self.citizens.get(cid).and_then(|c| c.car) else { return };
        let far = self.vehicles.cars.get(vid).and_then(|c| c.spot()).is_some_and(|s| s.pos.distance(near) > 120.0);
        if !far {
            return;
        }
        let home = self.citizens[cid].home;
        let Some(spot) = self.place_near(near, home) else { return };
        self.unpark(vid);
        self.park_at(vid, spot);
    }

    /// Dev and test hook: send a citizen somewhere (a billionaire's car is
    /// brought round first, so they drive when it is far enough).
    pub fn send_citizen(&mut self, cid: CitizenId, to: BuildingId) -> bool {
        if !self.citizens.contains_key(cid) || !self.buildings.contains_key(to) {
            return false;
        }
        if self.estate.is_elite(cid) {
            let at = self.citizens[cid].pos;
            self.bring_car(cid, at);
        }
        self.travel_to(cid, Some(to), Purpose::Visit(LandUse::Commercial));
        true
    }
}
