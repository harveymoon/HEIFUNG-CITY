//! [driving] Stopped hazards and the traffic easing past them.
//!
//! A crashed vehicle (a write-off waiting for the tow truck, or a fender
//! bender swapping details) is pushed to the edge of its lane, onto the
//! shoulder of a kerb lane (`push_aside`), the way the drivers or the
//! first police on scene would clear the road. Whoever comes up behind it
//! then eases past where there is physical room: its own body beside the
//! wreck's, with a little clearance, inside the lane and a little of the
//! next one (or the shoulder of a one-way road). It slows to a crawl going
//! by, and follows whoever is beyond the wreck rather than the wreck
//! (`OffLawMove::Hazard`, read by `offlaw_gap`). With no room (a wide
//! truck past a car wreck on a narrow street, a crash in an inner lane or
//! in a junction) traffic queues as before: it changes lanes, or waits and
//! looks for another way (`reroute_stalled`).
//!
//! A wreck used to stand in the middle of its lane until the tow truck came,
//! so one written-off motorcycle in a two-way tunnel held 40 cars for hours.

use super::*;
use std::collections::HashSet;

/// Width (m) of a vehicle's body, for the room beside a hazard.
pub fn body_width(c: &Car) -> f32 {
    use crate::vehicle::BodyType as B;
    if c.bus.is_some() {
        return 2.55;
    }
    match c.model.body {
        b if b.two_wheeler() => 0.8,
        B::Semi | B::ArticulatedBus | B::BubbleBus | B::GarbageTruck | B::PanelTruck | B::BoxVan => 2.5,
        _ if c.cargo.is_some() => 2.5,
        _ => CAR_WIDTH,
    }
}

/// A hazard pushed at least this far right of its lane centre can be
/// passed (and is out of the way for the collision check, as a car pulled
/// over for a siren is).
const HAZARD_ASIDE: f32 = 1.5;
/// Clearance kept from the hazard's body when passing it, and from the
/// road's edge when pushing a hazard onto the shoulder (m).
const HAZARD_CLEAR: f32 = 0.3;
const EDGE_CLEAR: f32 = 0.15;
/// How far into the next lane (or over the centre line) a driver edges to
/// get past, slowly (m).
const INTO_NEXT_LANE: f32 = 0.5;
/// A pass is planned when the hazard is this close ahead (m).
const HAZARD_LOOK: f32 = 30.0;
/// Speeds going past (m/s): a crawl when it is tight, a little more with
/// room to spare (everyone slows to look).
const HAZARD_CRAWL: f32 = 4.0;
const HAZARD_EASY: f32 = 8.0;
/// Spare room (m) beyond the clearance that counts as easy.
const EASY_ROOM: f32 = 0.8;

impl Traffic {
    /// How far right of its lane centre a stopped vehicle on a kerb lane can
    /// be pushed: onto the shoulder, its body clear of the road's edge.
    /// `None` in an inner lane (traffic changes lanes round it) or in a
    /// junction.
    pub fn aside_offset(&self, id: CarId) -> Option<f32> {
        let c = self.cars.get(id)?;
        let lane = &self.net.lanes[*c.route.get(c.idx)? as usize];
        if !self.kerbside(lane) {
            return None;
        }
        Some(LANE_WIDTH * 0.5 + SHOULDER - EDGE_CLEAR - body_width(c) * 0.5)
    }

    /// Push a crashed vehicle to the edge of its lane (see the module
    /// notes). True when it was moved; idempotent.
    pub fn push_aside(&mut self, id: CarId) -> bool {
        let Some(off) = self.aside_offset(id) else { return false };
        let c = &mut self.cars[id];
        if !c.crashed() || c.lateral >= off {
            return false;
        }
        c.lateral = off;
        true
    }

    /// A lane with a shoulder on its right: a kerb lane, or the short
    /// straight connector where a street runs on through a bend or a change
    /// of road (no junction), between kerb lanes.
    fn kerbside(&self, lane: &Lane) -> bool {
        if !lane.is_connector() {
            return lane.right.is_none();
        }
        let kerb = |l: &LaneId| self.net.lanes.get(*l as usize).is_some_and(|x| !x.is_connector() && x.right.is_none());
        lane.turn == 0 && lane.length < 8.0 && lane.next.len() == 1 && lane.next.iter().all(kerb) && lane.prev.len() == 1 && lane.prev.iter().all(kerb)
    }

    /// A stopped vehicle the traffic behind may ease past.
    pub(crate) fn is_hazard(&self, o: &Car) -> bool {
        o.crashed() && o.lateral >= HAZARD_ASIDE && !o.entering && !o.in_stunt()
    }

