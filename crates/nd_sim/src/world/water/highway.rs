//! The highway: laying it, links to it, and the through traffic on it.

use super::*;

impl World {
    /// Lay the through highway: two three-lane carriageways across the map
    /// at `HIGHWAY_Y`, bridging whatever water lies across them. Traffic
    /// drives on the right, so the eastbound carriageway is the north one.
    pub fn lay_highway(&mut self) -> [GestureId; 2] {
        let had_project = self.project.take();
        // Edge to edge of the map; built, both ends run on out of sight
        // (`landscape::reach_elsewhere`): traffic comes from elsewhere.
        let x = crate::landscape::MAP_EDGE;
        let east = self.add_gesture(Gesture::Road { points: vec![Vec2::new(-x, HIGHWAY_Y + HIGHWAY_HALF_GAP), Vec2::new(x, HIGHWAY_Y + HIGHWAY_HALF_GAP)], lanes_fwd: HIGHWAY_LANES, lanes_bwd: 0, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
        let west = self.add_gesture(Gesture::Road { points: vec![Vec2::new(x, HIGHWAY_Y - HIGHWAY_HALF_GAP), Vec2::new(-x, HIGHWAY_Y - HIGHWAY_HALF_GAP)], lanes_fwd: HIGHWAY_LANES, lanes_bwd: 0, curved: false, kind: RoadKind::Street, elevation: Vec::new() });
        self.implement();
        self.project = had_project;
        [east, west]
    }

    /// Join the street end at `from` (south of the highway) to the highway:
    /// a street north to a junction short of it, an off-ramp from the
    /// westbound carriageway and an on-ramp back onto it, built at once.
    /// False when there is no highway there (nothing is laid then). Scripts
    /// and tests use it; the player draws their own ramps.
    pub fn link_to_highway(&mut self, from: Vec2) -> bool {
        let Some(gestures) = self.highway_link(from) else { return false };
        let had_project = self.project.take();
        for g in gestures {
            self.add_gesture(g);
        }
        self.implement();
        self.project = had_project;
        true
    }

    /// The gestures `link_to_highway` lays for a street end at `from`, for
    /// a caller that adds them to a project of its own; None when there is
    /// no highway there or `from` is too close to it.
    pub fn highway_link(&self, from: Vec2) -> Option<Vec<Gesture>> {
        let y = HIGHWAY_Y - HIGHWAY_HALF_GAP;
        let x = from.x;
        let there = self.roads.alive_segments().any(|(id, s)| {
            let (a, b) = self.roads.segment_points(id);
            s.is_highway() && crate::geom::project_on_segment(Vec2::new(x, y), a, b).1.distance(Vec2::new(x, y)) < 1.0
        });
        if !there || from.y > y - 60.0 {
            return None;
        }
        let j = Vec2::new(x, y - 50.0);
        let street = |points: Vec<Vec2>, fwd: u8, bwd: u8| Gesture::Road { points, lanes_fwd: fwd, lanes_bwd: bwd, curved: false, kind: RoadKind::Street, elevation: Vec::new() };
        // Westbound traffic comes from the east: off on that side, back on
        // on the other.
        Some(vec![
            street(vec![from, j], 1, 1),
            street(vec![Vec2::new(x + 220.0, y), Vec2::new(x + 70.0, y - 30.0), j], 1, 0),
            street(vec![j, Vec2::new(x - 70.0, y - 30.0), Vec2::new(x - 220.0, y)], 1, 0),
        ])
    }

    /// Ends of highway carriageways out of sight beyond the map: where
    /// traffic comes in from elsewhere (`entries`) and leaves (`exits`),
    /// with the end segment. A highway stub ending on the map is neither.
    pub fn highway_ends(&self) -> (Vec<(NodeId, SegmentId)>, Vec<(NodeId, SegmentId)>) {
        let mut entries = Vec::new();
        let mut exits = Vec::new();
        for n in self.roads.leaf_nodes() {
            if !crate::landscape::is_elsewhere(self.roads.node(n).pos) {
                continue;
            }
            let Some(&sid) = self.roads.node(n).segments.iter().find(|&&s| self.roads.segment(s).alive) else { continue };
            let seg = self.roads.segment(sid);
            if !seg.is_highway() {
                continue;
            }
            // A one-way leaving the node is an entry, arriving an exit.
            let leaves = (seg.a == n && seg.lanes_fwd > 0) || (seg.b == n && seg.lanes_bwd > 0);
            if leaves {
                entries.push((n, sid));
            } else {
                exits.push((n, sid));
            }
        }
        (entries, exits)
    }

    /// Is this car passing through from elsewhere to elsewhere? Only cars
    /// made as through traffic carry `Errand::Through`: a test on "no
    /// driver, no owner, no cargo" also caught fire engines, tow and garbage
    /// trucks and takeover outsiders, which then ate the through-traffic cap
    /// and were re-spawned as through cars on a rebuild while their own
    /// system recovered them too (a duplicate).
    pub fn is_through_car(car: &crate::traffic::Car) -> bool {
        car.errand == crate::vehicle::Errand::Through && car.cargo.is_none() && car.bus.is_none()
    }

    pub fn through_cars(&self) -> usize {
        self.traffic.cars.values().filter(|c| Self::is_through_car(c)).count()
    }

    /// Traffic from elsewhere: cars join the highway at a map edge and
    /// drive to the far end, whatever the city does.
    pub(in crate::world) fn step_through_traffic(&mut self) {
        let (entries, exits) = self.highway_ends();
        if entries.is_empty() || exits.is_empty() {
            return;
        }
        let mut live = self.through_cars();
        for (n, sid) in entries {
            if live >= MAX_THROUGH_CARS || !self.rng.random_bool(THROUGH_CHANCE) {
                continue;
            }
            let start = self.roads.node(n).pos;
            // The far end: the exit farthest from where it came in.
            let Some(&(en, esid)) = exits.iter().max_by(|a, b| self.roads.node(a.0).pos.distance(start).total_cmp(&self.roads.node(b.0).pos.distance(start))) else { continue };
            let end = self.roads.node(en).pos;
            let Some((start_lane, s0, _)) = self.traffic.net.nearest_lane_on(sid, start) else { continue };
            // Spread over the lanes.
            let lanes: Vec<crate::traffic::LaneId> = self.traffic.net.segment_lanes.get(&sid).cloned().unwrap_or_default();
            let start_lane = lanes.choose(&mut self.rng).copied().unwrap_or(start_lane);
            if self.traffic.net.lanes[start_lane as usize].cars.first().is_some_and(|&c| self.traffic.cars[c].s < 12.0) {
                continue;
            }
            let Some((goal, s1, _)) = self.traffic.net.nearest_lane_on(esid, end) else { continue };
            let s0 = s0.min(1.0);
            let Some((route, s0, s1)) = self.traffic.net.route_lanes(start_lane, s0, goal, s1) else { continue };
            let color = self.rng.random_range(0..8);
            let id = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, color);
            self.traffic.cars[id].model = crate::vehicle::VehicleModel::through(&mut crate::vehicle::model_rng(self.clock.tick, slotmap::Key::data(&id).as_ffi()));
            self.traffic.cars[id].errand = crate::vehicle::Errand::Through;
            live += 1;
        }
    }

    /// Put a through car back on the highway after the roads were rebuilt
    /// (every car is taken off then): on the lane under it, at the same
    /// place and speed, bound for the far exit. Nothing vanishes on the
    /// highway because the player built a ramp. False when it is not on a
    /// highway any more or cannot get out from there.
    pub(in crate::world) fn resume_through_car(&mut self, car: &crate::traffic::Car) -> bool {
        let Some((sid, _, q)) = self.roads.nearest_where(car.pos, |s| s.is_highway()) else { return false };
        if q.distance(car.pos) > 8.0 {
            return false;
        }
        let Some((lane, s, _)) = self.traffic.net.nearest_lane_on(sid, car.pos) else { return false };
        let (_, mut exits) = self.highway_ends();
        exits.sort_by(|a, b| self.roads.node(b.0).pos.distance(car.pos).total_cmp(&self.roads.node(a.0).pos.distance(car.pos)));
        for (en, esid) in exits {
            let end = self.roads.node(en).pos;
            let Some((goal, s1, _)) = self.traffic.net.nearest_lane_on(esid, end) else { continue };
            // On the last piece already: it drives on in its own lane.
            let (goal, s1) = if esid == sid { (lane, self.traffic.net.lanes[lane as usize].project(end).0) } else { (goal, s1) };
            // Already on the last lane at its very end: it leaves from there.
            let s = if goal == lane { s.min(s1 - 0.5).max(0.0) } else { s };
            if let Some((route, s0, s1)) = self.traffic.net.route_lanes(lane, s, goal, s1) {
                let id = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, car.color);
                let c = &mut self.traffic.cars[id];
                c.model = car.model;
                c.errand = crate::vehicle::Errand::Through;
                c.v = car.v;
                c.prev_pos = car.prev_pos;
                c.prev_heading = car.prev_heading;
                return true;
            }
        }
        false
    }

