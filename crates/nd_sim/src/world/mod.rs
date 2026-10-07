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
//! - `cars`: private cars as things with a place (parked or driving),
//!   parking and the search for a space, buying and selling, trucks and
//!   vans going back to base
//! - `parking`: parking businesses
//! - `buses`: depots, routes, stops and the bus loop
//! - `trains`: stations, yards, rail links and trains
//! - `subway`: metro stations, lines, trains and riders
//! - `power`: plants and power allocation
//! - `services`: police, fire, health, schools, dispatch, attractions,
//!   emergencies and the vehicles that answer them
//! - `industry`: industry sub-zones and the player's bias, the ground
//!   industry fouls, logistics fees, truck and bus lots
//! - `water`: the landscape hooks: road heights, bridges, the built ground,
//!   the highway and its through traffic, ports, ferries and ships
//! - `tests`: the world-level tests

mod air;
/// [benches] Seats on park benches.
pub mod benches;
mod buses;
mod cargo_hubs;
#[cfg(test)]
mod cargo_hub_tests;
mod cars;
mod chases;
mod enforcement;
mod construction;
mod history;
mod develop;
mod districts;
#[cfg(test)]
mod districts_tests;
mod driving;
mod industry;
mod market;
mod parking;
mod pedestrians;
mod plan;
mod pillars;
pub use pillars::deck_half_width;
mod junctions;
pub use junctions::{ApproachView, CrossingView, IntersectionView, PhaseView};
mod riders;
mod places;
#[cfg(test)]
mod places_tests;
mod industry_bridge;
pub use plan::{ProjectInfo, ProjectPreview};
mod power;
pub use power::{GridAsset, PlantRow};
mod services;
mod society;
mod survey;
pub use survey::SiteContext;
mod subway;
#[cfg(test)]
mod car_tests;
#[cfg(test)]
mod elevation_tests;
#[cfg(test)]
mod tools_tests;
#[cfg(test)]
mod ramp_tests;
#[cfg(test)]
mod parking_tests;
#[cfg(test)]
mod edge_tests;
#[cfg(test)]
mod concerns_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod gameplay_tests;
mod taxes;
pub use taxes::TaxRow;
mod tolls;
mod taxis;
#[cfg(test)]
mod taxi_tests;
mod trains;
mod tourism;
mod trips;
mod water;
mod ferry;
mod rides;

use crate::buildings::{Building, BuildingId, Construction, Tier};
use crate::citizens::{Activity, Citizen, CitizenId, Household, HouseholdId, Purpose};
use crate::clock::SimClock;
use crate::economy::{Economy, Flow, Item, GIG_INCOME, OVERDRAFT};
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
use crate::power::{demand_mw, Battery, BatteryKind, PowerPlantKind, Weather, GRID_IMPORT_PRICE};
use crate::traffic::{Bus, CarId};
use std::collections::{BTreeMap, HashMap};
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
pub use driving::{CrashRecord, Driving, Tow, Wreck, RIGHTING_MIN};
pub use industry::{is_return_trip, return_cargo, IndustryArea};
use crate::landscape::{Ground, Landscape, LandscapeParams};
use crate::ports::Ports;
pub use industry_bridge::INDUSTRY_CHOICES;
pub use districts::{DistrictStats, PolicyChange};

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
    /// Fires, crimes and illness roll every hour (`world/services.rs`).
    /// Tests about other systems turn it off so a random fire cannot take
    /// their building away.
    pub emergencies: bool,
    /// The landscape a new world is generated with. After a load this
    /// mirrors `World::landscape.params`.
    pub landscape: LandscapeParams,
    /// [society] Crime, homelessness and trash (`world/society.rs`). Tests
    /// about other systems may turn them off.
    pub society: bool,
    /// The old rule, for small test towns with no road off the map: every
    /// dead end counts as a way in from elsewhere, so people and lorries
    /// appear there. A real game never sets it: there, only the far ends
    /// of ways run out past the map edge do (`landscape::reach_elsewhere`).
    pub dead_ends_lead_elsewhere: bool,
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
            emergencies: true,
            landscape: LandscapeParams::default(),
            society: true,
            dead_ends_lead_elsewhere: false,
        };
        c.derive_speeds();
        c
    }
}

