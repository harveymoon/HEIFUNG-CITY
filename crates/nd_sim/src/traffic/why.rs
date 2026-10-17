//! Why is that car standing there? The plain-language reason a vehicle is
//! held up, and the chain of vehicles in front of it back to the one that
//! is really holding the queue (the head of the jam).
//!
//! The physics step records only a coarse reason (`Car::wait`) and the one
//! vehicle it waits for (`Car::blocked_by`), because that is all the
//! car-following model and the gridlock breaker need. A player looking at a
//! jam needs more: *which* car it is queued behind, whether a yield is for
//! oncoming traffic, people on a crossing or a roundabout, and what the car
//! at the very front is doing. This module derives that on demand from the
//! same state, read-only, so it costs nothing per tick and can never change
//! what the simulation does.

use super::*;

/// What is holding a vehicle up, with the vehicle responsible when there is
/// one. `Moving` means nothing is: it is driving freely.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StopReason {
    /// Driving freely (or pulling away).
    Moving,
    /// Queued behind another vehicle in its lane, or just beyond a junction.
    Queued { behind: CarId },
    /// Stopping short to let a car out of the kerb.
    LettingOut { car: CarId },
    /// A red light.
    RedLight,
    /// People on a pedestrian crossing ahead (`crossing`: the index into
    /// `WalkGraph::crossings`, for the world to say who is on it).
    Pedestrians { crossing: Option<u32> },
    /// Braking hard for someone crossing where there is no crosswalk (a
    /// roundabout's lanes, a dead end, a jaywalker): nobody stops for them
    /// there, but nobody runs them down either if they can still stop.
    Jaywalker { crossing: Option<u32> },
    /// Giving way at an unsignalled junction to traffic already in it.
    GiveWay { to: Option<CarId> },
    /// Turning left on a shared green, waiting for oncoming traffic.
    TurningLeft { to: Option<CarId> },
    /// Giving way to traffic on a roundabout.
    Roundabout,
    /// The lane beyond the junction is full: it will not block the box.
    ExitFull { tail: Option<CarId> },
    /// At the end of its lane, waiting for a gap to move into the lane its
    /// next turn needs.
    LaneChange { beside: Option<CarId> },
    /// At the kerb, waiting for a gap to pull out.
    AtKerb { waiting_for: Option<CarId> },
    /// A bus standing at a stop.
    BusStop,
    /// A lorry or van standing at the kerb on a job (unloading).
    Unloading,
    /// [r2-traffic] A private car or bike stopped at the kerb (dropping
    /// someone off, after a knock with a walker): not "on a job".
    PulledOver,
    /// Pulling up at its destination.
    Arriving,
    /// Stopped after a crash.
    Crashed { wrecked: bool },
    /// A street takeover holds the junction ahead.
    Takeover,
    /// In a takeover show, off the lanes.
    InShow,
    /// [scene] Held up by something at the roadside with no room to pass
    /// (a fire engine at a fire, a tow truck at work).
    Scene,
    /// [scene] A tow truck at work off the lanes (against a one-way, or
    /// turning round).
    AtWork,
    /// [rail-crossing] At a level crossing: its lights are on, a train is
    /// coming or passing.
    LevelCrossing,
    /// Standing still for none of the reasons above. Should not happen: a
    /// car showing this is a bug worth reporting.
    Unexplained,
}

impl StopReason {
    /// The vehicle responsible, to follow towards the head of a jam.
    pub fn by(&self) -> Option<CarId> {
        match *self {
            StopReason::Queued { behind } => Some(behind),
            StopReason::LettingOut { car } => Some(car),
            StopReason::GiveWay { to } | StopReason::TurningLeft { to } => to,
            StopReason::ExitFull { tail } => tail,
            StopReason::LaneChange { beside } => beside,
            StopReason::AtKerb { waiting_for } => waiting_for,
            _ => None,
        }
    }

