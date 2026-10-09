//! One substep of the car model, in the order `Traffic::step` runs it:
//! the kerb claims, every car's acceleration worked out before any car
//! moves (`plan_substep`), the moves applied (`apply_steps`), cars pulled
//! out from the kerb, lanes advanced (`advance_lanes`) and passers
//! reordered.

use super::*;

/// One car's physics for a substep, worked out before any car moves.
pub(super) struct Step {
    pub(super) id: CarId,
    /// The vehicle holding it back, if one is.
    pub(super) by: Option<CarId>,
    pub(super) acc: f32,
    pub(super) why: Wait,
    pub(super) left_wait: bool,
    /// The leader's speed as the driver now perceives it.
    pub(super) seen: f32,
    /// Committed to running this red.
    pub(super) run: Option<LaneId>,
    /// [scene] How far sideways to ease round a scene.
    pub(super) dodge: f32,
}

/// Kerb spots claimed this substep, per lane: `(front at, length, car)`.
pub(super) type Claims = HashMap<LaneId, Vec<(f32, f32, CarId)>>;

impl Traffic {
    /// Kerb positions where a car has waited long enough to be let in.
    pub(super) fn kerb_claims(&self) -> Claims {
        // Each claim: where the kerb car's front will be, how long it is
        // and who it is. A spot near the start of a lane also holds up
        // traffic still on the lanes feeding it.
        let mut claims: Claims = HashMap::new();
        // A car stopped at the end of its lane for the lane change its
        // turn needs is let in the same way (the zip: the driver behind
        // in the next lane leaves it a gap as the queue moves up).
        // Without it a car in a slow queue waited for a gap that the
        // queue beside it never left, and blocked its own lane meanwhile.
        for (cid, c) in self.cars.iter().filter(|(_, c)| !c.entering && c.wait == Wait::LaneChange && c.stalled > COURTESY_AFTER) {
            if let Some(target) = c.change_pending(&self.net) {
                claims.entry(target).or_default().push((c.s.min(self.net.lanes[target as usize].length), c.length(), cid));
            }
        }
        for (cid, c) in self.cars.iter().filter(|(_, c)| c.entering && c.stalled > COURTESY_AFTER) {
            let lane = c.route[c.idx];
            claims.entry(lane).or_default().push((c.s, c.length(), cid));
            if c.s < c.length() + IDM_MIN_GAP + 30.0 {
                for &p in &self.net.lanes[lane as usize].prev {
                    claims.entry(p).or_default().push((c.s + self.net.lanes[p as usize].length, c.length(), cid));
                }
            }
        }
            claims
    }

