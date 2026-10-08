//! What a vehicle looks like, as the sim sees it: a small description chosen
//! when the vehicle is bought or dispatched (class, body, wear, tuning,
//! livery, paint). The sim never reads it for physics; the renderer turns it
//! into a mesh (`nd_procgen::vehicles`), and the report counts it.
//!
//! Who drives what: the poor run old beat-up cars and scooters, the middle
//! sensible hatchbacks, saloons and SUVs (the young often a motorcycle), the
//! rich sleek coupes, SUVs and cyber-trucks; young, rich hedonists soup
//! theirs up (underglow, spoilers, wide wheels) or ride an Akira bike.
//! Trucks follow their cargo and the seller's trade; shops' delivery vans
//! wear the shop's livery; buses are bubble pods (some articulated).

use crate::buildings::Tier;
use crate::market::{BusinessKind, Resource};
use crate::SimRng;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

/// Price band of a vehicle; also how sleek or boxy it is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VehicleClass {
    /// Old and boxy: rust, mismatched panels, dents.
    Cheap,
    #[default]
    Mid,
    /// Sleek and glossy.
    Expensive,
}

/// Silhouette family the generator draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BodyType {
    Hatchback,
    #[default]
    Sedan,
    Coupe,
    Suv,
    Pickup,
    /// Faceted stainless wedge.
    CyberTruck,
    BoxVan,
    DeliveryVan,
    PanelTruck,
    /// Tractor unit with a container trailer.
    Semi,
    GarbageTruck,
    /// Rounded driverless pod bus with a glass band and a lidar puck.
    BubbleBus,
    ArticulatedBus,
    Motorcycle,
    /// Long, low, red, with light trails at night.
    AkiraBike,
    Scooter,
    /// Six-wheeled pavement robot with a flag.
    DeliveryBot,
    /// Four ducted-fan eVTOL drone taxi (`crate::air`).
    AirTaxi,
    /// Air ambulance or police helicopter; the main rotor spins apart.
    Helicopter,
}

impl BodyType {
    pub const ALL: [BodyType; 19] = [
        BodyType::Hatchback,
        BodyType::Sedan,
        BodyType::Coupe,
        BodyType::Suv,
        BodyType::Pickup,
        BodyType::CyberTruck,
        BodyType::BoxVan,
        BodyType::DeliveryVan,
        BodyType::PanelTruck,
        BodyType::Semi,
        BodyType::GarbageTruck,
        BodyType::BubbleBus,
        BodyType::ArticulatedBus,
        BodyType::Motorcycle,
        BodyType::AkiraBike,
        BodyType::Scooter,
        BodyType::DeliveryBot,
        BodyType::AirTaxi,
        BodyType::Helicopter,
    ];

    pub fn two_wheeler(self) -> bool {
        matches!(self, BodyType::Motorcycle | BodyType::AkiraBike | BodyType::Scooter)
    }

    pub fn is_bus(self) -> bool {
        matches!(self, BodyType::BubbleBus | BodyType::ArticulatedBus)
    }

    pub fn flies(self) -> bool {
        matches!(self, BodyType::AirTaxi | BodyType::Helicopter)
    }

    pub fn is_truck(self) -> bool {
        matches!(self, BodyType::BoxVan | BodyType::DeliveryVan | BodyType::PanelTruck | BodyType::Semi | BodyType::GarbageTruck)
    }

    pub fn label(self) -> &'static str {
        match self {
            BodyType::Hatchback => "hatchback",
            BodyType::Sedan => "sedan",
            BodyType::Coupe => "coupe",
            BodyType::Suv => "suv",
            BodyType::Pickup => "pickup",
            BodyType::CyberTruck => "cyber_truck",
            BodyType::BoxVan => "box_van",
            BodyType::DeliveryVan => "delivery_van",
            BodyType::PanelTruck => "panel_truck",
            BodyType::Semi => "semi",
            BodyType::GarbageTruck => "garbage_truck",
            BodyType::BubbleBus => "bubble_bus",
            BodyType::ArticulatedBus => "articulated_bus",
            BodyType::Motorcycle => "motorcycle",
            BodyType::AkiraBike => "akira_bike",
            BodyType::Scooter => "scooter",
            BodyType::DeliveryBot => "delivery_bot",
            BodyType::AirTaxi => "air_taxi",
            BodyType::Helicopter => "helicopter",
        }
    }
}

