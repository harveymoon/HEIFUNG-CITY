//! [carlife] Arriving: looking for a space, paying, and walking the rest.

use super::*;

impl World {
    /// A vehicle of ours reached the end of its route. True when handled.
    /// (`step_traffic` in `mod.rs` hands through cars, buses,
    /// service units, convoys, garbage trucks and lorries elsewhere first.)
    pub(in crate::world) fn vehicle_arrived(&mut self, car_id: CarId) -> bool {
        let Some(car) = self.traffic.cars.get(car_id) else { return true };
        let (errand, owned, citizen) = (car.errand, car.owned, car.citizen);
        match errand {
            Errand::Haul { base } => {
                self.send_back(car_id, base);
                true
            }
            Errand::Return(b) => {
                self.traffic.remove_car(car_id);
                if self.buildings.contains_key(b) {
                    *self.vehicles.fleet_parked.entry(b).or_insert(0) += 1;
                    self.vehicles.stats.fleet_returned_today += 1;
                }
                true
            }
            Errand::Leave => {
                self.traffic.remove_car(car_id);
                if let Some(vid) = owned {
                    self.drop_car(vid, CarEvent::Emigrated);
                }
                true
            }
            // [carlife] Onto the dealer's forecourt, or out of town.
            Errand::Sale(d) => {
                self.sale_arrived(car_id, d, owned);
                true
            }
            Errand::Patrol(office) => {
                self.patrol_arrived(car_id, office);
                true
            }
            Errand::Tow(job) => {
                self.tow_truck_arrived(car_id, job);
                true
            }
            Errand::Recover(wreck) => {
                self.recovery_truck_arrived(car_id, wreck);
                true
            }
            Errand::Taxi(cab) => {
                self.cab_arrived(car_id, cab);
                true
            }
            Errand::Out => {
                self.out_arrived(car_id);
                true
            }
            // Its route ends at a highway exit off the map.
            Errand::Through => {
                self.traffic.remove_car(car_id);
                true
            }
            Errand::Deliver | Errand::Trip | Errand::Search { .. } | Errand::ToLot(_) | Errand::ToKerb => {
                if owned.is_some() || self.citizens.contains_key(citizen) {
                    self.private_arrived(car_id);
                    true
                } else {
                    false
                }
            }
        }
    }

    /// A truck or van done with its job drives back to `base`, or out of
    /// town. With no road to either it holds at the kerb and tries again
    /// (`retire_vehicle`): it no longer vanishes where it stands.
    pub(super) fn send_back(&mut self, car_id: CarId, base: Option<BuildingId>) {
        if let Some(b) = base.filter(|b| self.buildings.contains_key(*b)) {
            if let Some((seg, pt)) = self.truck_stop(b) {
                if let Some((goal, s_end, _)) = self.traffic.net.nearest_lane_on(seg, pt) {
                    if self.traffic.redirect(car_id, goal, s_end) {
                        self.traffic.cars[car_id].errand = Errand::Return(b);
                        self.traffic.pull_in(car_id, 45.0);
                        return;
                    }
                }
            }
        }
        if self.redirect_out(car_id) {
            self.traffic.cars[car_id].errand = Errand::Leave;
            return;
        }
        self.retire_vehicle(car_id);
    }

    /// The business a truck or van belongs to and returns to: the sender,
    /// when it is a working business with a street. Hook for fleet lots: a
    /// business that keeps its lorries elsewhere can answer that here.
    pub(in crate::world) fn truck_base(&self, from: Option<BuildingId>) -> Option<BuildingId> {
        let b = from?;
        let bld = self.buildings.get(b)?;
        (bld.business.is_some() && bld.operating() && self.building_lot(b).is_some_and(|l| l.car_segment.is_some())).then_some(b)
    }

    /// A truck or van was just put on the road by `from`: it takes one of
    /// the business's parked vehicles (or the business buys one) and will
    /// come back when done.
    pub(crate) fn truck_dispatched(&mut self, car: CarId, from: Option<BuildingId>) {
        let base = self.truck_base(from);
        if let Some(b) = base {
            match self.vehicles.fleet_parked.get_mut(&b) {
                Some(n) if *n > 0 => *n -= 1,
                _ => self.vehicles.stats.fleet_bought_total += 1,
            }
        }
        if let Some(c) = self.traffic.cars.get_mut(car) {
            c.errand = Errand::Haul { base };
        }
    }

