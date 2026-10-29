//! The kerb: cars set off from it, pull in to it and pull out of it into
//! a gap in the lane.

use super::*;

impl Traffic {
    pub(crate) fn insert_sorted(&mut self, lane: LaneId, car: CarId) {
        let s = self.cars[car].s;
        let list = &mut self.net.lanes[lane as usize].cars;
        let pos = list.iter().position(|&other| self.cars[other].s > s).unwrap_or(list.len());
        list.insert(pos, car);
    }

    pub fn spawn_car(&mut self, citizen: CitizenId, cargo: Option<Cargo>, route: Vec<LaneId>, s_start: f32, s_end: f32, color: u8) -> CarId {
        let lane = route[0];
        let (pos, dir) = self.net.lanes[lane as usize].point_at(s_start);
        let heading = dir.y.atan2(dir.x);
        let id = self.cars.insert(Car { citizen, cargo, route, idx: 0, s: s_start, s_end, v: 0.0, pos, heading, prev_pos: pos, prev_heading: heading, color, age: 0.0, bus: None, hold: 0.0, lateral: 0.0, stalled: 0.0, wait: Wait::Free, cooldown: 0.0, entering: false, blocked_by: None, replans: 0, avoided: Vec::new(), passing: false, turn_wait: 0.0, yielded: 0.0, siren: false, model: Default::default(), owned: None, errand: Default::default(), drive: DriveState::default(), toll_booth: 0, toll_paid: false, occupants: Vec::new(), glide_from: 0.0, glide_len: 0.0, glide_done: 0.0 });
        self.pull_out(id);
        // [toll-inspect] Did it set off round a booth? (The world looks,
        // once it knows whether this is a bus or a city vehicle.)
        if self.net.tolls {
            self.toll_checks.push((id, s_start));
        }
        id
    }

    /// Pull a vehicle standing in its lane over to the kerb for `secs`
    /// (a lorry unloading, a van at a job): out of the lane so traffic
    /// passes, drawn at the kerb, and afterwards it pulls out like a car
    /// leaving a space. Its route from here must already be set.
    pub fn pull_in(&mut self, id: CarId, secs: f32) {
        let Some(c) = self.cars.get(id) else { return };
        let lane = c.route[c.idx];
        let kerb = self.net.lanes[lane as usize].right.is_none();
        let was_entering = c.entering;
        if !was_entering {
            self.net.lanes[lane as usize].cars.retain(|&o| o != id);
        }
        // [r4-flags-look] (flag day 13 at (-589,89), "stacked trucks")
        // Pulled in behind (or ahead of) whoever already stands at this
        // kerb, never on top of them: every lorry for one site stopped at
        // the same spot, eight drawn as one.
        if kerb && !was_entering {
            let s = self.kerb_room(lane, id);
            self.cars[id].s = s;
        }
        let c = &mut self.cars[id];
        c.entering = true;
        c.v = 0.0;
        c.hold = c.hold.max(secs);
        c.stalled = 0.0;
        c.wait = Wait::Dwell;
        c.blocked_by = None;
        if kerb {
            c.lateral = LANE_WIDTH * 0.6;
        }
    }

    /// A new car joins its first lane only where there is room (no overlap
    /// with the vehicles there, nobody forced to brake hard); otherwise it
    /// waits at the kerb and tries again every step. After `COURTESY_AFTER`
    /// the next driver stops short for it (see the claims in `step`) and it
    /// commits as soon as its spot is physically clear; if standing traffic
    /// keeps the spot covered for `KERB_SLIDE_AFTER` it pulls out into the
    /// nearest real gap behind instead. So the kerb can never deadlock.
    pub(super) fn pull_out(&mut self, id: CarId) -> bool {
        let lane = self.cars[id].route[self.cars[id].idx];
        let (waited, len, s) = (self.cars[id].stalled, self.cars[id].length(), self.cars[id].s);
        // [services] Lights and sirens pull straight out; traffic yields.
        let mut room = self.cars[id].siren || self.change_ok(id, lane, LC_SAFE_BRAKE).is_some();
        let (leader, follower) = self.neighbours_at(lane, s.min(self.net.lanes[lane as usize].length), len, id);
        if !room && waited > COURTESY_AFTER {
            // Committed: only real overlap, or a follower too fast to stop, holds it.
            let clear_ahead = leader.is_none_or(|(_, g)| g >= 0.5);
            let clear_behind = follower.is_none_or(|(f, g)| g >= 0.5 && self.cars[f].v * self.cars[f].v <= 2.0 * LC_FORCED_BRAKE * g);
            room = clear_ahead && clear_behind;
        }
        if !room && waited > KERB_SLIDE_AFTER {
            if let Some(at) = self.kerb_gap(lane, s, len, id) {
                self.cars[id].s = at;
                room = true;
            }
        }
        let blocker = match (leader, follower) {
            (Some((l, g)), _) if g < 0.5 => Some(l),
            (_, Some((f, _))) => Some(f),
            (Some((l, _)), None) => Some(l),
            _ => None,
        };
        let kerb = self.net.lanes[lane as usize].right.is_none();
        let c = &mut self.cars[id];
        if room {
            if c.entering {
                c.entering = false;
                c.cooldown = LC_COOLDOWN;
            }
            c.blocked_by = None;
            self.insert_sorted(lane, id);
        } else {
            c.blocked_by = blocker;
            if !c.entering {
                c.entering = true;
                c.wait = Wait::Entering;
                // Drawn pulled in at the kerb until it can go.
                if kerb {
                    c.lateral = LANE_WIDTH * 0.6;
                }
            }
        }
        room
    }

