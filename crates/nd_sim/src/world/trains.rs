//! Rail: stations and yards, rail links, trains and their passengers
//! and freight. Shared with buses: `walk_to_transit`, `board_waiting`.

use super::*;

impl World {
    /// Stations and yards that run: connected both ways and standing.
    pub(super) fn facilities(&self, kind: RailFacility) -> Vec<BuildingId> {
        self.buildings.iter().filter(|(_, b)| b.rail.as_ref().is_some_and(|l| l.kind == kind && l.works()) && !b.is_derelict() && b.operating()).map(|(id, _)| id).collect()
    }

    /// Recompute the working station and yard lists. Called when links,
    /// rail buildings or conditions change; everything else reads the lists.
    pub(super) fn refresh_facility_cache(&mut self) {
        // [subway] A Central Station is a passenger station too.
        self.stations = self.facilities(RailFacility::Station);
        self.stations.extend(self.facilities(RailFacility::Central));
        self.yards = self.facilities(RailFacility::FreightYard);
        // [dock-access] Rail-only stations: on the railway, not yet on the roads.
        self.halts = self
            .buildings
            .iter()
            .filter(|(_, b)| b.rail.as_ref().is_some_and(|l| l.kind.is_passenger() && l.rail_connected && !l.works()) && !b.is_derelict())
            .map(|(id, _)| id)
            .collect();
    }

    pub(super) fn nearest_yard(&self, to: BuildingId) -> Option<BuildingId> {
        self.nearest_by_entrance(to, self.yards.iter().copied())
    }

