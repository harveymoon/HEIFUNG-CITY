//! Engine-independent simulation for HEIFUNG://CITY.
//!
//! One tick is one simulated minute. The [`World`] owns every system and is
//! fully serialisable, so saves are plain snapshots and the same seed with the
//! same inputs reproduces the same city.

pub mod air;
/// [attractions] Landmark attractions: museums, the pier, the grand park,
/// the zoo, the observatory and the rest, one of each per city.
pub mod attractions;
pub mod bank;
pub mod bots;
pub mod buildings;
/// Cargo hubs: freight terminals that consolidate imports and exports.
pub mod cargo_hub;
pub mod citizens;
pub mod clock;
/// [committee] The mayor's AI committee: pool, seats, alerts, proposals.
pub mod committee;
/// [concerns] Complaints by category and city happiness over time.
pub mod concerns;
/// [megacorp] Mega-corporations, universities, brands and logos.
/// [companies] Local companies: builders, fleet liveries, corp specialties.
pub mod companies;
pub mod corp;
pub mod crime;
/// [districts] Named districts with specialisations and the Historic freeze.
pub mod districts;
pub mod stories;
/// [dams] (H1) Hydro dam walls the player draws.
pub mod dams;
/// [r4-leisure] Commercial sub-zones: neighbourhood, retail, offices, leisure.
pub mod commercial;
/// [K34] Shipping containers, box by box: numbers, wear, owners and lives.
pub mod containers;
pub mod economy;
pub mod enforcement;
/// [land] The land market: pressure, prices, owners, merged plots, the super-elite.
pub mod estate;
pub mod events;
pub mod geom;
/// Stable hashes (splitmix, FNV) for seeds, names and stream-free decisions.
pub mod hash;
pub mod history;
pub mod homeless;
/// [identity] The city's name and game id.
pub mod identity;
/// [insight] The city's memory: trips, re-plans, life and building events.
pub mod insight;
pub mod ground;
pub mod industry;
pub mod junctions;
pub mod land_value;
// [r4-leisure-places] places to eat and go out
pub mod leisure;
pub mod landscape;
pub mod level_crossing;
/// [looks] What citizens look like.
pub mod looks;
pub mod market;
pub mod mind;
pub mod names;
pub mod park;
/// [parks] (D5) Parks kept or let go, clean-ups, nature preserves.
pub mod parks;
pub mod parking_layout;
pub mod pedestrians;
pub mod pillars;
/// [r5-layered] Life under the raised roads: bays, markets, camps.
pub mod undercroft;
/// What the Buildings tool places: the catalogue, its categories and costs.
pub mod placeable;
pub mod places;
pub mod plan;
pub mod fishing;
pub mod ports;
pub mod power;
pub mod pressure;
/// Section timings for perf probes (`profile` feature only).
pub mod prof;
pub mod rail;
pub mod rides;
/// [events] Road wear, the maintenance budget and its bias.
pub mod road_wear;
/// [road-inspect] Hourly traffic and wear history per street piece.
pub mod road_traffic;
pub mod roads;
/// Random streams in saves: seed, stream and position.
pub(crate) mod rng_save;
/// Which old street piece a new one continues, through a road rebuild.
pub(crate) mod seg_match;
/// Ready-made cities (the demo district) for the app, tests and probes.
pub mod scenarios;
pub mod services;
/// [waterfront] Waterfront lots, public beaches and private jetties.
pub mod shore;
/// Street names: chains of segments named once and kept across rebuilds.
pub mod streets;
/// [events] Street races and warehouse raves.
pub mod street_events;
pub mod takeover;
/// [tax] Tax rates per land use and sub-type, and the import duty.
pub mod tax;
/// [tolls] Toll booths on the roads.
pub mod timetable;
pub mod tolls;
/// [supertall] The skyscraper league: skyscrapers, supertalls and megatalls.
pub mod towers;
/// [taxis] Taxi companies, their cabs and the riders who hail them.
pub mod taxi;
pub mod society;
/// [social] Ties between citizens, conversations, crews and clubs.
pub mod social;
pub mod survey;
/// [talk] The mayor's chats with citizens, saved with the city.
pub mod talk;
/// [tourism] Visitors, attractions and hotels.
pub mod tourism;
/// [subway] Metro stations, lines and trains.
pub mod subway;
pub mod traffic;
pub mod transit;
pub mod trash;
pub mod waste;
pub mod vehicle;
pub mod versioned;
pub mod walk;
/// [grid-weather] Weather by climate; the wind and sun on the map.
pub mod weather;
pub mod world;
pub mod zoning;

