//! Car following: the intelligent driver model, what a driver sees ahead
//! (signals, junctions, rail crossings, the car in front) and passing a
//! waiting left-turner.

use super::*;

/// The intelligent driver model with a driver's own headway and acceleration.
pub(super) fn idm_with(v: f32, v0: f32, gap: f32, dv: f32, headway: f32, accel: f32) -> f32 {
    let gap = gap.max(0.1);
    let s_star = IDM_MIN_GAP + (v * headway + v * dv / (2.0 * (accel * IDM_BRAKE).sqrt())).max(0.0);
    // Over the desired speed the free-road term brakes firmly, not
    // without limit (see `FREE_BRAKE`).
    let free = (accel * (1.0 - (v / v0.max(0.1)).powi(4))).max(-FREE_BRAKE);
    free - accel * (s_star / gap).powi(2)
}

impl Traffic {
    /// Would a car entering the connector `conn` find no room on `exit`?
    /// Room is measured with the real vehicles on the exit lane: a bus
    /// takes nearly three cars' worth. `strict` (a real junction, where a
    /// car left in the box blocks cross traffic) also counts a stopped
    /// tail right at the start of the exit as no room.
    pub(super) fn exit_blocked(&self, conn: &Lane, exit: LaneId, strict: bool) -> bool {
        let el = &self.net.lanes[exit as usize];
        let queue: f32 = el.cars.iter().map(|&c| self.cars[c].length() + IDM_MIN_GAP).sum();
        let last = el.cars.first().map(|&c| &self.cars[c]);
        // Its tail is at the very start and the lane is packed, or that
        // tail is not moving off.
        let blocked = last.is_some_and(|c| c.s < c.length() + IDM_MIN_GAP + 1.0 && (queue > el.length - conn.length || (strict && c.v < 0.5)));
        let tail_near = last.is_some_and(|c| c.s < c.length() + 0.5);
        if strict && !conn.cars.is_empty() {
            // Room left at the start of the exit for the cars already in
            // the junction heading there, while its tail stands: counting
            // only the exit's own queue let a run of cars into the box
            // behind one another, and the last of them stood in it.
            let in_box: f32 = conn.cars.iter().map(|&c| self.cars[c].length() + IDM_MIN_GAP).sum();
            if last.is_some_and(|c| c.v < 0.5 && c.s - c.length() < in_box + CAR_LENGTH + IDM_MIN_GAP) {
                return true;
            }
        }
        blocked || (tail_near && !conn.cars.is_empty())
    }

    /// A connector inside a junction of three or more streets (not a bend).
    pub(super) fn at_junction(&self, conn: &Lane) -> bool {
        let LaneKind::Connector { node, .. } = conn.kind else { return false };
        self.net.node_intersection.get(node.0 as usize).and_then(|&i| self.net.intersections.get(i)).is_some_and(|i| i.approaches >= 3)
    }

    /// The speed limit a driver drives to: the limit where it is, or lower
    /// when a lane ahead on its route has a lower one it must brake for
    /// now to meet it at `ANTICIPATE_BRAKE` (a ramp easing down, a turn).
    pub(super) fn limit_ahead(&self, car: &Car, lane: &Lane) -> f32 {
        // [r4-hillside] Slower up (and down) a steep street; a bus or a
        // lorry slower still. A unit on a call under its siren keeps its
        // foot down: everyone else gives way to it on the hill.
        let share = if car.siren { 1.0 } else { lane.climb_share(car.bus.is_some() || car.cargo.is_some()) };
        let mut limit = lane.limit_at(car.s) * share;
        // The end of this lane (a ramp lane easing down).
        let to_end = (lane.length - car.s).max(0.0);
        limit = limit.min((lane.limit_end * lane.limit_end + 2.0 * ANTICIPATE_BRAKE * to_end).sqrt());
        let mut dist = to_end;
        let mut prev = lane;
        for &next in car.route.iter().skip(car.idx + 1).take(8) {
            if dist > ANTICIPATE_REACH {
                break;
            }
            let nl = &self.net.lanes[next as usize];
            if prev.is_beside(next) {
                // A lane change: the lane beside runs alongside, no farther.
                prev = nl;
                continue;
            }
            let reach = |v: f32, d: f32| (v * v + 2.0 * ANTICIPATE_BRAKE * d).sqrt();
            limit = limit.min(reach(nl.speed_limit, dist)).min(reach(nl.limit_end, dist + nl.length));
            dist += nl.length;
            prev = nl;
        }
        limit
    }

