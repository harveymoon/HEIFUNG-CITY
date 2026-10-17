//! [tow-moves] A tow truck at work bends the rules a little, slowly and with
//! its ambers flashing: it may drive the last stretch to a car against a
//! one-way street, and turn round in the street to head back.
//!
//! - **Against a one-way** (`plan_contraflow`): a car parked on a one-way
//!   street whose legal way in is a long loop (more than `CONTRA_SAVING`
//!   further than coming in from the far end) is reached from the far end:
//!   the truck is routed to the junction there, then leaves the lanes and
//!   creeps up the kerb side of the street against the traffic
//!   (`StuntKind::Manoeuvre`, `CONTRA_SPEED`) to stop nose to nose with
//!   the car, at most `CONTRA_MAX` along it. Drivers coming down the street
//!   slow and ease round it, or wait where there is no room
//!   (`traffic/scene.rs`). Hooked, it turns round in the street and joins
//!   the traffic the right way.
//! - **U-turns** (`maybe_u_turn`): a truck that has hooked its car (or a
//!   wreck) on a two-way street turns round in the road when the way back
//!   from the other side is much the shorter (`UTURN_SAVING`), instead of
//!   driving on round the block.
//!
//! The move under way is kept in `Driving::moves` with what comes after it
//! (hook the car, or go on to the pound or the yard), checked every tick.

use super::*;
use crate::enforcement::TowState;
use crate::traffic::{LaneId, LaneKind};
use serde::{Deserialize, Serialize};

/// Metres saved before a truck takes a one-way the wrong way.
pub const CONTRA_SAVING: f32 = 150.0;
/// Furthest it goes against the traffic (m).
pub const CONTRA_MAX: f32 = 160.0;
/// Its speed doing so, and turning round (m/s).
pub const CONTRA_SPEED: f32 = 3.0;
pub const UTURN_SPEED: f32 = 2.4;
/// Metres saved before a truck turns round in the street.
pub const UTURN_SAVING: f32 = 120.0;
/// [rage] Anger from which a driver may turn round illegally in the street,
/// and the metres (queues counted as `RAGE_QUEUE_M` a vehicle) it must
/// save them.
pub const RAGE_UTURN_ANGER: f32 = 0.6;
pub const RAGE_UTURN_SAVING: f32 = 150.0;
const RAGE_QUEUE_M: f32 = 25.0;

/// What a truck does once its move is done.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum AfterMove {
    /// Up to the car against the one-way: hook it (tow job).
    Hook { job: u32 },
    /// Turned round with the car on: on to the pound (tow job).
    ToPound { job: u32 },
    /// Turned round with a wreck on: back to the yard, or off the map.
    Home { company: Option<BuildingId> },
    /// [rage] A seething driver turned round illegally: on to where it
    /// was going (`s_end` along lane `goal`).
    Resume { goal: LaneId, s_end: f32 },
    /// [r2-traffic] A recovery truck in working mode beside its wreck:
    /// back on the lanes there and hook it.
    Recover { wreck: CarId },
}

/// A tow truck off the lanes on a move.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct TowMove {
    pub truck: CarId,
    pub then: AfterMove,
    pub since: u64,
    /// [tow-fix] The street the move is made in: the truck comes back onto
    /// its lanes, never onto a ramp or highway that passes over or beside
    /// it (it was put on the nearest "street" by distance alone, often a
    /// slip road overhead, where it had no way on to the pound and stood
    /// for hours: G10, G15, G19, G20). None from older saves.
    #[serde(default)]
    pub street: Option<SegmentId>,
}

/// [tow-moves] The way in against a one-way: the point the truck enters
/// the street at, where it stops (nose to nose with the car), and the way
/// it faces there.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Contra {
    pub entry: Vec2,
    pub stop: Vec2,
    pub face: f32,
}

