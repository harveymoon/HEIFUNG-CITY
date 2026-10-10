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
        // [free-placement] Any zone, or none: only park land is kept.
        if !crate::services::Placeable::BusDepot.allowed_on(lot.land_use) {
            return Err("that is park land: unzone it before putting a depot there".into());
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

    /// Append a stop to a line, snapped to the nearest street (or onto
    /// another line's stop, when the click is on one: lines share stops).
    pub fn add_bus_stop(&mut self, route: RouteId, p: Vec2) -> Result<usize, String> {
        let n = self.transit.routes.get(route).ok_or("no such line")?.stops.len();
        self.insert_bus_stop(route, n, p)
    }

    /// [pt-transit] Put a stop into a line at `idx` (0 before the first,
    /// the number of stops after the last), so the bus calls there between
    /// stops `idx - 1` and `idx`. Snapped like `add_bus_stop`. Buses and
    /// riders bound for later stops shift up with the list.
    pub fn insert_bus_stop(&mut self, route: RouteId, idx: usize, p: Vec2) -> Result<usize, String> {
        let stop = self.transit.stop_for_click(&self.roads, p).ok_or("click next to a street")?;
        let r = self.transit.routes.get_mut(route).ok_or("no such line")?;
        if r.stops.iter().any(|s| s.pos.distance(stop.pos) < 3.0) {
            return Err("this line stops there already".into());
        }
        let idx = idx.min(r.stops.len());
        let was = r.stops.len();
        r.stops.insert(idx, stop);
        // The stop before now drives to the new one.
        let n = r.stops.len();
        if n > 1 {
            r.stops[(idx + n - 1) % n].stats.forget_leg();
        }
        if idx < was {
            self.stop_inserted(route, idx);
        }
        self.transit.refresh_legs(&self.traffic.net);
        Ok(idx)
    }

    /// [pt-transit] A stop went into a line's list at `idx`: buses and
    /// riders bound for it or later shift up with the list. A bus driving
    /// to the stop that is now after the new one keeps heading there (it
    /// does not turn back for the new stop).
    fn stop_inserted(&mut self, route: RouteId, idx: usize) {
        let on_line: Vec<CarId> = self.traffic.cars.iter().filter(|(_, c)| c.bus.as_ref().is_some_and(|b| b.route == route)).map(|(id, _)| id).collect();
        for id in on_line {
            self.transit.departures.remove(&id);
            if let Some(bus) = self.traffic.cars[id].bus.as_mut() {
                if bus.next_stop != usize::MAX && bus.next_stop >= idx {
                    bus.next_stop += 1;
                }
            }
        }
        for c in self.citizens.values_mut() {
            let Some(t) = c.transit.as_mut() else { continue };
            let TransitLeg::Bus { route: r, board, alight } = &mut t.leg else { continue };
            if *r != route {
                continue;
            }
            if *board >= idx {
                *board += 1;
            }
            if *alight >= idx {
                *alight += 1;
            }
        }
    }

    /// [pt-transit] Set a line's fare (0 to 20). Riders weigh it when they
    /// choose how to travel.
    pub fn set_route_fare(&mut self, route: RouteId, fare: f32) -> Result<(), String> {
        let r = self.transit.routes.get_mut(route).ok_or("no such line")?;
        if !fare.is_finite() {
            return Err("not a fare".into());
        }
        r.fare = fare.clamp(0.0, transit::MAX_FARE);
        self.transit.version += 1;
        Ok(())
    }

    /// [pt-transit] Everything keeping a line from running as drawn, or
    /// worth a warning: its route (`BusRoute::route_problems`), no buses
    /// set, no depot or none that reaches it, more buses wanted than the
    /// depots hold, all depots' buses out, night. Empty when it runs.
    pub fn line_problems(&self, route: RouteId) -> Vec<LineProblem> {
        let Some(r) = self.transit.routes.get(route) else { return Vec::new() };
        let mut out = r.route_problems();
        if r.buses == 0 {
            out.push(LineProblem::NoBuses);
        }
        let depots: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.working_depot()).map(|(id, _)| id).collect();
        if depots.is_empty() {
            out.push(LineProblem::NoDepot);
        } else if r.stops.len() >= 2 {
            let reach = match r.dispatch {
                Some(DispatchProblem::Unreachable) => false,
                Some(_) => true,
                // Not tried yet (or sent fine): look.
                None => self.traffic.cars.values().any(|c| c.bus.as_ref().is_some_and(|b| b.route == route)) || depots.iter().any(|&d| self.depot_reaches(d, r.stops[0])),
            };
            if !reach {
                out.push(LineProblem::DepotUnreachable);
            }
            let (wanted, capacity) = self.fleet_balance();
            if wanted > capacity {
                out.push(LineProblem::FleetShort { wanted, capacity });
            }
            if r.dispatch == Some(DispatchProblem::DepotsFull) {
                out.push(LineProblem::DepotsFull);
            }
        }
        if !Transit::in_service(self.clock.minute_of_day() as u32) {
            out.push(LineProblem::OutOfService);
        }
        out
    }

    /// [pt-transit] Buses wanted across the lines that can run, and how
    /// many the working depots hold (their own bays and the truck lots
    /// they overflow into).
    pub fn fleet_balance(&self) -> (u32, u32) {
        let wanted = self.transit.routes.values().filter(|r| r.runnable()).map(|r| r.buses as u32).sum();
        let capacity = self.buildings.iter().filter(|(_, b)| b.working_depot()).map(|(id, _)| transit::DEPOT_CAPACITY + self.depot_overflow(id)).sum();
        (wanted, capacity)
    }

    /// Can a bus leave depot `d` and drive to `stop`?
    fn depot_reaches(&self, d: BuildingId, stop: BusStop) -> bool {
        self.car_stop(d).is_some_and(|(seg, pt)| self.traffic.net.find_route(seg, pt, stop.segment, stop.pos).is_some())
    }

    /// [pt-transit] Send out the next bus a line is due, from the nearest
    /// depot with room that can reach its first stop. `running` and
    /// `at_depot` count the buses out (per line, per depot) and are kept
    /// up to date. Returns why none went, if none did.
    fn dispatch_bus(&mut self, route: RouteId, running: &mut HashMap<RouteId, u32>, at_depot: &mut HashMap<BuildingId, u32>) -> Option<DispatchProblem> {
        let tick = self.clock.tick;
        // [pt-transit] The buses join the loop spread round it: the k-th of
        // n out starts at the stop k/n of the way round, so a long line is
        // served end to end soon after the morning start, evenly spaced.
        let start = {
            let r = &self.transit.routes[route];
            let out = running.get(&route).copied().unwrap_or(0) as usize;
            (out * r.stops.len() / (r.buses.max(1) as usize)) % r.stops.len()
        };
        let first = self.transit.routes[route].stops[start];
        let mut depots: Vec<(f32, BuildingId)> = self.buildings.iter().filter(|(_, b)| b.working_depot()).filter_map(|(id, _)| self.building_lot(id).map(|l| (l.car_point.distance(first.pos), id))).collect();
        if depots.is_empty() {
            return Some(DispatchProblem::NoDepot);
        }
        depots.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut any_room = false;
        for (_, depot) in depots {
            // [industry] full depots overflow into truck and bus lots nearby
            if at_depot.get(&depot).copied().unwrap_or(0) >= transit::DEPOT_CAPACITY + self.depot_overflow(depot) {
                continue;
            }
            any_room = true;
            let Some((seg, pt)) = self.car_stop(depot) else { continue };
            let Some((lanes, s0, s1)) = self.traffic.net.find_route(seg, pt, first.segment, first.pos) else { continue };
            let color = self.transit.routes[route].color;
            let id = self.traffic.spawn_car(CitizenId::default(), None, lanes, s0, s1, color);
            self.traffic.cars[id].model = crate::vehicle::VehicleModel::bus(color, &mut crate::vehicle::model_rng(tick, slotmap::Key::data(&id).as_ffi()));
            self.traffic.cars[id].bus = Some(Bus { route, next_stop: start, passengers: Vec::new(), to_depot: false, depot });
            self.transit.last_dispatch_tick = tick;
            *running.entry(route).or_default() += 1;
            *at_depot.entry(depot).or_default() += 1;
            return None;
        }
        Some(if any_room { DispatchProblem::Unreachable } else { DispatchProblem::DepotsFull })
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
        // [pt-transit] Each line's buses for the bus-minutes it is charged.
        for (route, n) in &running {
            if let Some(r) = self.transit.routes.get_mut(*route) {
                r.bus_minutes_today = r.bus_minutes_today.saturating_add(*n);
            }
        }
        // [pt-transit] Every line short of buses is served, each on its own
        // headway: a line that cannot be served (no depot reaches it) no
        // longer holds up the lines after it, and a line's buses leave the
        // depot a headway apart instead of all at once (they bunched).
        if in_service && tick.saturating_sub(self.transit.last_dispatch_tick) >= transit::DISPATCH_GAP {
            let secs = self.config.traffic_seconds_per_tick;
            let due: Vec<RouteId> = self.transit.routes.iter().filter(|(id, r)| r.runnable() && running.get(id).copied().unwrap_or(0) < r.buses as u32 && tick >= r.next_release).map(|(id, _)| id).collect();
            for route in due {
                let problem = self.dispatch_bus(route, &mut running, &mut at_depot);
                let r = &mut self.transit.routes[route];
                r.dispatch = problem;
                r.next_release = tick + if problem.is_some() { transit::DISPATCH_RETRY } else { r.release_gap(secs) };
                // One bus a tick from all the depots at most, as before.
                if problem.is_none() {
                    break;
                }
            }
        }
        for (id, r) in self.transit.routes.iter_mut() {
            if r.dispatch.is_some() && running.get(&id).copied().unwrap_or(0) >= r.buses as u32 {
                r.dispatch = None;
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
                // [pt-transit] Up from the metro platform to the street first.
                if let TransitLeg::Subway { board, .. } = trip.leg {
                    if let Some(p) = self.building_entrance(board) {
                        let c = &mut self.citizens[id];
                        c.pos = p;
                        c.prev_pos = p;
                    }
                }
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
        let mut others: Vec<(f32, BuildingId)> = self.buildings.iter().filter(|(id, b)| b.working_depot() && Some(*id) != own).filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (l.car_point.distance(pos), id))).collect();
        others.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let depots: Vec<BuildingId> = own.into_iter().chain(others.into_iter().map(|d| d.1)).collect();
        for d in depots {
            let Some((seg, pt)) = self.car_stop(d) else { continue };
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
            let r = &mut self.transit.routes[bus.route];
            r.riders_today += 1;
            r.fares_today += fare;
            r.stops[stop_idx].stats.on_today += 1;
        }
        // [pt-transit] Spacing: a bus calling less than half a headway
        // after the last one here waits a little, so the buses behind a
        // slow one do not all catch it up.
        let hold = {
            let secs = self.config.traffic_seconds_per_tick.max(0.1);
            let r = &mut self.transit.routes[bus.route];
            let half = r.headway(secs) as f32 * 0.5;
            let last = r.stops[stop_idx].stats.last_call;
            r.stops[stop_idx].stats.last_call = tick;
            let gap = tick.saturating_sub(last) as f32;
            let extra = if last > 0 && !bus.to_depot && r.buses > 1 && gap < half { ((half - gap) * secs).min(transit::HOLD_MAX_SECONDS) } else { 0.0 };
            transit::DWELL_SECONDS + extra
        };
        // On to the next stop, or home.
        let (next_idx, to_seg, to_pos) = if bus.to_depot {
            match self.car_stop(bus.depot) {
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
        // [pt-transit] On along the leg as drawn, when the bus stands on
        // the lane it starts from (it does, having driven the leg before);
        // otherwise from the lane it stands in; only if there is no way
        // from there is it set on a route from the stop's own street.
        let s_here = self.traffic.cars[car_id].s;
        let lane_here = self.traffic.cars[car_id].route[self.traffic.cars[car_id].idx];
        let drawn = if next_idx == usize::MAX {
            None
        } else {
            self.transit.routes[bus.route].leg_lanes.get(stop_idx).cloned().flatten().filter(|(lanes, s0, s1)| lanes.first() == Some(&lane_here) && (lanes.len() > 1 || *s1 >= s_here) && *s0 <= s_here + 30.0)
        };
        let goal = self.traffic.net.nearest_lane_on(to_seg, to_pos);
        let from_here = || goal.and_then(|(g, s1, _)| self.traffic.route_from_here(car_id, g, s1).map(|r| (r, s_here, s1)));
        match drawn.map(|(lanes, _, s1)| (lanes, s_here, s1)).or_else(from_here).or_else(|| self.traffic.net.find_route(here.segment, pos, to_seg, to_pos)) {
            Some((lanes, s0, s1)) => {
                self.traffic.reroute(car_id, lanes, s0, s1);
                let car = &mut self.traffic.cars[car_id];
                car.hold = hold;
                if let Some(b) = car.bus.as_mut() {
                    b.next_stop = next_idx;
                    b.passengers = passengers;
                }
                // [pt-transit] Each leg is a drive of its own: timed from
                // this stop (the inspector said "568 min on the road" for
                // a bus out since the morning).
                self.begin_leg(car_id);
            }
            None => gone(self, car_id),
        }
    }
}
