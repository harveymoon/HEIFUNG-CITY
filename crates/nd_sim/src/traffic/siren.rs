//! [emergency] Lights and sirens, and the people who get out of their way
//! (or do not).
//!
//! - Pulling over (`yield_to_sirens`): drivers a little ahead of an
//!   emergency vehicle, on its lane, on the next lane of its route or
//!   coming the other way on the lane it is overtaking in, pull to the
//!   right (onto the kerb edge from a kerb lane) and slow, stopping once it
//!   is right behind them. Whether a driver does is temperament
//!   (`pulls_over`): nearly everyone, but the rebellious, the drunk and
//!   takeover crews often not. Drivers about to cross a junction a siren is
//!   going through against the light hold at the line.
//! - Driving off-law (`plan_offlaw`, `offlaw_gap`): a siren with someone in
//!   its way who will not (or cannot) pull over drives round them: into the
//!   lane beside or onto the opposing lane when it is clear far enough
//!   ahead, else up onto the kerb past the queue, else it waits behind for a
//!   few seconds and then squeezes past at a crawl (as all sirens used to).
//!   Red lights are taken carefully: slowly into the junction, holding at
//!   the line while cross traffic is moving through it. A police car in
//!   pursuit drives at its quarry, not round it: touching it is the tag
//!   (`world/chases.rs`).
//! - A suspect fleeing the police (`DriveState::fleeing`) drives the same
//!   way round slower traffic, recklessly and through reds, but never
//!   squeezes through a solid queue.
//!
//! The physics stay in `traffic.rs`; this module only plans each off-law
//! car's move before the car-following step and overrides its leader where
//! the move takes it past. The sideways offset is drawn on top of the lane
//! (`OffLaw::side`), like `Car::lateral`.

use super::*;

/// Cars this far ahead of a siren (on its lane or the next) pull over.
const SIREN_CLEAR: f32 = 60.0;
/// How far right a car pulls over: into the gutter of a kerb lane (onto
/// the lane line of an inner one: `SIREN_PULL_ASIDE`).
const SIREN_PULL_KERB: f32 = 2.2;
/// How hard a car pulling aside for a siren brakes (m/s per s), the crawl
/// it slows to, and how close the siren must be before it stops outright.
const SIREN_YIELD_BRAKE: f32 = 3.0;
const SIREN_YIELD_CRAWL: f32 = 2.0;
const SIREN_YIELD_STOP: f32 = 20.0;
/// A car this far pulled right is out of the siren's way.
const PULLED_OVER: f32 = 1.0;
/// Someone this close ahead in the way: plan a way round.
const PASS_LOOK: f32 = 22.0;
/// How far the opposing lane must be clear beyond the cars to pass, and how
/// many seconds of an oncoming car's (and our) speed on top.
const PASS_CLEAR: f32 = 20.0;
const PASS_CLOSING: f32 = 3.0;
/// Sideways offsets of the moves: the lane to the left (or the opposing
/// lane's near half), and up on the kerb.
const PASS_SIDE: f32 = -LANE_WIDTH;
const KERB_SIDE: f32 = LANE_WIDTH * 0.5 + 1.3;
/// Speeds (m/s): overtaking a queue, up on the kerb, squeezing through, and
/// going into a junction against the light.
const PASS_SPEED: f32 = 11.0;
const KERB_SPEED: f32 = 4.5;
const SQUEEZE_SPEED: f32 = 3.0;
const JUNCTION_SPEED: f32 = 6.5;
/// Braking (m/s per s) planned for the slow approach to a red.
const JUNCTION_BRAKE: f32 = 2.5;
/// Seconds stuck behind someone with no way round before a siren squeezes
/// past anyway (a call does not wait on a jam).
const SQUEEZE_AFTER: f32 = 4.0;
/// Cross traffic moving this fast this close in the junction: a siren
/// against the light waits at the line for it.
const CROSS_REACH: f32 = 18.0;
const CROSS_SPEED: f32 = 2.0;
/// Within this of the end of its lane, a siren facing a red takes care.
const RED_CARE: f32 = 30.0;
/// Sideways speed (m/s) of the swing out and back.
const OFFLAW_LATERAL: f32 = 2.2;
/// A police car chasing someone drives this much over the limit.
pub const PURSUIT_SPEED_FACTOR: f32 = 1.4;