impl World {
    /// A one-way street in town (not a ring, a highway or a slip road).
    fn one_way_street(&self, seg: SegmentId) -> bool {
        self.roads.try_segment(seg).is_some_and(|s| s.alive && s.is_street() && !s.ring && !s.ramp && !s.is_highway() && (s.lanes_fwd == 0) != (s.lanes_bwd == 0))
    }

    fn route_len(&self, r: &(Vec<LaneId>, f32, f32)) -> f32 {
        let net = &self.traffic.net;
        let full = net.route_length(&r.0, r.1);
        let last = r.0.last().map_or(0.0, |&l| net.lanes[l as usize].length);
        (full - (last - r.2).max(0.0)).max(0.0)
    }

    /// [tow-moves] For a truck to reach the car parked at `spot` on `seg`
    /// with the `legal` route (or none): a route to the junction at the far
    /// end of the one-way street and the way up it against the traffic,
    /// when that is much shorter. `from` routes the truck to a lane.
    pub(super) fn plan_contraflow(&self, seg: SegmentId, spot: Vec2, legal: Option<&(Vec<LaneId>, f32, f32)>, from: &dyn Fn(LaneId, f32) -> Option<(Vec<LaneId>, f32, f32)>) -> Option<((Vec<LaneId>, f32, f32), Contra)> {
        if !self.one_way_street(seg) {
            return None;
        }
        let s = self.roads.segment(seg);
        let (a, b) = self.roads.segment_points(seg);
        // The traffic flows a -> b with forward lanes, else b -> a: the far
        // end, where the truck comes in, is the end it flows to.
        let (up, down, down_node) = if s.lanes_fwd > 0 { (a, b, s.b) } else { (b, a, s.a) };
        let len = up.distance(down);
        if len < 1.0 {
            return None;
        }
        let flow = (down - up) / len;
        let along = (spot - up).dot(flow).clamp(0.0, len);
        let stop_t = (along + 6.5).min(len - 4.0).max(along);
        let back = len - stop_t;
        if back > CONTRA_MAX {
            return None;
        }
        // A lane of another street ending at the far junction.
        let at = self.roads.node(down_node).pos;
        let mut ins: Vec<(LaneId, f32)> = Vec::new();
        for &other in &self.roads.node(down_node).segments {
            if other == seg {
                continue;
            }
            for &l in self.traffic.net.segment_lanes.get(&other).into_iter().flatten() {
                let lane = &self.traffic.net.lanes[l as usize];
                if matches!(lane.kind, LaneKind::Road { .. }) && lane.end().distance(at) < 25.0 {
                    ins.push((l, lane.length));
                }
            }
        }
        ins.sort_by_key(|x| x.0);
        let alt = ins.into_iter().filter_map(|(l, len)| from(l, (len - 0.5).max(0.5))).min_by(|x, y| self.route_len(x).total_cmp(&self.route_len(y)))?;
        let alt_len = self.route_len(&alt) + back + 12.0;
        if legal.is_some_and(|r| self.route_len(r) <= alt_len + CONTRA_SAVING) {
            return None;
        }
        // Along the kerb on the car's side, nose against the flow.
        let right = Vec2::new(-flow.y, flow.x);
        let side = if (spot - up).dot(right) >= 0.0 { 1.0 } else { -1.0 };
        let off = right * side * (s.half_width() - 1.6).max(0.0);
        let entry = down - flow * 6.0 + off;
        let stop = up + flow * stop_t + off;
        let face = (-flow).y.atan2(-flow.x);
        Some((alt, Contra { entry, stop, face }))
    }