/// Fleet paint for business vehicles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Livery {
    #[default]
    None,
    /// Bakeries: hot food out of the oven.
    Pizza,
    /// Gadget shops and parcel carriers.
    Parcels,
    Grocery,
    Construction,
    /// Power, water and city services (garbage).
    Utility,
    /// The bus network.
    Transit,
    /// [taxis] A taxi company's cab: neon yellow, a checker band, a lit
    /// roof sign and underglow (`crate::taxi`).
    Taxi,
    /// [megacorp] A mega-corp's fleet: its brand colour (an index into
    /// `crate::corp::BRAND_COLOURS`) and its mark (the logo shape,
    /// `LogoShape as u8`, in the low three bits, the monogram's letter from
    /// A in the rest), lit on the side.
    Corp(u8, u8),
}

impl Livery {
    pub const ALL: [Livery; 9] = [Livery::None, Livery::Pizza, Livery::Parcels, Livery::Grocery, Livery::Construction, Livery::Utility, Livery::Transit, Livery::Taxi, Livery::Corp(6, 2)];

    /// The fleet a business sends its deliveries in.
    pub fn for_business(kind: BusinessKind) -> Livery {
        match kind {
            BusinessKind::Bakery => Livery::Pizza,
            BusinessKind::Grocery | BusinessKind::VertFarm | BusinessKind::Dairy | BusinessKind::ProteinVat => Livery::Grocery,
            BusinessKind::GadgetShop | BusinessKind::Assembly | BusinessKind::CargoHub => Livery::Parcels,
            BusinessKind::Fab | BusinessKind::Mill | BusinessKind::GrainFarm => Livery::Utility,
            _ => Livery::None,
        }
    }
}

/// How a vehicle looks. Old saves load every car as a plain mid sedan.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VehicleModel {
    #[serde(default)]
    pub class: VehicleClass,
    #[serde(default)]
    pub body: BodyType,
    /// Wear `0..=2`: clean, scuffed, beat-up (rust, dents, odd panels).
    #[serde(default)]
    pub wear: u8,
    /// Souped up: underglow, spoiler, wide wheels.
    #[serde(default)]
    pub tuned: bool,
    #[serde(default)]
    pub livery: Livery,
    /// Paint index into the vehicle palette (`0..PAINTS`).
    #[serde(default)]
    pub paint: u8,
}

/// Daily income from which `Tier::for_income` counts someone rich.
pub const HIGH_INCOME: f32 = 140.0;

/// Paint colours the generator knows.
pub const PAINTS: u8 = 8;

/// A generator for a model choice, kept off the world's random stream so
/// choosing looks never shifts what else the simulation draws.
pub fn model_rng(a: u64, b: u64) -> SimRng {
    crate::rng_for(a ^ 0x7e41_c1e5, b)
}

/// The hedonism trait of a citizen's personality, as `nd_minds::Personality`
/// derives it from the seed (the third draw).
pub fn hedonism_of(seed: u64) -> f32 {
    let mut rng = SimRng::seed_from_u64(seed);
    let _: f32 = rng.random();
    let _: f32 = rng.random();
    rng.random()
}

impl VehicleModel {
    pub fn new(class: VehicleClass, body: BodyType) -> Self {
        Self { class, body, wear: 0, tuned: false, livery: Livery::None, paint: 0 }
    }