/// What an off-law car is doing this step.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OffLawMove {
    /// In its lane: nobody in the way, or someone far enough ahead to follow.
    #[default]
    Lane,
    /// Round a car or a queue in the lane to the left or on the opposing lane.
    Overtake,
    /// Up on the kerb past a queue.
    Kerb,
    /// Stuck behind someone with no way round (yet).
    Wait,
    /// Through anyway, at a crawl, half out of the lane.
    Squeeze,
    /// At its quarry: drive into it (the tag).
    Ram,
    /// [driving] Any driver easing past a stopped hazard (a crash pushed to
    /// the edge of the lane) where there is room, slowly
    /// (`traffic/hazard.rs`).
    Hazard,
}

/// The plan of a vehicle driving off-law, refreshed every substep (never
/// saved: a loaded siren plans afresh).
#[derive(Clone, Copy, Debug, Default)]
pub struct OffLaw {
    pub mode: OffLawMove,
    /// Sideways offset drawn on top of the lane (m, + right), easing to
    /// `target`; `slew` is how fast it is moving (for the nose).
    pub side: f32,
    pub target: f32,
    pub slew: f32,
    /// Speed cap for the move, if any.
    pub cap: Option<f32>,
    /// Hold at this distance along the current lane (cross traffic in the
    /// junction ahead).
    pub stop: Option<f32>,
    /// Seconds spent in `Wait`.
    pub held: f32,
    /// In `Wait`: the tail of whoever is in the way (metres along the
    /// current lane, past its end for the next lane) and their speed. In
    /// `Hazard`: the same for whoever is beyond the hazard.
    pub lead: Option<(f32, f32)>,
    /// In `Hazard`: the stopped vehicle being passed.
    pub hazard: Option<CarId>,
}

/// Does this driver pull over for lights and sirens? Nearly everyone does;
/// the less a driver minds the rules (compliance) and the drunker, the
/// likelier they carry on. Decided from the driver's stable phase, so the
/// same driver decides the same way all trip.
pub fn pulls_over(p: &DriverProfile) -> bool {
    if p.ignores_signals {
        return false;
    }
    if !p.assigned {
        return true;
    }
    let will = (0.35 + 0.65 * p.compliance - 0.5 * p.drunk).clamp(0.05, 1.0);
    crate::pedestrians::hash01(p.phase.to_bits() as u64, 0x5173_0FF, 3) < will
}

impl DriverProfile {
    /// [emergency] A suspect fleeing the police: fast, close, through every
    /// light, round anything slower.
    pub fn fleeing(mut self) -> Self {
        self.assigned = true;
        self.speed = 1.2;
        self.headway = self.headway.min(0.7);
        self.accel = self.accel.max(2.2);
        self.aggression = 1.0;
        self.compliance = 0.0;
        self.ignores_signals = true;
        self.skill = self.skill.max(0.6);
        self
    }
}

impl Car {
    /// Drives off-law: lights and sirens, fleeing them, or [services] a
    /// recovery truck with its ambers on, close to the wreck it is going to.
    pub fn offlaw(&self) -> bool {
        self.siren || self.drive.fleeing || (self.drive.amber && self.drive.recovering)
    }

    /// [driving] Easing past a stopped hazard (`traffic/hazard.rs`): its
    /// leader is whoever is beyond the hazard, at a crawl.
    pub fn passing_hazard(&self) -> bool {
        self.drive.offlaw.mode == OffLawMove::Hazard
    }
}

impl Traffic {
    /// The lane running the other way beside this one: the opposing
    /// direction's leftmost lane, beside our leftmost.
    fn opposing(&self, lane: &Lane) -> Option<LaneId> {
        let LaneKind::Road { segment, forward, index } = lane.kind else { return None };
        if index != 0 {
            return None;
        }
        self.net.segment_lanes.get(&segment)?.iter().copied().find(|&l| matches!(self.net.lanes[l as usize].kind, LaneKind::Road { forward: f, index: 0, .. } if f != forward))
    }

