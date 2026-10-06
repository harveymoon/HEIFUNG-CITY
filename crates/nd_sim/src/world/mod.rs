//! The [`World`]: every system the simulation owns, in one serialisable
//! struct. This file holds the struct, its configuration, construction,
//! loading and validation, the tick schedule and the small read-only
//! queries. The systems are separate `impl World` blocks, one per file:
//!
//! - `plan`: projects, gestures, undo/redo, `rebuild_from_master`
//! - `develop`: economy snapshot, land value, daily update, development
//! - `market`: businesses, restocking, trucks, imports/exports, shopping
//! - `construction`: building sites
//! - `trips`: citizens' days, (im/e)migration, jobs, journeys, footfall
//! - `parking`: car parks and parking on arrival
//! - `buses`: depots, routes, stops and the bus loop
//! - `trains`: stations, yards, rail links and trains
//! - `power`: plants and power allocation
//! - `water`: the landscape hooks: road heights, bridges, the built ground,
//!   the highway and its through traffic, ports, ferries and ships
//! - `tests`: the world-level tests

mod buses;
mod construction;
mod develop;
mod market;
mod parking;
mod plan;
mod industry_bridge;
pub use plan::{ProjectInfo, ProjectPreview};
mod power;
#[cfg(test)]
mod tests;
mod trains;
mod trips;
mod water;

use crate::buildings::{Building, BuildingId, Construction, Tier};
use crate::citizens::{Activity, Citizen, CitizenId, Household, HouseholdId, Purpose};
use crate::clock::SimClock;
use crate::economy::{Economy, Flow, GIG_INCOME, OVERDRAFT};
use crate::events::SimEvent;
use crate::land_value::{LandValue, LandValueBuilder};
use crate::market::{Business, BusinessKind, Resource, EXPORT_DISCOUNT, HAULAGE, IMPORT_MARKUP};
use crate::traffic::Cargo;
use crate::mind::{Decision, Intent, MindContext, MindProvider};
use crate::names;
use crate::plan::{preview, Gesture, GestureId, Plan, PlanPreview, Project};
use crate::roads::{NodeId, RoadNetwork, SegmentId};
use crate::traffic::Traffic;
use crate::walk::WalkGraph;
use crate::power::{demand_mw, PowerPlantKind, Weather, GRID_IMPORT_MW, GRID_IMPORT_PRICE};
use crate::traffic::{Bus, CarId};
use std::collections::HashMap;
use crate::transit::{self, BusStop, RouteId, Transit, TransitLeg, TransitTrip};
use crate::rail::{self, Facility, FacilityId, Rail, RailFacility, RailLink, Train, TrainId, TrainKind};
use crate::zoning::{outlines_differ, point_in_polygon, polygons_overlap};
use crate::citizens::TravelMode;
use crate::roads::RoadKind;
use crate::zoning::{LandUse, Lot, LotId, Zone, ZoneId};
use crate::SimRng;
use glam::Vec2;
use rand::seq::IndexedRandom;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use slotmap::SlotMap;

pub use crate::rail::TRUNK_Y;
use crate::landscape::{Ground, Landscape, LandscapeParams};
use crate::ports::Ports;
pub use industry_bridge::INDUSTRY_CHOICES;

/// A parcel that loses or gains more than this share of its area under a
/// building is treated as a new parcel: the building goes.
pub const EXPROPRIATE_AREA_TOL: f32 = 0.12;
/// ... or whose outline moves more than this many metres anywhere.
pub const EXPROPRIATE_DIST_TOL: f32 = 3.0;
/// Days a low-tier building may stand on high-value land before it is
/// redeveloped at the land's tier.
pub const REDEVELOP_DAYS: u8 = 3;
/// Buildings within this distance count towards a lot's local density.
pub const DENSITY_RADIUS: f32 = 150.0;

