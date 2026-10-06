//! Buses: depots, routes and stops, dispatch and the bus loop.

use super::*;

impl World {
    /// Place a bus depot on a lot with street access. Whatever grew there
    /// is bought out and pulled down, as for every civic structure.
    pub fn build_depot(&mut self, lot_id: LotId) -> Result<BuildingId, String> {
        let lot = self.lots.get(lot_id).ok_or("no such lot")?;
        if let Some(b) = lot.building.and_then(|b| self.buildings.get(b)) {
            if Self::is_civic_structure(b) {
                return Err(format!("{} stands there; bulldoze it first", b.name));
            }
        }
        if lot.landlocked || !lot.car_access() {
            return Err("a depot needs a street".into());
        }
        if self.under_facility(lot.center) {
            return Err("that ground belongs to a station or yard".into());
        }
        if matches!(lot.land_use, LandUse::Residential | LandUse::Park) {
            return Err("a depot cannot go on homes or parks".into());
        }
        if lot.area() < transit::DEPOT_MIN_AREA {
            return Err(format!("too small: a depot needs {:.0} m²", transit::DEPOT_MIN_AREA));
        }
        if self.economy.treasury < transit::DEPOT_COST {
            return Err(format!("the treasury has {:.0}; a depot costs {:.0}", self.economy.treasury, transit::DEPOT_COST));
        }
        self.take_over_lot(lot_id, "bus depot")?;
        self.economy.debit(Flow::Construction, transit::DEPOT_COST);
        let name = format!("Depot {}", self.rng.random_range(1..20));
        Ok(self.place_special(lot_id, name, transit::DEPOT_JOBS, 0, |b| b.depot = true))
    }

    pub fn add_bus_route(&mut self) -> RouteId {
        self.transit.new_route()
    }

    /// Append a stop to a line, snapped to the nearest street.
    pub fn add_bus_stop(&mut self, route: RouteId, p: Vec2) -> Result<usize, String> {
        let stop = BusStop::snap(&self.roads, p).ok_or("click next to a street")?;
        let r = self.transit.routes.get_mut(route).ok_or("no such line")?;
        if r.stops.iter().any(|s| s.pos.distance(stop.pos) < 3.0) {
            return Err("there is a stop there already".into());
        }
        r.stops.push(stop);
        let idx = r.stops.len() - 1;
        self.transit.refresh_legs(&self.traffic.net);
        Ok(idx)
    }

    pub fn move_bus_stop(&mut self, route: RouteId, idx: usize, p: Vec2) -> bool {
        let Some(stop) = BusStop::snap(&self.roads, p) else { return false };
        let Some(r) = self.transit.routes.get_mut(route) else { return false };
        if idx >= r.stops.len() {
            return false;
        }
        r.stops[idx] = stop;
        self.transit.refresh_legs(&self.traffic.net);
        true
    }

    /// Remove a stop; a line with no stops left is removed too.
    pub fn remove_bus_stop(&mut self, route: RouteId, idx: usize) {
        let Some(r) = self.transit.routes.get_mut(route) else { return };
        if idx < r.stops.len() {
            r.stops.remove(idx);
        }
        let empty = r.stops.is_empty();
        // Buses heading past the removed stop re-aim at what is now there.
        for car in self.traffic.cars.values_mut() {
            if let Some(bus) = car.bus.as_mut() {
                if bus.route == route && bus.next_stop != usize::MAX && bus.next_stop >= idx {
                    bus.next_stop = bus.next_stop.saturating_sub(1);
                }
            }
        }
        // Riders' trips shift with the stops; a trip through the removed
        // stop is cancelled and the rider decides again.
        let tick = self.clock.tick;
        for c in self.citizens.values_mut() {
            let Some(t) = c.transit.as_mut() else { continue };
            let TransitLeg::Bus { route: r, board, alight } = &mut t.leg else { continue };
            if *r != route {
                continue;
            }
            if *board == idx || *alight == idx {
                c.transit = None;
                if matches!(c.activity, Activity::Waiting { .. }) {
                    c.activity = Activity::Idle;
                    c.next_decision_tick = tick + 1;
                }
                continue;
            }
            if *board > idx {
                *board -= 1;
            }
            if *alight > idx {
                *alight -= 1;
            }
        }
        if empty {
            self.remove_bus_route(route);
        } else {
            self.transit.refresh_legs(&self.traffic.net);
        }
    }

