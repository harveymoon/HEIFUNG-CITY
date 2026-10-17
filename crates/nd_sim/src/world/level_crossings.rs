//! [rail-crossing] Level crossings in the world: each tick, before the
//! traffic moves, every crossing reads the trains coming (how many seconds
//! off, or over it now), runs its cycle (`crate::level_crossing`) and opens
//! or closes its connector lanes to vehicles. Trains ask whether they may
//! run onto a crossing (`crossing_hold`); walkers and riders whether their
//! next step would take them onto a closed one (`rail_stop`).

use super::*;
use crate::level_crossing::{LevelCrossing, HELD_EXTRA, PASS_WINDOW};
use crate::rail::{project_on_poly, Train, TrainId};
use crate::traffic::Wait;

/// A crossing on a train's route: where along the route its box starts and
/// ends (the head enters at `s_in`, the tail leaves at `s_out`).
pub(super) struct OnRoute {
    pub(super) k: usize,
    pub(super) s_in: f32,
    pub(super) s_out: f32,
}

impl World {
    /// The crossings `t` runs over, ahead or behind.
    pub(super) fn crossings_on(&self, t: &Train) -> Vec<OnRoute> {
        self.level_crossings
            .list
            .iter()
            .enumerate()
            .filter_map(|(k, c)| {
                let (s, d) = project_on_poly(&t.poly, c.pos)?;
                (d < 3.0).then_some(OnRoute { k, s_in: s - c.reach - 1.0, s_out: s + c.reach + 1.0 })
            })
            .collect()
    }

    /// No vehicle in the crossing's box (on its connector lanes).
    fn crossing_box_clear(&self, c: &LevelCrossing) -> bool {
        let net = &self.traffic.net;
        let Some(inter) = net.node_intersection.get(c.node.0 as usize).and_then(|&i| net.intersections.get(i)) else { return true };
        inter.connectors.iter().all(|&(l, _)| net.lanes[l as usize].cars.is_empty())
    }

    /// Run every crossing's cycle for this tick and close (or open) its
    /// lanes to vehicles. Called at the start of the traffic step.
    pub(super) fn step_level_crossings(&mut self) {
        if self.level_crossings.built_for != Some(self.roads.version) {
            self.level_crossings.rebuild(&self.roads);
        }
        if self.level_crossings.list.is_empty() {
            if self.traffic.net.rail_closed_any {
                for l in self.traffic.net.lanes.iter_mut() {
                    l.rail_closed = false;
                }
                self.traffic.net.rail_closed_any = false;
            }
            self.level_crossings.any_shut = false;
            return;
        }
        let dt = self.config.traffic_seconds_per_tick;
        let tick = self.clock.tick;
        let n = self.level_crossings.list.len();
        // The vehicles standing at each one after the last traffic step.
        {
            let net = &self.traffic.net;
            let cars = &self.traffic.cars;
            for c in self.level_crossings.list.iter_mut() {
                c.vehicles_waiting = 0;
                if !c.shut() {
                    continue;
                }
                let Some(inter) = net.node_intersection.get(c.node.0 as usize).and_then(|&i| net.intersections.get(i)) else { continue };
                for &(lane, _) in &inter.incoming {
                    for &id in &net.lanes[lane as usize].cars {
                        let Some(car) = cars.get(id) else { continue };
                        if car.v < 1.0 && matches!(car.wait, Wait::RailCrossing | Wait::Leader) {
                            c.vehicles_waiting += 1;
                            c.held_seconds += dt;
                            if !c.waiting.contains(&id) {
                                c.waiting.push(id);
                                c.vehicles_held += 1;
                            }
                        }
                    }
                }
            }
        }
        // The trains coming, and those over a crossing now.
        let mut eta: Vec<Option<f32>> = vec![None; n];
        let mut over: Vec<Vec<TrainId>> = vec![Vec::new(); n];
        for (id, t) in self.rail.trains.iter() {
            let waiting_here = self.level_crossings.trains_waiting.contains(&id);
            let tail = t.s - t.length();
            for r in self.crossings_on(t) {
                if t.s >= r.s_in && tail <= r.s_out {
                    over[r.k].push(id);
                    continue;
                }
                if t.s >= r.s_in {
                    continue;
                }
                // Running time, the stand it is making now and the calls
                // it makes before it gets there.
                let calls = t.stops.iter().skip(t.next_stop).filter(|(_, s)| *s > t.s + 0.5 && *s < r.s_in).count();
                // [r4] At the speed it has now, picking up speed (a train
                // standing at a call is a while yet).
                let mut e = t.seconds_to(r.s_in - t.s) + t.dwell.max(0.0) + calls as f32 * rail::DWELL_SECONDS;
                // Held at a block signal (not at this crossing): it may
                // stand a while yet.
                if t.held && !waiting_here {
                    e += HELD_EXTRA;
                }
                eta[r.k] = Some(eta[r.k].map_or(e, |x| x.min(e)));
            }
        }
        let mut any = false;
        for (k, c) in self.level_crossings.list.iter_mut().enumerate() {
            for &id in &over[k] {
                if !c.seen.contains(&id) {
                    c.seen.push(id);
                    c.passes.push(tick);
                    c.trains_total += 1;
                }
            }
            c.passes.retain(|&p| tick.saturating_sub(p) < PASS_WINDOW);
            c.step(dt, eta[k], !over[k].is_empty());
            c.people_waiting = 0;
            any |= c.shut();
        }
        self.level_crossings.any_shut = any;
        // Their lanes, closed to vehicles while the lights are on.
        let net = &mut self.traffic.net;
        let mut lanes_shut = false;
        for c in &self.level_crossings.list {
            let Some(inter) = net.node_intersection.get(c.node.0 as usize).and_then(|&i| net.intersections.get(i)) else { continue };
            let conns: Vec<u32> = inter.connectors.iter().map(|&(l, _)| l).collect();
            let shut = c.shut();
            lanes_shut |= shut && !conns.is_empty();
            for l in conns {
                net.lanes[l as usize].rail_closed = shut;
            }
        }
        net.rail_closed_any = lanes_shut;
    }

