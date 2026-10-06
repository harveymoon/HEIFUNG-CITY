//! [driving] Who is driving and what happens on the road: drinks at venues,
//! driver profiles from age, personality and drink, crashes (injuries, the
//! 911 call, police and ambulance, the tow truck) and street takeovers.
//!
//! The physics lives in `traffic.rs` (`DriverProfile`, crashes, stunts);
//! takeover data in `takeover.rs`. This file is the glue that reads and
//! writes the rest of the world. It draws from its own random streams
//! (`rng_for`), never the world's, so it does not shift anything else.

use super::*;
use crate::citizens::{driver_traits, rebelliousness, DRUNK};
use crate::services::{CrimeKind, Outcome, Stage as IncidentStage};
use crate::takeover::{self, Stage, Takeover, Takeovers};

use crate::traffic::{CrashCause, CrashReport, DriverProfile, GangPhase, GangRole, LaneKind, StuntKind, BAD_CRASH};
use crate::vehicle::{BodyType, Livery, VehicleClass, VehicleModel};

/// Drunkenness gained per tick at a venue (times `hedonism - 0.3`: the
/// sensible have a soda), and lost per tick everywhere else: a hedonist is
/// well gone after a couple of hours out and sober again by morning.
pub const DRINK_RATE: f32 = 0.004;
pub const SOBER_RATE: f32 = 0.0022;
/// Crash records kept for the API.
const KEEP_CRASHES: usize = 60;
/// Ticks before a wreck nobody reported is towed anyway.
const TOW_UNREPORTED: u64 = 45;

/// A written-off car waiting for the scene to clear.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Wreck {
    pub car: CarId,
    pub crash: u32,
    pub incident: Option<u32>,
    pub since: u64,
}

/// A tow truck hauling a wreck to the yard (`model` is what it carries).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tow {
    pub truck: CarId,
    pub model: VehicleModel,
    pub yard: Option<BuildingId>,
}

/// A crash, for the record.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CrashRecord {
    pub id: u32,
    pub tick: u64,
    pub pos: Vec2,
    pub cause: CrashCause,
    pub severity: f32,
    pub drivers: Vec<CitizenId>,
    pub injured: Vec<CitizenId>,
    pub drunk: bool,
    pub incident: Option<u32>,
    pub place: String,
}

/// The driving side of the city: counts, wrecks, tows, takeovers.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Driving {
    pub crashes_today: u32,
    pub injuries_today: u32,
    pub red_runs_today: u32,
    pub drunk_trips_today: u32,
    pub crashes_total: u32,
    pub towed_total: u32,
    pub wrecks: Vec<Wreck>,
    pub tows: Vec<Tow>,
    pub recent: Vec<CrashRecord>,
    pub takeovers: Takeovers,
}

