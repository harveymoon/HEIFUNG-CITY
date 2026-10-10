//! Lane-graph pathfinding (A*) and the cars' routes: planning, replanning
//! and rerouting a stalled car.

use super::*;

impl LaneNetwork {
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

    /// The polyline a route drives, for drawing.
    pub fn polyline(&self, lanes: &[LaneId], s0: f32, s1: f32) -> Vec<Vec2> {
        let mut out: Vec<Vec2> = Vec::new();
        // Where the drive enters each lane: a lane change is drawn a few
        // metres after the car joined the lane it leaves.
        let mut a = s0;
        for (k, &l) in lanes.iter().enumerate() {
            let lane = &self.lanes[l as usize];
            let changes = lanes.get(k + 1).is_some_and(|&n| lane.is_beside(n));
            let b = if k + 1 == lanes.len() {
                s1
            } else if changes {
                (a + 10.0).min(lane.length)
            } else {
                lane.length
            };
            for p in lane.points_between(a, b) {
                if out.last().is_none_or(|q| q.distance(p) > 0.05) {
                    out.push(p);
                }
            }
            a = if changes { b } else { 0.0 };
        }
        out
    }

    /// A* over lanes from a point on one segment to a point on another.
    pub fn find_route(&self, from_seg: SegmentId, from: Vec2, to_seg: SegmentId, to: Vec2) -> Option<(Vec<LaneId>, f32, f32)> {
        let (start, s_start, _) = self.nearest_lane_on(from_seg, from)?;
        let (goal, s_end, _) = self.nearest_lane_on(to_seg, to)?;
        self.route_lanes(start, s_start, goal, s_end)
    }

    /// [pt-transit] `find_route`, and where the goal lies behind the start
    /// on its own lane (which `find_route` never routes: it does not
    /// re-enter its start lane), the drive off the end of the lane and
    /// round the block back onto it. For bus lines, whose legs may run
    /// back along the street they started on; other vehicles look for
    /// another way to start instead.
    pub fn find_route_round(&self, from_seg: SegmentId, from: Vec2, to_seg: SegmentId, to: Vec2) -> Option<(Vec<LaneId>, f32, f32)> {
        if let Some(r) = self.find_route(from_seg, from, to_seg, to) {
            return Some(r);
        }
        let (start, s_start, _) = self.nearest_lane_on(from_seg, from)?;
        let (goal, s_end, _) = self.nearest_lane_on(to_seg, to)?;
        if start != goal {
            return None;
        }
        let mut best: Option<(f32, Vec<LaneId>)> = None;
        for &n in &self.lanes.get(start as usize)?.next {
            if !self.lanes[n as usize].permitted {
                continue;
            }
            let Some((rest, _, _)) = self.route_lanes(n, 0.0, goal, s_end) else { continue };
            let len: f32 = rest.iter().map(|&l| self.lanes[l as usize].length).sum();
            if best.as_ref().is_none_or(|(b, _)| len < *b) {
                best = Some((len, rest));
            }
        }
        best.map(|(_, rest)| {
            let mut route = vec![start];
            route.extend(rest);
            (route, s_start, s_end)
        })
    }

    /// A* over lanes from `s_start` along `start` to `s_end` along `goal`;
    /// the route begins with `start`. A goal behind the start on the same
    /// lane has no route: lanes are one way and the start is never re-entered.
    pub fn route_lanes(&self, start: LaneId, s_start: f32, goal: LaneId, s_end: f32) -> Option<(Vec<LaneId>, f32, f32)> {
        self.route_avoiding(start, s_start, goal, s_end, None, None)
    }

    /// `route_lanes` that never enters `avoid_lane` and never takes the
    /// step `avoid_step` (from, to): a car looking for another way round a
    /// full exit or a lane change it cannot make.
    ///
    /// Lane changes are steps between parallel lanes, costed at
    /// `LANE_CHANGE_COST`; every vehicle already on a lane adds
    /// `QUEUE_COST`, so routes spread over lanes and around queues.
    pub fn route_avoiding(&self, start: LaneId, s_start: f32, goal: LaneId, s_end: f32, avoid_lane: Option<LaneId>, avoid_step: Option<(LaneId, LaneId)>) -> Option<(Vec<LaneId>, f32, f32)> {
        self.route_search(start, s_start, goal, s_end, avoid_lane, avoid_step, true)
    }

