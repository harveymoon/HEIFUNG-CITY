//! [moto] Moto packs: on some evenings the local gang members on two
//! wheels (the takeover crew who ride bikes) meet a motorcycle club from
//! out of town at a road end and ride through town together as a pack: a
//! loose column on the same route (through the nightlife, else the middle
//! of town), fast, close, overtaking and weaving through ordinary traffic,
//! the leader through every red and the rest through some. Locals peel off
//! home once the pack is through the nightlife; the club rides on off the
//! map. Somebody usually calls it in, and a police car that comes near a
//! local rider may give chase (`World::start_chase`). The data is in
//! `crate::street_events` (`MotoPack`); decisions hash the seed, the day
//! and the ids, and the few streams come from `rng_for`, never the
//! world's RNG.

use super::chases::Getaway;
use super::*;
use crate::pedestrians::hash01;
use crate::services::{CrimeKind, Outcome, Stage as IncidentStage, UnitId, UnitKind, UnitState};
use crate::street_events::*;
use crate::traffic::{DriverProfile, GangPhase, GangRole, LaneId, LaneKind};
use crate::vehicle::{BodyType, Livery, VehicleClass, VehicleModel};
use rand::Rng;

fn key_of<K: slotmap::Key>(k: K) -> u64 {
    k.data().as_ffi()
}

/// How a pack rider rides: fast and close, overtaking anything slower; the
/// leader through every light, the others through a late red now and then.
/// `h` (`0..1`) spreads the riders a little.
pub fn moto_profile(phase: f32, leader: bool, h: f32) -> DriverProfile {
    DriverProfile {
        assigned: true,
        speed: 1.3 + 0.15 * h,
        headway: 0.7,
        accel: 2.6,
        reaction: 0.4,
        compliance: if leader { 0.0 } else { 0.08 + 0.25 * h },
        aggression: 1.0,
        skill: 0.8,
        drunk: 0.0,
        ignores_signals: leader,
        phase,
    }
}

impl World {
    /// The takeover crew who own a two-wheeler, best first.
    fn moto_crew(&self) -> Vec<CitizenId> {
        self.takeover_crew().into_iter().filter(|&id| self.citizens[id].car.and_then(|v| self.vehicles.cars.get(v)).is_some_and(|v| v.model.body.two_wheeler() && v.spot().is_some())).collect()
    }

    /// Every tick (from `step_nights`): roll tonight's ride, start it, run it.
    pub(super) fn step_moto(&mut self) {
        let tick = self.clock.tick;
        let day = self.clock.day();
        if self.clock.minute_of_day() == MOTO_ROLL_MINUTE as u64 && self.street_events.moto_rolled_day != day && self.street_events.moto.is_none() {
            self.street_events.moto_rolled_day = day;
            let h = hash01(self.config.seed ^ 0x070_7AC4, day, 1);
            let takeover = self.driving.takeovers.active.is_some() || self.driving.takeovers.planned.is_some();
            if day >= SCENE_FROM_DAY && self.citizens.len() >= 60 && !takeover && h < MOTO_NIGHT_CHANCE && !self.moto_crew().is_empty() {
                let window = MOTO_START_WINDOW.1 - MOTO_START_WINDOW.0;
                let k = (hash01(self.config.seed ^ 0x070_7AC4, day, 2) * window as f32) as u64;
                self.street_events.moto_planned = Some(tick + MOTO_START_WINDOW.0 + k);
            }
        }
        if self.street_events.moto_planned.is_some_and(|t| tick >= t) {
            self.street_events.moto_planned = None;
            let _ = self.start_moto_pack(None);
        }
        if self.street_events.moto.is_some() {
            self.run_moto();
        }
    }

    /// The lane in from road end `n` (starting at it), if a street leads in.
    fn lane_in_from(&self, n: NodeId) -> Option<LaneId> {
        let at = self.roads.node(n).pos;
        let sid = *self.roads.node(n).segments.iter().find(|&&s| self.roads.segment(s).alive && self.roads.segment(s).is_street())?;
        let net = &self.traffic.net;
        net.segment_lanes.get(&sid)?.iter().copied().filter(|&l| net.lanes[l as usize].start().distance(at) < net.lanes[l as usize].end().distance(at)).min_by_key(|&l| (net.lanes[l as usize].right.is_some(), l))
    }