    pub fn remove_bus_route(&mut self, route: RouteId) {
        self.transit.routes.remove(route);
        let buses: Vec<CarId> = self.traffic.cars.iter().filter(|(_, c)| c.bus.as_ref().is_some_and(|b| b.route == route)).map(|(id, _)| id).collect();
        for id in buses {
            if let Some(car) = self.traffic.remove_car(id) {
                self.strand_car(car);
            }
        }
        let riders: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| c.transit.is_some_and(|t| matches!(t.leg, TransitLeg::Bus { route: r, .. } if r == route))).map(|(id, _)| id).collect();
        for id in riders {
            self.citizens[id].transit = None;
            if matches!(self.citizens[id].activity, Activity::Waiting { .. }) {
                self.citizens[id].activity = Activity::Idle;
                self.citizens[id].next_decision_tick = self.clock.tick + 1;
            }
        }
        self.transit.refresh_legs(&self.traffic.net);
    }

    pub fn set_route_buses(&mut self, route: RouteId, n: u8) {
        if let Some(r) = self.transit.routes.get_mut(route) {
            r.buses = n.min(transit::DEPOT_CAPACITY as u8);
            self.transit.version += 1;
        }
    }

    /// Dispatch buses from depots during service, call them home after,
    /// count the system, and let riders who waited too long walk.
    pub(super) fn step_transit(&mut self) {
        let tick = self.clock.tick;
        let minute = self.clock.minute_of_day();
        let in_service = Transit::in_service(minute as u32);
        // Buses out per line, and per depot.
        let mut running: HashMap<RouteId, u32> = HashMap::new();
        let mut at_depot: HashMap<BuildingId, u32> = HashMap::new();
        for car in self.traffic.cars.values() {
            if let Some(bus) = &car.bus {
                if !bus.to_depot {
                    *running.entry(bus.route).or_default() += 1;
                }
                *at_depot.entry(bus.depot).or_default() += 1;
            }
        }
        if in_service && tick.saturating_sub(self.transit.last_dispatch_tick) >= transit::DISPATCH_GAP {
            let want: Option<RouteId> = self.transit.routes.iter().filter(|(id, r)| r.runnable() && running.get(id).copied().unwrap_or(0) < r.buses as u32).map(|(id, _)| id).next();
            if let Some(route) = want {
                // The depot with room nearest the first stop.
                let first = self.transit.routes[route].stops[0];
                let depots: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.depot && !b.is_derelict()).map(|(id, _)| id).collect();
                let depot = depots
                    .iter()
                    .copied()
                    .filter(|d| at_depot.get(d).copied().unwrap_or(0) < transit::DEPOT_CAPACITY)
                    .filter_map(|d| self.building_lot(d).map(|l| (d, l.car_point.distance(first.pos))))
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(d, _)| d);
                if let Some(depot) = depot {
                    if let Some((seg, pt)) = self.building_lot(depot).and_then(|l| l.car_segment.map(|s| (s, l.car_point))) {
                        if let Some((lanes, s0, s1)) = self.traffic.net.find_route(seg, pt, first.segment, first.pos) {
                            let color = self.transit.routes[route].color;
                            let id = self.traffic.spawn_car(CitizenId::default(), None, lanes, s0, s1, color);
                            self.traffic.cars[id].model = crate::vehicle::VehicleModel::bus(color, &mut crate::vehicle::model_rng(tick, slotmap::Key::data(&id).as_ffi()));
                            self.traffic.cars[id].bus = Some(Bus { route, next_stop: 0, passengers: Vec::new(), to_depot: false, depot });
                            self.transit.last_dispatch_tick = tick;
                        }
                    }
                }
            }
        }
        if !in_service {
            for car in self.traffic.cars.values_mut() {
                if let Some(bus) = car.bus.as_mut() {
                    bus.to_depot = true;
                }
            }
        }
        // Riders who gave up: checked every ten ticks, since the limit is
        // hours and the scan is everyone.
        if tick % 10 == 0 {
            let tired: Vec<CitizenId> = self
                .citizens
                .iter()
                .filter(|(_, c)| matches!(c.activity, Activity::Waiting { .. }) && c.transit.is_some_and(|t| t.since > 0 && tick.saturating_sub(t.since) > if matches!(t.leg, TransitLeg::Rail { .. }) { transit::WAIT_GIVE_UP * 2 } else { transit::WAIT_GIVE_UP }))
                .map(|(id, _)| id)
                .collect();
            for id in tired {
                let Some(trip) = self.citizens[id].transit.take() else { continue };
                self.citizens[id].thought = "No bus is coming. Walking it.".to_string();
                // [carlife] Remembered when choosing how to travel next time.
                self.transit_failed(id);
                self.travel_to_impl(id, Some(trip.target), trip.purpose, false, false);
            }
        }
    }

    /// A bus reached the end of a leg: a stop, or the depot.
    pub(super) fn bus_arrived(&mut self, car_id: CarId) {
        let tick = self.clock.tick;
        let (bus, pos) = {
            let car = &self.traffic.cars[car_id];
            (car.bus.clone().unwrap(), car.pos)
        };
        let gone = |w: &mut World, car_id: CarId| {
            if let Some(car) = w.traffic.remove_car(car_id) {
                w.strand_car(car);
            }
        };
        if bus.next_stop == usize::MAX {
            // Home at the depot.
            gone(self, car_id);
            return;
        }
        let stop_idx = bus.next_stop;
        let (here, fare, n) = match self.transit.routes.get(bus.route) {
            Some(r) if stop_idx < r.stops.len() => (r.stops[stop_idx], r.fare, r.stops.len()),
            _ => {
                gone(self, car_id);
                return;
            }
        };
        // Off.
        let mut passengers = bus.passengers.clone();
        let alighting: Vec<CitizenId> = passengers.iter().copied().filter(|p| self.citizens.get(*p).and_then(|c| c.transit).is_none_or(|t| match t.leg { TransitLeg::Bus { route, alight, .. } => alight == stop_idx || route != bus.route, _ => true })).collect();
        passengers.retain(|p| !alighting.contains(p));
        for p in alighting {
            let Some(trip) = self.citizens.get_mut(p).and_then(|c| {
                c.pos = here.pos;
                c.prev_pos = here.pos;
                c.activity = Activity::Idle;
                c.transit.take()
            }) else {
                continue;
            };
            self.travel_to_impl(p, Some(trip.target), trip.purpose, false, false);
        }
        // On, up to the capacity.
        let _ = tick;
        let memory = format!("Caught the {} bus.", self.transit.routes[bus.route].name);
        let boarding = self.board_waiting(|leg| matches!(leg, TransitLeg::Bus { route, board, .. } if *route == bus.route && *board == stop_idx), transit::BUS_CAPACITY.saturating_sub(passengers.len()), fare, Flow::BusFares, &memory);
        for p in boarding {
            self.citizens[p].activity = Activity::Riding { bus: car_id };
            passengers.push(p);
            self.economy.transit.fares_today += fare;
            self.economy.transit.riders_today += 1;
            self.transit.routes[bus.route].riders_today += 1;
        }
        // On to the next stop, or home.
        let (next_idx, to_seg, to_pos) = if bus.to_depot {
            match self.building_lot(bus.depot).and_then(|l| l.car_segment.map(|s| (s, l.car_point))) {
                Some((s, p)) => (usize::MAX, s, p),
                None => {
                    gone(self, car_id);
                    return;
                }
            }
        } else {
            let k = (stop_idx + 1) % n;
            let s = self.transit.routes[bus.route].stops[k];
            (k, s.segment, s.pos)
        };
        match self.traffic.net.find_route(here.segment, pos, to_seg, to_pos) {
            Some((lanes, s0, s1)) => {
                self.traffic.reroute(car_id, lanes, s0, s1);
                let car = &mut self.traffic.cars[car_id];
                car.hold = transit::DWELL_SECONDS;
                if let Some(b) = car.bus.as_mut() {
                    b.next_stop = next_idx;
                    b.passengers = passengers;
                }
            }
            None => gone(self, car_id),
        }
    }
}
