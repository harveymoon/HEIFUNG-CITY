//! Setting off: leaving a building, routing, and the mode of travel.

use super::*;

impl World {
    pub(in crate::world) fn leave_building(&mut self, id: CitizenId) -> Option<Vec2> {
        self.up_from_platform(id);
        if matches!(self.citizens[id].activity, Activity::Waiting { .. }) {
            self.citizens[id].activity = Activity::Idle;
        }
        if let Activity::Sitting { park, at } = self.citizens[id].activity {
            if let Some(b) = self.buildings.get_mut(park) {
                b.visitors = b.visitors.saturating_sub(1);
            }
            // [benches] Neighbours may have got talking; the seat is free.
            self.stand_up(id, at, park);
            self.note_left(id, park);
            self.citizens[id].activity = Activity::Idle;
        }
        let c = &mut self.citizens[id];
        let inside = c.inside.take()?;
        if matches!(c.activity, Activity::Visiting(_)) {
            if let Some(b) = self.buildings.get_mut(inside) {
                b.visitors = b.visitors.saturating_sub(1);
            }
        }
        // [clarity] Where the trip starts, for "coming from".
        self.note_left(id, inside);
        self.building_entrance(inside)
    }

    /// [r4-metroloop] Someone waiting on a metro platform who goes
    /// anywhere else comes up to the station's street entrance first: a
    /// trip planned from the platform (under the middle of the lot) set off
    /// from underground and was snapped onto whatever walkway was nearest,
    /// a viaduct's overhead in Harvey's city. The give-up and line-closed
    /// paths did this already; now every way out does.
    pub(in crate::world) fn up_from_platform(&mut self, id: CitizenId) {
        let Some(c) = self.citizens.get(id) else { return };
        if !c.on_platform() {
            return;
        }
        let Some(TransitLeg::Subway { board, .. }) = c.transit.map(|t| t.leg) else { return };
        if let Some(p) = self.building_entrance(board) {
            let c = &mut self.citizens[id];
            c.pos = p;
            c.prev_pos = p;
        }
    }

    /// Where a citizen currently touches the road network: segment, point, node.
    pub(in crate::world) fn origin_for(&self, id: CitizenId) -> Option<(SegmentId, Vec2, NodeId)> {
        crate::prof_scope!("origin_for");
        let c = &self.citizens[id];
        if let Some(b) = c.inside {
            let lot = self.building_lot(b)?;
            let entrance = lot.entrance();
            return Some((lot.front_segment, entrance, lot.access_node));
        }
        // [ped-highway] From the nearest way people walk: never a highway
        // or ramp (no foot path starts there; standing beside one, every
        // trip used to fail with "No way through").
        let (sid, t, q) = self.roads.nearest_where(c.pos, |s| !s.is_rail() && !s.fronts_nothing()).or_else(|| self.roads.nearest_on_network(c.pos))?;
        // Nor from an end out on a carriageway (a footpath dropped onto a
        // ramp): a cyclist set off by riding out to the ramp and back.
        let near = self.roads.nearest_endpoint(sid, t);
        let seg = self.roads.segment(sid);
        let far = if near == seg.a { seg.b } else { seg.a };
        let on_motorway = |n: NodeId| self.roads.node(n).segments.iter().any(|&s| self.roads.segment(s).alive && self.roads.segment(s).fronts_nothing());
        let start = if on_motorway(near) && !on_motorway(far) { far } else { near };
        Some((sid, q, start))
    }

    pub(in crate::world) fn travel_to(&mut self, id: CitizenId, target: Option<BuildingId>, purpose: Purpose) {
        crate::prof_scope!("travel_to");
        self.travel_to_impl(id, target, purpose, true, true);
    }

    /// Start a journey: on foot, by bike, by car, or by bus or train with a
    /// walk at each end. `fresh` is false for the legs after a ride, so a
    /// journey's footfall is noted once, for the leg that began it.
    pub(in crate::world) fn travel_to_impl(&mut self, id: CitizenId, target: Option<BuildingId>, purpose: Purpose, allow_bus: bool, fresh: bool) {
        self.travel_with(id, target, purpose, allow_bus, fresh, true);
    }