/// A stable `0..1` from a car id, for its weave phase and a stranger's traits.
fn car_hash(id: CarId, salt: u64) -> f32 {
    let x = slotmap::Key::data(&id).as_ffi().wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ salt.wrapping_mul(0xD6E8_FEB8_6659_FD93);
    let x = (x ^ (x >> 29)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    ((x ^ (x >> 32)) % 10_000) as f32 / 10_000.0
}

impl World {
    /// Everything driving does in a tick: after the cars have moved.
    pub(super) fn step_driving(&mut self) {
        let tick = self.clock.tick;
        if self.clock.minute_of_day() == 0 {
            let d = &mut self.driving;
            d.crashes_today = 0;
            d.injuries_today = 0;
            d.red_runs_today = 0;
            d.drunk_trips_today = 0;
        }
        self.driving.red_runs_today += std::mem::take(&mut self.traffic.red_runs);
        if tick % 5 == 0 {
            self.step_drink(5);
        }
        self.assign_drivers();
        let crashes = std::mem::take(&mut self.traffic.crashes);
        for c in crashes {
            self.handle_crash(c);
        }
        self.step_wrecks();
        self.step_takeovers();
    }

    /// Drinks at venues; sobering up everywhere else.
    fn step_drink(&mut self, ticks: u32) {
        let buildings = &self.buildings;
        for c in self.citizens.values_mut() {
            let at_venue = match c.activity {
                Activity::Visiting(b) => buildings.get(b).is_some_and(|b| b.land_use == LandUse::Entertainment || b.business.as_ref().is_some_and(|z| z.kind == BusinessKind::Venue)),
                _ => false,
            };
            if at_venue && c.age >= 18 {
                let (_, hedonism) = driver_traits(c.seed);
                c.drunk = (c.drunk + DRINK_RATE * (hedonism - 0.3).max(0.0) * ticks as f32).min(1.0);
            } else if c.drunk > 0.0 {
                c.drunk = (c.drunk - SOBER_RATE * ticks as f32).max(0.0);
            }
        }
    }

    /// Give every car that set off since the last tick its driver's profile.
    fn assign_drivers(&mut self) {
        let hour = self.clock.hour();
        let tick = self.clock.tick;
        let new: Vec<CarId> = self.traffic.cars.iter().filter(|(_, c)| !c.drive.profile.assigned).map(|(id, _)| id).collect();
        for id in new {
            let phase = car_hash(id, 1);
            let car = &self.traffic.cars[id];
            let gang = car.drive.gang.is_some();
            let profile = if car.siren || car.bus.is_some() || car.cargo.is_some() || self.services.unit_of_car(id).is_some() {
                DriverProfile::professional(phase)
            } else if let Some(c) = self.citizens.get(car.citizen) {
                let (caution, hedonism) = driver_traits(c.seed);
                let knack = ((c.seed.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) % 1000) as f32 / 1000.0;
                let p = DriverProfile::for_driver(c.age.max(17), caution, hedonism, c.drunk, knack, phase);
                if c.drunk >= DRUNK && !gang {
                    self.driving.drunk_trips_today += 1;
                    let cid = car.citizen;
                    let c = &mut self.citizens[cid];
                    c.remember(tick, "Drove after a few too many. Felt fine. Mostly.");
                }
                p
            } else {
                // A stranger driving through: anyone, now and then drunk at night.
                let age = 18 + (car_hash(id, 2) * 62.0) as u8;
                let night = !(5..22).contains(&hour);
                let drunk = if night && car_hash(id, 5) < 0.04 { 0.4 + 0.5 * car_hash(id, 6) } else { 0.0 };
                DriverProfile::for_driver(age, car_hash(id, 3), car_hash(id, 4), drunk, car_hash(id, 7), phase)
            };
            let car = &mut self.traffic.cars[id];
            car.drive.profile = if gang { profile.pack() } else { profile };
            car.drive.seen_lead_v = car.v;
        }
    }

    /// Drunk drivers on the road right now.
    pub fn drunk_drivers_now(&self) -> usize {
        self.traffic.cars.values().filter(|c| c.drive.profile.drunk >= DRUNK && !c.crashed()).count()
    }

    /// The building nearest a point, to name a place on the street.
    fn building_near(&self, p: Vec2) -> Option<BuildingId> {
        self.buildings.iter().filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (id, l.center.distance_squared(p)))).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(id, _)| id)
    }

    /// Somebody near enough to see it and call it in (not those in it).
    fn witness(&self, p: Vec2, reach: f32, except: &[CitizenId]) -> Option<CitizenId> {
        self.citizens
            .iter()
            .filter(|(id, c)| !except.contains(id) && !c.asleep && c.age >= 12 && !matches!(c.activity, Activity::OnTrain { .. } | Activity::OnMetro { .. } | Activity::Arriving))
            .map(|(id, c)| (id, c.pos.distance(p)))
            .filter(|(_, d)| *d < reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// Turn a crash from the traffic model into what follows: a scrape and
    /// a few minutes lost, or a write-off, the hurt waiting in the road, a
    /// witness calling 911 and police and ambulances on their way.
    fn handle_crash(&mut self, r: CrashReport) {
        let tick = self.clock.tick;
        let mut rng = crate::rng_for(self.config.seed ^ 0xC2A5_4ED1, r.id as u64);
        let cars: Vec<CarId> = r.cars.iter().copied().filter(|&c| self.traffic.cars.contains_key(c)).collect();
        if cars.is_empty() {
            return;
        }
        // Buses, lorries, emergency vehicles and takeover stunts only scrape:
        // nobody is written off in them (their riders and loads are someone
        // else's story).
        for &c in &cars {
            let car = &mut self.traffic.cars[c];
            let protected = car.bus.is_some() || car.cargo.is_some() || car.siren || car.in_stunt();
            if let Some(cs) = car.drive.crash.as_mut().filter(|cs| cs.wrecked && protected) {
                cs.wrecked = false;
                cs.hold = 90.0;
            }
        }
        let near = self.building_near(r.pos);
        let place = near.and_then(|b| self.buildings.get(b)).map(|b| b.name.clone()).unwrap_or_else(|| "the street".to_string());
        let drivers: Vec<CitizenId> = cars.iter().map(|&c| self.traffic.cars[c].citizen).filter(|c| self.citizens.contains_key(*c)).collect();
        let drunk = cars.iter().any(|&c| self.traffic.cars[c].drive.profile.drunk >= DRUNK);
        let bad = r.severity >= BAD_CRASH && cars.iter().any(|&c| self.traffic.cars[c].drive.crash.is_some_and(|cs| cs.wrecked));
        self.driving.crashes_today += 1;
        self.driving.crashes_total += 1;
        let mut injured = Vec::new();
        let mut incident = None;
        if bad {
            for &c in &cars {
                let car = &self.traffic.cars[c];
                let driver = car.citizen;
                let bike = car.model.body.two_wheeler();
                let chance = ((r.severity - 0.3) * 1.3 + if bike { 0.25 } else { 0.0 }).clamp(0.0, 0.95);
                let hurt = self.citizens.contains_key(driver) && rng.random::<f32>() < chance;
                if hurt {
                    injured.push(driver);
                }
                let wrecked = car.drive.crash.is_some_and(|cs| cs.wrecked);
                if !wrecked {
                    continue;
                }
                let (pos, heading, model, owned) = (car.pos, car.heading, car.model, car.owned);
                self.traffic.cars[c].citizen = CitizenId::default();
                // [carlife] Written off: scrapped (the tow truck hauls it to the yard).
                self.traffic.cars[c].owned = None;
                if let Some(vid) = owned {
                    self.drop_car(vid, crate::vehicle::CarEvent::Scrapped);
                }
                self.driving.wrecks.push(Wreck { car: c, crash: r.id, incident: None, since: tick });
                if let Some(p) = self.citizens.get_mut(driver) {
                    // Out of the car, standing (or lying) by it.
                    p.activity = Activity::Idle;
                    p.inside = None;
                    p.pos = pos + Vec2::new(-heading.sin(), heading.cos()) * 2.2;
                    p.prev_pos = p.pos;
                    p.mood = (p.mood - 0.15).max(0.0);
                    p.next_decision_tick = tick + if hurt { crate::services::MEDICAL_GIVE_UP + 120 } else { 30 };
                    p.thought = if hurt { "Everything hurts. Where's the ambulance?".to_string() } else { "The car's totalled.".to_string() };
                    let what = match model.body {
                        b if b.two_wheeler() => "bike",
                        _ => "car",
                    };
                    p.remember(tick, format!("Wrote off my {what} in a crash by {place} ({}).", r.cause.label()));
                    self.concerns.heard(driver, p, crate::concerns::Concern::Commute, 0.6);
                }
            }
            let at_fault = cars.first().map(|&c| &self.traffic.cars[c]).filter(|c| c.drive.profile.drunk >= DRUNK || matches!(r.cause, CrashCause::RedLight | CrashCause::OffRoad)).and(drivers.first().copied());
            let caller = self.witness(r.pos, 300.0, &drivers);
            if let Some(near) = near {
                if let Some((id, _)) = self.report_crash(r.pos, near, &drivers, &injured, at_fault, caller) {
                    incident = Some(id);
                    for w in self.driving.wrecks.iter_mut().filter(|w| w.crash == r.id) {
                        w.incident = Some(id);
                    }
                }
            }
            if let Some(cid) = caller {
                let c = &mut self.citizens[cid];
                c.remember(tick, format!("Saw a bad crash by {place}. Called it in."));
                c.thought = "Called 911 about a crash.".to_string();
            }
            self.driving.injuries_today += injured.len() as u32;
        } else {
            for &d in &drivers {
                let c = &mut self.citizens[d];
                c.remember(tick, format!("Fender bender by {place}. Lost a few minutes swapping details."));
                self.concerns.heard(d, c, crate::concerns::Concern::Commute, 0.2);
                c.mood = (c.mood - 0.03).max(0.0);
            }
        }
        self.events.push(SimEvent::Crash(r.id));
        self.driving.recent.push(CrashRecord { id: r.id, tick, pos: r.pos, cause: r.cause, severity: r.severity, drivers, injured, drunk, incident, place });
        if self.driving.recent.len() > KEEP_CRASHES {
            self.driving.recent.remove(0);
        }
    }

    /// Wrecks are towed once the police have cleared the scene (or nobody
    /// came): a tow truck takes the place of the wreck and hauls it to the
    /// nearest industrial yard. Nothing is simply deleted.
    fn step_wrecks(&mut self) {
        let tick = self.clock.tick;
        let wrecks = std::mem::take(&mut self.driving.wrecks);
        let mut keep = Vec::new();
        for w in wrecks {
            if !self.traffic.cars.contains_key(w.car) {
                continue;
            }
            let cleared = match w.incident {
                Some(i) => self.services.incident(i).is_none_or(|i| !i.active()),
                None => tick >= w.since + TOW_UNREPORTED,
            };
            if cleared {
                self.tow(w.car);
            } else {
                keep.push(w);
            }
        }
        self.driving.wrecks = keep;
        let before = self.driving.tows.len();
        let traffic = &self.traffic;
        self.driving.tows.retain(|t| traffic.cars.contains_key(t.truck));
        self.driving.towed_total += (before - self.driving.tows.len()) as u32;
    }

    /// Replace a car with a tow truck carrying it to a yard.
    fn tow(&mut self, car: CarId) {
        let Some(c) = self.traffic.cars.get(car) else { return };
        let (lane, s, model, pos) = (c.route[c.idx], c.s, c.model, c.pos);
        // [parking] The city's tow companies haul wrecks too; with none in
        // town, one from out of town takes it off the map.
        let yard = self.tow_company_near(pos);
        self.enforcement.stats.wrecks_towed_total += 1;
        let goal = yard.and_then(|y| self.building_lot(y)).and_then(|l| l.car_segment.map(|s| (s, l.car_point))).or_else(|| {
            // No yard: off the map, to the nearest road end.
            let leaves = self.city_leaves();
            leaves.into_iter().filter_map(|n| self.roads.node(n).segments.iter().find(|&&s| self.roads.segment(s).alive && self.roads.segment(s).is_street()).map(|&s| (s, self.roads.node(n).pos))).min_by(|a, b| a.1.distance(pos).total_cmp(&b.1.distance(pos)))
        });
        let route = goal.and_then(|(seg, p)| {
            let (gl, gs, _) = self.traffic.net.nearest_lane_on(seg, p)?;
            self.traffic.net.route_lanes(lane, s, gl, gs)
        });
        self.traffic.remove_car(car);
        self.driving.towed_total += route.is_none() as u32;
        let Some((route, s0, s1)) = route else { return };
        let truck = self.traffic.spawn_car(CitizenId::default(), None, route, s0, s1, 0);
        let t = &mut self.traffic.cars[truck];
        t.model = VehicleModel { class: VehicleClass::Mid, body: BodyType::Pickup, wear: 1, tuned: false, livery: Livery::Utility, paint: 3 };
        t.drive.profile = DriverProfile::professional(car_hash(truck, 1));
        self.driving.tows.push(Tow { truck, model, yard });
    }

    /// Tow trucks on the road and what they carry.
    pub fn tows(&self) -> &[Tow] {
        &self.driving.tows
    }

    // ── street takeovers ───────────────────────────────────────────────────

    /// Locals who would join a takeover: young, rebellious, with wheels,
    /// free right now. Best candidates (flashiest rides) first.
    fn takeover_crew(&self) -> Vec<CitizenId> {
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
    fn takeover_junctions(&self, near: Option<Vec2>) -> Vec<(NodeId, Vec2, f32)> {
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
    fn step_takeovers(&mut self) {
        let tick = self.clock.tick;
        let day = self.clock.day();
        if self.clock.minute_of_day() == 22 * 60 && self.driving.takeovers.rolled_day != day && self.driving.takeovers.active.is_none() {
            self.driving.takeovers.rolled_day = day;
            let mut rng = crate::rng_for(self.config.seed ^ 0x7A4E_0FE5, day);
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
        let mut rng = crate::rng_for(self.config.seed ^ 0x7A4E_0FE6, tick);
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
    fn reaches(&self, end: NodeId, node: NodeId, inbound: bool) -> bool {
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
    fn meet_lane(&self, t: &Takeover, from: Vec2, k: usize) -> Option<(crate::traffic::LaneId, f32)> {
        let inter = self.traffic.net.node_intersection.get(t.node.0 as usize).and_then(|&i| self.traffic.net.intersections.get(i))?;
        let mut lanes: Vec<(crate::traffic::LaneId, f32)> = inter.incoming.iter().map(|&(l, _)| (l, self.traffic.net.lanes[l as usize].start().distance(from))).collect();
        lanes.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        let &(l, _) = lanes.get(k % lanes.len().clamp(1, 2))?;
        let len = self.traffic.net.lanes[l as usize].length;
        Some((l, (len - 3.0).max(0.5)))
    }

    /// Put a local in their car, bound for the meet.
    fn drive_to_meet(&mut self, cid: CitizenId, t: &Takeover) -> Option<CarId> {
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
        tc.drive.gang = Some(GangRole { takeover: t.id, phase: GangPhase::Joining });
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
    fn spawn_outsiders(&mut self, t: &mut Takeover) {
        let Some(entry) = t.entry else {
            t.to_spawn = 0;
            return;
        };
        let tick = self.clock.tick;
        let mut rng = crate::rng_for(self.config.seed ^ 0x0575_1DE5, tick);
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
            tc.drive.gang = Some(GangRole { takeover: t.id, phase: GangPhase::Joining });
            t.cars.push(car);
            t.outsiders += 1;
            t.to_spawn -= 1;
        }
    }

    /// One tick of the takeover under way.
    fn run_takeover(&mut self) {
        let Some(mut t) = self.driving.takeovers.active.take() else { return };
        let tick = self.clock.tick;
        let mut rng = crate::rng_for(self.config.seed ^ 0x7A4E_0FE7, tick);
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
    fn gather_crowd(&mut self, t: &mut Takeover, rng: &mut SimRng) {
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
    fn police_break_up(&mut self, t: &mut Takeover, rng: &mut SimRng) {
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
            self.tow(car);
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
    fn end_takeover(&mut self, t: Takeover, police: bool) {
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
            let dest = match home.and_then(|h| self.building_lot(h)).and_then(|l| l.car_segment.map(|s| (s, l.car_point))) {
                Some(d) => Some(d),
                None => exit_to,
            };
            let Some((dseg, dp)) = dest else { continue };
            let Some((goal, gs, _)) = self.traffic.net.nearest_lane_on(dseg, dp) else { continue };
            if c.in_stunt() {
                let pos = c.pos;
                let mut starts = outgoing.clone();
                starts.sort_by(|a, b| self.traffic.net.lanes[*a as usize].start().distance(pos).total_cmp(&self.traffic.net.lanes[*b as usize].start().distance(pos)).then(a.cmp(b)));
                let found = starts.iter().find_map(|&l| self.traffic.net.route_lanes(l, 0.5, goal, gs));
                match found {
                    Some((route, s0, s1)) => self.traffic.end_stunt(car, route, s0, s1),
                    None => {
                        // Nowhere to drive: towed off like a wreck.
                        self.traffic.cars[car].citizen = CitizenId::default();
                        if let Some(vid) = self.traffic.cars[car].owned.take() {
                            self.drop_car(vid, crate::vehicle::CarEvent::Scrapped);
                        }
                        self.tow(car);
                        if let Some(p) = self.citizens.get_mut(driver) {
                            p.activity = Activity::Idle;
                            p.next_decision_tick = tick + 1;
                        }
                        continue;
                    }
                }
            } else {
                // Never got there: turn round for home.
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
        let mut rng = crate::rng_for(self.config.seed ^ 0x7A4E_0FE8, tick);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buildings::Tier;
    use crate::mind::IdleMind;
    use crate::services::{IncidentKind, ServiceKind, UnitKind, UnitState};

    fn square(x0: f32, y0: f32, w: f32, h: f32) -> Vec<Vec2> {
        vec![Vec2::new(x0, y0), Vec2::new(x0 + w, y0), Vec2::new(x0 + w, y0 + h), Vec2::new(x0, y0 + h)]
    }

    /// Two avenues joined by cross streets, civic land between them and
    /// homes to the south; a side street makes (0, 0) a four-way junction.
    fn town(seed: u64) -> World {
        let mut w = World::new(WorldConfig { seed, emergencies: false, ..WorldConfig::test_town() });
        w.economy.treasury = 1_000_000.0;
        w.add_road(&[Vec2::new(-700.0, 0.0), Vec2::new(700.0, 0.0)], 1, 1);
        w.add_road(&[Vec2::new(-700.0, 120.0), Vec2::new(700.0, 120.0)], 1, 1);
        for x in [-600.0, -300.0, 0.0, 300.0, 600.0] {
            w.add_road(&[Vec2::new(x, 0.0), Vec2::new(x, 120.0)], 1, 1);
        }
        w.add_road(&[Vec2::new(0.0, -200.0), Vec2::new(0.0, 0.0)], 1, 1);
        w.add_zone(square(-690.0, 8.0, 1380.0, 50.0), LandUse::Civic);
        w.add_zone(square(-690.0, -60.0, 680.0, 52.0), LandUse::Residential);
        w.add_zone(square(10.0, -60.0, 680.0, 52.0), LandUse::Residential);
        let lots: Vec<LotId> = w.lots.iter().filter(|(_, l)| l.land_use == LandUse::Residential && l.usable()).map(|(id, _)| id).take(10).collect();
        for l in lots {
            w.build_on(l);
            if let Some(b) = w.lots[l].building {
                w.finish_construction(b);
            }
        }
        for _ in 0..40 {
            if w.citizens.len() >= 16 {
                break;
            }
            w.immigrate();
        }
        w
    }

    fn civic_lot(w: &World, x: f32) -> LotId {
        w.lots
            .iter()
            .filter(|(_, l)| l.land_use == LandUse::Civic && l.usable() && l.building.is_none() && l.car_access() && l.area() >= 420.0 && l.car_point.y.abs() < 12.0)
            .min_by(|a, b| (a.1.center.x - x).abs().total_cmp(&(b.1.center.x - x).abs()))
            .map(|(id, _)| id)
            .expect("a civic lot")
    }

    /// A crewed station of `kind` near `x`, staffed by `who`.
    fn station(w: &mut World, kind: ServiceKind, x: f32, who: &[CitizenId]) -> BuildingId {
        let b = w.build_service(civic_lot(w, x), kind, Tier::Mid).unwrap();
        for &c in who {
            if let Some(old) = w.citizens[c].work {
                w.buildings[old].workers.retain(|x| *x != c);
            }
            w.citizens[c].work = Some(b);
            w.buildings[b].workers.push(c);
        }
        b
    }

    fn tick(w: &mut World, n: usize, mut stop: impl FnMut(&World) -> bool) -> bool {
        let mut minds = IdleMind;
        for _ in 0..n {
            if stop(w) {
                return true;
            }
            w.tick(&mut minds);
        }
        stop(w)
    }

    #[test]
    fn a_bad_crash_is_called_in_and_the_hurt_go_to_hospital() {
        let mut w = town(7);
        let folk: Vec<CitizenId> = w.citizens.keys().collect();
        assert!(folk.len() >= 12);
        station(&mut w, ServiceKind::Police, -450.0, &folk[0..4]);
        let hospital = station(&mut w, ServiceKind::Health, 450.0, &folk[4..8]);
        let rider = folk[10];
        // A motorcyclist heading east on the avenue, on their own bike.
        let near = w.citizens[rider].home.and_then(|h| w.building_entrance(h)).unwrap();
        let vid = w.give_car_parked(rider, near);
        w.vehicles.cars[vid].model = VehicleModel::new(VehicleClass::Mid, BodyType::Motorcycle);
        let to = w.roads.nearest_street(Vec2::new(250.0, 0.0)).unwrap().0;
        let car = w.drive_from_spot(vid, to, Vec2::new(250.0, 1.75), rider, crate::vehicle::Errand::Trip).expect("a route");
        w.leave_building(rider);
        w.citizens[rider].activity = Activity::Driving { car, to: None, purpose: Purpose::Wander };
        for _ in 0..30 {
            tick(&mut w, 1, |_| false);
            if w.traffic.cars[car].v > 8.0 {
                break;
            }
        }
        tick(&mut w, 3, |_| false);
        assert!(w.traffic.cars[car].drive.profile.assigned, "the rider got a profile");
        w.traffic.crash(vec![car], CrashCause::OffRoad, 30.0);
        tick(&mut w, 1, |_| false);
        let rec = w.driving.recent.last().expect("crash recorded").clone();
        assert_eq!(rec.injured, vec![rider], "a bad spill at speed hurts the rider");
        assert_eq!(w.driving.crashes_today, 1);
        assert_eq!(w.driving.injuries_today, 1);
        let crash = w.services.incident(rec.incident.unwrap()).unwrap();
        assert_eq!(crash.kind, IncidentKind::Crash);
        assert!(crash.on_road && crash.pos.distance(rec.pos) < 1.0);
        assert!(!w.citizens[rider].owns_car && w.citizens[rider].car.is_none() && !w.vehicles.cars.contains_key(vid), "the bike is written off (scrapped)");
        let medical = w.services.incidents.iter().find(|i| i.kind == IncidentKind::Medical && i.citizen == Some(rider)).expect("a medical call").id;
        // An ambulance goes, the rider ends up in a hospital bed.
        let sent = tick(&mut w, 200, |w| w.services.units.values().any(|u| u.kind == UnitKind::Ambulance && u.incident == Some(medical) && u.state != UnitState::Idle));
        assert!(sent, "an ambulance was dispatched");
        let admitted = tick(&mut w, 400, |w| matches!(w.citizens[rider].activity, Activity::Visiting(b) if b == hospital));
        assert!(admitted, "taken to hospital: {:?}", w.services.incident(medical).map(|i| i.stage));
        // The police clear the scene and the wreck is towed, not deleted.
        let towed = tick(&mut w, 400, |w| !w.traffic.cars.contains_key(car) && (w.driving.towed_total > 0 || !w.driving.tows.is_empty()));
        assert!(towed, "wreck towed");
        assert!(!w.services.incident(rec.incident.unwrap()).unwrap().active());
    }

    /// A seed whose personality makes a takeover regular.
    fn rebel_seed(from: u64) -> u64 {
        (from..from + 5000)
            .find(|&s| {
                let (c, h) = driver_traits(s);
                rebelliousness(c, h) > 0.8
            })
            .unwrap()
    }

    fn node_at(w: &World, at: Vec2) -> usize {
        w.roads.nodes.iter().position(|n| n.pos.distance(at) < 0.5).unwrap()
    }

    /// A takeover at (0, 0) with police in town. Returns its record, the
    /// cars in the show, the donut cars and whether the junction was shut.
    fn takeover_night(seed: u64) -> (takeover::Record, usize, usize, bool) {
        let mut w = town(seed);
        let folk: Vec<CitizenId> = w.citizens.keys().collect();
        station(&mut w, ServiceKind::Police, -450.0, &folk[0..4]);
        let mut s = 1;
        for &c in &folk[8..12] {
            s = rebel_seed(s + 1);
            let p = &mut w.citizens[c];
            p.seed = s;
            p.age = 23;
            p.next_decision_tick = u64::MAX / 2;
            if let Some(h) = p.home {
                p.inside = Some(h);
                p.activity = Activity::AtHome;
            }
            let near = w.citizens[c].home.and_then(|h| w.building_entrance(h)).unwrap();
            let vid = w.give_car_parked(c, near);
            w.vehicles.cars[vid].model = VehicleModel { tuned: true, ..VehicleModel::new(VehicleClass::Expensive, BodyType::Coupe) };
        }
        let id = w.start_takeover(Some(Vec2::ZERO)).expect("starts");
        assert!(w.takeover().is_some_and(|t| t.id == id && t.residents.len() == 4));
        let show = tick(&mut w, 300, |w| w.takeover().is_some_and(|t| t.stage == Stage::Show));
        assert!(show, "the crews gathered and the show began");
        let t = w.takeover().unwrap().clone();
        let inter = &w.traffic.net.intersections[w.traffic.net.node_intersection[t.node.0 as usize]];
        let blocked = inter.connectors.iter().all(|&(l, _)| w.traffic.net.lanes[l as usize].blocked);
        let stunts = t.cars.iter().filter(|&&c| w.traffic.cars[c].in_stunt()).count();
        let donuts = t.cars.iter().filter(|&&c| w.traffic.cars[c].drive.stunt.is_some_and(|s| s.is_donut())).count();
        assert!(stunts >= 3 && donuts >= 1, "{stunts} in the show, {donuts} spinning");
        let over = tick(&mut w, 600, |w| w.takeover().is_none());
        assert!(over, "it ended");
        let rec = w.driving.takeovers.history.last().unwrap().clone();
        let inter = &w.traffic.net.intersections[w.traffic.net.node_intersection[node_at(&w, rec.centre)]];
        assert!(inter.connectors.iter().all(|&(l, _)| !w.traffic.net.lanes[l as usize].blocked), "the junction reopened");
        assert!(w.traffic.cars.values().all(|c| !c.in_stunt()), "everyone back on the lanes");
        (rec, stunts, donuts, blocked)
    }

    #[test]
    fn a_takeover_holds_a_junction_and_breaks_up_when_the_police_arrive() {
        let (rec, stunts, _, blocked) = takeover_night(11);
        assert!(blocked, "the junction was closed to traffic during the show");
        assert!(rec.police, "the police broke it up");
        assert!(rec.residents == 4 && rec.outsiders >= takeover::OUTSIDERS.0, "{} locals, {} outsiders", rec.residents, rec.outsiders);
        assert!(rec.participants >= stunts);
        // Deterministic.
        let (again, s2, _, _) = takeover_night(11);
        assert_eq!((rec.ended, rec.participants, rec.crowd, rec.arrests), (again.ended, again.participants, again.crowd, again.arrests));
        assert_eq!(stunts, s2);
    }

    #[test]
    fn drink_rises_at_a_venue_and_wears_off() {
        let mut w = town(3);
        let c = w.citizens.keys().next().unwrap();
        // A hedonist with no time for rules.
        w.citizens[c].seed = (1..5000u64).find(|&s| { let (ca, h) = driver_traits(s); h > 0.85 && rebelliousness(ca, h) > 0.7 }).unwrap();
        w.citizens[c].age = 30;
        let venue = w.buildings.keys().next().unwrap();
        w.buildings[venue].land_use = LandUse::Entertainment;
        w.citizens[c].activity = Activity::Visiting(venue);
        w.step_drink(150);
        let drunk = w.citizens[c].drunk;
        assert!(drunk >= DRUNK, "an evening at the bar: {drunk}");
        w.citizens[c].owns_car = true;
        assert!(crate::citizens::wants_to_drive_drunk(&w.citizens[c]), "a rebel drives anyway");
        w.citizens[c].activity = Activity::Idle;
        w.step_drink(600);
        assert_eq!(w.citizens[c].drunk, 0.0, "sober by morning");
    }
}
