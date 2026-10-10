//! [carlife] Setting off by car, and how the trip felt.

use super::*;

impl World {
    /// The citizen's car, if it is parked within reach of `near`.
    pub(in crate::world) fn car_ready(&self, id: CitizenId, near: Vec2) -> Option<(VehicleId, Vec2)> {
        // `owns_car` mirrors `car`; a test or a tool clearing it means "no car".
        let c = self.citizens.get(id).filter(|c| c.owns_car)?;
        let vid = c.car?;
        let spot = self.vehicles.cars.get(vid)?.spot()?;
        // [boot] A clamped car goes nowhere.
        (!matches!(spot.kind, ParkKind::Dealer | ParkKind::Hooked | ParkKind::Pound) && !spot.booted && spot.pos.distance(near) <= CAR_REACH).then_some((vid, spot.pos))
    }

    /// Get in and drive to `target`. False when the car cannot get there
    /// (no route); the citizen is then left as they were.
    pub(in crate::world) fn start_drive(&mut self, id: CitizenId, vid: VehicleId, target: BuildingId, purpose: Purpose) -> bool {
        crate::prof_scope!("start_drive");
        let Some(lot) = self.building_lot(target) else { return false };
        let Some((ct_seg, ct_pt)) = lot.car_segment.map(|s| (s, lot.car_point)) else { return false };
        let spot = self.vehicles.cars.get(vid).and_then(|c| c.spot().copied());
        if spot.is_some_and(|s| s.kind == ParkKind::Hooked) {
            return false;
        }
        let Some(car) = self.drive_from_spot(vid, ct_seg, ct_pt, id, Errand::Trip) else { return false };
        // [parking] Fetching it from the pound: pay to get it out.
        if let Some(s) = spot.filter(|s| s.kind == ParkKind::Pound) {
            self.pay_release(id, s);
        }
        self.leave_building(id);
        let (pos, heading, model) = {
            let c = &self.traffic.cars[car];
            (c.pos, c.heading, c.model)
        };
        let c = &mut self.citizens[id];
        c.pos = pos;
        c.prev_pos = pos;
        c.heading = heading;
        c.vehicle = Some(model);
        c.to_car = None;
        c.commute.drive_since = self.clock.tick.max(1);
        c.activity = Activity::Driving { car, to: Some(target), purpose };
        true
    }

    /// Walk to the parked car first, then drive (`arrive` picks it up).
    pub(in crate::world) fn walk_to_car(&mut self, id: CitizenId, vid: VehicleId, at: Vec2, target: BuildingId, purpose: Purpose, from: Vec2) {
        crate::prof_scope!("walk_to_car");
        let exit = self.leave_building(id);
        let start = exit.unwrap_or(from);
        let mut path = match self.walk.route(start, at) {
            Some((pts, _)) => pts,
            None => vec![start],
        };
        path.push(at);
        let c = &mut self.citizens[id];
        c.pos = start;
        c.prev_pos = start;
        c.mode = TravelMode::Walk;
        c.to_car = Some(ToCar { car: vid, to: target, purpose });
        c.activity = Activity::Traveling { to: Some(target), purpose, path, idx: 0 };
    }

    /// The trip under way began: note its mode and when.
    pub(in crate::world) fn begin_trip(&mut self, id: CitizenId, mode: TripMode, distance: f32) {
        crate::prof_scope!("begin_trip");
        self.clear_stuck(id);
        let tick = self.clock.tick;
        let c = &mut self.citizens[id];
        c.commute.mode = Some(mode);
        c.commute.started = tick;
        c.commute.distance = distance;
        c.commute.searched = 0.0;
        c.commute.search_since = 0;
        c.commute.fee = 0.0;
        c.commute.toll = 0.0;
        c.commute.drive_since = 0;
        c.commute.drive_minutes = 0.0;
        c.commute.walk_after = 0.0;
        c.commute.rage = 0.0;
        c.commute.expected_drive = 0.0;
        self.vehicles.stats.trips_today[mode.index()] += 1;
    }

    /// Riders who waited for hours and gave up: transit let them down.
    pub(in crate::world) fn transit_failed(&mut self, id: CitizenId) {
        let tick = self.clock.tick;
        let Some(c) = self.citizens.get_mut(id) else { return };
        let mode = c.commute.mode.filter(|m| matches!(m, TripMode::Bus | TripMode::Train | TripMode::Subway)).unwrap_or(TripMode::Bus);
        c.commute.feel(mode, 3.0);
        c.remember(tick, match mode {
            TripMode::Train => "Waited hours for a train that never came.",
            TripMode::Subway => "Stood on the metro platform for hours. Nothing came.",
            _ => "Waited hours for a bus that never came.",
        });
        self.concerns.heard(id, c, crate::concerns::Concern::Commute, 0.5);
    }

