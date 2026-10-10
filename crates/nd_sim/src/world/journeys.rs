//! [clarity] Where every vehicle and traveller is coming from and going
//! to, and why, in words the inspector can show ("going home to Habitat
//! 21F", "coming from Eastgate Police HQ, patrolling"); who is at the wheel
//! of a city vehicle (its crew are citizens with jobs at the station); and
//! the crash sites behind the traffic danger layer.
//!
//! Origins are noted as they happen: the building a citizen last walked out
//! of (`note_left`), and for a vehicle, where it was when it first showed up
//! on the road (`step_journeys`). Neither is saved: after a load, a trip
//! under way says it came from the street it was on.

use super::*;
use crate::services::{IncidentKind, UnitId, UnitState};
use crate::taxi::CabJob;
use crate::vehicle::Errand;

/// Crash sites kept for the danger layer (the oldest go first).
pub const CRASH_SITES_KEPT: usize = 4000;
/// A crash counts towards the danger of the street segment it is nearest,
/// within this far.
pub const CRASH_SITE_REACH: f32 = 30.0;
/// Crashes on one street segment (in the record) at which people living or
/// working on it start to complain of a dangerous street.
pub const DANGEROUS_STREET: u32 = 4;

/// How the unknown are named.
pub const STRANGER: &str = "unknown person from out of the city";

/// Where a trip began or ends.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Origin {
    Building(BuildingId),
    /// Off the map, in another town.
    OutOfTown,
    /// Somewhere on the street (no building to name).
    Street(Vec2),
}

/// One end of a journey, ready to show: a label, and what selecting it
/// should pick (a building) or where it is.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub label: String,
    pub building: Option<BuildingId>,
    pub car: Option<CarId>,
    pub pos: Option<Vec2>,
}

impl Place {
    fn text(label: impl Into<String>) -> Self {
        Self { label: label.into(), building: None, car: None, pos: None }
    }
}

/// Who is at the wheel.
#[derive(Clone, Debug, PartialEq)]
pub enum Driver {
    Citizen(CitizenId),
    /// A city vehicle's crew: citizens on the station's payroll.
    Crew(Vec<CitizenId>),
    /// Nobody the city knows ("unknown person from out of the city").
    Unknown(String),
}

/// A vehicle's journey in words.
#[derive(Clone, Debug, PartialEq)]
pub struct Journey {
    /// What it is doing: "going home", "patrolling", "looking for a pickup".
    pub purpose: String,
    pub from: Place,
    /// `None` when it has no destination (patrolling, cruising).
    pub to: Option<Place>,
    pub driver: Driver,
}

/// A crash, where and when (for the danger layer).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct CrashSite {
    pub pos: Vec2,
    pub tick: u64,
}

/// The state behind it.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Journeys {
    /// The building each citizen last left (or the park they sat in).
    #[serde(skip)]
    pub left_from: HashMap<CitizenId, BuildingId>,
    /// Where each vehicle on the road came from.
    #[serde(skip)]
    pub car_from: HashMap<CarId, Origin>,
    /// Every crash, oldest first, up to `CRASH_SITES_KEPT`.
    pub crash_sites: Vec<CrashSite>,
    /// The last crash record id taken into `crash_sites`.
    pub last_crash: u32,
}

/// "going home", "going to work", ... for a purpose.
pub fn purpose_phrase(p: &Purpose) -> String {
    match p {
        Purpose::Home => "going home".into(),
        Purpose::Work => "going to work".into(),
        Purpose::Visit(lu) => format!("out to the {}", lu.label().to_lowercase()),
        Purpose::Shop(r) => format!("shopping for {}", r.label()),
        Purpose::Wander => "out for a wander".into(),
        Purpose::Sit => "off to sit in a park".into(),
        Purpose::ToStop => "heading for a stop".into(),
    }
}

impl World {
    /// A citizen walked out of `b` (or got up from a bench in it).
    pub(super) fn note_left(&mut self, id: CitizenId, b: BuildingId) {
        self.journeys.left_from.insert(id, b);
    }