pub use buildings::{Building, BuildingId, Construction, Demolition, DemolitionReason, Occupant, SiteBlock, Tier};
pub use land_value::LandValue;
pub use market::{Business, BusinessKind, Hours, MarketStats, Resource};
pub use citizens::{Activity, Citizen, CitizenId, Household, HouseholdId, LifeStage, Memory, Purpose};
pub use clock::SimClock;
pub use identity::CityIdentity;
pub use economy::Economy;
pub use events::SimEvent;
pub use mind::{Decision, Intent, MindContext, MindProvider};
pub use plan::{Gesture, GestureId, Plan, PlanPreview, Project};
pub use roads::{NodeId, RoadNetwork, Segment, SegmentId};
pub use traffic::{Car, CarId, Cargo, LaneId, PlayerCar, Traffic};
pub use walk::{GroundsJunction, GroundsKind, WalkGraph};
pub use vehicle::{BodyType, CarEvent, CarPlace, Errand, Livery, OwnedCar, ParkKind, ParkSpot, VehicleClass, VehicleId, VehicleModel, Vehicles};
pub use bots::{BotId, Bots, DeliveryBot};
pub use power::{Battery, BatteryKind, PlantSite, PowerPlantKind, PowerStats, Weather};
pub use landscape::{Climate, Landscape, LandscapeParams, LandscapePreset, MapEdge, WaterKind};
pub use ground::{Contamination, GroundSample, Soil};
pub use industry::{IndustryKind, IndustrySite};
pub use ports::{Port, PortId, PortKind, Ports, Ship, ShipKind};
pub use park::{ParkFeature, ParkLayout};
pub use subway::{MetroLine, MetroLineId, MetroTrain, MetroTrainId, Subway, SubwayStats};
pub use transit::{BusRoute, BusStop, DispatchProblem, LineProblem, RouteId, Transit, TransitStats};
pub use rail::{Facility, FacilityId, Rail, RailFacility, RailLink, RailStats, Train, TrainId, TrainKind};
pub use services::{BuildCategory, Incident, IncidentKind, Placeable, ServiceKind, ServiceSite, Services, ServiceStats, Unit, UnitId, UnitKind};
pub use world::{GridAsset, PlacementSite, PlantGroup, PlantJoin, PlantPreview, PlantRow, World, WorldConfig, RIGHTING_MIN};
pub use world::{ApproachView, Blocker, Joint, CrossingView, IntersectionView, LaneView, MoveView, PhaseView, ProjectCheck};
/// [camera]
pub use world::{Gift, Holding, GIFT_AMOUNT};
pub use world::{TowerOrder, TowerSite};
pub use world::{DriveBlock, DriveGround, DRIVE_STEP};
pub use world::{LotFit, LotNeed};
pub use zoning::{LandUse, Lot, LotId, Parcel, ParcelKey, ParcelMap, Zone, ZoneId};

pub use glam::Vec2;

/// Deterministic random number generator used everywhere in the simulation.
pub type SimRng = rand_chacha::ChaCha8Rng;

/// Build a seeded RNG for a sub-system or an entity, so that generators stay
/// deterministic regardless of the order other systems consume randomness.
pub fn rng_for(seed: u64, salt: u64) -> SimRng {
    use rand::SeedableRng;
    SimRng::seed_from_u64(seed ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15))
}