    /// [carlife] `travel_to_impl`, optionally without the car (it was not
    /// where the citizen walked to). The mode is the citizen's choice
    /// (`choose_mode`); if the best one cannot be done after all (no route
    /// for the car), the next best is taken.
    pub(in crate::world) fn travel_with(&mut self, id: CitizenId, target: Option<BuildingId>, purpose: Purpose, allow_bus: bool, fresh: bool, allow_car: bool) {
        let Some(target) = target else { return };
        // [under-construction] Nobody goes out to, shops at or sits in a
        // building that is not open yet: whatever chose it should not have.
        if fresh && matches!(purpose, Purpose::Visit(_) | Purpose::Shop(_) | Purpose::Sit) && self.buildings.get(target).is_some_and(|b| !b.operating()) && self.citizens[id].home != Some(target) {
            return;
        }
        let origin = self.citizens[id].inside;
        // [r7-citizen-story] Where the travel history says they set off
        // from: at home counts even when not inside.
        let log_from = origin.or_else(|| matches!(self.citizens[id].activity, Activity::AtHome).then_some(self.citizens[id].home).flatten());
        self.up_from_platform(id);
        self.citizens[id].transit = None;
        self.citizens[id].to_car = None;
        if fresh && purpose != Purpose::Home {
            self.pack_meal(id);
        }
        if fresh {
            self.citizens[id].pending_footfall.clear();
        }
        // [megablocks] Within one arcology: a lift ride, not the streets.
        if self.trip_inside_megablock(id, origin, target, purpose) {
            return;
        }
        // [ped-highway] Someone out on a highway or ramp steps onto the
        // pavement before setting off.
        if self.citizens[id].inside.is_none() {
            let p = self.citizens[id].pos;
            let to = self.kerbside(p);
            if to != p {
                let c = &mut self.citizens[id];
                c.pos = to;
                c.prev_pos = to;
            }
        }
        let Some((_from_seg, from_pt, from_node)) = self.origin_for(id) else {
            self.citizens[id].activity = Activity::Idle;
            return;
        };
        let Some(lot) = self.building_lot(target) else { return };
        let to_node = lot.access_node;
        let entrance = lot.entrance();
        let seated = matches!(self.citizens[id].activity, Activity::Sitting { .. });
        let walk_from = if self.citizens[id].inside.is_some() { from_pt } else { self.citizens[id].pos };
        // [air] The rich fly when pads serve both ends and a drone beats
        // the drive (`world/air.rs` decides); everyone else ranks the
        // ground modes below. Drone trips keep their own statistics.
        if allow_bus {
            if let Some(plan) = self.consider_air(id, target, purpose, walk_from, entrance) {
                self.note_trip(id, &[(crate::insight::AIR, 0.0)], 0, purpose, target, fresh);
                self.log_leg(id, log_from, target, purpose, crate::travel_log::MODE_AIR, fresh);
                self.start_air_trip(id, plan, target, purpose, fresh);
                return;
            }
        }
        let q = TripQuery { from: walk_from, to: entrance, target, purpose, allow_transit: allow_bus, allow_car, allow_bike: !seated };
        let ranked = self.choose_mode(id, &q);
        // [taxis] A trip that would have gone by cab had one been in reach
        // is the demand a taxi company opens on.
        if fresh {
            self.note_taxi_demand(id, &q, &ranked);
        }
        let dist = walk_from.distance(entrance);
        // [insight] The ranking, for the citizen's history.
        let modes: Vec<(u8, f32)> = ranked.iter().map(|o| (o.mode.index() as u8, o.minutes)).collect();
        for (k, opt) in ranked.into_iter().enumerate() {
            match opt.plan {
                ModePlan::Car { car, at } => {
                    if at.distance(walk_from) > 25.0 {
                        self.walk_to_car(id, car, at, target, purpose, walk_from);
                    } else if !self.start_drive(id, car, target, purpose) {
                        continue;
                    }
                    self.note_trip(id, &modes, k, purpose, target, fresh);
                    self.log_leg(id, log_from, target, purpose, crate::travel_log::mode_bit(opt.mode), fresh);
                    if fresh {
                        self.begin_trip(id, opt.mode, dist);
                        // [passengers] Coworkers and family setting off
                        // from here the same way come along.
                        self.pick_up(id, origin, target, purpose);
                    }
                    return;
                }
                // [passengers] A seat in someone else's car.
                ModePlan::Lift { driver } => {
                    if !self.join_ride(id, driver, target, purpose, crate::rides::RideKind::Lift, false) {
                        continue;
                    }
                }
                ModePlan::Train { board, alight, stop } => self.walk_to_transit(id, stop, TransitLeg::Rail { board, alight }, target, purpose, fresh),
                ModePlan::Bus { route, board, alight, stop, bike: false } => self.walk_to_transit(id, stop, TransitLeg::Bus { route, board, alight }, target, purpose, fresh),
                ModePlan::Bus { route, board, alight, stop, bike: true } => self.ride_to_bus_stop(id, stop, TransitLeg::Bus { route, board, alight }, target, purpose, fresh),
                ModePlan::Subway { line, board, alight, stop } => self.walk_to_transit(id, stop, TransitLeg::Subway { line, board, alight }, target, purpose, fresh),
                ModePlan::Taxi { segment, at, wait_at } => self.hail_taxi(id, segment, at, wait_at, target, purpose, fresh),
                // [ferry-islands] To the pier, to wait for the boat.
                ModePlan::Ferry { board, alight, stop } => self.set_off_for_pier(id, walk_from, board, alight, stop, target, purpose, fresh),
                ModePlan::Walk | ModePlan::Bike => {
                    let mode = if matches!(opt.plan, ModePlan::Bike) { TravelMode::Bike } else { TravelMode::Walk };
                    if !self.walk_or_ride(id, target, purpose, mode, from_node, to_node, walk_from, entrance, fresh) {
                        continue;
                    }
                }
            }
            self.note_trip(id, &modes, k, purpose, target, fresh);
            let bits = if matches!(opt.plan, ModePlan::Lift { .. }) { crate::travel_log::MODE_LIFT | crate::travel_log::mode_bit(opt.mode) } else { crate::travel_log::mode_bit(opt.mode) };
            self.log_leg(id, log_from, target, purpose, bits, fresh);
            if fresh {
                self.begin_trip(id, opt.mode, dist);
            }
            return;
        }
        // Nothing gets there.
        if let Some(exit) = self.leave_building(id) {
            self.citizens[id].pos = exit;
        }
        // [pt-walk] Say why (a place cut off from the pedestrian grid, or
        // the walker), log it for the "no route found" alert, and show them
        // as stuck rather than merely loitering.
        let why = self.no_route_reason(walk_from, target);
        // [ferry-islands] Across the water with the boats stopped (or
        // stopping before they could get back): they stay put, no alarm.
        if why == super::super::islands::NO_FERRY {
            let c = &mut self.citizens[id];
            c.thought = "The last boat's too soon. Staying this side of the water tonight.".to_string();
            c.activity = Activity::Idle;
            c.next_decision_tick = self.clock.tick + 20;
            return;
        }
        let thought = match why.as_str() {
            "the grid doesn't connect" => "No way through. The grid doesn't connect.".to_string(),
            w => format!("No way through: {w}."),
        };
        self.note_failed_trip(id, walk_from, target, &why);
        let c = &mut self.citizens[id];
        c.thought = thought.clone();
        c.activity = Activity::Idle;
        let tick = self.clock.tick;
        self.concerns.said(tick, id, &self.citizens[id], crate::concerns::Concern::Commute, 0.5, &thought);
    }