    /// Every tick: note where vehicles that just appeared came from, and
    /// take new crashes into the record of crash sites.
    pub(super) fn step_journeys(&mut self) {
        let fresh: Vec<CarId> = self.traffic.cars.keys().filter(|id| !self.journeys.car_from.contains_key(id)).collect();
        for id in fresh {
            let o = self.origin_of_new_car(id);
            self.journeys.car_from.insert(id, o);
        }
        if self.clock.tick % 60 == 0 {
            let cars = &self.traffic.cars;
            self.journeys.car_from.retain(|id, _| cars.contains_key(*id));
            let citizens = &self.citizens;
            self.journeys.left_from.retain(|id, _| citizens.contains_key(*id));
        }
        let last = self.journeys.last_crash;
        let new: Vec<CrashSite> = self.driving.recent.iter().filter(|r| r.id > last).map(|r| CrashSite { pos: r.pos, tick: r.tick }).collect();
        if let Some(top) = self.driving.recent.iter().map(|r| r.id).max() {
            self.journeys.last_crash = self.journeys.last_crash.max(top);
        }
        if !new.is_empty() {
            let sites = &mut self.journeys.crash_sites;
            sites.extend(new);
            if sites.len() > CRASH_SITES_KEPT {
                let drop = sites.len() - CRASH_SITES_KEPT;
                sites.drain(..drop);
            }
        }
    }

    /// [clarity] A stop made on the way: the next leg starts from `b`.
    pub(super) fn car_departs_from(&mut self, car: CarId, b: BuildingId) {
        self.journeys.car_from.insert(car, Origin::Building(b));
    }

    fn origin_of_new_car(&self, id: CarId) -> Origin {
        let Some(car) = self.traffic.cars.get(id) else { return Origin::OutOfTown };
        if let Some(u) = self.services.unit_of_car(id).and_then(|u| self.services.units.get(u)) {
            // From the station, or (re-sent while out) from the street.
            return if self.building_lot(u.station).is_some_and(|l| l.center.distance(car.pos) < 120.0) { Origin::Building(u.station) } else { Origin::Street(car.pos) };
        }
        if let Some(bus) = &car.bus {
            return Origin::Building(bus.depot);
        }
        if let Some(c) = &car.cargo {
            if crate::world::is_return_trip(c) {
                return Origin::Street(car.pos);
            }
            return if self.buildings.contains_key(c.from) { Origin::Building(c.from) } else { Origin::OutOfTown };
        }
        if car.errand == Errand::Through || self.point_at_map_edge(car.pos) {
            return Origin::OutOfTown;
        }
        if let Some(b) = self.journeys.left_from.get(&car.citizen).copied().filter(|b| self.buildings.contains_key(*b)) {
            return Origin::Building(b);
        }
        if let Errand::Taxi(cab) = car.errand {
            if let Some(c) = self.taxis.cabs.get(cab) {
                if self.building_lot(c.company).is_some_and(|l| l.center.distance(car.pos) < 120.0) {
                    return Origin::Building(c.company);
                }
            }
        }
        Origin::Street(car.pos)
    }

    /// Near the edge of the map (where roads run on to elsewhere).
    pub fn point_at_map_edge(&self, p: Vec2) -> bool {
        let e = crate::landscape::MAP_EDGE - 30.0;
        p.x.abs() > e || p.y.abs() > e
    }

    fn place_of_origin(&self, o: Origin) -> Place {
        match o {
            Origin::Building(b) => self.place_of_building(b),
            Origin::OutOfTown => Place::text("out of town"),
            Origin::Street(p) => Place { label: self.street_name_near(p), building: None, car: None, pos: Some(p) },
        }
    }

    fn place_of_building(&self, b: BuildingId) -> Place {
        match self.buildings.get(b) {
            Some(x) => Place { label: x.name.clone(), building: Some(b), car: None, pos: self.building_entrance(b).or_else(|| self.building_lot(b).map(|l| l.center)) },
            None => Place::text("out of town"),
        }
    }

    /// "the street by Habitat 21F", for a point with no building of its own.
    pub(super) fn street_name_near(&self, p: Vec2) -> String {
        match self.building_nearest(p).filter(|b| self.building_lot(*b).is_some_and(|l| l.center.distance(p) < 150.0)).and_then(|b| self.buildings.get(b)) {
            Some(b) => format!("the street by {}", b.name),
            None => "the street".into(),
        }
    }

