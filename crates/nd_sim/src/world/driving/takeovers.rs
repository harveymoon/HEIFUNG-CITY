//! [driving] Street takeovers: a crew holds a junction until the police come.

use super::*;

impl World {
    /// Locals who would join a takeover: young, rebellious, with wheels,
    /// free right now. Best candidates (flashiest rides) first.
    pub(in crate::world) fn takeover_crew(&self) -> Vec<CitizenId> {
        let mut out: Vec<(CitizenId, f32)> = self
            .citizens
            .iter()
            .filter(|(id, c)| (17..=36).contains(&c.age) && c.owns_car && c.car.is_some() && !self.services.in_custody(*id) && !self.is_patient(*id))
            .filter(|(_, c)| matches!(c.activity, Activity::AtHome | Activity::Visiting(_) | Activity::Idle))
            .filter_map(|(id, c)| {
                let (caution, hedonism) = driver_traits(c.seed);
                let r = rebelliousness(caution, hedonism);
                let flash = c.vehicle.map_or(0.0, |v| if v.tuned || v.body == BodyType::AkiraBike { 0.3 } else if v.body.two_wheeler() || v.body == BodyType::Coupe { 0.15 } else { 0.0 });
                (r >= 0.6).then_some((id, r + flash))
            })
            .collect();
        out.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        out.into_iter().map(|(id, _)| id).collect()
    }

    /// Junctions a takeover could hold: real crossings (three streets or
    /// more), nearest `near` first, else the ones by the nightlife.
    pub(super) fn takeover_junctions(&self, near: Option<Vec2>) -> Vec<(NodeId, Vec2, f32)> {
        let mut out: Vec<(NodeId, Vec2, f32, f32)> = self
            .traffic
            .net
            .intersections
            .iter()
            .filter(|i| i.approaches >= 3 && !i.incoming.is_empty())
            .filter(|i| !self.roads.node(i.node).segments.iter().any(|&s| self.roads.segment(s).is_highway()))
            .map(|i| {
                let p = self.roads.node(i.node).pos;
                let score = match near {
                    Some(q) => p.distance(q),
                    None => {
                        let fun = self.lots.values().filter(|l| l.land_use == LandUse::Entertainment && l.center.distance(p) < 250.0).count() as f32;
                        -(fun * 10.0 + i.approaches as f32 * 5.0 + if i.signalized { 5.0 } else { 0.0 })
                    }
                };
                (i.node, p, i.radius, score)
            })
            .collect();
        out.sort_by(|a, b| a.3.total_cmp(&b.3).then(a.0.0.cmp(&b.0.0)));
        out.into_iter().map(|(n, p, r, _)| (n, p, r)).collect()
    }

    /// Roll tonight's takeover at 22:00 and start it when it is due.
    pub(super) fn step_takeovers(&mut self) {
        let tick = self.clock.tick;
        let day = self.clock.day();
        if self.clock.minute_of_day() == 22 * 60 && self.driving.takeovers.rolled_day != day && self.driving.takeovers.active.is_none() {
            self.driving.takeovers.rolled_day = day;
            let mut rng = crate::rng_for(self.stream_seed(Stream::TakeoverDay), day);
            let crew = self.takeover_crew().len();
            let chance = if crew >= 2 { takeover::NIGHT_CHANCE + 0.1 } else { takeover::NIGHT_CHANCE * 0.5 };
            if self.citizens.len() >= 100 && rng.random_bool(chance) {
                self.driving.takeovers.planned = Some(tick + rng.random_range(40..200));
            }
        }
        if self.driving.takeovers.planned.is_some_and(|t| tick >= t) {
            self.driving.takeovers.planned = None;
            let _ = self.start_takeover(None);
        }
        if self.driving.takeovers.active.is_some() {
            self.run_takeover();
        }
    }