/// Walking and cycling speeds in metres per second; the per-tick values in
/// `WorldConfig` are derived from these through `traffic_seconds_per_tick`.
pub const WALK_MPS: f32 = 1.4;
pub const BIKE_MPS: f32 = 4.5;
/// Save format version written into every snapshot. Bump it when a load
/// needs migration; `World::after_load` upgrades older saves.
pub const SAVE_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldConfig {
    pub seed: u64,
    /// Half-size of the buildable square, in metres.
    pub half_extent: f32,
    /// Metres walked per tick. Derived: `WALK_MPS` seconds of walking per
    /// tick, see `metres_per_tick`; recomputed on load, never set by hand.
    pub walk_speed: f32,
    /// Metres cycled per tick, derived like `walk_speed`.
    pub bike_speed: f32,
    /// Trips at least this long are cycled when the citizen owns a bike.
    pub bike_threshold: f32,
    /// Trips at least this long (straight line) are driven, shorter ones walked.
    pub drive_threshold: f32,
    /// Seconds of real motion simulated per tick, for everything that moves:
    /// cars, buses, trains, walkers and cyclists. A tick is a clock minute,
    /// so 60 would be real time. That makes a 500 m walk six ticks and every
    /// trip a blink; 1.5 (the old value) made the same walk four hours of
    /// clock, so people left for work and arrived at closing time and
    /// hunger was driven by walking. The compromise is 4: a 300 m commute
    /// takes a quarter of an hour of clock, a kilometre across town by bus
    /// well under an hour, and vehicles look about four times real speed at
    /// 1x, which reads as a city builder rather than a stopwatch.
    pub traffic_seconds_per_tick: f32,
    /// Physics substeps per tick; keeps each car step near half a second.
    pub traffic_substeps: usize,
    pub immigration_interval: u64,
    pub development_interval: u64,
    /// Upper bound on buildings started per development interval.
    pub max_builds_per_interval: usize,
    /// The landscape a new world is generated with. After a load this
    /// mirrors `World::landscape.params`.
    pub landscape: LandscapeParams,
}

impl WorldConfig {
    /// Metres something moving at `mps` covers in one tick.
    pub fn metres_per_tick(&self, mps: f32) -> f32 {
        mps * self.traffic_seconds_per_tick
    }

    /// Recompute the speeds that follow from `traffic_seconds_per_tick`.
    pub fn derive_speeds(&mut self) {
        self.walk_speed = self.metres_per_tick(WALK_MPS);
        self.bike_speed = self.metres_per_tick(BIKE_MPS);
    }
}

impl Default for WorldConfig {
    fn default() -> Self {
        let mut c = Self {
            seed: 7,
            half_extent: 900.0,
            walk_speed: 0.0,
            bike_speed: 0.0,
            bike_threshold: 40.0,
            drive_threshold: 60.0,
            traffic_seconds_per_tick: 4.0,
            traffic_substeps: 8,
            immigration_interval: 30,
            development_interval: 20,
            max_builds_per_interval: 2,
            landscape: LandscapeParams::default(),
        };
        c.derive_speeds();
        c
    }
}

/// A land value recomputation in progress, spread over a few ticks so one
/// tick does not carry the whole grid. Cloning a world mid-update drops the
/// partial work: the next cycle starts afresh and the grid merely stays a
/// little older for two hours.
#[derive(Default)]
struct LandValueWork {
    builder: Option<LandValueBuilder>,
}

impl Clone for LandValueWork {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for LandValueWork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LandValueWork({})", if self.builder.is_some() { "pending" } else { "idle" })
    }
}