    /// [clarity] The crew of a city vehicle: the station's staff, two (or
    /// three) to a unit, in order of the unit's number.
    pub fn unit_crew(&self, uid: UnitId) -> Vec<CitizenId> {
        let Some(u) = self.services.units.get(uid) else { return Vec::new() };
        let Some(b) = self.buildings.get(u.station) else { return Vec::new() };
        let n = u.kind.crew();
        let start = (u.number.max(1) as usize - 1) * n;
        b.workers.iter().skip(start).take(n).copied().filter(|c| self.citizens.contains_key(*c)).collect()
    }

    /// [fire-scene] The unit a citizen is out with: on its crew
    /// (`unit_crew`) while it is out of the station.
    pub fn crew_unit_of(&self, id: CitizenId) -> Option<UnitId> {
        let work = self.citizens.get(id)?.work?;
        self.services.units.iter().filter(|(_, u)| u.station == work && u.out()).map(|(uid, _)| uid).find(|uid| self.unit_crew(*uid).contains(&id))
    }

    /// Where a citizen is headed and why: `(destination, purpose)`, from
    /// whatever they are doing (walking, driving, riding, waiting).
    pub fn citizen_target(&self, id: CitizenId) -> Option<(Option<BuildingId>, Purpose)> {
        let c = self.citizens.get(id)?;
        match &c.activity {
            Activity::Traveling { to, purpose, .. } | Activity::Driving { to, purpose, .. } | Activity::Passenger { to, purpose, .. } => Some((*to, *purpose)),
            Activity::Riding { .. } | Activity::Waiting { .. } | Activity::OnTrain { .. } | Activity::OnMetro { .. } | Activity::Flying { .. } => c.transit.as_ref().map(|t| (Some(t.target), t.purpose)),
            _ => None,
        }
    }

    /// [clarity] What a citizen is doing, in a line: "going home to
    /// Habitat 21F", "going to work at Nova Foods", "at home", "working at
    /// Nova Foods".
    pub fn activity_line(&self, id: CitizenId) -> String {
        let Some(c) = self.citizens.get(id) else { return String::new() };
        let name = |b: BuildingId| self.buildings.get(b).map(|b| b.name.clone());
        if let Some((to, purpose)) = self.citizen_target(id) {
            let how = match (&c.activity, c.mode) {
                (Activity::Driving { .. }, _) => " (driving)",
                (Activity::Riding { .. }, _) => " (on the bus)",
                (Activity::OnTrain { .. }, _) => " (on the train)",
                (Activity::OnMetro { .. }, _) => " (on the subway)",
                (Activity::Passenger { .. }, _) => " (getting a lift)",
                // [pt-transit] Each kind of wait by its name.
                (Activity::Waiting { .. }, _) => match c.transit.map(|t| t.leg) {
                    Some(TransitLeg::Subway { .. }) => " (on the metro platform)",
                    Some(TransitLeg::Bus { .. }) => " (at the bus stop)",
                    Some(TransitLeg::Rail { .. }) => " (on the station platform)",
                    Some(TransitLeg::Taxi) => " (waiting for a cab)",
                    Some(TransitLeg::Air { .. }) => " (at the drone pad)",
                    None => " (waiting for a ride)",
                },
                (Activity::Traveling { .. }, TravelMode::Bike) => " (cycling)",
                _ => "",
            };
            let base = purpose_phrase(&purpose);
            let prep = match purpose {
                Purpose::Home => "to",
                Purpose::Work => "at",
                _ => "at",
            };
            return match to.and_then(name) {
                Some(n) if purpose == Purpose::Home => format!("going home to {n}{how}"),
                Some(n) if purpose == Purpose::Work => format!("going to work at {n}{how}"),
                Some(n) => format!("{base}: {prep} {n}{how}"),
                None => format!("{base}{how}"),
            };
        }
        // [fire-scene] Out on a call with their unit.
        if let Some(u) = self.crew_unit_of(id).and_then(|u| self.services.units.get(u)) {
            let at = u.incident.and_then(|i| self.services.incident(i)).and_then(|i| self.buildings.get(i.building)).map(|b| b.name.clone());
            let doing = match (u.state, at) {
                (UnitState::OnScene, Some(b)) if u.packing() => format!("packing up at {b}"),
                (UnitState::OnScene, Some(b)) => format!("on scene at {b}"),
                (UnitState::Responding, Some(b)) => format!("on the way to {b}"),
                (s, _) => s.label().to_string(),
            };
            return format!("out with {} {}: {doing}", u.kind.call_sign(), u.number);
        }
        if c.inside.is_none() {
            let down = self.down_on_street();
            match self.pose_with(id, &down) {
                crate::world::Pose::Lying if down.contains(&id) => return "lying hurt in the street, waiting for the paramedics".into(),
                crate::world::Pose::Lying => return "lying down on the pavement".into(),
                crate::world::Pose::SittingKerb => return "sitting on the kerb".into(),
                _ => {}
            }
        }
        match (&c.activity, c.inside) {
            (Activity::AtHome, Some(b)) => name(b).map_or("at home".into(), |n| format!("at home in {n}")),
            (Activity::Working, Some(b)) => name(b).map_or("working".into(), |n| format!("working at {n}")),
            (Activity::Visiting(b), _) => name(*b).map_or("out".into(), |n| format!("at {n}")),
            (Activity::Sitting { park, .. }, _) => name(*park).map_or("sitting in the park".into(), |n| format!("sitting in {n}")),
            _ => c.activity.label().to_string(),
        }
    }