    /// [review] Of `ids`, the building whose entrance is nearest `to`'s
    /// (the first of equals, in the order given).
    pub(super) fn nearest_by_entrance(&self, to: BuildingId, ids: impl IntoIterator<Item = BuildingId>) -> Option<BuildingId> {
        let p = self.building_entrance(to)?;
        ids.into_iter().filter_map(|y| self.building_entrance(y).map(|q| (y, q.distance(p)))).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(y, _)| y)
    }

    /// Set down a station or freight yard at `p`: it fuses to track or
    /// street within reach, lays connector stubs for whatever is missing,
    /// and gets a lot and a building of its own. It runs once both stubs
    /// are joined to the networks.
    pub fn place_rail_facility(&mut self, kind: RailFacility, p: Vec2) -> Result<BuildingId, String> {
        let (facility, rail_fused, road_fused) = Facility::plan(kind, p, &self.roads);
        let cost = kind.cost();
        if self.economy.treasury < cost {
            return Err(format!("the treasury has {:.0}; a {} costs {:.0}", self.economy.treasury, kind.label(), cost));
        }
        if let Some(why) = self.rail_piece_problem(&facility) {
            return Err(why);
        }
        let fp = facility.footprint();
        let under: Vec<(BuildingId, LotId)> = self
            .buildings
            .iter()
            .filter(|(_, b)| self.lots.get(b.lot).is_some_and(|l| point_in_polygon(l.center, &fp) || polygons_overlap(&fp, &l.polygon)))
            .map(|(id, b)| (id, b.lot))
            .collect();
        for (_, lot) in under {
            self.take_over_lot(lot, kind.label())?;
        }
        self.economy.debit_item(Flow::Construction, cost, Item::Named(kind.label().to_string()));
        let fid = self.facilities.insert(facility.clone());
        // Connector stubs become part of the plan, so they survive rebuilds
        // and roads and track can be snapped onto their ends.
        let had_project = self.project.take();
        if !rail_fused {
            let [a, b] = facility.rail_line();
            self.add_gesture(Gesture::Road { points: vec![a, b], lanes_fwd: 0, lanes_bwd: 0, curved: false, kind: RoadKind::Rail, elevation: Vec::new(), car_only: None });
        }
        if !road_fused {
            let [a, b] = facility.road_line();
            self.add_gesture(Gesture::Road { points: vec![a, b], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new(), car_only: None });
        }
        if self.project.is_some() {
            self.implement();
        } else {
            self.rebuild_from_master();
        }
        self.project = had_project;
        // [w1-roads] One that cannot stand after all is taken back, not left
        // as a building-less facility blocking the ground (and every later
        // station near it as "too close").
        let built = self.facilities[fid].lot.ok_or_else(|| "the facility lost its lot".to_string()).and_then(|lot_id| self.attach_rail_building(lot_id, kind));
        let id = match built {
            Ok(id) => id,
            Err(e) => {
                self.facilities.remove(fid);
                self.rebuild_from_master();
                return Err(e);
            }
        };
        self.facilities[fid].building = Some(id);
        self.refresh_rail_links();
        Ok(id)
    }

    /// Why a station or yard planned as `facility` cannot go where it is
    /// (`place_rail_facility` and the Build tool's verdict share this).
    /// Everything that grew under the footprint is bought out; another
    /// civic structure there stops the placement. No road is needed.
    pub fn rail_piece_problem(&self, facility: &Facility) -> Option<String> {
        let kind = facility.kind;
        let fp = facility.footprint();
        if let Some(b) = self.buildings.values().filter(|b| self.lots.get(b.lot).is_some_and(|l| point_in_polygon(l.center, &fp) || polygons_overlap(&fp, &l.polygon))).find(|b| Self::is_civic_structure(b)) {
            return Some(format!("{} stands there; bulldoze it first", b.name));
        }
        // [w1-roads] Nothing over a road (its own track runs through it).
        if let Some(why) = self.road_under(crate::world::over_roads::Footprint::Area(&fp), kind.label(), false) {
            return Some(why);
        }
        // [subway] The Central Station is the one grand terminus.
        if kind == RailFacility::Central && self.buildings.values().any(|b| b.rail.as_ref().is_some_and(|l| l.kind == RailFacility::Central)) {
            return Some("the city has its Central Station already".into());
        }
        let spacing = |k: RailFacility| Facility::size(k).0 * 0.5;
        if self.facilities.values().any(|f| f.center.distance(facility.center) < (spacing(kind) + spacing(f.kind)) * 0.9) {
            return Some("too close to another station or yard".into());
        }
        None
    }

    /// The lot a facility stands on: its footprint, fronting the nearest
    /// street (normally its own stub). `None` when the city has no street
    /// at all: a lot needs a frontage, and inventing one on the railway
    /// trunk put stations' entrances a kilometre away.
    pub(super) fn facility_lot(&self, fid: FacilityId) -> Option<Lot> {
        let f = &self.facilities[fid];
        let corners = f.corners();
        let front_mid = f.front_mid();
        let probe = front_mid - f.inward() * rail::ROAD_GAP;
        let (front_segment, front_t, car_point) = self.roads.nearest_street(probe)?;
        Some(Lot {
            zone: ZoneId::default(),
            land_use: if f.kind.is_passenger() { LandUse::Civic } else { LandUse::Industrial },
            polygon: corners.to_vec(),
            corners,
            front_edge: [corners[0], corners[1]],
            center: f.center,
            front_dir: -f.inward(),
            front_segment,
            front_t,
            access_node: self.roads.nearest_endpoint(front_segment, front_t),
            // Left empty: the rebuild matches the old lot's building onto it.
            building: None,
            landlocked: false,
            footfall_today: 0,
            footfall_hour: 0,
            car_segment: Some(front_segment),
            car_point,
            park: None,
            shore: Default::default(),
        })
    }

    /// Recompute whether each station and yard reaches the railway and the
    /// city's roads.
    pub(super) fn refresh_rail_links(&mut self) {
        // The railway proper is reached when a trunk end is: a facility's
        // own stub has ends too, and they do not count. The roads are
        // reached when the facility's street shares a component with a
        // zoned lot's street: there is a city at the other end of it.
        let trunk_ends: Vec<NodeId> = self.roads.rail_leaf_nodes().into_iter().filter(|&n| (self.roads.node(n).pos.y - TRUNK_Y).abs() < 1.0).collect();
        // [dock-access] Or reaches off the map (the highway, a road out).
        let (city_walk, city) = self.network_components();
        let ids: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.rail.is_some()).map(|(id, _)| id).collect();
        for id in ids {
            // The track beside the facility may have been split, re-laid or
            // removed, and segment ids are renumbered by every rebuild, so
            // find it afresh before asking whether it reaches the trunk.
            let lot_poly = self.lots.get(self.buildings[id].lot).map(|l| (l.polygon.clone(), l.center, l.front_edge));
            let kind = self.buildings[id].rail.as_ref().unwrap().kind;
            let length = kind.platform_length();
            let fresh = lot_poly.and_then(|(poly, center, front)| RailLink::beside(&self.roads, &poly, center, front, kind, length));
            let segment = fresh.as_ref().map(|f| f.segment).or_else(|| {
                let old = self.buildings[id].rail.as_ref().unwrap().segment;
                self.roads.try_segment(old).filter(|s| s.is_rail()).map(|_| old)
            });
            let rail_ok = segment.is_some_and(|sid| {
                let node = self.roads.segment(sid).a;
                trunk_ends.iter().any(|&l| self.roads.rail_path(node, l).is_some_and(|p| p.len() > 1))
            });
            let node = self.lots.get(self.buildings[id].lot).map(|l| l.access_node);
            let road_ok = node.and_then(|n| self.drive_component(n)).is_some_and(|c| city.contains(&c));
            // [sweep-sim] On foot: a path to the street stub will do for passengers.
            let walk_ok = node.and_then(|n| self.road_component(n)).is_some_and(|c| city_walk.contains(&c));
            if let Some(mut fresh) = fresh {
                fresh.rail_connected = rail_ok;
                fresh.road_connected = road_ok;
                fresh.walk_connected = walk_ok;
                self.buildings[id].rail = Some(fresh);
            } else {
                let link = self.buildings[id].rail.as_mut().unwrap();
                link.rail_connected = rail_ok;
                link.road_connected = road_ok;
                link.walk_connected = walk_ok;
            }
        }
        let had_yards = !self.yards.is_empty();
        self.refresh_facility_cache();
        // The freight timetable starts when the first yard opens; otherwise
        // the stranded-goods drain would see "no train for two headways"
        // before the first one has had its chance.
        if !had_yards && !self.yards.is_empty() {
            self.rail.last_freight_tick = self.clock.tick;
        }
    }

    /// The building on a facility's lot.
    pub(super) fn attach_rail_building(&mut self, lot_id: LotId, kind: RailFacility) -> Result<BuildingId, String> {
        let lot = self.lots.get(lot_id).ok_or("no such lot")?;
        let (jobs, length) = (kind.jobs(), kind.platform_length());
        let link = RailLink::beside(&self.roads, &lot.polygon, lot.center, lot.front_edge, kind, length).ok_or("the track stub did not get laid")?;
        let name = match kind {
            RailFacility::Station => format!("{} Station", names::building_name(&mut self.rng, LandUse::Civic).split(' ').next().unwrap_or("Central")),
            RailFacility::FreightYard => format!("Yard {}", self.rng.random_range(1..30)),
            RailFacility::Central => "Central Station".to_string(),
        };
        let parking = match kind {
            RailFacility::Station => 12,
            RailFacility::FreightYard => 4,
            RailFacility::Central => 60,
        };
        // [subway] The Central Station is also a metro station (its
        // platforms under the hall) and a bus terminal (a depot at its
        // front, where buses start and end their day).
        let central = kind == RailFacility::Central;
        let id = self.place_special(lot_id, name, jobs, parking, |b| {
            b.rail = Some(link);
            b.metro = central;
            b.depot = central;
        });
        // [dock-access] Like a dock: on its own footprint and drawn as itself
        // at once, not as the clearing of what grew there.
        self.buildings[id].demolition = None;
        if central {
            self.subway.version += 1;
        }
        Ok(id)
    }

    /// A building the player places finished: one floor, middle tier, no
    /// homes, `jobs` jobs, no business unless `setup` adds one. Plants,
    /// depots, stations, yards and car parks all start here.
    pub(super) fn place_special(&mut self, lot_id: LotId, name: String, jobs: u16, parking: u16, setup: impl FnOnce(&mut Building)) -> BuildingId {
        let lot = &self.lots[lot_id];
        let (lu, value) = (lot.land_use, self.land_value.at(lot.center));
        let seed = self.fresh_seed();
        let mut b = Building::new(lot_id, lu, 1, Tier::Mid, seed, name, self.clock.tick, value);
        b.homes = 0;
        b.jobs = jobs;
        b.business = None;
        b.parking_capacity = parking;
        setup(&mut b);
        // [free-placement] The city's own building: its ground leaves the
        // zone (a station's or dock's own lot is in none and stays as it is).
        // The building keeps its own use for demand, wages and venues
        // (`city_use`), whatever zone the ground was in.
        if Self::is_civic_structure(&b) && self.zones.contains_key(self.lots[lot_id].zone) {
            self.take_for_city(lot_id);
            if let Some(own) = Self::city_use(&b) {
                b.land_use = own;
            }
        }
        // What the city took the lot over from is cleared first.
        b.demolition = self.take_clearing(lot_id);
        let id = self.buildings.insert(b);
        self.lots[lot_id].building = Some(id);
        self.events.push(SimEvent::BuildingBuilt(id));
        id
    }

    /// Stations within reach of both ends of a long walk: (board, alight,
    /// where to walk to).
    pub(super) fn plan_rail_trip(&self, from: Vec2, to: Vec2) -> Option<(BuildingId, BuildingId, Vec2)> {
        if from.distance(to) < rail::RAIL_TRIP_MIN {
            return None;
        }
        // [subway] Each station draws from its own reach: a Central Station
        // from further than a halt.
        let stations: Vec<(BuildingId, Vec2, f32)> = self.stations.iter().copied().filter_map(|s| self.building_entrance(s).map(|p| (s, p, self.buildings[s].rail.as_ref().map_or(rail::RAIL_REACH, |l| l.kind.reach())))).collect();
        if stations.len() < 2 {
            return None;
        }
        let near = |p: Vec2| stations.iter().map(|(id, q, r)| (*id, *q, q.distance(p), *r)).filter(|(_, _, d, r)| *d <= *r).min_by(|a, b| a.2.total_cmp(&b.2)).map(|(id, q, d, _)| (id, q, d));
        let (a, pa, da) = near(from)?;
        let (b, _, db) = near(to)?;
        if a == b || da + db + 200.0 > from.distance(to) {
            return None;
        }
        Some((a, b, pa))
    }

    /// Walk to a stop or station and wait there for the ride. `count` says
    /// whether this is the start of a journey (its footfall is noted) or
    /// the continuation of one.
    pub(super) fn walk_to_transit(&mut self, id: CitizenId, stop: Vec2, leg: TransitLeg, target: BuildingId, purpose: Purpose, count: bool) {
        crate::prof_scope!("walk_to_transit");
        let exit_pos = self.leave_building(id);
        let start = exit_pos.unwrap_or(self.citizens[id].pos);
        let mut path = match self.walk.route(start, stop) {
            Some((pts, nodes)) => {
                if count {
                    self.note_footfall(id, &nodes);
                } else {
                    self.add_footfall(id, &nodes);
                }
                pts
            }
            None => vec![start],
        };
        path.push(stop);
        let tick = self.clock.tick;
        let c = &mut self.citizens[id];
        if let Some(p) = exit_pos {
            c.pos = p;
            c.prev_pos = p;
        }
        c.mode = TravelMode::Walk;
        c.transit = Some(TransitTrip { leg, target, purpose, since: tick, bike: false });
        c.activity = Activity::Traveling { to: Some(target), purpose: Purpose::ToStop, path, idx: 0 };
    }

    /// [bike-racks] A cyclist taking the bus with their bike: ride to the
    /// stop along the streets (or wheel it along the pavement when there is
    /// no way by road), and wait there for a bus with a free place on its
    /// rack.
    pub(super) fn ride_to_bus_stop(&mut self, id: CitizenId, stop: Vec2, leg: TransitLeg, target: BuildingId, purpose: Purpose, count: bool) {
        let from_node = self.origin_for(id).map(|o| o.2);
        let to_node = self.roads.nearest_where(stop, |s| s.is_street() && !s.fronts_nothing()).map(|(sid, t, _)| self.roads.nearest_endpoint(sid, t));
        let streets = from_node.zip(to_node).and_then(|(a, b)| self.roads.foot_path(a, b));
        self.walk_to_transit(id, stop, leg, target, purpose, count);
        let tick = self.clock.tick;
        if let Some(t) = self.citizens[id].transit.as_mut() {
            t.bike = true;
        }
        let Some(nodes) = streets.filter(|n| n.len() >= 2) else { return };
        let mut path = self.route_path(&nodes, TravelMode::Bike);
        path.push(stop);
        self.bike_over_footbridges(&mut path);
        let c = &mut self.citizens[id];
        c.mode = TravelMode::Bike;
        c.activity = Activity::Traveling { to: Some(target), purpose: Purpose::ToStop, path, idx: 0 };
        c.transit = Some(TransitTrip { leg, target, purpose, since: tick, bike: true });
    }

    /// [train-inspect] What a freight train carries: the imports loaded
    /// onto it at the edge of the map (they come off at their yards as it
    /// calls), the exports it has picked up, and the exports waiting at the
    /// yards ahead. Loads bound for a cargo hub name the business that
    /// ordered them. `None` for a passenger train or no such train.
    pub fn freight_manifest(&self, id: TrainId) -> Option<rail::FreightManifest> {
        let t = self.rail.trains.get(id).filter(|t| t.kind == TrainKind::Freight)?;
        let mut m = rail::FreightManifest { exports_aboard: t.exports, unloaded: t.unloaded, wagons: t.cars, capacity: t.freight_capacity(), aboard: t.freight_aboard(), ..Default::default() };
        // Hub orders already matched to a load, so two equal loads name two orders.
        let mut used: Vec<(BuildingId, usize)> = Vec::new();
        for c in &t.cargo {
            let (buyer, hub) = match self.cargo_hubs.hubs.get(&c.to) {
                Some(h) => {
                    let found = h.inbound.iter().enumerate().find(|(i, l)| l.via == crate::cargo_hub::Via::Rail && l.resource == c.resource && (l.amount - c.amount).abs() < 0.01 && !used.contains(&(c.to, *i)));
                    match found {
                        Some((i, l)) => {
                            used.push((c.to, i));
                            (l.buyer, Some(c.to))
                        }
                        None => (c.to, Some(c.to)),
                    }
                }
                None => (c.to, None),
            };
            m.loads.push(rail::FreightLoad { resource: c.resource, amount: c.amount, price: c.price, yard: c.from, buyer, hub });
        }
        let ahead = t.calls_ahead(rail::DWELL_SECONDS, self.config.traffic_seconds_per_tick, |b| self.buildings.contains_key(b));
        for call in ahead {
            let Some(yard) = self.buildings.get(call.stop) else { continue };
            if yard.rail_outbound > 0.0 {
                m.exports_to_collect.push((call.stop, yard.rail_outbound));
            }
        }
        Some(m)
    }

    /// The route a train takes from the west end of the line to the east,
    /// calling at each facility in turn, and where along it each one lies.
    pub(super) fn plan_train_route(&self, facilities: &[BuildingId]) -> Option<(Vec<NodeId>, Vec<Vec2>, Vec<(BuildingId, f32)>)> {
        let leaves = self.roads.rail_leaf_nodes();
        let entry = leaves.iter().copied().min_by(|a, b| self.roads.node(*a).pos.x.total_cmp(&self.roads.node(*b).pos.x))?;
        let exit = leaves.iter().copied().max_by(|a, b| self.roads.node(*a).pos.x.total_cmp(&self.roads.node(*b).pos.x)).unwrap_or(entry);
        let mut order: Vec<(BuildingId, SegmentId, Vec2)> = facilities.iter().filter_map(|&f| self.buildings.get(f)?.rail.as_ref().map(|l| (f, l.segment, l.centre()))).filter(|(_, s, _)| self.roads.try_segment(*s).is_some_and(|s| s.alive && s.is_rail())).collect();
        order.sort_by(|a, b| a.2.x.total_cmp(&b.2.x));
        // Points the train drives through: node positions, plus the platform
        // of the last call, where a train on a dead-end spur turns round
        // instead of running on to the end of the track.
        let mut poly: Vec<Vec2> = vec![self.roads.node(entry).pos];
        let mut nodes = vec![entry];
        let mut cur = entry;
        let push_path = |poly: &mut Vec<Vec2>, nodes: &mut Vec<NodeId>, path: Vec<NodeId>, roads: &RoadNetwork| {
            for n in path.into_iter().skip(1) {
                let p = roads.node(n).pos;
                if poly.last().is_none_or(|q: &Vec2| q.distance(p) > 0.1) {
                    poly.push(p);
                }
                nodes.push(n);
            }
        };
        let last = order.len().saturating_sub(1);
        for (i, (_, seg, centre)) in order.iter().enumerate() {
            let s = self.roads.segment(*seg);
            let pa = self.roads.node(s.a).pos;
            let pb = self.roads.node(s.b).pos;
            let here = self.roads.node(cur).pos;
            let (first, second) = if here.distance(pa) <= here.distance(pb) { (s.a, s.b) } else { (s.b, s.a) };
            if let Some(path) = self.roads.rail_path(cur, first) {
                push_path(&mut poly, &mut nodes, path, &self.roads);
                cur = first;
            }
            let (_, on_track) = crate::roads::project_on_segment(*centre, pa, pb);
            let spur_end = self.roads.neighbors(second).count() <= 1;
            if i == last && spur_end {
                // Run to the platform and come back the same way.
                poly.push(on_track);
            } else if let Some(path) = self.roads.rail_path(cur, second) {
                push_path(&mut poly, &mut nodes, path, &self.roads);
                cur = second;
            }
        }
        let home = if exit != cur { exit } else { entry };
        if let Some(path) = self.roads.rail_path(cur, home) {
            let first_back = self.roads.node(cur).pos;
            if poly.last().is_none_or(|q: &Vec2| q.distance(first_back) > 0.1) {
                poly.push(first_back);
            }
            push_path(&mut poly, &mut nodes, path, &self.roads);
        }
        if poly.len() < 2 {
            return None;
        }
        let mut stops: Vec<(BuildingId, f32)> = Vec::new();
        for (f, _, c) in &order {
            if let Some((s, d)) = rail::project_on_poly(&poly, *c) {
                if d < 8.0 {
                    stops.push((*f, s));
                }
            }
        }
        stops.sort_by(|a, b| a.1.total_cmp(&b.1));
        Some((nodes, poly, stops))
    }

    pub(super) fn spawn_train(&mut self, kind: TrainKind, facilities: &[BuildingId]) -> bool {
        let Some((nodes, poly, stops)) = self.plan_train_route(facilities) else { return false };
        if stops.is_empty() {
            return false;
        }
        // [freight] A freight train is made up at the edge of the map: the
        // orders for the yards it calls at are loaded, oldest first, and it
        // gets wagons for them and for the exports waiting on its way.
        let mut cargo: Vec<Cargo> = Vec::new();
        let (cars, speed) = match kind {
            TrainKind::Passenger => (4, rail::PASSENGER_SPEED),
            TrainKind::Freight => {
                let yards: Vec<BuildingId> = stops.iter().map(|s| s.0).collect();
                let imports: f32 = self.rail.orders.iter().filter(|o| yards.contains(&o.cargo.from)).map(|o| o.cargo.amount).sum();
                let exports: f32 = yards.iter().filter_map(|y| self.buildings.get(*y)).map(|b| b.rail_outbound).sum();
                let cars = rail::wagons_for(imports.max(exports));
                cargo = rail::fill_wagons(&mut self.rail.orders, &yards, cars as f32 * rail::WAGON_CAPACITY);
                (cars, rail::FREIGHT_SPEED)
            }
        };
        let color = self.rng.random_range(0..8u8);
        // Where the route leaves the trunk and where it is back on it.
        // The hold point is the last trunk point before the track leaves
        // the trunk (the switch); the branch is clear again once the train
        // is back on a trunk point.
        let mut branch: Option<(f32, f32)> = None;
        let mut acc = 0.0;
        let mut prev_acc = 0.0;
        let mut off = false;
        for (i, p) in poly.iter().enumerate() {
            if i > 0 {
                acc += poly[i - 1].distance(*p);
            }
            let on_trunk = (p.y - TRUNK_Y).abs() < 1.0;
            if !on_trunk && !off {
                off = true;
                branch = Some(match branch {
                    None => (prev_acc - 12.0, acc + 12.0),
                    Some((a, _)) => (a, acc + 12.0),
                });
            } else if !on_trunk {
                if let Some((a, _)) = branch {
                    branch = Some((a, acc + 12.0));
                }
            } else if off {
                off = false;
                if let Some((a, _)) = branch {
                    branch = Some((a, acc + 12.0));
                }
            }
            prev_acc = acc;
        }
        self.rail.trains.insert(Train { kind, cars, nodes, poly, s: 0.0, prev_s: 0.0, speed, stops, next_stop: 0, dwell: 0.0, passengers: Vec::new(), cargo, color, held: false, branch, exports: 0.0, unloaded: 0.0 });
        true
    }

    /// [freight] Whether a freight train is due: on the headway when goods
    /// wait (imports out of town, exports on an apron), every two headways
    /// regardless, and an extra one soon after the last when goods were left
    /// behind (more than one train holds, or an order that missed a train).
    pub(super) fn freight_due(&self) -> bool {
        let tick = self.clock.tick;
        let since = tick.saturating_sub(self.rail.last_freight_tick);
        let imports: f32 = self.rail.orders.iter().filter(|o| self.yards.contains(&o.cargo.from)).map(|o| o.cargo.amount).sum();
        let exports: f32 = self.yards.iter().filter_map(|y| self.buildings.get(*y)).map(|b| b.rail_outbound).sum();
        let waiting = imports > 0.0 || exports > 0.0;
        if since >= rail::FREIGHT_HEADWAY && waiting || since >= rail::FREIGHT_HEADWAY * 2 {
            return true;
        }
        let full = rail::MAX_WAGONS as f32 * rail::WAGON_CAPACITY;
        let missed = self.rail.orders.iter().any(|o| self.yards.contains(&o.cargo.from) && tick.saturating_sub(o.since) >= rail::FREIGHT_HEADWAY);
        since >= rail::EXTRA_FREIGHT_GAP && (imports.max(exports) > full || missed)
    }

    /// [freight] A freight train ran off the end of the line with imports
    /// still aboard (its yard was pulled down or shut while it ran): loads
    /// for a yard that still works wait for the next train; the rest come
    /// in by road from the edge of the map.
    fn freight_left_town(&mut self, cargo: Vec<Cargo>) {
        let tick = self.clock.tick;
        for c in cargo {
            if self.yards.contains(&c.from) {
                self.rail.orders.push(rail::RailOrder { cargo: c, since: tick.saturating_sub(rail::FREIGHT_HEADWAY) });
            } else {
                self.dispatch_truck(super::market::TruckOrigin::CityEdge, Cargo { from: BuildingId::default(), ..c }, 6);
            }
        }
    }

    /// Dispatch on the headway, move every train, call at stops.
    pub(super) fn step_rail(&mut self) {
        let tick = self.clock.tick;
        let minute = self.clock.minute_of_day() as u32;
        let seconds = self.config.traffic_seconds_per_tick;
        let queued = self.rail.trains.values().any(|t| t.held && t.branch.is_some_and(|(a, _)| t.s <= a + 0.5));
        let room = self.rail.trains.len() < rail::MAX_TRAINS && !queued;
        // [dock-access] Trains call at stations not yet on the roads too
        // (rail to rail); nobody gets on or off there.
        if room && Rail::in_service(minute) && !(self.stations.is_empty() && self.halts.is_empty()) && tick.saturating_sub(self.rail.last_passenger_tick) >= rail::PASSENGER_HEADWAY {
            let stations: Vec<BuildingId> = self.stations.iter().chain(self.halts.iter()).copied().collect();
            if self.spawn_train(TrainKind::Passenger, &stations) {
                self.rail.stats.passenger_trains_today += 1;
            }
            self.rail.last_passenger_tick = tick;
        }
        if room && !self.yards.is_empty() && self.freight_due() {
            let yards = self.yards.clone();
            if self.spawn_train(TrainKind::Freight, &yards) {
                self.rail.stats.freight_trains_today += 1;
            }
            self.rail.last_freight_tick = tick;
        }
        let ids: Vec<TrainId> = self.rail.trains.keys().collect();
        let waited = std::mem::take(&mut self.level_crossings.trains_waiting);
        for id in ids {
            let blocked = self.rail.blocked(id);
            // [rail-crossing] Short of a crossing not yet closed and clear.
            let crossing = self.crossing_hold(id);
            let branch_busy = self.rail.trains.iter().any(|(oid, o)| oid != id && o.on_branch());
            // A stop whose station or yard was pulled down is passed.
            while self.rail.trains[id].next_stop < self.rail.trains[id].stops.len() && !self.buildings.contains_key(self.rail.trains[id].stops[self.rail.trains[id].next_stop].0) {
                self.rail.trains[id].next_stop += 1;
            }
            let arrived = {
                let t = &mut self.rail.trains[id];
                t.prev_s = t.s;
                if t.dwell > 0.0 {
                    t.dwell -= seconds;
                    if t.dwell <= 0.0 {
                        t.next_stop += 1;
                    }
                    continue;
                }
                if blocked {
                    t.held = true;
                    continue;
                }
                t.held = false;
                let target = if t.next_stop < t.stops.len() { t.stops[t.next_stop].1 } else { t.total() };
                // Hold at the junction while another train is on the branch.
                let mut limit = target;
                if let Some((bin, _)) = t.branch {
                    if t.s <= bin + 0.5 && branch_busy {
                        limit = limit.min(bin);
                        t.held = true;
                    }
                }
                let mut at_crossing = None;
                if let Some((h, k)) = crossing.filter(|&(h, _)| h < limit && h < t.s + t.speed * seconds) {
                    limit = h;
                    at_crossing = Some(k);
                }
                t.s = (t.s + t.speed * seconds).min(limit);
                if let Some(k) = at_crossing.filter(|_| t.s >= limit - 0.01) {
                    self.level_crossings.trains_waiting.push(id);
                    if !waited.contains(&id) {
                        if let Some(c) = self.level_crossings.list.get_mut(k) {
                            c.trains_held += 1;
                        }
                    }
                }
                if t.s >= target - 0.01 {
                    if t.next_stop < t.stops.len() { Some(true) } else { Some(false) }
                } else {
                    None
                }
            };
            match arrived {
                Some(true) => self.train_arrived(id),
                Some(false) => {
                    let mut t = self.rail.trains.remove(id).unwrap();
                    let pos = t.head();
                    let left = std::mem::take(&mut t.cargo);
                    if !left.is_empty() {
                        self.freight_left_town(left);
                    }
                    for p in t.passengers {
                        if let Some(c) = self.citizens.get_mut(p) {
                            c.transit = None;
                        }
                        self.strand_driver(p, pos);
                    }
                }
                None => {}
            }
        }
        // Riders ride along.
        let heads: Vec<(Vec<CitizenId>, Vec2, f32)> = self.rail.trains.values().map(|t| (t.passengers.clone(), t.head(), t.vehicle_pose(0).1)).collect();
        for (ps, pos, heading) in heads {
            for p in ps {
                if let Some(c) = self.citizens.get_mut(p) {
                    c.pos = pos;
                    c.prev_pos = pos;
                    c.heading = heading;
                }
            }
        }
        // The counts are tallied in `update_economy`; only the countdown is
        // worth keeping to the minute.
        self.rail.stats.trains = self.rail.trains.len() as u32;
        self.plan_rail_day_trips();
        self.rail.stats.next_passenger_in = if self.stations.is_empty() && self.halts.is_empty() { 0 } else { rail::PASSENGER_HEADWAY.saturating_sub(tick.saturating_sub(self.rail.last_passenger_tick)) as u32 };
    }

    /// A train stands at a station or yard: people off and on, goods off.
    pub(super) fn train_arrived(&mut self, id: TrainId) {
        let (at, kind) = {
            let t = &mut self.rail.trains[id];
            t.dwell = rail::DWELL_SECONDS;
            (t.stops[t.next_stop].0, t.kind)
        };
        match kind {
            // [dock-access] A station off the roads: the train stands its
            // time and goes on; nobody could walk away from it.
            TrainKind::Passenger if !self.stations.contains(&at) => {}
            TrainKind::Passenger => {
                let entrance = self.building_entrance(at).unwrap_or(self.rail.trains[id].head());
                let passengers = self.rail.trains[id].passengers.clone();
                let mut keep: Vec<CitizenId> = Vec::new();
                for p in passengers {
                    let off = self.citizens.get(p).and_then(|c| c.transit).is_none_or(|t| !matches!(t.leg, TransitLeg::Rail { alight, .. } if alight != at));
                    if !off {
                        keep.push(p);
                        continue;
                    }
                    let Some(trip) = self.citizens.get_mut(p).and_then(|c| {
                        c.pos = entrance;
                        c.prev_pos = entrance;
                        c.activity = Activity::Idle;
                        c.transit.take()
                    }) else {
                        continue;
                    };
                    self.travel_to_impl(p, Some(trip.target), trip.purpose, true, false);
                }
                let name = self.buildings.get(at).map(|b| b.name.clone()).unwrap_or_default();
                let boarding = self.board_waiting(|t| matches!(&t.leg, TransitLeg::Rail { board, .. } if *board == at), rail::TRAIN_CAPACITY.saturating_sub(keep.len()), rail::RAIL_FARE, Flow::RailFares, Item::Building(at), &format!("Took the train from {name}."));
                for p in boarding {
                    self.citizens[p].activity = Activity::OnTrain { train: id };
                    keep.push(p);
                    self.rail.stats.fares_today += rail::RAIL_FARE;
                    self.rail.stats.riders_today += 1;
                }
                if let Some(b) = self.buildings.get_mut(at) {
                    b.visitors = b.visitors.saturating_add(1);
                }
                self.rail.trains[id].passengers = keep;
                // [pt-transit] And whoever it brings from out of town.
                self.rail_arrivals(at);
            }
            TrainKind::Freight => {
                if !self.buildings.contains_key(at) {
                    return;
                }
                // [freight] The loads for this yard come off; then exports
                // go on, as much as the wagons have room for (the rest wait
                // on the apron for the next train).
                let t = &mut self.rail.trains[id];
                let (queued, stay): (Vec<Cargo>, Vec<Cargo>) = std::mem::take(&mut t.cargo).into_iter().partition(|c| c.from == at);
                t.cargo = stay;
                t.unloaded += queued.iter().map(|c| c.amount).sum::<f32>();
                let room = (t.freight_capacity() - t.freight_aboard()).max(0.0);
                let b = &mut self.buildings[at];
                let exports = b.rail_outbound.min(room);
                b.rail_outbound -= exports;
                if b.rail_outbound < 1e-3 {
                    b.rail_outbound = 0.0;
                }
                self.rail.trains[id].exports += exports;
                for cargo in queued {
                    self.rail.stats.freight_in_today += cargo.amount;
                    // Into a cargo hub on the yard's siding, else a lorry.
                    self.unload_freight(at, cargo);
                }
            }
        }
    }

    /// People waiting at a stop or station whose leg `pred` accepts board,
    /// up to `capacity`: each pays `fare` into the treasury under `flow`
    /// and remembers `memory`. The caller seats them in its vehicle. The
    /// fares are itemised under `item` (the bus line, the station).
    pub(super) fn board_waiting(&mut self, pred: impl Fn(&TransitTrip) -> bool, capacity: usize, fare: f32, flow: Flow, item: Item, memory: &str) -> Vec<CitizenId> {
        crate::prof_scope!("board_waiting");
        let tick = self.clock.tick;
        // [perf] Everyone waiting for anything, listed once a tick (in slot
        // order, as a scan of every citizen would find them), instead of a
        // scan of the whole town at every stop a bus or train calls at.
        // Each is checked again here: they may have boarded or given up.
        let waiting = match self.waiting_cache.take() {
            Some((t, v)) if t == tick => v,
            _ => self.citizens.iter().filter(|(_, c)| matches!(c.activity, Activity::Waiting { .. })).map(|(id, _)| id).collect(),
        };
        let boarding: Vec<CitizenId> = waiting
            .iter()
            .copied()
            .filter(|&id| self.citizens.get(id).is_some_and(|c| matches!(c.activity, Activity::Waiting { .. }) && c.transit.as_ref().is_some_and(&pred)))
            .take(capacity)
            .collect();
        self.waiting_cache = Some((tick, waiting));
        for &p in &boarding {
            let c = &mut self.citizens[p];
            c.savings -= fare;
            c.remember(tick, memory.to_string());
            self.economy.credit_item(flow, fare, item.clone());
        }
        boarding
    }

    /// [bike-racks] How many are still waiting whose trip `pred` accepts
    /// (after `board_waiting` this tick, from its list).
    pub(super) fn still_waiting(&self, pred: impl Fn(&TransitTrip) -> bool) -> usize {
        let Some((_, v)) = self.waiting_cache.as_ref().filter(|(t, _)| *t == self.clock.tick) else { return 0 };
        v.iter().filter(|&&id| self.citizens.get(id).is_some_and(|c| matches!(c.activity, Activity::Waiting { .. }) && c.transit.as_ref().is_some_and(&pred))).count()
    }
}
