//! Vehicle routes: driving off from a spot, out of town or in from the map
//! edge, and retiring a vehicle (shared by every system with vehicles).

use super::*;

impl World {
    /// Where a parked car joins the road: its street and the point beside it.
    pub(in crate::world) fn spot_exit(&self, spot: &ParkSpot) -> Option<(SegmentId, Vec2)> {
        if spot.kind.on_street() {
            if let Some(s) = spot.segment.filter(|s| self.kerb_allowed(*s)) {
                return Some((s, spot.pos));
            }
        } else if let Some(stop) = spot.building.and_then(|b| self.truck_stop(b)) {
            return Some(stop);
        }
        self.roads.nearest_street(spot.pos).map(|(s, _, q)| (s, q))
    }

    /// Drive a parked car to `to` on `to_seg`: it pulls out of its space
    /// into the first lane. `None` (the car stays put) without a route.
    pub(in crate::world) fn drive_from_spot(&mut self, vid: VehicleId, to_seg: SegmentId, to_pt: Vec2, driver: CitizenId, errand: Errand) -> Option<CarId> {
        let spot = *self.vehicles.cars.get(vid)?.spot()?;
        let (fseg, fpt) = self.spot_exit(&spot)?;
        let (route, s0, s1) = self.route_any_lane(fseg, fpt, to_seg, to_pt)?;
        self.launch(vid, spot, route, s0, s1, driver, errand)
    }

    /// [parking] A route from a point beside a street: from the nearest
    /// lane, else from any lane of the street (pulling out the other way:
    /// the goal may lie behind on the near lane with no turning place).
    pub(in crate::world) fn route_any_lane(&self, fseg: SegmentId, fpt: Vec2, to_seg: SegmentId, to_pt: Vec2) -> Option<(Vec<LaneId>, f32, f32)> {
        if let Some(r) = self.traffic.net.find_route(fseg, fpt, to_seg, to_pt) {
            return Some(r);
        }
        let (goal, s_end, _) = self.traffic.net.nearest_lane_on(to_seg, to_pt)?;
        let lanes = self.traffic.net.segment_lanes.get(&fseg)?.clone();
        lanes.into_iter().find_map(|l| {
            let (s, _) = self.traffic.net.lanes[l as usize].project(fpt);
            self.traffic.net.route_lanes(l, s, goal, s_end)
        })
    }

    /// Drive a parked car off the map (sold out of town, emigrating).
    pub(in crate::world) fn drive_out_of_town(&mut self, vid: VehicleId, errand: Errand) -> Option<CarId> {
        let spot = *self.vehicles.cars.get(vid)?.spot()?;
        let (fseg, fpt) = self.spot_exit(&spot)?;
        let (start, s0, _) = self.traffic.net.nearest_lane_on(fseg, fpt)?;
        let (route, s0, s1) = self.edge_exits(spot.pos).into_iter().find_map(|(goal, s_end)| self.traffic.net.route_lanes(start, s0, goal, s_end))?;
        self.launch(vid, spot, route, s0, s1, CitizenId::default(), errand)
    }

    pub(in crate::world) fn launch(&mut self, vid: VehicleId, spot: ParkSpot, route: Vec<LaneId>, s0: f32, s1: f32, driver: CitizenId, errand: Errand) -> Option<CarId> {
        // [boot] Clamped: it stays where it is until the tow truck comes.
        if spot.booted {
            return None;
        }
        self.unpark(vid);
        let (model, color) = {
            let c = &self.vehicles.cars[vid];
            (c.model, c.color)
        };
        let car = self.traffic.spawn_car(driver, None, route, s0, s1, color);
        let c = &mut self.traffic.cars[car];
        c.model = model;
        c.owned = Some(vid);
        c.errand = errand;
        // Drawn pulling out from the kerb or the driveway, not appearing in
        // the lane.
        let kerb_lane = self.traffic.net.lanes.get(c.route[0] as usize).is_some_and(|l| l.right.is_none());
        let off = if spot.kind.on_street() { spot.pos.distance(c.pos).min(2.6) } else if kerb_lane { 2.0 } else { 0.0 };
        c.lateral = c.lateral.max(off);
        self.vehicles.cars[vid].place = CarPlace::Driving(car);
        Some(car)
    }

