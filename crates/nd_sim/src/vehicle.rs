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
}

impl BodyType {
    pub const ALL: [BodyType; 17] = [
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
    ];

    pub fn two_wheeler(self) -> bool {
        matches!(self, BodyType::Motorcycle | BodyType::AkiraBike | BodyType::Scooter)
    }

    pub fn is_bus(self) -> bool {
        matches!(self, BodyType::BubbleBus | BodyType::ArticulatedBus)
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
}

impl Livery {
    pub const ALL: [Livery; 7] = [Livery::None, Livery::Pizza, Livery::Parcels, Livery::Grocery, Livery::Construction, Livery::Utility, Livery::Transit];

    /// The fleet a business sends its deliveries in.
    pub fn for_business(kind: BusinessKind) -> Livery {
        match kind {
            BusinessKind::Bakery => Livery::Pizza,
            BusinessKind::Grocery | BusinessKind::VertFarm | BusinessKind::Dairy | BusinessKind::ProteinVat => Livery::Grocery,
            BusinessKind::GadgetShop | BusinessKind::Assembly => Livery::Parcels,
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