    /// The A* behind `route_avoiding`; `tolls` false searches as if no road
    /// were tolled.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn route_search(&self, start: LaneId, s_start: f32, goal: LaneId, s_end: f32, avoid_lane: Option<LaneId>, avoid_step: Option<(LaneId, LaneId)>, tolls: bool) -> Option<(Vec<LaneId>, f32, f32)> {
        if start == goal && s_end >= s_start {
            return Some((vec![start], s_start, s_end));
        }
        let to = self.lanes.get(goal as usize)?.point_at(s_end).0;
        SEARCH.with(|cell| {
            let mut scratch = cell.borrow_mut();
            scratch.begin(self.lanes.len());
            self.route_search_in(&mut scratch, start, s_start, goal, s_end, to, avoid_lane, avoid_step, tolls)
        })
    }

    /// [`Self::route_search`] on reused scratch (the costs and parents of
    /// the lanes reached, by lane index).
    #[allow(clippy::too_many_arguments)]
    fn route_search_in(&self, sc: &mut SearchScratch, start: LaneId, s_start: f32, goal: LaneId, s_end: f32, to: Vec2, avoid_lane: Option<LaneId>, avoid_step: Option<(LaneId, LaneId)>, tolls: bool) -> Option<(Vec<LaneId>, f32, f32)> {
        let mut open = BinaryHeap::new();
        let start_cost = self.lanes[start as usize].length - s_start;
        sc.set(start, start_cost, None);
        open.push(Open { f: start_cost + self.lanes[start as usize].end().distance(to), g: start_cost, lane: start, expanded: 0 });
        let mut expansions = 0usize;
        while let Some(Open { lane, expanded, g: popped_g, .. }) = open.pop() {
            // A lane reached again more cheaply leaves its older entry in
            // the heap: skip it rather than expand it (and count it toward
            // the cap) a second time.
            if popped_g > sc.g(lane).unwrap() {
                continue;
            }
            if lane == goal && expanded > 0 {
                let mut route = vec![goal];
                let mut cur = goal;
                while let Some(prev) = sc.came(cur) {
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
            let gl = sc.g(lane).unwrap();
            let here = &self.lanes[lane as usize];
            // [tolls] A toll counts as the detour a driver would take to avoid it.
            let ahead = here.next.iter().filter(|&&n| self.lanes[n as usize].permitted).map(|&n| (n, self.lanes[n as usize].length + self.rings.penalty(n) + if tolls { self.toll_penalty(n) } else { 0.0 }));
            // Room on this piece before the solid line of the junction
            // ahead (the line may begin pieces back, round bends).
            // On a roundabout, slow and with arms close together, the line
            // is the shortest there is and changing between arms is how
            // the ring is driven: only a change inside the line costs.
            let ring = self.rings.circulating.contains(&lane);
            let line = if ring { SOLID_LINE_MIN } else { SOLID_LINE_MAX };
            let room = here.length - (line - self.junction_after(lane, line)).max(0.0);
            let late = if ring { 0.0 } else { (1.0 - room / SHORT_CHANGE_LEN).max(0.0) };
            let short = if room <= 0.0 || (!ring && room < CHANGE_ROOM_MIN) {
                SOLID_CHANGE_COST
            } else if ring {
                0.0
            } else {
                SHORT_CHANGE_COST * late
            };
            let beside = [here.left, here.right].into_iter().flatten().map(|n| (n, LANE_CHANGE_COST + short + CUT_IN_COST * late * self.lanes[n as usize].cars.len() as f32));
            for (next, step) in ahead.chain(beside) {
                if next == start || Some(next) == avoid_lane || Some((lane, next)) == avoid_step {
                    continue;
                }
                // Changing onto the goal lane from the start puts the car
                // level with where it is: the goal must not be behind that.
                if lane == start && next == goal && here.is_beside(next) && s_end < s_start + 1.0 {
                    continue;
                }
                let cost = gl + step + QUEUE_COST * self.lanes[next as usize].cars.len() as f32;
                if sc.g(next).map_or(true, |old| cost < old) {
                    sc.set(next, cost, Some(lane));
                    open.push(Open { f: cost + self.lanes[next as usize].end().distance(to), g: cost, lane: next, expanded: expanded + 1 });
                }
            }
        }
        None
    }

    /// Does this road lane end at a junction (three or more streets, or a
    /// roundabout junction with arms), not at a bend? Where the solid line
    /// before it forbids lane changes.
    pub fn ends_at_junction(&self, lane: LaneId) -> bool {
        let Some(l) = self.lanes.get(lane as usize) else { return false };
        if l.is_connector() {
            return false;
        }
        let node = l.end_node;
        self.node_intersection.get(node.0 as usize).and_then(|&i| self.intersections.get(i)).is_some_and(|i| i.approaches >= 3)
            || self.rings.nodes.get(&node).is_some_and(|n| !n.entries.is_empty() || !n.exits.is_empty())
    }

    /// Metres from the end of `lane` to the next junction straight on
    /// (through bends, where a lane has one way on), up to `limit`.
    pub fn junction_after(&self, lane: LaneId, limit: f32) -> f32 {
        let mut d = 0.0;
        let mut l = lane;
        for _ in 0..16 {
            if self.ends_at_junction(l) {
                return d;
            }
            let here = &self.lanes[l as usize];
            let [conn] = here.next[..] else { return limit };
            let Some(&n) = self.lanes[conn as usize].next.first() else { return limit };
            d += self.lanes[conn as usize].length + self.lanes[n as usize].length;
            if d >= limit {
                return limit;
            }
            l = n;
        }
        limit
    }

    /// Metres from `s` along `route[idx]` to the next junction on the
    /// route (`ends_at_junction`), up to `limit`; lane changes on the way
    /// add nothing (the lane beside runs alongside).
    pub fn to_junction(&self, route: &[LaneId], idx: usize, s: f32, limit: f32) -> f32 {
        let mut d = 0.0;
        let mut k = idx;
        let mut from = s;
        while let Some(&l) = route.get(k) {
            let lane = &self.lanes[l as usize];
            if route.get(k + 1).is_some_and(|&n| lane.is_beside(n)) {
                k += 1;
                continue;
            }
            d += (lane.length - from).max(0.0);
            if self.ends_at_junction(l) || d >= limit {
                return d.min(limit);
            }
            from = 0.0;
            k += 1;
        }
        limit
    }

    /// Metres left to drive on `route` from `s` along its first lane.
    pub fn route_length(&self, route: &[LaneId], s: f32) -> f32 {
        let mut total = 0.0;
        for (k, &l) in route.iter().enumerate() {
            let lane = &self.lanes[l as usize];
            total += if k == 0 {
                lane.length - s
            } else if self.lanes[route[k - 1] as usize].is_beside(l) {
                LANE_CHANGE_COST
            } else {
                lane.length
            };
        }
        total
    }
}

thread_local! {
    /// [review] Scratch for the lane A*, reused across searches instead of
    /// two fresh hash maps per call.
    static SEARCH: std::cell::RefCell<SearchScratch> = std::cell::RefCell::new(SearchScratch::default());
}

/// Per-lane cost and parent, valid for the lanes stamped with the current
/// search's number (so nothing is cleared between searches).
#[derive(Default)]
pub(super) struct SearchScratch {
    search: u32,
    stamp: Vec<u32>,
    cost: Vec<f32>,
    parent: Vec<LaneId>,
}

impl SearchScratch {
    const NONE: LaneId = LaneId::MAX;

    fn begin(&mut self, lanes: usize) {
        if self.stamp.len() < lanes {
            self.stamp.resize(lanes, 0);
            self.cost.resize(lanes, 0.0);
            self.parent.resize(lanes, Self::NONE);
        }
        self.search = self.search.wrapping_add(1);
        if self.search == 0 {
            // Wrapped: forget every old stamp.
            self.stamp.iter_mut().for_each(|s| *s = 0);
            self.search = 1;
        }
    }

    fn g(&self, lane: LaneId) -> Option<f32> {
        (self.stamp[lane as usize] == self.search).then(|| self.cost[lane as usize])
    }

    fn came(&self, lane: LaneId) -> Option<LaneId> {
        (self.stamp[lane as usize] == self.search && self.parent[lane as usize] != Self::NONE).then(|| self.parent[lane as usize])
    }

    fn set(&mut self, lane: LaneId, cost: f32, parent: Option<LaneId>) {
        let i = lane as usize;
        self.stamp[i] = self.search;
        self.cost[i] = cost;
        self.parent[i] = parent.unwrap_or(Self::NONE);
    }
}

#[derive(PartialEq)]
pub(super) struct Open {
    pub(super) f: f32,
    /// The cost to reach `lane` when this entry was pushed (stale when
    /// the lane has since been reached for less).
    pub(super) g: f32,
    pub(super) lane: LaneId,
    pub(super) expanded: u32,
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

impl Traffic {
    /// Send a car on a new route from where it stands: a bus leaving a stop.
    /// If the new route starts on the car's current lane it keeps its place;
    /// otherwise it is moved to the start of the new route.
    pub fn reroute(&mut self, id: CarId, route: Vec<LaneId>, s_start: f32, s_end: f32) {
        if route.is_empty() {
            return;
        }
        let old_lane = self.cars[id].route[self.cars[id].idx];
        if route[0] == old_lane {
            let car = &mut self.cars[id];
            car.route = route;
            car.idx = 0;
            car.s_end = s_end;
            car.age = 0.0;
            return;
        }
        if let Some(l) = self.net.lanes.get_mut(old_lane as usize) {
            l.cars.retain(|&c| c != id);
        }
        let first = route[0];
        let off_lanes = {
            let car = &mut self.cars[id];
            car.route = route;
            car.idx = 0;
            car.s = s_start;
            car.s_end = s_end;
            car.age = 0.0;
            car.entering || car.in_stunt()
        };
        // A car at the kerb (or in a stunt) is in no lane list: `pull_out`
        // (or `end_stunt`) puts it in. Inserting it here too listed it twice.
        if !off_lanes {
            self.insert_sorted(first, id);
        }
    }

    /// Route a stalled car afresh from the lane it stands on to the same
    /// destination, keeping its place in the lane. False when no route
    /// exists (the destination is behind it on its own lane, say).
    pub(super) fn replan_from_here(&mut self, id: CarId) -> bool {
        let (here, s, goal, s_end) = {
            let c = &self.cars[id];
            (c.route[c.idx], c.s, *c.route.last().unwrap(), c.s_end)
        };
        let Some((route, _, _)) = self.net.route_lanes(here, s, goal, s_end) else { return false };
        let car = &mut self.cars[id];
        car.route = route;
        car.idx = 0;
        car.age = 0.0;
        true
    }

    /// [carlife] Send a car from where it stands to a new goal (`goal` lane
    /// at `s_end`), keeping its place in its lane: a driver going on to a
    /// parking space, a truck heading back to base. False when there is no
    /// way from here (the car keeps its old route).
    pub fn redirect(&mut self, id: CarId, goal: LaneId, s_end: f32) -> bool {
        match self.route_from_here(id, goal, s_end) {
            Some(route) => self.follow(id, route, s_end),
            None => false,
        }
    }

    /// [carlife] The route a car would take from where it stands to `goal`
    /// at `s_end`, beginning with its own lane.
    pub fn route_from_here(&self, id: CarId, goal: LaneId, s_end: f32) -> Option<Vec<LaneId>> {
        let c = self.cars.get(id)?;
        self.net.route_lanes(c.route[c.idx], c.s, goal, s_end).map(|r| r.0)
    }

    /// [carlife] Metres a car would drive on `route` (from where it stands
    /// on the first lane to `s_end` on the last).
    pub fn route_metres(&self, id: CarId, route: &[LaneId], s_end: f32) -> f32 {
        let s = self.cars.get(id).map_or(0.0, |c| c.s);
        let full = self.net.route_length(route, s);
        let last = route.last().map_or(0.0, |&l| self.net.lanes[l as usize].length);
        (full - (last - s_end).max(0.0)).max(0.0)
    }

    /// [carlife] Follow `route` (from `route_from_here`) from where the car
    /// stands; false if it does not start on the car's lane.
    pub fn follow(&mut self, id: CarId, route: Vec<LaneId>, s_end: f32) -> bool {
        let Some(car) = self.cars.get_mut(id) else { return false };
        if route.first() != Some(&car.route[car.idx]) {
            return false;
        }
        car.route = route;
        car.idx = 0;
        car.s_end = s_end;
        car.age = 0.0;
        true
    }

    /// Cars stalled at the head of their lane for `REROUTE_AFTER` seconds
    /// because their exit is full (a gridlock ring, a jammed street) or
    /// because they cannot get into the lane their turn needs look for
    /// another way from where they stand.
    pub(super) fn reroute_stalled(&mut self) {
        let ids: Vec<CarId> = self.cars.keys().collect();
        for &id in &ids {
            self.bail_off_ring(id);
        }
        for id in ids {
            let c = &self.cars[id];
            if c.stalled < REROUTE_LANE_CHANGE_AFTER || c.cooldown > 0.0 || c.hold > 0.0 || c.entering || c.idx + 1 >= c.route.len() {
                continue;
            }
            let here = c.route[c.idx];
            let lane = &self.net.lanes[here as usize];
            if lane.is_connector() || lane.cars.last() != Some(&id) || lane.length - c.s > 6.0 {
                continue;
            }
            let next = c.route[c.idx + 1];
            // A crash in the junction (or just beyond it) on the way: go round it.
            let nl = &self.net.lanes[next as usize];
            let wreck_ahead = nl.cars.iter().any(|&o| self.cars[o].crashed())
                || c.route.get(c.idx + 2).is_some_and(|&e| self.net.lanes[e as usize].cars.first().is_some_and(|&o| self.cars[o].crashed() && self.cars[o].s < 2.0 * CAR_LENGTH));
<<<<<<< HEAD
            // [pt-transit] A bus keeps to its line: it waits for the lane
            // change (swapping in at the end of the lane, if need be) and
            // for a full exit to clear, and only goes another way after
            // `BUS_DETOUR_AFTER`, or round a crash.
            let bus_waits = c.bus.is_some() && c.stalled < BUS_DETOUR_AFTER;
=======
            let mut full_exit = false;
>>>>>>> main
            let (avoid_lane, avoid_step) = if lane.is_beside(next) {
                if bus_waits {
                    continue;
                }
                (None, Some((here, next)))
            } else if wreck_ahead && c.stalled >= REROUTE_AFTER && nl.is_connector() {
                (Some(next), None)
            } else if c.stalled < REROUTE_AFTER || bus_waits {
                continue;
            } else {
                match c.route.get(c.idx + 2) {
                    Some(&exit) if self.exit_blocked(&self.net.lanes[next as usize], exit, true) => {
                        full_exit = true;
                        (Some(exit), None)
                    }
                    _ => continue,
                }
            };
            let (goal, s_end, s) = (*c.route.last().unwrap(), c.s_end, c.s);
            let mut found = self.net.route_avoiding(here, s, goal, s_end, avoid_lane, avoid_step);
            // [rage] Round a full exit only by a modest detour; otherwise it
            // waits its turn in the queue (the exit clears).
            if full_exit {
                let left = self.net.route_length(&c.route[c.idx..], s);
                if found.as_ref().is_some_and(|r| self.net.route_length(&r.0, s) > left + REROUTE_DETOUR_M.max(left * REROUTE_DETOUR_SHARE)) {
                    found = None;
                }
            }
            let c = &mut self.cars[id];
            // Back off: a car that keeps standing tries again ever more rarely.
            c.cooldown = (REROUTE_LANE_CHANGE_AFTER * 2f32.powi(c.replans as i32)).min(REROUTE_BACKOFF_MAX);
            c.replans = c.replans.saturating_add(1).min(8);
            if let Some((route, _, _)) = found {
                c.route = route;
                c.idx = 0;
            }
        }
    }
}