    /// A short machine name, for the remote API.
    pub fn key(&self) -> &'static str {
        match self {
            StopReason::Moving => "moving",
            StopReason::Queued { .. } => "queued",
            StopReason::LettingOut { .. } => "letting_out",
            StopReason::RedLight => "red_light",
            StopReason::Pedestrians { .. } => "pedestrians",
            StopReason::Jaywalker { .. } => "jaywalker",
            StopReason::GiveWay { .. } => "give_way",
            StopReason::TurningLeft { .. } => "turning_left",
            StopReason::Roundabout => "roundabout",
            StopReason::ExitFull { .. } => "exit_full",
            StopReason::LaneChange { .. } => "lane_change",
            StopReason::AtKerb { .. } => "at_kerb",
            StopReason::BusStop => "bus_stop",
            StopReason::Unloading => "unloading",
            StopReason::PulledOver => "pulled_over",
            StopReason::Arriving => "arriving",
            StopReason::Crashed { .. } => "crashed",
            StopReason::Takeover => "takeover",
            StopReason::InShow => "in_show",
            StopReason::Scene => "scene",
            StopReason::AtWork => "at_work",
            StopReason::LevelCrossing => "level_crossing",
            StopReason::Unexplained => "unexplained",
        }
    }

    /// Plain language, naming the vehicle responsible with `name`.
    pub fn text(&self, name: impl Fn(CarId) -> String) -> String {
        let or = |c: Option<CarId>, with: &str, without: &str| c.map_or(without.to_string(), |c| format!("{with} {}", name(c)));
        match *self {
            StopReason::Moving => "driving".into(),
            StopReason::Queued { behind } => format!("queued behind {}", name(behind)),
            StopReason::LettingOut { car } => format!("letting {} out of the kerb", name(car)),
            StopReason::RedLight => "waiting at a red light".into(),
            StopReason::Pedestrians { .. } => "stopped for people on a crossing".into(),
            StopReason::Jaywalker { .. } => "braking for someone crossing the road (no crosswalk)".into(),
            StopReason::GiveWay { to } => or(to, "giving way at the junction to", "giving way at the junction"),
            StopReason::TurningLeft { to } => or(to, "turning left, waiting for oncoming", "turning left, waiting for a gap in oncoming traffic"),
            StopReason::Roundabout => "giving way to the roundabout".into(),
            StopReason::ExitFull { tail } => or(tail, "the road beyond the junction is full, back to", "the road beyond the junction is full"),
            StopReason::LaneChange { beside } => or(beside, "waiting to change lanes, blocked by", "waiting for a gap to change lanes"),
            StopReason::AtKerb { waiting_for } => or(waiting_for, "at the kerb, waiting to pull out past", "at the kerb, waiting to pull out"),
            StopReason::BusStop => "at a bus stop".into(),
            StopReason::Unloading => "stopped at the kerb on a job (unloading)".into(),
            StopReason::PulledOver => "pulled over at the kerb".into(),
            StopReason::Arriving => "pulling up at its destination".into(),
            StopReason::Crashed { wrecked: true } => "written off in a crash, waiting for the tow".into(),
            StopReason::Crashed { wrecked: false } => "stopped after a bump, exchanging details".into(),
            StopReason::Takeover => "a street takeover is blocking the junction".into(),
            StopReason::InShow => "in a street takeover show".into(),
            StopReason::Scene => "held up at an emergency scene or a tow truck at work, no room to pass".into(),
            StopReason::AtWork => "a tow truck at work, slowly, hazard lights on".into(),
            StopReason::LevelCrossing => "waiting at a level crossing for a train".into(),
            StopReason::Unexplained => "stopped for no reason the sim knows (a bug: please report it)".into(),
        }
    }

    /// The words before the responsible vehicle's name, for a UI that
    /// shows the vehicle as a link: "queued behind" [car].
    pub fn lead_in(&self) -> &'static str {
        match self {
            StopReason::Queued { .. } => "queued behind",
            StopReason::LettingOut { .. } => "letting out of the kerb:",
            StopReason::GiveWay { .. } => "giving way at the junction to",
            StopReason::TurningLeft { .. } => "turning left, waiting for oncoming",
            StopReason::ExitFull { .. } => "the road beyond the junction is full, back to",
            StopReason::LaneChange { .. } => "waiting to change lanes, blocked by",
            StopReason::AtKerb { .. } => "at the kerb, waiting to pull out past",
            _ => "",
        }
    }

    /// A reason that is, by itself, a fine reason to stand still: the head
    /// of a jam with one of these is traffic doing what traffic does.
    pub fn is_good(&self) -> bool {
        !matches!(self, StopReason::Unexplained)
    }
}

