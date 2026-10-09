//! Traffic signals: phases, greens stretched by demand, protected arrows.

use super::*;

/// What a signal knows about one approach of its junction.
#[derive(Clone, Debug, Default)]
pub struct SignalDemand {
    /// Someone at (or near) the stop line.
    pub any: Vec<bool>,
    /// Left turners near the stop line.
    pub left_queue: Vec<u8>,
    /// A left turner at the head of its lane has waited `LEFT_STARVED`.
    pub left_starved: Vec<bool>,
    /// The approach has a left-only lane.
    pub dedicated: Vec<bool>,
    /// The left turner at the head has been yielding to oncoming traffic.
    pub left_yielding: Vec<bool>,
}

/// Whether a junction gets signals by default from its shape: three
/// two-way streets or four streets of any kind, with at least two phases
/// to run.
pub fn default_signalized(approaches: usize, two_way: usize, phases: usize) -> bool {
    ((approaches >= 3 && two_way >= 3) || approaches >= 4) && phases >= 2
}

impl LaneNetwork {
    pub(super) fn apply_signals(&mut self) {
        for inter in &self.intersections {
            let group = inter.phases.get(inter.phase);
            for &(lane, approach) in &inter.connectors {
                let mine = group.is_some_and(|g| g.contains(&approach));
                let open = if inter.timer < inter.arrow {
                    // Protected arrow: left turns only.
                    self.lanes[lane as usize].turn < 0
                } else {
                    inter.timer < inter.arrow + inter.green()
                };
                self.lanes[lane as usize].green = !inter.signalized || (mine && open);
            }
        }
    }

    /// Fixed-cycle signal step (no knowledge of traffic).
    #[cfg(test)]
    pub(super) fn update_signals(&mut self, dt: f32) {
        self.step_signals(dt, None);
    }

    /// Advance the signals. With `demand` (per intersection, per approach:
    /// is anyone waiting at the stop line?) they are actuated: a green with
    /// nobody left to serve is cut short after `MIN_GREEN` when another
    /// approach is waiting, a green is held while only its own approach
    /// wants it, and phases nobody waits for are skipped. Without demand
    /// anywhere the plain fixed cycle runs.
    pub(super) fn step_signals(&mut self, dt: f32, demand: Option<&[SignalDemand]>) {
        for (k, inter) in self.intersections.iter_mut().enumerate() {
            if !inter.signalized || inter.phases.is_empty() {
                continue;
            }
            inter.timer += dt;
            let d = demand.and_then(|d| d.get(k)).filter(|d| d.any.len() >= inter.approaches);
            if let Some(d) = d {
                let wants = |g: &Vec<usize>| g.iter().any(|&ap| d.any[ap]);
                let mine = wants(&inter.phases[inter.phase]);
                let others = inter.phases.iter().enumerate().any(|(k, g)| k != inter.phase && wants(g));
                // The arrow ends early once nobody is left to turn.
                if inter.timer < inter.arrow && inter.timer >= ARROW_MIN && inter.phases[inter.phase].iter().all(|&ap| d.left_queue[ap] == 0) {
                    inter.arrow = inter.timer;
                }
                let green_end = inter.arrow + inter.green();
                // A left turner starving in an endless oncoming stream: end
                // this green so the next one can open with an arrow.
                let starving = inter.phases[inter.phase].iter().any(|&ap| d.left_starved[ap]);
                if inter.timer >= inter.arrow && inter.timer < green_end {
                    if inter.timer >= inter.arrow + MIN_GREEN && ((!mine && others) || starving) {
                        inter.timer = green_end;
                    } else if mine && !others && inter.timer > green_end - 1.0 {
                        inter.timer = green_end - 1.0;
                    }
                }
            }
            if inter.timer >= inter.arrow + inter.green() + CLEARANCE_SECONDS {
                inter.timer = 0.0;
                let n = inter.phases.len();
                let mut next = (inter.phase + 1) % n;
                if let Some(d) = d {
                    if let Some(step) = (1..=n).find(|&st| inter.phases[(inter.phase + st) % n].iter().any(|&ap| d.any[ap])) {
                        next = (inter.phase + step) % n;
                    }
                }
                inter.phase = next;
                // A protected left where opposed lefts queue in a lane of
                // their own, or a left turner has waited too long.
                let g = &inter.phases[next];
                // [tools] The player may have the arrow always or never.
                inter.arrow = match (inter.arrow_mode, d) {
                    (crate::junctions::LeftArrow::Never, _) => 0.0,
                    (crate::junctions::LeftArrow::Always, _) if g.len() > 1 => inter.arrow_len,
                    (crate::junctions::LeftArrow::Auto, Some(d)) if g.len() > 1 && g.iter().any(|&ap| (d.dedicated[ap] && d.left_queue[ap] >= 3 && d.left_yielding[ap]) || d.left_starved[ap]) => inter.arrow_len,
                    _ => 0.0,
                };
            }
        }
        self.apply_signals();
    }
}