    /// A private car at the end of its route: park it, or go on looking.
    pub(super) fn private_arrived(&mut self, car_id: CarId) {
        // [passengers] A stop on the way (a child's school): let them out
        // and drive on.
        if self.traffic.cars.get(car_id).is_some_and(|c| c.errand == Errand::Trip) && self.drop_off(car_id) {
            return;
        }
        let (vid, driver, errand, pos) = {
            let c = &self.traffic.cars[car_id];
            (c.owned, c.citizen, c.errand, c.pos)
        };
        let Some(vid) = vid.filter(|v| self.vehicles.cars.contains_key(*v)) else {
            // A car with no record (a save from before cars were kept):
            // the driver gets out; the car parks where it is under them.
            self.traffic.remove_car(car_id);
            self.walk_from_car(driver, pos);
            return;
        };
        let owner = self.vehicles.cars[vid].owner;
        let home = self.citizens.get(owner).and_then(|c| c.home);
        let dest = if errand == Errand::Deliver {
            home
        } else {
            match self.citizens.get(driver).map(|c| &c.activity) {
                Some(Activity::Driving { to, .. }) => *to,
                _ => None,
            }
        };
        let target = dest.and_then(|b| self.building_entrance(b)).unwrap_or(pos);
        // First arrival: the destination's own spaces.
        if matches!(errand, Errand::Trip | Errand::Deliver) {
            if let Some(b) = dest {
                let own = self.buildings.get(b).is_some_and(|bld| !bld.is_parking() && bld.parking_free() > 0);
                if own {
                    let kind = if Some(b) == home { ParkKind::Home } else { ParkKind::Building };
                    if let Some(spot) = self.building_spot(kind, b) {
                        self.finish_park(car_id, vid, spot, 0);
                        return;
                    }
                }
            }
        }
        let tries = match errand {
            Errand::Search { tries } => tries,
            Errand::ToLot(_) | Errand::ToKerb => 1,
            _ => 0,
        };
        if let Errand::ToLot(b) = errand {
            if self.lot_open(b) {
                if let Some(spot) = self.building_spot(ParkKind::Lot, b) {
                    self.pay_lot(driver, b);
                    self.finish_park(car_id, vid, spot, tries);
                    return;
                }
            }
        }
        if errand == Errand::ToKerb {
            if let Some(k) = self.free_kerb_near(pos, 16.0).or_else(|| self.free_illegal_slots_near(pos, 16.0, 1).into_iter().next()) {
                let spot = self.kerb_spot(k);
                self.finish_park(car_id, vid, spot, tries);
                return;
            }
        }
        self.look_for_space(car_id, vid, driver, target, tries);
    }

    /// [parking] A driver who has hunted for a space past `HUNT_GIVE_UP`
    /// and is standing in a queue (`HUNT_STUCK` seconds) does not wait to
    /// reach the street it was making for: it leaves the car at the kerb
    /// where it is (a banned kerb nearby, a free one, else squeezed in).
    /// Hunters going round the same blocks filled a dead-end street and
    /// its junction solid in a player's city, each waiting for the next,
    /// for hours; every one that gives up leaves the gap the rest need.
    pub(in crate::world) fn hunters_give_up(&mut self) {
        let tick = self.clock.tick;
        let stuck: Vec<CarId> = self
            .traffic
            .cars
            .iter()
            .filter(|(_, c)| matches!(c.errand, Errand::Search { .. } | Errand::ToKerb | Errand::ToLot(_)) && !c.entering && c.hold <= 0.0 && !c.crashed() && !c.in_stunt())
            .filter(|(_, c)| {
                let hunted = self.citizens.get(c.citizen).filter(|z| z.commute.search_since > 0).map_or(0, |z| tick.saturating_sub(z.commute.search_since));
                (hunted > HUNT_GIVE_UP && c.v < 0.5 && c.stalled > HUNT_STUCK) || (hunted > HUNT_ABANDON && c.v < HUNT_ABANDON_SPEED)
            })
            .map(|(id, _)| id)
            .collect();
        for id in stuck {
            if let Some(c) = self.traffic.cars.get_mut(id) {
                // Not at the lot or the slot it was making for: a search
                // from here.
                c.errand = Errand::Search { tries: MAX_TRIES };
                self.private_arrived(id);
            }
        }
    }

