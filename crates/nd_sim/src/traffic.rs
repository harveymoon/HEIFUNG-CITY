//! Lane-level traffic: lanes derived from the road graph, generated
//! intersections with connector lanes and signal phases, lane-graph
//! pathfinding, and car-following (intelligent driver model) microtraffic.
//!
//! The lane network is rebuilt from the road network whenever roads change
//! and is never serialised; cars are.

use crate::buildings::BuildingId;
use crate::citizens::CitizenId;
use crate::market::Resource;
use crate::roads::{project_on_segment, NodeId, RoadNetwork, SegmentId, LANE_WIDTH};
use glam::Vec2;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

new_key_type! {
    pub struct CarId;
}

pub type LaneId = u32;

pub const CAR_LENGTH: f32 = 4.5;
const ROAD_SPEED: f32 = 13.0;
const CONNECTOR_SPEED: f32 = 6.0;
const IDM_ACCEL: f32 = 1.4;
const IDM_BRAKE: f32 = 2.0;
const IDM_MIN_GAP: f32 = 2.0;
const IDM_HEADWAY: f32 = 1.2;
const GREEN_SECONDS: f32 = 14.0;
const CLEARANCE_SECONDS: f32 = 3.0;
const LOOKAHEAD: f32 = 90.0;
/// Trips older than this (traffic seconds) end where the car stands.
const TRIP_TIMEOUT: f32 = 1800.0;

#[derive(Clone, Debug)]
pub enum LaneKind {
    Road { segment: SegmentId, forward: bool, index: u8 },
    Connector { node: NodeId, approach: usize },
}

#[derive(Clone, Debug)]
pub struct Lane {
    pub points: Vec<Vec2>,
    cum: Vec<f32>,
    pub length: f32,
    pub next: Vec<LaneId>,
    pub kind: LaneKind,
    /// Cars on this lane ordered by increasing position.
    pub cars: Vec<CarId>,
    pub green: bool,
    pub speed_limit: f32,
    pub end_node: NodeId,
}

impl Lane {
    fn new(points: Vec<Vec2>, kind: LaneKind, end_node: NodeId, speed_limit: f32) -> Self {
        let mut cum = vec![0.0];
        for w in points.windows(2) {
            cum.push(cum.last().unwrap() + w[0].distance(w[1]));
        }
        let length = *cum.last().unwrap();
        Self { points, cum, length, next: Vec::new(), kind, cars: Vec::new(), green: true, speed_limit, end_node }
    }

    pub fn is_connector(&self) -> bool {
        matches!(self.kind, LaneKind::Connector { .. })
    }

    /// Position and unit direction at distance `s` along the lane.
    pub fn point_at(&self, s: f32) -> (Vec2, Vec2) {
        if self.points.len() < 2 {
            return (self.points.first().copied().unwrap_or(Vec2::ZERO), Vec2::X);
        }
        let s = s.clamp(0.0, self.length);
        let mut i = 0;
        while i + 2 < self.points.len() && self.cum[i + 1] < s {
            i += 1;
        }
        let seg_len = (self.cum[i + 1] - self.cum[i]).max(1e-4);
        let t = ((s - self.cum[i]) / seg_len).clamp(0.0, 1.0);
        let dir = (self.points[i + 1] - self.points[i]).normalize_or_zero();
        (self.points[i].lerp(self.points[i + 1], t), if dir == Vec2::ZERO { Vec2::X } else { dir })
    }

    /// Closest point on the lane to `p`: `(distance along, distance from)`.
    pub fn project(&self, p: Vec2) -> (f32, f32) {
        let mut best = (0.0, f32::MAX);
        for (i, w) in self.points.windows(2).enumerate() {
            let (t, q) = project_on_segment(p, w[0], w[1]);
            let d = p.distance(q);
            if d < best.1 {
                best = (self.cum[i] + t * (self.cum[i + 1] - self.cum[i]), d);
            }
        }
        best
    }

    pub fn start(&self) -> Vec2 {
        self.points[0]
    }