    /// Gap, leader speed and the reason for a car with no leader on its
    /// own lane: look down the route for the next car or a red light.
    pub(super) fn lookahead(&self, car: &Car, lane: &Lane) -> (f32, f32, Wait, Option<LaneId>) {
        let mut dist = lane.length - car.s;
        let route = &car.route;
        let mut prev = route[car.idx];
        let mut ring_go = false;
        let mut run = None;
        for (k, &next) in route.iter().enumerate().skip(car.idx + 1).take(4) {
            if self.net.lanes[prev as usize].is_beside(next) {
                // A lane change further on; the car deals with it there.
                break;
            }
            let nl = &self.net.lanes[next as usize];
            if nl.is_connector() && nl.blocked {
                // [driving] A takeover holds the junction.
                return (dist - 1.0 + IDM_MIN_GAP, 0.0, Wait::Blocked, None);
            }
            if nl.is_connector() && !nl.green {
                // [driving] Late ambers and reds, by temperament.
                if car.drive.running == Some(next) || (run.is_none() && Self::runs_red(car, nl, dist)) {
                    run = Some(next);
                } else {
                    // Stop line just before the lane ends; the min gap keeps the nose clear.
                    return (dist - 1.0 + IDM_MIN_GAP, 0.0, Wait::Signal, None);
                }
            }
            // Do not enter a junction unless the exit lane has room: this is
            // what keeps a saturated grid from locking up.
            if nl.is_connector() && !lane.is_connector() && k == car.idx + 1 {
                if let LaneKind::Connector { node, approach } = nl.kind {
                    // Roundabouts: entries give way to the ring, the ring has priority.
                    let ring = self.net.rings.nodes.contains_key(&node);
                    if ring {
                        match self.ring_wait(car, next, dist) {
                            roundabout::RingCall::Wait(gap, v, why) => return (gap, v, why, None),
                            roundabout::RingCall::Go => ring_go = true,
                        }
                    }
                    if let Some(inter) = self.net.node_intersection.get(node.0 as usize).and_then(|&i| self.net.intersections.get(i)) {
                        // Merging traffic yields; the highway mainline does not.
                        let barging = run.is_some() || car.drive.profile.ignores_signals;
                        if !ring && !barging && !inter.signalized && inter.approaches >= 3 && !lane.highway {
                            let busy = inter.connectors.iter().any(|&(lid, ap)| ap != approach && !self.net.lanes[lid as usize].cars.is_empty());
                            if busy {
                                return (dist - 1.0 + IDM_MIN_GAP, 0.0, Wait::Yield, None);
                            }
                        }
                        if !barging && inter.signalized && nl.turn < 0 && self.oncoming(inter, approach) {
                            return (dist - 1.0 + IDM_MIN_GAP, 0.0, Wait::Yield, None);
                        }
                    }
                }
                if let Some(&exit) = route.get(k + 1).filter(|_| !ring_go) {
                    if self.exit_blocked(nl, exit, self.at_junction(nl)) {
                        return (dist - 1.0 + IDM_MIN_GAP, 0.0, Wait::ExitFull, None);
                    }
                }
            }
            if let Some(&first) = nl.cars.first() {
                let lead = &self.cars[first];
                return (dist + lead.s - lead.length(), lead.v, Wait::Leader, run);
            }
            dist += nl.length;
            if dist > LOOKAHEAD {
                break;
            }
            prev = next;
        }
        (LOOKAHEAD * 2.0, ROAD_SPEED, Wait::Free, run)
    }

    /// [rail-crossing] The gap to the stop line of a level crossing ahead
    /// on the route whose lights are on, or `None`: none ahead, the car is
    /// on the crossing already (it drives off), or it is too close to stop
    /// at [`RAIL_STOP_BRAKE`] (it goes on over and clears it, as drivers do
    /// when the lights come on just in front of them).
    pub(super) fn rail_gap(&self, car: &Car, lane: &Lane) -> Option<f32> {
        if lane.is_connector() {
            return None;
        }
        let mut dist = lane.length - car.s;
        let mut prev = car.route[car.idx];
        for &next in car.route.iter().skip(car.idx + 1).take(6) {
            if self.net.lanes[prev as usize].is_beside(next) {
                prev = next;
                continue;
            }
            let nl = &self.net.lanes[next as usize];
            if nl.is_connector() && nl.rail_closed {
                let stop = dist - RAIL_STOP_SHORT;
                // Already braking for it: carries on braking (the decision
                // is not taken again at every step).
                if car.wait != Wait::RailCrossing && car.v * car.v > 2.0 * RAIL_STOP_BRAKE * stop.max(0.0) {
                    return None;
                }
                return Some(stop + IDM_MIN_GAP);
            }
            dist += nl.length;
            if dist > LOOKAHEAD {
                break;
            }
            prev = next;
        }
        None
    }

    /// The turn of the connector a car on a road lane takes next, if any.
    pub(super) fn next_turn(&self, car: &Car) -> Option<i8> {
        let here = &self.net.lanes[car.route[car.idx] as usize];
        if here.is_connector() {
            return None;
        }
        let next = &self.net.lanes[*car.route.get(car.idx + 1)? as usize];
        next.is_connector().then_some(next.turn)
    }

    /// A left turner at the head of its lane, close to the stop line and
    /// slow: someone the car behind may pass on the right.
    pub(super) fn waiting_left(&self, lane: &Lane, id: CarId) -> bool {
        let c = &self.cars[id];
        lane.cars.last() == Some(&id) && c.v < 3.0 && lane.length - c.s < 25.0 && self.next_turn(c) == Some(-1)
    }

    /// How far right of its lane centre a car can squeeze past another on
    /// this lane without touching the kerb, if there is room at all.
    pub(super) fn pass_offset(lane: &Lane) -> Option<f32> {
        let room = LANE_WIDTH * 0.5 + SHOULDER - CAR_WIDTH * 0.5 - 0.15;
        let need = CAR_WIDTH + 0.25;
        (lane.right.is_none() && room >= need).then_some(need)
    }
}