    /// What it costs to buy (and roughly a half back when sold).
    pub fn price(&self) -> f32 {
        let base = match self.body {
            BodyType::Scooter => 60.0,
            BodyType::Motorcycle => 140.0,
            BodyType::AkiraBike => 900.0,
            BodyType::Hatchback => 260.0,
            BodyType::Sedan => 320.0,
            BodyType::Coupe => 600.0,
            BodyType::Suv => 450.0,
            BodyType::Pickup => 300.0,
            BodyType::CyberTruck => 1100.0,
            _ => 800.0,
        };
        let k = match self.class {
            VehicleClass::Cheap => 0.35,
            VehicleClass::Mid => 1.0,
            VehicleClass::Expensive => 2.2,
        };
        base * k * if self.tuned { 1.4 } else { 1.0 }
    }

    /// A private vehicle for a buyer with this daily income, age and
    /// hedonism. The poor get old beat-up cars and scooters; the rich sleek
    /// ones; young rich hedonists tune theirs or ride an Akira bike.
    pub fn for_owner(income: f32, age: u8, hedonism: f32, rng: &mut impl Rng) -> Self {
        let young = age < 32;
        let pick = |rng: &mut dyn rand::RngCore, opts: &[(BodyType, u32)]| -> BodyType {
            let total: u32 = opts.iter().map(|o| o.1).sum();
            let mut r = rng.random_range(0..total.max(1));
            for &(b, w) in opts {
                if r < w {
                    return b;
                }
                r -= w;
            }
            opts[0].0
        };
        let mut m = match Tier::for_income(income) {
            Tier::Low => {
                let body = pick(rng, &[(BodyType::Scooter, 35), (BodyType::Motorcycle, if young { 25 } else { 10 }), (BodyType::Hatchback, 25), (BodyType::Sedan, 20), (BodyType::Pickup, 15)]);
                let mut m = Self::new(VehicleClass::Cheap, body);
                m.wear = if rng.random_bool(0.7) { 2 } else { 1 };
                m
            }
            Tier::Mid => {
                let body = pick(
                    rng,
                    &[(BodyType::Hatchback, 25), (BodyType::Sedan, 30), (BodyType::Suv, 20), (BodyType::Pickup, 10), (BodyType::Motorcycle, if young { 25 } else { 5 }), (BodyType::Scooter, if young { 8 } else { 2 }), (BodyType::CyberTruck, 3)],
                );
                let mut m = Self::new(VehicleClass::Mid, body);
                m.wear = if rng.random_bool(0.3) { 1 } else { 0 };
                // A young hedonist on a middle wage scrapes together a bike.
                if young && hedonism > 0.8 && rng.random_bool(0.2) {
                    m.body = BodyType::AkiraBike;
                }
                m
            }
            Tier::High => {
                let body = pick(rng, &[(BodyType::Coupe, 30), (BodyType::Sedan, 25), (BodyType::Suv, 25), (BodyType::CyberTruck, 15), (BodyType::Motorcycle, if young { 8 } else { 2 })]);
                Self::new(VehicleClass::Expensive, body)
            }
        };
        // Young money with a taste for the night: souped up.
        if young && hedonism > 0.6 && m.class != VehicleClass::Cheap && income >= HIGH_INCOME && rng.random_bool(0.6) {
            if rng.random_bool(0.3) {
                m.body = BodyType::AkiraBike;
            } else {
                m.tuned = !m.body.two_wheeler();
                if m.body.two_wheeler() {
                    m.body = BodyType::AkiraBike;
                }
            }
        }
        if m.body == BodyType::AkiraBike {
            m.class = VehicleClass::Expensive;
            m.wear = 0;
        }
        m.paint = rng.random_range(0..PAINTS);
        m
    }