    /// Start a takeover now, at the junction nearest `near` (or the best one
    /// by the nightlife). Returns its id.
    pub fn start_takeover(&mut self, near: Option<Vec2>) -> Result<u32, String> {
        if self.driving.takeovers.active.is_some() {
            return Err("a takeover is already under way".into());
        }
        let tick = self.clock.tick;
        let Some(&(node, centre, jr)) = self.takeover_junctions(near).first() else { return Err("no junction to hold".into()) };
        let mut rng = crate::rng_for(self.stream_seed(Stream::TakeoverStart), tick);
        let id = self.driving.takeovers.next_id;
        self.driving.takeovers.next_id += 1;
        let place = self.building_near(centre).and_then(|b| self.buildings.get(b)).map(|b| b.name.clone()).unwrap_or_else(|| "a junction".to_string());
        // Outsiders come in off the highway (or the farthest road end) and
        // leave the same way.
        let (entries, exits) = self.highway_ends();
        let leaves = self.city_leaves();
        // The farthest road end with a way in to (or out from) the junction.
        let far = |w: &World, ns: Vec<NodeId>, inbound: bool| {
            let mut ns: Vec<NodeId> = ns.into_iter().filter(|&n| w.reaches(n, node, inbound)).collect();
            ns.sort_by(|a, b| w.roads.node(*b).pos.distance(centre).total_cmp(&w.roads.node(*a).pos.distance(centre)).then(a.0.cmp(&b.0)));
            ns.first().copied()
        };
        let entry = far(self, entries.iter().map(|e| e.0).collect(), true).or_else(|| far(self, leaves.clone(), true));
        let exit = far(self, exits.iter().map(|e| e.0).collect(), false).or_else(|| far(self, leaves.clone(), false)).or(entry);
        let outsiders = if entry.is_some() { rng.random_range(takeover::OUTSIDERS.0..=takeover::OUTSIDERS.1) } else { 0 };
        let mut t = Takeover {
            id,
            node,
            centre,
            radius: jr + 6.0,
            place: place.clone(),
            stage: Stage::Gathering,
            started: tick,
            show_from: None,
            ends_at: None,
            cars: Vec::new(),
            residents: Vec::new(),
            outsiders: 0,
            to_spawn: outsiders,
            entry,
            exit,
            crowd: Vec::new(),
            incident: None,
            arrests: 0,
            ring_used: 0,
            donuts: 0,
        };
        // The locals drive over from wherever they are.
        let crew: Vec<CitizenId> = self.takeover_crew().into_iter().take(takeover::MAX_RESIDENTS).collect();
        for cid in crew {
            if let Some(car) = self.drive_to_meet(cid, &t) {
                t.cars.push(car);
                t.residents.push(cid);
                let c = &mut self.citizens[cid];
                c.asleep = false;
                c.remember(tick, format!("Word went round: takeover at {place} tonight. Rolled out."));
                c.thought = "Takeover tonight. Let's go.".to_string();
            }
        }
        if t.cars.is_empty() && t.to_spawn == 0 {
            return Err("nobody to run it".into());
        }
        self.driving.takeovers.active = Some(t);
        self.driving.takeovers.total += 1;
        self.events.push(SimEvent::Takeover(id));
        Ok(id)
    }

    /// Can a car drive from road end `end` to the junction at `node` (or
    /// from the junction out to the end, `inbound` false)?
    pub(super) fn reaches(&self, end: NodeId, node: NodeId, inbound: bool) -> bool {
        let net = &self.traffic.net;
        let Some(inter) = net.node_intersection.get(node.0 as usize).and_then(|&i| net.intersections.get(i)) else { return false };
        let at = self.roads.node(end).pos;
        let lanes: Vec<crate::traffic::LaneId> = self.roads.node(end).segments.iter().filter_map(|s| net.segment_lanes.get(s)).flatten().copied().collect();
        let starts_here = |l: crate::traffic::LaneId| net.lanes[l as usize].start().distance(at) < net.lanes[l as usize].end().distance(at);
        let Some(&(junction_lane, _)) = inter.incoming.first() else { return false };
        if inbound {
            lanes.iter().filter(|&&l| starts_here(l)).any(|&l| net.route_lanes(l, 0.5, junction_lane, net.lanes[junction_lane as usize].length * 0.5).is_some())
        } else {
            let Some(&(c, _)) = inter.connectors.first() else { return false };
            let out = net.lanes[c as usize].next[0];
            lanes.iter().filter(|&&l| !starts_here(l)).any(|&l| net.route_lanes(out, 0.5, l, net.lanes[l as usize].length * 0.5).is_some())
        }
    }