    /// Every car's acceleration for this substep, worked out before any
    /// car moves.
    pub(super) fn plan_substep(&self, dt: f32, claims: &Claims, zones: &[SceneZone]) -> Vec<Step> {
        let mut accs: Vec<Step> = Vec::with_capacity(self.cars.len());
        for (lane_id, lane) in self.net.lanes.iter().enumerate() {
            let lane_claims = claims.get(&(lane_id as LaneId));
            for (k, &cid) in lane.cars.iter().enumerate() {
                let car = &self.cars[cid];
                if car.hold > 0.0 {
                    accs.push(Step { by: None, id: cid, acc: -1000.0, why: Wait::Dwell, left_wait: false, seen: 0.0, run: None, dodge: 0.0 });
                    continue;
                }
                if car.crashed() {
                    accs.push(Step { by: None, id: cid, acc: -1000.0, why: Wait::Crashed, left_wait: false, seen: 0.0, run: None, dodge: car.drive.dodge });
                    continue;
                }
                let pending = car.change_pending(&self.net).is_some();
                // Passing a left turner: once out on the shoulder, the
                // turner is no longer in the way.
                let beside = car.passing
                    && Self::pass_offset(lane).is_some_and(|o| car.lateral >= 0.8 * o)
                    && lane.cars.get(k + 1).is_some_and(|&l| self.next_turn(&self.cars[l]) == Some(-1));
                let lead = lane.cars.get(if beside { k + 2 } else { k + 1 });
                let mut run = None;
                let (mut gap, mut lead_v, mut why) = match lead {
                    Some(&lead) => (self.cars[lead].s - car.s - self.cars[lead].length(), self.cars[lead].v, Wait::Leader),
                    None if pending => (LOOKAHEAD * 2.0, ROAD_SPEED, Wait::Free),
                    None => {
                        let (g, v, w, r) = self.lookahead(car, lane);
                        run = r;
                        (g, v, w)
                    }
                };
                if car.passing && matches!(why, Wait::Signal | Wait::ExitFull | Wait::Yield) {
                    // On the shoulder: stop short of the crossing.
                    gap -= CROSSING_SETBACK;
                }
                // [emergency] Lights and sirens (and whoever flees them):
                // past what their move takes them round, no red light.
                if car.offlaw() || car.passing_hazard() {
                    (gap, lead_v, why) = self.offlaw_gap(car, gap, lead_v, why);
                }
                if pending {
                    // Stop at the end of the lane until the change is made.
                    let to_end = lane.length - car.s - 1.0 + IDM_MIN_GAP;
                    if to_end < gap {
                        (gap, lead_v, why) = (to_end, 0.0, Wait::LaneChange);
                    }
                } else if car.idx + 1 == car.route.len() {
                    let to_stop = car.s_end + IDM_MIN_GAP - car.s;
                    if to_stop < gap {
                        (gap, lead_v, why) = (to_stop, 0.0, Wait::Arriving);
                    }
                }
                // The first driver behind a waiting car stops short to let it out.
                let mut by: Option<CarId> = None;
                if let Some(cl) = lane_claims {
                    let ahead = lane.cars.get(k + 1).map_or(f32::MAX, |&l| self.cars[l].s);
                    // ([services] Lights and sirens do not stop to let a
                    // car change lanes in front of them.)
                    for &(at, len, owner) in cl.iter().filter(|&&(at, _, o)| at > car.s + 1.0 && at < ahead - 1.0 && !(car.siren && !self.cars[o].entering)) {
                        // Room for the whole vehicle (a bus is 12 m), not a car.
                        let room = at - len - IDM_MIN_GAP - car.s;
                        if room < gap && car.v * car.v < 2.0 * IDM_BRAKE * room.max(0.0) {
                            (gap, lead_v, why) = (room + IDM_MIN_GAP, 0.0, Wait::Leader);
                            by = Some(owner);
                        }
                    }
                }
                // [pedestrians] Someone on a crossing ahead, or the stop line
                // (sirens included: they slow for people in the road).
                if let Some(g) = self.crosswalk_gap(car).filter(|&g| g < gap) {
                    (gap, lead_v, why) = (g, 0.0, Wait::Yield);
                }
                // [rail-crossing] A level crossing ahead with its lights on.
                if self.net.rail_closed_any {
                    if let Some(g) = self.rail_gap(car, lane).filter(|&g| g < gap) {
                        (gap, lead_v, why) = (g, 0.0, Wait::RailCrossing);
                    }
                }
                // [scene] Something at the roadside: slow, ease round
                // it, or stop short where there is no room.
                let ask = if zones.is_empty() { Default::default() } else { self.scene_ask(car, lane, &zones) };
                if let Some(g) = ask.stop.filter(|&g| g < gap) {
                    (gap, lead_v, why) = (g, 0.0, Wait::Blocked);
                }
                if why == Wait::Leader && gap > 50.0 {
                    why = Wait::Free;
                }
                if by.is_none() {
                    by = match (why, lead) {
                        (Wait::Leader, Some(&l)) => Some(l),
                        (Wait::Leader | Wait::ExitFull, None) => self.blocker_ahead(car, why),
                        _ => None,
                    };
                }
                let left_wait = car.v < 0.5 && self.next_turn(car) == Some(-1);
                // [driving] The driver sees the car ahead change speed
                // only after their reaction time.
                let reaction = car.drive.profile.reaction;
                let seen = if reaction > 0.0 { car.drive.seen_lead_v + (lead_v - car.drive.seen_lead_v) * (dt / reaction).min(1.0) } else { lead_v };
                // The limit here, or a lower one ahead the driver is
                // already braking for; [tolls] slow through a toll booth.
                let mut v0 = car.desired_speed(self.limit_ahead(car, lane));
                if self.net.tolls && !car.siren {
                    if let Some(cap) = self.toll_cap(car) {
                        v0 = v0.min(cap);
                    }
                }
                if let Some(cap) = Self::offlaw_cap(car) {
                    v0 = v0.min(cap);
                }
                if let Some(cap) = ask.cap {
                    v0 = v0.min(cap);
                }
                accs.push(Step { by, id: cid, acc: car.idm(v0, gap, car.v - seen), why, left_wait, seen, run, dodge: ask.dodge });
            }
        }
            accs
    }