    /// The lorry a load travels in: by the goods, the seller's trade, and
    /// whether it comes from out of town or goes to a building site.
    pub fn for_cargo(resource: Resource, seller: Option<BusinessKind>, to_site: bool, long_haul: bool, rng: &mut impl Rng) -> Self {
        let mut m = if to_site {
            let mut m = Self::new(VehicleClass::Mid, if rng.random_bool(0.6) { BodyType::PanelTruck } else { BodyType::Pickup });
            m.livery = Livery::Construction;
            m
        } else if long_haul {
            Self::new(VehicleClass::Mid, BodyType::Semi)
        } else {
            let body = match resource {
                Resource::Groceries | Resource::BakedGoods | Resource::Goods => BodyType::DeliveryVan,
                Resource::Meat | Resource::Dairy => BodyType::BoxVan,
                Resource::Electronics => BodyType::BoxVan,
                Resource::Parts => BodyType::Semi,
                Resource::Produce | Resource::Grain | Resource::Flour => BodyType::PanelTruck,
                // [industry] tankers for oil, reefers for fish
                Resource::Crude | Resource::Fuel => BodyType::Semi,
                Resource::Plastics | Resource::Fish => BodyType::BoxVan,
                // [economy] cattle trucks, ore tippers, reefers and art vans
                Resource::Livestock | Resource::Ore => BodyType::Semi,
                // [dirty-industry] ingots on a flatbed semi
                Resource::Metal => BodyType::Semi,
                Resource::Fillets | Resource::Art => BodyType::BoxVan,
            };
            let mut m = Self::new(VehicleClass::Mid, body);
            m.livery = seller.map(Livery::for_business).unwrap_or(Livery::None);
            m
        };
        m.wear = rng.random_range(0..2);
        m.paint = rng.random_range(0..PAINTS);
        m
    }

    /// A shop's home-delivery vehicle in its livery.
    pub fn delivery(kind: BusinessKind, rng: &mut impl Rng) -> Self {
        let livery = Livery::for_business(kind);
        let body = match kind {
            BusinessKind::Bakery => {
                if rng.random_bool(0.6) {
                    BodyType::Scooter
                } else {
                    BodyType::Hatchback
                }
            }
            _ => BodyType::DeliveryVan,
        };
        Self { class: VehicleClass::Mid, body, wear: 0, tuned: false, livery, paint: rng.random_range(0..PAINTS) }
    }

    /// A bus for a line drawn in route colour `paint`.
    pub fn bus(paint: u8, rng: &mut impl Rng) -> Self {
        let body = if rng.random_bool(0.25) { BodyType::ArticulatedBus } else { BodyType::BubbleBus };
        Self { class: VehicleClass::Expensive, body, wear: 0, tuned: false, livery: Livery::Transit, paint: paint % PAINTS }
    }

    /// Through traffic crossing the map: anybody's car, a few lorries, the
    /// odd garbage truck.
    pub fn through(rng: &mut impl Rng) -> Self {
        let r: f32 = rng.random();
        let mut m = if r < 0.08 {
            Self::new(VehicleClass::Mid, BodyType::Semi)
        } else if r < 0.12 {
            let mut m = Self::new(VehicleClass::Mid, BodyType::GarbageTruck);
            m.livery = Livery::Utility;
            m
        } else if r < 0.17 {
            let mut m = Self::new(VehicleClass::Mid, BodyType::DeliveryVan);
            m.livery = Livery::Parcels;
            m
        } else {
            let income = rng.random_range(20.0..260.0);
            Self::for_owner(income, rng.random_range(18..80), rng.random(), rng)
        };
        m.paint = rng.random_range(0..PAINTS);
        m
    }

    /// The model a citizen's car falls back to when nobody chose one (old
    /// saves): derived from the seed so it never changes between trips.
    pub fn fallback(seed: u64, income: f32, age: u8) -> Self {
        let mut rng = SimRng::seed_from_u64(seed ^ 0x5eed_ca75);
        Self::for_owner(income, age, hedonism_of(seed), &mut rng)
    }
}

// ── ownership: every private car is a thing with a place ─────────────────

