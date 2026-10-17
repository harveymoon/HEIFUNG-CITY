//! Lane changes: for the next turn, to overtake, and swaps between two
//! cars that each want the other's lane.

use super::*;

impl Traffic {
    /// Leader on `lane` ahead of position `s` and the follower behind it
    /// (looking back onto the lanes feeding it), as `(id, gap)` measured for
    /// a vehicle of length `len` standing at `s`.
    pub(super) fn neighbours_at(&self, lane: LaneId, s: f32, len: f32, skip: CarId) -> (Option<(CarId, f32)>, Option<(CarId, f32)>) {
        let tl = &self.net.lanes[lane as usize];
        let cars: Vec<CarId> = tl.cars.iter().copied().filter(|&c| c != skip).collect();
        let pos = cars.iter().position(|&o| self.cars[o].s > s).unwrap_or(cars.len());
        let leader = cars.get(pos).map(|&l| (l, self.cars[l].s - s - self.cars[l].length()));
        let follower = if pos > 0 {
            let f = cars[pos - 1];
            Some((f, s - self.cars[f].s - len))
        } else {
            tl.prev
                .iter()
                .filter_map(|&p| {
                    let pl = &self.net.lanes[p as usize];
                    pl.cars.last().map(|&f| (f, s + pl.length - self.cars[f].s - len))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
        };
        (leader, follower)
    }

    /// Can the car move across into `target` (beside its lane) right now
    /// without itself or its new follower braking harder than `brake`?
    /// Returns its acceleration there.
    pub(super) fn change_ok(&self, id: CarId, target: LaneId, brake: f32) -> Option<f32> {
        let c = &self.cars[id];
        let tl = &self.net.lanes[target as usize];
        let s = c.s.min(tl.length);
        let (leader, follower) = self.neighbours_at(target, s, c.length(), id);
        let acc = match leader {
            Some((l, gap)) => {
                if gap < IDM_MIN_GAP * 0.5 {
                    return None;
                }
                c.idm(c.desired_speed(tl.limit_at(s)), gap, c.v - self.cars[l].v)
            }
            None => c.idm(c.desired_speed(tl.limit_at(s)), tl.length - s + LOOKAHEAD, 0.0),
        };
        if acc < -brake {
            return None;
        }
        if let Some((f, gap)) = follower {
            let fc = &self.cars[f];
            if gap < IDM_MIN_GAP * 0.5 {
                return None;
            }
            if fc.idm(fc.desired_speed(tl.limit_at(fc.s)), gap, fc.v - c.v) < -brake {
                return None;
            }
        }
        Some(acc)
    }

    /// Move a car into the lane beside it, on its route's next step or on a
    /// new route that starts there.
    pub(super) fn change_lane(&mut self, id: CarId, target: LaneId, new_route: Option<Vec<LaneId>>) {
        let old = self.cars[id].route[self.cars[id].idx];
        self.net.lanes[old as usize].cars.retain(|&c| c != id);
        let shift = if self.net.lanes[old as usize].right == Some(target) { -LANE_WIDTH } else { LANE_WIDTH };
        let len = self.net.lanes[target as usize].length;
        let c = &mut self.cars[id];
        match new_route {
            Some(r) => {
                c.route = r;
                c.idx = 0;
            }
            None => c.idx += 1,
        }
        c.s = c.s.min(len);
        c.lateral = (c.lateral + shift).clamp(-1.5 * LANE_WIDTH, 1.5 * LANE_WIDTH);
        // A glide across, longer the faster the car goes.
        c.glide_from = c.lateral;
        c.glide_len = (GLIDE_SECONDS * c.v).clamp(GLIDE_MIN, GLIDE_MAX);
        c.glide_done = 0.0;
        // [driving] Eager drivers change again sooner.
        c.cooldown = LC_COOLDOWN * (1.5 - c.drive.profile.aggression).max(0.3);
        self.insert_sorted(target, id);
    }

    /// Lane changes for one physics step: cars whose route steps into the
    /// lane beside them change as soon as it is safe; others overtake a
    /// vehicle holding them up when a parallel lane is better and the
    /// route from there is no real detour.
    pub(super) fn lane_changes(&mut self) {
        let ids: Vec<CarId> = self.cars.keys().collect();
        for id in ids {
            let c = &self.cars[id];
            if c.hold > 0.0 || c.entering || c.crashed() || c.in_stunt() {
                continue;
            }
            let lane = &self.net.lanes[c.route[c.idx] as usize];
            if lane.is_connector() {
                continue;
            }
            // [driving] How eagerly this driver changes lanes.
            let aggr = c.drive.profile.aggression;
            let safe_brake = LC_SAFE_BRAKE + 2.0 * (aggr - 0.5).max(0.0);
            if lane.left.is_none() && lane.right.is_none() {
                // One lane: squeeze past a waiting left turner on the right.
                let k = lane.cars.iter().position(|&o| o == id);
                let lead = k.and_then(|k| lane.cars.get(k + 1)).copied();
                if c.passing {
                    if !lead.is_some_and(|l| self.next_turn(&self.cars[l]) == Some(-1)) {
                        self.cars[id].passing = false;
                    }
                } else if let (Some(l), Some(_)) = (lead, Self::pass_offset(lane)) {
                    let gap = self.cars[l].s - c.s - self.cars[l].length();
                    let through = self.next_turn(c).is_some_and(|t| t >= 0) && self.net.lanes[c.route[c.idx + 1] as usize].permitted;
                    let at_junction = self.net.node_intersection.get(lane.end_node.0 as usize).and_then(|&i| self.net.intersections.get(i)).is_some_and(|i| i.approaches >= 3);
                    if c.bus.is_none() && through && at_junction && c.v < PASS_MAX_SPEED && gap < 30.0 && c.s < lane.length - CROSSING_SETBACK - 6.0 && self.waiting_left(lane, l) {
                        self.cars[id].passing = true;
                    }
                }
                continue;
            }
            // The solid line before a junction: no changing lanes there.
            let zone = if self.net.rings.circulating.contains(&c.route[c.idx]) { SOLID_LINE_MIN } else { (SOLID_LINE_SECONDS * c.v).clamp(SOLID_LINE_MIN, SOLID_LINE_MAX) };
            // ([services] Lights and sirens cross the line: a missed turn
            // would cost an ambulance minutes.)
            // ([pt-transit] Nor does a bus give up its turn there: it waits
            // for its gap, so it keeps to its line instead of finding
            // another way round the blocks.)
            let solid = !c.siren && c.bus.is_none() && self.net.to_junction(&c.route, c.idx, c.s, zone + 1.0) <= zone;
            if let Some(target) = c.change_pending(&self.net) {
                // (Retried every `SOLID_RETRY` when there was no other way;
                // a longer back-off from elsewhere does not hold it at the
                // line.)
                if solid && !(c.cooldown > 0.0 && c.cooldown <= SOLID_RETRY) {
                    // Not in the lane its turn needs by the solid line: it
                    // carries on in its own lane and finds another way (it
                    // misses the turn) rather than stopping at the line to
                    // wait for a gap. Two cars there each wanting the
                    // other's lane used to stand side by side for good.
                    let here = c.route[c.idx];
                    let (goal, s_end, s) = (*c.route.last().unwrap(), c.s_end, c.s);
                    // [r2-traffic] Not back into ways it gave up on, queues
                    // and turning back weighed (out and back down a dead
                    // end into the same jam: K52).
                    let avoid: Vec<LaneId> = c.avoided.iter().map(|&(l, _)| l).filter(|&l| l != goal).collect();
                    let mut found = self.net.route_replan(here, s, goal, s_end, &avoid, Some((here, target)));
                    if found.is_none() && !avoid.is_empty() {
                        found = self.net.route_replan(here, s, goal, s_end, &[], Some((here, target)));
                    }
                    match found {
                        Some((route, _, _)) => {
                            let c = &mut self.cars[id];
                            c.route = route;
                            c.idx = 0;
                            c.remember_avoided(target);
                            continue;
                        }
                        // No other way at all: as a last resort it waits
                        // for its gap (let in by the zip, or a swap).
                        None => self.cars[id].cooldown = SOLID_RETRY,
                    }
                }
                let c = &self.cars[id];
                let forced = lane.length - c.s < 8.0 && c.v < 1.0;
                if self.change_ok(id, target, if forced { LC_FORCED_BRAKE.max(safe_brake) } else { safe_brake }).is_some() {
                    self.change_lane(id, target, None);
                } else if forced && c.stalled > SWAP_AFTER {
                    self.swap_lanes(id, target);
                }
                continue;
            }
            if solid || c.cooldown > 0.0 || lane.length - c.s < 12.0 || (c.idx + 1 == c.route.len() && c.s_end - c.s < 40.0) {
                continue;
            }
            // Held up by a vehicle that is slow on its own account?
            let Some(k) = lane.cars.iter().position(|&o| o == id) else { continue };
            let Some(&lead) = lane.cars.get(k + 1) else { continue };
            let l = &self.cars[lead];
            let gap = l.s - c.s - l.length();
            let v0 = c.desired_speed(lane.limit_at(c.s));
            // A bus at a stop, a slower truck, or a left turner about to wait
            // for a gap in the oncoming traffic.
            // [driving] A crashed car is in the way too; an aggressive driver
            // weaves past anyone a little slower than they would like.
            let slow = l.hold > 0.0 || l.crashed() || (l.wait == Wait::Free && l.v < 0.9 * v0) || (aggr > 0.7 && l.v < 0.95 * v0) || (self.waiting_left(lane, lead) && self.next_turn(c) != Some(-1));
            if gap > 35.0 + 40.0 * (aggr - 0.5).max(0.0) || !slow {
                continue;
            }
            let acc_here = c.idm(v0, gap, c.v - l.v);
            let gain = OVERTAKE_GAIN * ((1.2 - aggr) / 0.7).max(0.15);
            let (goal, s_end, s) = (*c.route.last().unwrap(), c.s_end, c.s);
            let old_len = self.net.route_length(&c.route[c.idx..], s);
            for target in [lane.left, lane.right].into_iter().flatten() {
                let Some(acc) = self.change_ok(id, target, safe_brake) else { continue };
                if acc - acc_here < gain {
                    continue;
                }
                let Some((route, _, _)) = self.net.route_lanes(target, s, goal, s_end) else { continue };
                if self.net.route_length(&route, s) > old_len + 3.0 * LANE_CHANGE_COST + 20.0 {
                    continue;
                }
                self.change_lane(id, target, Some(route));
                break;
            }
        }
    }

    /// Two stopped cars side by side, each waiting at the end of its lane
    /// to move into the other's, can never make room for each other: each
    /// stands in the gap the other needs. They squeeze past at a crawl and
    /// swap places, each taking the other's spot (so nobody else in either
    /// lane is overlapped).
    pub(super) fn swap_lanes(&mut self, id: CarId, target: LaneId) {
        let c = &self.cars[id];
        let here = c.route[c.idx];
        let s = c.s;
        let other = self.net.lanes[target as usize].cars.iter().copied().filter(|&o| o != id).min_by(|&a, &b| (self.cars[a].s - s).abs().total_cmp(&(self.cars[b].s - s).abs()));
        let Some(o) = other else { return };
        let oc = &self.cars[o];
        if oc.v >= 0.5 || oc.entering || oc.hold > 0.0 || oc.crashed() || (oc.s - s).abs() > 3.0 || oc.change_pending(&self.net) != Some(here) {
            return;
        }
        let (s_a, s_b) = (s, oc.s);
        self.change_lane(id, target, None);
        self.change_lane(o, here, None);
        self.cars[id].s = s_b.min(self.net.lanes[target as usize].length);
        self.cars[o].s = s_a.min(self.net.lanes[here as usize].length);
        for l in [here, target] {
            let mut cars = std::mem::take(&mut self.net.lanes[l as usize].cars);
            cars.sort_by(|a, b| self.cars[*a].s.total_cmp(&self.cars[*b].s));
            self.net.lanes[l as usize].cars = cars;
        }
    }

    /// A lane change at the very end of a route, on a lane that leads
    /// nowhere (a carriageway running off the map), is pointless: the car
    /// is gone at the end whichever lane it is in. Routes to a map exit end
    /// on one chosen lane, so a car in another lane that could not merge on
    /// the way stopped at the map's edge to squeeze in, and the queue behind
    /// it stood for minutes (found in a player's save: the highway's three
    /// lanes funnelled into one at both ends of the map, cars standing up to
    /// five minutes). A car standing at the end of such a lane for
    /// `DEAD_END_GIVE_UP` seconds drops the change and leaves in its own
    /// lane. Only then: a car that merges on the way, as nearly all do,
    /// drives exactly as before.
    pub(super) fn drop_dead_end_changes(&mut self) {
        let lanes = &self.net.lanes;
        for car in self.cars.values_mut() {
            let n = car.route.len();
            if car.entering || n < 2 || car.idx + 2 != n || car.v >= 0.5 || car.stalled < DEAD_END_GIVE_UP {
                continue;
            }
            let (here, to) = (car.route[n - 2], car.route[n - 1]);
            let Some(l) = lanes.get(here as usize) else { continue };
            if l.next.is_empty() && l.is_beside(to) && car.s >= l.length - 3.0 {
                car.route.pop();
                car.s_end = car.s_end.min(l.length - 0.5).max(car.s.min(l.length - 0.5));
            }
        }
    }
}