    /// Apply the planned accelerations: speeds, positions, waits, and the
    /// sideways drift of passes and lane changes.
    pub(super) fn apply_steps(&mut self, accs: Vec<Step>, dt: f32) {
        for st in accs {
            let car = &mut self.cars[st.id];
            Self::ease_dodge(car, st.dodge, dt);
            car.wait = st.why;
            car.blocked_by = st.by;
            car.drive.seen_lead_v = st.seen;
            if st.run.is_some() {
                car.drive.running = st.run;
            }
            if st.left_wait {
                car.turn_wait += dt;
                if st.why == Wait::Yield {
                    car.yielded += dt;
                }
            }
            car.cooldown = (car.cooldown - dt).max(0.0);
            if let Some(cr) = car.drive.crash.as_mut() {
                car.v = 0.0;
                car.stalled = 0.0;
                if !cr.wrecked {
                    cr.hold -= dt;
                    if cr.hold <= 0.0 {
                        car.drive.crash = None;
                    }
                }
                continue;
            }
            if car.hold > 0.0 {
                car.hold -= dt;
                car.v = 0.0;
                car.stalled = 0.0;
                continue;
            }
            // [driving] A real driver cannot brake infinitely hard.
            let acc = if car.drive.profile.assigned { st.acc.max(-MAX_BRAKE) } else { st.acc };
            car.v = (car.v + acc * dt).max(0.0);
            car.s += car.v * dt;
            car.stalled = if car.v < 0.5 { car.stalled + dt } else { 0.0 };
            if car.v >= 0.5 {
                car.replans = 0;
            }
            if car.v < 0.3 {
                // Stopped after all: the next light is a new decision.
                car.drive.running = None;
            }
            if car.passing {
                // Out onto the shoulder (the offset was checked against
                // the kerb when the pass began).
                let target = CAR_WIDTH + 0.25;
                let rate = PASS_LATERAL_SPEED * (car.v / 3.0).clamp(0.5, 1.0) * dt;
                car.lateral = (car.lateral + rate).min(target.max(car.lateral));
            } else if car.glide_from != 0.0 && (car.lateral - car.glide_at()).abs() < 0.05 {
                // Eased out and back in along a half cosine (a car
                // standing mid-change still creeps across).
                car.glide_done += car.v.max(GLIDE_CREEP) * dt;
                car.lateral = car.glide_at();
                if car.glide_done >= car.glide_len {
                    car.lateral = 0.0;
                    car.glide_from = 0.0;
                }
            } else if car.lateral != 0.0 {
                // Settling back from a nudge (pulled aside for a siren,
                // back from the shoulder): no glide under way.
                car.glide_from = 0.0;
                let rate = LC_LATERAL_SPEED * (car.v / 3.0).clamp(0.3, 1.0) * dt;
                car.lateral -= car.lateral.signum() * car.lateral.abs().min(rate);
            }
        }
    }

    /// Cars at the kerb pull out into a gap, or wait (counting the stall).
    pub(super) fn pull_out_waiting(&mut self, dt: f32) {
        let waiting: Vec<CarId> = self.cars.iter().filter(|(_, c)| c.entering).map(|(id, _)| id).collect();
        for id in waiting {
            // Standing at the kerb on purpose (unloading): not waiting yet.
            if self.cars[id].hold > 0.0 {
                let c = &mut self.cars[id];
                c.hold -= dt;
                c.v = 0.0;
                c.stalled = 0.0;
                c.wait = Wait::Dwell;
                continue;
            }
            if !self.pull_out(id) {
                let c = &mut self.cars[id];
                c.stalled += dt;
                c.wait = Wait::Entering;
            }
        }
    }