    /// An incoming lane of the meet junction to aim a car at: the approach
    /// nearest `from`, spread by `k`.
    pub(super) fn meet_lane(&self, t: &Takeover, from: Vec2, k: usize) -> Option<(crate::traffic::LaneId, f32)> {
        let inter = self.traffic.net.node_intersection.get(t.node.0 as usize).and_then(|&i| self.traffic.net.intersections.get(i))?;
        let mut lanes: Vec<(crate::traffic::LaneId, f32)> = inter.incoming.iter().map(|&(l, _)| (l, self.traffic.net.lanes[l as usize].start().distance(from))).collect();
        lanes.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        let &(l, _) = lanes.get(k % lanes.len().clamp(1, 2))?;
        let len = self.traffic.net.lanes[l as usize].length;
        Some((l, (len - 3.0).max(0.5)))
    }

    /// Put a local in their car, bound for the meet.
    pub(super) fn drive_to_meet(&mut self, cid: CitizenId, t: &Takeover) -> Option<CarId> {
        let c = self.citizens.get(cid)?;
        let here = c.inside.and_then(|b| self.building_entrance(b)).unwrap_or(c.pos);
        let home = c.home;
        // [carlife] Their own car, where it is parked.
        let (vid, at) = self.car_ready(cid, here)?;
        let (goal, s1) = self.meet_lane(t, at, t.cars.len())?;
        let LaneKind::Road { segment, .. } = self.traffic.net.lanes[goal as usize].kind else { return None };
        let to_pt = self.traffic.net.lanes[goal as usize].point_at(s1).0;
        let car = self.drive_from_spot(vid, segment, to_pt, cid, crate::vehicle::Errand::Trip)?;
        self.leave_building(cid);
        let tc = &mut self.traffic.cars[car];
        tc.drive.gang = Some(GangRole { takeover: t.id, phase: GangPhase::Joining, race: false, pack: false });
        let (pos, model) = (tc.pos, tc.model);
        let c = &mut self.citizens[cid];
        c.pos = pos;
        c.prev_pos = pos;
        c.vehicle = Some(model);
        c.to_car = None;
        c.activity = Activity::Driving { car, to: home, purpose: Purpose::Wander };
        Some(car)
    }