    /// [parking] A driver hunting for a space (circling the blocks, or
    /// making for a kerb slot further off) who is about to pass the gate of
    /// an open parking lot with room, within reach of where they are going,
    /// turns in there instead of driving on past it (a player's city:
    /// "looking for parking" past three lots with room).
    pub(in crate::world) fn hunters_spot_lots(&mut self) {
        let mut gates: std::collections::HashMap<LaneId, Vec<(BuildingId, f32, Vec2)>> = std::collections::HashMap::new();
        let open: Vec<BuildingId> = self.buildings.keys().filter(|&b| self.lot_open(b)).collect();
        for b in open {
            let Some((seg, pt)) = self.truck_stop(b) else { continue };
            let Some((lane, s, _)) = self.traffic.net.nearest_lane_on(seg, pt) else { continue };
            let Some(center) = self.building_lot(b).map(|l| l.center) else { continue };
            gates.entry(lane).or_default().push((b, s, center));
        }
        if gates.is_empty() {
            return;
        }
        let hunting: Vec<(CarId, LaneId, f32, Errand, CitizenId, Vec2)> = self
            .traffic
            .cars
            .iter()
            .filter(|(_, c)| matches!(c.errand, Errand::Search { .. } | Errand::ToKerb) && c.owned.is_some() && !c.entering && c.hold <= 0.0 && !c.crashed() && !c.in_stunt())
            .filter(|(_, c)| gates.contains_key(&c.route[c.idx]))
            .map(|(id, c)| {
                let last = *c.route.last().unwrap();
                let end = self.traffic.net.lanes[last as usize].point_at(c.s_end).0;
                (id, c.route[c.idx], c.s, c.errand, c.citizen, end)
            })
            .collect();
        for (id, lane, s, errand, driver, end) in hunting {
            let target = match self.citizens.get(driver).map(|c| &c.activity) {
                Some(Activity::Driving { to: Some(b), .. }) => self.building_entrance(*b),
                _ => None,
            };
            let Some(target) = target else { continue };
            let reach = LOT_WALK * 2.0;
            let money_w = self.money_weight(driver);
            let pick = gates[&lane].iter().filter(|(b, gate_s, center)| {
                let ahead = *gate_s - s;
                let fee = self.buildings[*b].parking_price * money_w;
                // Ahead within sight, near enough the destination, and (for
                // a driver making for a free kerb) no further from it than
                // that slot or not dear for this driver.
                (3.0..LOT_SPOT_AHEAD).contains(&ahead) && center.distance(target) <= reach && (matches!(errand, Errand::Search { .. }) || center.distance(target) <= end.distance(target) + 40.0 || fee < 15.0)
            }).min_by(|a, b| a.1.total_cmp(&b.1)).copied();
            let Some((b, gate_s, _)) = pick else { continue };
            if self.traffic.follow(id, vec![lane], gate_s) {
                self.traffic.cars[id].errand = Errand::ToLot(b);
            }
        }
    }

    pub(in crate::world) fn lot_open(&self, b: BuildingId) -> bool {
        self.buildings.get(b).is_some_and(|bld| bld.is_parking() && bld.parking_free() > 0 && bld.business.as_ref().is_some_and(|z| !z.closed))
    }

    /// Money per minute of a citizen's time, as they see it: the poor count
    /// every coin. Minute-equivalents per unit of money.
    pub(in crate::world) fn money_weight(&self, id: CitizenId) -> f32 {
        let income = self.citizens.get(id).map_or(60.0, |c| c.income);
        (300.0 / income.max(25.0)).clamp(1.0, 12.0)
    }