    /// [r4-flags-look] Where on `lane` vehicle `id` can stand pulled in at
    /// the kerb (the position of its front) without overlapping the others
    /// already standing there: where it is if that is clear, else the
    /// nearest clear spot behind it (a metre apart), else ahead of them, else
    /// where it is (a lane too short for all of them).
    pub(super) fn kerb_room(&self, lane: LaneId, id: CarId) -> f32 {
        const GAP: f32 = 1.0;
        let (s, len) = (self.cars[id].s, self.cars[id].length());
        let lane_len = self.net.lanes[lane as usize].length;
        let mut taken: Vec<(f32, f32)> = self
            .cars
            .iter()
            .filter(|&(o, c)| o != id && c.entering && c.route.get(c.idx) == Some(&lane))
            .map(|(_, c)| (c.s - c.length(), c.s))
            .collect();
        if taken.is_empty() {
            return s;
        }
        taken.sort_by(|a, b| a.0.total_cmp(&b.0));
        let clear = |f: f32| taken.iter().all(|&(t0, t1)| f - len >= t1 + GAP || f + GAP <= t0);
        if clear(s) {
            return s;
        }
        // Behind: just short of the tail of each one standing behind us.
        let mut back: Vec<f32> = taken.iter().map(|&(t0, _)| t0 - GAP).filter(|&f| f <= s && f >= len).collect();
        back.sort_by(|a, b| b.total_cmp(a));
        if let Some(f) = back.into_iter().find(|&f| clear(f)) {
            return f;
        }
        let mut ahead: Vec<f32> = taken.iter().map(|&(_, t1)| t1 + GAP + len).filter(|&f| f >= s && f <= lane_len).collect();
        ahead.sort_by(|a, b| a.total_cmp(b));
        ahead.into_iter().find(|&f| clear(f)).unwrap_or(s)
    }

    /// The nearest place behind `s` on `lane` (within `KERB_SLIDE_REACH`)
    /// where a vehicle of `len` fits between standing or slow traffic with
    /// half a metre each side, as the position of its front.
    pub(super) fn kerb_gap(&self, lane: LaneId, s: f32, len: f32, skip: CarId) -> Option<f32> {
        let l = &self.net.lanes[lane as usize];
        let cars: Vec<&Car> = l.cars.iter().filter(|&&c| c != skip).map(|&c| &self.cars[c]).collect();
        // Spaces between consecutive vehicles (and before the first), back to front.
        let mut ahead_tail = f32::MAX;
        for k in (0..=cars.len()).rev() {
            let (behind_front, behind_v) = if k == 0 { (0.0f32, 0.0) } else { (cars[k - 1].s, cars[k - 1].v) };
            let front = (ahead_tail - 0.5).min(s).min(l.length);
            if front <= s && s - front <= KERB_SLIDE_REACH && front - len - 0.5 >= behind_front && behind_v < 2.0 && front >= len {
                return Some(front);
            }
            if k > 0 {
                ahead_tail = cars[k - 1].s - cars[k - 1].length();
                if s - ahead_tail > KERB_SLIDE_REACH + len {
                    break;
                }
            }
        }
        None
    }
}