    /// Arrivals, the half-hour replan, and cars on into their next lanes.
    pub(super) fn advance_lanes(&mut self, arrived: &mut Vec<CarId>) {
        let ids: Vec<CarId> = self.cars.keys().collect();
        for cid in ids {
            if self.cars[cid].in_stunt() {
                // [driving] In a takeover show, off the lanes.
                self.cars[cid].age = 0.0;
                continue;
            }
            // [carlife] A trip this long is stuck somewhere: look for
            // another way from where it stands. Nobody is taken off
            // the road; a jam is part of the city and its drivers feel it.
            if self.cars[cid].age > REPLAN_AFTER {
                self.cars[cid].age = 0.0;
                if !self.cars[cid].entering && self.cars[cid].idx + 1 < self.cars[cid].route.len() {
                    self.replan_from_here(cid);
                }
            }
            if self.cars[cid].entering {
                continue;
            }
            // Read after the replan: it swaps the route and resets `idx`,
            // so a snapshot taken before would test the old index against
            // the new route (and could "arrive" a car blocks from its goal).
            let (lane_id, idx, len, s, s_end) = {
                let c = &self.cars[cid];
                let lane_id = c.route[c.idx];
                (lane_id, c.idx, self.net.lanes[lane_id as usize].length, c.s, c.s_end)
            };
            if idx + 1 == self.cars[cid].route.len() {
                if s >= s_end - 0.5 {
                    let c = &mut self.cars[cid];
                    c.s = s_end.min(len);
                    c.v = 0.0;
                    // [driving] A takeover car waits at the meet for the show.
                    if c.drive.gang.is_some_and(|g| matches!(g.phase, GangPhase::Joining | GangPhase::Racing)) {
                        continue;
                    }
                    if !arrived.contains(&cid) {
                        arrived.push(cid);
                    }
                }
            } else if s >= len {
                // On through as many lanes as the step covered: a fast
                // car crosses a short connector or curve piece within
                // one step. Taking one lane a step left it standing at
                // the end of each short lane (drawn there) and then
                // springing ahead with the backlog: the stutter along
                // curved ramps.
                let (mut lane_id, mut len) = (lane_id, len);
                loop {
                    if self.cars[cid].change_pending(&self.net).is_some() {
                        // Never roll off the end into the lane beside.
                        let c = &mut self.cars[cid];
                        c.s = len - 0.05;
                        c.v = 0.0;
                        break;
                    }
                    // Never land on (or ahead of) the tail of the last
                    // vehicle into the next lane: it would be shoved back
                    // there onto whoever stands behind, and with nowhere
                    // behind the lane start cars piled up on it (rings of
                    // short segments locked solid that way). Wait at the
                    // end of this lane until there is room. (Lights and
                    // sirens squeeze past as they always have.)
                    let (at, idx_now, siren) = (self.cars[cid].s, self.cars[cid].idx, self.cars[cid].siren);
                    if let Some(&next) = self.cars[cid].route.get(idx_now + 1).filter(|_| !siren) {
                        // [driving] Nor a crash on the shoulder it is
                        // easing past (`traffic/hazard.rs`).
                        let passing = self.cars[cid].passing_hazard();
                        if let Some(&tail) = self.net.lanes[next as usize].cars.first().filter(|&&t| !(passing && self.is_hazard(&self.cars[t]))) {
                            let room = self.cars[tail].s - self.cars[tail].length() - 0.1;
                            if at - len > room {
                                let tv = self.cars[tail].v;
                                let c = &mut self.cars[cid];
                                c.s = len - 0.05;
                                c.v = c.v.min(tv);
                                break;
                            }
                        }
                    }
                    self.net.lanes[lane_id as usize].cars.retain(|&c| c != cid);
                    let next = {
                        let c = &mut self.cars[cid];
                        c.s -= len;
                        c.idx += 1;
                        c.passing = false;
                        c.route[c.idx]
                    };
                    self.entered(lane_id, next, cid);
                    self.insert_sorted(next, cid);
                    self.entered_red(next, cid);
                    // [tolls] Came onto a tolled lane from behind: it will
                    // pay once past the booth line (even if already past it
                    // after this substep's hop).
                    if let Some(t) = self.net.lanes[next as usize].toll {
                        let c = &mut self.cars[cid];
                        if c.toll_booth != t.booth {
                            c.toll_booth = t.booth;
                            c.toll_paid = false;
                        }
                    }
                    let c = &self.cars[cid];
                    let next_len = self.net.lanes[next as usize].length;
                    if c.idx + 1 >= c.route.len() || c.s < next_len {
                        break;
                    }
                    (lane_id, len) = (next, next_len);
                }
            }
        }
    }

    /// A car that drew level with the turner it passes goes ahead of it in
    /// the lane's order.
    pub(super) fn reorder_passers(&mut self) {
        let passers: Vec<CarId> = self.cars.iter().filter(|(_, c)| c.passing).map(|(id, _)| id).collect();
        for id in passers {
            let lane = self.cars[id].route[self.cars[id].idx];
            let cars = &mut self.net.lanes[lane as usize].cars;
            if let Some(k) = cars.iter().position(|&c| c == id) {
                if k + 1 < cars.len() && self.cars[cars[k + 1]].s < self.cars[id].s {
                    cars.swap(k, k + 1);
                }
            }
        }
    }
}