    /// On foot or by bike along the streets (walkers through the blocks).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn walk_or_ride(&mut self, id: CitizenId, target: BuildingId, purpose: Purpose, mode: TravelMode, from_node: NodeId, to_node: NodeId, walk_from: Vec2, entrance: Vec2, fresh: bool) -> bool {
        crate::prof_scope!("walk_or_ride");
        // A walker's way through the blocks first: the street path (an A*)
        // is only searched when there is none, or for a bike.
        let walk = if mode == TravelMode::Walk { self.walk_route_for(id, walk_from, entrance, purpose) } else { None };
        let mut streets = Vec::new();
        if walk.is_none() {
            let Some(nodes) = self.roads.foot_path(from_node, to_node) else { return false };
            streets = nodes;
        }
        let exit_pos = self.leave_building(id);
        let mut path = match walk {
            Some((pts, walk_nodes)) => {
                if fresh {
                    self.note_footfall(id, &walk_nodes);
                } else {
                    // [pt-transit] The walk on from a stop or station passes
                    // shops too (it used to count for nothing).
                    self.add_footfall(id, &walk_nodes);
                }
                pts
            }
            None => self.route_path(&streets, mode),
        };
        path.push(entrance);
        if mode == TravelMode::Bike {
            self.bike_over_footbridges(&mut path);
        }
        let c = &mut self.citizens[id];
        if let Some(p) = exit_pos {
            c.pos = p;
            c.prev_pos = p;
        }
        c.mode = mode;
        c.activity = Activity::Traveling { to: Some(target), purpose, path, idx: 0 };
        true
    }

    /// [carlife] Every way this citizen could make this trip, with what
    /// each would cost them (`ModeOption`). Extend here for a new mode (air
    /// taxis, say): push an option with its own `ModePlan`.
    pub fn mode_options(&self, id: CitizenId, q: &TripQuery) -> Vec<ModeOption> {
        let Some(c) = self.citizens.get(id) else { return Vec::new() };
        let dist = q.from.distance(q.to);
        let secs = self.config.traffic_seconds_per_tick;
        let walk = self.config.walk_speed.max(0.1);
        let bike = self.config.bike_speed.max(0.1);
        let [sociable, ambition, hedonism, caution, curiosity] = crate::vehicle::traits_of(c.seed);
        let frail = if matches!(c.stage(), LifeStage::Child | LifeStage::Retiree) { 1.5 } else { 1.0 };
        let time_w = 0.7 + 0.6 * ambition;
        let money_w = self.money_weight(id);
        let mut out = Vec::new();
        let mut push = |mode: TripMode, minutes: f32, money: f32, comfort: f32, plan: ModePlan| {
            let pain = c.commute.pain_of(mode);
            let cost = minutes * time_w + money * money_w + comfort + PAIN_WEIGHT * pain;
            out.push(ModeOption { mode, minutes, money, comfort, pain, cost, plan });
        };
        // On foot: always possible, tiring when long.
        let minutes = dist * 1.25 / walk;
        push(TripMode::Walk, minutes, 0.0, (minutes - 15.0).max(0.0) * 0.35 * frail * (1.2 - 0.5 * curiosity), ModePlan::Walk);
        // A bike, for owners: quick, a little sweaty, risky to the cautious.
        let park_ride = matches!(q.purpose, Purpose::Visit(LandUse::Park));
        if c.owns_bike && q.allow_bike && (park_ride || dist >= self.config.bike_threshold) {
            let minutes = dist * 1.2 / bike + 2.0;
            // Riding to the park is the outing itself.
            let comfort = if park_ride { -30.0 } else { minutes * 0.25 * (0.5 + caution) * frail + 4.0 };
            push(TripMode::Bike, minutes, 0.0, comfort, ModePlan::Bike);
        }
        // The bus and the train, for longer trips: the walk to the stop,
        // the wait, the ride, the fare; the unsociable mind the crowd.
        let crowd = 6.0 - 6.0 * sociable;
        // Only while the buses run: at night a rider would stand at the
        // stop until morning (or give up hours later). [r4-timetable] By
        // each line's timetable, when the rider gets to the stop.
        let now = self.clock.minute_of_day() as u32;
        let arrive = |metres: f32| now + (metres * 1.25 / walk).ceil() as u32;
        if q.allow_transit && self.economy.transit.buses > 0 {
            // Buses out when they get there, and still out a headway later
            // (at the end of the evening the buses go home from their next
            // stop, so a rider just before the end would wait in vain).
            let runs = |r: &crate::transit::BusRoute, d: f32| {
                let n = r.buses_at(arrive(d));
                n > 0 && r.buses_at(arrive(d) + (r.headway_with(secs, n) as u32).min(transit::MAX_EXPECTED_WAIT as u32 * 2)) > 0
            };
            if let Some((route, board, alight)) = self.transit.plan_trip_when(q.from, q.to, &runs) {
                let r = &self.transit.routes[route];
                let (s0, s1) = (r.stops[board].pos, r.stops[alight].pos);
                // [pt-transit] The wait is half the line's headway (more
                // buses, shorter waits), not a flat twelve minutes.
                // [r4-timetable] With the buses out when the rider gets there.
                let wait = (r.headway_with(secs, r.buses_at(arrive(q.from.distance(s0)))) as f32 * 0.5).clamp(transit::MIN_EXPECTED_WAIT, transit::MAX_EXPECTED_WAIT);
                let minutes = (q.from.distance(s0) + s1.distance(q.to)) / walk + wait + r.ride_length(board, alight) / (7.0 * secs);
                push(TripMode::Bus, minutes, r.fare, crowd, ModePlan::Bus { route, board, alight, stop: s0, bike: false });
                // [bike-racks] A cyclist facing a long ride may put the bike
                // on the bus's front rack for the long middle of it: quick
                // rides to and from the stops, no slog in between. A rack
                // may be full (another bus, then), so a little worse than a
                // seat alone.
                if c.owns_bike && q.allow_bike && dist >= self.config.bike_threshold {
                    let ends = q.from.distance(s0) + s1.distance(q.to);
                    let minutes = ends * 1.2 / bike + wait + 1.0 + r.ride_length(board, alight) / (7.0 * secs);
                    let comfort = crowd + 3.0 + (ends * 1.2 / bike) * 0.25 * (0.5 + caution) * frail;
                    push(TripMode::Bus, minutes, r.fare, comfort, ModePlan::Bus { route, board, alight, stop: s0, bike: true });
                }
            } else if let Some((ra, board, x, rb, y, alight)) = self.transit.plan_transfer_when(q.from, q.to, &runs) {
                // [pt-transit] Two lines with a change between: the first
                // leg is booked; at its end the rider decides again and the
                // second line is the direct ride from there.
                let (a, b) = (&self.transit.routes[ra], &self.transit.routes[rb]);
                let wait = |r: &crate::transit::BusRoute| (r.headway_with(secs, r.buses_at(now).max(1)) as f32 * 0.5).clamp(transit::MIN_EXPECTED_WAIT, transit::MAX_EXPECTED_WAIT);
                let walking = q.from.distance(a.stops[board].pos) + a.stops[x].pos.distance(b.stops[y].pos) + b.stops[alight].pos.distance(q.to);
                let riding = (a.ride_length(board, x) + b.ride_length(y, alight)) / (7.0 * secs);
                let minutes = walking / walk + wait(a) + wait(b) + transit::TRANSFER_PENALTY + riding;
                push(TripMode::Bus, minutes, a.fare + b.fare, crowd, ModePlan::Bus { route: ra, board, alight: x, stop: a.stops[board].pos, bike: false });
            }
        }
        // [r4-timetable] Only if a train still runs (or has started) when
        // the rider reaches the platform.
        if q.allow_transit && (Rail::in_service(now) || self.rail.timetable.is_some()) {
            if let Some((board, alight, stop)) = self.plan_rail_trip(q.from, q.to).filter(|(_, _, stop)| self.rail.headway_at(arrive(q.from.distance(*stop))).is_some()) {
                let end = self.building_entrance(alight).unwrap_or(q.to);
                let wait = match self.rail.stats.next_passenger_in {
                    0 => 30.0,
                    n => (n as f32).min(120.0),
                };
                let minutes = (q.from.distance(stop) + end.distance(q.to)) / walk + wait + dist / (15.0 * secs);
                push(TripMode::Train, minutes, rail::RAIL_FARE, crowd, ModePlan::Train { board, alight, stop });
            }
        }
        // [subway] The metro: the walk to an entrance, the stairs, half a
        // headway on the platform, the ride. Out of the weather and the
        // traffic, so a little kinder than the bus to the crowd-shy.
        // [r4-timetable] Only lines still running when the rider gets to
        // the platform, at the headway they run then.
        if q.allow_transit {
            if let Some(p) = self.plan_subway_trip_now(q.from, q.to) {
                let at = arrive(q.from.distance(p.entrance));
                let headway = self.subway.lines.get(p.line).map_or(crate::subway::DEFAULT_HEADWAY, |l| l.headway_at(at).unwrap_or(l.headway)) as f32;
                let minutes = (q.from.distance(p.entrance) + p.exit.distance(q.to)) / walk + 2.0 + headway * 0.5 + p.minutes_riding;
                push(TripMode::Subway, minutes, crate::subway::FARE, crowd * 0.8, ModePlan::Subway { line: p.line, board: p.board, alight: p.alight, stop: p.entrance });
            }
        }
        // The car, when it is parked nearby and the trip is worth it: the
        // walk to it, the drive, finding a space (as remembered), fuel and
        // the fee at the far end. A car is a comfortable seat out of the
        // weather; hedonists like driving most.
        // [driving] A drunk leaves the car, unless rebellious enough to drive
        // anyway (`citizens::wants_to_drive_drunk`), and then takes it even
        // for a short hop.
        let drunk_drive = crate::citizens::wants_to_drive_drunk(c);
        let too_drunk = c.drunk >= crate::citizens::DRUNK && !drunk_drive;
        if q.allow_car && !too_drunk && (dist >= self.config.drive_threshold || drunk_drive) && self.building_lot(q.target).is_some_and(|l| l.car_segment.is_some()) {
            if let Some((car, at)) = self.car_ready(id, q.from) {
                let own_space = self.buildings.get(q.target).is_some_and(|b| !b.is_parking() && b.parking_free() > 0);
                let fee = if own_space {
                    0.0
                } else {
                    self.buildings.values().filter(|b| b.is_parking() && b.parking_free() > 0).filter_map(|b| self.lots.get(b.lot).map(|l| (l.center.distance(q.to), b.parking_price))).filter(|(d, _)| *d < 180.0).map(|(_, p)| p).fold(f32::INFINITY, f32::min)
                };
                let fee = if fee.is_finite() { fee * 0.5 } else { 0.0 };
                let search = if own_space { 0.0 } else { 2.0 + c.commute.parking_pain * 10.0 };
                let minutes = at.distance(q.from) / walk + dist * 1.3 / (9.0 * secs) + 3.0 + search;
                // [taxis] With a cab to be had, fewer drunks risk the wheel.
                let bravado = if !drunk_drive {
                    0.0
                } else if self.taxis_in_service() {
                    -15.0
                } else {
                    -40.0
                };
                // [tolls] The tolls a car trip has cost lately.
                push(TripMode::Car, minutes, dist / 1000.0 * 0.5 + fee + c.commute.toll_avg, -4.0 - 8.0 * hedonism + bravado, ModePlan::Car { car, at });
            }
        }
        // [passengers] A lift: family or a coworker setting off by car from
        // right here to near where this trip goes has a seat. Free, a
        // comfortable seat, and company the sociable enjoy; the drive's
        // jams and the walk from wherever they park are the same.
        if q.allow_car && dist >= self.config.drive_threshold * 0.5 {
            if let Some((driver, dest)) = self.lift_on_offer(id, q) {
                let minutes = dist * 1.3 / (9.0 * secs) + 3.0 + dest.distance(q.to) / walk;
                push(TripMode::Car, minutes, 0.0, -6.0 - 6.0 * sociable, ModePlan::Lift { driver });
            }
        }
        // [taxis] A cab, when a company has one on duty within reach.
        if q.allow_transit {
            if let Some(o) = self.taxi_option(id, q, false) {
                out.push(o);
            }
        }
        // [ferry-islands] Across the water to (or from) an island: the
        // ferry, and no walking, riding or cab there (no way joins them).
        if q.allow_transit {
            if let Some(o) = self.ferry_option(id, q, &arrive) {
                out.retain(|o| !matches!(o.mode, TripMode::Walk | TripMode::Bike | TripMode::Taxi) && !matches!(o.plan, ModePlan::Lift { .. }));
                out.push(o);
            }
        }
        out
    }

    /// [carlife] The options ranked best first, with a little day-to-day
    /// whim so habits are not clockwork.
    pub fn choose_mode(&mut self, id: CitizenId, q: &TripQuery) -> Vec<ModeOption> {
        crate::prof_scope!("choose_mode");
        let mut options = self.mode_options(id, q);
        for o in &mut options {
            o.cost *= self.rng.random_range(0.85..1.15f32);
        }
        // [land] A billionaire is driven, or walks a few steps with the
        // guards; never the bus, the train or a bike.
        if self.estate.is_elite(id) {
            options.retain(|o| matches!(o.mode, crate::citizens::TripMode::Car | crate::citizens::TripMode::Walk | crate::citizens::TripMode::Taxi));
            for o in &mut options {
                if o.mode == crate::citizens::TripMode::Car {
                    o.cost *= 0.2;
                }
            }
        }
        options.sort_by(|a, b| a.cost.total_cmp(&b.cost));
        options
    }

    /// Remember which lots a journey on foot will pass; they are counted
    /// when the walker gets there.
    pub(in crate::world) fn note_footfall(&mut self, id: CitizenId, walk_nodes: &[u32]) {
        crate::prof_scope!("note_footfall");
        let lots = self.walk.lots_passed(walk_nodes);
        if let Some(c) = self.citizens.get_mut(id) {
            c.pending_footfall = lots;
        }
    }

    /// [pt-transit] A later walk of the same journey (after a ride): its
    /// lots join the ones already noted, each counted once.
    pub(in crate::world) fn add_footfall(&mut self, id: CitizenId, walk_nodes: &[u32]) {
        let lots = self.walk.lots_passed(walk_nodes);
        if let Some(c) = self.citizens.get_mut(id) {
            for l in lots {
                if !c.pending_footfall.contains(&l) {
                    c.pending_footfall.push(l);
                }
            }
        }
    }

    /// Everyone on foot who passes a lot is a potential customer: credit
    /// the lots of the journey just completed, once.
    pub(in crate::world) fn settle_footfall(&mut self, id: CitizenId) {
        crate::prof_scope!("settle_footfall");
        let Some(c) = self.citizens.get_mut(id) else { return };
        let lots = std::mem::take(&mut c.pending_footfall);
        if lots.is_empty() {
            return;
        }
        self.economy.walk_trips_today += 1;
        for lid in lots {
            if let Some(l) = self.lots.get_mut(lid) {
                l.footfall_today += 1;
                l.footfall_hour += 1;
            }
        }
    }

    /// Route along the right-hand side of each leg: walkers in the middle
    /// of the pavement, bikes on the road shoulder; both down the middle of
    /// a path (walkers keep right, bikes keep left).
    pub(in crate::world) fn route_path(&self, nodes: &[NodeId], mode: TravelMode) -> Vec<Vec2> {
        if nodes.len() < 2 {
            return nodes.iter().map(|&n| self.roads.node(n).pos).collect();
        }
        // Each piece of street as a line offset to its kerb side, then
        // joined at the corner where one piece's line meets the next. Ending
        // one piece level with the junction's centre and starting the next
        // from there sent a turning rider (or walker) through the middle of
        // the road: on a right turn, back across the lanes they came in on.
        let mut pieces: Vec<(Vec2, Vec2)> = Vec::with_capacity(nodes.len());
        for w in nodes.windows(2) {
            let a = self.roads.node(w[0]).pos;
            let b = self.roads.node(w[1]).pos;
            let d = (b - a).normalize_or_zero();
            let right = Vec2::new(-d.y, d.x);
            let seg = self.roads.neighbors(w[0]).find(|(n, _)| *n == w[1]).map(|(_, sid)| self.roads.segment(sid));
            let offset = match (seg.map(|s| s.kind).unwrap_or(RoadKind::Street), mode) {
                (RoadKind::Street, TravelMode::Walk) => seg.map_or(5.0, |s| s.half_width()) + crate::zoning::SIDEWALK * 0.55,
                (RoadKind::Street, TravelMode::Bike) => seg.map_or(5.0, |s| s.half_width()) - crate::roads::SHOULDER * 0.5,
                (RoadKind::Path, TravelMode::Walk) => 0.6,
                (RoadKind::Path, TravelMode::Bike) => -0.5,
                (RoadKind::Rail, _) => 0.0,
            };
            let off = right * offset;
            pieces.push((a + off, b + off));
        }
        let mut out = Vec::with_capacity(pieces.len() + 1);
        out.push(pieces[0].0);
        for (i, pair) in pieces.windows(2).enumerate() {
            let ((p0, p1), (q0, q1)) = (pair[0], pair[1]);
            let (d, e) = ((p1 - p0).normalize_or_zero(), (q1 - q0).normalize_or_zero());
            let node = self.roads.node(nodes[i + 1]).pos;
            let reach = p1.distance(node).max(q0.distance(node)) * 3.0 + 2.0;
            let cross = d.perp_dot(e);
            // The corner, unless the pieces run (nearly) straight on or back
            // the way they came, or it lies implausibly far off the junction.
            let corner = (cross.abs() > 0.05).then(|| p1 + d * (q0 - p1).perp_dot(e) / cross).filter(|c| c.distance(node) < reach);
            match corner {
                Some(c) => out.push(c),
                None => {
                    out.push(p1);
                    out.push(q0);
                }
            }
        }
        out.push(pieces[pieces.len() - 1].1);
        out
    }
}