    /// Road ends people and lorries can arrive by: the far ends of ways to
    /// elsewhere (out of sight past the map edge) a car can drive away
    /// from, on a network that reaches the city's lots. Before any lot
    /// exists, every such end. A dead end on the map is never one: nothing
    /// appears or vanishes where the player can see it.
    pub(in crate::world) fn city_leaves(&self) -> Vec<NodeId> {
        let open = self.config.dead_ends_lead_elsewhere;
        let leaves: Vec<NodeId> = self.roads.leaf_nodes().into_iter().filter(|&n| open || crate::landscape::is_elsewhere(self.roads.node(n).pos)).collect();
        let comps: std::collections::HashSet<u32> = self.lots.values().filter(|l| self.zones.contains_key(l.zone)).filter_map(|l| self.road_component(l.access_node)).collect();
        let drivable = |n: NodeId| {
            self.roads.node(n).segments.iter().filter(|&&s| self.roads.segment(s).alive).all(|&s| {
                let seg = self.roads.segment(s);
                !seg.is_street() || (seg.a == n && seg.lanes_fwd > 0) || (seg.b == n && seg.lanes_bwd > 0) || seg.lanes() == 0
            })
        };
        let keep: Vec<NodeId> = leaves.iter().copied().filter(|&n| drivable(n) && (comps.is_empty() || self.road_component(n).is_some_and(|c| comps.contains(&c)))).collect();
        if keep.is_empty() && open { leaves } else { keep }
    }