    /// The first vehicle in the off-law car's way: on its lane from
    /// alongside onward, else the first on the next lane of its route,
    /// skipping cars pulled over, other sirens and anything behind. Its
    /// gap (0 when alongside).
    fn in_the_way(&self, id: CarId) -> Option<(CarId, f32)> {
        let c = &self.cars[id];
        let lane = &self.net.lanes[c.route[c.idx] as usize];
        let clear = |o: &Car| o.siren || o.lateral >= PULLED_OVER || o.in_stunt();
        let mut best: Option<(CarId, f32)> = None;
        for &o in &lane.cars {
            let oc = &self.cars[o];
            if o == id || clear(oc) || oc.s <= c.s - c.length() {
                continue;
            }
            let gap = (oc.s - oc.length() - c.s).max(0.0);
            if best.is_none_or(|b| gap < b.1) {
                best = Some((o, gap));
            }
        }
        if best.is_some() {
            return best;
        }
        let rem = lane.length - c.s;
        if rem > PASS_LOOK {
            return None;
        }
        let next = *c.route.get(c.idx + 1)?;
        if lane.is_beside(next) {
            return None;
        }
        let nl = &self.net.lanes[next as usize];
        nl.cars.iter().copied().filter(|&o| !clear(&self.cars[o])).map(|o| (o, (rem + self.cars[o].s - self.cars[o].length()).max(0.0))).min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Is `lane` free of traffic over `[from, to]` (our lane's metres;
    /// `against`: it runs the other way, so its cars come at us)?
    fn lane_clear(&self, lane: LaneId, from: f32, to: f32, against: bool, v: f32) -> bool {
        let l = &self.net.lanes[lane as usize];
        l.cars.iter().all(|&o| {
            let oc = &self.cars[o];
            if oc.lateral >= PULLED_OVER && !against {
                return true;
            }
            if against {
                // Its front, in our metres, and how far it closes meanwhile.
                let front = l.length - oc.s;
                let reach = to + (oc.v + v) * PASS_CLOSING;
                front + oc.length() < from || front > reach
            } else {
                oc.s < from || oc.s - oc.length() > to
            }
        })
    }

    /// Where an off-law car with `blocker` `gap` ahead can go round: a
    /// sideways offset, or `None`.
    fn way_round(&self, id: CarId, blocker: CarId, gap: f32) -> Option<(OffLawMove, f32)> {
        let c = &self.cars[id];
        let lane = &self.net.lanes[c.route[c.idx] as usize];
        if lane.is_connector() {
            return None;
        }
        // The stretch to pass: the blocker and whoever queues ahead of it
        // within a few car lengths, then room to cut back in.
        let b = &self.cars[blocker];
        let mut end = c.s + gap + b.length();
        for &o in &lane.cars {
            let oc = &self.cars[o];
            if oc.s > end && oc.s - oc.length() < end + 8.0 && oc.lateral < PULLED_OVER && !oc.siren {
                end = oc.s;
            }
        }
        let to = (end + PASS_CLEAR).min(lane.length + PASS_CLEAR);
        let from = c.s - c.length() - 2.0;
        if let Some(left) = lane.left {
            if self.lane_clear(left, from, to, false, c.v) {
                return Some((OffLawMove::Overtake, PASS_SIDE));
            }
        } else if let Some(opp) = self.opposing(lane) {
            if self.lane_clear(opp, from, to, true, c.v.max(PASS_SPEED)) {
                return Some((OffLawMove::Overtake, PASS_SIDE));
            }
        }
        // The kerb: only from the kerb lane of a street, past standing or
        // crawling traffic (not at speed past moving cars).
        if lane.right.is_none() && !lane.highway && b.v < 4.0 {
            return Some((OffLawMove::Kerb, KERB_SIDE));
        }
        None
    }

    /// A siren facing a red (or already on one): how fast to take it, and
    /// where to hold while cross traffic is moving through the junction.
    fn junction_care(&self, id: CarId) -> (Option<f32>, Option<f32>) {
        let c = &self.cars[id];
        let lane = &self.net.lanes[c.route[c.idx] as usize];
        let to_end = (lane.length - c.s).max(0.0);
        let (conn, approaching) = if lane.is_connector() && !lane.green {
            (c.route[c.idx], false)
        } else {
            match c.route.get(c.idx + 1).copied() {
                Some(n) if to_end < RED_CARE && self.net.lanes[n as usize].is_connector() && !self.net.lanes[n as usize].green => (n, true),
                _ => return (None, None),
            }
        };
        let cap = if approaching { (JUNCTION_SPEED * JUNCTION_SPEED + 2.0 * JUNCTION_BRAKE * (to_end - 2.0).max(0.0)).sqrt() } else { JUNCTION_SPEED };
        if !approaching {
            return (Some(cap), None);
        }
        let LaneKind::Connector { node, .. } = self.net.lanes[conn as usize].kind else { return (Some(cap), None) };
        let Some(inter) = self.net.node_intersection.get(node.0 as usize).and_then(|&i| self.net.intersections.get(i)) else { return (Some(cap), None) };
        let here = self.net.lanes[conn as usize].start();
        let busy = inter.connectors.iter().filter(|&&(l, _)| l != conn && self.net.lanes[l as usize].green).flat_map(|&(l, _)| self.net.lanes[l as usize].cars.iter()).any(|&o| {
            let oc = &self.cars[o];
            !oc.siren && !oc.crashed() && oc.v > CROSS_SPEED && oc.pos.distance(here) < CROSS_REACH
        });
        (Some(cap), busy.then_some(lane.length - 1.0))
    }

    /// Plan every off-law vehicle's move for this substep, and ease the
    /// sideways offsets (back to the lane for any that stopped).
    pub(super) fn plan_offlaw(&mut self, dt: f32) {
        // Cars easing past a hazard are planned in `plan_hazard_passes`.
        let ids: Vec<CarId> = self.cars.iter().filter(|(_, c)| c.offlaw() || (c.drive.offlaw.side != 0.0 && !c.passing_hazard())).map(|(id, _)| id).collect();
        for id in ids {
            let c = &self.cars[id];
            let active = c.offlaw() && !c.entering && !c.crashed() && !c.in_stunt() && c.hold <= 0.0;
            let mut plan = OffLaw { side: c.drive.offlaw.side, held: c.drive.offlaw.held, ..Default::default() };
            if active {
                if c.siren {
                    (plan.cap, plan.stop) = self.junction_care(id);
                }
                match self.in_the_way(id) {
                    Some((b, _)) if c.drive.chasing == Some(b) => plan.mode = OffLawMove::Ram,
                    Some((b, gap)) if gap < PASS_LOOK + c.v => match self.way_round(id, b, gap) {
                        Some((mode, side)) => {
                            plan.mode = mode;
                            plan.target = side;
                            let v = if mode == OffLawMove::Kerb { KERB_SPEED } else { PASS_SPEED.max(self.cars[b].v + 4.0) };
                            plan.cap = Some(plan.cap.map_or(v, |c: f32| c.min(v)));
                        }
                        None if c.siren && plan.held >= SQUEEZE_AFTER => {
                            plan.mode = OffLawMove::Squeeze;
                            plan.target = -LANE_WIDTH * 0.4;
                            plan.cap = Some(plan.cap.map_or(SQUEEZE_SPEED, |c: f32| c.min(SQUEEZE_SPEED)));
                        }
                        None => {
                            plan.mode = OffLawMove::Wait;
                            plan.lead = Some((c.s + gap, self.cars[b].v));
                        }
                    },
                    // Someone further ahead: close up as on any road.
                    Some((b, gap)) => {
                        plan.mode = OffLawMove::Wait;
                        plan.lead = Some((c.s + gap, self.cars[b].v));
                    }
                    None => {
                        // Room beside the cars pulled over for it.
                        plan.target = if c.siren { -0.6 } else { 0.0 };
                    }
                }
                // Keep out of the move until the car it passed is behind.
                let o = c.drive.offlaw;
                if plan.mode == OffLawMove::Lane && matches!(o.mode, OffLawMove::Overtake | OffLawMove::Kerb) && o.side.abs() > 1.0 && self.alongside(id) {
                    plan.mode = o.mode;
                    plan.target = o.target;
                    plan.cap = o.cap;
                }
                // Time stuck counts on through a squeeze (which only starts
                // after `SQUEEZE_AFTER`): reset there, a siren behind a wreck
                // alternated a substep of squeezing with four seconds of
                // waiting and took an hour to reach a crash.
                plan.held = match plan.mode {
                    OffLawMove::Wait if c.v < 1.0 => plan.held + dt,
                    OffLawMove::Wait | OffLawMove::Squeeze => plan.held,
                    _ => 0.0,
                };
            }
            let rate = OFFLAW_LATERAL * dt * (c.v / 3.0).clamp(0.4, 1.0);
            let step = (plan.target - plan.side).clamp(-rate, rate);
            plan.side += step;
            plan.slew = step / dt.max(1e-3);
            if !active && plan.side.abs() < 1e-3 {
                plan.side = 0.0;
            }
            self.cars[id].drive.offlaw = plan;
        }
        // [driving] Everyone else past crashes pushed to the edge.
        self.plan_hazard_passes(dt);
    }

    /// Something (not pulled over) still overlaps this car along its lane.
    fn alongside(&self, id: CarId) -> bool {
        let c = &self.cars[id];
        let lane = &self.net.lanes[c.route[c.idx] as usize];
        lane.cars.iter().any(|&o| {
            let oc = &self.cars[o];
            o != id && !oc.siren && oc.lateral < PULLED_OVER && oc.s > c.s - c.length() - 1.0 && oc.s - oc.length() < c.s + 1.0
        })
    }

    /// The leader an off-law car drives to, given the ordinary one: past
    /// whoever its move takes it round, through red lights (sirens), and
    /// holding for cross traffic.
    pub(super) fn offlaw_gap(&self, car: &Car, gap: f32, lead_v: f32, why: Wait) -> (f32, f32, Wait) {
        let free = (LOOKAHEAD * 2.0, ROAD_SPEED, Wait::Free);
        let o = &car.drive.offlaw;
        let mut out = match (o.mode, o.lead) {
            // Behind whoever is really in the way (the lane's own leader may
            // be someone pulled over).
            (OffLawMove::Wait, Some((tail, v))) => {
                let g = (tail - car.s).max(0.0);
                if car.siren || why != Wait::Leader || g < gap { (g, v, Wait::Leader) } else { (gap, lead_v, why) }
            }
            (OffLawMove::Overtake | OffLawMove::Kerb | OffLawMove::Squeeze | OffLawMove::Ram, _) if why == Wait::Leader => free,
            // [driving] Past a hazard: follow whoever is beyond it (the
            // hazard is the leader the plan was made for).
            (OffLawMove::Hazard, lead) if why == Wait::Leader => match lead {
                Some((tail, v)) => ((tail - car.s).max(0.0), v, Wait::Leader),
                None => free,
            },
            // Lights and sirens: no red light or full exit holds them, and a
            // leader in the lane with no move planned is no real obstacle
            // (pulled over, or passed and not yet behind in the lane order).
            _ if car.siren => free,
            _ => (gap, lead_v, why),
        };
        if let Some(st) = o.stop {
            let g = st - car.s + IDM_MIN_GAP;
            if g < out.0 {
                out = (g.max(0.0), 0.0, Wait::Yield);
            }
        }
        out
    }

    /// The speed cap of the car's off-law move, if any.
    pub(super) fn offlaw_cap(car: &Car) -> Option<f32> {
        (car.offlaw() || car.passing_hazard()).then_some(car.drive.offlaw.cap).flatten().map(|c| c.max(0.5))
    }

    /// An off-law car that drove past others keeps the lane's order:
    /// ahead of whoever it has passed (else the car behind would follow
    /// the wrong leader and drive into the one passed).
    pub(super) fn offlaw_reorder(&mut self) {
        let ids: Vec<CarId> = self.cars.iter().filter(|(_, c)| (c.offlaw() || c.passing_hazard()) && !c.entering).map(|(id, _)| id).collect();
        for id in ids {
            let lane = self.cars[id].route[self.cars[id].idx];
            let s = self.cars[id].s;
            let cars = &mut self.net.lanes[lane as usize].cars;
            let Some(mut k) = cars.iter().position(|&c| c == id) else { continue };
            while k + 1 < cars.len() && self.cars[cars[k + 1]].s < s {
                cars.swap(k, k + 1);
                k += 1;
            }
        }
    }

    /// Drivers get out of the way of lights and sirens (see the module
    /// notes): those ahead on the siren's lane and the next lane of its
    /// route, and oncoming drivers on a lane it is overtaking in, pull
    /// right and slow; drivers about to cross a junction it is running
    /// against the light hold at the line. Not everyone (`pulls_over`).
    pub(super) fn yield_to_sirens(&mut self, dt: f32) {
        // (car, siren metres behind it)
        let mut pull: Vec<(CarId, f32)> = Vec::new();
        let mut hold: Vec<(CarId, f32)> = Vec::new();
        for (sid, sc) in self.cars.iter().filter(|(_, c)| c.siren && !c.entering) {
            let lane = &self.net.lanes[sc.route[sc.idx] as usize];
            for &o in &lane.cars {
                let d = self.cars[o].s - sc.s;
                if o != sid && d > -2.0 && d < SIREN_CLEAR {
                    pull.push((o, d.max(0.0)));
                }
            }
            let rem = lane.length - sc.s;
            if let Some(&next) = sc.route.get(sc.idx + 1).filter(|&&n| rem < SIREN_CLEAR && !lane.is_beside(n)) {
                for &o in &self.net.lanes[next as usize].cars {
                    let d = rem + self.cars[o].s;
                    if d < SIREN_CLEAR {
                        pull.push((o, d));
                    }
                }
            }
            if sc.drive.offlaw.mode == OffLawMove::Overtake && lane.left.is_none() {
                if let Some(opp) = self.opposing(lane) {
                    let ol = &self.net.lanes[opp as usize];
                    for &o in &ol.cars {
                        let d = ol.length - self.cars[o].s - sc.s;
                        if d > -2.0 && d < SIREN_CLEAR * 1.5 {
                            pull.push((o, d.max(0.0)));
                        }
                    }
                }
            }
            // Cross traffic at the junction it is about to run.
            let conn = if lane.is_connector() && !lane.green { Some(sc.route[sc.idx]) } else { sc.route.get(sc.idx + 1).copied().filter(|&n| rem < RED_CARE && self.net.lanes[n as usize].is_connector() && !self.net.lanes[n as usize].green) };
            let node = conn.and_then(|c| match self.net.lanes[c as usize].kind {
                LaneKind::Connector { node, .. } => Some(node),
                _ => None,
            });
            if let Some(inter) = node.and_then(|n| self.net.node_intersection.get(n.0 as usize)).and_then(|&i| self.net.intersections.get(i)) {
                for &(cl, _) in inter.connectors.iter().filter(|&&(l, _)| Some(l) != conn && self.net.lanes[l as usize].green) {
                    for &inc in &self.net.lanes[cl as usize].prev {
                        let il = &self.net.lanes[inc as usize];
                        for &o in &il.cars {
                            let oc = &self.cars[o];
                            let to_line = il.length - oc.s;
                            if oc.route.get(oc.idx + 1) == Some(&cl) && to_line < 35.0 && to_line > 0.5 {
                                hold.push((o, to_line));
                            }
                        }
                    }
                }
            }
        }
        for (id, d) in pull {
            let lane_kerb = {
                let c = &self.cars[id];
                self.net.lanes[c.route[c.idx] as usize].right.is_none()
            };
            let c = &mut self.cars[id];
            if c.siren || c.offlaw() || c.crashed() || c.in_stunt() || c.entering || c.hold > 0.0 || !pulls_over(&c.drive.profile) {
                continue;
            }
            c.lateral = c.lateral.max(if lane_kerb { SIREN_PULL_KERB } else { SIREN_PULL_ASIDE });
            // Slow to a crawl as a driver would, braking firmly but not
            // stopping dead (a car snapped from 13 to 2 m/s was run into by
            // the one behind once the siren had passed); with the siren
            // right behind, to a stop at the kerb.
            let floor = if d < SIREN_YIELD_STOP && lane_kerb { 0.0 } else { SIREN_YIELD_CRAWL };
            c.v = c.v.min((c.v - SIREN_YIELD_BRAKE * dt).max(floor));
        }
        for (id, to_line) in hold {
            let c = &mut self.cars[id];
            if c.offlaw() || c.crashed() || !pulls_over(&c.drive.profile) {
                continue;
            }
            // Brake to stop short of the line if it still can.
            let room = (to_line - 2.0).max(0.0);
            let v_stop = (2.0 * SIREN_YIELD_BRAKE * room).sqrt();
            if c.v * c.v <= 2.0 * 2.0 * SIREN_YIELD_BRAKE * room + 1.0 {
                c.v = c.v.min(v_stop.max((c.v - SIREN_YIELD_BRAKE * 1.5 * dt).max(0.0)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::citizens::CitizenId;

    fn street(len: f32) -> RoadNetwork {
        let mut net = RoadNetwork::default();
        net.add_polyline(&[Vec2::new(-len, 0.0), Vec2::new(len, 0.0)], 1, 1);
        net
    }

    fn send(t: &mut Traffic, roads: &RoadNetwork, from: Vec2, to: Vec2) -> CarId {
        let (fs, _, _) = roads.nearest_street(from).unwrap();
        let (ts, _, _) = roads.nearest_street(to).unwrap();
        let (route, s0, s1) = t.net.find_route(fs, from, ts, to).expect("route");
        t.spawn_car(CitizenId::default(), None, route, s0, s1, 0)
    }

    #[test]
    fn nearly_everyone_pulls_over_and_the_reckless_less_often() {
        let share = |caution: f32, hedonism: f32, drunk: f32| {
            let n = 2000;
            (0..n).filter(|&k| pulls_over(&DriverProfile::for_driver(35, caution, hedonism, drunk, 0.5, k as f32 / n as f32))).count() as f32 / n as f32
        };
        let ordinary = share(0.6, 0.4, 0.0);
        let rebel = share(0.05, 0.95, 0.0);
        let drunk = share(0.3, 0.8, 0.8);
        assert!(ordinary > 0.9, "most good citizens pull over: {ordinary}");
        assert!(rebel < ordinary - 0.1 && rebel > 0.4, "the reckless less often: {rebel}");
        assert!(drunk < rebel, "the drunk least: {drunk}");
        assert!(!pulls_over(&DriverProfile::default().pack()), "a takeover pack never");
    }

    /// A siren behind a car that will not pull over, on a two-way street
    /// with nothing coming: it swings out onto the opposing lane, passes,
    /// and comes back in ahead.
    #[test]
    fn a_siren_overtakes_a_driver_who_will_not_pull_over_on_the_opposing_lane() {
        let roads = street(500.0);
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let y = 0.5 * LANE_WIDTH;
        let slow = send(&mut t, &roads, Vec2::new(-300.0, y), Vec2::new(450.0, y));
        // Find a profile that does not pull over, and make it a crawler.
        let mut p = DriverProfile::for_driver(22, 0.05, 0.95, 0.0, 0.5, 0.0);
        let mut k = 0;
        while pulls_over(&p) {
            k += 1;
            p.phase = k as f32 * 0.0137;
        }
        p.speed = 0.4;
        t.cars[slow].drive.profile = p;
        for _ in 0..10 {
            t.step(1.0, 4);
        }
        let siren = send(&mut t, &roads, Vec2::new(-400.0, y), Vec2::new(450.0, y));
        t.cars[siren].siren = true;
        t.cars[siren].drive.profile = DriverProfile::professional(0.3);
        let (mut out, mut passed) = (false, false);
        for _ in 0..120 {
            t.step(1.0, 4);
            let (s, l) = (&t.cars[siren], &t.cars[slow]);
            out |= s.drive.offlaw.side < -2.0 && s.drive.offlaw.mode == OffLawMove::Overtake;
            if s.route[s.idx] == l.route[l.idx] && s.s > l.s + 6.0 {
                passed = true;
                break;
            }
            assert!(t.crashes.is_empty(), "no crash");
        }
        assert!(out, "swung out onto the opposing lane");
        assert!(passed, "got past the driver who would not pull over");
        assert!(t.cars[slow].lateral < PULLED_OVER, "who did not pull over");
    }

    /// With a car coming the other way, the siren does not pull out in
    /// front of it: it waits (and, with no kerb room, squeezes past slowly
    /// once it has waited long enough).
    #[test]
    fn a_siren_does_not_overtake_into_oncoming_traffic() {
        let roads = street(500.0);
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let y = 0.5 * LANE_WIDTH;
        let blocker = send(&mut t, &roads, Vec2::new(-300.0, y), Vec2::new(450.0, y));
        t.step(1.0, 4);
        // Standing still (a broken-down car, a stubborn driver at a stop).
        t.cars[blocker].hold = 1e6;
        t.cars[blocker].v = 0.0;
        let siren = send(&mut t, &roads, Vec2::new(-330.0, y), Vec2::new(450.0, y));
        t.cars[siren].siren = true;
        t.cars[siren].drive.profile = DriverProfile::professional(0.3);
        // Oncoming cars all the time.
        let mut seen_overtake_with_traffic = false;
        for step in 0..40 {
            if step % 2 == 0 {
                send(&mut t, &roads, Vec2::new(-200.0, -y), Vec2::new(-480.0, -y));
            }
            t.step(1.0, 4);
            let s = &t.cars[siren];
            if s.drive.offlaw.mode == OffLawMove::Overtake {
                let lane = &t.net.lanes[s.route[s.idx] as usize];
                let opp = t.opposing(lane).unwrap();
                let ol = &t.net.lanes[opp as usize];
                seen_overtake_with_traffic |= ol.cars.iter().any(|&o| {
                    let front = ol.length - t.cars[o].s;
                    front > s.s - 5.0 && front < s.s + 15.0
                });
            }
        }
        assert!(!seen_overtake_with_traffic, "never out on the opposing lane with a car right there");
    }

    #[test]
    fn drivers_ahead_of_a_siren_pull_over_and_slow() {
        let roads = street(500.0);
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let y = 0.5 * LANE_WIDTH;
        let mut cars = Vec::new();
        for k in 0..6 {
            let c = send(&mut t, &roads, Vec2::new(-250.0 + 30.0 * k as f32, y), Vec2::new(450.0, y));
            let mut p = DriverProfile::professional(k as f32 * 0.1);
            p.speed = 0.5;
            t.cars[c].drive.profile = p;
            cars.push(c);
        }
        for _ in 0..8 {
            t.step(1.0, 4);
        }
        let siren = send(&mut t, &roads, Vec2::new(-330.0, y), Vec2::new(450.0, y));
        t.cars[siren].siren = true;
        t.cars[siren].drive.profile = DriverProfile::professional(0.3);
        let mut pulled = 0;
        for _ in 0..40 {
            t.step(1.0, 4);
            pulled = pulled.max(cars.iter().filter(|&&c| t.cars[c].lateral >= PULLED_OVER).count());
        }
        assert!(pulled >= 2, "drivers pulled over as it came: {pulled}");
        let s = &t.cars[siren];
        let behind = cars.iter().filter(|&&c| t.cars[c].pos.x < s.pos.x).count();
        assert!(behind >= 4, "it got past them: {behind} of 6 behind it");
    }
}