    /// The lane out to road end `n` (ending at it).
    fn lane_out_to(&self, n: NodeId) -> Option<LaneId> {
        let at = self.roads.node(n).pos;
        let sid = *self.roads.node(n).segments.iter().find(|&&s| self.roads.segment(s).alive && self.roads.segment(s).is_street())?;
        let net = &self.traffic.net;
        net.segment_lanes.get(&sid)?.iter().copied().filter(|&l| net.lanes[l as usize].end().distance(at) < net.lanes[l as usize].start().distance(at)).min()
    }

    /// Two legs as one route: from `from` (lane, s) by `via` to `to`.
    fn route_via(&self, from: (LaneId, f32), via: (LaneId, f32), to: (LaneId, f32)) -> Option<(Vec<LaneId>, f32, f32)> {
        let net = &self.traffic.net;
        let (mut a, s0, s1) = net.route_lanes(from.0, from.1, via.0, via.1)?;
        let (b, _, s2) = net.route_lanes(via.0, s1, to.0, to.1)?;
        a.extend(b.into_iter().skip(1));
        Some((a, s0, s2))
    }

    /// Where the pack rides through: a nightlife street near the middle of
    /// town, else the lot nearest the middle. Its car lane and the name.
    fn moto_via(&self) -> Option<(LaneId, f32, Vec2, String)> {
        let lots: Vec<&Lot> = self.lots.values().filter(|l| l.car_segment.is_some()).collect();
        if lots.is_empty() {
            return None;
        }
        let mid = lots.iter().map(|l| l.center).sum::<Vec2>() / lots.len() as f32;
        let best = |fun: bool| lots.iter().filter(|l| !fun || l.land_use == LandUse::Entertainment).min_by(|a, b| a.center.distance(mid).total_cmp(&b.center.distance(mid))).copied();
        let lot = best(true).or_else(|| best(false))?;
        let (lane, s, _) = self.traffic.net.nearest_lane_on(lot.car_segment?, lot.car_point)?;
        let name = lot.building.and_then(|b| self.buildings.get(b)).map(|b| b.name.clone()).or_else(|| self.building_near(lot.center).and_then(|b| self.buildings.get(b)).map(|b| b.name.clone())).unwrap_or_else(|| "the middle of town".into());
        Some((lane, s, lot.car_point, name))
    }

