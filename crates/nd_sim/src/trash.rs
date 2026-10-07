//! Trash. Every building fills its bins every day: homes by the people in
//! them (the rich throw out more), workplaces by their staff, by land use.
//! A bin holds about `BIN_DAYS` days of it; past that it overflows onto the
//! pavement, smells, brings complaints (mood and memories) from the
//! residents and the neighbours, takes land value off the block and, with
//! rats about, raises the chance of falling ill (the services' medical
//! hook).
//!
//! The city collects with a trash depot (Buildings > Services): every staffed
//! depot runs trucks (two crew each) from 06:00 to 18:00. A truck drives the
//! streets as a car of the traffic model (a garbage truck body), stops at the
//! fullest covered building within its depot's radius that nobody else is
//! heading for, spends a few minutes loading, and goes on until it is
//! nearly full or the shift ends, then unloads at the depot. A truck makes
//! some forty to seventy stops a day, so one depot serves a district, not a
//! city.
//!
//! Without the city's trucks, the rich pay a private hauler. Rich homes (a
//! high tier building, or one where most households are rich) and high-tier
//! businesses outside every depot's reach subscribe for a daily fee; the
//! company sends trucks in from the edge of the map to empty its clients'
//! bins and drives the trash out of town. As the city's coverage grows its
//! clients cancel: its demand is whatever the city does not cover. Everyone
//! else's trash piles up.
//!
//! [economy] Where the city's rounds leave many buildings out, local trash
//! companies open (`crate::pressure`): a yard of trucks on industrial land.
//! A local company serves every building out of the city's reach within
//! `COMPANY_REACH` of its yard, not only the rich, for a fee that falls the
//! more companies compete there (`company_fee`); a building takes the
//! nearest. The fees are the company's takings; it goes under like any
//! business that cannot pay its crews.

use crate::buildings::{BuildingId, Tier};
use crate::citizens::{CitizenId, HouseholdId};
use crate::land_value::LandValueBuilder;
use crate::roads::SegmentId;
use crate::services::ServiceKind;
use crate::traffic::CarId;
use crate::vehicle::{BodyType, Livery, VehicleClass, VehicleModel};
use crate::zoning::LandUse;
use crate::World;
use glam::Vec2;
use rand::Rng;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};
use std::collections::BTreeMap;

new_key_type! {
    pub struct TruckId;
}

// ── balance ────────────────────────────────────────────────────────────────

/// Kilograms a person throws out a day, by household class (low, mid, high).
pub const KG_PER_PERSON_DAY: [f32; 3] = [1.5, 1.8, 2.4];
/// Kilograms a worker's day leaves behind, by land use.
pub fn kg_per_job_day(lu: LandUse) -> f32 {
    match lu {
        LandUse::Commercial => 1.4,
        LandUse::Entertainment => 2.2,
        LandUse::Industrial => 2.8,
        LandUse::Civic => 0.6,
        LandUse::Residential | LandUse::Park => 0.0,
    }
}
/// A building's bins (or dumpster) hold this many days of its trash.
pub const BIN_DAYS: f32 = 3.0;
pub const MIN_BIN: f32 = 8.0;
/// A garbage truck carries this much before it must unload.
pub const TRUCK_CAPACITY: f32 = 9_000.0;
/// It heads to unload when this full.
pub const TRUCK_FULL: f32 = 0.85;
/// Trucks per staffed depot, by tier; each needs two crew.
pub const TRUCKS: [usize; 3] = [2, 4, 7];
pub const TRUCK_CREW: usize = 2;
/// The collection shift (hours of the clock).
pub const SHIFT: (u64, u64) = (6, 18);
pub const PRIVATE_SHIFT: (u64, u64) = (7, 17);
/// Bins are worth a stop from this fill.
pub const PICKUP_FILL: f32 = 0.3;
/// Minutes loading at a stop: a base plus one per this many kilograms.
pub const LOAD_BASE: u16 = 2;
pub const LOAD_KG_PER_MIN: f32 = 300.0;
pub const LOAD_MAX: u16 = 8;
pub const UNLOAD_TICKS: u16 = 6;
/// The private hauler: daily fee per rich household and per job at a rich
/// business, clients per truck and the most trucks it runs.
pub const PRIVATE_FEE_HOUSEHOLD: f32 = 1.5;
pub const PRIVATE_FEE_JOB: f32 = 0.12;
pub const CLIENTS_PER_TRUCK: usize = 20;
pub const MAX_PRIVATE_TRUCKS: usize = 5;
pub const COMPANY: &str = "Haul-Away Waste Co.";
/// [economy] A local trash company serves buildings this far from its yard.
pub const COMPANY_REACH: f32 = 1800.0;
/// [economy] Trucks a local company runs at most.
pub const COMPANY_TRUCKS: usize = 4;
/// [economy] Households in cheap buildings sign up only at a fee this low.
pub const LOW_INCOME_FEE: f32 = 1.0;