    /// Outsiders roll in off the edge of the map in a pack.
    pub(super) fn spawn_outsiders(&mut self, t: &mut Takeover) {
        let Some(entry) = t.entry else {
            t.to_spawn = 0;
            return;
        };
        let tick = self.clock.tick;
        let mut rng = crate::rng_for(self.stream_seed(Stream::TakeoverOutsiders), tick);
        let Some(&sid) = self.roads.node(entry).segments.iter().find(|&&s| self.roads.segment(s).alive && self.roads.segment(s).is_street()) else {
            t.to_spawn = 0;
            return;
        };
        let at = self.roads.node(entry).pos;
        let lanes: Vec<crate::traffic::LaneId> = self.traffic.net.segment_lanes.get(&sid).cloned().unwrap_or_default();
        // Lanes leaving the road end, inbound.
        let inbound: Vec<crate::traffic::LaneId> = lanes.into_iter().filter(|&l| self.traffic.net.lanes[l as usize].start().distance(at) < self.traffic.net.lanes[l as usize].end().distance(at)).collect();
        for (k, &lane) in inbound.iter().enumerate().take(2) {
            if t.to_spawn == 0 {
                break;
            }
            if self.traffic.net.lanes[lane as usize].cars.first().is_some_and(|&c| self.traffic.cars[c].s < 10.0) {
                continue;
            }
            let Some((goal, s1)) = self.meet_lane(t, at, t.cars.len() + k) else { continue };
            let Some((route, s0, s1)) = self.traffic.net.route_lanes(lane, 0.5, goal, s1) else { continue };
            let car = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, 0);
            let bike = rng.random_bool(0.4);
            let body = if bike {
                if rng.random_bool(0.5) { BodyType::AkiraBike } else { BodyType::Motorcycle }
            } else {
                *[BodyType::Coupe, BodyType::Coupe, BodyType::Sedan, BodyType::Suv, BodyType::Hatchback].choose(&mut rng).unwrap()
            };
            let tc = &mut self.traffic.cars[car];
            tc.model = VehicleModel { class: VehicleClass::Expensive, body, wear: 0, tuned: !body.two_wheeler(), livery: Livery::None, paint: rng.random_range(0..crate::vehicle::PAINTS) };
            tc.drive.gang = Some(GangRole { takeover: t.id, phase: GangPhase::Joining, race: false, pack: false });
            t.cars.push(car);
            t.outsiders += 1;
            t.to_spawn -= 1;
        }
    }

    /// One tick of the takeover under way.
    pub(super) fn run_takeover(&mut self) {
        let Some(mut t) = self.driving.takeovers.active.take() else { return };
        let tick = self.clock.tick;
        let mut rng = crate::rng_for(self.stream_seed(Stream::TakeoverRun), tick);
        t.cars.retain(|&c| self.traffic.cars.contains_key(c));
        if t.to_spawn > 0 {
            self.spawn_outsiders(&mut t);
        }
        // Cars that made it to the meet join the show.
        let arriving: Vec<CarId> = t
            .cars
            .iter()
            .copied()
            .filter(|&c| {
                let car = &self.traffic.cars[c];
                car.drive.gang.is_some_and(|g| g.phase == GangPhase::Joining) && !car.crashed() && car.pos.distance(t.centre) < takeover::JOIN_REACH && (car.v < 0.5 || car.pos.distance(t.centre) < t.radius)
            })
            .collect();
        for c in arriving {
            let car = &self.traffic.cars[c];
            let flashy = !car.model.body.two_wheeler() && (car.model.tuned || matches!(car.model.body, BodyType::Coupe | BodyType::Sedan | BodyType::CyberTruck));
            let kind = if flashy && t.donuts < 3 {
                let (centre, dir) = takeover::donut_spot(t.centre, t.donuts);
                t.donuts += 1;
                StuntKind::Donut { centre, radius: 2.4, dir }
            } else {
                let (at, face) = takeover::ring_slot(t.centre, t.radius, t.ring_used, 10);
                t.ring_used += 1;
                StuntKind::Ring { at, face }
            };
            self.traffic.begin_stunt(c, kind);
        }
        let in_show = t.cars.iter().filter(|&&c| self.traffic.cars[c].in_stunt()).count();
        match t.stage {
            Stage::Gathering => {
                let expected = t.cars.len() + t.to_spawn as usize;
                let waited = tick.saturating_sub(t.started);
                if (in_show >= 3 && (in_show * 10 >= expected * 6 || waited >= takeover::GATHER_TICKS)) || (waited >= takeover::GATHER_TICKS * 2 && in_show >= 1) {
                    t.stage = Stage::Show;
                    t.show_from = Some(tick);
                    t.ends_at = Some(tick + rng.random_range(takeover::SHOW_TICKS.0..=takeover::SHOW_TICKS.1));
                    self.traffic.set_blocked(t.node, true);
                    // A neighbour calls the police.
                    let caller = self.witness(t.centre, 400.0, &t.residents);
                    let near = self.building_near(t.centre);
                    if let Some(near) = near {
                        if let Some(id) = self.report_crime(CrimeKind::StreetTakeover, near, t.residents.first().copied(), None) {
                            if let Some(i) = self.services.incident_mut(id) {
                                i.pos = t.centre;
                                i.on_road = true;
                                i.caller = caller;
                            }
                            t.incident = Some(id);
                        }
                    }
                    if let Some((who, c)) = caller.and_then(|c| self.citizens.get_mut(c).map(|x| (c, x))) {
                        c.remember(tick, format!("Called the cops on a street takeover at {}. Can't sleep through that.", t.place));
                        self.concerns.heard(who, c, crate::concerns::Concern::Noise, 0.5);
                    }
                } else if waited > takeover::GATHER_TICKS * 3 {
                    // Nobody showed: it fizzles out.
                    self.end_takeover(t, false);
                    return;
                }
            }
            Stage::Show => {
                // Keep the junction closed (a rebuilt lane network forgets).
                self.traffic.set_blocked(t.node, true);
                if tick % 3 == 0 {
                    self.gather_crowd(&mut t, &mut rng);
                }
                let ends = t.ends_at.unwrap_or(tick);
                for &cid in &t.crowd {
                    if let Some(c) = self.citizens.get_mut(cid) {
                        if matches!(c.activity, Activity::Idle) {
                            c.next_decision_tick = c.next_decision_tick.max(ends);
                            let d = t.centre - c.pos;
                            c.heading = d.y.atan2(d.x);
                            c.prev_heading = c.heading;
                        }
                    }
                }
                let police = t.incident.and_then(|i| self.services.incident(i)).is_some_and(|i| i.stage == IncidentStage::OnScene);
                if police {
                    self.police_break_up(&mut t, &mut rng);
                    self.end_takeover(t, true);
                    return;
                }
                if tick >= ends {
                    self.end_takeover(t, false);
                    return;
                }
            }
        }
        self.driving.takeovers.active = Some(t);
    }

    /// Spectators walk over from nearby: the young and the hedonistic, out
    /// or awake at home.
    pub(super) fn gather_crowd(&mut self, t: &mut Takeover, rng: &mut SimRng) {
        if t.crowd.len() >= takeover::CROWD_MAX {
            return;
        }
        let mut pool: Vec<(CitizenId, f32)> = self
            .citizens
            .iter()
            .filter(|(id, c)| !t.crowd.contains(id) && !t.residents.contains(id) && (15..=45).contains(&c.age) && !c.asleep)
            .filter(|(_, c)| matches!(c.activity, Activity::Idle | Activity::Visiting(_) | Activity::AtHome))
            .filter(|(id, _)| !self.services.in_custody(*id) && !self.is_patient(*id))
            .filter_map(|(id, c)| {
                let d = c.pos.distance(t.centre);
                let (caution, hedonism) = driver_traits(c.seed);
                let keen = 0.6 * hedonism + 0.4 * (1.0 - caution);
                (d < takeover::CROWD_REACH && keen > 0.45 && (!matches!(c.activity, Activity::AtHome) || keen > 0.6)).then_some((id, d))
            })
            .collect();
        pool.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        let tick = self.clock.tick;
        let ends = t.ends_at.unwrap_or(tick);
        for (cid, _) in pool.into_iter().take(3) {
            let k = t.crowd.len();
            let spot = takeover::crowd_spot(t.centre, t.radius, k, rng.random::<f32>() * 0.4);
            let from = match self.citizens[cid].inside.and_then(|b| self.building_entrance(b)) {
                Some(e) => e,
                None => self.citizens[cid].pos,
            };
            let Some((mut path, _)) = self.walk.route(from, spot) else { continue };
            path.push(spot);
            let exit = self.leave_building(cid);
            let c = &mut self.citizens[cid];
            if let Some(p) = exit {
                c.pos = p;
                c.prev_pos = p;
            }
            c.mode = crate::citizens::TravelMode::Walk;
            c.transit = None;
            c.activity = Activity::Traveling { to: None, purpose: Purpose::Wander, path, idx: 0 };
            c.next_decision_tick = ends;
            c.thought = "Engines screaming. Gotta see this.".to_string();
            t.crowd.push(cid);
        }
    }

    /// The police are on scene: locals still in the ring may be arrested
    /// and their rides impounded; everyone else scatters.
    pub(super) fn police_break_up(&mut self, t: &mut Takeover, rng: &mut SimRng) {
        let tick = self.clock.tick;
        let Some(inc) = t.incident else { return };
        for &cid in &t.residents.clone() {
            if t.arrests >= takeover::MAX_ARRESTS {
                break;
            }
            let car = match self.citizens.get(cid).map(|c| &c.activity) {
                Some(Activity::Driving { car, .. }) => *car,
                _ => continue,
            };
            if !self.traffic.cars.get(car).is_some_and(|c| c.in_stunt()) || rng.random::<f32>() >= takeover::ARREST_CHANCE {
                continue;
            }
            // The car is impounded: towed like a wreck (and gone for good).
            self.traffic.cars[car].citizen = CitizenId::default();
            if let Some(vid) = self.traffic.cars[car].owned.take() {
                self.drop_car(vid, crate::vehicle::CarEvent::Scrapped);
            }
            t.cars.retain(|&c| c != car);
            self.tow_or_queue(car);
            let place = t.place.clone();
            let c = &mut self.citizens[cid];
            c.activity = Activity::Idle;
            c.inside = None;
            c.remember(tick, format!("Busted at the takeover at {place}. They impounded my ride."));
            c.thought = "Busted.".to_string();
            c.mood = (c.mood - 0.2).max(0.0);
            self.services.stats.arrests_today += 1;
            self.take_into_custody(inc, cid);
            t.arrests += 1;
        }
        self.resolve(inc, if t.arrests > 0 { Outcome::Arrest } else { Outcome::Escaped });
    }

    /// Open the junction, send everyone home in convoy, remember it.
    pub(super) fn end_takeover(&mut self, t: Takeover, police: bool) {
        let tick = self.clock.tick;
        self.traffic.set_blocked(t.node, false);
        let exit_to = t.exit.and_then(|n| {
            let seg = *self.roads.node(n).segments.iter().find(|&&s| self.roads.segment(s).alive && self.roads.segment(s).is_street())?;
            Some((seg, self.roads.node(n).pos))
        });
        // Lanes leaving the junction, to pull out onto.
        let outgoing: Vec<crate::traffic::LaneId> = self
            .roads
            .node(t.node)
            .segments
            .iter()
            .filter_map(|s| self.traffic.net.segment_lanes.get(s))
            .flatten()
            .copied()
            .filter(|&l| self.traffic.net.lanes[l as usize].start().distance(t.centre) < self.traffic.net.lanes[l as usize].end().distance(t.centre))
            .collect();
        for &car in &t.cars {
            let Some(c) = self.traffic.cars.get(car) else { continue };
            let driver = c.citizen;
            let home = self.citizens.get(driver).and_then(|p| p.home);
            let dest = match home.and_then(|h| self.car_stop(h)) {
                Some(d) => Some(d),
                None => exit_to,
            };
            // REVIEW2 L9: a stunt car with nowhere to go (no home, no exit,
            // no lane there) used to be skipped and did donuts for good; it
            // is towed off below like one with no route.
            let goal = dest.and_then(|(dseg, dp)| self.traffic.net.nearest_lane_on(dseg, dp));
            if c.in_stunt() {
                let pos = c.pos;
                let mut starts = outgoing.clone();
                starts.sort_by(|a, b| self.traffic.net.lanes[*a as usize].start().distance(pos).total_cmp(&self.traffic.net.lanes[*b as usize].start().distance(pos)).then(a.cmp(b)));
                let found = goal.and_then(|(goal, gs, _)| starts.iter().find_map(|&l| self.traffic.net.route_lanes(l, 0.5, goal, gs)));
                match found {
                    Some((route, s0, s1)) => self.traffic.end_stunt(car, route, s0, s1),
                    None => {
                        // Nowhere to drive: towed off like a wreck.
                        self.traffic.cars[car].citizen = CitizenId::default();
                        if let Some(vid) = self.traffic.cars[car].owned.take() {
                            self.drop_car(vid, crate::vehicle::CarEvent::Scrapped);
                        }
                        self.tow_or_queue(car);
                        if let Some(p) = self.citizens.get_mut(driver) {
                            p.activity = Activity::Idle;
                            p.next_decision_tick = tick + 1;
                        }
                        continue;
                    }
                }
            } else {
                // Never got there: turn round for home.
                let Some((goal, gs, _)) = goal else { continue };
                let (here, s) = (c.route[c.idx], c.s);
                if let Some((route, _, s1)) = self.traffic.net.route_lanes(here, s, goal, gs) {
                    self.traffic.reroute(car, route, s, s1);
                }
                if let Some(g) = self.traffic.cars[car].drive.gang.as_mut() {
                    g.phase = GangPhase::Leaving;
                }
            }
            if let Some(p) = self.citizens.get_mut(driver) {
                if let Activity::Driving { purpose, .. } = &mut p.activity {
                    *purpose = Purpose::Home;
                }
                p.remember(tick, if police { format!("Ran the takeover at {} till the cops showed. Scattered.", t.place) } else { format!("Held {} for an hour. Donuts, smoke, the whole crowd screaming.", t.place) });
                p.mood = (p.mood + 0.1).min(1.0);
            }
        }
        // The crowd drifts off; the neighbours remember the noise.
        let crowd_n = t.crowd.iter().filter(|&&c| self.citizens.get(c).is_some_and(|c| c.pos.distance(t.centre) < t.radius + 20.0)).count();
        let mut rng = crate::rng_for(self.stream_seed(Stream::TakeoverEnd), tick);
        for &cid in &t.crowd {
            if let Some(c) = self.citizens.get_mut(cid) {
                if c.pos.distance(t.centre) < t.radius + 25.0 {
                    c.remember(tick, format!("Watched a street takeover at {}. Smoke everywhere, cars spinning in the middle of the junction.", t.place));
                    c.mood = (c.mood + 0.05).min(1.0);
                }
                if matches!(c.activity, Activity::Idle | Activity::Traveling { .. }) {
                    c.next_decision_tick = tick + rng.random_range(1..15);
                }
            }
        }
        let homes: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| !b.residents.is_empty() && self.lots.get(b.lot).is_some_and(|l| l.center.distance(t.centre) < 160.0)).map(|(id, _)| id).collect();
        let mut told = 0;
        for b in homes {
            for &r in &self.buildings[b].residents.clone() {
                if told >= 40 {
                    break;
                }
                if let Some(c) = self.citizens.get_mut(r) {
                    if c.inside == Some(b) {
                        c.remember(tick, format!("Engines and burning rubber outside at {} half the night.", t.place));
                        self.concerns.heard(r, c, crate::concerns::Concern::Noise, 0.3);
                        c.mood = (c.mood - 0.03).max(0.0);
                        told += 1;
                    }
                }
            }
        }
        if let Some(i) = t.incident {
            if !police && self.services.incident(i).is_some_and(|i| i.active() && i.stage != IncidentStage::OnScene) {
                // Gone before anyone came: the call is closed.
                self.resolve(i, Outcome::Escaped);
            }
        }
        self.driving.takeovers.history.push(takeover::Record { id: t.id, place: t.place.clone(), centre: t.centre, started: t.started, ended: tick, participants: t.participants(), residents: t.residents.len(), outsiders: t.outsiders, crowd: crowd_n, arrests: t.arrests, police });
        if self.driving.takeovers.history.len() > 30 {
            self.driving.takeovers.history.remove(0);
        }
        self.events.push(SimEvent::TakeoverOver(t.id));
    }

    /// The takeover under way, if any.
    pub fn takeover(&self) -> Option<&Takeover> {
        self.driving.takeovers.active.as_ref()
    }

    /// Spectators standing round the ring right now.
    pub fn takeover_crowd_now(&self) -> usize {
        self.takeover().map_or(0, |t| t.crowd.iter().filter(|&&c| self.citizens.get(c).is_some_and(|c| matches!(c.activity, Activity::Idle) && c.pos.distance(t.centre) < t.radius + 20.0)).count())
    }
}