    /// [moto] Start a moto pack ride now: the road end nearest `near` (or
    /// one picked for the day) is the meet. Returns its id.
    pub fn start_moto_pack(&mut self, near: Option<Vec2>) -> Result<u32, String> {
        if self.street_events.moto.is_some() {
            return Err("a moto pack is already riding".into());
        }
        let tick = self.clock.tick;
        // Road ends to ride in by and out by: the city's own, and the
        // highway's carriageways at the map edge (one-way: in on one, out
        // on another; a city reached only by the highway has no other).
        let (hw_in, hw_out) = self.highway_ends();
        let mut ends: Vec<NodeId> = self.city_leaves();
        ends.extend(hw_in.iter().chain(hw_out.iter()).map(|e| e.0));
        ends.sort_by_key(|n| n.0);
        ends.dedup();
        let leaves: Vec<NodeId> = ends.iter().copied().filter(|&n| self.lane_in_from(n).is_some()).collect();
        let outs: Vec<NodeId> = ends.iter().copied().filter(|&n| self.lane_out_to(n).is_some()).collect();
        if leaves.is_empty() || outs.iter().all(|o| leaves == [*o]) {
            return Err("no road ends to ride in and out by".into());
        }
        let (via_lane, via_s, via, via_name) = self.moto_via().ok_or("nowhere to ride to")?;
        let id = self.street_events.next_id;
        let pick = |w: &World, ns: &[NodeId]| -> Vec<NodeId> {
            let mut ns = ns.to_vec();
            match near {
                Some(q) => ns.sort_by(|a, b| w.roads.node(*a).pos.distance(q).total_cmp(&w.roads.node(*b).pos.distance(q)).then(a.0.cmp(&b.0))),
                None => ns.sort_by_key(|n| (hash01(w.config.seed ^ 0x070_7E47, n.0 as u64, tick / 1440) * 1e6) as u64),
            }
            ns
        };
        // The meet: a road end the pack can ride in from, through the
        // nightlife and out at another (the one farthest away).
        let mut plan = None;
        'outer: for entry in pick(self, &leaves) {
            let lane = self.lane_in_from(entry).unwrap();
            let len = self.traffic.net.lanes[lane as usize].length;
            let meet_s = (len * 0.5).min(40.0).max(0.5);
            let at = self.roads.node(entry).pos;
            let mut exits: Vec<NodeId> = outs.iter().copied().filter(|&n| n != entry).collect();
            exits.sort_by(|a, b| self.roads.node(*b).pos.distance(at).total_cmp(&self.roads.node(*a).pos.distance(at)).then(a.0.cmp(&b.0)));
            for exit in exits {
                let out = self.lane_out_to(exit).unwrap();
                let out_s = (self.traffic.net.lanes[out as usize].length - 1.0).max(0.0);
                if self.route_via((lane, 0.5), (via_lane, via_s), (out, out_s)).is_some() {
                    plan = Some((entry, lane, meet_s, (out, out_s)));
                    break 'outer;
                }
            }
        }
        let (entry, meet_lane, meet_s, exit) = plan.ok_or("no way through town for a pack")?;
        let meet = self.traffic.net.lanes[meet_lane as usize].point_at(meet_s).0;
        let place = self.building_near(meet).and_then(|b| self.buildings.get(b)).map(|b| b.name.clone()).unwrap_or_else(|| "the edge of town".into());
        let mut rng = crate::rng_for(self.config.seed ^ 0x070_70C5, tick);
        let outsiders = rng.random_range(MOTO_OUTSIDERS.0..=MOTO_OUTSIDERS.1);
        let mut pack = MotoPack { id, stage: MotoStage::Gathering, entry, meet_lane, meet_s, meet, via, via_name, exit, place: place.clone(), started: tick, set_off: None, riders: Vec::new(), to_spawn: outsiders, outsiders: 0, incident: None, chase: None, chase_tried: false, red_runs: 0, lane_changes: 0, locals: 0 };
        let LaneKind::Road { segment: meet_seg, .. } = self.traffic.net.lanes[meet_lane as usize].kind else { return Err("the meet is not on a street".into()) };
        for cid in self.moto_crew().into_iter().take(MOTO_LOCALS) {
            let Some(c) = self.citizens.get(cid) else { continue };
            let here = c.inside.and_then(|b| self.building_entrance(b)).unwrap_or(c.pos);
            let home = c.home;
            let Some((vid, _)) = self.car_ready(cid, here) else { continue };
            let Some(car) = self.drive_from_spot(vid, meet_seg, meet, cid, crate::vehicle::Errand::Trip) else { continue };
            self.leave_building(cid);
            let phase = hash01(key_of(car), 0x070_7AFE, 1);
            let tc = &mut self.traffic.cars[car];
            tc.drive.gang = Some(GangRole { takeover: id, phase: GangPhase::Joining, race: false, pack: true });
            tc.drive.profile = moto_profile(phase, false, hash01(key_of(car), 0x070_7AFE, 2));
            let (pos, model) = (tc.pos, tc.model);
            let c = &mut self.citizens[cid];
            c.asleep = false;
            c.pos = pos;
            c.prev_pos = pos;
            c.vehicle = Some(model);
            c.to_car = None;
            c.activity = Activity::Driving { car, to: home, purpose: Purpose::Wander };
            c.remember(tick, format!("The crew's riding tonight. Meeting the club at {place}."));
            c.thought = "Ride night.".to_string();
            pack.riders.push(Rider { car, citizen: Some(cid), riding: false, last_run: None, lane: None });
            pack.locals += 1;
        }
        self.street_events.next_id += 1;
        self.street_events.packs_total += 1;
        let n = pack.riders.len() as u32 + pack.to_spawn;
        self.street_events.moto = Some(pack);
        self.news(EventKind::MotoPack, format!("Bikers gathering at {place}: a pack of {n} about to ride through town"), meet);
        Ok(id)
    }

    /// One tick of the pack.
    fn run_moto(&mut self) {
        let Some(mut p) = self.street_events.moto.take() else { return };
        let tick = self.clock.tick;
        p.riders.retain(|r| self.traffic.cars.get(r.car).is_some_and(|c| c.drive.gang.is_some_and(|g| g.pack && g.takeover == p.id)));
        match p.stage {
            MotoStage::Gathering => {
                // Locals at the meet wait at the kerb, out of the lane.
                for r in p.riders.iter_mut().filter(|r| !r.riding) {
                    let c = &self.traffic.cars[r.car];
                    let there = c.idx + 1 == c.route.len() && c.s >= c.s_end - 2.0 && c.v < 1.0;
                    if there || (c.entering && c.pos.distance(p.meet) < 30.0) {
                        r.riding = true;
                        if !c.entering {
                            self.traffic.pull_in(r.car, 1.0e6);
                        }
                    }
                }
                let ready = p.riders.iter().all(|r| r.riding);
                if ready || tick >= p.started + MOTO_GATHER {
                    // Whoever has not made it goes home.
                    let late: Vec<CarId> = p.riders.iter().filter(|r| !r.riding).map(|r| r.car).collect();
                    for car in late {
                        self.release_rider(car);
                    }
                    p.riders.retain(|r| r.riding);
                    p.stage = MotoStage::Rolling;
                }
            }
            MotoStage::Rolling => {
                self.spawn_club(&mut p);
                {
                    // The column comes by: the locals pull out into it.
                    let locals: Vec<(CarId, CitizenId)> = p.riders.iter().filter_map(|r| r.citizen.map(|c| (r.car, c))).collect();
                    for (car, cid) in locals {
                        if !self.set_off_local(car, cid, &p) {
                            self.release_rider(car);
                        }
                    }
                    p.riders.retain(|r| self.traffic.cars.get(r.car).is_some_and(|c| c.drive.gang.is_some_and(|g| g.pack)));
                    p.stage = MotoStage::Riding;
                    p.set_off = Some(tick);
                    let n = p.riders.len();
                    self.news(EventKind::MotoPack, format!("A pack of {n} bikes tearing through town from {} towards {}", p.place, p.via_name), p.meet);
                }
            }
            MotoStage::Riding => self.ride(&mut p),
        }
        let over = match p.stage {
            MotoStage::Riding => p.riders.is_empty() || tick >= p.set_off.unwrap_or(tick) + MOTO_MAX,
            _ => p.riders.is_empty() && p.to_spawn == 0 && p.stage != MotoStage::Gathering || tick >= p.started + MOTO_GATHER * 4,
        };
        if over {
            self.end_moto(p);
        } else {
            self.street_events.moto = Some(p);
        }
    }

    /// The club rolls in at the road end all at once, in a loose column
    /// (two abreast where the street has two lanes in).
    fn spawn_club(&mut self, p: &mut MotoPack) {
        let tick = self.clock.tick;
        let mut rng = crate::rng_for(self.config.seed ^ 0x070_7C1B, tick);
        let lanes: Vec<LaneId> = std::iter::once(p.meet_lane).chain(self.traffic.net.lanes[p.meet_lane as usize].left).collect();
        let via = self.roads.nearest_street(p.via).and_then(|(s, _, _)| self.traffic.net.nearest_lane_on(s, p.via)).map(|(l, s, _)| (l, s));
        let Some(via) = via else {
            p.to_spawn = 0;
            return;
        };
        let n = p.to_spawn as usize;
        // Front of the column first, so each joins its lane behind the last.
        for k in (0..n).rev() {
            let lane = lanes[k % lanes.len()];
            let len = self.traffic.net.lanes[lane as usize].length;
            let s = (0.5 + 9.0 * (k / lanes.len()) as f32).min((len - 1.0).max(0.5));
            let Some((route, s0, s1)) = self.route_via((lane, s), via, p.exit) else { continue };
            let car = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, 0);
            let body = if rng.random_bool(0.4) { BodyType::AkiraBike } else { BodyType::Motorcycle };
            let leader = k == 0;
            let phase = hash01(key_of(car), 0x070_7AFE, 1);
            let tc = &mut self.traffic.cars[car];
            tc.model = VehicleModel { class: VehicleClass::Expensive, body, wear: 1, tuned: false, livery: Livery::None, paint: rng.random_range(0..crate::vehicle::PAINTS) };
            tc.drive.gang = Some(GangRole { takeover: p.id, phase: GangPhase::Leaving, race: false, pack: true });
            tc.drive.profile = moto_profile(phase, leader, hash01(key_of(car), 0x070_7AFE, 2));
            p.riders.push(Rider { car, citizen: None, riding: true, last_run: None, lane: None });
            p.outsiders += 1;
        }
        p.to_spawn = 0;
    }

    /// A local at the meet pulls out with the pack: through `via`, then home.
    fn set_off_local(&mut self, car: CarId, cid: CitizenId, p: &MotoPack) -> bool {
        let Some(c) = self.traffic.cars.get(car) else { return false };
        let from = (c.route[c.idx], c.s);
        let home = self.citizens.get(cid).and_then(|x| x.home).and_then(|h| self.building_lot(h)).and_then(|l| l.car_segment.map(|s| (s, l.car_point)));
        let Some(home) = home.and_then(|(s, q)| self.traffic.net.nearest_lane_on(s, q)).map(|(l, s, _)| (l, s)) else { return false };
        let Some(via) = self.roads.nearest_street(p.via).and_then(|(s, _, _)| self.traffic.net.nearest_lane_on(s, p.via)).map(|(l, s, _)| (l, s)) else { return false };
        let Some((route, _, s_end)) = self.route_via(from, via, home) else { return false };
        self.traffic.reroute(car, route, from.1, s_end);
        let tc = &mut self.traffic.cars[car];
        tc.hold = 0.0;
        if let Some(g) = tc.drive.gang.as_mut() {
            g.phase = GangPhase::Leaving;
        }
        let leader = p.outsiders == 0 && p.riders.first().is_some_and(|r| r.car == car);
        tc.drive.profile = moto_profile(tc.drive.profile.phase, leader, hash01(key_of(car), 0x070_7AFE, 2));
        let c = &mut self.citizens[cid];
        if let Activity::Driving { purpose, .. } = &mut c.activity {
            *purpose = Purpose::Home;
        }
        true
    }

    /// A local leaves the pack (gone home, never made the meet): an ordinary
    /// rider again, bound for home.
    fn release_rider(&mut self, car: CarId) {
        let Some(c) = self.traffic.cars.get(car) else { return };
        let driver = c.citizen;
        let home = self.citizens.get(driver).and_then(|p| p.home);
        let goal = home.and_then(|h| self.building_lot(h)).and_then(|l| l.car_segment.map(|s| (s, l.car_point))).and_then(|(s, q)| self.traffic.net.nearest_lane_on(s, q));
        let (here, s, entering) = (c.route[c.idx], c.s, c.entering);
        let on_home_route = goal.is_some_and(|(g, _, _)| c.route.last() == Some(&g));
        if !on_home_route {
            if let Some((g, gs, _)) = goal {
                if let Some((route, _, s1)) = self.traffic.net.route_lanes(here, s, g, gs) {
                    self.traffic.reroute(car, route, s, s1);
                }
            }
        }
        let tc = &mut self.traffic.cars[car];
        if entering {
            tc.hold = 0.0;
        }
        tc.drive.gang = None;
        tc.drive.profile.assigned = false;
        tc.drive.calm = None;
        if let Some(p) = self.citizens.get_mut(driver) {
            if let Activity::Driving { purpose, to, .. } = &mut p.activity {
                *purpose = Purpose::Home;
                *to = home;
            }
        }
    }

    /// The pack on the streets: keep the column together, count the reds
    /// and the weaving, peel the locals off home, the 911 call, a chase.
    fn ride(&mut self, p: &mut MotoPack) {
        let tick = self.clock.tick;
        if p.riders.is_empty() {
            return;
        }
        let pts: Vec<(Vec2, f32)> = p.riders.iter().map(|r| (self.traffic.cars[r.car].pos, self.traffic.cars[r.car].heading)).collect();
        let centre = pts.iter().map(|x| x.0).sum::<Vec2>() / pts.len() as f32;
        let dir = pts.iter().map(|x| Vec2::new(x.1.cos(), x.1.sin())).sum::<Vec2>().normalize_or_zero();
        let mut peel = Vec::new();
        for r in p.riders.iter_mut() {
            let c = &mut self.traffic.cars[r.car];
            // A loose column: the front eases off, the back opens up.
            let base = 1.3 + 0.15 * hash01(key_of(r.car), 0x070_7AFE, 2);
            let ahead = (c.pos - centre).dot(dir);
            c.drive.profile.speed = if ahead > MOTO_SPREAD { base - 0.18 } else if ahead < -MOTO_SPREAD { base + 0.15 } else { base };
            if let Some(run) = c.drive.running.filter(|l| r.last_run != Some(*l)) {
                r.last_run = Some(run);
                p.red_runs += 1;
            }
            let lane = c.route[c.idx];
            if let Some(old) = r.lane.filter(|&o| o != lane) {
                if self.traffic.net.lanes[lane as usize].is_beside(old) {
                    p.lane_changes += 1;
                }
            }
            r.lane = Some(lane);
            // Through the nightlife: the locals go home from here.
            if r.citizen.is_some() && c.pos.distance(p.via) < 45.0 {
                peel.push(r.car);
            }
        }
        for car in peel {
            self.release_rider(car);
            if let Some(cid) = self.traffic.cars.get(car).map(|c| c.citizen).and_then(|c| self.citizens.get_mut(c)) {
                cid.remember(tick, format!("Rode with the pack through {}. Felt like flying.", p.via_name));
                cid.mood = (cid.mood + 0.08).min(1.0);
            }
        }
        p.riders.retain(|r| self.traffic.cars.get(r.car).is_some_and(|c| c.drive.gang.is_some_and(|g| g.pack)));
        let set_off = p.set_off.unwrap_or(tick);
        // Engines and a red run: somebody calls it in (most rides).
        if p.incident.is_none() && tick == set_off + 3 && hash01(self.config.seed ^ 0x070_7CA1, p.id as u64, 1) < 0.75 {
            if let Some(&Rider { car, .. }) = p.riders.first() {
                let at = self.traffic.cars[car].pos;
                let crew: Vec<CitizenId> = p.riders.iter().filter_map(|r| r.citizen).collect();
                let caller = self.witness(at, 300.0, &crew);
                if let Some(near) = self.building_near(at) {
                    if let Some(id) = self.report_crime(CrimeKind::RecklessRiding, near, None, None) {
                        if let Some(i) = self.services.incident_mut(id) {
                            i.pos = at;
                            i.on_road = true;
                            i.caller = caller;
                            i.report_at = tick;
                        }
                        p.incident = Some(id);
                    }
                }
                if let Some((who, c)) = caller.and_then(|c| self.citizens.get_mut(c).map(|x| (c, x))) {
                    c.remember(tick, "Called the police: a gang of bikes tearing down the street, straight through the lights.");
                    self.concerns.heard(who, c, crate::concerns::Concern::Safety, 0.3);
                }
            }
        }
        if p.chase.is_none() && !p.chase_tried {
            self.maybe_chase_pack(p);
        }
    }

    /// A police car near a local rider may go after them (once a ride).
    fn maybe_chase_pack(&mut self, p: &mut MotoPack) {
        let locals: Vec<(CarId, CitizenId, Vec2)> = p.riders.iter().filter_map(|r| r.citizen.map(|c| (r.car, c, self.traffic.cars[r.car].pos))).collect();
        if locals.is_empty() {
            return;
        }
        let mut best: Option<(f32, UnitId, CarId, CitizenId)> = None;
        for (uid, u) in self.services.units.iter() {
            if u.kind != UnitKind::PoliceCar || u.leaving {
                continue;
            }
            let ours = p.incident.is_some() && u.incident == p.incident;
            if !(u.state == UnitState::Patrol || (u.state == UnitState::Responding && ours)) {
                continue;
            }
            let Some(pos) = u.car.and_then(|c| self.traffic.cars.get(c)).map(|c| c.pos) else { continue };
            for &(car, cid, at) in &locals {
                let d = pos.distance(at);
                if d < MOTO_CHASE_REACH && best.is_none_or(|b| d < b.0) {
                    best = Some((d, uid, car, cid));
                }
            }
        }
        let Some((_, uid, car, cid)) = best else { return };
        p.chase_tried = true;
        if hash01(self.config.seed ^ 0x070_7C4A, p.id as u64, 2) >= MOTO_CHASE_CHANCE {
            return;
        }
        self.chase_rider(p, uid, car, cid);
    }

    /// Start the chase after local rider `cid` on `car` by unit `uid`.
    fn chase_rider(&mut self, p: &mut MotoPack, uid: UnitId, car: CarId, cid: CitizenId) -> bool {
        let at = self.traffic.cars[car].pos;
        let id = match p.incident.filter(|&i| self.services.incident(i).is_some_and(|x| x.active())) {
            Some(i) => i,
            None => {
                let Some(near) = self.building_near(at) else { return false };
                let Some(i) = self.report_crime(CrimeKind::RecklessRiding, near, None, None) else { return false };
                if let Some(x) = self.services.incident_mut(i) {
                    x.pos = at;
                    x.on_road = true;
                }
                p.incident = Some(i);
                i
            }
        };
        let tick = self.clock.tick;
        let name = self.citizens.get(cid).map(|c| c.name.clone()).unwrap_or_default();
        if let Some(i) = self.services.incident_mut(id) {
            i.suspect = Some(cid);
            i.involve(cid, &name, "suspect");
            i.unit = Some(uid);
            i.stage = IncidentStage::Dispatched;
            i.dispatched.get_or_insert(tick);
            i.arrived.get_or_insert(tick);
            i.reported.get_or_insert(tick);
        }
        self.pull_off_road(uid);
        if let Some(u) = self.services.units.get_mut(uid) {
            u.incident = Some(id);
        }
        // Out of the pack: the chase takes the bike from here.
        if let Some(tc) = self.traffic.cars.get_mut(car) {
            tc.drive.gang = None;
        }
        if !self.start_chase(id, uid, cid, Getaway::Driving(car)) {
            if let Some(tc) = self.traffic.cars.get_mut(car) {
                tc.drive.gang = Some(GangRole { takeover: p.id, phase: GangPhase::Leaving, race: false, pack: true });
            }
            if let Some(i) = self.services.incident_mut(id) {
                i.unit = None;
                i.stage = IncidentStage::Reported;
                i.suspect = None;
            }
            if let Some(u) = self.services.units.get_mut(uid) {
                u.incident = None;
            }
            return false;
        }
        p.riders.retain(|r| r.car != car);
        p.chase = Some(id);
        self.street_events.pack_chases_total += 1;
        self.news(EventKind::MotoPack, format!("Police chase a biker from the pack near {}", self.services.incident(id).map(|i| i.place.clone()).unwrap_or_default()), at);
        true
    }

    /// The ride is over: stragglers sent on, the record and the news.
    fn end_moto(&mut self, p: MotoPack) {
        let tick = self.clock.tick;
        for r in &p.riders {
            match r.citizen {
                Some(_) => self.release_rider(r.car),
                None => {
                    if self.traffic.cars.get(r.car).is_some_and(|c| c.idx + 1 < c.route.len()) {
                        continue;
                    }
                    if !self.redirect_out(r.car) {
                        self.retire_vehicle(r.car);
                    }
                }
            }
        }
        if let Some(i) = p.incident.filter(|_| p.chase.is_none()) {
            if self.services.incident(i).is_some_and(|i| i.active() && matches!(i.stage, IncidentStage::Unreported | IncidentStage::Reported | IncidentStage::Dispatched)) {
                self.resolve(i, Outcome::Escaped);
            }
        }
        let people = p.outsiders + p.locals;
        self.street_events.record(EventRecord {
            id: p.id,
            kind: EventKind::MotoPack,
            place: p.place.clone(),
            pos: p.meet,
            finish: Some(p.via_name.clone()),
            started: p.started,
            ended: tick,
            people,
            crowd: 0,
            winner: None,
            police: p.incident.is_some(),
            arrests: 0,
            fines: 0.0,
            complaints: u32::from(p.incident.is_some()),
            chase: p.chase.is_some(),
            culture: 0.0,
        });
        self.news(EventKind::MotoPack, format!("The moto pack is gone: {} reds run on the way through {}", p.red_runs, p.via_name), p.via);
    }

    /// [moto] The pack riding now, if any.
    pub fn moto_pack(&self) -> Option<&MotoPack> {
        self.street_events.moto.as_ref()
    }
}