    /// The trip is over: how bad was it? Slow trips hurt by mode (a long
    /// walk, a slow ride, a jam), and so do searching for a space and the
    /// fee. Memories of the worst.
    pub(in crate::world) fn end_trip(&mut self, id: CitizenId) {
        let tick = self.clock.tick;
        let secs = self.config.traffic_seconds_per_tick;
        let (walk, bike) = (self.config.walk_speed.max(0.1), self.config.bike_speed.max(0.1));
        let Some(c) = self.citizens.get_mut(id) else { return };
        let Some(mode) = c.commute.mode.take() else { return };
        let minutes = tick.saturating_sub(c.commute.started) as f32;
        let d = c.commute.distance;
        let frail = matches!(c.stage(), crate::citizens::LifeStage::Child | crate::citizens::LifeStage::Retiree);
        // Minutes over what the trip "should" take, in bad half-hours.
        let pain = match mode {
            TripMode::Walk => (minutes - 25.0).max(0.0) / 30.0 * if frail { 1.4 } else { 1.0 },
            TripMode::Bike => (minutes - 35.0).max(0.0) / 30.0 + (d / bike - 60.0).max(0.0) / 60.0,
            // [pt-transit] Against twice walking pace door to door, plus
            // a wait: the walks to and from the stop are part of any ride,
            // and four times walking pace (the old mark) no bus here could
            // make on a short trip, so every rider came off it hurting and
            // chose the bus less next time.
            TripMode::Bus | TripMode::Train | TripMode::Subway => (minutes - (d / (walk * 2.0) + 25.0)).max(0.0) / 30.0,
            // [taxis] The wait at the kerb and the ride against a drive.
            TripMode::Taxi => (minutes - (d * 1.3 / (9.0 * secs) + 12.0)).max(0.0) / 30.0,
            TripMode::Car => {
                // The drive against free flow (about 9 m/s door to door);
                // the hunt for a space, the fee and the walk from the car
                // are the parking's share.
                let free = d * 1.3 / (9.0 * secs) + 4.0;
                let driving = if c.commute.drive_minutes > 0.0 { c.commute.drive_minutes - c.commute.searched / secs } else { minutes };
                let jam = (driving - free * 2.0 - 5.0).max(0.0) / 30.0;
                let search = c.commute.searched / secs / 20.0;
                let fee = c.commute.fee / 6.0;
                let walk_back = (c.commute.walk_after - 80.0).max(0.0) / 200.0;
                let parking = search + fee + walk_back;
                c.commute.parking_pain += (parking - c.commute.parking_pain) * 0.4;
                // [tolls] Tolls hurt like a fee, and are expected next time.
                let toll = c.commute.toll;
                c.commute.toll_avg += (toll - c.commute.toll_avg) * 0.4;
                // [rage] Losing your temper makes a bad drive feel worse.
                jam + parking + toll / 6.0 + c.commute.rage * 0.8
            }
        };
        c.commute.feel(mode, pain);
        c.commute.last_minutes = minutes as u32;
        // [economy] A long walk: a bike would have helped (bike shops).
        if mode == TripMode::Walk && minutes >= 25.0 {
            self.pressure.long_walks_today += 1;
        }
        let Some(c) = self.citizens.get_mut(id) else { return };
        if mode == TripMode::Car && c.commute.rage >= 0.5 {
            // [rage] Always remembered: it is a story they will tell.
            let expected = c.commute.expected_drive.max(1.0);
            c.remember(tick, format!("Lost my temper in traffic: {minutes:.0} minutes for what should be a {expected:.0}-minute drive."));
            self.concerns.heard(id, c, crate::concerns::Concern::Commute, 0.3);
        } else if pain >= 1.5 && (tick + id_hash(id)) % 3 == 0 {
            let text = match mode {
                TripMode::Car if c.commute.searched / secs > 15.0 => format!("{:.0} minutes hunting for a parking space. Driving here is a chore.", c.commute.searched / secs),
                TripMode::Car => format!("{minutes:.0} minutes stuck in traffic. There has to be a better way."),
                TripMode::Walk => format!("{minutes:.0} minutes on foot. My feet are done."),
                TripMode::Bike => format!("{minutes:.0} minutes in the saddle. Too far to ride."),
                TripMode::Bus | TripMode::Train | TripMode::Subway => format!("{minutes:.0} minutes on transit for that. Crawling."),
                TripMode::Taxi => format!("{minutes:.0} minutes for a cab ride. Waited forever at the kerb."),
            };
            c.remember(tick, text);
            let k = if mode == TripMode::Car && c.commute.searched / secs > 15.0 { crate::concerns::Concern::Parking } else { crate::concerns::Concern::Commute };
            self.concerns.heard(id, c, k, (pain / 4.0).clamp(0.3, 0.8));
        }
    }
}