    /// Can newcomers get in: a road from elsewhere (the highway, a road
    /// off the map edge) reaching the city's lots, or a working station?
    pub fn has_way_in(&self) -> bool {
        if !self.stations.is_empty() {
            return true;
        }
        let comps: std::collections::HashSet<u32> = self.lots.values().filter(|l| self.zones.contains_key(l.zone)).filter_map(|l| self.road_component(l.access_node)).collect();
        !comps.is_empty() && !self.city_leaves().is_empty()
    }

    /// [dock-access] The road network a dock or station joins: the road
    /// components that hold zoned lots (a city at the other end) or reach
    /// off the map (the highway, a road out to elsewhere). A street in an
    /// empty field is neither.
    /// [sweep-sim] `.0` over streets and paths (walkers), `.1` over
    /// streets alone (drivers).
    pub(in crate::world) fn network_components(&self) -> (std::collections::HashSet<u32>, std::collections::HashSet<u32>) {
        let lots: Vec<NodeId> = self.lots.values().filter(|l| self.zones.contains_key(l.zone) && l.zone != ZoneId::default()).map(|l| l.access_node).collect();
        let outs: Vec<NodeId> = self.roads.leaf_nodes().into_iter().filter(|&n| crate::landscape::is_elsewhere(self.roads.node(n).pos)).collect();
        let walk = lots.iter().chain(&outs).filter_map(|&n| self.road_component(n)).collect();
        let drive = lots.iter().chain(&outs).filter_map(|&n| self.drive_component(n)).collect();
        (walk, drive)
    }
}