    /// Exits off the map for a car near `from`, nearest first: highway
    /// carriageways leaving the map, then road ends at the city's edge.
    /// Each is a lane and where on it the car is gone.
    pub(in crate::world) fn edge_exits(&self, from: Vec2) -> Vec<(LaneId, f32)> {
        let (_, mut ends) = self.highway_ends();
        let near = |a: &(NodeId, SegmentId), b: &(NodeId, SegmentId)| self.roads.node(a.0).pos.distance(from).total_cmp(&self.roads.node(b.0).pos.distance(from));
        ends.sort_by(near);
        ends.truncate(3);
        let mut leaves: Vec<(NodeId, SegmentId)> = self
            .city_leaves()
            .into_iter()
            .filter_map(|n| self.roads.node(n).segments.iter().copied().find(|&s| self.roads.segment(s).alive && self.roads.segment(s).is_street()).map(|s| (n, s)))
            .collect();
        let max = leaves.iter().map(|(n, _)| self.roads.node(*n).pos.length()).fold(0.0, f32::max);
        leaves.retain(|(n, _)| self.roads.node(*n).pos.length() >= max * 0.6);
        leaves.sort_by(near);
        ends.extend(leaves.into_iter().take(3));
        ends.into_iter()
            .filter_map(|(n, sid)| {
                let at = self.roads.node(n).pos;
                let lanes = self.traffic.net.segment_lanes.get(&sid)?;
                let lane = *lanes.iter().min_by(|a, b| self.traffic.net.lanes[**a as usize].end().distance(at).total_cmp(&self.traffic.net.lanes[**b as usize].end().distance(at)))?;
                Some((lane, (self.traffic.net.lanes[lane as usize].length - 1.0).max(0.0)))
            })
            .collect()
    }

    /// [services] `edge_exits`, then every other road end leading off the
    /// map (the nearer ends `edge_exits` leaves to walkers), nearest first:
    /// for a tow that must get away from wherever a wreck lies.
    pub(in crate::world) fn any_exits(&self, from: Vec2) -> Vec<(LaneId, f32)> {
        let mut out = self.edge_exits(from);
        let mut leaves: Vec<NodeId> = self.city_leaves();
        leaves.sort_by(|a, b| self.roads.node(*a).pos.distance(from).total_cmp(&self.roads.node(*b).pos.distance(from)).then(a.0.cmp(&b.0)));
        for n in leaves {
            let at = self.roads.node(n).pos;
            let Some(sid) = self.roads.node(n).segments.iter().copied().find(|&s| self.roads.segment(s).alive && self.roads.segment(s).is_street()) else { continue };
            let Some(lanes) = self.traffic.net.segment_lanes.get(&sid) else { continue };
            let Some(&lane) = lanes.iter().min_by(|a, b| self.traffic.net.lanes[**a as usize].end().distance(at).total_cmp(&self.traffic.net.lanes[**b as usize].end().distance(at))) else { continue };
            if !out.iter().any(|&(l, _)| l == lane) {
                out.push((lane, (self.traffic.net.lanes[lane as usize].length - 1.0).max(0.0)));
            }
        }
        out
    }

    /// Send a car on the road off the map by the first exit it can reach.
    pub(in crate::world) fn redirect_out(&mut self, car: CarId) -> bool {
        let pos = self.traffic.cars[car].pos;
        self.edge_exits(pos).into_iter().any(|(goal, s_end)| self.traffic.redirect(car, goal, s_end))
    }

    /// [services] A city vehicle with nowhere left to go in town (its
    /// station, office, depot or line is gone, or no road leads back to
    /// it) drives off the map by the nearest exit it can reach, as
    /// `Errand::Out`, and leaves the world there (`out_arrived`). With no
    /// road out from where it stands it pulls in and tries again in a
    /// while. Vehicles only ever leave the world at the map edge or into a
    /// parking place or base: deleting one mid-street made fire engines,
    /// buses and tow trucks blink out in front of the player.
    pub(in crate::world) fn retire_vehicle(&mut self, car: CarId) {
        let Some(c) = self.traffic.cars.get_mut(car) else { return };
        c.siren = false;
        c.errand = Errand::Out;
        if !self.redirect_out(car) {
            // [r4-flags-sim] No road out of the pocket it stands in.
            self.rescue_trapped(car);
            self.traffic.pull_in(car, RETIRE_RETRY_SECONDS);
            if let Some(c) = self.traffic.cars.get_mut(car) {
                c.stranded = true;
            }
        }
    }

    /// [services] An `Errand::Out` vehicle at the end of its route: gone
    /// at the map edge, else (a vehicle that could not route out and held)
    /// it tries again from here.
    pub(in crate::world) fn out_arrived(&mut self, car: CarId) {
        if self.at_map_edge(car) {
            self.traffic.remove_car(car);
        } else {
            self.retire_vehicle(car);
        }
    }

