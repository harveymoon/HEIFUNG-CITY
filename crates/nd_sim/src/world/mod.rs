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
/// [lines] Inserting and sharing bus stops.
pub mod bus_edit;
mod cargo_hubs;
/// [K34] Shipping containers' lives (`crate::containers`).
mod containers;
pub use containers::{truck_look, BoxCounts, BoxStory, DepotKind};
#[cfg(test)]
mod containers_tests;
/// [r5-trash] Where the trash goes (`crate::waste`).
#[cfg(test)]
mod waste_tests;
#[cfg(test)]
mod cargo_hub_tests;
#[cfg(test)]
mod island_lots_tests;
#[cfg(test)]
mod hillside_tests;
mod cars;
mod chases;
mod enforcement;
/// [camera] Gifts from the treasury; the places that are someone's.
pub mod gifts;
pub use gifts::{Gift, Holding, GIFT_AMOUNT};
mod construction;
/// [w1] Sites going up and coming down, for the player.
pub mod sites;
/// [land] Pressure, prices, owners, merged plots and the super-elite.
mod estate;
#[cfg(test)]
mod estate_tests;
#[cfg(test)]
mod growth_tests;
pub use estate::{escort_model, police_escort_model, supercar};
mod history;
/// [insight] Recording the city's memory (`crate::insight`).
mod insight;
pub use insight::{act_of, errand_of, ReplayPoint};
#[cfg(test)]
mod insight_tests;
mod memory;
mod develop;
mod districts;
// [stories] District stories' policies in the sim.
mod stories;
#[cfg(test)]
mod stories_tests;
#[cfg(test)]
mod districts_tests;
mod driving;
mod industry;
mod kinds;
pub mod land_tiles;
#[cfg(test)]
mod land_tiles_tests;
mod ledger;
mod save;
/// [saves] Compressed saves: reading either form.
pub use save::{is_compressed, save_text, GZIP_MAGIC};
mod tick;
mod streams;
use streams::Stream;
mod floors_grid;
// [interchange] The Road tool's Interchange preset: diamond interchanges.
pub mod interchange;
#[cfg(test)]
mod interchange_tests;
mod market;
pub mod delivery_hours;
pub mod games;
pub mod airlift;
pub mod watchdog;
mod parking;
mod pedestrians;
mod plan;
/// [free-placement] City buildings on any usable parcel; their ground out of the zoning.
mod placement;
pub mod over_roads;
#[cfg(test)]
mod over_roads_tests;
#[cfg(test)]
mod road_use_tests;
pub use placement::PlacementSite;
pub(crate) use placement::SiteRule;
#[cfg(test)]
mod placement_tests;
#[cfg(test)]
mod stable_parcels_tests;
#[cfg(test)]
mod backland_tests;
#[cfg(test)]
mod dock_access_tests;
pub mod poses;
/// [clarity] Origins and destinations, crews, crash sites.
pub mod journeys;
mod kerbside;
/// [rail-crossing] Level crossings: lights, barriers, who waits.
mod level_crossings;
#[cfg(test)]
mod level_crossing_tests;
#[cfg(test)]
mod r4_rail_tests;
#[cfg(test)]
mod walk_crossings_tests;
#[cfg(test)]
mod bridge_riders_tests;
#[cfg(test)]
mod site_access_tests;
#[cfg(test)]
mod ped_highway_tests;
mod pillars;
mod undercroft;
#[cfg(test)]
mod undercroft_tests;
pub use pillars::{deck_half_width, deck_half_width_use};
mod crossing_anchors;
#[cfg(test)]
mod crossing_anchor_tests;
pub use crossing_anchors::MIGRATE_REACH;
mod junctions;
pub use junctions::{ApproachView, CrossingView, IntersectionView, LaneView, MoveView, PhaseView};
mod riders;
mod places;
#[cfg(test)]
mod places_tests;
#[cfg(test)]
mod under_construction_tests;
mod industry_bridge;
mod commercial;
#[cfg(test)]
mod commercial_tests;
#[cfg(test)]
mod leisure_places_tests;
pub use plan::{insert_on_leg, leg_near, Blocker, Joint, ProjectCheck, ProjectInfo, ProjectPreview, JOINT_SAME};
mod power;
pub use power::{GridAsset, GridHour, PlantRow};
pub mod plant_groups;
pub use plant_groups::{PlantGroup, PlantJoin, PlantPreview};
#[cfg(test)]
mod plant_groups_tests;
#[cfg(test)]
mod plant_scale_tests;
mod scenes;
pub mod tow_moves;
#[cfg(test)]
mod blocker_tests;
mod services;
mod society;
/// [social] Encounters, conversations, friends and feuds, crews and clubs.
mod social;
#[cfg(test)]
mod social_tests;
mod survey;
pub use survey::SiteContext;
mod subway;
/// [w1] Metro lines edited stop by stop.
pub mod metro_edit;
#[cfg(test)]
mod car_tests;
#[cfg(test)]
mod elevation_tests;
#[cfg(test)]
mod tools_tests;
#[cfg(test)]
mod deck_ground_tests;
#[cfg(test)]
mod r1_roads_tests;
#[cfg(test)]
mod r4_crossings_tests;
#[cfg(test)]
mod ramp_tests;
#[cfg(test)]
mod ramp_draw_tests;
#[cfg(test)]
mod parking_tests;
#[cfg(test)]
mod edge_tests;
#[cfg(test)]
mod concerns_tests;
pub mod connectivity;
pub mod walk_networks;
#[cfg(test)]
mod connectivity_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod gameplay_tests;
mod taxes;
pub use taxes::TaxRow;
pub use towers::{TowerOrder, TowerSite};
mod tolls;
/// [supertall] The skyscraper league on the `World`.
mod towers;
#[cfg(test)]
mod towers_tests;
mod taxis;
#[cfg(test)]
mod taxi_tests;
mod trains;
// [pt-transit] The railway to and from out of town.
mod rail_travel;
// [pt-transit] Why people do (not) ride.
mod transit_report;
pub mod timetables;
#[cfg(test)]
mod timetable_tests;
#[cfg(test)]
mod transit_tests;
#[cfg(test)]
mod metro_loop_tests;
pub use transit_report::RidershipReport;
mod tourism;
mod trips;
mod water;
mod ferry;
/// [ferry-routes] Ferry routes and shipping lanes as lines on the water.
pub mod ferry_routes;
mod rides;
// [economy] business pressure, the new trades, the waterfront and the art scene
mod pressure;
mod waterfront;
/// [attractions] Landmark attractions: placement rules, grounds, visits.
mod attractions;
/// [dams] (H1) Hydro dam walls drawn bank to bank; no road on a dam.
mod dams;
/// [parks] (D5) Parks run down and cleaned up; nature preserves.
mod parks;
#[cfg(test)]
mod attractions_tests;
/// [waterfront] Waterfront lots, public beaches and private jetties.
mod shore;
#[cfg(test)]
mod shore_tests;
mod fishing;
#[cfg(test)]
mod fishing_tests;
// [events] street races and warehouse raves; road wear and maintenance
mod nights;
mod moto;
mod roadworks;
mod relocate;
// [tenants] several businesses in one building
mod tenants;
// [malls] Strip malls, malls and mega-malls.
pub mod malls;
// [land-assembly] Companies spreading over the land behind the road.
mod land_assembly;
#[cfg(test)]
mod land_malls_tests;
pub use tenants::{fits_shopfront, unit_layout};
#[cfg(test)]
mod tenants_tests;
mod rich;
/// [megacorp] Mega-corporations.
mod corp;
/// [companies] Local companies: builders, growth, fleet liveries, the
/// company directory.
mod companies;
pub use companies::{owner_kind_words, CompanySummary};
#[cfg(test)]
mod companies_tests;
/// [megacorp] Universities, students and schools near families.
mod campus;
#[cfg(test)]
mod corp_tests;
#[cfg(test)]
mod econ_followups_tests;
mod dirt;
pub use fishing::BoatReport;
pub use dirt::{CleanupJob, CleanupQuote, DirtyNeighbour, ADJACENCY_OUTPUT_HIT, ADJACENCY_VALUE_HIT, CLEANUP_COST, CLEANUP_MAX_RADIUS, CLEANUP_MIN_RADIUS};
// [road-inspect] the road inspector's traffic counts, live read-out and Repair now
mod road_inspect;
pub mod grounds;
pub use grounds::{PathLink, PathLinks};
#[cfg(test)]
mod grounds_tests;
#[cfg(test)]
mod events_tests;
#[cfg(test)]
mod economy_tests;
#[cfg(test)]
mod ledger_tests;