slotmap::new_key_type! {
    /// A privately owned car (or motorcycle, scooter) in the city.
    pub struct VehicleId;
}

/// What kind of space a parked car occupies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ParkKind {
    /// A space of the owner's home building.
    #[default]
    Home,
    /// A space of the building the driver went to (a tower's garage).
    Building,
    /// A paid space at a parking business.
    Lot,
    /// A kerb slot on a street, free.
    Kerb,
    /// Squeezed onto the street with no legal space left (counted as a
    /// shortage); it still has a place, just not a good one.
    Overflow,
    /// On a dealer's forecourt.
    Dealer,
    /// [parking] Hooked onto a tow truck, on its way to the pound (`pos`
    /// follows the truck).
    Hooked,
    /// [parking] In a tow company's pound (`building`; none: a company out
    /// of town), waiting for its owner to pay `fee` and fetch it.
    Pound,
}

impl ParkKind {
    pub fn label(self) -> &'static str {
        match self {
            ParkKind::Home => "home",
            ParkKind::Building => "building",
            ParkKind::Lot => "parking lot",
            ParkKind::Kerb => "kerb",
            ParkKind::Overflow => "street (illegally)",
            ParkKind::Dealer => "dealer",
            ParkKind::Hooked => "on a tow truck",
            ParkKind::Pound => "impounded",
        }
    }

    /// Occupies one of a building's `parking_capacity` spaces.
    pub fn uses_building(self) -> bool {
        matches!(self, ParkKind::Home | ParkKind::Building | ParkKind::Lot)
    }

    /// Stands on a street (drawn at the kerb).
    pub fn on_street(self) -> bool {
        matches!(self, ParkKind::Kerb | ParkKind::Overflow)
    }
}

/// Where a parked car stands. `pos`/`heading` are where it is drawn; a car
/// on a street also holds `slot` on `segment` (`side` 0 is the kerb to the
/// right of a→b, 1 the other kerb).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ParkSpot {
    pub kind: ParkKind,
    #[serde(default)]
    pub building: Option<crate::buildings::BuildingId>,
    #[serde(default)]
    pub segment: Option<crate::roads::SegmentId>,
    #[serde(default)]
    pub side: u8,
    #[serde(default)]
    pub slot: u16,
    pub pos: glam::Vec2,
    pub heading: f32,
    /// Tick it was parked.
    #[serde(default)]
    pub since: u64,
    /// [parking] Tick a parking ticket was left on it here (0: none).
    #[serde(default)]
    pub ticketed: u64,
    /// [parking] In the pound: the tow fee owed (storage is added daily).
    #[serde(default)]
    pub fee: f32,
    /// [boot] A wheel clamp is on: the car cannot be driven until the tow
    /// truck has taken it (the hook replaces the spot, clamp and all).
    #[serde(default)]
    pub booted: bool,
}

/// Where a private car is: parked somewhere, or on the road.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum CarPlace {
    Parked(ParkSpot),
    Driving(crate::traffic::CarId),
}

/// A private car: who owns it, what it is, and where it is right now.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OwnedCar {
    pub owner: crate::citizens::CitizenId,
    pub model: VehicleModel,
    pub place: CarPlace,
    #[serde(default)]
    pub color: u8,
    /// Tick it came into the city (arrived with its household or bought).
    #[serde(default)]
    pub since: u64,
    /// [rich] A car kept in the owner's garage beside the one they drive
    /// (`Citizen::car`): parked in their house's own spaces, never driven.
    #[serde(default)]
    pub spare: bool,
    /// [sweep-sim] The everyday car, left in the garage while a dearer one
    /// is out for the weekend (`World::weekend_cars` swaps it back).
    #[serde(default)]
    pub usual: bool,
}

impl OwnedCar {
    pub fn spot(&self) -> Option<&ParkSpot> {
        match &self.place {
            CarPlace::Parked(s) => Some(s),
            CarPlace::Driving(_) => None,
        }
    }
}

