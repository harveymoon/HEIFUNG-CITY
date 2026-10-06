//! Engine-independent simulation for Neon District.
//!
//! One tick is one simulated minute. The [`World`] owns every system and is
//! fully serialisable, so saves are plain snapshots and the same seed with the
//! same inputs reproduces the same city.

pub mod air;
pub mod bots;
pub mod buildings;
pub mod citizens;
pub mod clock;
/// [concerns] Complaints by category and city happiness over time.
pub mod concerns;
pub mod crime;
pub mod economy;
pub mod enforcement;
pub mod events;
pub mod geom;
pub mod homeless;
pub mod ground;
pub mod industry;
pub mod land_value;
pub mod landscape;
/// [looks] What citizens look like.
pub mod looks;
pub mod market;
pub mod mind;
pub mod names;
pub mod park;
pub mod pedestrians;
pub mod places;
pub mod plan;
pub mod ports;
pub mod power;
pub mod rail;
pub mod roads;
pub mod services;
pub mod takeover;
pub mod society;
/// [subway] Metro stations, lines and trains.
pub mod subway;
pub mod traffic;
pub mod transit;
pub mod trash;
pub mod vehicle;
pub mod walk;
pub mod world;
pub mod zoning;

pub use buildings::{Building, BuildingId, Construction, Demolition, Tier};
pub use land_value::LandValue;
pub use market::{Business, BusinessKind, Hours, MarketStats, Resource};
pub use citizens::{Activity, Citizen, CitizenId, Household, HouseholdId, LifeStage, Memory, Purpose};
pub use clock::SimClock;
pub use economy::Economy;
pub use events::SimEvent;
pub use mind::{Decision, Intent, MindContext, MindProvider};
pub use plan::{Gesture, GestureId, Plan, PlanPreview, Project};
pub use roads::{NodeId, RoadNetwork, Segment, SegmentId};
pub use traffic::{Car, CarId, Cargo, LaneId, Traffic};
pub use walk::WalkGraph;
pub use vehicle::{BodyType, CarEvent, CarPlace, Errand, Livery, OwnedCar, ParkKind, ParkSpot, VehicleClass, VehicleId, VehicleModel, Vehicles};
pub use bots::{BotId, Bots, DeliveryBot};
pub use power::{PlantSite, PowerPlantKind, PowerStats, Weather};
pub use landscape::{Landscape, LandscapeParams, LandscapePreset, WaterKind};
pub use ground::{Contamination, GroundSample, Soil};
pub use industry::{IndustryKind, IndustrySite};
pub use ports::{Port, PortId, PortKind, Ports, Ship, ShipKind};
pub use park::{ParkFeature, ParkLayout};
pub use subway::{MetroLine, MetroLineId, MetroTrain, MetroTrainId, Subway, SubwayStats};
pub use transit::{BusRoute, BusStop, RouteId, Transit, TransitStats};
pub use rail::{Facility, FacilityId, Rail, RailFacility, RailLink, RailStats, Train, TrainId, TrainKind};
pub use services::{BuildCategory, Incident, IncidentKind, Placeable, ServiceKind, ServiceSite, Services, ServiceStats, Unit, UnitId, UnitKind};
pub use world::{World, WorldConfig};
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