#[cfg(test)]
impl WorldConfig {
    /// The default, for a test town whose dead ends lead elsewhere.
    pub(crate) fn test_town() -> Self {
        Self { dead_ends_lead_elsewhere: true, ..Self::default() }
    }
}

/// A land value recomputation in progress, spread over a few ticks so one
/// tick does not carry the whole grid. Cloning a world mid-update drops the
/// partial work: the next cycle starts afresh and the grid merely stays a
/// little older for two hours. A save keeps it (REVIEW2 L7), so a game
/// saved mid-cycle finishes the same grid after loading.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
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
const LAND_VALUE_STRIPS: u64 = 6;

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
    /// Why the last implement was refused (the project's roads would take
    /// one of the city's own structures), cleared by the next that works.
    #[serde(skip)]
    pub implement_refused: Option<String>,
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
    /// City services, emergencies and their vehicles (`world/services.rs`).
    #[serde(default)]
    pub services: crate::services::Services,
    pub land_value: LandValue,
    #[serde(default)]
    land_value_work: LandValueWork,
    #[serde(skip)]
    pub events: Vec<SimEvent>,
    next_seed: u64,
    /// Connected component of each road node over street and path
    /// segments (index = node id), rebuilt with the roads.
    #[serde(skip)]
    road_component: Vec<u32>,
    /// Late mind answers (`MindProvider::poll`) for citizens who were
    /// travelling when they came, with the tick they came: acted on at the
    /// citizen's next decision (`step_citizens`) if still fresh. Not saved:
    /// an answer in flight is lost on load anyway.
    #[serde(skip)]
    deferred_intents: BTreeMap<CitizenId, (crate::mind::Intent, u64)>,
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
    /// Heights inside long streets and paths between their nodes, per
    /// segment id: `(t, z)` in rising `t`, empty for a straight line
    /// (`water::inner_heights`). Read through `road_height`.
    #[serde(skip)]
    pub road_inner: Vec<Vec<(f32, f32)>>,
    /// Anything stands off the flat datum: a landscape, a road above or
    /// below the ground, or a deck. False on the old flat map with every
    /// way at grade, where heights are all zero.
    #[serde(skip)]
    pub relief: bool,
    /// The ground as built, for drawing and for setting things on it.
    #[serde(skip)]
    pub ground: Ground,
    /// Metres of bridge already paid for.
    #[serde(default)]
    pub bridge_metres: f32,
    /// Pavement delivery robots and the day's home-delivery counts.
    #[serde(default)]
    pub bots: crate::bots::Bots,
    /// [driving] Crashes, wrecks, tow trucks and street takeovers.
    #[serde(default)]
    pub driving: Driving,
    /// [pedestrians] Walkers' state and the day's jaywalking, near misses
    /// and people hit (`world/pedestrians.rs`).
    #[serde(default)]
    pub peds: crate::pedestrians::Pedestrians,
    /// [perf] Who is inside each place, built at most once a tick for
    /// everyone choosing or rating a place (`world/places.rs`), instead of a
    /// scan of every citizen per decision. Arrivals later in the same tick
    /// show from the next one.
    #[serde(skip)]
    waiting_cache: Option<(u64, Vec<CitizenId>)>,
    #[serde(skip)]
    crowds_cache: Option<(u64, HashMap<BuildingId, Vec<CitizenId>>)>,
    /// [benches] Who sits (or is on the way to sit) on which park bench seat.
    #[serde(default)]
    pub seats: benches::Seats,
    /// [society] Crime, homelessness and trash (`world/society.rs`).
    #[serde(default)]
    pub society: crate::society::Society,
    /// [carlife] Every private car and where it is; trucks parked at
    /// their businesses; buying, selling and trip counts (`world/cars.rs`).
    #[serde(default)]
    pub vehicles: crate::vehicle::Vehicles,
    /// [parking] Parking officers on patrol, tow jobs, fines and the pound
    /// (`world/enforcement.rs`).
    #[serde(default)]
    pub enforcement: crate::enforcement::Enforcement,
    /// Drone taxis, air ambulances, police helicopters and their pads
    /// (`world/air.rs`).
    #[serde(default)]
    pub air: crate::air::Air,
    /// What industry has done to the ground (oil spills, ash): saved,
    /// spreads and fades daily. Old saves load clean.
    #[serde(default)]
    pub contamination: crate::ground::Contamination,
    /// The player's industry sub-zone bias over areas, newest last.
    #[serde(default)]
    pub industry_bias: Vec<crate::industry::IndustryBias>,
    /// [places] Venue, shop and park reputations (`world/places.rs`).
    #[serde(default)]
    pub places: crate::places::Places,
    /// [concerns] What people complain about, and city happiness, by day.
    #[serde(default)]
    pub concerns: crate::concerns::Concerns,
    /// [districts] Named districts painted over the map and their
    /// policies (`world/districts.rs`).
    #[serde(default)]
    pub districts: crate::districts::Districts,
    /// [tourism] Visitors, what draws them and the hotels they sleep in
    /// (`world/tourism.rs`).
    #[serde(default)]
    pub tourism: crate::tourism::Tourism,
    /// [subway] Metro lines and their trains (`world/subway.rs`).
    #[serde(default)]
    pub subway: crate::subway::Subway,
    /// [tolls] Toll booths on the roads (`world/tolls.rs`).
    #[serde(default)]
    pub tolls: crate::tolls::Tolls,
    /// [taxis] Taxi companies' cabs, riders waiting at the kerb and the
    /// demand nobody served (`world/taxis.rs`).
    #[serde(default)]
    pub taxis: crate::taxi::Taxis,
    /// Cargo hubs: what each holds and has coming, and the day's freight
    /// (`world/cargo_hubs.rs`).
    #[serde(default)]
    pub cargo_hubs: crate::cargo_hub::CargoHubs,
    /// [passengers] Stops on the way and the counts of people riding
    /// along in others' cars (`world/rides.rs`).
    #[serde(default)]
    pub rides: crate::rides::Rides,
    /// Street names, kept across rebuilds by matching the new chains to
    /// the old outlines (`streets.rs`). Saves from before names get them
    /// on load.
    #[serde(default)]
    pub streets: crate::streets::StreetNames,
    /// [tools] The columns under the raised roads (`world/pillars.rs`).
    #[serde(default)]
    pub pillars: Vec<crate::pillars::Pillar>,
    /// [tools] Stretches of raised road with no room for a column, from the
    /// last rebuild (the trial rebuild of a project refuses on them).
    #[serde(skip)]
    pub pillar_gaps: Vec<(Vec2, Vec2)>,
    /// [tools] Crosswalk upgrades from the Intersection editor (push-button
    /// signals, footbridges): `world/junctions.rs`. The junctions' own
    /// settings live with the traffic (`Traffic::junction_rules`).
    #[serde(default)]
    pub crosswalks: crate::junctions::Crosswalks,
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
            implement_refused: None,
            deferred_intents: BTreeMap::new(),
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
            services: crate::services::Services::default(),
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
            road_inner: Vec::new(),
            relief: false,
            ground: Ground::default(),
            bridge_metres: 0.0,
            bots: Default::default(),
            driving: Driving::default(),
            peds: Default::default(),
            seats: Default::default(),
            crowds_cache: None,
            waiting_cache: None,
            society: Default::default(),
            vehicles: Default::default(),
            enforcement: Default::default(),
            air: Default::default(),
            contamination: crate::ground::Contamination::new(half + 200.0, crate::ground::CONTAMINATION_CELL),
            industry_bias: Vec::new(),
            places: Default::default(),
            concerns: Default::default(),
            districts: Default::default(),
            tourism: Default::default(),
            subway: Default::default(),
            tolls: Default::default(),
            taxis: Default::default(),
            cargo_hubs: Default::default(),
            rides: Default::default(),
            streets: Default::default(),
            pillars: Vec::new(),
            pillar_gaps: Vec::new(),
            crosswalks: Default::default(),
        };
        w.landscape = Landscape::new(&w.config.landscape);
        w.plan_changed();
        // The trunk line: a railway already crosses the map south of the
        // centre, from elsewhere to elsewhere (edge to edge, run on out of
        // sight when built). Branches, stations and yards hang off it.
        let edge = crate::landscape::MAP_EDGE;
        w.add_rail(&[Vec2::new(-edge, TRUNK_Y), Vec2::new(edge, TRUNK_Y)]);
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
        self.walk.apply_upgrades(&self.crosswalks.rules, &self.roads);
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
        self.migrate_through_cars();
        // [people] Saves from before households: one per citizen. Detected
        // per citizen (no household), so no version bump is needed.
        self.migrate_households();
        self.version = SAVE_VERSION;
        // Ring pieces and slip roads are flagged from the network's shape;
        // a save from before the flags has them unset.
        self.roads.classify();
        self.rebuild_road_components();
        self.streets.relink(&self.roads, self.config.seed);
        self.rebuild_ground();
        self.traffic.rebuild(&self.roads);
        self.apply_tolls();
        // Park layouts are saved with their lots (the benches people sit on
        // must survive a load), so they are only planned again when a save
        // from before parks were serialised has park lots without one.
        if self.lots.values().any(|l| l.land_use == LandUse::Park && l.polygon.len() >= 3 && l.park.is_none()) {
            crate::park::plan_parks(&mut self.lots, &self.land_value);
        }
        self.walk = WalkGraph::build(&self.roads, &self.lots, &self.land_value, self.walk.version + 1);
        // [tools] Footbridges and push buttons on the crossings.
        self.walk.apply_upgrades(&self.crosswalks.rules, &self.roads);
        // [tools] Columns for raised roads from a save without them.
        self.pillars_after_load();
        for (_, car) in self.traffic.reattach_cars() {
            self.strand_car(car);
        }
        self.vehicles_after_load();
        // [services] Which car is which unit's is not saved: without it the
        // first unit car to arrive after a load went unclaimed.
        self.services.by_car = self.services.units.iter().filter_map(|(id, u)| u.car.map(|c| (c, id))).collect();
        // [services] Docks retiered by the daily update before `is_special`
        // knew them (an older save): their jobs back.
        for b in self.buildings.values_mut() {
            if let Some(kind) = b.port.filter(|_| b.construction.is_none()) {
                b.jobs = kind.jobs();
            }
        }
        self.transit.refresh_legs(&self.traffic.net);
        let on_trains: Vec<CitizenId> = self.citizens.iter().filter(|(_, c)| matches!(c.activity, Activity::OnTrain { .. })).map(|(id, _)| id).collect();
        for id in on_trains {
            let pos = self.citizens[id].pos;
            self.citizens[id].transit = None;
            self.strand_driver(id, pos);
        }
        self.refresh_facility_cache();
        self.subway_after_load();
        self.seats_after_load();
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
                Some(TransitLeg::Air { from, to }) => buildings.contains_key(from) && buildings.contains_key(to),
                Some(TransitLeg::Subway { line, board, alight }) => self.subway.lines.get(line).is_some_and(|l| l.index_of(board).is_some() && l.index_of(alight).is_some()),
                Some(TransitLeg::Taxi) => true,
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
                Activity::Flying { drone } => !self.air.drones.contains_key(*drone),
                // Trains are not saved: `subway_after_load` sets riders down.
                Activity::OnMetro { .. } => c.transit.is_none(),
                _ => false,
            };
            if lost || target_gone {
                c.activity = Activity::Idle;
                c.inside = None;
                c.next_decision_tick = tick + 1;
            }
        }
        // Vehicles whose ends no longer exist are not deleted where they
        // stand: a lorry bound for a building that is gone drives on when
        // it gets there (`lorry_arrived` looks every building up), and a bus
        // whose line is gone lets its riders off (`validate` above already
        // set them to decide again) and heads for its depot when it reaches
        // the end of this leg. Riders who no longer exist get off.
        for car in self.traffic.cars.values_mut() {
            if let Some(bus) = car.bus.as_mut() {
                if !routes.contains_key(bus.route) {
                    bus.passengers.clear();
                    bus.to_depot = true;
                    bus.next_stop = usize::MAX;
                }
                bus.passengers.retain(|p| self.citizens.contains_key(*p));
            }
        }
    }

    /// [services] Saves from before `Errand::Through` marked through traffic
    /// only by having no driver, owner, cargo or bus, which also matched
    /// service units, tow and garbage trucks and takeover outsiders. Those
    /// tracked by their own system are left as they are; the rest are
    /// through cars.
    fn migrate_through_cars(&mut self) {
        use crate::vehicle::Errand;
        let mut tracked: std::collections::HashSet<CarId> = self.services.units.values().filter_map(|u| u.car).collect();
        tracked.extend(self.society.trash.trucks.values().filter_map(|t| t.car));
        tracked.extend(self.driving.tows.iter().map(|t| t.truck));
        let citizens = &self.citizens;
        for (id, car) in self.traffic.cars.iter_mut() {
            let legacy = car.errand == Errand::Trip && !citizens.contains_key(car.citizen) && car.cargo.is_none() && car.bus.is_none() && car.owned.is_none() && !car.siren && car.drive.gang.is_none();
            if legacy && !tracked.contains(&id) {
                car.errand = Errand::Through;
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
            .filter(|b| b.lot != lot_id && b.land_use != LandUse::Park && !b.is_parking() && !b.is_power() && !b.depot && b.rail.is_none() && b.service.is_none())
            .filter_map(|b| self.lots.get(b.lot).map(|l| (b.floors as u32, l.center)))
            .filter(|(_, c)| c.distance(center) <= DENSITY_RADIUS)
            .map(|(f, _)| f)
            .sum();
        let density = near_floors as f32 + lot.footfall_today as f32 / 40.0;
        let cap = (2.0 + density / 10.0).min(255.0) as u8;
        // [tools] Under a deck spanning over the lot: what fits beneath it.
        match self.deck_cap(lot_id) {
            Some(deck) => cap.min(deck),
            None => cap,
        }
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
            // The day's counters, read before anything below resets them.
            let record = self.open_day_record();
            // Services settle their upkeep before the books close.
            crate::prof!("daily_services", self.daily_services());
            // [subway] Upkeep before the books close.
            crate::prof!("daily_subway", self.daily_subway());
            // [society] crime, the street and the bins settle their day.
            self.daily_society();
            self.daily_update();
            self.daily_vehicles();
            self.industry_daily();
            self.daily_places();
            self.rides.stats.roll();
            // [benches] seats and bench-neighbour tallies
            self.daily_benches();
            // [districts] facelifts; [tourism] hotel nights and the books.
            crate::prof!("daily_districts", self.daily_districts());
            crate::prof!("daily_tourism", self.daily_tourism());
            crate::prof!("close_day_record", self.close_day_record(record));
        }
        // Periodic work is staggered so no tick carries more than one heavy
        // job: the hour starts at :00, sites advance at :30, land value runs
        // in strips from :20, development and immigration sit between.
        if tick % 60 == 0 {
            crate::prof!("hourly_economy", self.hourly_economy());
            crate::prof!("industry_hourly", self.industry_hourly());
            crate::prof!("record_hour", self.record_hour());
        }
        if tick % 60 == 30 {
            crate::prof!("step_construction", self.step_construction());
        }
        let lv = tick % 120;
        if (LAND_VALUE_PHASE..LAND_VALUE_PHASE + LAND_VALUE_STRIPS).contains(&lv) {
            crate::prof!("update_land_value_strip", self.update_land_value_strip((lv - LAND_VALUE_PHASE) as u8));
        }
        if tick % 10 == 7 {
            crate::prof!("update_economy", self.update_economy());
        }
        if (tick + 5) % self.config.development_interval == 0 {
            crate::prof!("develop", self.develop());
        }
        if (tick + 15) % self.config.immigration_interval == 0 {
            crate::prof!("immigrate", self.immigrate());
        }
        if tick % 120 == 90 {
            crate::prof!("job_search", self.job_search());
        }
        // Fires, crimes and illness roll hourly at :50, clear of the rest.
        if tick % 60 == 50 {
            crate::prof!("hourly_services", self.hourly_services());
        }
        // [society] Bins, camps and crime at :40.
        if tick % 60 == 40 {
            crate::prof!("hourly_society", self.hourly_society());
        }
        if tick % 60 == 45 {
            crate::prof!("hourly_vehicles", self.hourly_vehicles());
        }
        // [parking] Drivers worn out by the hunt and stuck in a queue leave
        // the car where it stands.
        if tick % 5 == 2 {
            self.hunters_give_up();
        }
        // Answers a mind worked out in the background (an LLM's) land
        // whenever they are ready. Someone mid-journey (driving, on a bus or
        // train, waiting at the stop) keeps going: only the thought and the
        // memory are taken now, and the next decision asks again (REVIEW2
        // M4: acting on it stranded cars and bus riders).
        for (id, decision) in minds.poll() {
            if self.citizens.contains_key(id) {
                self.apply_late_decision(id, decision);
            }
        }
        // Late answers nobody came back for (they are still out, or gone).
        if tick % 60 == 0 {
            self.deferred_intents.retain(|_, (_, at)| tick.saturating_sub(*at) <= trips::DEFERRED_INTENT_TTL);
        }
        self.step_traffic();
        self.step_air();
        self.step_taxis();
        self.step_driving();
        self.step_enforcement();
        self.step_services();
        // [society] Trash trucks.
        self.step_society();
        self.step_through_traffic();
        self.step_transit();
        self.step_rail();
        self.step_subway();
        self.step_ships();
        // [ferries] Day trips out, and anyone stranded abroad.
        self.step_ferry_people();
        self.step_cargo_hubs();
        if self.clock.minute_of_day() == 0 {
            self.bots.delivered_today = 0;
            self.bots.van_orders_today = 0;
        }
        if tick % crate::bots::ROUND_TICKS == 11 {
            crate::prof!("dispatch_deliveries", self.dispatch_deliveries());
        }
        crate::prof!("step_bots", self.step_bots());
        // [tourism] Arrivals on the hour, visitors' days, departures.
        self.step_tourism();
        self.step_citizens(minds);
        // [passengers] With their drivers, in or out of the car.
        self.step_passengers();
        // [concerns] Happiness on the hour and at midnight.
        crate::prof!("step_concerns", self.step_concerns());
        // Every system that moves people has run: lay the breadcrumbs.
        for c in self.citizens.values_mut() {
            c.push_trail();
        }
    }

    /// A vehicle no system claimed reached the end of its route. Wreck tow
    /// trucks are home at their yard (or off the map), takeover outsiders
    /// and through cars from older saves are at their exit: they leave the
    /// world. Anything else is not deleted where it stands (that used to
    /// make vehicles vanish mid-street): it drives off the map
    /// (`retire_vehicle`).
    fn unclaimed_arrival(&mut self, car_id: CarId) {
        let Some(car) = self.traffic.cars.get(car_id) else { return };
        let gang = car.drive.gang.is_some();
        let tow = self.driving.tows.iter().find(|t| t.truck == car_id).map(|t| t.yard);
        let home = match tow {
            Some(Some(yard)) => self.buildings.contains_key(yard),
            Some(None) => true,
            None => false,
        };
        if home || gang || self.at_map_edge(car_id) {
            self.traffic.remove_car(car_id);
        } else {
            self.retire_vehicle(car_id);
        }
    }

    fn step_traffic(&mut self) {
        let arrived = self.traffic.step(self.config.traffic_seconds_per_tick, self.config.traffic_substeps);
        // [tolls] Before anyone arriving is taken off the road.
        self.settle_tolls();
        for car_id in arrived {
            // [emergency] A fugitive got where it was running: on it runs.
            if self.chase_car_arrived(car_id) {
                continue;
            }
            // Fire engines, police cars and ambulances: services handle them.
            if self.services.unit_of_car(car_id).is_some() {
                self.unit_car_arrived(car_id);
                continue;
            }
            if self.traffic.cars.get(car_id).is_some_and(|c| c.bus.is_some()) {
                self.bus_arrived(car_id);
                continue;
            }
            // [society] Garbage trucks.
            if self.society_car_arrived(car_id) {
                continue;
            }
            let Some(car) = self.traffic.cars.get(car_id) else { continue };
            // Lorries deliver and head back to a truck lot, their business
            // or out of town (`world/industry`).
            if let Some(_) = car.cargo {
                self.lorry_arrived(car_id);
                continue;
            }
            // [carlife] Private cars park (or go on looking), trucks and
            // vans head back to base, cars sold or emigrating leave.
            if self.vehicle_arrived(car_id) {
                continue;
            }
            self.unclaimed_arrival(car_id);
        }
        // Keep citizen positions in sync with their cars for inspection.
        for (cid, car) in self.traffic.cars.iter() {
            // [carlife] Only someone actually in it (not a driver who got
            // out while it was taken elsewhere).
            if let Some(c) = self.citizens.get_mut(car.citizen).filter(|c| matches!(c.activity, Activity::Driving { car, .. } if car == cid)) {
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