/// What a vehicle on the road is doing beyond driving its route. The
/// lifecycle (`world/cars.rs`) reads it when the vehicle arrives.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub enum Errand {
    /// An ordinary trip: a private car to its owner's destination, or a
    /// city vehicle whose own system (services, trash, tow trucks) tracks
    /// it by car id.
    #[default]
    Trip,
    /// A private car looking for a space: `tries` places checked so far.
    Search { tries: u8 },
    /// Heading for a parking business it means to use.
    ToLot(crate::buildings::BuildingId),
    /// Heading for a free kerb slot it saw.
    ToKerb,
    /// Driven to `dealer` to be sold there (a null dealer: sold out of
    /// town, driven off the map).
    Sale(crate::buildings::BuildingId),
    /// Delivered from the dealer to the new owner's home.
    Deliver,
    /// Leaving town for good with its emigrating owner.
    Leave,
    /// A truck or van out on a job; afterwards it returns to `base` (the
    /// business that keeps it), or leaves town when `None`.
    Haul { base: Option<crate::buildings::BuildingId> },
    /// A truck or van driving back to the business it belongs to.
    Return(crate::buildings::BuildingId),
    /// [parking] A parking enforcement car on its rounds from an office.
    Patrol(crate::buildings::BuildingId),
    /// [parking] A tow truck on job `n` (`crate::enforcement::TowJob`).
    Tow(u32),
    /// [services] A tow truck on its way to the wreck `CarId` (a crash
    /// written off in the road, `World::step_wrecks`), ambers flashing.
    Recover(crate::traffic::CarId),
    /// [taxis] A cab of a taxi company (`crate::taxi`): what it does on
    /// arriving is in its record's job.
    Taxi(crate::taxi::CabId),
    /// Through traffic from elsewhere to elsewhere along the highway. Set
    /// only where through cars are made (`World::step_through_traffic`,
    /// `resume_through_car`), so nothing else (a fire engine, a tow or
    /// garbage truck, a takeover outsider) is counted, capped or re-spawned
    /// as through traffic just because it has no driver or owner.
    Through,
    /// A city vehicle with nowhere left to go in town (its station, office,
    /// depot or line was pulled down): it drives off the map by the nearest
    /// exit it can reach and leaves the world there
    /// (`World::retire_vehicle`). Vehicles never vanish mid-street.
    Out,
}

/// Why a private car came into or left the city: the only ways it can.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CarEvent {
    /// Drove in with a household moving to town.
    ArrivedWithHousehold,
    /// Bought at a dealer in town.
    Bought,
    /// Bought out of town and driven in from the map edge.
    BoughtOutOfTown,
    /// Given a place on loading a save from before cars were persistent.
    Migrated,
    /// Sold to a dealer.
    Sold,
    /// Scrapped (worth nothing).
    Scrapped,
    /// Driven out of town by an emigrating owner.
    Emigrated,
}

impl CarEvent {
    pub fn creates(self) -> bool {
        matches!(self, CarEvent::ArrivedWithHousehold | CarEvent::Bought | CarEvent::BoughtOutOfTown | CarEvent::Migrated)
    }

    pub fn label(self) -> &'static str {
        match self {
            CarEvent::ArrivedWithHousehold => "arrived_with_household",
            CarEvent::Bought => "bought",
            CarEvent::BoughtOutOfTown => "bought_out_of_town",
            CarEvent::Migrated => "migrated",
            CarEvent::Sold => "sold",
            CarEvent::Scrapped => "scrapped",
            CarEvent::Emigrated => "emigrated",
        }
    }
}

/// Trip modes counted per day, indexed by `citizens::TripMode::index`
/// (room for modes added later, such as air taxis).
pub const MODE_SLOTS: usize = 8;