    /// Where along its route train `id` must stop short of a crossing that
    /// is not yet closed with its box clear, if any. A train already on a
    /// crossing runs on.
    pub(super) fn crossing_hold(&self, id: TrainId) -> Option<(f32, usize)> {
        let t = self.rail.trains.get(id)?;
        if self.level_crossings.list.is_empty() {
            return None;
        }
        let mut best: Option<(f32, usize)> = None;
        for r in self.crossings_on(t) {
            let hold = r.s_in - 1.0;
            if t.s > hold + 0.01 {
                continue;
            }
            let c = &self.level_crossings.list[r.k];
            if c.down() && self.crossing_box_clear(c) {
                continue;
            }
            if best.is_none_or(|(b, _)| hold < b) {
                best = Some((hold, r.k));
            }
        }
        best
    }

    /// Where a walker or rider going from `a` to `b` must stop short of a
    /// closed crossing (the crossing's index and the spot), if anywhere.
    pub(super) fn rail_stop(&self, a: Vec2, b: Vec2) -> Option<(usize, Vec2)> {
        if !self.level_crossings.any_shut {
            return None;
        }
        let mut best: Option<(f32, usize)> = None;
        for (k, c) in self.level_crossings.list.iter().enumerate() {
            if !c.shut() || c.pos.distance(a) > c.reach + 40.0 {
                continue;
            }
            if let Some(t) = c.entry(a, b) {
                if best.is_none_or(|(bt, _)| t < bt) {
                    best = Some((t, k));
                }
            }
        }
        let (t, k) = best?;
        let len = a.distance(b);
        let back = (t * len - 0.4).max(0.0);
        Some((k, a + (b - a).normalize_or_zero() * back))
    }

    /// Count a walker or rider standing at crossing `k` this tick.
    pub(super) fn note_rail_wait(&mut self, k: usize) {
        if let Some(c) = self.level_crossings.list.get_mut(k) {
            c.people_waiting += 1;
        }
    }

    /// Every level crossing, with its cycle and counts.
    pub fn level_crossings(&self) -> &[LevelCrossing] {
        &self.level_crossings.list
    }

    /// The level crossing nearest `p` within `radius` metres.
    pub fn level_crossing_near(&self, p: Vec2, radius: f32) -> Option<&LevelCrossing> {
        self.level_crossings.list.iter().filter(|c| c.pos.distance(p) <= radius).min_by(|a, b| a.pos.distance(p).total_cmp(&b.pos.distance(p)))
    }

    /// Seconds until the next train reaches each crossing it runs over, for
    /// the inspector: (train, seconds) for the trains coming to `c`.
    pub fn trains_coming(&self, c: &LevelCrossing) -> Vec<(TrainId, f32)> {
        let mut out = Vec::new();
        for (id, t) in self.rail.trains.iter() {
            let Some((s, d)) = project_on_poly(&t.poly, c.pos) else { continue };
            let s_in = s - c.reach - 1.0;
            if d < 3.0 && t.s < s_in {
                out.push((id, (s_in - t.s) / t.speed.max(0.1) + t.dwell.max(0.0)));
            }
        }
        out.sort_by(|a, b| a.1.total_cmp(&b.1));
        out
    }
}
