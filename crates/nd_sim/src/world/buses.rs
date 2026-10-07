//! Buses: depots, routes and stops, dispatch and the bus loop.

use super::*;

/// [services] How near its depot's driveway a bus going home must stand to
/// be in (metres): farther, it drives there first.
const DEPOT_REACH: f32 = 25.0;

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
        self.economy.debit_item(Flow::Construction, transit::DEPOT_COST, Item::Named("bus depot".into()));
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
        // The old last stop now drives to the new one, not round to the first.
        if idx > 0 {
            r.stops[idx - 1].stats.forget_leg();
        }
        self.transit.refresh_legs(&self.traffic.net);
        Ok(idx)
    }

    pub fn move_bus_stop(&mut self, route: RouteId, idx: usize, p: Vec2) -> bool {
        let Some(stop) = BusStop::snap(&self.roads, p) else { return false };
        let Some(r) = self.transit.routes.get_mut(route) else { return false };
        if idx >= r.stops.len() {
            return false;
        }
        // The stop keeps its ridership; the legs into and out of it are new.
        let stats = r.stops[idx].stats;
        r.stops[idx] = stop;
        r.stops[idx].stats = stats;
        let n = r.stops.len();
        r.stops[idx].stats.forget_leg();
        r.stops[(idx + n - 1) % n].stats.forget_leg();
        self.transit.refresh_legs(&self.traffic.net);
        true
    }

    /// Rename a bus line. Blank names are refused.
    pub fn rename_bus_route(&mut self, route: RouteId, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("a line needs a name".into());
        }
        let r = self.transit.routes.get_mut(route).ok_or("no such line")?;
        r.name = name.chars().take(40).collect();
        self.transit.version += 1;
        Ok(())
    }

    /// Recolour a bus line, buses on the road included, so the line and
    /// its fleet keep matching. `color` indexes the vehicle palette.
    pub fn recolour_bus_route(&mut self, route: RouteId, color: u8) -> Result<(), String> {
        let r = self.transit.routes.get_mut(route).ok_or("no such line")?;
        r.color = color % crate::vehicle::PAINTS;
        let c = r.color;
        for car in self.traffic.cars.values_mut() {
            if car.bus.as_ref().is_some_and(|b| b.route == route) {
                car.color = c;
                car.model.paint = c;
            }
        }
        self.transit.version += 1;
        Ok(())
    }

    /// Remove a stop; a line with no stops left is removed too.
    pub fn remove_bus_stop(&mut self, route: RouteId, idx: usize) {
        let Some(r) = self.transit.routes.get_mut(route) else { return };
        if idx < r.stops.len() {
            r.stops.remove(idx);
            // The stop before now drives straight on to the one after.
            let n = r.stops.len();
            if n > 0 {
                r.stops[(idx + n - 1) % n].stats.forget_leg();
            }
        }
        let empty = r.stops.is_empty();
        self.stop_removed(route, idx);
        if empty {
            self.remove_bus_route(route);
        } else {
            self.transit.refresh_legs(&self.traffic.net);
        }
    }

    /// Stop `idx` of a line has just been taken out of its list (by the
    /// player, or because its street went): buses and riders bound for
    /// later stops shift down with the list. A bus driving to the removed
    /// stop keeps its index, which now names the stop after it (wrapping
    /// to the first), and turns for it from the lane it is on: re-aiming it
    /// at the stop behind it sent it back there by teleport. A rider whose
    /// trip boards or alights at the removed stop decides again.
    pub(super) fn stop_removed(&mut self, route: RouteId, idx: usize) {
        let n = self.transit.routes.get(route).map_or(0, |r| r.stops.len());
        // Leg timings in flight refer to the old numbering: drop them.
        let on_line: Vec<CarId> = self.traffic.cars.iter().filter(|(_, c)| c.bus.as_ref().is_some_and(|b| b.route == route)).map(|(id, _)| id).collect();
        for id in on_line {
            self.transit.departures.remove(&id);
        }
        let mut turn: Vec<CarId> = Vec::new();
        for (id, car) in self.traffic.cars.iter_mut() {
            if let Some(bus) = car.bus.as_mut() {
                if bus.route != route || bus.next_stop == usize::MAX {
                    continue;
                }
                if bus.next_stop > idx {
                    bus.next_stop -= 1;
                } else if bus.next_stop == idx && n > 0 {
                    bus.next_stop = if idx >= n { 0 } else { idx };
                    turn.push(id);
                }
            }
        }
        for id in turn {
            let Some(k) = self.traffic.cars.get(id).and_then(|c| c.bus.as_ref()).map(|b| b.next_stop) else { continue };
            let stop = self.transit.routes[route].stops[k];
            if let Some((goal, s1, _)) = self.traffic.net.nearest_lane_on(stop.segment, stop.pos) {
                if let Some(lanes) = self.traffic.route_from_here(id, goal, s1) {
                    self.traffic.follow(id, lanes, s1);
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
    }

    pub fn remove_bus_route(&mut self, route: RouteId) {
        self.transit.routes.remove(route);
        // Its buses let their riders off where they are and go home.
        let buses: Vec<CarId> = self.traffic.cars.iter().filter(|(_, c)| c.bus.as_ref().is_some_and(|b| b.route == route)).map(|(id, _)| id).collect();
        for id in buses {
            self.transit.departures.remove(&id);
            self.bus_home(id);
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
                    // [industry] full depots overflow into truck and bus lots nearby
                    .filter(|d| at_depot.get(d).copied().unwrap_or(0) < transit::DEPOT_CAPACITY + self.depot_overflow(*d))
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

    /// [services] Riders get off where the bus stands and decide again.
    fn bus_set_down(&mut self, car_id: CarId) {
        let Some(car) = self.traffic.cars.get_mut(car_id) else { return };
        let pos = car.pos;
        let riders = car.bus.as_mut().map(|b| std::mem::take(&mut b.passengers)).unwrap_or_default();
        for p in riders {
            if let Some(c) = self.citizens.get_mut(p) {
                c.transit = None;
            }
            self.strand_driver(p, pos);
        }
    }

    /// [services] A bus with no line left to run (the line was removed, the
    /// next stop cannot be reached, the depot went): its riders get off,
    /// then it drives to its depot, else the nearest other one, else off
    /// the map. Buses used to vanish where they stood.
    pub(super) fn bus_home(&mut self, car_id: CarId) {
        self.bus_set_down(car_id);
        let Some(car) = self.traffic.cars.get(car_id) else { return };
        let pos = car.pos;
        let own = car.bus.as_ref().map(|b| b.depot).filter(|d| self.buildings.get(*d).is_some_and(|b| b.depot));
        let mut others: Vec<(f32, BuildingId)> = self.buildings.iter().filter(|(id, b)| b.depot && !b.is_derelict() && Some(*id) != own).filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (l.car_point.distance(pos), id))).collect();
        others.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let depots: Vec<BuildingId> = own.into_iter().chain(others.into_iter().map(|d| d.1)).collect();
        for d in depots {
            let Some((seg, pt)) = self.building_lot(d).and_then(|l| l.car_segment.map(|s| (s, l.car_point))) else { continue };
            let Some((goal, s_end, _)) = self.traffic.net.nearest_lane_on(seg, pt) else { continue };
            if self.traffic.redirect(car_id, goal, s_end) {
                if let Some(b) = self.traffic.cars[car_id].bus.as_mut() {
                    b.depot = d;
                    b.to_depot = true;
                    b.next_stop = usize::MAX;
                }
                return;
            }
        }
        // No depot it can reach: off the map, as a plain vehicle.
        self.traffic.cars[car_id].bus = None;
        self.retire_vehicle(car_id);
    }

    /// A bus reached the end of a leg: a stop, or the depot.
    pub(super) fn bus_arrived(&mut self, car_id: CarId) {
        let tick = self.clock.tick;
        let (bus, pos) = {
            let car = &self.traffic.cars[car_id];
            (car.bus.clone().unwrap(), car.pos)
        };
        let gone = |w: &mut World, car_id: CarId| {
            w.transit.departures.remove(&car_id);
            w.bus_home(car_id)
        };
        if bus.next_stop == usize::MAX {
            // Home at the depot (when that is where it stands: a bus
            // re-aimed at its depot on load or the depot gone meanwhile
            // finds one).
            let stop = self.building_lot(bus.depot).filter(|_| self.buildings.get(bus.depot).is_some_and(|b| b.depot)).and_then(|l| l.car_segment.map(|s| (s, l.car_point)));
            let home = stop.is_some_and(|(seg, p)| self.car_at_stop(car_id, seg, p, DEPOT_REACH));
            if home {
                if let Some(car) = self.traffic.remove_car(car_id) {
                    self.strand_car(car);
                }
            } else {
                self.bus_home(car_id);
            }
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
        // [lines] Count the stop's riders off, and time the leg that brought
        // the bus here if it came straight from the stop before.
        {
            let r = &mut self.transit.routes[bus.route];
            r.stops[stop_idx].stats.off_today += alighting.len() as u32;
            if let Some((prev, t0)) = self.transit.departures.get(&car_id).copied() {
                if prev < n && (prev + 1) % n == stop_idx && tick > t0 {
                    r.stops[prev].stats.time_leg((tick - t0) as f32);
                }
            }
            self.transit.departures.insert(car_id, (stop_idx, tick));
        }
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
        let memory = format!("Caught the {} bus.", self.transit.routes[bus.route].name);
        let boarding = self.board_waiting(|leg| matches!(leg, TransitLeg::Bus { route, board, .. } if *route == bus.route && *board == stop_idx), transit::BUS_CAPACITY.saturating_sub(passengers.len()), fare, Flow::BusFares, Item::Route(bus.route), &memory);
        for p in boarding {
            self.citizens[p].activity = Activity::Riding { bus: car_id };
            passengers.push(p);
            self.economy.transit.fares_today += fare;
            self.economy.transit.riders_today += 1;
            self.transit.routes[bus.route].riders_today += 1;
            self.transit.routes[bus.route].stops[stop_idx].stats.on_today += 1;
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
        // On from the lane the bus stands in; only if there is no way from
        // there is it set on a route from the stop's own street.
        let goal = self.traffic.net.nearest_lane_on(to_seg, to_pos);
        let from_here = goal.and_then(|(g, s1, _)| self.traffic.route_from_here(car_id, g, s1).map(|r| (r, s1)));
        let s_here = self.traffic.cars[car_id].s;
        match from_here.map(|(r, s1)| (r, s_here, s1)).or_else(|| self.traffic.net.find_route(here.segment, pos, to_seg, to_pos)) {
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