/// Counts the lifecycle and the mode choice keep, today and in total.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CarStats {
    /// Private cars created and destroyed since the city began.
    pub created_total: u32,
    pub destroyed_total: u32,
    /// The same by event (`CarEvent::label`).
    pub events_total: std::collections::BTreeMap<String, u32>,
    pub bought_today: u32,
    pub sold_today: u32,
    pub bikes_bought_today: u32,
    pub bikes_sold_today: u32,
    /// Trips begun today and yesterday, by mode.
    pub trips_today: [u32; MODE_SLOTS],
    pub trips_yesterday: [u32; MODE_SLOTS],
    /// Drivers who had to look beyond their destination for a space.
    pub circled_today: u32,
    /// Traffic seconds spent looking for a space today.
    pub search_seconds_today: f32,
    /// Where arriving cars parked today: [home/building, lot, kerb, overflow].
    pub parked_today: [u32; 4],
    /// Trucks and vans bought by businesses (none was parked at base).
    pub fleet_bought_total: u32,
    /// Trucks and vans back at their base today.
    pub fleet_returned_today: u32,
}

/// Every private car in the city, the trucks and vans parked at their
/// businesses, and the counts.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Vehicles {
    pub cars: slotmap::SlotMap<VehicleId, OwnedCar>,
    /// Trucks and vans parked at the business that owns them.
    #[serde(default)]
    pub fleet_parked: std::collections::BTreeMap<crate::buildings::BuildingId, u16>,
    #[serde(default)]
    pub stats: CarStats,
    /// Kerb slots taken, per street segment: (side, slot) → car. Rebuilt
    /// from `cars` after a load or a road rebuild.
    #[serde(skip)]
    pub kerbs: std::collections::HashMap<crate::roads::SegmentId, std::collections::HashMap<(u8, u16), VehicleId>>,
    /// Streets with no kerb parking (shopping streets keep their kerbs for
    /// loading), and the (plan version, roads version) they were found for.
    #[serde(skip)]
    pub no_kerb: (Option<(u64, u64)>, std::collections::HashSet<crate::roads::SegmentId>),
    /// [parking] Per-street kerb time limits in minutes, overriding the
    /// default `enforcement::KERB_LIMIT` (0: no limit). The hook for
    /// per-street rules.
    #[serde(default)]
    pub kerb_rules: std::collections::BTreeMap<crate::roads::SegmentId, u16>,
    /// [parking] Parking lots the city placed and runs, with their size and
    /// price.
    #[serde(default)]
    pub city_lots: std::collections::BTreeMap<crate::buildings::BuildingId, crate::enforcement::CityLot>,
}

impl Vehicles {
    pub fn note(&mut self, e: CarEvent) {
        *self.stats.events_total.entry(e.label().to_string()).or_insert(0) += 1;
        if e.creates() {
            self.stats.created_total += 1;
        } else {
            self.stats.destroyed_total += 1;
        }
    }

    /// Rebuild the kerb index from where cars stand.
    pub fn reindex_kerbs(&mut self) {
        self.kerbs.clear();
        for (id, car) in &self.cars {
            if let Some(s) = car.spot().filter(|s| s.kind.on_street()) {
                if let Some(seg) = s.segment {
                    self.kerbs.entry(seg).or_default().insert((s.side, s.slot), id);
                }
            }
        }
    }

    /// Private cars parked, driving, and driving while looking for a space.
    pub fn counts(&self, traffic: &crate::traffic::Traffic) -> (usize, usize, usize) {
        let (mut parked, mut driving, mut searching) = (0, 0, 0);
        for car in self.cars.values() {
            match car.place {
                CarPlace::Parked(_) => parked += 1,
                CarPlace::Driving(cid) => {
                    driving += 1;
                    if traffic.cars.get(cid).is_some_and(|c| matches!(c.errand, Errand::Search { .. } | Errand::ToKerb | Errand::ToLot(_))) {
                        searching += 1;
                    }
                }
            }
        }
        (parked, driving, searching)
    }
}

