//! [driving] Crashes: red lights run, collisions, hazards, and cars pulled
//! out of a pile-up.

use super::*;

impl Traffic {
    /// Take car `f` out of its lane to wait at the side and pull back in
    /// when there is room, like a car leaving a space.
    pub(super) fn pull_aside_from_pile(&mut self, f: CarId) -> bool {
        let lane = self.cars[f].route[self.cars[f].idx];
        self.net.lanes[lane as usize].cars.retain(|&o| o != f);
        let kerb = self.net.lanes[lane as usize].right.is_none();
        let c = &mut self.cars[f];
        c.s = 0.0;
        c.v = 0.0;
        c.entering = true;
        c.wait = Wait::Entering;
        c.blocked_by = None;
        if kerb {
            c.lateral = LANE_WIDTH * 0.6;
        }
        true
    }

    /// Put car `f` back `-s` metres before the start of its lane, on the
    /// lane behind it (the one it came from, else the longest that feeds
    /// it), moving at most `v`. False when no lane leads into this one.
    pub(super) fn back_a_lane(&mut self, f: CarId, s: f32, v: f32) -> bool {
        let (lane, idx) = (self.cars[f].route[self.cars[f].idx], self.cars[f].idx);
        let came = idx.checked_sub(1).map(|i| self.cars[f].route[i]).filter(|p| self.net.lanes[lane as usize].prev.contains(p));
        let Some(prev) = came.or_else(|| {
            self.net.lanes[lane as usize].prev.iter().copied().max_by(|a, b| self.net.lanes[*a as usize].length.total_cmp(&self.net.lanes[*b as usize].length).then(a.cmp(b)))
        }) else {
            return false;
        };
        self.net.lanes[lane as usize].cars.retain(|&o| o != f);
        let len = self.net.lanes[prev as usize].length;
        let c = &mut self.cars[f];
        if came.is_some() {
            c.idx -= 1;
        } else {
            c.route.insert(idx, prev);
        }
        c.s = (len + s).max(0.0);
        c.v = c.v.min(v);
        self.insert_sorted(prev, f);
        true
    }

    /// The next number from the traffic model's own random stream
    /// (splitmix64), `0..1`. Only profiled drivers draw from it.
    pub(super) fn roll(&mut self) -> f32 {
        self.rng = self.rng.wrapping_add(crate::hash::PHI);
        crate::hash::unit(crate::hash::finish(self.rng))
    }

    /// Would this driver go through the red on `nl`, `dist` metres ahead?
    /// Anyone too close to stop goes through a fresh amber; the less a
    /// driver cares for the rules, the later into the red they keep going;
    /// a takeover pack never stops.
    pub(super) fn runs_red(car: &Car, nl: &Lane, dist: f32) -> bool {
        let p = &car.drive.profile;
        if !p.assigned || car.siren {
            return false;
        }
        if p.ignores_signals {
            return true;
        }
        let to_line = (dist - 1.0).max(0.1);
        let cannot_stop = car.v * car.v > 2.0 * 3.5 * to_line;
        // Only a decision taken in the last couple of seconds before the line.
        dist < car.v * 2.0 + 2.0 && ((nl.red_for < AMBER && cannot_stop && car.v > 3.0) || (car.v > 5.0 && nl.red_for < (1.0 - p.compliance) * 8.0))
    }

    /// A car just drove onto connector `conn`. On a red, cross traffic in
    /// the junction may hit it.
    pub(super) fn entered_red(&mut self, conn: LaneId, id: CarId) {
        let lane = &self.net.lanes[conn as usize];
        // Into the junction past the amber (the first `AMBER` seconds of red
        // are clearance: going through then is legal, the cross street still
        // has red).
        let ran = lane.is_connector() && !lane.green && lane.red_for > AMBER && !self.cars[id].siren;
        self.cars[id].drive.running = None;
        if !ran || !self.cars[id].drive.profile.assigned {
            return;
        }
        self.red_runs += 1;
        let LaneKind::Connector { node, approach } = lane.kind else { return };
        let Some(inter) = self.net.node_intersection.get(node.0 as usize).and_then(|&i| self.net.intersections.get(i)) else { return };
        let here = self.cars[id].pos;
        let other = inter
            .connectors
            .iter()
            .filter(|&&(l, ap)| ap != approach && self.net.lanes[l as usize].green)
            .flat_map(|&(l, _)| self.net.lanes[l as usize].cars.iter().copied())
            .filter(|&o| o != id && self.cars[o].v > 2.0 && !self.cars[o].crashed())
            .min_by(|&a, &b| self.cars[a].pos.distance(here).total_cmp(&self.cars[b].pos.distance(here)));
        let Some(other) = other else { return };
        let p = &self.cars[id].drive.profile;
        let chance = 0.12 + 0.35 * p.drunk + 0.2 * (1.0 - p.skill);
        if self.roll() < chance {
            let speed = (self.cars[id].v + self.cars[other].v) * 0.7;
            self.crash(vec![id, other], CrashCause::RedLight, speed);
        }
    }

