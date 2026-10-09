//! [driving] Crashes and wrecks: the call, the injured, recovery trucks and
//! tows.

use super::*;

impl World {
    /// The building nearest a point, to name a place on the street.
    pub(in crate::world) fn building_near(&self, p: Vec2) -> Option<BuildingId> {
        self.buildings.iter().filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (id, l.center.distance_squared(p)))).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(id, _)| id)
    }

    /// Somebody near enough to see it and call it in (not those in it).
    pub(in crate::world) fn witness(&self, p: Vec2, reach: f32, except: &[CitizenId]) -> Option<CitizenId> {
        self.citizens
            .iter()
            .filter(|(id, c)| !except.contains(id) && !c.asleep && c.age >= 12 && !matches!(c.activity, Activity::OnTrain { .. } | Activity::OnMetro { .. } | Activity::Arriving))
            .map(|(id, c)| (id, c.pos.distance(p)))
            .filter(|(_, d)| *d < reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// Turn a crash from the traffic model into what follows: a scrape and
    /// a few minutes lost, or a write-off, the hurt waiting in the road, a
    /// witness calling 911 and police and ambulances on their way.
    pub(super) fn handle_crash(&mut self, r: CrashReport) {
        let tick = self.clock.tick;
        let mut rng = crate::rng_for(self.stream_seed(Stream::Crash), r.id as u64);
        let cars: Vec<CarId> = r.cars.iter().copied().filter(|&c| self.traffic.cars.contains_key(c)).collect();
        if cars.is_empty() {
            return;
        }
        // Buses, lorries, emergency vehicles and takeover stunts only scrape:
        // nobody is written off in them (their riders and loads are someone
        // else's story).
        for &c in &cars {
            let car = &mut self.traffic.cars[c];
            let protected = car.bus.is_some() || car.cargo.is_some() || car.siren || car.in_stunt();
            if let Some(cs) = car.drive.crash.as_mut().filter(|cs| cs.wrecked && protected) {
                cs.wrecked = false;
                cs.hold = 90.0;
            }
        }
        let near = self.building_near(r.pos);
        let place = near.and_then(|b| self.buildings.get(b)).map(|b| b.name.clone()).unwrap_or_else(|| "the street".to_string());
        let drivers: Vec<CitizenId> = cars.iter().map(|&c| self.traffic.cars[c].citizen).filter(|c| self.citizens.contains_key(*c)).collect();
        let drunk = cars.iter().any(|&c| self.traffic.cars[c].drive.profile.drunk >= DRUNK);
        let bad = r.severity >= BAD_CRASH && cars.iter().any(|&c| self.traffic.cars[c].drive.crash.is_some_and(|cs| cs.wrecked));
        self.driving.crashes_today += 1;
        self.driving.crashes_total += 1;
        // [driving] Out of the lane onto the shoulder, so the traffic
        // behind can ease past (`traffic/hazard.rs`).
        for &c in &cars {
            self.traffic.push_aside(c);
        }
        let mut injured = Vec::new();
        let mut incident = None;
        if bad {
            // [review] Who is to blame: the driver of the car whose fault it
            // was (`cars[0]` gives the cause), if that is a citizen. It took
            // the first citizen driver instead, so a stranger's red light got
            // the citizen they hit arrested. Read before the write-offs below
            // unseat anyone.
            let at_fault = cars.first().map(|&c| &self.traffic.cars[c]).filter(|c| c.drive.profile.drunk >= DRUNK || matches!(r.cause, CrashCause::RedLight | CrashCause::OffRoad)).map(|c| c.citizen).filter(|d| self.citizens.contains_key(*d));
            for &c in &cars {
                let car = &self.traffic.cars[c];
                let driver = car.citizen;
                let bike = car.model.body.two_wheeler();
                let chance = ((r.severity - 0.3) * 1.3 + if bike { 0.25 } else { 0.0 }).clamp(0.0, 0.95);
                let hurt = self.citizens.contains_key(driver) && rng.random::<f32>() < chance;
                let protected = car.bus.is_some() || car.cargo.is_some() || car.siren || car.in_stunt();
                let mut wrecked = car.drive.crash.is_some_and(|cs| cs.wrecked);
                if hurt && !wrecked && !protected {
                    // [review] Hurt, they cannot drive on: the car stays as a
                    // write-off and they wait by it for the ambulance (they
                    // kept driving while an incident waited for them).
                    if let Some(cs) = self.traffic.cars[c].drive.crash.as_mut() {
                        cs.wrecked = true;
                        wrecked = true;
                    }
                }
                // (A bus, lorry, siren or stunt only scrapes: nobody in it is
                // hurt.)
                if hurt && !protected {
                    injured.push(driver);
                }
                if !wrecked {
                    continue;
                }
                let car = &self.traffic.cars[c];
                let (pos, heading, model, owned) = (car.pos, car.heading, car.model, car.owned);
                self.traffic.cars[c].citizen = CitizenId::default();
                // [carlife] Written off: scrapped (the tow truck hauls it to the yard).
                self.traffic.cars[c].owned = None;
                if let Some(vid) = owned {
                    self.drop_car(vid, crate::vehicle::CarEvent::Scrapped);
                }
                // [driving] Hard enough and it rolls: a bike goes down on
                // its side, a car onto its side or its roof.
                // (Its own draw, so the crash's other dice fall as before.)
                let (roll, side) = (car_hash(c, 0xF1 + r.id as u64), car_hash(c, 0xF2 + r.id as u64));
                let flipped = if r.severity > 0.7 && roll < (r.severity - 0.6) * 1.5 {
                    if model.body.two_wheeler() || side < 0.5 { 1 } else { 2 }
                } else {
                    0
                };
                self.driving.wrecks.push(Wreck { car: c, crash: r.id, incident: None, since: tick, retry_at: 0, due: 0, truck: None, sent: 0, company: None, flipped, righted_at: 0 });
                if let Some(p) = self.citizens.get_mut(driver) {
                    // Out of the car, standing (or lying) by it.
                    p.activity = Activity::Idle;
                    p.inside = None;
                    p.pos = pos + Vec2::new(-heading.sin(), heading.cos()) * 2.2;
                    p.prev_pos = p.pos;
                    p.mood = (p.mood - 0.15).max(0.0);
                    p.next_decision_tick = tick + if hurt { crate::services::MEDICAL_GIVE_UP + 120 } else { 30 };
                    p.thought = if hurt { "Everything hurts. Where's the ambulance?".to_string() } else { "The car's totalled.".to_string() };
                    let what = match model.body {
                        b if b.two_wheeler() => "bike",
                        _ => "car",
                    };
                    p.remember(tick, format!("Wrote off my {what} in a crash by {place} ({}).", r.cause.label()));
                    self.concerns.heard(driver, p, crate::concerns::Concern::Commute, 0.6);
                }
            }
            let caller = self.witness(r.pos, 300.0, &drivers);
            if let Some(near) = near {
                if let Some((id, _)) = self.report_crash(r.pos, near, &drivers, &injured, at_fault, caller) {
                    incident = Some(id);
                    for w in self.driving.wrecks.iter_mut().filter(|w| w.crash == r.id) {
                        w.incident = Some(id);
                    }
                }
            }
            if let Some(cid) = caller {
                let c = &mut self.citizens[cid];
                c.remember(tick, format!("Saw a bad crash by {place}. Called it in."));
                c.thought = "Called 911 about a crash.".to_string();
            }
            self.driving.injuries_today += injured.len() as u32;
        } else {
            for &d in &drivers {
                let c = &mut self.citizens[d];
                c.remember(tick, format!("Fender bender by {place}. Lost a few minutes swapping details."));
                self.concerns.heard(d, c, crate::concerns::Concern::Commute, 0.2);
                c.mood = (c.mood - 0.03).max(0.0);
            }
        }
        self.events.push(SimEvent::Crash(r.id));
        self.driving.recent.push(CrashRecord { id: r.id, tick, pos: r.pos, cause: r.cause, severity: r.severity, drivers, injured, drunk, incident, place });
        if self.driving.recent.len() > KEEP_CRASHES {
            self.driving.recent.remove(0);
        }
    }

    /// [services] Wrecks are towed: once the police are on scene (or have
    /// been and gone, or nobody reported it) the nearest tow company with a
    /// truck free sends one (from out of town, in from the map edge, when
    /// the city has none). It drives to the wreck with its ambers on, and
    /// close to it takes the off-law moves round the queue (`Car::offlaw`);
    /// there it hooks the wreck and hauls it to the yard or off the map.
    /// With no road to the wreck, or a truck not there within
    /// `TOW_ARRIVE_MAX`, the wreck is hooked where it lies by one from the
    /// far side (`tow`). Nothing is simply deleted but a wreck no road
    /// leads away from at all (`TOW_GIVE_UP`).
    pub(super) fn step_wrecks(&mut self) {
        let tick = self.clock.tick;
        let wrecks = std::mem::take(&mut self.driving.wrecks);
        let mut keep = Vec::new();
        for mut w in wrecks {
            if !self.traffic.cars.contains_key(w.car) {
                if let Some(t) = w.truck {
                    self.recovery_truck_home(t, w.company);
                }
                continue;
            }
            // Out of the way of the traffic behind (wrecks from older saves).
            self.traffic.push_aside(w.car);
            if let Some(t) = w.truck {
                let wreck_pos = self.traffic.cars[w.car].pos;
                let to_go = self.traffic.cars.get(t).filter(|c| c.idx < c.route.len()).map(|c| self.traffic.route_metres(t, &c.route[c.idx..], c.s_end));
                match self.traffic.cars.get_mut(t).filter(|c| c.errand == Errand::Recover(w.car)) {
                    Some(truck) => {
                        truck.drive.amber = true;
                        truck.drive.recovering = truck.pos.distance(wreck_pos) < RECOVERY_REACH || to_go.is_some_and(|m| m < RECOVERY_ROUTE);
                        let allowed = ((truck.drive.expected_min * 2.0) as u64).clamp(TOW_ARRIVE_MAX, TOW_ARRIVE_CAP);
                        let late = w.righted_at == 0 && (tick >= w.sent + allowed || truck.stalled > TOW_STALL_MIN * self.config.traffic_seconds_per_tick);
                        if late && self.tow(w.car) {
                            self.recovery_truck_home(t, w.company);
                            continue;
                        }
                        keep.push(w);
                        continue;
                    }
                    // Lost (the roads were rebuilt under it): call again.
                    None => w.truck = None,
                }
            }
            let called = match w.incident {
                Some(i) => self.services.incident(i).is_none_or(|i| !i.active() || i.stage == IncidentStage::OnScene) || tick >= w.since + TOW_CALL_MAX,
                None => tick >= w.since + TOW_UNREPORTED,
            };
            if !called || tick < w.retry_at {
                keep.push(w);
                continue;
            }
            let due = if w.due == 0 { tick } else { w.due };
            if let Some((truck, company)) = self.send_recovery(w.car) {
                keep.push(Wreck { truck: Some(truck), sent: tick, company, due, ..w });
                continue;
            }
            if self.tow(w.car) {
                continue;
            }
            if tick >= due + TOW_GIVE_UP {
                // See TOW_GIVE_UP: craned off where it lies.
                self.traffic.remove_car(w.car);
                self.driving.towed_total += 1;
                continue;
            }
            keep.push(Wreck { retry_at: tick + TOW_RETRY, due, ..w });
        }
        self.driving.wrecks = keep;
        let before = self.driving.tows.len();
        let traffic = &self.traffic;
        self.driving.tows.retain(|t| traffic.cars.contains_key(t.truck));
        self.driving.towed_total += (before - self.driving.tows.len()) as u32;
    }

    /// [services] Send a recovery truck to the wreck `car`: from the
    /// nearest tow company with one free, else from out of town (which
    /// always has one for a wreck: `OUT_OF_TOWN_TOW_TRUCKS` caps its
    /// parking tows, not its recoveries, so a crash never waits on it). It aims
    /// for just behind the wreck on its lane. `None` when no road leads
    /// there from either.
    pub(super) fn send_recovery(&mut self, car: CarId) -> Option<(CarId, Option<BuildingId>)> {
        let c = self.traffic.cars.get(car)?;
        let (lane, pos) = (c.route[c.idx], c.pos);
        let len = self.traffic.net.lanes[lane as usize].length;
        let back = c.s - c.length() - 2.5;
        // No room behind it on its own lane (a short connector, the very
        // start of a lane): stop at the end of the lane leading in instead.
        // Aimed at the wreck's own lane it could never get there, held at
        // the lane's end by the wreck itself.
        let (lane, goal_s) = match self.traffic.net.lanes[lane as usize].prev.first().copied() {
            Some(p) if back < 0.5 => (p, (self.traffic.net.lanes[p as usize].length + back).max(0.5).min(self.traffic.net.lanes[p as usize].length)),
            _ => (lane, back.clamp(0.5f32.min(len), len)),
        };
        let company = self.tow_company_near(pos);
        let from_yard = company.and_then(|y| self.truck_stop(y)).and_then(|(seg, pt)| {
            let mut lanes: Vec<(crate::traffic::LaneId, f32, f32)> = self
                .traffic
                .net
                .segment_lanes
                .get(&seg)?
                .iter()
                .map(|&l| {
                    let (s, d) = self.traffic.net.lanes[l as usize].project(pt);
                    (l, s, d)
                })
                .collect();
            lanes.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.0.cmp(&b.0)));
            lanes.into_iter().find_map(|(l, s, _)| self.traffic.net.route_lanes(l, s, lane, goal_s))
        });
        let (route, company) = match from_yard {
            Some(r) => (r, company),
            None => (self.route_from_edge_to(lane, goal_s)?, None),
        };
        let (route, s0, s1) = route;
        let truck = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, 3);
        let t = &mut self.traffic.cars[truck];
        t.model = super::enforcement::tow_model();
        t.errand = Errand::Recover(car);
        t.drive.profile = DriverProfile::professional(car_hash(truck, 1));
        t.drive.amber = true;
        // [rage] Timed, but a tow driver keeps their head (no calm profile).
        t.drive.trip_start = self.clock.tick;
        t.drive.expected_min = (t.expected_seconds(&self.traffic.net) / self.config.traffic_seconds_per_tick.max(0.1)).max(1.0);
        Some((truck, company))
    }

    /// [services] A recovery truck at the end of its route: at the wreck,
    /// it hooks it and hauls it away; short of it (re-routed round a jam)
    /// it is called off and another sent; with the wreck gone meanwhile it
    /// goes back empty.
    pub(in crate::world) fn recovery_truck_arrived(&mut self, truck: CarId, wreck: CarId) {
        let Some(k) = self.driving.wrecks.iter().position(|w| w.car == wreck && w.truck == Some(truck)) else {
            self.recovery_truck_home(truck, None);
            return;
        };
        let company = self.driving.wrecks[k].company;
        let near = match (self.traffic.cars.get(truck), self.traffic.cars.get(wreck)) {
            (Some(t), Some(w)) => t.pos.distance(w.pos) < HOOK_REACH,
            _ => false,
        };
        if !near {
            self.driving.wrecks[k].truck = None;
            self.recovery_truck_home(truck, company);
            return;
        }
        // [driving] On its side or roof: rolled back onto its wheels first,
        // the truck standing by (it is reported here every tick it stands
        // at the end of its route).
        let tick = self.clock.tick;
        let wr = &mut self.driving.wrecks[k];
        if wr.flipped > 0 {
            if wr.righted_at == 0 {
                wr.righted_at = tick + RIGHTING_MIN;
            }
            if tick < wr.righted_at {
                return;
            }
            wr.flipped = 0;
        }
        self.driving.wrecks.remove(k);
        let model = self.traffic.cars[wreck].model;
        self.traffic.remove_car(wreck);
        self.enforcement.stats.wrecks_towed_total += 1;
        self.driving.tows.push(Tow { truck, model, yard: company });
        // [tow-moves] Turned round in the street when the yard lies back
        // the other way.
        if let Some(goal) = self.yard_goal(company) {
            if let Some(t) = self.traffic.cars.get_mut(truck) {
                t.drive.recovering = false;
            }
            if self.maybe_u_turn(truck, goal, super::tow_moves::AfterMove::Home { company }) {
                return;
            }
        }
        self.recovery_truck_home(truck, company);
    }

    /// [services] A recovery truck done (or called off): lights off, back
    /// with its load to its yard (put away there, `unclaimed_arrival`), or
    /// off the map for one from out of town or one going back empty. With
    /// no road there it pulls in and tries again (`retire_vehicle`).
    pub(in crate::world) fn recovery_truck_home(&mut self, truck: CarId, company: Option<BuildingId>) {
        let Some(t) = self.traffic.cars.get_mut(truck) else { return };
        t.drive.amber = false;
        t.drive.recovering = false;
        t.errand = Errand::Trip;
        // Empty, it is not put away at a yard: off the map it goes.
        if !self.driving.tows.iter().any(|x| x.truck == truck) {
            self.retire_vehicle(truck);
            return;
        }
        let home = company.filter(|c| self.buildings.contains_key(*c)).and_then(|c| self.truck_stop(c)).and_then(|(seg, pt)| self.traffic.net.nearest_lane_on(seg, pt));
        if let Some((goal, s_end, _)) = home {
            if self.traffic.redirect(truck, goal, s_end) {
                return;
            }
        }
        // Off the map with its load by any road out: it is put away at the
        // end of that road (`unclaimed_arrival`, a tow with no yard).
        if company.is_none() {
            let pos = self.traffic.cars[truck].pos;
            if self.any_exits(pos).into_iter().any(|(g, se)| self.traffic.redirect(truck, g, se)) {
                return;
            }
        }
        self.retire_vehicle(truck);
    }

    /// Replace a car with a tow truck carrying it to a yard, or off the map
    /// by the nearest exit it can reach. False (and the wreck stays where it
    /// is) when no road leads to either from it.
    pub(super) fn tow(&mut self, car: CarId) -> bool {
        let Some(c) = self.traffic.cars.get(car) else { return true };
        let (lane, s, model, pos) = (c.route[c.idx], c.s, c.model, c.pos);
        // [parking] The city's tow companies haul wrecks too; with none in
        // town (or no road to it), one from out of town takes it off the map.
        let mut yard = self.tow_company_near(pos);
        let to_yard = yard.and_then(|y| self.car_stop(y)).and_then(|(seg, p)| {
            let (gl, gs, _) = self.traffic.net.nearest_lane_on(seg, p)?;
            self.traffic.net.route_lanes(lane, s, gl, gs)
        });
        // [services] Any exit it can reach: only the nearest road end was
        // tried, and with no route from the wreck to that one every wreck in
        // a city without a tow yard stood for a day before it was craned off.
        let route = match to_yard {
            Some(r) => Some(r),
            None => {
                yard = None;
                self.any_exits(pos).into_iter().find_map(|(g, se)| self.traffic.net.route_lanes(lane, s, g, se))
            }
        };
        let Some((route, s0, s1)) = route else { return false };
        self.enforcement.stats.wrecks_towed_total += 1;
        self.traffic.remove_car(car);
        let truck = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, 0);
        let t = &mut self.traffic.cars[truck];
        t.model = VehicleModel { class: VehicleClass::Mid, body: BodyType::Pickup, wear: 1, tuned: false, livery: Livery::Utility, paint: 3 };
        t.drive.profile = DriverProfile::professional(car_hash(truck, 1));
        // [rage] Timed, but a tow driver keeps their head (no calm profile).
        t.drive.trip_start = self.clock.tick;
        t.drive.expected_min = (t.expected_seconds(&self.traffic.net) / self.config.traffic_seconds_per_tick.max(0.1)).max(1.0);
        self.driving.tows.push(Tow { truck, model, yard });
        true
    }

    /// [services] Tow `car` now, or (no road away from it) queue it with
    /// the wrecks to be tried again (`step_wrecks`).
    pub(in crate::world) fn tow_or_queue(&mut self, car: CarId) {
        if self.tow(car) {
            return;
        }
        let tick = self.clock.tick;
        self.driving.wrecks.push(Wreck { car, crash: 0, incident: None, since: tick.saturating_sub(TOW_UNREPORTED), retry_at: tick + TOW_RETRY, due: tick, truck: None, sent: 0, company: None, flipped: 0, righted_at: 0 });
    }

    /// Tow trucks on the road and what they carry.
    pub fn tows(&self) -> &[Tow] {
        &self.driving.tows
    }
}