use crate::buildings::{Building, BuildingId, Construction, Tier};
use crate::citizens::{Activity, Citizen, CitizenId, Household, HouseholdId, Purpose};
use crate::clock::SimClock;
use crate::bank::{Account, Kind};
use crate::economy::{Economy, Flow, Item, GIG_INCOME, OVERDRAFT};
use crate::events::SimEvent;
use crate::land_value::{Channel, LandValue, LandValueBuilder};
use crate::market::{Business, BusinessKind, Resource, EXPORT_DISCOUNT, HAULAGE, IMPORT_MARKUP};
use crate::traffic::Cargo;
use crate::mind::{Decision, Intent, MindContext, MindProvider};
use crate::names;
use crate::plan::{preview, Gesture, GestureId, Plan, PlanPreview, Project};
use crate::roads::{NodeId, RoadNetwork, SegmentId};
use crate::traffic::Traffic;
use crate::walk::WalkGraph;
use crate::power::{Battery, BatteryKind, PowerPlantKind, Weather};
#[cfg(test)]
use crate::power::demand_mw;
use crate::traffic::{Bus, CarId};
use std::collections::{BTreeMap, HashMap};
use crate::transit::{self, BusStop, DispatchProblem, LineProblem, RouteId, Transit, TransitLeg, TransitTrip};
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
pub use poses::Pose;
pub use journeys::{Driver, Journey, Place};
pub use pressure::{artist_seed, is_artist, BusinessStory, LotFit, LotNeed};

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
    /// [social] People talking, friendships and feuds, groups
    /// (`world/social.rs`). Tests about other systems may turn it off.
    pub social: bool,
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
            social: true,
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
    #[serde(with = "crate::rng_save::one")]
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
    /// [stable-parcels] The parcels as last cut, and what from: a rebuild
    /// recuts only the land near roads that changed (`zoning::recut`).
    /// Saved as outlines; a save from before cuts them afresh on load.
    #[serde(default)]
    pub(crate) parcel_cut: crate::zoning::ParcelCut,
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
    /// [p1-sim] Wages each business paid its staff in today's household
    /// pass (`daily_household`), taken off its payroll in the market's
    /// daily books just after (`daily_businesses`). Empty between days.
    #[serde(skip)]
    pub(crate) wages_paid: std::collections::BTreeMap<BuildingId, f32>,
    pub roads: crate::versioned::Versioned<RoadNetwork>,
    pub zones: SlotMap<ZoneId, Zone>,
    pub lots: crate::versioned::Versioned<SlotMap<LotId, Lot>>,
    pub buildings: crate::versioned::Versioned<SlotMap<BuildingId, Building>>,
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
    /// [rail-crossing] Level crossings: their cycles and counts.
    #[serde(default)]
    pub level_crossings: crate::level_crossing::LevelCrossings,
    /// Stations and yards placed as pieces, with their own lots.
    #[serde(default)]
    pub facilities: SlotMap<FacilityId, Facility>,
    pub economy: Economy,
    /// City services, emergencies and their vehicles (`world/services.rs`).
    #[serde(default)]
    pub services: crate::services::Services,
    pub land_value: crate::versioned::Versioned<LandValue>,
    #[serde(default)]
    land_value_work: LandValueWork,
    #[serde(skip)]
    pub events: Vec<SimEvent>,
    next_seed: u64,
    /// Connected component of each road node over street and path
    /// segments (index = node id), rebuilt with the roads.
    #[serde(skip)]
    road_component: Vec<u32>,
    /// [sweep-sim] The same over street segments alone: where a car or a
    /// lorry can get (a footpath joins walkers, not drivers).
    #[serde(skip)]
    drive_component: Vec<u32>,
    /// [pt-walk] Stranded areas, failed trips and their alerts
    /// (`world/connectivity.rs`).
    #[serde(default)]
    pub connectivity: connectivity::Connectivity,
    /// [walk-networks] The walk graph's separate networks with ids kept
    /// across rebuilds (`world/walk_networks.rs`); derived, not saved.
    #[serde(skip)]
    pub walk_networks: walk_networks::WalkNetworks,
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
    /// [dock-access] Passenger stations on the railway whose street does not
    /// reach the city yet: trains call there, nobody gets on or off.
    #[serde(skip)]
    halts: Vec<BuildingId>,
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
    /// [r4-leisure-places] A gym is open in town (for the morning workout).
    #[serde(skip)]
    gyms_open: bool,
    /// Heightfield, rivers, lakes, sea and hot spots. Old saves load flat.
    #[serde(default)]
    pub landscape: crate::versioned::Versioned<Landscape>,
    /// Shipping ports, ferry terminals and the ships between them.
    #[serde(default)]
    pub ports: Ports,
    /// [fishing] The fishing ports' trawlers and the grounds they fish
    /// (`crate::fishing`, `world/fishing.rs`). Old saves start with every
    /// boat in port.
    #[serde(default)]
    pub fishing: crate::fishing::Fishing,
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
    pub ground: crate::versioned::Versioned<Ground>,
    /// Metres of bridge already paid for.
    #[serde(default)]
    pub bridge_metres: f32,
    /// Pavement delivery robots and the day's home-delivery counts.
    #[serde(default)]
    pub bots: crate::bots::Bots,
    /// [driving] Crashes, wrecks, tow trucks and street takeovers.
    #[serde(default)]
    pub driving: Driving,
    /// [clarity] Where trips came from, and the crash sites.
    #[serde(default)]
    pub journeys: journeys::Journeys,
    /// [pedestrians] Walkers' state and the day's jaywalking, near misses
    /// and people hit (`world/pedestrians.rs`).
    #[serde(default)]
    pub peds: crate::pedestrians::Pedestrians,
    /// [perf] Who is waiting at a station or stop, built at most once a
    /// tick (`world/trains.rs`).
    #[serde(skip)]
    waiting_cache: Option<(u64, Vec<CitizenId>)>,
    /// [perf] Who is inside each place, built at most once a tick for
    /// everyone choosing or rating a place (`world/places.rs`), instead of a
    /// scan of every citizen per decision. Arrivals later in the same tick
    /// show from the next one.
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
    /// [r4-tiles] The ground of the tiles round the map, surveyed once.
    #[serde(skip)]
    land_tile_grounds: std::sync::OnceLock<Vec<land_tiles::TileGround>>,
    /// [dirty-industry] Clean-up crews digging out poisoned ground, paid
    /// for up front (`world/dirt.rs`, `World::start_cleanup`).
    #[serde(default)]
    pub cleanups: Vec<dirt::CleanupJob>,
    /// The player's industry sub-zone bias over areas, newest last.
    #[serde(default)]
    pub industry_bias: Vec<crate::industry::IndustryBias>,
    /// [r4-leisure] The player's commercial sub-zone over areas, newest
    /// last (`world/commercial.rs`).
    #[serde(default)]
    pub commercial_bias: Vec<crate::commercial::CommercialBias>,
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
    pub pillar_gaps: Vec<crate::pillars::Gap>,
    /// [r5-layered] The bays of ground under the decks and what they are
    /// used for (`world/undercroft.rs`).
    #[serde(default)]
    pub undercroft: crate::undercroft::Undercroft,
    /// [tools] Crosswalk upgrades from the Intersection editor (push-button
    /// signals, footbridges): `world/junctions.rs`. The junctions' own
    /// settings live with the traffic (`Traffic::junction_rules`).
    #[serde(default)]
    pub crosswalks: crate::junctions::Crosswalks,
    /// [land] The land market: pressure, owners, merged plots, the
    /// super-elite (`crate::estate`, `world/estate.rs`). Old saves start
    /// with an empty one.
    #[serde(default)]
    pub estate: crate::estate::EstateMarket,
    /// [supertall] The skyscraper league: height limits, the tower
    /// developers' money, the record book and its news (`crate::towers`,
    /// `world/towers.rs`). Old saves load with no limit and no records.
    #[serde(default)]
    pub skyline: crate::towers::Skyline,
    /// [economy] Business pressure: the city's needs, read daily, and the
    /// trades opened to meet them (`crate::pressure`, `world/pressure.rs`).
    #[serde(default)]
    pub pressure: crate::pressure::PressureState,
    /// [economy] Waterfront boats, gallery openings and the art scene
    /// (`world/waterfront.rs`, `world/pressure.rs`).
    #[serde(default)]
    pub scene: crate::pressure::Scene,
    /// [identity] The city's name and its game's id. Saves from before
    /// load unnamed and without an id (the app gives them one when saved).
    #[serde(default)]
    pub identity: crate::identity::CityIdentity,
    /// [events] Street races and warehouse raves: tonight's, the record and
    /// the news (`world/nights.rs`).
    #[serde(default)]
    pub street_events: crate::street_events::StreetEvents,
    /// [events] Wear on every street piece and the maintenance budget
    /// (`world/roadworks.rs`). Old saves load with every road like new.
    #[serde(default)]
    pub road_wear: crate::road_wear::RoadCondition,
    /// [dirty-industry] The dirty plants and how hard each is working,
    /// refreshed every hour (`world/dirt.rs`). Saved, so a loaded city
    /// reads its neighbours exactly as the running one did; old saves fill
    /// it on load.
    #[serde(default)]
    pub dirty: Vec<crate::industry::DirtySource>,
    /// [road-inspect] Vehicles per street piece hour by hour, and each
    /// piece's wear history, for the road inspector (`world/road_inspect.rs`).
    #[serde(default)]
    pub road_log: crate::road_traffic::RoadLog,
    /// [social] Ties, conversations, groups and what they made
    /// (`crate::social`, `world/social.rs`). Old saves start with nobody
    /// knowing anybody.
    #[serde(default)]
    pub social: crate::social::Social,
    /// [social] The city's concern of the week, for small talk (hourly).
    /// [review] Saved: skipped, a loaded game talked of nothing else until
    /// the next :25. A save from before it reads it in `after_load`.
    #[serde(default)]
    social_concern: Option<crate::concerns::Concern>,
    /// [committee] The mayor's committee: the pool, the seats, alerts,
    /// meetings and their projects (`crate::committee`). The sim never
    /// reads it; old saves load with none and set one up when first used.
    #[serde(default)]
    pub committee: crate::committee::Committee,
    /// [waterfront] Waterfront parcels, public beaches and private jetties
    /// (`crate::shore`, `world/shore.rs`). Old saves load with none, and
    /// keep the parcels they were cut with.
    #[serde(default)]
    pub shore: crate::shore::ShoreState,
    /// [park-paths] Player paths drawn to a park's (or other grounds')
    /// walkways, and what they are joined to (`world/grounds.rs`). Old
    /// saves load with none.
    #[serde(default)]
    pub path_links: grounds::PathLinks,
    /// [megacorp] Mega-corporations and universities: brands, HQs, campus
    /// annexes, students and the player's rules (`crate::corp`,
    /// `world/corp.rs`, `world/campus.rs`). Old saves load with none.
    #[serde(default)]
    pub corps: crate::corp::Corporations,
    /// [delivery-hours] Lorries ordered but not yet on the road: deliveries
    /// go out in working hours, each at its own minute
    /// (`world/delivery_hours.rs`). Old saves load with none.
    #[serde(default)]
    pub freight_queue: Vec<delivery_hours::QueuedTruck>,
    /// [games] Stadium game days: away fans driving in and parked
    /// (`world/games.rs`). Old saves load with none.
    #[serde(default)]
    pub games: games::Games,
    /// [airlift] Helicopters lifting stuck vehicles off the road
    /// (`world/airlift.rs`). Old saves load with none.
    #[serde(default)]
    pub airlifts: Vec<airlift::Airlift>,
    /// [attractions] The landmark attractions' piers and books
    /// (`crate::attractions`, `world/attractions.rs`). Old saves load with none.
    #[serde(default)]
    pub attractions: crate::attractions::Attractions,
    /// [dams] (H1) Dam walls the player drew for hydro plants, and the
    /// plants waiting for theirs (`world/dams.rs`). Old saves load with none:
    /// their plants keep the automatic arch.
    #[serde(default)]
    pub dams: crate::dams::Dams,
    /// [parks] (D5) The parks: one state per park of lots side by side,
    /// litter, campers, dealers, clean-ups, nature preserves
    /// (`world/parks.rs`). Old saves start with every park kept.
    #[serde(default)]
    pub parks: crate::parks::Parks,
    /// [insight] The city's memory: vehicles' trips (where they went,
    /// re-plans and why, stalls, anger), citizens' trips and life events,
    /// what happened to each building (`crate::insight`,
    /// `world/insight.rs`). Bounded; saved as one compact string. Old saves
    /// load with none.
    #[serde(default)]
    pub insight: crate::insight::Insight,
    /// [K34] Every shipping container in town, one by one: numbers, lines,
    /// wear, where each is and has been, and the Stacks homes built from
    /// them (`crate::containers`, `world/containers.rs`). Old saves load
    /// with none and are given a starting stock on load.
    #[serde(default)]
    pub containers: crate::containers::Containers,
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
            land_value: land_value.into(),
            rng: SimRng::seed_from_u64(config.seed),
            config,
            clock: SimClock::starting_at(0, 7, 0),
            master: Plan::default(),
            project: None,
            parked_projects: Vec::new(),
            next_project: 1,
            plan_warnings: Vec::new(),
            parcels: crate::zoning::ParcelMap::default(),
            parcel_cut: Default::default(),
            planned: Vec::new(),
            parcel_lots: true,
            next_gesture: 1,
            plan_version: 0,
            plan_cache: None,
            wages_paid: Default::default(),
            implement_refused: None,
            deferred_intents: BTreeMap::new(),
            roads: Default::default(),
            zones: SlotMap::with_key(),
            lots: Default::default(),
            buildings: Default::default(),
            citizens: SlotMap::with_key(),
            households: SlotMap::with_key(),
            traffic: Traffic::default(),
            walk: WalkGraph::default(),
            weather: Weather::default(),
            transit: Transit::default(),
            rail: Rail::default(),
            level_crossings: Default::default(),
            facilities: SlotMap::with_key(),
            economy,
            services: crate::services::Services::default(),
            land_value_work: LandValueWork::default(),
            events: Vec::new(),
            next_seed: 1,
            road_component: Vec::new(),
            drive_component: Vec::new(),
            connectivity: Default::default(),
            walk_networks: Default::default(),
            stations: Vec::new(),
            yards: Vec::new(),
            halts: Vec::new(),
            seller_index: HashMap::new(),
            seller_index_hour: None,
            available: [false; 6],
            groceries_for_sale: false,
            meals_for_sale: false,
            gyms_open: false,
            landscape: Default::default(),
            ports: Ports::default(),
            fishing: Default::default(),
            road_z: Vec::new(),
            road_inner: Vec::new(),
            relief: false,
            ground: Default::default(),
            bridge_metres: 0.0,
            bots: Default::default(),
            driving: Driving::default(),
            journeys: Default::default(),
            peds: Default::default(),
            seats: Default::default(),
            crowds_cache: None,
            waiting_cache: None,
            society: Default::default(),
            vehicles: Default::default(),
            enforcement: Default::default(),
            air: Default::default(),
            contamination: crate::ground::Contamination::new(half + 200.0, crate::ground::CONTAMINATION_CELL),
            land_tile_grounds: Default::default(),
            cleanups: Vec::new(),
            industry_bias: Vec::new(),
            commercial_bias: Vec::new(),
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
            undercroft: Default::default(),
            crosswalks: Default::default(),
            estate: Default::default(),
            skyline: Default::default(),
            pressure: Default::default(),
            scene: Default::default(),
            identity: Default::default(),
            street_events: Default::default(),
            road_wear: Default::default(),
            dirty: Vec::new(),
            road_log: Default::default(),
            social: Default::default(),
            social_concern: None,
            committee: Default::default(),
            shore: crate::shore::ShoreState { reach_water: true, ..Default::default() },
            path_links: Default::default(),
            corps: Default::default(),
            freight_queue: Vec::new(),
            games: Default::default(),
            airlifts: Vec::new(),
            attractions: Default::default(),
            dams: Default::default(),
            parks: Default::default(),
            insight: Default::default(),
            // [K34] A new city keeps records from its first box.
            containers: crate::containers::Containers { live: true, ..Default::default() },
        };
        w.landscape.set(Landscape::new(&w.config.landscape));
        // [sweep-sim] Grids over the whole buildable land (far islands too).
        w.land_value.set(LandValue::new(w.grid_half(32.0), 32.0));
        w.estate.pressure = crate::estate::Pressure::new(w.grid_half(crate::estate::PRESSURE_CELL));
        // [grid-weather] The climate's ordinary weather to start.
        w.weather = Weather::for_climate(w.landscape.params.climate);
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
        // [ledger] The books open on the starting money.
        w.economy.bank = Default::default();
        w.open_ledger();
        w
    }

    fn rebuild_walk(&mut self) {
        // [park-paths] Paths just drawn to a park's gate are joined to it,
        // and the parks lay their walkways out to meet them.
        self.note_path_links();
        let links: Vec<Vec2> = self.path_links.links.iter().map(|l| l.pos).collect();
        crate::park::plan_parks_wild(&mut self.lots, &self.land_value, &links, &self.parks.preserves);
        // [attractions] The park clears every non-park lot's plan: the
        // landmarks people walk round lay theirs out again.
        self.plan_attraction_grounds();
        self.build_walk_graph();
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

    /// [review] Where a car stops for building `b`: its lot's street and
    /// the kerb point on it (None for a lot with no street to drive to).
    pub(super) fn car_stop(&self, b: BuildingId) -> Option<(SegmentId, Vec2)> {
        self.building_lot(b).and_then(|l| l.car_segment.map(|s| (s, l.car_point)))
    }

    /// Is `p` inside a station's or yard's footprint? Nothing but the
    /// facility itself is ever built on that ground.
    pub fn under_facility(&self, p: Vec2) -> bool {
        // [review] The corners as they are (`footprint()` copies them into
        // a new Vec per facility per call; this runs per lot in many loops).
        self.facilities.values().any(|f| point_in_polygon(p, &f.corners()))
    }

    /// The ground every station, yard and dock stands on, facilities first
    /// (in slot order), then docks.
    pub(super) fn structure_footprints(&self) -> Vec<Vec<Vec2>> {
        self.facilities.values().map(|f| f.footprint()).chain(self.ports.ports.values().map(|p| p.footprint())).collect()
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
            .filter(|b| b.lot != lot_id && b.counts_for_height_cap())
            .filter_map(|b| self.lots.get(b.lot).map(|l| (b.floors as u32, l.center)))
            .filter(|(_, c)| c.distance(center) <= DENSITY_RADIUS)
            .map(|(f, _)| f)
            .sum();
        let density = near_floors as f32 + lot.footfall_today as f32 / 40.0;
        let neighbours = (2.0 + density / 10.0).min(255.0) as u8;
        // [land] Pressure decides: quiet land stays low however much is
        // built round it, and a busy, well-served block may rise a few
        // floors over its neighbours (`estate::Pressure::floors_at`).
        let cap = if self.pressure_active() { self.pressure_floors(lot_id).min(neighbours.saturating_add(crate::estate::PRESSURE_OVER_NEIGHBOURS)).max(1) } else { neighbours };
        // [supertall] The city's or the district's height limit holds
        // ordinary buildings too.
        let cap = match self.height_limit_at(lot_id) {
            Some(m) => cap.min((m / crate::towers::FLOOR_M[lot.land_use.index()]).floor().clamp(1.0, 255.0) as u8),
            None => cap,
        };
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
            // [pt-growth] (F8) Civic buildings are halls, courts and
            // monuments: two to five storeys at any tier (the High
            // multiplier gave a ten-floor "Data Exchange" drawn as a low
            // monument, the city's "tallest" by its floors).
            (LandUse::Civic, _) => base,
            (_, Tier::High) => (base as f32 * 1.8) as u8 + 2,
            (_, Tier::Mid) => base + 1,
            (_, Tier::Low) => base.max(1),
        };
        // [pt-growth] On a busy street the town wants more of, developers
        // build towards what the land density allows: a shop on a crowded
        // high street opens taller than the tier alone would make it.
        let floors = if matches!(lu, LandUse::Residential | LandUse::Commercial | LandUse::Entertainment) && self.pressure_active() {
            let want = self.pressure_floors(lot_id).min(self.height_cap(lot_id));
            let pull = self.economy.demand_for(lu).clamp(0.0, 1.0);
            floors.max((want as f32 * 0.6 * pull).round() as u8)
        } else {
            floors
        };
        let floors = floors.clamp(1, self.height_cap(lot_id).max(1));
        // [land] Cheap homes on quiet land: one storey (a row of shacks, a
        // trailer park, huts) housing as many as the walk-up it replaces.
        if lu == LandUse::Residential && tier == Tier::Low && floors <= 2 && self.pressure_active() && self.pressure_floors(lot_id) <= 2 {
            return 1;
        }
        floors
    }

    fn available_land_uses(&self) -> [bool; 6] {
        let mut out = [false; 6];
        for b in self.buildings.values().filter(|b| b.operating()) {
            out[b.land_use.index()] = true;
        }
        out
    }
}

/// [review] A save in the oldest format still met (every field added
/// since left out, as in `saves/flags/flag_patch.ron`, which is gitignored
/// and 40 MB): a small town two days in, cut down by field path to what
/// that save has. The old-save tests read it; they used to read the
/// gitignored file and pass "skipped" when it was missing.
#[cfg(test)]
pub(crate) fn legacy_save_text() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy_town.ron");
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("the committed legacy fixture {} is missing: {e}", path.display()))
}