/// Where in the two-hour cycle the land value strips run. They sit away
/// from the hourly tick (0) and the development ticks so no single tick
/// does everything.
const LAND_VALUE_PHASE: u64 = 20;
const LAND_VALUE_STRIPS: u64 = 5;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct World {
    /// `SAVE_VERSION` at the time of saving; older saves are migrated on load.
    #[serde(default)]
    pub version: u32,
    pub config: WorldConfig,
    pub clock: SimClock,
    #[serde(with = "rng_serde")]
    rng: SimRng,
    /// What is built.
    pub master: Plan,
    /// The active project (pending changes being edited), if any.
    pub project: Option<Project>,
    /// Other pending projects, previewed dimmed and implemented one at a
    /// time. Saves from before named projects have none.
    #[serde(default)]
    pub parked_projects: Vec<Project>,
    #[serde(default)]
    next_project: u64,
    /// Warnings from the last implement: gestures or parcels another
    /// pending project also changes.
    #[serde(skip)]
    pub plan_warnings: Vec<String>,
    /// Parcels along every street and path, zoned or not; derived from the
    /// roads and rebuilt with them.
    #[serde(skip)]
    pub parcels: crate::zoning::ParcelMap,
    /// Land use of each parcel under the effective plan (`planned_zoning`).
    #[serde(skip)]
    planned: Vec<Option<LandUse>>,
    /// Lots are cut from the parcels. False for a save from before that:
    /// its next rebuild moves buildings onto the new lots without eminent
    /// domain.
    #[serde(default)]
    parcel_lots: bool,
    next_gesture: u64,
    /// Bumped whenever master or project changes.
    pub plan_version: u64,
    /// The project applied to the master, for the `plan_version` it was
    /// built at; refreshed whenever the plan changes, so reads are free.
    #[serde(skip)]
    plan_cache: Option<(u64, Plan)>,
    pub roads: RoadNetwork,
    pub zones: SlotMap<ZoneId, Zone>,
    pub lots: SlotMap<LotId, Lot>,
    pub buildings: SlotMap<BuildingId, Building>,
    pub citizens: SlotMap<CitizenId, Citizen>,
    /// [people] Households: who lives together and shares money and a
    /// pantry. Saves before version 2 get one per citizen in `after_load`.
    #[serde(default)]
    pub households: SlotMap<HouseholdId, Household>,
    pub traffic: Traffic,
    /// Pedestrian network, derived from roads and lots.
    #[serde(skip)]
    pub walk: WalkGraph,
    #[serde(default)]
    pub weather: Weather,
    #[serde(default)]
    pub transit: Transit,
    #[serde(default)]
    pub rail: Rail,
    /// Stations and yards placed as pieces, with their own lots.
    #[serde(default)]
    pub facilities: SlotMap<FacilityId, Facility>,
    pub economy: Economy,
    pub land_value: LandValue,
    #[serde(skip)]
    land_value_work: LandValueWork,
    #[serde(skip)]
    pub events: Vec<SimEvent>,
    next_seed: u64,
    /// Connected component of each road node over street and path
    /// segments (index = node id), rebuilt with the roads.
    #[serde(skip)]
    road_component: Vec<u32>,
    /// Working stations and yards (connected and not derelict), refreshed
    /// when links, buildings or conditions change.
    #[serde(skip)]
    stations: Vec<BuildingId>,
    #[serde(skip)]
    yards: Vec<BuildingId>,
    /// Businesses selling each resource, with their lot centres, for the
    /// hour `seller_index_hour` was built in.
    #[serde(skip)]
    seller_index: HashMap<Resource, Vec<(BuildingId, Vec2)>>,
    #[serde(skip)]
    seller_index_hour: Option<u64>,
    /// Statistics read every tick but recomputed every ten.
    #[serde(skip)]
    available: [bool; 6],
    #[serde(skip)]
    groceries_for_sale: bool,
    #[serde(skip)]
    meals_for_sale: bool,
    /// Heightfield, rivers, lakes, sea and hot spots. Old saves load flat.
    #[serde(default)]
    pub landscape: Landscape,
    /// Shipping ports, ferry terminals and the ships between them.
    #[serde(default)]
    pub ports: Ports,
    /// Height of every road node (index = node id): the terrain under it,
    /// eased to gentle grades and lifted onto bridge decks over water.
    #[serde(skip)]
    pub road_z: Vec<f32>,
    /// The ground as built, for drawing and for setting things on it.
    #[serde(skip)]
    pub ground: Ground,
    /// Metres of bridge already paid for.
    #[serde(default)]
    pub bridge_metres: f32,
}