    /// [clarity] Where a traveller set off from (the building they last
    /// left), for the route's "o".
    pub fn citizen_origin(&self, id: CitizenId) -> Option<Place> {
        self.citizen_target(id)?;
        let b = self.journeys.left_from.get(&id).copied()?;
        self.buildings.contains_key(b).then(|| self.place_of_building(b))
    }

    /// [clarity] Where a traveller is going, for the route's "x".
    pub fn citizen_destination(&self, id: CitizenId) -> Option<Place> {
        let (to, _) = self.citizen_target(id)?;
        to.map(|b| self.place_of_building(b))
    }

    /// [clarity] A vehicle's journey: what it is doing, where it came
    /// from, where it is going, and who is driving.
    pub fn car_journey(&self, id: CarId) -> Option<Journey> {
        let car = self.traffic.cars.get(id)?;
        let from = self.journeys.car_from.get(&id).copied().map_or_else(|| Place { label: self.street_name_near(car.pos), building: None, car: None, pos: Some(car.pos) }, |o| self.place_of_origin(o));
        let stranger = || Driver::Unknown(STRANGER.to_string());
        // City vehicles: the crew from the station's payroll.
        if let Some(uid) = self.services.unit_of_car(id) {
            let u = &self.services.units[uid];
            let crew = self.unit_crew(uid);
            let driver = if crew.is_empty() { Driver::Unknown(format!("the crew of {} {}", u.kind.call_sign(), u.number)) } else { Driver::Crew(crew) };
            let inc = u.incident.and_then(|i| self.services.incident(i));
            let (purpose, to) = match u.state {
                UnitState::Patrol if u.kind == crate::services::UnitKind::PoliceCar => ("patrolling".to_string(), None),
                UnitState::Patrol => ("on standby in the district".to_string(), None),
                UnitState::Returning => ("going back to the station".to_string(), Some(self.place_of_building(u.station))),
                UnitState::Transporting => ("taking a patient to hospital".to_string(), u.hospital.map(|h| self.place_of_building(h))),
                UnitState::Pursuing => ("in pursuit".to_string(), None),
                _ => match inc {
                    Some(i) => {
                        let what = match i.kind {
                            IncidentKind::Fire => "a fire",
                            IncidentKind::Crime => "a crime",
                            IncidentKind::Medical => "a medical call",
                            IncidentKind::Crash => "a crash",
                        };
                        let to = if i.on_road || !self.buildings.contains_key(i.building) { Place { label: if i.place.is_empty() { "the scene".into() } else { i.place.clone() }, building: None, car: None, pos: Some(i.pos) } } else { self.place_of_building(i.building) };
                        (format!("answering {what}"), Some(to))
                    }
                    None if u.kind == crate::services::UnitKind::PoliceCar => ("patrolling".to_string(), None),
                    None => ("on the road".to_string(), None),
                },
            };
            return Some(Journey { purpose, from, to, driver });
        }
        if let Some(bus) = &car.bus {
            let line = self.transit.routes.get(bus.route);
            let purpose = line.map_or("a bus".to_string(), |r| format!("bus on {}", r.name));
            let to = if bus.to_depot || bus.next_stop == usize::MAX {
                Some(self.place_of_building(bus.depot))
            } else {
                line.and_then(|r| r.stops.get(bus.next_stop)).map(|s| Place { label: format!("stop {}", bus.next_stop + 1), building: None, car: None, pos: Some(s.pos) })
            };
            // [pt-transit] From the stop it last called at (the leg is what
            // is timed), else from the depot.
            let from = match (self.transit.departures.get(&id), line) {
                (Some(&(k, _)), Some(r)) if k < r.stops.len() && !bus.to_depot => Place { label: format!("stop {}", k + 1), building: None, car: None, pos: Some(r.stops[k].pos) },
                _ => from,
            };
            return Some(Journey { purpose, from, to, driver: Driver::Unknown("a bus driver".into()) });
        }
        if let Some(c) = &car.cargo {
            let to = if self.buildings.contains_key(c.to) { self.place_of_building(c.to) } else { Place::text("out of town") };
            let purpose = if crate::world::is_return_trip(c) { "driving back empty".to_string() } else { format!("delivering {:.0} {}", c.amount, c.resource.label()) };
            let driver = if self.buildings.contains_key(c.from) || self.buildings.contains_key(c.to) { Driver::Unknown("a delivery driver".into()) } else { stranger() };
            return Some(Journey { purpose, from, to: Some(to), driver });
        }
        let car_place = |c: CarId| self.traffic.cars.get(c).map(|x| Place { label: self.car_label(c), building: None, car: Some(c), pos: Some(x.pos) });
        let errand_story: Option<(String, Option<Place>)> = match car.errand {
            Errand::Taxi(cab) => {
                let job = self.taxis.cabs.get(cab).map(|c| c.job).unwrap_or_default();
                Some(match job {
                    CabJob::Idle | CabJob::ToRank { .. } => ("looking for a pickup".to_string(), match job {
                        CabJob::ToRank { rank } => Some(self.place_of_building(rank)),
                        _ => None,
                    }),
                    CabJob::Pickup { rider } => (format!("picking up {}", self.citizens.get(rider).map_or("a fare".to_string(), |c| c.name.clone())), self.citizens.get(rider).map(|c| Place { label: c.name.clone(), building: None, car: None, pos: Some(c.pos) })),
                    CabJob::Fare { rider, .. } => {
                        let to = self.citizen_target(rider).and_then(|(b, _)| b).map(|b| self.place_of_building(b));
                        (format!("carrying {}", self.citizens.get(rider).map_or("a fare".to_string(), |c| c.name.clone())), to)
                    }
                    CabJob::ToYard => ("going back to the yard".to_string(), self.taxis.cabs.get(cab).map(|c| self.place_of_building(c.company))),
                    CabJob::Leaving => ("leaving town for good".to_string(), Some(Place::text("out of town"))),
                })
            }
            Errand::Through => Some(("passing through".to_string(), Some(Place::text("out of town")))),
            Errand::Recover(wreck) => Some(("recovering a wreck".to_string(), car_place(wreck))),
            Errand::Tow(_) => Some(("towing a car to the pound".to_string(), None)),
            Errand::Patrol(_) => Some(("on parking patrol".to_string(), None)),
            Errand::Return(b) => Some(("going back to base".to_string(), Some(self.place_of_building(b)))),
            Errand::Sale(d) => Some(("being sold".to_string(), Some(if self.buildings.contains_key(d) { self.place_of_building(d) } else { Place::text("out of town") }))),
            Errand::Leave | Errand::Out => Some(("leaving the city".to_string(), Some(Place::text("out of town")))),
            Errand::Haul { base } => Some(("on a job".to_string(), base.map(|b| self.place_of_building(b)))),
            _ => None,
        };
        // A city resident's own car with nobody known at the wheel (its
        // driver dropped off, or gone) is still theirs.
        let owner = car.owned.and_then(|v| self.vehicles.cars.get(v)).map(|o| o.owner).filter(|o| self.citizens.contains_key(*o));
        let driver = if self.citizens.contains_key(car.citizen) {
            Driver::Citizen(car.citizen)
        } else if let Some(o) = owner {
            Driver::Citizen(o)
        } else {
            stranger()
        };
        if !self.citizens.contains_key(car.citizen) && owner.is_some() {
            let purpose = match car.errand {
                Errand::Search { .. } | Errand::ToKerb | Errand::ToLot(_) => "looking for parking",
                Errand::Deliver => "being delivered to its new owner",
                _ => "on the road",
            };
            return Some(Journey { purpose: purpose.into(), from, to: None, driver });
        }
        if let Some((purpose, to)) = errand_story {
            let driver = match car.errand {
                Errand::Taxi(_) if !self.citizens.contains_key(car.citizen) => Driver::Unknown("a cab driver".into()),
                _ => driver,
            };
            return Some(Journey { purpose, from, to, driver });
        }
        // A private car: its driver's trip.
        let (purpose, to) = match self.citizen_target(car.citizen) {
            Some((to, p)) => {
                let purpose = match car.errand {
                    Errand::Search { .. } | Errand::ToKerb | Errand::ToLot(_) => format!("{}, looking for parking", purpose_phrase(&p)),
                    _ => purpose_phrase(&p),
                };
                (purpose, to.map(|b| self.place_of_building(b)))
            }
            None if self.citizens.contains_key(car.citizen) => ("cruising around".to_string(), None),
            None => ("passing through".to_string(), Some(Place::text("out of town"))),
        };
        Some(Journey { purpose, from, to, driver })
    }