    /// [tow-moves] Up the one-way to the car: off the lanes, ambers on.
    pub(super) fn begin_contraflow(&mut self, truck: CarId, job: u32, c: Contra) {
        // [tow-fix] The one-way street the car stands in.
        let street = self.enforcement.job(job).and_then(|j| self.vehicles.cars.get(j.car)).and_then(|v| v.spot()).and_then(|s| s.segment);
        self.traffic.begin_manoeuvre(truck, vec![c.entry, c.stop], c.face, CONTRA_SPEED);
        if let Some(t) = self.traffic.cars.get_mut(truck) {
            t.drive.amber = true;
        }
        self.driving.moves.push(TowMove { truck, then: AfterMove::Hook { job }, since: self.clock.tick, street });
        self.enforcement.stats.contraflow_total += 1;
    }

    /// [tow-moves] The truck (on the lanes, its load on) turns round in
    /// the street when the way to `goal` from the other side is much
    /// shorter, then does `then`. True when it began to.
    pub(super) fn maybe_u_turn(&mut self, truck: CarId, goal: (LaneId, f32), then: AfterMove) -> bool {
        let Some(c) = self.traffic.cars.get(truck) else { return false };
        if c.in_stunt() {
            return false;
        }
        let Some(&lane) = c.route.get(c.idx) else { return false };
        let LaneKind::Road { segment, forward, .. } = self.traffic.net.lanes[lane as usize].kind else { return false };
        let (pos, heading, s) = (c.pos, c.heading, c.s);
        let here = self.traffic.net.route_lanes(lane, s, goal.0, goal.1);
        // The nearest lane the other way on the same street.
        let opp = self.traffic.net.segment_lanes.get(&segment).into_iter().flatten().copied().filter(|&l| matches!(self.traffic.net.lanes[l as usize].kind, LaneKind::Road { forward: f, .. } if f != forward)).map(|l| {
            let (os, d) = self.traffic.net.lanes[l as usize].project(pos);
            (l, os, d)
        }).min_by(|x, y| x.2.total_cmp(&y.2));
        let Some((ol, os, _)) = opp else { return false };
        let olen = self.traffic.net.lanes[ol as usize].length;
        let os = (os + 4.0).min(olen - 0.5).max(0.5);
        let Some(there) = self.traffic.net.route_lanes(ol, os, goal.0, goal.1) else { return false };
        if here.as_ref().is_some_and(|r| self.route_len(r) <= self.route_len(&there) + UTURN_SAVING) {
            return false;
        }
        self.turn_round(truck, pos, heading, Some(self.traffic.net.lanes[ol as usize].point_at(os).0), then, Some(segment));
        true
    }