    pub fn end(&self) -> Vec2 {
        *self.points.last().unwrap()
    }

    fn start_dir(&self) -> Vec2 {
        (self.points[1] - self.points[0]).normalize_or_zero()
    }

    fn end_dir(&self) -> Vec2 {
        let n = self.points.len();
        (self.points[n - 1] - self.points[n - 2]).normalize_or_zero()
    }
}

#[derive(Clone, Debug)]
pub struct Intersection {
    pub node: NodeId,
    pub radius: f32,
    pub approaches: usize,
    pub phase: usize,
    pub timer: f32,
    pub connectors: Vec<(LaneId, usize)>,
    pub signalized: bool,
}

#[derive(Clone, Debug, Default)]
pub struct LaneNetwork {
    pub lanes: Vec<Lane>,
    pub intersections: Vec<Intersection>,
    pub segment_lanes: HashMap<SegmentId, Vec<LaneId>>,
    pub node_radius: Vec<f32>,
    /// Index into `intersections` per node, `usize::MAX` for none.
    pub node_intersection: Vec<usize>,
    pub version: u64,
}

/// Freight carried by a truck between two businesses.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Cargo {
    pub from: BuildingId,
    pub to: BuildingId,
    pub resource: Resource,
    pub amount: f32,
    /// Money the buyer owes on delivery.
    pub price: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Car {
    /// Driver for private cars; a null key for trucks.
    pub citizen: CitizenId,
    pub cargo: Option<Cargo>,
    pub route: Vec<LaneId>,
    pub idx: usize,
    pub s: f32,
    pub s_end: f32,
    pub v: f32,
    pub pos: Vec2,
    pub heading: f32,
    pub prev_pos: Vec2,
    pub prev_heading: f32,
    pub color: u8,
    /// Seconds spent on the trip; stalled trips time out.
    #[serde(default)]
    pub age: f32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Traffic {
    #[serde(skip)]
    pub net: LaneNetwork,
    pub cars: SlotMap<CarId, Car>,
}

fn quad_bezier(p0: Vec2, c: Vec2, p1: Vec2, samples: usize) -> Vec<Vec2> {
    (0..=samples)
        .map(|i| {
            let t = i as f32 / samples as f32;
            let u = 1.0 - t;
            p0 * (u * u) + c * (2.0 * u * t) + p1 * (t * t)
        })
        .collect()
}

/// Curve joining the end of one lane to the start of another inside an
/// intersection. Uses the intersection of the two lane directions as the
/// bezier control point when it is sensible, so through-connectors stay
/// straight and turns bend.
fn connector_curve(p_in: Vec2, d_in: Vec2, p_out: Vec2, d_out: Vec2, radius: f32) -> Vec<Vec2> {
    if p_in.distance(p_out) < 0.5 {
        return vec![p_in, p_out];
    }
    let denom = d_in.perp_dot(d_out);
    let mut control = (p_in + p_out) * 0.5;
    if denom.abs() > 1e-3 {
        let qp = p_out - p_in;
        let t = qp.perp_dot(d_out) / denom;
        let u = qp.perp_dot(d_in) / denom;
        // Keep the control point inside the junction so turns never swing
        // out over the kerb, even at shallow angles.
        let limit = radius * 1.3 + 1.0;
        if t > 0.0 && u > 0.0 {
            let t = t.min(limit);
            control = p_in + d_in * t;
        }
    }
    let straight = d_in.dot(d_out) > 0.98;
    quad_bezier(p_in, control, p_out, if straight { 1 } else { 6 })
}

impl LaneNetwork {
    pub fn rebuild(&mut self, roads: &RoadNetwork) {
        self.lanes.clear();
        self.intersections.clear();
        self.segment_lanes.clear();
        self.node_intersection = vec![usize::MAX; roads.nodes.len()];
        self.version += 1;

        // Intersection radius per node: enough to fit the widest road.
        self.node_radius = roads
            .nodes
            .iter()
            .map(|n| {
                let alive: Vec<_> = n.segments.iter().filter(|&&s| roads.segment(s).alive && roads.segment(s).is_street()).collect();
                let w = alive.iter().map(|&&s| roads.segment(s).half_width()).fold(0.0, f32::max);
                match alive.len() {
                    0 => 0.0,
                    1 => w * 0.8,
                    2 => {
                        // A bend: the sharper it is, the more room the turn needs.
                        let (sa, sb) = (*alive[0], *alive[1]);
                        let (a0, a1) = roads.segment_points(sa);
                        let (b0, b1) = roads.segment_points(sb);
                        let here = n.pos;
                        let da = (if a0.distance(here) < a1.distance(here) { a1 - a0 } else { a0 - a1 }).normalize_or_zero();
                        let db = (if b0.distance(here) < b1.distance(here) { b1 - b0 } else { b0 - b1 }).normalize_or_zero();
                        // da and db both point away from the node; straight through means da . db = -1.
                        let bend = (1.0 + da.dot(db)).clamp(0.0, 2.0) * 0.5; // 0 straight, 1 hairpin
                        (0.6 + (w + 1.0) * bend).min(w + 1.0)
                    }
                    _ => w + 1.0,
                }
            })
            .collect();

        for (sid, seg) in roads.alive_segments() {
            if !seg.is_street() {
                continue;
            }
            let (pa, pb) = roads.segment_points(sid);
            let len = pa.distance(pb);
            if len < 0.5 {
                continue;
            }
            let dir = (pb - pa) / len;
            let mut ra = self.node_radius[seg.a.0 as usize];
            let mut rb = self.node_radius[seg.b.0 as usize];
            if len - ra - rb < 2.0 {
                let k = ((len - 2.0) / (ra + rb).max(1e-3)).clamp(0.0, 1.0);
                ra *= k;
                rb *= k;
            }
            let p0 = pa + dir * ra;
            let p1 = pb - dir * rb;
            // Right-hand traffic. The sim plane maps to world XZ with y up, so the
            // right of a car heading `dir` is (-dir.y, dir.x). The whole lane set is
            // centred on the segment line: backward lanes on the left half, forward
            // lanes on the right half, boundary k at (k - total/2) lane widths.
            let right = Vec2::new(-dir.y, dir.x);
            let total = seg.lanes() as f32;
            let left_edge = -total * 0.5 * LANE_WIDTH;
            let mut ids = Vec::new();
            for i in 0..seg.lanes_fwd {
                let off = right * (left_edge + (seg.lanes_bwd as f32 + i as f32 + 0.5) * LANE_WIDTH);
                let id = self.lanes.len() as LaneId;
                self.lanes.push(Lane::new(vec![p0 + off, p1 + off], LaneKind::Road { segment: sid, forward: true, index: i }, seg.b, ROAD_SPEED));
                ids.push(id);
            }
            for j in 0..seg.lanes_bwd {
                let off = right * (left_edge + (seg.lanes_bwd as f32 - j as f32 - 0.5) * LANE_WIDTH);
                let id = self.lanes.len() as LaneId;
                self.lanes.push(Lane::new(vec![p1 + off, p0 + off], LaneKind::Road { segment: sid, forward: false, index: j }, seg.a, ROAD_SPEED));
                ids.push(id);
            }
            self.segment_lanes.insert(sid, ids);
        }

        let road_lane_count = self.lanes.len();
        for (ni, node) in roads.nodes.iter().enumerate() {
            let nid = NodeId(ni as u32);
            let approaches: Vec<SegmentId> = node.segments.iter().copied().filter(|&s| roads.segment(s).alive && roads.segment(s).is_street()).collect();
            if approaches.is_empty() {
                continue;
            }
            let mut incoming: Vec<(LaneId, SegmentId)> = Vec::new();
            let mut outgoing: Vec<(LaneId, SegmentId)> = Vec::new();
            for &sid in &approaches {
                let seg = roads.segment(sid);
                for &lid in &self.segment_lanes[&sid] {
                    if let LaneKind::Road { forward, .. } = self.lanes[lid as usize].kind {
                        let ends_here = if forward { seg.b == nid } else { seg.a == nid };
                        if ends_here {
                            incoming.push((lid, sid));
                        } else {
                            outgoing.push((lid, sid));
                        }
                    }
                }
            }
            let radius = self.node_radius[ni];
            let mut connectors = Vec::new();
            for &(inc, inc_seg) in &incoming {
                let approach = approaches.iter().position(|&s| s == inc_seg).unwrap_or(0);
                for &(out, out_seg) in &outgoing {
                    if out_seg == inc_seg && approaches.len() > 1 {
                        continue; // U-turns only at dead ends
                    }
                    let (p_in, d_in) = (self.lanes[inc as usize].end(), self.lanes[inc as usize].end_dir());
                    let (p_out, d_out) = (self.lanes[out as usize].start(), self.lanes[out as usize].start_dir());
                    let pts = connector_curve(p_in, d_in, p_out, d_out, radius);
                    let id = self.lanes.len() as LaneId;
                    let speed = if approaches.len() <= 2 && d_in.dot(d_out) > 0.7 { ROAD_SPEED } else { CONNECTOR_SPEED };
                    self.lanes.push(Lane::new(pts, LaneKind::Connector { node: nid, approach }, nid, speed));
                    self.lanes[id as usize].next.push(out);
                    self.lanes[inc as usize].next.push(id);
                    connectors.push((id, approach));
                }
            }
            let two_way = approaches.iter().filter(|&&s| roads.segment(s).lanes_fwd > 0 && roads.segment(s).lanes_bwd > 0).count();
            let signalized = (approaches.len() >= 3 && two_way >= 3) || approaches.len() >= 4;
            self.node_intersection.resize(roads.nodes.len().max(self.node_intersection.len()), usize::MAX);
            self.node_intersection[ni] = self.intersections.len();
            self.intersections.push(Intersection { node: nid, radius, approaches: approaches.len(), phase: 0, timer: 0.0, connectors, signalized });
        }
        let _ = road_lane_count;
        self.apply_signals();
    }

    fn apply_signals(&mut self) {
        for inter in &self.intersections {
            for &(lane, approach) in &inter.connectors {
                self.lanes[lane as usize].green = !inter.signalized || (approach == inter.phase && inter.timer < GREEN_SECONDS);
            }
        }
    }

    fn update_signals(&mut self, dt: f32) {
        for inter in &mut self.intersections {
            if !inter.signalized {
                continue;
            }
            inter.timer += dt;
            if inter.timer >= GREEN_SECONDS + CLEARANCE_SECONDS {
                inter.timer = 0.0;
                inter.phase = (inter.phase + 1) % inter.approaches.max(1);
            }
        }
        self.apply_signals();
    }

    /// Closest lane of a segment to a point: `(lane, s, distance)`.
    pub fn nearest_lane_on(&self, segment: SegmentId, p: Vec2) -> Option<(LaneId, f32, f32)> {
        let mut best: Option<(LaneId, f32, f32)> = None;
        for &lid in self.segment_lanes.get(&segment)? {
            let (s, d) = self.lanes[lid as usize].project(p);
            if best.map_or(true, |b| d < b.2) {
                best = Some((lid, s, d));
            }
        }
        best
    }

    /// A* over lanes from a point on one segment to a point on another.
    pub fn find_route(&self, from_seg: SegmentId, from: Vec2, to_seg: SegmentId, to: Vec2) -> Option<(Vec<LaneId>, f32, f32)> {
        let (start, s_start, _) = self.nearest_lane_on(from_seg, from)?;
        let (goal, s_end, _) = self.nearest_lane_on(to_seg, to)?;
        if start == goal && s_end >= s_start {
            return Some((vec![start], s_start, s_end));
        }
        let mut open = BinaryHeap::new();
        let mut came: HashMap<LaneId, LaneId> = HashMap::new();
        let mut g: HashMap<LaneId, f32> = HashMap::new();
        let start_cost = self.lanes[start as usize].length - s_start;
        g.insert(start, start_cost);
        open.push(Open { f: start_cost + self.lanes[start as usize].end().distance(to), lane: start, expanded: 0 });
        let mut expansions = 0usize;
        while let Some(Open { lane, expanded, .. }) = open.pop() {
            if lane == goal && expanded > 0 {
                let mut route = vec![goal];
                let mut cur = goal;
                while let Some(&prev) = came.get(&cur) {
                    route.push(prev);
                    if prev == start {
                        break;
                    }
                    cur = prev;
                }
                route.reverse();
                return Some((route, s_start, s_end));
            }
            expansions += 1;
            if expansions > 20_000 {
                return None;
            }
            let gl = g[&lane];
            for &next in &self.lanes[lane as usize].next {
                let cost = gl + self.lanes[next as usize].length;
                if next == start {
                    continue;
                }
                if g.get(&next).map_or(true, |&old| cost < old) {
                    g.insert(next, cost);
                    came.insert(next, lane);
                    open.push(Open { f: cost + self.lanes[next as usize].end().distance(to), lane: next, expanded: expanded + 1 });
                }
            }
        }
        None
    }
}

#[derive(PartialEq)]
struct Open {
    f: f32,
    lane: LaneId,
    expanded: u32,
}

impl Eq for Open {}

impl PartialOrd for Open {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Open {
    fn cmp(&self, other: &Self) -> Ordering {
        other.f.total_cmp(&self.f).then_with(|| other.lane.cmp(&self.lane))
    }
}

fn idm(v: f32, v0: f32, gap: f32, dv: f32) -> f32 {
    let gap = gap.max(0.1);
    let s_star = IDM_MIN_GAP + (v * IDM_HEADWAY + v * dv / (2.0 * (IDM_ACCEL * IDM_BRAKE).sqrt())).max(0.0);
    IDM_ACCEL * (1.0 - (v / v0).powi(4) - (s_star / gap).powi(2))
}

impl Traffic {
    pub fn rebuild(&mut self, roads: &RoadNetwork) {
        self.net.rebuild(roads);
    }

    /// Drop every car; used when the road network changes shape.
    pub fn clear_cars(&mut self) -> Vec<(CarId, Car)> {
        let out: Vec<(CarId, Car)> = self.cars.drain().collect();
        for lane in &mut self.net.lanes {
            lane.cars.clear();
        }
        out
    }

    /// After loading a save the lane network is rebuilt; put cars back on
    /// their lanes and drop any whose route no longer exists.
    pub fn reattach_cars(&mut self) -> Vec<(CarId, Car)> {
        let lane_count = self.net.lanes.len() as LaneId;
        let invalid: Vec<CarId> = self
            .cars
            .iter()
            .filter(|(_, c)| c.route.iter().any(|&l| l >= lane_count) || c.idx >= c.route.len())
            .map(|(id, _)| id)
            .collect();
        let dropped: Vec<(CarId, Car)> = invalid.into_iter().filter_map(|id| self.cars.remove(id).map(|c| (id, c))).collect();
        for lane in &mut self.net.lanes {
            lane.cars.clear();
        }
        let ids: Vec<CarId> = self.cars.keys().collect();
        for id in ids {
            let lane = self.cars[id].route[self.cars[id].idx];
            self.insert_sorted(lane, id);
        }
        dropped
    }

    fn insert_sorted(&mut self, lane: LaneId, car: CarId) {
        let s = self.cars[car].s;
        let list = &mut self.net.lanes[lane as usize].cars;
        let pos = list.iter().position(|&other| self.cars[other].s > s).unwrap_or(list.len());
        list.insert(pos, car);
    }

    pub fn spawn_car(&mut self, citizen: CitizenId, cargo: Option<Cargo>, route: Vec<LaneId>, s_start: f32, s_end: f32, color: u8) -> CarId {
        let lane = route[0];
        let (pos, dir) = self.net.lanes[lane as usize].point_at(s_start);
        let heading = dir.y.atan2(dir.x);
        let id = self.cars.insert(Car { citizen, cargo, route, idx: 0, s: s_start, s_end, v: 0.0, pos, heading, prev_pos: pos, prev_heading: heading, color, age: 0.0 });
        self.insert_sorted(lane, id);
        id
    }

    pub fn remove_car(&mut self, id: CarId) -> Option<Car> {
        let car = self.cars.remove(id)?;
        if let Some(&lane) = car.route.get(car.idx) {
            if let Some(l) = self.net.lanes.get_mut(lane as usize) {
                l.cars.retain(|&c| c != id);
            }
        }
        Some(car)
    }

    /// Gap and leader speed for a car with no leader on its own lane: look
    /// down the route for the next car or a red light.
    fn lookahead(&self, car: &Car, lane: &Lane) -> (f32, f32) {
        let mut dist = lane.length - car.s;
        let route = &car.route;
        for (k, &next) in route.iter().enumerate().skip(car.idx + 1).take(4) {
            let nl = &self.net.lanes[next as usize];
            if nl.is_connector() && !nl.green {
                // Stop line just before the lane ends; the min gap keeps the nose clear.
                return (dist - 1.0 + IDM_MIN_GAP, 0.0);
            }
            // Do not enter a junction unless the exit lane has room: this is
            // what keeps a saturated grid from locking up.
            if nl.is_connector() && !lane.is_connector() && k == car.idx + 1 {
                if let LaneKind::Connector { node, approach } = nl.kind {
                    if let Some(inter) = self.net.node_intersection.get(node.0 as usize).and_then(|&i| self.net.intersections.get(i)) {
                        if !inter.signalized && inter.approaches >= 3 {
                            let busy = inter.connectors.iter().any(|&(lid, ap)| ap != approach && !self.net.lanes[lid as usize].cars.is_empty());
                            if busy {
                                return (dist - 1.0 + IDM_MIN_GAP, 0.0);
                            }
                        }
                    }
                }
                if let Some(&exit) = route.get(k + 1) {
                    let el = &self.net.lanes[exit as usize];
                    let blocked = el.cars.first().map_or(false, |&c| self.cars[c].s < CAR_LENGTH + IDM_MIN_GAP + 1.0) && el.cars.len() as f32 * (CAR_LENGTH + IDM_MIN_GAP) > el.length - nl.length;
                    let tail_near = el.cars.first().map_or(false, |&c| self.cars[c].s < CAR_LENGTH + 0.5);
                    if blocked || (tail_near && !nl.cars.is_empty()) {
                        return (dist - 1.0 + IDM_MIN_GAP, 0.0);
                    }
                }
            }
            if let Some(&first) = nl.cars.first() {
                let lead = &self.cars[first];
                return (dist + lead.s - CAR_LENGTH, lead.v);
            }
            dist += nl.length;
            if dist > LOOKAHEAD {
                break;
            }
        }
        (LOOKAHEAD * 2.0, ROAD_SPEED)
    }

    /// Advance all cars by `seconds` in `substeps` steps. Returns cars that
    /// reached their destination (they stay in `cars` until removed).
    pub fn step(&mut self, seconds: f32, substeps: usize) -> Vec<CarId> {
        let dt = seconds / substeps.max(1) as f32;
        for car in self.cars.values_mut() {
            car.prev_pos = car.pos;
            car.prev_heading = car.heading;
            car.age += seconds;
        }
        let mut arrived = Vec::new();
        for _ in 0..substeps {
            self.net.update_signals(dt);

            let mut accs: Vec<(CarId, f32)> = Vec::with_capacity(self.cars.len());
            for lane in &self.net.lanes {
                for (k, &cid) in lane.cars.iter().enumerate() {
                    let car = &self.cars[cid];
                    let (mut gap, mut lead_v) = match lane.cars.get(k + 1) {
                        Some(&lead) => (self.cars[lead].s - car.s - CAR_LENGTH, self.cars[lead].v),
                        None => self.lookahead(car, lane),
                    };
                    if car.idx + 1 == car.route.len() {
                        let to_stop = car.s_end + IDM_MIN_GAP - car.s;
                        if to_stop < gap {
                            gap = to_stop;
                            lead_v = 0.0;
                        }
                    }
                    accs.push((cid, idm(car.v, lane.speed_limit, gap, car.v - lead_v)));
                }
            }
            for (cid, acc) in accs {
                let car = &mut self.cars[cid];
                car.v = (car.v + acc * dt).max(0.0);
                car.s += car.v * dt;
            }

            let ids: Vec<CarId> = self.cars.keys().collect();
            for cid in ids {
                let (lane_id, idx, len, s, s_end) = {
                    let c = &self.cars[cid];
                    let lane_id = c.route[c.idx];
                    (lane_id, c.idx, self.net.lanes[lane_id as usize].length, c.s, c.s_end)
                };
                if self.cars[cid].age > TRIP_TIMEOUT {
                    if !arrived.contains(&cid) {
                        arrived.push(cid);
                    }
                    continue;
                }
                if idx + 1 == self.cars[cid].route.len() {
                    if s >= s_end - 0.5 {
                        let c = &mut self.cars[cid];
                        c.s = s_end.min(len);
                        c.v = 0.0;
                        if !arrived.contains(&cid) {
                            arrived.push(cid);
                        }
                    }
                } else if s >= len {
                    self.net.lanes[lane_id as usize].cars.retain(|&c| c != cid);
                    let next = {
                        let c = &mut self.cars[cid];
                        c.s -= len;
                        c.idx += 1;
                        c.route[c.idx]
                    };
                    self.insert_sorted(next, cid);
                }
            }
        }
        for car in self.cars.values_mut() {
            let lane = &self.net.lanes[car.route[car.idx] as usize];
            let (p, d) = lane.point_at(car.s);
            car.pos = p;
            car.heading = d.y.atan2(d.x);
        }
        arrived
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cross() -> RoadNetwork {
        let mut net = RoadNetwork::default();
        net.add_polyline(&[Vec2::new(-200.0, 0.0), Vec2::new(200.0, 0.0)], 1, 1);
        net.add_polyline(&[Vec2::new(0.0, -200.0), Vec2::new(0.0, 200.0)], 1, 1);
        net
    }

    #[test]
    fn lanes_and_connectors_are_generated() {
        let roads = cross();
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let road_lanes = t.net.lanes.iter().filter(|l| !l.is_connector()).count();
        let connectors = t.net.lanes.iter().filter(|l| l.is_connector()).count();
        assert_eq!(road_lanes, 8);
        // Centre: 4 incoming lanes x 3 outgoing (no U-turn) = 12; dead ends: 4 U-turns.
        assert_eq!(connectors, 16);
        assert!(t.net.intersections.iter().any(|i| i.signalized));
    }

    #[test]
    fn lanes_keep_right_and_turns_stay_inside_junctions() {
        let roads = cross();
        let mut t = Traffic::default();
        t.rebuild(&roads);
        // Eastbound lane (a -> b with +x direction) must sit on the +y side in
        // sim coordinates, which is the driver's right once y maps to world z.
        for lane in t.net.lanes.iter().filter(|l| !l.is_connector()) {
            let d = (lane.end() - lane.start()).normalize_or_zero();
            let mid = (lane.start() + lane.end()) * 0.5;
            // Perpendicular offset of the lane from the segment centreline.
            let centre_offset = if d.x.abs() > 0.5 { mid.y } else { mid.x };
            let right = Vec2::new(-d.y, d.x);
            let side = if d.x.abs() > 0.5 { right.y } else { right.x };
            assert!(centre_offset * side > 0.0, "lane {:?} is on the left of its direction", lane.kind);
        }
        for lane in t.net.lanes.iter().filter(|l| l.is_connector()) {
            let node = roads.node(lane.end_node).pos;
            let r = t.net.node_radius[lane.end_node.0 as usize];
            for p in &lane.points {
                assert!(p.distance(node) <= r * 1.6 + 0.5, "connector swings {:.1} m out of a {:.1} m junction", p.distance(node), r);
            }
        }
    }

    #[test]
    fn a_car_drives_across_the_intersection() {
        let roads = cross();
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let west = roads.nearest_on_network(Vec2::new(-150.0, 0.0)).unwrap().0;
        let north = roads.nearest_on_network(Vec2::new(0.0, 150.0)).unwrap().0;
        let (route, s0, s1) = t.net.find_route(west, Vec2::new(-150.0, -2.0), north, Vec2::new(2.0, 150.0)).expect("route");
        assert!(route.len() >= 3);
        let citizen = CitizenId::default();
        let car = t.spawn_car(citizen, None, route, s0, s1, 0);
        let mut arrived = false;
        for _ in 0..240 {
            if t.step(1.0, 2).contains(&car) {
                arrived = true;
                break;
            }
        }
        assert!(arrived, "car should arrive within four minutes");
        assert!(t.cars[car].pos.distance(Vec2::new(2.0, 150.0)) < 6.0);
    }

    #[test]
    fn ramps_merge_without_signals_and_bends_keep_speed() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 2, 2);
        // A one-way ramp joining at a shallow angle.
        roads.add_polyline(&[Vec2::new(-150.0, 60.0), Vec2::new(-20.0, 2.0)], 1, 0);
        // A two-way side street.
        roads.add_polyline(&[Vec2::new(150.0, 200.0), Vec2::new(150.0, 3.0)], 1, 1);
        let mut net = LaneNetwork::default();
        net.rebuild(&roads);
        let merge = net.intersections.iter().find(|i| roads.node(i.node).pos.distance(Vec2::new(-20.0, 0.0)) < 1.0).expect("merge junction");
        assert_eq!(merge.approaches, 3);
        assert!(!merge.signalized, "a one-way ramp yields instead of getting a signal");
        let t = net.intersections.iter().find(|i| roads.node(i.node).pos.distance(Vec2::new(150.0, 0.0)) < 1.0).expect("T junction");
        assert!(t.signalized, "three two-way roads get signals");

        // A curved road is a chain of short straight pieces; its bends run at road speed.
        let curve = crate::plan::sample_curve(&[Vec2::new(0.0, -300.0), Vec2::new(60.0, -200.0), Vec2::new(0.0, -100.0)]);
        assert!(curve.len() > 10);
        let mut roads2 = RoadNetwork::default();
        roads2.add_polyline(&curve, 1, 1);
        let mut net2 = LaneNetwork::default();
        net2.rebuild(&roads2);
        // Only the bends: dead ends keep their slow U-turn connectors.
        let bend_connectors: Vec<&Lane> = net2.lanes.iter().filter(|l| matches!(l.kind, LaneKind::Connector { node, .. } if roads2.node(node).segments.len() == 2)).collect();
        assert!(!bend_connectors.is_empty());
        assert!(bend_connectors.iter().all(|l| l.speed_limit >= ROAD_SPEED - 0.01), "bends do not slow traffic to junction speed");
    }

    #[test]
    fn paths_carry_no_car_lanes_and_do_not_make_junctions() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 1, 1);
        let path = roads.add_polyline_kind(&[Vec2::new(0.0, 150.0), Vec2::new(0.0, 3.0)], 0, 0, crate::roads::RoadKind::Path);
        let mut net = LaneNetwork::default();
        net.rebuild(&roads);
        assert!(path.iter().all(|s| !net.segment_lanes.contains_key(s)), "paths have no lanes");
        let node = roads.nodes.iter().position(|n| n.pos.distance(Vec2::new(0.0, 0.0)) < 0.5).unwrap();
        let inter = net.intersections.iter().find(|i| i.node.0 as usize == node).unwrap();
        assert_eq!(inter.approaches, 2, "a path joining a street is not a junction for cars");
        assert!(!inter.signalized);
        assert!(roads.path(crate::roads::NodeId(node as u32), roads.segment(path[0]).a).is_some(), "walkers can use the path");
    }
}