    // ── the danger layer ──────────────────────────────────────────────────

    /// [clarity] Crashes on the record per street segment (the nearest
    /// within `CRASH_SITE_REACH`), worst first.
    pub fn crashes_per_segment(&self) -> Vec<(SegmentId, u32)> {
        let mut counts: HashMap<SegmentId, u32> = HashMap::new();
        for s in &self.journeys.crash_sites {
            if let Some((sid, _, p)) = self.roads.nearest_street(s.pos) {
                if p.distance(s.pos) <= CRASH_SITE_REACH {
                    *counts.entry(sid).or_default() += 1;
                }
            }
        }
        let mut out: Vec<(SegmentId, u32)> = counts.into_iter().collect();
        out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0 .0.cmp(&b.0 .0)));
        out
    }

    /// [clarity] Once a day: people who live or work on a street with
    /// `DANGEROUS_STREET` crashes or more on the record complain of it.
    pub(super) fn daily_dangerous_streets(&mut self) {
        let bad: Vec<(SegmentId, u32)> = self.crashes_per_segment().into_iter().filter(|(_, n)| *n >= DANGEROUS_STREET).collect();
        if bad.is_empty() {
            return;
        }
        let tick = self.clock.tick;
        for (sid, n) in bad {
            // The buildings fronting it.
            let fronting: Vec<BuildingId> = self.buildings.iter().filter(|(_, b)| self.lots.get(b.lot).is_some_and(|l| l.car_segment == Some(sid))).map(|(id, _)| id).collect();
            let mut said = 0;
            for b in fronting {
                let people: Vec<CitizenId> = self.buildings[b].residents.iter().copied().take(2).collect();
                for p in people {
                    let Some(c) = self.citizens.get(p) else { continue };
                    let severity = (0.3 + 0.08 * n as f32).min(0.9);
                    let text = format!("{n} crashes on our street. Someone's going to get killed out there.");
                    self.concerns.said(tick, p, c, crate::concerns::Concern::DangerousStreets, severity, &text);
                    said += 1;
                }
                if said >= 6 {
                    break;
                }
            }
        }
    }
}