    /// [services] Has `car` stopped where a route to `pt` on `seg` ends (the
    /// lane `nearest_lane_on` gives for it, at that place), or within
    /// `reach` metres of `pt`? Asked before a vehicle is taken off the road
    /// "at" a base, so one re-aimed elsewhere drives on instead.
    pub(in crate::world) fn car_at_stop(&self, car: CarId, seg: SegmentId, pt: Vec2, reach: f32) -> bool {
        let Some(c) = self.traffic.cars.get(car) else { return true };
        if c.pos.distance(pt) < reach {
            return true;
        }
        self.traffic.net.nearest_lane_on(seg, pt).is_some_and(|(goal, s_end, _)| c.route.last() == Some(&goal) && (c.s - s_end).abs() < 3.0)
    }

    /// [services] A route off the map for a vehicle standing at `from` (a
    /// unit parked on scene, a truck at a kerb): from the street beside it
    /// to the nearest exit it can reach.
    pub(in crate::world) fn route_out_from(&self, from: Vec2) -> Option<(Vec<LaneId>, f32, f32)> {
        let (seg, _, q) = self.roads.nearest_street(from)?;
        let (start, s0, _) = self.traffic.net.nearest_lane_on(seg, q)?;
        self.edge_exits(from).into_iter().find_map(|(goal, s_end)| self.traffic.net.route_lanes(start, s0, goal, s_end))
    }

    /// A route onto the map from out of town to `to` on `to_seg`: in on a
    /// highway carriageway entering the map when one leads there, else
    /// along a road from the city's edge.
    pub(in crate::world) fn route_from_edge(&mut self, to_seg: SegmentId, to_pt: Vec2) -> Option<(Vec<LaneId>, f32, f32)> {
        let (goal, s_end, _) = self.traffic.net.nearest_lane_on(to_seg, to_pt)?;
        self.route_from_edge_to(goal, s_end)
    }

    /// `route_from_edge` to a given lane: `s_end` metres along `goal`.
    pub(in crate::world) fn route_from_edge_to(&mut self, goal: LaneId, s_end: f32) -> Option<(Vec<LaneId>, f32, f32)> {
        let (mut highway, _) = self.highway_ends();
        highway.shuffle(&mut self.rng);
        let mut leaves: Vec<(NodeId, SegmentId)> = self
            .city_leaves()
            .into_iter()
            .filter_map(|n| self.roads.node(n).segments.iter().copied().find(|&s| self.roads.segment(s).alive && self.roads.segment(s).is_street()).map(|s| (n, s)))
            .collect();
        // The far ends of the city's roads, as people walking in use.
        let max = leaves.iter().map(|(n, _)| self.roads.node(*n).pos.length()).fold(0.0, f32::max);
        leaves.retain(|(n, _)| self.roads.node(*n).pos.length() >= max * 0.6);
        leaves.shuffle(&mut self.rng);
        for (n, sid) in highway.into_iter().take(3).chain(leaves.into_iter().take(3)) {
            if let Some(r) = self.route_in_from(n, sid, goal, s_end) {
                return Some(r);
            }
        }
        // [services] Then any road end leading off the map, nearest first:
        // a recovery truck must get to a wreck however it can.
        let to = self.traffic.net.lanes.get(goal as usize)?.point_at(s_end).0;
        let mut all: Vec<(NodeId, SegmentId)> = self
            .city_leaves()
            .into_iter()
            .filter_map(|n| self.roads.node(n).segments.iter().copied().find(|&s| self.roads.segment(s).alive && self.roads.segment(s).is_street()).map(|s| (n, s)))
            .collect();
        all.sort_by(|a, b| self.roads.node(a.0).pos.distance(to).total_cmp(&self.roads.node(b.0).pos.distance(to)).then(a.0 .0.cmp(&b.0 .0)));
        all.into_iter().find_map(|(n, sid)| self.route_in_from(n, sid, goal, s_end))
    }

    /// A route in from the road end `n` (segment `sid`) to `s_end` on `goal`.
    pub(in crate::world) fn route_in_from(&self, n: NodeId, sid: SegmentId, goal: LaneId, s_end: f32) -> Option<(Vec<LaneId>, f32, f32)> {
        let at = self.roads.node(n).pos;
        let lanes = self.traffic.net.segment_lanes.get(&sid)?;
        let &lane = lanes.iter().min_by(|a, b| self.traffic.net.lanes[**a as usize].start().distance(at).total_cmp(&self.traffic.net.lanes[**b as usize].start().distance(at)))?;
        let s0 = 1.0f32.min(self.traffic.net.lanes[lane as usize].length * 0.5);
        self.traffic.net.route_lanes(lane, s0, goal, s_end)
    }
}