/// The queue in front of a vehicle: each car waiting for the next, back to
/// the head (the last), which waits for nobody (or for a moving car).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JamChain {
    /// From the car asked about (first) to the head of the jam (last).
    pub cars: Vec<CarId>,
    /// The chain came back on itself: a gridlock ring, with no real head.
    pub cycle: bool,
}

impl JamChain {
    pub fn head(&self) -> Option<CarId> {
        self.cars.last().copied()
    }
}

/// How far the chain is followed at most (a long jam across town).
const MAX_CHAIN: usize = 400;
/// Slower than this (m/s) counts as standing.
const STANDING: f32 = 0.5;

impl Traffic {
    /// Why `id` is held up (or `Moving`). Read-only.
    pub fn stop_reason(&self, id: CarId) -> StopReason {
        let Some(c) = self.cars.get(id) else { return StopReason::Unexplained };
        if c.in_stunt() {
            return if matches!(c.drive.stunt, Some(Stunt { kind: StuntKind::Manoeuvre { .. }, .. })) { StopReason::AtWork } else { StopReason::InShow };
        }
        if let Some(cr) = c.drive.crash {
            return StopReason::Crashed { wrecked: cr.wrecked };
        }
        if c.hold > 0.0 || c.wait == Wait::Dwell {
            return if c.bus.is_some() {
                StopReason::BusStop
            } else if c.cargo.is_none() && c.owned.is_some() {
                StopReason::PulledOver
            } else {
                StopReason::Unloading
            };
        }
        if c.entering {
            return StopReason::AtKerb { waiting_for: c.blocked_by.filter(|b| self.cars.contains_key(*b)) };
        }
        let Some(&here) = c.route.get(c.idx) else { return StopReason::Unexplained };
        let by = c.blocked_by.filter(|b| self.cars.contains_key(*b));
        let r = match c.wait {
            Wait::Free => {
                // Nothing in the way: just pulling away from a stop.
                if c.v < STANDING && c.stalled > 30.0 { StopReason::Unexplained } else { StopReason::Moving }
            }
            Wait::Leader => match by {
                Some(b) if self.cars[b].entering => StopReason::LettingOut { car: b },
                Some(b) => StopReason::Queued { behind: b },
                // The leader it braked for has just left the road (arrived,
                // parked, off the map): it is about to pull away.
                None => self.next_on_route(c).map_or(if c.stalled < 30.0 { StopReason::Moving } else { StopReason::Unexplained }, |b| StopReason::Queued { behind: b }),
            },
            // At a red light with the road beyond full as well: the light
            // is not what holds it (a player saw a head "at a red light"
            // for two hours at a junction whose every exit was a queue).
            // Named for the full exit, so the chain goes on to its head.
            Wait::Signal => match (c.route.get(c.idx + 1), c.route.get(c.idx + 2)) {
                (Some(&conn), Some(&exit)) if self.net.lanes[conn as usize].is_connector() && self.exit_blocked(&self.net.lanes[conn as usize], exit, true) => StopReason::ExitFull { tail: self.net.lanes[exit as usize].cars.first().copied() },
                _ => StopReason::RedLight,
            },
            Wait::Yield => self.yield_reason(c, here),
            Wait::ExitFull => StopReason::ExitFull { tail: by },
            Wait::LaneChange => StopReason::LaneChange { beside: self.lane_change_blocker(c) },
            Wait::Entering => StopReason::AtKerb { waiting_for: by },
            Wait::Dwell => StopReason::Unloading,
            Wait::Arriving => StopReason::Arriving,
            Wait::Crashed => StopReason::Crashed { wrecked: false },
            Wait::Blocked if self.scene_zones().iter().any(|z| z.pos.distance(c.pos) < 60.0) => StopReason::Scene,
            Wait::Blocked => StopReason::Takeover,
            Wait::RailCrossing => StopReason::LevelCrossing,
        };
        // A car rolling along behind another is not "held up" in any way a
        // player cares about; it is driving.
        match r {
            StopReason::Queued { .. } if c.v > 3.0 => StopReason::Moving,
            r => r,
        }
    }

    /// The car whose rear is next ahead on the route (this lane or the next
    /// few), for a leader the step did not record.
    fn next_on_route(&self, c: &Car) -> Option<CarId> {
        let lane = &self.net.lanes[c.route[c.idx] as usize];
        if let Some(k) = lane.cars.iter().position(|&o| self.cars.get(o).is_some_and(|x| x.s > c.s)) {
            return Some(lane.cars[k]);
        }
        let mut prev = c.route[c.idx];
        for &next in c.route.iter().skip(c.idx + 1).take(4) {
            if self.net.lanes[prev as usize].is_beside(next) {
                return None;
            }
            if let Some(&first) = self.net.lanes[next as usize].cars.first() {
                return Some(first);
            }
            prev = next;
        }
        None
    }