/// [clarity] A home farther than this from the nearest clinic or hospital
/// (straight line) complains of it.
pub const HOSPITAL_FAR: f32 = 900.0;
/// Towns smaller than this do not expect a hospital yet.
pub const HEALTHCARE_TOWN: usize = 50;
/// Homes heard from per day on healthcare, at most.
const HEALTHCARE_VOICES: usize = 24;

impl World {
    /// [clarity] Once a day: households far from any clinic or hospital (or
    /// in a town that has none) complain of it.
    pub(super) fn daily_healthcare(&mut self) {
        if self.citizens.len() < HEALTHCARE_TOWN {
            return;
        }
        let health: Vec<Vec2> = self
            .buildings
            .values()
            .filter(|b| b.operating() && !b.is_derelict() && b.service.is_some_and(|s| s.kind == crate::services::ServiceKind::Health))
            .filter_map(|b| self.lots.get(b.lot).map(|l| l.center))
            .collect();
        let day = self.clock.tick / 1440;
        let mut homes: Vec<(BuildingId, Vec2, CitizenId)> = self
            .buildings
            .iter()
            .filter(|(_, b)| b.land_use == LandUse::Residential && b.operating())
            .filter_map(|(id, b)| Some((id, self.lots.get(b.lot)?.center, *b.residents.first()?)))
            .collect();
        // A different few each day.
        homes.sort_by_key(|(id, _, _)| (slotmap::Key::data(id).as_ffi() ^ day.wrapping_mul(0x9E37_79B9)).wrapping_mul(0xBF58_476D_1CE4_E5B9));
        let tick = self.clock.tick;
        let mut said = 0;
        for (_, at, p) in homes {
            if said >= HEALTHCARE_VOICES {
                break;
            }
            let nearest = health.iter().map(|h| h.distance(at)).fold(f32::INFINITY, f32::min);
            let (text, severity) = if health.is_empty() {
                ("Not one hospital in this whole city. Get sick and you wait for the next town.".to_string(), 0.6)
            } else if nearest > HOSPITAL_FAR {
                (format!("The nearest hospital is {:.1} km away. Too far if it's serious.", nearest / 1000.0), 0.35 + 0.2 * ((nearest - HOSPITAL_FAR) / HOSPITAL_FAR).min(1.0))
            } else {
                continue;
            };
            let Some(c) = self.citizens.get(p) else { continue };
            self.concerns.said(tick, p, c, crate::concerns::Concern::Healthcare, severity, text);
            said += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn purposes_read_as_phrases() {
        assert_eq!(purpose_phrase(&Purpose::Home), "going home");
        assert_eq!(purpose_phrase(&Purpose::Work), "going to work");
    }

    #[test]
    fn an_ambulance_from_out_of_town_costs_more_than_a_week_of_a_clinic() {
        use crate::services::{ServiceKind, OUT_OF_TOWN_FEE};
        let fee = OUT_OF_TOWN_FEE[IncidentKind::Medical.slot()];
        assert!(fee >= 7.0 * ServiceKind::Health.upkeep_per_day(crate::buildings::Tier::Low), "{fee}");
    }

    #[test]
    fn the_tourist_tax_is_a_setting_and_a_dear_one_puts_visitors_off() {
        use crate::tourism::TOURIST_TAX;
        let mut t = crate::tax::TaxBook::default();
        assert_eq!(t.tourist_tax(), TOURIST_TAX, "the standard rate by default");
        assert_eq!(t.tourist_pull(), 1.0);
        t.set_tourist_tax(0.30);
        assert_eq!(t.tourist_tax(), 0.30);
        assert!(t.tourist_pull() < 0.6, "a dear tax keeps visitors away: {}", t.tourist_pull());
        t.set_tourist_tax(0.0);
        assert!(t.tourist_pull() > 1.3, "no tax draws more");
        assert_eq!(t.set_tourist_tax(9.0), crate::tax::MAX_TOURIST_TAX);
        t.reset();
        assert_eq!(t.tourist_tax(), TOURIST_TAX);
        // Saved with the rest of the book; old books load at the standard.
        t.set_tourist_tax(0.2);
        let back: crate::tax::TaxBook = ron::from_str(&ron::to_string(&t).unwrap()).unwrap();
        assert_eq!(back.tourist_tax(), 0.2);
        let old: crate::tax::TaxBook = ron::from_str("(rates: [])").unwrap();
        assert_eq!(old.tourist_tax(), TOURIST_TAX);
    }
}
