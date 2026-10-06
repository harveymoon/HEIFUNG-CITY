//! Rail: stations and yards, rail links, trains and their passengers
//! and freight. Shared with buses: `walk_to_transit`, `board_waiting`.

use super::*;

impl World {
    /// Stations and yards that run: connected both ways and standing.
    pub(super) fn facilities(&self, kind: RailFacility) -> Vec<BuildingId> {
        self.buildings.iter().filter(|(_, b)| b.rail.as_ref().is_some_and(|l| l.kind == kind && l.works()) && !b.is_derelict()).map(|(id, _)| id).collect()
    }

    /// Recompute the working station and yard lists. Called when links,
    /// rail buildings or conditions change; everything else reads the lists.
    pub(super) fn refresh_facility_cache(&mut self) {
        self.stations = self.facilities(RailFacility::Station);
        self.yards = self.facilities(RailFacility::FreightYard);
    }

    pub(super) fn nearest_yard(&self, to: BuildingId) -> Option<BuildingId> {
        let p = self.building_entrance(to)?;
        self.yards.iter().copied().filter_map(|y| self.building_entrance(y).map(|q| (y, q.distance(p)))).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(y, _)| y)
    }

    /// Set down a station or freight yard at `p`: it fuses to track or
    /// street within reach, lays connector stubs for whatever is missing,
    /// and gets a lot and a building of its own. It runs once both stubs
    /// are joined to the networks.
    pub fn place_rail_facility(&mut self, kind: RailFacility, p: Vec2) -> Result<BuildingId, String> {
        let (facility, rail_fused, road_fused) = Facility::plan(kind, p, &self.roads);
        let cost = match kind {
            RailFacility::Station => rail::STATION_COST,
            RailFacility::FreightYard => rail::YARD_COST,
        };
        if self.economy.treasury < cost {
            return Err(format!("the treasury has {:.0}; a {} costs {:.0}", self.economy.treasury, kind.label(), cost));
        }
        let fp = facility.footprint();
        // Everything that grew under the footprint is bought out; another
        // civic structure there stops the placement.
        let under: Vec<(BuildingId, LotId)> = self
            .buildings
            .iter()
            .filter(|(_, b)| self.lots.get(b.lot).is_some_and(|l| point_in_polygon(l.center, &fp) || polygons_overlap(&fp, &l.polygon)))
            .map(|(id, b)| (id, b.lot))
            .collect();
        if let Some(b) = under.iter().filter_map(|(id, _)| self.buildings.get(*id)).find(|b| Self::is_civic_structure(b)) {
            return Err(format!("{} stands there; bulldoze it first", b.name));
        }
        if self.facilities.values().any(|f| f.center.distance(facility.center) < Facility::size(kind).0 * 0.9) {
            return Err("too close to another station or yard".into());
        }
        for (_, lot) in under {
            self.take_over_lot(lot, kind.label())?;
        }
        self.economy.debit(Flow::Construction, cost);
        let fid = self.facilities.insert(facility.clone());
        // Connector stubs become part of the plan, so they survive rebuilds
        // and roads and track can be snapped onto their ends.
        let had_project = self.project.take();
        if !rail_fused {
            let [a, b] = facility.rail_line();
            self.add_gesture(Gesture::Road { points: vec![a, b], lanes_fwd: 0, lanes_bwd: 0, curved: false, kind: RoadKind::Rail, elevation: Vec::new() });
        }
        if !road_fused {
            let [a, b] = facility.road_line();
            self.add_gesture(Gesture::Road { points: vec![a, b], lanes_fwd: 1, lanes_bwd: 1, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
        }
        if self.project.is_some() {
            self.implement();
        } else {
            self.rebuild_from_master();
        }
        self.project = had_project;
        let lot_id = self.facilities[fid].lot.ok_or("the facility lost its lot")?;
        let id = self.attach_rail_building(lot_id, kind)?;
        self.facilities[fid].building = Some(id);
        self.refresh_rail_links();
        Ok(id)
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
            land_use: if f.kind == RailFacility::Station { LandUse::Civic } else { LandUse::Industrial },
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
        let city: std::collections::HashSet<u32> = self.lots.values().filter(|l| l.zone != ZoneId::default() && l.usable()).filter_map(|l| self.road_component(l.access_node)).collect();
        let ids: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| b.rail.is_some()).map(|(id, _)| id).collect();
        for id in ids {
            // The track beside the facility may have been split, re-laid or
            // removed, and segment ids are renumbered by every rebuild, so
            // find it afresh before asking whether it reaches the trunk.
            let lot_poly = self.lots.get(self.buildings[id].lot).map(|l| (l.polygon.clone(), l.center, l.front_edge));
            let kind = self.buildings[id].rail.as_ref().unwrap().kind;
            let length = if kind == RailFacility::Station { rail::STATION_PLATFORM } else { rail::YARD_APRON };
            let fresh = lot_poly.and_then(|(poly, center, front)| RailLink::beside(&self.roads, &poly, center, front, kind, length));
            let segment = fresh.as_ref().map(|f| f.segment).or_else(|| {
                let old = self.buildings[id].rail.as_ref().unwrap().segment;
                self.roads.try_segment(old).filter(|s| s.is_rail()).map(|_| old)
            });
            let rail_ok = segment.is_some_and(|sid| {
                let node = self.roads.segment(sid).a;
                trunk_ends.iter().any(|&l| self.roads.rail_path(node, l).is_some_and(|p| p.len() > 1))
            });
            let road_ok = self.lots.get(self.buildings[id].lot).and_then(|l| self.road_component(l.access_node)).is_some_and(|c| city.contains(&c));
            if let Some(mut fresh) = fresh {
                fresh.rail_connected = rail_ok;
                fresh.road_connected = road_ok;
                self.buildings[id].rail = Some(fresh);
            } else {
                let link = self.buildings[id].rail.as_mut().unwrap();
                link.rail_connected = rail_ok;
                link.road_connected = road_ok;
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
        let (jobs, length) = match kind {
            RailFacility::Station => (rail::STATION_JOBS, rail::STATION_PLATFORM),
            RailFacility::FreightYard => (rail::YARD_JOBS, rail::YARD_APRON),
        };
        let link = RailLink::beside(&self.roads, &lot.polygon, lot.center, lot.front_edge, kind, length).ok_or("the track stub did not get laid")?;
        let name = match kind {
            RailFacility::Station => format!("{} Station", names::building_name(&mut self.rng, LandUse::Civic).split(' ').next().unwrap_or("Central")),
            RailFacility::FreightYard => format!("Yard {}", self.rng.random_range(1..30)),
        };
        let parking = if kind == RailFacility::Station { 12 } else { 4 };
        Ok(self.place_special(lot_id, name, jobs, parking, |b| b.rail = Some(link)))
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
        let stations: Vec<(BuildingId, Vec2)> = self.stations.iter().copied().filter_map(|s| self.building_entrance(s).map(|p| (s, p))).collect();
        if stations.len() < 2 {
            return None;
        }
        let near = |p: Vec2| stations.iter().map(|(id, q)| (*id, *q, q.distance(p))).filter(|(_, _, d)| *d <= rail::RAIL_REACH).min_by(|a, b| a.2.total_cmp(&b.2));
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
        let exit_pos = self.leave_building(id);
        let start = exit_pos.unwrap_or(self.citizens[id].pos);
        let mut path = match self.walk.route(start, stop) {
            Some((pts, nodes)) => {
                if count {
                    self.note_footfall(id, &nodes);
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
        c.transit = Some(TransitTrip { leg, target, purpose, since: tick });
        c.activity = Activity::Traveling { to: Some(target), purpose: Purpose::ToStop, path, idx: 0 };
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
        let waiting: usize = facilities.iter().filter_map(|f| self.buildings.get(*f)).map(|b| b.rail_inbound.len()).sum();
        let (cars, speed) = match kind {
            TrainKind::Passenger => (4, rail::PASSENGER_SPEED),
            TrainKind::Freight => ((5 + waiting / 3).min(10) as u8, rail::FREIGHT_SPEED),
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
        self.rail.trains.insert(Train { kind, cars, nodes, poly, s: 0.0, prev_s: 0.0, speed, stops, next_stop: 0, dwell: 0.0, passengers: Vec::new(), cargo: Vec::new(), color, held: false, branch });
        true
    }

    /// Dispatch on the headway, move every train, call at stops.
    pub(super) fn step_rail(&mut self) {
        let tick = self.clock.tick;
        let minute = self.clock.minute_of_day() as u32;
        let seconds = self.config.traffic_seconds_per_tick;
        let queued = self.rail.trains.values().any(|t| t.held && t.branch.is_some_and(|(a, _)| t.s <= a + 0.5));
        let room = self.rail.trains.len() < rail::MAX_TRAINS && !queued;
        if room && Rail::in_service(minute) && !self.stations.is_empty() && tick.saturating_sub(self.rail.last_passenger_tick) >= rail::PASSENGER_HEADWAY {
            let stations = self.stations.clone();
            if self.spawn_train(TrainKind::Passenger, &stations) {
                self.rail.stats.passenger_trains_today += 1;
            }
            self.rail.last_passenger_tick = tick;
        }
        let freight_waiting = self.yards.iter().filter_map(|y| self.buildings.get(*y)).any(|b| !b.rail_inbound.is_empty() || b.rail_outbound > 0.0);
        let since_freight = tick.saturating_sub(self.rail.last_freight_tick);
        if room && !self.yards.is_empty() && since_freight >= rail::FREIGHT_HEADWAY && (freight_waiting || since_freight >= rail::FREIGHT_HEADWAY * 2) {
            let yards = self.yards.clone();
            if self.spawn_train(TrainKind::Freight, &yards) {
                self.rail.stats.freight_trains_today += 1;
            }
            self.rail.last_freight_tick = tick;
        }
        let ids: Vec<TrainId> = self.rail.trains.keys().collect();
        for id in ids {
            let blocked = self.rail.blocked(id);
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
                t.s = (t.s + t.speed * seconds).min(limit);
                if t.s >= target - 0.01 {
                    if t.next_stop < t.stops.len() { Some(true) } else { Some(false) }
                } else {
                    None
                }
            };
            match arrived {
                Some(true) => self.train_arrived(id),
                Some(false) => {
                    let t = self.rail.trains.remove(id).unwrap();
                    let pos = t.head();
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
        self.rail.stats.next_passenger_in = if self.stations.is_empty() { 0 } else { rail::PASSENGER_HEADWAY.saturating_sub(tick.saturating_sub(self.rail.last_passenger_tick)) as u32 };
    }

    /// A train stands at a station or yard: people off and on, goods off.
    pub(super) fn train_arrived(&mut self, id: TrainId) {
        let tick = self.clock.tick;
        let (at, kind) = {
            let t = &mut self.rail.trains[id];
            t.dwell = rail::DWELL_SECONDS;
            (t.stops[t.next_stop].0, t.kind)
        };
        let _ = tick;
        match kind {
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
                let boarding = self.board_waiting(|leg| matches!(leg, TransitLeg::Rail { board, .. } if *board == at), rail::TRAIN_CAPACITY.saturating_sub(keep.len()), rail::RAIL_FARE, Flow::RailFares, &format!("Took the train from {name}."));
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
            }
            TrainKind::Freight => {
                let Some(b) = self.buildings.get_mut(at) else { return };
                let queued = std::mem::take(&mut b.rail_inbound);
                b.rail_outbound = 0.0;
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
    /// and remembers `memory`. The caller seats them in its vehicle.
    pub(super) fn board_waiting(&mut self, pred: impl Fn(&TransitLeg) -> bool, capacity: usize, fare: f32, flow: Flow, memory: &str) -> Vec<CitizenId> {
        let tick = self.clock.tick;
        let boarding: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| matches!(c.activity, Activity::Waiting { .. }) && c.transit.as_ref().is_some_and(|t| pred(&t.leg))).map(|(id, _)| id).take(capacity).collect();
        for &p in &boarding {
            let c = &mut self.citizens[p];
            c.savings -= fare;
            c.remember(tick, memory.to_string());
            self.economy.credit(flow, fare);
        }
        boarding
    }
}