    /// Cars that ran into the one ahead on their lane: a crash when they
    /// closed fast, else they just stop touching. Only profiled drivers can
    /// get there at all (they cannot brake harder than `MAX_BRAKE`).
    pub(super) fn collisions(&mut self) {
        let mut hits: Vec<(CarId, CarId, f32)> = Vec::new();
        let mut bumps: Vec<(CarId, f32, f32)> = Vec::new();
        for lane in &self.net.lanes {
            for w in lane.cars.windows(2) {
                let (f, l) = (&self.cars[w[0]], &self.cars[w[1]]);
                // [services] Nor into a car pulled onto the shoulder for a
                // siren: the one behind (often tailgating the siren as it
                // came through) squeezes past or settles behind it.
                // [emergency] Nor while either is out of the lane on an
                // off-law move (overtaking on the wrong side, on the kerb).
                if !f.drive.profile.assigned || f.passing || f.siren || f.crashed() || f.lateral.abs() > 1.0 || l.lateral >= SIREN_PULL_ASIDE * 0.9 || f.drive.offlaw.side.abs() > 1.0 || l.drive.offlaw.side.abs() > 1.0 {
                    continue;
                }
                let gap = l.s - l.length() - f.s;
                if gap >= 0.0 {
                    continue;
                }
                let dv = f.v - l.v;
                // Squeezed together by a merge or a lane change (the model
                // placing them, not the driver): no crash, they just part.
                let settled = f.drive.on_lane > 2.0 && l.drive.on_lane > 2.0;
                if dv > BUMP_SPEED && settled {
                    hits.push((w[0], w[1], dv));
                } else {
                    bumps.push((w[0], l.s - l.length() - 0.1, l.v));
                }
            }
        }
        for (f, s, v) in bumps {
            // Squeezed back past the start of its lane (a merge with no room
            // behind, or a pile left by an older build): it cannot stand on
            // the car ahead, so it goes back onto the end of the lane behind
            // it, still in traffic. Clamped at the lane start instead, cars
            // piled up there on each other and rings of short pieces locked.
            // Squeezed a little (a merge): back onto the lane behind, still in
            // traffic. Standing right on another car (a pile): out of the
            // lane, rejoining when there is room, so a jam full of them
            // thins out instead of packing solid again.
            if s < -PILE_OVERLAP && self.pull_aside_from_pile(f) {
                continue;
            }
            if s < 0.0 && self.back_a_lane(f, s, v) {
                continue;
            }
            let c = &mut self.cars[f];
            c.s = s.max(0.0).min(c.s);
            c.v = c.v.min(v);
        }
        for (f, l, dv) in hits {
            if self.cars[f].crashed() {
                continue;
            }
            self.crash(vec![f, l], CrashCause::RearEnd, dv);
        }
    }

    /// Chance mishaps: a drunk running off the road, a weaving car
    /// swiping the one alongside.
    pub(super) fn hazards(&mut self, dt: f32) {
        let ids: Vec<CarId> = self.cars.iter().filter(|(_, c)| c.drive.profile.assigned && !c.crashed() && !c.in_stunt() && !c.entering && c.v > 6.0).map(|(id, _)| id).collect();
        for id in ids {
            if self.cars[id].crashed() {
                continue;
            }
            let (drunk, v, weave, rough) = {
                let c = &self.cars[id];
                (c.drive.profile.drunk, c.v, c.drive.weave, self.net.lanes[c.route[c.idx] as usize].rough)
            };
            // [events] Potholes and ruts: the same mishaps, a little likelier.
            let worse = crate::road_wear::crash_factor(rough);
            if drunk > 0.05 && self.roll() < 0.0012 * drunk * drunk * (v / 10.0) * dt * worse {
                self.crash(vec![id], CrashCause::OffRoad, v);
                continue;
            }
            // Misjudging the gap to the car ahead: the less skill, the likelier.
            let (skill, lead) = {
                let c = &self.cars[id];
                let lane = &self.net.lanes[c.route[c.idx] as usize];
                let k = lane.cars.iter().position(|&o| o == id);
                (c.drive.profile.skill, k.and_then(|k| lane.cars.get(k + 1)).copied())
            };
            if let Some(l) = lead {
                let (lc, c) = (&self.cars[l], &self.cars[id]);
                let gap = lc.s - lc.length() - c.s;
                let dv = c.v - lc.v;
                if dv > 2.0 && gap < c.v * 0.8 && !lc.in_stunt() && self.roll() < 0.015 * (1.0 - skill) * (1.0 - skill) * dt * worse {
                    self.crash(vec![id, l], CrashCause::RearEnd, dv);
                    continue;
                }
            }
            if weave.abs() > 0.9 {
                let c = &self.cars[id];
                let lane = &self.net.lanes[c.route[c.idx] as usize];
                let side = if weave > 0.0 { lane.right } else { lane.left };
                let s = c.s;
                let along = side.and_then(|l| self.net.lanes[l as usize].cars.iter().copied().find(|&o| (self.cars[o].s - s).abs() < 4.5 && !self.cars[o].crashed()));
                if let Some(o) = along {
                    if self.roll() < 0.6 * dt {
                        let speed = (v - self.cars[o].v).abs().max(4.0);
                        self.crash(vec![id, o], CrashCause::Sideswipe, speed);
                    }
                }
            }
        }
    }

    /// Stop the cars and file the crash for the world.
    pub fn crash(&mut self, cars: Vec<CarId>, cause: CrashCause, speed: f32) {
        let id = self.next_crash;
        self.next_crash += 1;
        let drunk = cars.iter().map(|&c| self.cars[c].drive.profile.drunk).fold(0.0, f32::max);
        let luck = 0.7 + 0.6 * self.roll();
        let severity = (speed / 16.0 * (1.0 + 0.3 * drunk) * luck).clamp(0.05, 1.0);
        let wrecked = severity >= BAD_CRASH;
        let pos = self.cars[cars[0]].pos;
        for &c in &cars {
            let car = &mut self.cars[c];
            if car.crashed() {
                continue;
            }
            car.v = 0.0;
            car.passing = false;
            car.drive.running = None;
            car.drive.crash = Some(CrashState { id, hold: if wrecked { f32::MAX } else { 60.0 + 150.0 * severity }, wrecked });
        }
        self.crashes.push(CrashReport { id, cars, pos, speed, cause, severity });
    }
}