/// The five traits of `nd_minds::Personality` (sociability, ambition,
/// hedonism, caution, curiosity), drawn from the seed the same way, for
/// choices the sim makes itself (how to travel).
pub fn traits_of(seed: u64) -> [f32; 5] {
    let mut rng = SimRng::seed_from_u64(seed);
    [rng.random(), rng.random(), rng.random(), rng.random(), rng.random()]
}

/// What a bike costs at a shop.
pub const BIKE_PRICE: f32 = 45.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hedonism_matches_the_personality() {
        for seed in [1u64, 42, 9_999, 123_456_789] {
            let p = nd_minds::Personality::from_seed(seed);
            assert_eq!(hedonism_of(seed), p.hedonism);
        }
    }

    #[test]
    fn poor_buy_old_and_rich_buy_new() {
        let mut rng = SimRng::seed_from_u64(3);
        for _ in 0..200 {
            let poor = VehicleModel::for_owner(20.0, 45, 0.5, &mut rng);
            assert_eq!(poor.class, VehicleClass::Cheap);
            assert!(poor.wear >= 1 && !poor.tuned);
            let rich = VehicleModel::for_owner(HIGH_INCOME + 50.0, 50, 0.2, &mut rng);
            assert_eq!(rich.class, VehicleClass::Expensive);
            assert_eq!(rich.wear, 0);
            assert!(!rich.tuned, "only the young tune");
        }
    }

    #[test]
    fn young_rich_hedonists_soup_up() {
        let mut rng = SimRng::seed_from_u64(5);
        let n = 400;
        let flashy = (0..n).filter(|_| {
            let m = VehicleModel::for_owner(HIGH_INCOME + 50.0, 24, 0.95, &mut rng);
            m.tuned || m.body == BodyType::AkiraBike
        });
        let k = flashy.count();
        assert!(k > n / 3, "{k} of {n}");
        let old = (0..n).filter(|_| {
            let m = VehicleModel::for_owner(HIGH_INCOME + 50.0, 60, 0.95, &mut rng);
            m.tuned
        });
        assert_eq!(old.count(), 0);
    }

    #[test]
    fn the_young_ride_motorcycles_more() {
        let mut rng = SimRng::seed_from_u64(9);
        let bikes = |age: u8, rng: &mut SimRng| (0..1000).filter(|_| VehicleModel::for_owner(80.0, age, 0.3, rng).body.two_wheeler()).count();
        let young = bikes(22, &mut rng);
        let old = bikes(55, &mut rng);
        assert!(young > old * 2, "young {young} old {old}");
    }

    #[test]
    fn trucks_follow_their_cargo() {
        let mut rng = SimRng::seed_from_u64(1);
        assert_eq!(VehicleModel::for_cargo(Resource::Groceries, Some(BusinessKind::Grocery), false, false, &mut rng).livery, Livery::Grocery);
        assert_eq!(VehicleModel::for_cargo(Resource::Grain, None, false, true, &mut rng).body, BodyType::Semi);
        assert_eq!(VehicleModel::for_cargo(Resource::Parts, None, true, false, &mut rng).livery, Livery::Construction);
        assert_eq!(VehicleModel::delivery(BusinessKind::GadgetShop, &mut rng).livery, Livery::Parcels);
        assert!(VehicleModel::bus(3, &mut rng).body.is_bus());
    }

    #[test]
    fn an_old_car_without_a_model_loads_as_a_mid_sedan() {
        let m: VehicleModel = ron::from_str("()").unwrap();
        assert_eq!(m, VehicleModel::default());
        assert_eq!(m.body, BodyType::Sedan);
        let m2 = VehicleModel { tuned: true, livery: Livery::Pizza, ..Default::default() };
        let back: VehicleModel = ron::from_str(&ron::to_string(&m2).unwrap()).unwrap();
        assert_eq!(back, m2);
    }
}