    /// No space where the car stopped: weigh a free kerb slot against a
    /// paid lot near the destination and drive to the better; with neither,
    /// circle the streets around, and after `MAX_TRIES` park anywhere.
    pub(super) fn look_for_space(&mut self, car_id: CarId, vid: VehicleId, driver: CitizenId, target: Vec2, tries: u8) {
        let pos = self.traffic.cars[car_id].pos;
        if tries > 0 || self.citizens.get(driver).is_some_and(|c| c.commute.search_since == 0) {
            // The search clock starts the first time the driver has to look.
            if let Some(c) = self.citizens.get_mut(driver) {
                if c.commute.search_since == 0 {
                    c.commute.search_since = self.clock.tick.max(1);
                }
            }
        }
        // Candidates: the nearest free kerb slots and parking businesses,
        // each costed as the driver sees it: the drive there (by the route
        // it would really take, round the block if need be), the walk back
        // and the fee.
        let walk = self.config.walk_speed.max(0.1);
        let drive = 9.0 * self.config.traffic_seconds_per_tick;
        let money_w = self.money_weight(driver);
        let mut options: Vec<(f32, Vec<LaneId>, f32, Errand, Option<KerbSlot>)> = Vec::new();
        // [parking] How long this driver has been at it. A driver sent to a
        // kerb slot or a lot that was taken on arrival starts that leg at
        // one try again, so only the clock tells how long the hunt has run.
        let hunted = self.citizens.get(driver).filter(|c| c.commute.search_since > 0).map_or(0, |c| self.clock.tick.saturating_sub(c.commute.search_since));
        // After a few fruitless rounds (or a while at it) the driver looks
        // further, and at slots where parking is banned (a fine, maybe the
        // tow truck).
        let desperate = tries >= MAX_TRIES || hunted > HUNT_DESPERATE;
        // [parking] Hours of circling help nobody: past `HUNT_GIVE_UP` the
        // driver stops looking and leaves the car wherever it can.
        let worn_out = hunted > HUNT_GIVE_UP;
        let reach = if desperate { KERB_WALK * 2.0 } else { KERB_WALK };
        let mut slots: Vec<(KerbSlot, f32)> = if worn_out { Vec::new() } else { self.free_kerbs_near(target, reach, 5).into_iter().map(|k| (k, 0.0)).collect() };
        if desperate && !worn_out {
            slots.extend(self.free_illegal_slots_near(target, KERB_WALK, 3).into_iter().map(|k| (k, 12.0)));
        }
        for (k, risk) in slots {
            let walk_min = k.pos.distance(target) / walk + risk;
            if k.pos.distance(pos) < 14.0 {
                // Right here: pull in.
                options.push((walk_min, Vec::new(), 0.0, Errand::ToKerb, Some(k)));
                continue;
            }
            let Some((goal, s_end, _)) = self.traffic.net.nearest_lane_on(k.segment, k.pos) else { continue };
            let Some(route) = self.traffic.route_from_here(car_id, goal, s_end) else { continue };
            let m = self.traffic.route_metres(car_id, &route, s_end);
            options.push((walk_min + m / drive, route, s_end, Errand::ToKerb, None));
        }
        let mut lots: Vec<(f32, BuildingId, f32)> = self
            .buildings
            .iter()
            .filter(|(id, _)| self.lot_open(*id))
            .filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (l.center.distance(target), id, b.parking_price)))
            // [parking] A driver who has been at it a while walks further
            // from a lot, and a lot still beats leaving the car anywhere.
            .filter(|(d, _, _)| *d <= if desperate { LOT_WALK * 2.0 } else { LOT_WALK })
            .collect();
        lots.sort_by(|a, b| a.0.total_cmp(&b.0));
        // [bridge-riders] Worn out, a driver no longer sets off for a lot
        // across town: that sent hunters who had given up (stuck in a
        // queue) off on another long drive, into the next queue, for hours.
        if worn_out {
            lots.clear();
        }
        for (d, b, fee) in lots.into_iter().take(3) {
            let Some((seg, pt)) = self.truck_stop(b) else { continue };
            let Some((goal, s_end, _)) = self.traffic.net.nearest_lane_on(seg, pt) else { continue };
            let Some(route) = self.traffic.route_from_here(car_id, goal, s_end) else { continue };
            let m = self.traffic.route_metres(car_id, &route, s_end);
            options.push((d / walk + m / drive + fee * money_w, route, s_end, Errand::ToLot(b), None));
        }
        let best = options.into_iter().min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, route, s_end, errand, here)) = best {
            if let Some(k) = here {
                let spot = self.kerb_spot(k);
                self.finish_park(car_id, vid, spot, tries);
                return;
            }
            if self.traffic.follow(car_id, route, s_end) {
                self.traffic.cars[car_id].errand = errand;
                return;
            }
        }
        // Nothing free in sight (or no way to it from here: lanes never
        // lead back onto the one the car is on): drive round the block
        // and look again.
        // [review] One search counts once toward the parking trouble, even
        // when the circling finds no street and it gives up at once.
        let mut counted = false;
        if tries < GIVE_UP && !worn_out {
            let r = if desperate { SEARCH_RADIUS * 2.0 } else { SEARCH_RADIUS };
            let mut segs: Vec<SegmentId> = self.roads.segments_in_box(target - Vec2::splat(r), target + Vec2::splat(r)).filter(|&s| self.roads.try_segment(s).is_some_and(|g| g.is_street() && !g.is_highway() && g.lanes() > 0)).collect();
            segs.sort_by_key(|s| s.0);
            if tries == 0 {
                self.vehicles.stats.circled_today += 1;
                self.economy.parking_street_today += 1;
                self.economy.parking_shortage_at = Some(target);
                counted = true;
            }
            for _ in 0..4 {
                let Some(&sid) = segs.choose(&mut self.rng) else { break };
                let (a, b) = self.roads.segment_points(sid);
                let mid = a.lerp(b, 0.5);
                if mid.distance(pos) < 30.0 {
                    continue;
                }
                if let Some((goal, s_end, _)) = self.traffic.net.nearest_lane_on(sid, mid) {
                    if self.traffic.redirect(car_id, goal, s_end) {
                        self.traffic.cars[car_id].errand = Errand::Search { tries: tries + 1 };
                        return;
                    }
                }
            }
        }
        // Gave up: onto the street, wherever.
        if tries == 0 && !counted {
            self.economy.parking_street_today += 1;
            self.economy.parking_shortage_at = Some(target);
        }
        match self.overflow_spot(pos) {
            Some(spot) => self.finish_park(car_id, vid, spot, tries.max(1)),
            None if !worn_out => {
                // Not one free slot within reach: keep driving round.
                if let Some(c) = self.traffic.cars.get_mut(car_id) {
                    c.errand = Errand::Search { tries: MAX_TRIES };
                }
                self.circle(car_id, target);
            }
            None => {
                // An hour of it and not one slot free within 700 m: the car
                // is left where it stands, squeezed in at the kerb (it used
                // to drive round for good: drivers hunting two to ten hours
                // for a space, through the jam they were part of).
                let spot = self.squeezed_spot(pos);
                self.finish_park(car_id, vid, spot, tries.max(1));
            }
        }
    }

    /// [parking] Drive round the block towards some street near `target`.
    pub(super) fn circle(&mut self, car_id: CarId, target: Vec2) {
        let r = SEARCH_RADIUS * 2.0;
        let mut segs: Vec<SegmentId> = self.roads.segments_in_box(target - Vec2::splat(r), target + Vec2::splat(r)).filter(|&s| self.roads.try_segment(s).is_some_and(|g| g.is_street() && !g.is_highway() && g.lanes() > 0)).collect();
        segs.sort_by_key(|s| s.0);
        for _ in 0..6 {
            let Some(&sid) = segs.choose(&mut self.rng) else { break };
            let (a, b) = self.roads.segment_points(sid);
            if let Some((goal, s_end, _)) = self.traffic.net.nearest_lane_on(sid, a.lerp(b, 0.5)) {
                if self.traffic.redirect(car_id, goal, s_end) {
                    return;
                }
            }
        }
    }

    /// Pay a parking business for a space.
    pub(in crate::world) fn pay_lot(&mut self, driver: CitizenId, b: BuildingId) {
        let tick = self.clock.tick;
        let (fee, name) = {
            let bld = &self.buildings[b];
            (bld.parking_price, bld.name.clone())
        };
        if self.vehicles.city_lots.contains_key(&b) {
            // [parking] A lot the city runs: the fee goes to the treasury.
            self.economy.credit_item(Flow::ParkingFees, fee, Item::Building(b));
            self.enforcement.stats.city_lot_fees_today += fee;
            if let Some(bz) = self.buildings[b].business.as_mut() {
                bz.deals += 1;
            }
        } else if let Some(bz) = self.buildings[b].business.as_mut() {
            bz.money += fee;
            bz.revenue_today += fee;
            bz.deals += 1;
        }
        self.economy.parking_fees_today += fee;
        let roll = self.rng.random_bool(0.05);
        if let Some(c) = self.citizens.get_mut(driver) {
            c.savings -= fee;
            c.commute.fee += fee;
            if roll {
                c.remember(tick, format!("Paid {fee:.0} to park at {name}."));
            }
        }
    }

    /// The car is in its space: off the road, into the record; the driver
    /// gets out and walks the rest.
    pub(super) fn finish_park(&mut self, car_id: CarId, vid: VehicleId, spot: ParkSpot, tries: u8) {
        let Some(car) = self.traffic.remove_car(car_id) else { return };
        self.park_at(vid, spot);
        let slot = match spot.kind {
            ParkKind::Home | ParkKind::Building | ParkKind::Dealer | ParkKind::Hooked | ParkKind::Pound => 0,
            ParkKind::Lot => 1,
            ParkKind::Kerb => 2,
            ParkKind::Overflow => 3,
        };
        self.vehicles.stats.parked_today[slot] += 1;
        let tick = self.clock.tick;
        if car.errand == Errand::Deliver {
            let owner = self.vehicles.cars[vid].owner;
            if let Some(c) = self.citizens.get_mut(owner) {
                c.remember(tick, "The new wheels were delivered to the door.");
            }
            return;
        }
        let driver = car.citizen;
        let secs = self.config.traffic_seconds_per_tick;
        let roll = self.rng.random_bool(0.15);
        let Some(c) = self.citizens.get_mut(driver) else { return };
        if c.commute.search_since > 0 {
            let s = tick.saturating_sub(c.commute.search_since) as f32 * secs;
            c.commute.searched += s;
            c.commute.search_since = 0;
            self.vehicles.stats.search_seconds_today += s;
        }
        if c.commute.drive_since > 0 {
            c.commute.drive_minutes = tick.saturating_sub(c.commute.drive_since) as f32;
            c.commute.drive_since = 0;
        }
        // [rage] How the drive went to their head; the anger stays in the car.
        c.commute.rage = c.commute.rage.max(car.drive.anger_peak);
        if car.drive.expected_min > 0.0 {
            c.commute.expected_drive = car.drive.expected_min;
        }
        if spot.kind == ParkKind::Overflow {
            c.mood = (c.mood - 0.03).max(0.0);
            if roll {
                c.remember(tick, "Circled for ages. Nowhere to park, so I left it on the street.");
            }
            self.concerns.said(tick, driver, c, crate::concerns::Concern::Parking, 0.4, "Circled for ages. Nowhere to park, so I left it on the street.");
        } else if tries >= 2 && roll {
            c.remember(tick, "Went round the block twice before I found a space.");
            self.concerns.heard(driver, c, crate::concerns::Concern::Parking, 0.2);
        }
        self.walk_from_car(driver, spot.pos);
    }

    /// Out of the car at `from`: walk to the destination, or be there.
    pub(in crate::world) fn walk_from_car(&mut self, id: CitizenId, from: Vec2) {
        let Some(c) = self.citizens.get(id) else { return };
        let (to, purpose) = match &c.activity {
            Activity::Driving { to, purpose, .. } => (*to, *purpose),
            _ => return,
        };
        let Some(entrance) = to.and_then(|b| self.building_entrance(b)) else {
            let c = &mut self.citizens[id];
            c.pos = from;
            self.arrive(id);
            return;
        };
        if from.distance(entrance) < GET_OUT {
            let c = &mut self.citizens[id];
            c.pos = from;
            self.arrive(id);
            return;
        }
        let mut path = match self.walk.route(from, entrance) {
            Some((pts, _)) => pts,
            None => vec![from],
        };
        path.push(entrance);
        let c = &mut self.citizens[id];
        c.pos = from;
        c.mode = TravelMode::Walk;
        c.commute.walk_after = from.distance(entrance);
        c.activity = Activity::Traveling { to, purpose, path, idx: 0 };
    }
}