    /// The sideways offset (m, + right) `id` drives at to pass the hazard
    /// `h` on `lane` (the lane `h` stands on), and its speed cap; `None`
    /// when there is no room.
    fn pass_side(&self, id: CarId, h: CarId, lane: &Lane) -> Option<(f32, f32)> {
        if !self.kerbside(lane) {
            return None;
        }
        let (c, hc) = (&self.cars[id], &self.cars[h]);
        let half = body_width(c) * 0.5;
        // The hazard's near edge, less the clearance: our right edge goes no
        // further.
        let right = hc.lateral - body_width(hc) * 0.5 - HAZARD_CLEAR;
        let side = (right - half).min(0.0);
        // The far side: a little of the lane beside or over the centre line,
        // else the shoulder of a one-way road.
        let beside = lane.left.is_some() || self.net.opposing(lane).is_some();
        let left = -LANE_WIDTH * 0.5 - if beside { INTO_NEXT_LANE } else { SHOULDER - EDGE_CLEAR };
        if side - half < left {
            return None;
        }
        let cap = if right - half > EASY_ROOM { HAZARD_EASY } else { HAZARD_CRAWL };
        Some((side, cap))
    }

    /// The hazard `id` is coming up to, if any: the vehicle right ahead on
    /// its lane, or the first on the next lane of its route when it is last
    /// on its own. With where (in its lane's metres) the hazard's lane
    /// starts.
    fn hazard_ahead(&self, id: CarId) -> Option<(CarId, f32)> {
        let c = &self.cars[id];
        let here = c.route[c.idx];
        let lane = &self.net.lanes[here as usize];
        let k = lane.cars.iter().position(|&o| o == id)?;
        if let Some(&l) = lane.cars.get(k + 1) {
            let lc = &self.cars[l];
            return (self.is_hazard(lc) && lc.s - lc.length() - c.s < HAZARD_LOOK).then_some((l, 0.0));
        }
        let rem = lane.length - c.s;
        if rem > HAZARD_LOOK {
            return None;
        }
        let next = *c.route.get(c.idx + 1)?;
        if lane.is_beside(next) {
            return None;
        }
        let &l = self.net.lanes[next as usize].cars.first()?;
        let lc = &self.cars[l];
        (self.is_hazard(lc) && rem + lc.s - lc.length() < HAZARD_LOOK).then_some((l, lane.length))
    }

    /// What `id` follows once past hazard `h` (whose lane starts at `base`
    /// in its lane's metres): the next vehicle beyond it on its lane, else
    /// the first on the lane after, else the stop line of a red light
    /// there. In `id`'s lane metres, with its speed.
    fn beyond_hazard(&self, id: CarId, h: CarId, base: f32) -> Option<(f32, f32)> {
        let c = &self.cars[id];
        let hc = &self.cars[h];
        let hl = hc.route[hc.idx];
        let lane = &self.net.lanes[hl as usize];
        let k = lane.cars.iter().position(|&o| o == h)?;
        // Skipping ourselves: drawn level, we may be ahead of it in the
        // lane's order already.
        if let Some(&o) = lane.cars[k + 1..].iter().find(|&&o| o != id) {
            let oc = &self.cars[o];
            return Some((base + oc.s - oc.length(), oc.v));
        }
        // The lane after the hazard's on our route.
        let at = c.route.iter().skip(c.idx).position(|&l| l == hl).map(|p| p + c.idx)?;
        let next = *c.route.get(at + 1)?;
        let nl = &self.net.lanes[next as usize];
        if nl.is_connector() && (!nl.green || nl.blocked) {
            return Some((base + lane.length - 0.5, 0.0));
        }
        if let Some(&o) = nl.cars.first() {
            let oc = &self.cars[o];
            return Some((base + lane.length + oc.s - oc.length(), oc.v));
        }
        None
    }