/// [economy] A household's daily fee with `rivals` local companies within
/// reach: competition brings it down from the out-of-town hauler's.
pub fn company_fee(rivals: usize) -> f32 {
    if rivals == 0 {
        return PRIVATE_FEE_HOUSEHOLD;
    }
    (PRIVATE_FEE_HOUSEHOLD * 0.8 / (1.0 + 0.3 * (rivals as f32 - 1.0))).max(PRIVATE_FEE_HOUSEHOLD * 0.4)
}
/// Overflow: mood lost a day by the residents (per level, up to two levels),
/// by homes within `SMELL_RADIUS`, and the hourly chance a resident inside
/// falls ill per level (rats).
pub const COMPLAINT_MOOD: f32 = 0.03;
pub const SMELL_RADIUS: f32 = 40.0;
pub const SMELL_MOOD: f32 = 0.01;
pub const RAT_ILLNESS: f32 = 0.0003;
/// Land value lost around an overflowing building per level (radius `TRASH_LV_RADIUS`).
pub const TRASH_LV: f32 = 0.04;
pub const TRASH_LV_RADIUS: f32 = 70.0;

/// Who runs a truck.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Hauler {
    /// The city, from this depot.
    City(BuildingId),
    /// The private company, from out of town.
    Private,
    /// [economy] A local trash company, from its yard.
    Company(BuildingId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TruckState {
    /// Parked at the depot (or just arrived from out of town).
    Idle,
    ToStop,
    /// Loading at `target`, `wait` minutes left.
    Collecting,
    ToDump,
    Unloading,
    /// Private trucks driving the load out of town.
    Leaving,
}

impl TruckState {
    pub fn label(self) -> &'static str {
        match self {
            TruckState::Idle => "parked",
            TruckState::ToStop => "driving to a pickup",
            TruckState::Collecting => "loading",
            TruckState::ToDump => "returning to the depot",
            TruckState::Unloading => "unloading",
            TruckState::Leaving => "hauling out of town",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrashTruck {
    pub hauler: Hauler,
    pub number: u8,
    pub state: TruckState,
    pub car: Option<CarId>,
    pub pos: Vec2,
    pub heading: f32,
    /// Where it is driving to.
    pub dest: Vec2,
    pub target: Option<BuildingId>,
    pub load: f32,
    pub wait: u16,
    pub stops_today: u16,
}

/// A building's bins.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Bin {
    pub stored: f32,
    pub capacity: f32,
    /// Kilograms produced a day at the last count.
    pub daily: f32,
    /// Within a staffed depot's reach.
    pub covered: bool,
    /// Pays the private hauler.
    pub private: bool,
    pub overflow_days: u8,
    pub last_pickup: u64,
    /// [economy] The local trash company it pays, if any (else the hauler
    /// from out of town when `private`).
    pub company: Option<BuildingId>,
    /// [economy] Its daily fee per household.
    pub fee: f32,
}

impl Bin {
    /// How far past full: 0 at or under capacity, 1 at twice it.
    pub fn overflow(&self) -> f32 {
        if self.capacity <= 0.0 {
            0.0
        } else {
            ((self.stored - self.capacity) / self.capacity).max(0.0)
        }
    }

    pub fn fill(&self) -> f32 {
        if self.capacity <= 0.0 {
            0.0
        } else {
            self.stored / self.capacity
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TrashStats {
    pub produced_today: f32,
    pub collected_city_today: f32,
    pub collected_private_today: f32,
    pub produced_yesterday: f32,
    pub collected_city_yesterday: f32,
    pub collected_private_yesterday: f32,
    pub collected_city_total: f32,
    pub collected_private_total: f32,
    pub stops_today: u32,
    pub overflowing_buildings: u32,
    pub covered_buildings: u32,
    pub private_clients: u32,
    pub private_revenue_today: f32,
    pub private_revenue_yesterday: f32,
    pub private_revenue_total: f32,
    pub complaints_today: u32,
    pub complaints_yesterday: u32,
    pub rat_illness_total: u32,
    /// [economy] Buildings signed with a local company, and the mean
    /// household fee they pay.
    pub local_clients: u32,
    pub local_fee: f32,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Trash {
    pub bins: BTreeMap<BuildingId, Bin>,
    pub trucks: SlotMap<TruckId, TrashTruck>,
    pub stats: TrashStats,
}

impl Trash {
    pub fn truck_of_car(&self, car: CarId) -> Option<TruckId> {
        self.trucks.iter().find(|(_, t)| t.car == Some(car)).map(|(id, _)| id)
    }

    pub fn bin(&self, b: BuildingId) -> Option<&Bin> {
        self.bins.get(&b)
    }

    /// Kilograms lying in every bin in town.
    pub fn stored(&self) -> f32 {
        self.bins.values().map(|b| b.stored).sum()
    }
}

/// The body a truck is drawn with.
pub fn truck_model(h: Hauler, number: u8) -> VehicleModel {
    let mut m = VehicleModel::new(if h == Hauler::Private { VehicleClass::Expensive } else { VehicleClass::Mid }, BodyType::GarbageTruck);
    match h {
        Hauler::City(_) => {
            m.livery = Livery::Utility;
            m.wear = (number % 2) as u8;
        }
        Hauler::Private => {
            m.paint = 3 + number % 3;
        }
        // [economy] Each local company in its own colour.
        Hauler::Company(b) => {
            m.paint = (slotmap::Key::data(&b).as_ffi() % 6) as u8;
            m.wear = 1;
        }
    }
    m
}

impl World {
    fn in_shift(&self, (from, to): (u64, u64)) -> bool {
        (from..to).contains(&self.clock.hour())
    }

    /// Staffed depots: (building, centre, radius, trucks it can crew).
    pub fn trash_depots(&self) -> Vec<(BuildingId, Vec2, f32, usize)> {
        self.buildings
            .iter()
            .filter_map(|(id, b)| b.service.filter(|s| s.kind == ServiceKind::TrashDepot).map(|s| (id, b, s)))
            .filter(|(_, b, _)| !b.is_derelict() && b.construction.is_none())
            .filter_map(|(id, b, s)| self.lots.get(b.lot).map(|l| (id, l.center, s.kind.radius(s.tier), TRUCKS[s.tier.index()].min(b.workers.len() / TRUCK_CREW))))
            .collect()
    }

    /// Where a truck pulls up for a building: its car point, or the street
    /// entrance of a lot on a path.
    fn kerb_of(&self, b: BuildingId) -> Option<(SegmentId, Vec2)> {
        let l = self.building_lot(b)?;
        let (seg, p) = match l.car_segment {
            Some(s) => (s, l.car_point),
            None => (l.front_segment, l.entrance()),
        };
        self.roads.try_segment(seg).map(|_| (seg, p))
    }

    /// Kilograms a building throws out a day, and its bin size.
    fn trash_rate(&self, id: BuildingId) -> (f32, f32) {
        let Some(b) = self.buildings.get(id) else { return (0.0, 0.0) };
        if b.construction.is_some() || b.land_use == LandUse::Park || b.is_power() || b.rail.is_some() || b.is_parking() {
            return (0.0, 0.0);
        }
        let mut daily = 0.0;
        for h in &b.households {
            let Some(hh) = self.households.get(*h) else { continue };
            let income: f32 = hh.members.iter().filter_map(|m| self.citizens.get(*m)).map(|c| c.income).sum();
            let earners = hh.members.iter().filter_map(|m| self.citizens.get(*m)).filter(|c| c.income > 0.0).count().max(1) as f32;
            daily += hh.members.len() as f32 * KG_PER_PERSON_DAY[Tier::for_income(income / earners).index()];
        }
        let lu = if b.service.is_some() { LandUse::Civic } else { b.land_use };
        daily += b.workers.len() as f32 * kg_per_job_day(lu);
        // Bins are sized (and added) for the people using the building.
        (daily, (daily * BIN_DAYS).max(MIN_BIN))
    }

    /// [economy] Open local trash companies: (yard, centre, trucks it can crew).
    pub fn trash_companies(&self) -> Vec<(BuildingId, Vec2, usize)> {
        self.buildings
            .iter()
            .filter(|(_, b)| b.construction.is_none() && !b.is_derelict() && b.business.as_ref().is_some_and(|z| z.kind == crate::market::BusinessKind::TrashCompany && !z.closed))
            .filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (id, l.center, (b.workers.len() / TRUCK_CREW).min(COMPANY_TRUCKS))))
            .collect()
    }

    /// A rich building that would pay the private hauler.
    pub(crate) fn rich_client(&self, id: BuildingId) -> bool {
        let Some(b) = self.buildings.get(id) else { return false };
        if b.tier == Tier::High && (b.homes > 0 || b.business.is_some()) {
            return true;
        }
        if b.households.is_empty() {
            return false;
        }
        let rich = b
            .households
            .iter()
            .filter_map(|h| self.households.get(*h))
            .filter(|hh| {
                let income: f32 = hh.members.iter().filter_map(|m| self.citizens.get(*m)).map(|c| c.income).sum();
                let earners = hh.members.iter().filter_map(|m| self.citizens.get(*m)).filter(|c| c.income > 0.0).count().max(1) as f32;
                Tier::for_income(income / earners) == Tier::High
            })
            .count();
        rich * 2 >= b.households.len()
    }

    // ── the hourly round ───────────────────────────────────────────────────

    /// Bins fill; coverage and the hauler's client list are refreshed; rats.
    pub(crate) fn hourly_trash(&mut self) {
        crate::prof_scope!("hourly_trash");
        let depots = self.trash_depots();
        let companies = self.trash_companies();
        let ids: Vec<BuildingId> = self.buildings.keys().collect();
        let mut produced = 0.0;
        let (mut covered_n, mut clients, mut overflowing) = (0u32, 0u32, 0u32);
        let (mut local_n, mut local_fees) = (0u32, 0.0f32);
        for id in ids {
            let (daily, capacity) = self.trash_rate(id);
            if capacity <= 0.0 {
                self.society.trash.bins.remove(&id);
                continue;
            }
            let at = self.building_lot(id).map_or(Vec2::ZERO, |l| l.center);
            let covered = depots.iter().any(|d| d.3 > 0 && d.1.distance(at) <= d.2);
            // [economy] Out of the city's reach: the nearest local company,
            // at the fee the competition there sets; the rich take the
            // hauler from out of town when no company comes this far.
            let (company, fee) = if covered {
                (None, 0.0)
            } else {
                let mut near: Vec<(f32, BuildingId)> = companies.iter().map(|c| (c.1.distance(at), c.0)).filter(|(d, _)| *d <= COMPANY_REACH).collect();
                near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                let fee = company_fee(near.len());
                let cheap = self.buildings.get(id).is_some_and(|b| b.tier == Tier::Low && b.homes > 0);
                match near.first() {
                    Some(&(_, c)) if !cheap || fee <= LOW_INCOME_FEE || self.rich_client(id) => (Some(c), fee),
                    _ => (None, PRIVATE_FEE_HOUSEHOLD),
                }
            };
            let private = !covered && (company.is_some() || self.rich_client(id));
            if company.is_some() {
                local_n += 1;
                local_fees += fee;
            }
            let bin = self.society.trash.bins.entry(id).or_default();
            bin.daily = daily;
            bin.capacity = capacity;
            bin.stored += daily / 24.0;
            bin.covered = covered;
            bin.private = private;
            bin.company = company;
            bin.fee = if private { fee } else { 0.0 };
            produced += daily / 24.0;
            covered_n += covered as u32;
            clients += private as u32;
            overflowing += (bin.overflow() > 0.0) as u32;
        }
        let ids: Vec<BuildingId> = self.society.trash.bins.keys().copied().collect();
        for id in ids {
            if !self.buildings.contains_key(id) {
                self.society.trash.bins.remove(&id);
            }
        }
        let s = &mut self.society.trash.stats;
        s.produced_today += produced;
        s.covered_buildings = covered_n;
        s.private_clients = clients;
        s.overflowing_buildings = overflowing;
        s.local_clients = local_n;
        s.local_fee = if local_n > 0 { local_fees / local_n as f32 } else { 0.0 };
        // Rats: people inside an overflowing building may fall ill.
        if self.config.emergencies {
            let sick: Vec<(BuildingId, f32)> = self.society.trash.bins.iter().filter(|(_, b)| b.overflow() > 0.0).map(|(id, b)| (*id, b.overflow().min(3.0))).collect();
            for (bid, level) in sick {
                let inside: Vec<CitizenId> = self.buildings.get(bid).map(|b| b.residents.clone()).unwrap_or_default();
                for cid in inside {
                    if !self.citizens.get(cid).is_some_and(|c| c.inside == Some(bid)) {
                        continue;
                    }
                    if self.society_rng().random::<f32>() < RAT_ILLNESS * level && !self.is_patient(cid) && self.fall_ill(cid).is_some() {
                        self.society.trash.stats.rat_illness_total += 1;
                        let tick = self.clock.tick;
                        if let Some(c) = self.citizens.get_mut(cid) {
                            c.remember(tick, "Rats in the bins downstairs. Now I'm sick.");
                            self.concerns.heard(cid, c, crate::concerns::Concern::Trash, 0.7);
                        }
                    }
                }
            }
        }
    }

    // ── every tick: the trucks ─────────────────────────────────────────────

    pub(crate) fn step_trash(&mut self) {
        let tick = self.clock.tick;
        if tick % 5 == 2 {
            self.staff_trucks();
        }
        // Cars that vanished (roads rebuilt, a save loaded): carry on from
        // where they were going.
        let lost: Vec<TruckId> = self.society.trash.trucks.iter().filter(|(_, t)| t.car.is_some_and(|c| !self.traffic.cars.contains_key(c))).map(|(id, _)| id).collect();
        for id in lost {
            let t = &mut self.society.trash.trucks[id];
            t.car = None;
            t.pos = t.dest;
            self.trash_truck_arrived(id);
        }
        let ids: Vec<TruckId> = self.society.trash.trucks.keys().collect();
        for id in ids {
            let Some(state) = self.society.trash.trucks.get(id).map(|t| t.state) else { continue };
            match state {
                TruckState::Collecting | TruckState::Unloading => {
                    let t = &mut self.society.trash.trucks[id];
                    t.wait = t.wait.saturating_sub(1);
                    if t.wait > 0 {
                        continue;
                    }
                    if state == TruckState::Collecting {
                        self.load_truck(id);
                    } else {
                        t.load = 0.0;
                        t.state = TruckState::Idle;
                    }
                }
                TruckState::Idle if tick % 5 == 0 => self.next_job(id),
                _ => {}
            }
        }
    }

    /// Depots put the trucks their crews can drive on duty; the private
    /// hauler sends in what its clients' full bins need.
    fn staff_trucks(&mut self) {
        let depots = self.trash_depots();
        let shift = self.in_shift(SHIFT);
        // Trucks of depots that are gone, or not crewed any more, go once parked.
        let alive: Vec<(BuildingId, usize)> = depots.iter().map(|d| (d.0, d.3)).collect();
        let mut count: BTreeMap<BuildingId, usize> = BTreeMap::new();
        let mut drop: Vec<TruckId> = Vec::new();
        for (id, t) in self.society.trash.trucks.iter() {
            if let Hauler::City(d) = t.hauler {
                let n = count.entry(d).or_default();
                *n += 1;
                let crewed = alive.iter().find(|a| a.0 == d).map_or(0, |a| a.1);
                // [services] Parked at a depot that still stands: into the
                // shed. A truck whose depot is gone drives off the map
                // instead (`truck_home`), not vanish where it stands.
                let home = self.buildings.contains_key(d);
                if home && *n > crewed && t.car.is_none() && matches!(t.state, TruckState::Idle) {
                    drop.push(id);
                }
            }
        }
        for id in drop {
            self.society.trash.trucks.remove(id);
        }
        if shift {
            for (d, at, _, crewed) in depots {
                let have = self.society.trash.trucks.values().filter(|t| t.hauler == Hauler::City(d)).count();
                let home = self.kerb_of(d).map_or(at, |k| k.1);
                for k in have..crewed {
                    self.society.trash.trucks.insert(TrashTruck { hauler: Hauler::City(d), number: k as u8 + 1, state: TruckState::Idle, car: None, pos: home, heading: 0.0, dest: home, target: None, load: 0.0, wait: 0, stops_today: 0 });
                }
            }
        }
        // [economy] Local companies crew their trucks from their yards;
        // trucks of a company gone or short of crew go once parked.
        let companies = self.trash_companies();
        let mut count: BTreeMap<BuildingId, usize> = BTreeMap::new();
        let mut drop: Vec<TruckId> = Vec::new();
        for (id, t) in self.society.trash.trucks.iter() {
            if let Hauler::Company(c) = t.hauler {
                let n = count.entry(c).or_default();
                *n += 1;
                let crewed = companies.iter().find(|x| x.0 == c).map_or(0, |x| x.2);
                if self.buildings.contains_key(c) && *n > crewed && t.car.is_none() && matches!(t.state, TruckState::Idle) {
                    drop.push(id);
                }
            }
        }
        for id in drop {
            self.society.trash.trucks.remove(id);
        }
        if self.in_shift(PRIVATE_SHIFT) {
            for (c, at, crewed) in companies {
                let due = self.society.trash.bins.values().filter(|b| b.company == Some(c) && b.fill() >= PICKUP_FILL).count();
                let wanted = due.div_ceil(CLIENTS_PER_TRUCK).min(crewed);
                let have = self.society.trash.trucks.values().filter(|t| t.hauler == Hauler::Company(c)).count();
                let home = self.kerb_of(c).map_or(at, |k| k.1);
                for k in have..wanted {
                    self.society.trash.trucks.insert(TrashTruck { hauler: Hauler::Company(c), number: k as u8 + 1, state: TruckState::Idle, car: None, pos: home, heading: 0.0, dest: home, target: None, load: 0.0, wait: 0, stops_today: 0 });
                }
            }
        }
        // The private hauler, from the edge of town.
        if self.in_shift(PRIVATE_SHIFT) {
            let due = self.society.trash.bins.values().filter(|b| b.private && b.company.is_none() && b.fill() >= PICKUP_FILL).count();
            let wanted = due.div_ceil(CLIENTS_PER_TRUCK).min(MAX_PRIVATE_TRUCKS);
            let have = self.society.trash.trucks.values().filter(|t| t.hauler == Hauler::Private).count();
            if have < wanted {
                if let Some(edge) = self.trash_edge() {
                    for k in have..wanted {
                        self.society.trash.trucks.insert(TrashTruck { hauler: Hauler::Private, number: k as u8 + 1, state: TruckState::Idle, car: None, pos: edge, heading: 0.0, dest: edge, target: None, load: 0.0, wait: 0, stops_today: 0 });
                    }
                }
            }
        }
    }

    /// Where the private hauler's trucks come from and go: the end of a
    /// road at the edge of town farthest from the centre.
    fn trash_edge(&self) -> Option<Vec2> {
        self.society_city_leaves().into_iter().map(|n| self.roads.node(n).pos).max_by(|a, b| a.length().total_cmp(&b.length()).then(a.x.total_cmp(&b.x)))
    }

    /// The best pickup for a truck: the fullest bin it serves that nobody
    /// else is heading for, nearer ones first.
    fn pick_stop(&self, id: TruckId) -> Option<BuildingId> {
        let t = self.society.trash.trucks.get(id)?;
        let claimed: Vec<BuildingId> = self.society.trash.trucks.iter().filter(|(k, _)| *k != id).filter_map(|(_, t)| t.target).collect();
        let reach = match t.hauler {
            Hauler::City(d) => {
                let b = self.buildings.get(d)?;
                let s = b.service?;
                Some((self.lots.get(b.lot)?.center, s.kind.radius(s.tier)))
            }
            Hauler::Private | Hauler::Company(_) => None,
        };
        let mut best: Option<(f32, BuildingId)> = None;
        for (bid, bin) in &self.society.trash.bins {
            if bin.fill() < PICKUP_FILL || claimed.contains(bid) {
                continue;
            }
            let Some(at) = self.building_lot(*bid).map(|l| l.center) else { continue };
            let ok = match (reach, t.hauler) {
                (Some((c, r)), _) => bin.covered && c.distance(at) <= r,
                // [economy] A local company's own clients; the hauler from
                // out of town the rest.
                (None, Hauler::Company(c)) => bin.private && bin.company == Some(c),
                (None, _) => bin.private && bin.company.is_none(),
            };
            if !ok {
                continue;
            }
            let score = bin.fill().min(3.0) - t.pos.distance(at) / 500.0;
            if best.is_none_or(|(s, _)| score > s) {
                best = Some((score, *bid));
            }
        }
        best.map(|(_, b)| b)
    }

    /// Drive a truck to `to`. With no road there it simply arrives.
    fn drive_truck(&mut self, id: TruckId, to: (SegmentId, Vec2), state: TruckState) {
        let (from, hauler, number) = {
            let t = &self.society.trash.trucks[id];
            (t.pos, t.hauler, t.number)
        };
        let route = self.roads.nearest_street(from).and_then(|(seg, _, q)| self.traffic.net.find_route(seg, q, to.0, to.1));
        // [services] Leaving town with no road there yet: it waits where it
        // stands and tries again (`next_job`), rather than "arriving" at the
        // edge, which took it out of the world on the spot.
        if state == TruckState::Leaving && route.is_none() && from.distance(to.1) > 8.0 {
            self.society.trash.trucks[id].state = TruckState::Idle;
            return;
        }
        let t = &mut self.society.trash.trucks[id];
        t.state = state;
        t.dest = to.1;
        match route {
            Some((lanes, s0, s1)) if from.distance(to.1) > 8.0 => {
                let car = self.traffic.spawn_car(CitizenId::default(), None, lanes, s0, s1, 0);
                self.traffic.cars[car].model = truck_model(hauler, number);
                self.society.trash.trucks[id].car = Some(car);
            }
            _ => {
                let t = &mut self.society.trash.trucks[id];
                t.pos = to.1;
                self.trash_truck_arrived(id);
            }
        }
    }

    /// An idle truck: out to the next pickup, or (private) out of town.
    fn next_job(&mut self, id: TruckId) {
        let Some(t) = self.society.trash.trucks.get(id) else { return };
        let hauler = t.hauler;
        // [services] A city truck whose depot is gone has no round to run.
        if let Hauler::City(d) | Hauler::Company(d) = hauler {
            if !self.buildings.contains_key(d) {
                self.truck_home(id);
                return;
            }
        }
        let on = match hauler {
            Hauler::City(_) => self.in_shift(SHIFT),
            Hauler::Private | Hauler::Company(_) => self.in_shift(PRIVATE_SHIFT),
        };
        let stop = if on && t.load < TRUCK_CAPACITY * TRUCK_FULL { self.pick_stop(id) } else { None };
        match stop.and_then(|b| self.kerb_of(b).map(|k| (b, k))) {
            Some((b, kerb)) => {
                self.society.trash.trucks[id].target = Some(b);
                self.drive_truck(id, kerb, TruckState::ToStop);
            }
            None => self.truck_home(id),
        }
    }

    /// Back to the depot to unload, or (private) away out of town.
    fn truck_home(&mut self, id: TruckId) {
        let Some(t) = self.society.trash.trucks.get_mut(id) else { return };
        t.target = None;
        match t.hauler {
            Hauler::City(d) | Hauler::Company(d) => {
                let at_depot = self.kerb_of(d);
                let t = &self.society.trash.trucks[id];
                match at_depot {
                    Some(k) if t.pos.distance(k.1) > 8.0 => self.drive_truck(id, k, TruckState::ToDump),
                    Some(_) if t.load > 0.0 => {
                        let t = &mut self.society.trash.trucks[id];
                        t.state = TruckState::Unloading;
                        t.wait = UNLOAD_TICKS;
                    }
                    Some(_) => self.society.trash.trucks[id].state = TruckState::Idle,
                    // [services] The depot is gone: off the map with the
                    // load, as the private hauler's trucks go (it used to
                    // vanish at the kerb).
                    None => self.truck_leaves(id),
                }
            }
            Hauler::Private => self.truck_leaves(id),
        }
    }

    /// [services] A truck with no depot in town drives to the edge of town
    /// and is gone there. Only with no road end at all to drive to is it
    /// taken away where it stands (`drive_truck` sets it there at once when
    /// no road leads from it).
    fn truck_leaves(&mut self, id: TruckId) {
        let edge = self.trash_edge().and_then(|p| self.roads.nearest_street(p).map(|(s, _, q)| (s, q)));
        match edge {
            Some(k) => self.drive_truck(id, k, TruckState::Leaving),
            // No road end anywhere: the network is gone (nothing left to
            // drive to or from); unavoidable.
            None => {
                self.society.trash.trucks.remove(id);
            }
        }
    }

    /// A truck got where it was going.
    fn trash_truck_arrived(&mut self, id: TruckId) {
        let Some(t) = self.society.trash.trucks.get_mut(id) else { return };
        match t.state {
            TruckState::ToStop => {
                let stored = t.target.and_then(|b| self.society.trash.bins.get(&b)).map_or(0.0, |b| b.stored);
                let t = &mut self.society.trash.trucks[id];
                t.state = TruckState::Collecting;
                t.wait = (LOAD_BASE + (stored / LOAD_KG_PER_MIN) as u16).min(LOAD_MAX);
            }
            TruckState::ToDump => {
                t.state = TruckState::Unloading;
                t.wait = UNLOAD_TICKS;
            }
            TruckState::Leaving => {
                self.society.trash.trucks.remove(id);
            }
            _ => t.state = TruckState::Idle,
        }
    }

    /// Loading done: the bin is emptied into the truck, as far as it holds.
    fn load_truck(&mut self, id: TruckId) {
        let tick = self.clock.tick;
        let (target, room, hauler) = {
            let t = &self.society.trash.trucks[id];
            (t.target, TRUCK_CAPACITY - t.load, t.hauler)
        };
        let take = match target.and_then(|b| self.society.trash.bins.get_mut(&b)) {
            Some(bin) => {
                let take = bin.stored.min(room).max(0.0);
                bin.stored -= take;
                bin.last_pickup = tick;
                if bin.overflow() <= 0.0 {
                    bin.overflow_days = 0;
                }
                take
            }
            None => 0.0,
        };
        let s = &mut self.society.trash.stats;
        s.stops_today += 1;
        match hauler {
            Hauler::City(_) => {
                s.collected_city_today += take;
                s.collected_city_total += take;
            }
            Hauler::Private | Hauler::Company(_) => {
                s.collected_private_today += take;
                s.collected_private_total += take;
            }
        }
        let t = &mut self.society.trash.trucks[id];
        t.load += take;
        t.stops_today += 1;
        t.target = None;
        t.state = TruckState::Idle;
        self.next_job(id);
    }

    /// The traffic model says a truck's car reached the end of its route.
    pub(crate) fn trash_car_arrived(&mut self, car: CarId) {
        let Some(id) = self.society.trash.truck_of_car(car) else { return };
        let Some(c) = self.traffic.remove_car(car) else { return };
        let t = &mut self.society.trash.trucks[id];
        t.car = None;
        t.pos = c.pos;
        t.heading = c.heading;
        self.trash_truck_arrived(id);
    }

    // ── midnight ───────────────────────────────────────────────────────────

    /// Complaints, the hauler's fees, and the day's counters.
    pub(crate) fn daily_trash(&mut self) {
        crate::prof_scope!("daily_trash");
        let tick = self.clock.tick;
        // Overflowing bins: the residents complain, the block smells.
        let over: Vec<(BuildingId, f32)> = self.society.trash.bins.iter().filter(|(_, b)| b.overflow() > 0.0).map(|(id, b)| (*id, b.overflow())).collect();
        let mut complaints = 0;
        let homes: Vec<(BuildingId, Vec2)> = self.buildings.iter().filter(|(_, b)| !b.residents.is_empty()).filter_map(|(id, b)| self.lots.get(b.lot).map(|l| (id, l.center))).collect();
        // [perf] Homes bucketed by `SMELL_RADIUS` cells: each overflowing
        // bin looks at the homes in nine cells, not at every home in town.
        let smell_cell = |p: Vec2| ((p.x / SMELL_RADIUS).floor() as i32, (p.y / SMELL_RADIUS).floor() as i32);
        let mut home_cells: std::collections::HashMap<(i32, i32), Vec<usize>> = std::collections::HashMap::new();
        for (k, (_, p)) in homes.iter().enumerate() {
            home_cells.entry(smell_cell(*p)).or_default().push(k);
        }
        for (bid, level) in &over {
            if let Some(bin) = self.society.trash.bins.get_mut(bid) {
                bin.overflow_days = bin.overflow_days.saturating_add(1);
            }
            let Some(b) = self.buildings.get(*bid) else { continue };
            let name = b.name.clone();
            let residents = b.residents.clone();
            let at = self.lots.get(b.lot).map_or(Vec2::ZERO, |l| l.center);
            for (k, cid) in residents.iter().enumerate() {
                let Some(c) = self.citizens.get_mut(*cid) else { continue };
                c.mood = (c.mood - COMPLAINT_MOOD * (level + 0.5).min(2.0)).max(0.0);
                // One complaint per household-ish a day, not one per person.
                if k % 2 == 0 {
                    c.remember(tick, format!("The bins at {name} are overflowing again. The whole block stinks."));
                    self.concerns.heard_at(*cid, c, crate::concerns::Concern::Trash, 0.3 + 0.1 * level.min(3.0), Some((*bid, at)));
                    complaints += 1;
                }
            }
            let (cx, cy) = smell_cell(at);
            let mut near: Vec<usize> = Vec::new();
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if let Some(v) = home_cells.get(&(cx + dx, cy + dy)) {
                        near.extend_from_slice(v);
                    }
                }
            }
            // The order a scan of every home would visit them in.
            near.sort_unstable();
            for (hb, hp) in near.into_iter().map(|k| &homes[k]) {
                if hb == bid || hp.distance(at) > SMELL_RADIUS {
                    continue;
                }
                for cid in self.buildings[*hb].residents.clone() {
                    if let Some(c) = self.citizens.get_mut(cid) {
                        c.mood = (c.mood - SMELL_MOOD).max(0.0);
                    }
                }
            }
        }
        // The private hauler bills its clients; those who cannot pay drop out.
        // [economy] A local company bills its own, at its fee, as takings.
        let mut revenue = 0.0;
        let clients: Vec<(BuildingId, Option<BuildingId>, f32)> = self.society.trash.bins.iter().filter(|(_, b)| b.private).map(|(id, b)| (*id, b.company, if b.fee > 0.0 { b.fee } else { PRIVATE_FEE_HOUSEHOLD })).collect();
        for (bid, company, fee_hh) in clients {
            let Some(b) = self.buildings.get(bid) else { continue };
            let hhs: Vec<HouseholdId> = b.households.clone();
            let jobs = b.workers.len() as f32;
            let mut paid = 0.0;
            for h in hhs {
                if let Some(hh) = self.households.get_mut(h) {
                    if hh.money > fee_hh * 10.0 {
                        hh.money -= fee_hh;
                        paid += fee_hh;
                    }
                }
            }
            let job_fee = PRIVATE_FEE_JOB * jobs * fee_hh / PRIVATE_FEE_HOUSEHOLD;
            if let Some(z) = self.buildings.get_mut(bid).and_then(|b| b.business.as_mut()) {
                if z.money > job_fee {
                    z.money -= job_fee;
                    paid += job_fee;
                }
            }
            match company.and_then(|c| self.buildings.get_mut(c)).and_then(|b| b.business.as_mut()) {
                Some(z) => {
                    z.money += paid;
                    z.revenue_today += paid;
                }
                None => revenue += paid,
            }
        }
        let s = &mut self.society.trash.stats;
        s.private_revenue_today += revenue;
        s.private_revenue_total += revenue;
        s.complaints_today += complaints;
        s.produced_yesterday = s.produced_today;
        s.collected_city_yesterday = s.collected_city_today;
        s.collected_private_yesterday = s.collected_private_today;
        s.private_revenue_yesterday = s.private_revenue_today;
        s.complaints_yesterday = s.complaints_today;
        s.produced_today = 0.0;
        s.collected_city_today = 0.0;
        s.collected_private_today = 0.0;
        s.private_revenue_today = 0.0;
        s.complaints_today = 0;
        s.stops_today = 0;
        for t in self.society.trash.trucks.values_mut() {
            t.stops_today = 0;
        }
    }

    pub(crate) fn splat_trash(&self, b: &mut LandValueBuilder) {
        for (bid, bin) in &self.society.trash.bins {
            let o = bin.overflow();
            if o <= 0.0 {
                continue;
            }
            if let Some(l) = self.building_lot(*bid) {
                b.splat(l.center, TRASH_LV_RADIUS, -TRASH_LV * (0.5 + o).min(2.0));
            }
        }
    }
}