impl Traffic {
    /// A left turn on a shared green yields to oncoming traffic going
    /// straight or right: vehicles in the junction from the opposite
    /// approach, or about to enter it at speed. Opposing left turns pass
    /// each other and do not yield.
    pub(super) fn oncoming(&self, inter: &Intersection, approach: usize) -> bool {
        let Some(opp) = inter.opposite.get(approach).copied().flatten() else { return false };
        let lanes = &self.net.lanes;
        // A wreck standing in the junction is not oncoming traffic.
        let in_junction = inter.connectors.iter().any(|&(lid, ap)| ap == opp && lanes[lid as usize].turn >= 0 && lanes[lid as usize].cars.iter().any(|&c| !self.cars[c].crashed()));
        in_junction
            || inter.incoming.iter().any(|&(lid, ap)| {
                let lane = &lanes[lid as usize];
                ap == opp
                    && lane.cars.last().is_some_and(|&h| {
                        let c = &self.cars[h];
                        let through = c.route.get(c.idx + 1).is_some_and(|&n| {
                            let nl = &lanes[n as usize];
                            nl.is_connector() && nl.green && nl.turn >= 0
                        });
                        through && c.v > 1.0 && lane.length - c.s < 25.0
                    })
            })
    }

    /// Per signalised intersection and approach: is a car waiting at (or
    /// close to) the stop line?
    pub(super) fn signal_demand(&self) -> Vec<SignalDemand> {
        self.net
            .intersections
            .iter()
            .map(|inter| {
                let n = if inter.signalized { inter.approaches } else { 0 };
                let mut d = SignalDemand { any: vec![false; n], left_queue: vec![0; n], left_starved: vec![false; n], dedicated: vec![false; n], left_yielding: vec![false; n] };
                if inter.signalized {
                    for &(lid, ap) in &inter.incoming {
                        let lane = &self.net.lanes[lid as usize];
                        if lane.arrows == LEFT {
                            d.dedicated[ap] = true;
                        }
                        if lane.cars.last().is_some_and(|&c| self.cars[c].s > lane.length - DEMAND_REACH) {
                            d.any[ap] = true;
                        }
                        // Left turners count only behind a head that could
                        // actually go: an arrow is wasted on a full exit.
                        let head_can_turn = lane.cars.last().is_some_and(|&h| {
                            let car = &self.cars[h];
                            self.next_turn(car) == Some(-1) && match (car.route.get(car.idx + 1), car.route.get(car.idx + 2)) {
                                (Some(&conn), Some(&exit)) => !self.exit_blocked(&self.net.lanes[conn as usize], exit, true),
                                _ => false,
                            }
                        });
                        if head_can_turn {
                            for (k, &c) in lane.cars.iter().enumerate().rev() {
                                let car = &self.cars[c];
                                if car.s < lane.length - DEMAND_REACH {
                                    break;
                                }
                                if self.next_turn(car) == Some(-1) {
                                    d.left_queue[ap] = d.left_queue[ap].saturating_add(1);
                                    if k + 1 == lane.cars.len() {
                                        d.left_starved[ap] |= car.yielded > LEFT_STARVED;
                                        d.left_yielding[ap] |= car.yielded > LEFT_YIELDING;
                                    }
                                }
                            }
                        }
                    }
                }
                d
            })
            .collect()
    }
}