    /// [rage] Drivers seething in a jam may break the law to get out of it:
    /// one whose anger is past `RAGE_UTURN_ANGER`, slowed or stood in the
    /// traffic in the middle of a two-way street, turns round across it
    /// when the way from the other side is much the quicker (queues on the
    /// way counted), the more readily the less law-abiding it is (a roll
    /// per trip on its compliance and anger). Gone round, it drives on to
    /// where it was going.
    pub(in crate::world) fn rage_u_turns(&mut self) {
        let net = &self.traffic.net;
        let cands: Vec<(CarId, LaneId, f32, Vec2, f32, LaneId, f32)> = self
            .traffic
            .cars
            .iter()
            .filter(|(_, c)| c.drive.anger >= RAGE_UTURN_ANGER && c.v < 4.0 && c.bus.is_none() && c.cargo.is_none() && !c.siren && !c.entering && c.hold <= 0.0 && !c.crashed() && !c.in_stunt() && c.drive.gang.is_none() && self.citizens.contains_key(c.citizen))
            .filter(|(id, c)| {
                // By temperament: the less law-abiding and the angrier, the likelier.
                let roll = crate::hash::hash01(slotmap::Key::data(id).as_ffi(), c.drive.trip_start, 0x7a11);
                roll < (1.0 - c.drive.profile.compliance).max(0.05) * c.drive.anger
            })
            .filter_map(|(id, c)| {
                let lane = *c.route.get(c.idx)?;
                let l = net.lanes.get(lane as usize)?;
                // Mid-block, not at the junction or just past it.
                (matches!(l.kind, LaneKind::Road { .. }) && c.s > 8.0 && l.length - c.s > 12.0).then(|| (id, lane, c.s, c.pos, c.heading, *c.route.last().unwrap(), c.s_end))
            })
            .collect();
        for (id, lane, s, pos, heading, goal, s_end) in cands {
            if self.tow_moving(id) {
                continue;
            }
            let LaneKind::Road { segment, forward, .. } = self.traffic.net.lanes[lane as usize].kind else { continue };
            if !self.roads.try_segment(segment).is_some_and(|g| g.alive && g.is_street() && !g.is_highway() && !g.ramp && !g.ring && g.lanes_fwd > 0 && g.lanes_bwd > 0) {
                continue;
            }
            let net = &self.traffic.net;
            let cost = |r: &(Vec<LaneId>, f32, f32)| self.route_len(r) + r.0.iter().take(8).map(|&l| net.lanes[l as usize].cars.len() as f32 * RAGE_QUEUE_M).sum::<f32>();
            let Some(here) = net.route_lanes(lane, s, goal, s_end) else { continue };
            let opp = net
                .segment_lanes
                .get(&segment)
                .into_iter()
                .flatten()
                .copied()
                .filter(|&l| matches!(net.lanes[l as usize].kind, LaneKind::Road { forward: f, .. } if f != forward))
                .map(|l| {
                    let (os, d) = net.lanes[l as usize].project(pos);
                    (l, os, d)
                })
                .min_by(|x, y| x.2.total_cmp(&y.2).then(x.0.cmp(&y.0)));
            let Some((ol, os, _)) = opp else { continue };
            let olane = &net.lanes[ol as usize];
            let os = (os + 4.0).min(olane.length - 0.5).max(0.5);
            // Only into a gap in the oncoming traffic.
            if olane.cars.iter().any(|&o| (self.traffic.cars[o].s - os).abs() < 15.0) {
                continue;
            }
            let Some(there) = net.route_lanes(ol, os, goal, s_end) else { continue };
            if cost(&here) <= cost(&there) + RAGE_UTURN_SAVING {
                continue;
            }
            let to = net.lanes[ol as usize].point_at(os).0;
            self.turn_round(id, pos, heading, Some(to), AfterMove::Resume { goal, s_end }, Some(segment));
            self.driving.rage_u_turns_today += 1;
            self.driving.rage_u_turns_total += 1;
        }
    }

    /// [tow-moves] Turn round where it stands (a three-point turn drawn as
    /// a slow arc) to end at `to` (or across the street) facing back.
    fn turn_round(&mut self, truck: CarId, pos: Vec2, heading: f32, to: Option<Vec2>, then: AfterMove, street: Option<SegmentId>) {
        let fwd = Vec2::new(heading.cos(), heading.sin());
        let target = to.unwrap_or(pos);
        // Swing out to the side the far lane is on (else to the left).
        let mut side = Vec2::new(-fwd.y, fwd.x);
        if (target - pos).dot(side) < 0.0 && to.is_some() {
            side = -side;
        }
        let across = (target - pos).dot(side).abs().max(3.0);
        let p1 = pos + fwd * 3.5 + side * (across * 0.45);
        let p2 = pos + fwd * 2.0 + side * across;
        let p3 = target - fwd * 1.5;
        self.traffic.begin_manoeuvre(truck, vec![p1, p2, p3], heading + std::f32::consts::PI, UTURN_SPEED);
        let rage = matches!(then, AfterMove::Resume { .. });
        if let Some(t) = self.traffic.cars.get_mut(truck) {
            // A tow truck's ambers flash; a seething driver just goes.
            t.drive.amber = !rage;
        }
        self.driving.moves.push(TowMove { truck, then, since: self.clock.tick, street });
        if !rage {
            self.enforcement.stats.u_turns_total += 1;
        }
    }