    /// Who a yielding car gives way to, and why.
    fn yield_reason(&self, c: &Car, here: LaneId) -> StopReason {
        let lanes = &self.net.lanes;
        // Somebody on a crossing in the next stretch of road.
        let mut base = -c.s;
        for k in c.idx..(c.idx + 3).min(c.route.len()) {
            let l = c.route[k];
            let ahead = |m: &std::collections::BTreeMap<LaneId, Vec<Hold>>| m.get(&l).and_then(|hs| hs.iter().filter(|h| base + h.s >= -0.5 && base + h.s < 40.0).min_by(|a, b| a.s.total_cmp(&b.s)).map(|h| h.crossing));
            if let Some(cr) = ahead(&self.peds.holds) {
                return StopReason::Pedestrians { crossing: Some(cr) };
            }
            if let Some(cr) = ahead(&self.peds.watch) {
                return StopReason::Jaywalker { crossing: Some(cr) };
            }
            base += lanes[l as usize].length;
        }
        let Some(&next) = c.route.get(c.idx + 1) else { return StopReason::GiveWay { to: None } };
        let nl = &lanes[next as usize];
        let LaneKind::Connector { node, approach } = nl.kind else { return StopReason::GiveWay { to: None } };
        if lanes[here as usize].is_connector() {
            return StopReason::GiveWay { to: None };
        }
        if self.net.rings.nodes.contains_key(&node) {
            return StopReason::Roundabout;
        }
        let Some(inter) = self.net.node_intersection.get(node.0 as usize).and_then(|&i| self.net.intersections.get(i)) else { return StopReason::GiveWay { to: None } };
        let in_junction = |from: &dyn Fn(usize, &Lane) -> bool| -> Option<CarId> {
            inter.connectors.iter().filter(|&&(lid, ap)| from(ap, &lanes[lid as usize])).flat_map(|&(lid, _)| lanes[lid as usize].cars.iter().copied()).find(|&o| self.cars.get(o).is_some_and(|x| !x.crashed()))
        };
        if inter.signalized && nl.turn < 0 {
            let opp = inter.opposite.get(approach).copied().flatten();
            let to = opp.and_then(|opp| {
                in_junction(&|ap, l| ap == opp && l.turn >= 0).or_else(|| inter.incoming.iter().filter(|&&(_, ap)| ap == opp).find_map(|&(lid, _)| lanes[lid as usize].cars.last().copied()))
            });
            return StopReason::TurningLeft { to };
        }
        StopReason::GiveWay { to: in_junction(&|ap, _| ap != approach) }
    }

    /// The vehicle in the lane a car wants to move into that is in its way
    /// (the nearest one alongside).
    fn lane_change_blocker(&self, c: &Car) -> Option<CarId> {
        let target = *c.route.get(c.idx + 1)?;
        let tl = &self.net.lanes[target as usize];
        tl.cars.iter().copied().filter(|&o| self.cars.contains_key(o)).min_by(|&a, &b| (self.cars[a].s - c.s).abs().total_cmp(&(self.cars[b].s - c.s).abs()))
    }

    /// Follow who-waits-for-whom from `id` to the head of the jam. The
    /// chain stops at a car that waits for nobody, at one that is moving
    /// (it is included: the queue is crawling after it), or where it comes
    /// back on itself (a gridlock ring).
    pub fn jam_chain(&self, id: CarId) -> JamChain {
        let mut chain = JamChain::default();
        if !self.cars.contains_key(id) {
            return chain;
        }
        let mut seen = std::collections::HashSet::new();
        let mut cur = id;
        loop {
            chain.cars.push(cur);
            seen.insert(cur);
            if chain.cars.len() >= MAX_CHAIN {
                break;
            }
            // The car asked about may be rolling; the rest must stand.
            if cur != id && self.cars[cur].v >= STANDING {
                break;
            }
            let Some(next) = self.stop_reason(cur).by() else { break };
            if seen.contains(&next) {
                chain.cycle = true;
                break;
            }
            cur = next;
        }
        chain
    }
}