impl World {
    pub fn new(mut config: WorldConfig) -> Self {
        config.derive_speeds();
        let land_value = LandValue::new(config.half_extent + 200.0, 32.0);
        let mut economy = Economy::default();
        economy.treasury = 60_000.0;
        let half = config.half_extent;
        let mut w = Self {
            version: SAVE_VERSION,
            land_value,
            rng: SimRng::seed_from_u64(config.seed),
            config,
            clock: SimClock::starting_at(0, 7, 0),
            master: Plan::default(),
            project: None,
            parked_projects: Vec::new(),
            next_project: 1,
            plan_warnings: Vec::new(),
            parcels: crate::zoning::ParcelMap::default(),
            planned: Vec::new(),
            parcel_lots: true,
            next_gesture: 1,
            plan_version: 0,
            plan_cache: None,
            roads: RoadNetwork::default(),
            zones: SlotMap::with_key(),
            lots: SlotMap::with_key(),
            buildings: SlotMap::with_key(),
            citizens: SlotMap::with_key(),
            households: SlotMap::with_key(),
            traffic: Traffic::default(),
            walk: WalkGraph::default(),
            weather: Weather::default(),
            transit: Transit::default(),
            rail: Rail::default(),
            facilities: SlotMap::with_key(),
            economy,
            land_value_work: LandValueWork::default(),
            events: Vec::new(),
            next_seed: 1,
            road_component: Vec::new(),
            stations: Vec::new(),
            yards: Vec::new(),
            seller_index: HashMap::new(),
            seller_index_hour: None,
            available: [false; 6],
            groceries_for_sale: false,
            meals_for_sale: false,
            landscape: Landscape::default(),
            ports: Ports::default(),
            road_z: Vec::new(),
            ground: Ground::default(),
            bridge_metres: 0.0,
        };
        w.landscape = Landscape::new(&w.config.landscape);
        w.plan_changed();
        // The trunk line: a railway already crosses the map south of the
        // centre. Branches, stations and yards hang off it.
        w.add_rail(&[Vec2::new(-half - 100.0, TRUNK_Y), Vec2::new(half + 100.0, TRUNK_Y)]);
        if w.config.landscape.highway {
            w.lay_highway();
        }
        // The starting bridges come with the map.
        w.economy.treasury = 60_000.0;
        w.economy.ledger = Default::default();
        w
    }

    fn rebuild_walk(&mut self) {
        crate::park::plan_parks(&mut self.lots, &self.land_value);
        self.walk = WalkGraph::build(&self.roads, &self.lots, &self.land_value, self.walk.version + 1);
    }