    /// [tow-moves] Every tick: moves that are done go on to what comes
    /// next; a truck gone (roads rebuilt) drops its move.
    pub(super) fn step_tow_moves(&mut self) {
        if self.driving.moves.is_empty() {
            return;
        }
        let tick = self.clock.tick;
        let moves = std::mem::take(&mut self.driving.moves);
        let mut keep = Vec::new();
        for m in moves {
            if !self.traffic.cars.contains_key(m.truck) {
                continue;
            }
            // Stuck far too long (blocked by something it cannot pass): it
            // is done where it stands.
            let stuck = tick >= m.since + 30;
            if !self.traffic.manoeuvre_done(m.truck) && !stuck {
                keep.push(m);
                continue;
            }
            match m.then {
                AfterMove::Hook { job } => {
                    // Hook from off the lanes, then turn round to leave.
                    let company = self.enforcement.job(job).and_then(|j| j.company);
                    let hooked = self.hook_parked(m.truck, job) && self.enforcement.job(job).is_some_and(|j| j.state == TowState::ToPound);
                    let (pos, heading) = self.traffic.cars.get(m.truck).map_or((Vec2::ZERO, 0.0), |c| (c.pos, c.heading));
                    if hooked {
                        // Round towards the middle of the street, to face
                        // the way the traffic goes.
                        let street = self.street_under(pos, m.street);
                        let mid = street.map(|s| {
                            let (a, b) = self.roads.segment_points(s);
                            let d = (b - a).normalize_or_zero();
                            a + d * (pos - a).dot(d)
                        });
                        self.turn_round(m.truck, pos, heading, mid, AfterMove::ToPound { job }, street);
                    } else {
                        self.back_on_lanes(m.truck, m.street, None);
                        if self.traffic.cars.get(m.truck).is_some_and(|c| !c.in_stunt()) {
                            self.tow_truck_home(m.truck, company);
                        }
                    }
                }
                AfterMove::ToPound { job } => {
                    let goal = self.pound_goal(m.truck, job);
                    self.back_on_lanes(m.truck, m.street, goal);
                    if self.traffic.cars.get(m.truck).is_some_and(|c| !c.in_stunt()) && !self.route_to_pound(m.truck, job) {
                        self.traffic.pull_in(m.truck, super::cars::RETIRE_RETRY_SECONDS);
                    }
                }
                AfterMove::Home { company } => {
                    let goal = self.yard_goal(company);
                    self.back_on_lanes(m.truck, m.street, goal);
                    if self.traffic.cars.get(m.truck).is_some_and(|c| !c.in_stunt()) {
                        self.recovery_truck_home(m.truck, company);
                    }
                }
                AfterMove::Recover { wreck } => {
                    self.back_on_lanes(m.truck, m.street, None);
                    if self.traffic.cars.get(m.truck).is_some_and(|c| !c.in_stunt()) {
                        self.recovery_truck_arrived(m.truck, wreck);
                    }
                }
                AfterMove::Resume { goal, s_end } => {
                    self.back_on_lanes(m.truck, m.street, Some((goal, s_end)));
                    // On to where it was going (with no way from this side
                    // after all, it arrives where it is and looks again).
                    if self.traffic.cars.get(m.truck).is_some_and(|c| !c.in_stunt()) {
                        self.traffic.redirect(m.truck, goal, s_end);
                    }
                }
            }
        }
        keep.extend(std::mem::take(&mut self.driving.moves));
        self.driving.moves = keep;
    }