    /// Plan every ordinary driver's way past a hazard for this substep
    /// (sirens, fugitives and recovery trucks plan theirs in
    /// `plan_offlaw`), easing the sideways offset; a driver past it goes
    /// back to `Lane` and drifts back to the lane centre there.
    pub(super) fn plan_hazard_passes(&mut self, dt: f32) {
        // Lanes with a hazard on them, and whoever may be coming up to one.
        let mut lanes: HashSet<LaneId> = HashSet::new();
        for c in self.cars.values().filter(|c| self.is_hazard(c)) {
            let l = c.route[c.idx];
            lanes.insert(l);
            lanes.extend(self.net.lanes[l as usize].prev.iter().copied());
        }
        let ids: Vec<CarId> = self
            .cars
            .iter()
            .filter(|(_, c)| !c.offlaw() && (c.passing_hazard() || lanes.contains(&c.route[c.idx])))
            .map(|(id, _)| id)
            .collect();
        for id in ids {
            let c = &self.cars[id];
            let free = !c.entering && !c.crashed() && !c.in_stunt() && c.hold <= 0.0 && !c.passing;
            let o = c.drive.offlaw;
            // Still beside (or not yet clear of) the one being passed.
            let alongside = o.hazard.filter(|&h| {
                self.cars.get(h).is_some_and(|hc| self.is_hazard(hc) && hc.route[hc.idx] == c.route[c.idx] && c.s - c.length() < hc.s + 0.5 && c.s > hc.s - hc.length() - 1.0)
            });
            let target = if !free {
                None
            } else if let Some(h) = alongside {
                let base = 0.0;
                self.pass_side(id, h, &self.net.lanes[self.cars[h].route[self.cars[h].idx] as usize]).map(|(side, cap)| (h, side, cap, self.beyond_hazard(id, h, base)))
            } else {
                self.hazard_ahead(id).and_then(|(h, base)| {
                    let lane = &self.net.lanes[self.cars[h].route[self.cars[h].idx] as usize];
                    self.pass_side(id, h, lane).map(|(side, cap)| (h, side, cap, self.beyond_hazard(id, h, base)))
                })
            };
            let mut plan = OffLaw { side: o.side, ..Default::default() };
            match target {
                Some((h, side, cap, lead)) => {
                    plan.mode = OffLawMove::Hazard;
                    plan.target = side;
                    plan.cap = Some(cap);
                    plan.lead = lead;
                    plan.hazard = Some(h);
                }
                // Past it (or no room): back to the lane, where
                // `plan_offlaw` eases the offset back.
                None => plan.target = 0.0,
            }
            let rate = 2.2 * dt * (c.v / 3.0).clamp(0.4, 1.0);
            let step = (plan.target - plan.side).clamp(-rate, rate);
            plan.side += step;
            plan.slew = step / dt.max(1e-3);
            if plan.mode != OffLawMove::Hazard && plan.side.abs() < 1e-3 {
                plan.side = 0.0;
            }
            self.cars[id].drive.offlaw = plan;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::citizens::CitizenId;
    use crate::vehicle::BodyType;

    fn send(t: &mut Traffic, roads: &RoadNetwork, from: Vec2, to: Vec2) -> CarId {
        let (fs, _, _) = roads.nearest_street(from).unwrap();
        let (ts, _, _) = roads.nearest_street(to).unwrap();
        let (route, s0, s1) = t.net.find_route(fs, from, ts, to).expect("route");
        t.spawn_car(CitizenId::default(), None, route, s0, s1, 0)
    }

    /// A two-way street with one lane each way, a wreck of `body` in the
    /// eastbound lane at x = 0 and a stream of eastbound cars behind it.
    /// Returns how many got past it in `secs` and the wreck.
    fn stream_past(body: BodyType, push: bool, secs: usize) -> (usize, usize, Traffic, CarId) {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-600.0, 0.0), Vec2::new(600.0, 0.0)], 1, 1);
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let y = 0.5 * LANE_WIDTH;
        let wreck = send(&mut t, &roads, Vec2::new(-20.0, y), Vec2::new(500.0, y));
        t.cars[wreck].model.body = body;
        for _ in 0..3 {
            t.step(1.0, 4);
        }
        t.crash(vec![wreck], CrashCause::OffRoad, 30.0);
        assert!(t.cars[wreck].drive.crash.is_some_and(|c| c.wrecked));
        if push {
            assert!(t.push_aside(wreck), "pushed onto the shoulder");
        }
        let wx = t.cars[wreck].pos.x;
        let mut cars = Vec::new();
        for k in 0..secs {
            // Sent while there is time for them to get there and by.
            if k % 6 == 0 && k + 90 < secs {
                let c = send(&mut t, &roads, Vec2::new(-400.0, y), Vec2::new(550.0, y));
                t.cars[c].drive.profile = DriverProfile::professional(k as f32 * 0.01);
                cars.push(c);
            }
            t.step(1.0, 4);
        }
        let past = cars.iter().filter(|&&c| t.cars.get(c).is_none_or(|c| c.pos.x > wx + 5.0)).count();
        (past, cars.len(), t, wreck)
    }

    /// A motorcycle written off in its lane, pushed onto the shoulder:
    /// everyone eases past it, slowly, without touching it; left in the
    /// middle of the lane it held them all.
    #[test]
    fn traffic_eases_past_a_wreck_pushed_onto_the_shoulder() {
        let (held, n, _, _) = stream_past(BodyType::Motorcycle, false, 240);
        assert_eq!(held, 0, "a wreck left in the lane holds everyone ({n} cars)");
        let (past, n, t, wreck) = stream_past(BodyType::Motorcycle, true, 240);
        assert!(past * 10 >= n * 7, "most of the stream got past: {past} of {n}");
        assert!(t.crashes.iter().all(|c| c.cars == vec![wreck]), "nobody ran into the wreck");
        assert!(t.cars[wreck].lateral >= HAZARD_ASIDE, "on the shoulder");
        // A written-off car leaves room too, on a street with a shoulder.
        let (past, n, _, _) = stream_past(BodyType::Sedan, true, 240);
        assert!(past * 10 >= n * 7, "past a car wreck: {past} of {n}");
    }

    /// Going by, a driver slows to a crawl and keeps its body clear of the
    /// wreck's; a truck too wide for the room left waits behind instead.
    #[test]
    fn passing_is_slow_and_needs_room() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-600.0, 0.0), Vec2::new(600.0, 0.0)], 1, 1);
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let y = 0.5 * LANE_WIDTH;
        let wreck = send(&mut t, &roads, Vec2::new(-20.0, y), Vec2::new(500.0, y));
        t.cars[wreck].model.body = BodyType::Sedan;
        t.step(1.0, 4);
        t.crash(vec![wreck], CrashCause::OffRoad, 30.0);
        t.push_aside(wreck);
        let car = send(&mut t, &roads, Vec2::new(-200.0, y), Vec2::new(500.0, y));
        t.cars[car].drive.profile = DriverProfile::professional(0.2);
        let (mut beside_v, mut gap): (f32, f32) = (0.0, f32::MAX);
        for _ in 0..90 {
            t.step(1.0, 4);
            let (c, w) = (&t.cars[car], &t.cars[wreck]);
            if (c.pos.x - w.pos.x).abs() < 3.0 {
                beside_v = beside_v.max(c.v);
                let right = c.lateral + c.drive.offlaw.side + CAR_WIDTH * 0.5;
                gap = gap.min(w.lateral - CAR_WIDTH * 0.5 - right);
            }
        }
        assert!(t.cars[car].pos.x > t.cars[wreck].pos.x + 10.0, "got past");
        assert!(beside_v > 0.0 && beside_v <= HAZARD_EASY + 0.5, "slow going by: {beside_v:.1} m/s");
        assert!(gap >= HAZARD_CLEAR - 0.05, "clear of the wreck: {gap:.2} m");
        // A lorry fits past a car wreck; past a lorry only half onto the
        // shoulder, it does not.
        let lane = t.cars[wreck].route[t.cars[wreck].idx] as usize;
        t.cars[car].model.body = BodyType::Semi;
        assert!(t.pass_side(car, wreck, &t.net.lanes[lane]).is_some(), "a lorry past a car on the shoulder");
        t.cars[wreck].model.body = BodyType::Semi;
        t.cars[wreck].lateral = HAZARD_ASIDE;
        assert!(t.pass_side(car, wreck, &t.net.lanes[lane]).is_none(), "no room for a lorry past a lorry");
    }

    /// Only a kerb lane has a shoulder to push onto; a crash in an inner
    /// lane or a junction stays put.
    #[test]
    fn only_kerb_lanes_have_room_aside() {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-300.0, 0.0), Vec2::new(300.0, 0.0)], 2, 2);
        let mut t = Traffic::default();
        t.rebuild(&roads);
        let inner = send(&mut t, &roads, Vec2::new(-100.0, 0.5 * LANE_WIDTH), Vec2::new(250.0, 0.5 * LANE_WIDTH));
        let kerb = send(&mut t, &roads, Vec2::new(-100.0, 1.5 * LANE_WIDTH), Vec2::new(250.0, 1.5 * LANE_WIDTH));
        t.step(1.0, 4);
        t.crash(vec![inner], CrashCause::OffRoad, 30.0);
        t.crash(vec![kerb], CrashCause::OffRoad, 30.0);
        let (li, lk) = (&t.net.lanes[t.cars[inner].route[t.cars[inner].idx] as usize], &t.net.lanes[t.cars[kerb].route[t.cars[kerb].idx] as usize]);
        assert!(li.right.is_some() && lk.right.is_none(), "one inner, one kerb lane");
        assert!(!t.push_aside(inner));
        assert!(t.push_aside(kerb));
        assert!(!t.push_aside(kerb), "idempotent");
    }
}