    pub fn to_ron(&self) -> Result<String, ron::Error> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default().depth_limit(6))
    }

    pub fn from_ron(text: &str) -> Result<Self, ron::error::SpannedError> {
        let mut w: World = ron::from_str(text)?;
        w.after_load();
        Ok(w)
    }

    /// Rebuild derived state that is not serialised, and make a save from
    /// any earlier version safe to run.
    fn after_load(&mut self) {
        // Speeds follow the time scale, so a save from before a retune
        // walks at today's pace.
        self.config.derive_speeds();
        self.config.landscape = self.landscape.params.clone();
        self.validate();
        // [people] Saves from before households: one per citizen. Detected
        // per citizen (no household), so no version bump is needed.
        self.migrate_households();
        self.version = SAVE_VERSION;
        self.rebuild_road_components();
        self.rebuild_ground();
        self.traffic.rebuild(&self.roads);
        // Park layouts are saved with their lots (the benches people sit on
        // must survive a load), so they are only planned again when a save
        // from before parks were serialised has park lots without one.
        if self.lots.values().any(|l| l.land_use == LandUse::Park && l.polygon.len() >= 3 && l.park.is_none()) {
            crate::park::plan_parks(&mut self.lots, &self.land_value);
        }
        self.walk = WalkGraph::build(&self.roads, &self.lots, &self.land_value, self.walk.version + 1);
        for (_, car) in self.traffic.reattach_cars() {
            self.strand_car(car);
        }
        self.transit.refresh_legs(&self.traffic.net);
        let on_trains: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| matches!(c.activity, Activity::OnTrain { .. })).map(|(id, _)| id).collect();
        for id in on_trains {
            let pos = self.citizens[id].pos;
            self.citizens[id].transit = None;
            self.strand_driver(id, pos);
        }
        self.refresh_facility_cache();
        self.update_economy();
        // A save from before named projects: number and name the one it has.
        self.name_active_project();
        self.recut_parcels();
        self.plan_changed();
        self.events.push(SimEvent::PlanChanged);
    }

    /// Drop every reference to something that is not there: a hand-edited
    /// or truncated save must never index a missing building or citizen.
    fn validate(&mut self) {
        let tick = self.clock.tick;
        // Lots and buildings must agree about who stands where.
        for l in self.lots.values_mut() {
            if l.building.is_some_and(|b| !self.buildings.contains_key(b)) {
                l.building = None;
            }
        }
        let building_ids: std::collections::HashSet<BuildingId> = self.buildings.keys().collect();
        for b in self.buildings.values_mut() {
            b.residents.retain(|c| self.citizens.contains_key(*c));
            b.workers.retain(|c| self.citizens.contains_key(*c));
            b.rail_inbound.retain(|c| building_ids.contains(&c.to));
        }
        for f in self.facilities.values_mut() {
            if f.building.is_some_and(|b| !self.buildings.contains_key(b)) {
                f.building = None;
            }
            if f.lot.is_some_and(|l| !self.lots.contains_key(l)) {
                f.lot = None;
            }
        }
        // [people] Households and citizens must agree about who belongs where.
        for h in self.households.values_mut() {
            h.members.retain(|c| self.citizens.contains_key(*c));
            if h.home.is_some_and(|b| !self.buildings.contains_key(b)) {
                h.home = None;
            }
        }
        self.households.retain(|_, h| !h.members.is_empty());
        for b in self.buildings.values_mut() {
            b.households.retain(|h| self.households.contains_key(*h));
        }
        for c in self.citizens.values_mut() {
            if c.household.is_some_and(|h| !self.households.contains_key(h)) {
                c.household = None;
            }
        }
        let buildings = &self.buildings;
        let routes = &self.transit.routes;
        for c in self.citizens.values_mut() {
            let mut lost = false;
            for slot in [&mut c.home, &mut c.work, &mut c.inside, &mut c.parked_at] {
                if slot.is_some_and(|b| !buildings.contains_key(b)) {
                    *slot = None;
                    lost = true;
                }
            }
            let trip_ok = match c.transit.map(|t| t.leg) {
                None => true,
                Some(TransitLeg::Bus { route, board, alight }) => routes.get(route).is_some_and(|r| board < r.stops.len() && alight < r.stops.len()),
                Some(TransitLeg::Rail { board, alight }) => buildings.contains_key(board) && buildings.contains_key(alight),
            } && c.transit.is_none_or(|t| buildings.contains_key(t.target));
            if !trip_ok {
                c.transit = None;
                lost = true;
            }
            let target_gone = match &c.activity {
                Activity::Visiting(b) | Activity::Sitting { park: b, .. } => !buildings.contains_key(*b),
                Activity::Traveling { to: Some(b), .. } | Activity::Driving { to: Some(b), .. } => !buildings.contains_key(*b),
                Activity::AtHome => c.home.is_none(),
                Activity::Working => c.work.is_none(),
                Activity::Waiting { .. } | Activity::Riding { .. } | Activity::OnTrain { .. } => c.transit.is_none(),
                _ => false,
            };
            if lost || target_gone {
                c.activity = Activity::Idle;
                c.inside = None;
                c.next_decision_tick = tick + 1;
            }
        }
        // Vehicles whose ends no longer exist, and riders who do not.
        let cars: Vec<CarId> = self.traffic.cars.iter().filter(|(_, car)| car.cargo.is_some_and(|c| !buildings.contains_key(c.to)) || car.bus.as_ref().is_some_and(|b| !routes.contains_key(b.route))).map(|(id, _)| id).collect();
        for id in cars {
            self.traffic.remove_car(id);
        }
        for car in self.traffic.cars.values_mut() {
            if let Some(bus) = car.bus.as_mut() {
                bus.passengers.retain(|p| self.citizens.contains_key(*p));
            }
        }
    }

    fn fresh_seed(&mut self) -> u64 {
        self.next_seed = self.next_seed.wrapping_add(1);
        self.next_seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ self.config.seed
    }

    pub fn drain_events(&mut self) -> Vec<SimEvent> {
        std::mem::take(&mut self.events)
    }

    pub fn building_lot(&self, b: BuildingId) -> Option<&Lot> {
        self.buildings.get(b).and_then(|b| self.lots.get(b.lot))
    }

    pub fn building_entrance(&self, b: BuildingId) -> Option<Vec2> {
        self.building_lot(b).map(|l| l.entrance())
    }

    /// Is `p` inside a station's or yard's footprint? Nothing but the
    /// facility itself is ever built on that ground.
    pub fn under_facility(&self, p: Vec2) -> bool {
        self.facilities.values().any(|f| point_in_polygon(p, &f.footprint()))
    }

    /// How tall the surroundings of a lot justify building: the floors of
    /// the ordinary buildings within `DENSITY_RADIUS` plus the lot's footfall
    /// today. An empty field allows two floors; every ten floors nearby (or
    /// four hundred passers-by) allow one more, so towers only rise where
    /// the block is already busy.
    pub fn height_cap(&self, lot_id: LotId) -> u8 {
        let Some(lot) = self.lots.get(lot_id) else { return 3 };
        let center = lot.center;
        let near_floors: u32 = self
            .buildings
            .values()
            .filter(|b| b.lot != lot_id && b.land_use != LandUse::Park && !b.is_parking() && b.plant.is_none() && !b.depot && b.rail.is_none())
            .filter_map(|b| self.lots.get(b.lot).map(|l| (b.floors as u32, l.center)))
            .filter(|(_, c)| c.distance(center) <= DENSITY_RADIUS)
            .map(|(f, _)| f)
            .sum();
        let density = near_floors as f32 + lot.footfall_today as f32 / 40.0;
        (2.0 + density / 10.0).min(255.0) as u8
    }

    /// Floors for a new building on `lot_id`: the tier's range, held down
    /// by `height_cap` so the first houses in a field are low.
    fn pick_floors(&mut self, lot_id: LotId, lu: LandUse, tier: Tier) -> u8 {
        if lu == LandUse::Park {
            return 0;
        }
        let base: u8 = match lu {
            LandUse::Residential => self.rng.random_range(2..=7),
            LandUse::Commercial => self.rng.random_range(2..=8),
            LandUse::Industrial => self.rng.random_range(1..=3),
            LandUse::Entertainment => self.rng.random_range(2..=5),
            LandUse::Civic => self.rng.random_range(2..=5),
            LandUse::Park => 0,
        };
        let floors = match (lu, tier) {
            (LandUse::Industrial, _) => base,
            (_, Tier::High) => (base as f32 * 1.8) as u8 + 2,
            (_, Tier::Mid) => base + 1,
            (_, Tier::Low) => base.max(1),
        };
        floors.clamp(1, self.height_cap(lot_id).max(1))
    }

    fn available_land_uses(&self) -> [bool; 6] {
        let mut out = [false; 6];
        for b in self.buildings.values().filter(|b| b.construction.is_none()) {
            out[b.land_use.index()] = true;
        }
        out
    }

    pub fn tick(&mut self, minds: &mut dyn MindProvider) {
        self.clock.advance();
        let tick = self.clock.tick;
        if self.clock.minute_of_day() == 0 {
            self.events.push(SimEvent::NewDay(self.clock.day()));
            self.daily_update();
        }
        // Periodic work is staggered so no tick carries more than one heavy
        // job: the hour starts at :00, sites advance at :30, land value runs
        // in strips from :20, development and immigration sit between.
        if tick % 60 == 0 {
            self.hourly_economy();
        }
        if tick % 60 == 30 {
            self.step_construction();
        }
        let lv = tick % 120;
        if (LAND_VALUE_PHASE..LAND_VALUE_PHASE + LAND_VALUE_STRIPS).contains(&lv) {
            self.update_land_value_strip((lv - LAND_VALUE_PHASE) as u8);
        }
        if tick % 10 == 7 {
            self.update_economy();
        }
        if (tick + 5) % self.config.development_interval == 0 {
            self.develop();
        }
        if (tick + 15) % self.config.immigration_interval == 0 {
            self.immigrate();
        }
        if tick % 120 == 90 {
            self.job_search();
        }
        for (id, decision) in minds.poll() {
            if self.citizens.contains_key(id) {
                self.apply_decision(id, decision);
            }
        }
        self.step_traffic();
        self.step_through_traffic();
        self.step_transit();
        self.step_rail();
        self.step_ships();
        self.step_citizens(minds);
        // Every system that moves people has run: lay the breadcrumbs.
        for c in self.citizens.values_mut() {
            c.push_trail();
        }
    }

    fn step_traffic(&mut self) {
        let arrived = self.traffic.step(self.config.traffic_seconds_per_tick, self.config.traffic_substeps);
        for car_id in arrived {
            if self.traffic.cars.get(car_id).is_some_and(|c| c.bus.is_some()) {
                self.bus_arrived(car_id);
                continue;
            }
            let Some(car) = self.traffic.remove_car(car_id) else { continue };
            if let Some(cargo) = car.cargo {
                self.deliver(cargo);
                continue;
            }
            if let Some(c) = self.citizens.get_mut(car.citizen) {
                c.pos = car.pos;
                c.heading = car.heading;
                self.park_car(car.citizen);
                if !self.walk_last_leg(car.citizen) {
                    self.arrive(car.citizen);
                }
            }
        }
        // Keep citizen positions in sync with their cars for inspection.
        for car in self.traffic.cars.values() {
            if let Some(c) = self.citizens.get_mut(car.citizen) {
                c.pos = car.pos;
                c.heading = car.heading;
            }
            if let Some(bus) = &car.bus {
                for &p in &bus.passengers {
                    if let Some(c) = self.citizens.get_mut(p) {
                        c.pos = car.pos;
                        c.prev_pos = car.pos;
                        c.heading = car.heading;
                    }
                }
            }
        }
    }
}

/// RON cannot encode the RNG's 128-bit counter, so persist it as seed + position.
mod rng_serde {
    use crate::SimRng;
    use rand::SeedableRng;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct Saved {
        seed: [u8; 32],
        stream: u64,
        pos_hi: u64,
        pos_lo: u64,
    }

    pub fn serialize<S: Serializer>(rng: &SimRng, s: S) -> Result<S::Ok, S::Error> {
        let pos = rng.get_word_pos();
        Saved { seed: rng.get_seed(), stream: rng.get_stream(), pos_hi: (pos >> 64) as u64, pos_lo: pos as u64 }.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<SimRng, D::Error> {
        let saved = Saved::deserialize(d)?;
        let mut rng = SimRng::from_seed(saved.seed);
        rng.set_stream(saved.stream);
        rng.set_word_pos(((saved.pos_hi as u128) << 64) | saved.pos_lo as u128);
        Ok(rng)
    }
}