    /// [tow-moves] Back onto the lanes where it stands: the lane of the
    /// street under it running the way it faces (any lane of the nearest
    /// street failing that), a few metres to go.
    ///
    /// [tow-fix] "The street under it" is the one the move was made in
    /// (`street`), else the nearest ordinary street: never a highway or
    /// slip road passing over or beside it, unless there is nothing else.
    /// Of its lanes one from which `goal` can be reached is preferred.
    pub(super) fn back_on_lanes(&mut self, truck: CarId, street: Option<SegmentId>, goal: Option<(LaneId, f32)>) {
        let Some(c) = self.traffic.cars.get(truck) else { return };
        if !c.in_stunt() {
            return;
        }
        let (pos, heading) = (c.pos, c.heading);
        let fwd = Vec2::new(heading.cos(), heading.sin());
        let Some(seg) = self.street_under(pos, street) else {
            self.retire_vehicle(truck);
            return;
        };
        let net = &self.traffic.net;
        let best = net
            .segment_lanes
            .get(&seg)
            .into_iter()
            .flatten()
            .copied()
            .map(|l| {
                let lane = &net.lanes[l as usize];
                let (s, d) = lane.project(pos);
                let along = lane.point_at(s).1.dot(fwd);
                let s0 = s.clamp(0.0, (lane.length - 0.5).max(0.0));
                let dead = goal.is_some_and(|(g, gs)| net.route_lanes(l, s0, g, gs).is_none());
                (l, s, d - along * 10.0 + if dead { 1000.0 } else { 0.0 })
            })
            .min_by(|a, b| a.2.total_cmp(&b.2).then(a.0.cmp(&b.0)));
        let Some((lane, s, _)) = best else {
            self.retire_vehicle(truck);
            return;
        };
        let len = self.traffic.net.lanes[lane as usize].length;
        let s0 = s.clamp(0.0, (len - 0.5).max(0.0));
        let s1 = (s0 + 3.0).min(len);
        self.traffic.end_stunt(truck, vec![lane], s0, s1);
        if let Some(t) = self.traffic.cars.get_mut(truck) {
            t.drive.amber = false;
        }
    }

    /// [tow-fix] The street a vehicle off the lanes at `pos` is in: `hint`
    /// while it stands, else the nearest ordinary street (not a highway,
    /// motorway or slip road), else any.
    pub(super) fn street_under(&self, pos: Vec2, hint: Option<SegmentId>) -> Option<SegmentId> {
        if let Some(s) = hint.filter(|&s| self.roads.try_segment(s).is_some_and(|g| g.alive && g.is_street()) && self.traffic.net.segment_lanes.get(&s).is_some_and(|l| !l.is_empty())) {
            return Some(s);
        }
        let plain = self.roads.nearest_where(pos, |s| s.is_street() && !s.is_highway() && !s.is_motorway() && !s.ramp).filter(|(_, _, q)| q.distance(pos) < 30.0);
        plain.or_else(|| self.roads.nearest_street(pos)).map(|(s, _, _)| s)
    }

    /// [tow-moves] Where a tow job's truck goes with the car on: the yard's
    /// driveway lane, else (out of town) the nearest way off the map.
    pub(super) fn pound_goal(&self, truck: CarId, job: u32) -> Option<(LaneId, f32)> {
        let company = self.enforcement.job(job).and_then(|j| j.company).filter(|c| self.buildings.contains_key(*c));
        match company {
            Some(c) => self.truck_stop(c).and_then(|(s, p)| self.traffic.net.nearest_lane_on(s, p)).map(|(l, s, _)| (l, s)),
            None => {
                let pos = self.traffic.cars.get(truck)?.pos;
                self.edge_exits(pos).into_iter().next()
            }
        }
    }

    /// [tow-moves] Where a recovery truck goes with its wreck: the yard.
    pub(super) fn yard_goal(&self, company: Option<BuildingId>) -> Option<(LaneId, f32)> {
        let c = company.filter(|c| self.buildings.contains_key(*c))?;
        self.truck_stop(c).and_then(|(s, p)| self.traffic.net.nearest_lane_on(s, p)).map(|(l, s, _)| (l, s))
    }

    /// [tow-moves] Is `truck` doing a move right now?
    pub fn tow_moving(&self, truck: CarId) -> bool {
        self.driving.moves.iter().any(|m| m.truck == truck)
    }
}
