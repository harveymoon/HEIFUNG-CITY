//! [companies] Companies grow from local roots: the names, colours and
//! rules behind the town's own firms (`world/companies.rs` is the `World`
//! side).
//!
//! *Builders*: there are no developers from out of town any more. Where the
//! land density asks for a bigger building, a local builder and landlord
//! firm (`OwnerKind::Builder`) buys the plots: the nearest one that is not
//! too far, or a new one founded by a resident who lives near the site
//! (named after them: "Okafor Builders"). It starts with that one building,
//! lets its homes and grows with every block it buys; past
//! [`BUILDER_GROWN`] properties it takes a grown-up name ("Okafor
//! Development Group"), and a big firm buys out a small one nearby
//! ([`MERGE_MAX_SMALL`]). Old saves: the out-of-town developers stop buying
//! and sell their buildings to local builders, [`RETIRE_PER_DAY`] a day
//! each, until they own nothing and leave.
//!
//! *Fleet liveries*: every local company with vehicles (cabs, tow trucks,
//! garbage trucks, lorries and vans, a builder's site lorries) paints them
//! in its own colour from [`FLEET_COLOURS`], one its nearest rivals of the
//! same trade do not use (`crate::vehicle::Livery::Fleet`).
//!
//! *Specialty*: a mega-corp builds and buys what its roots know
//! ([`Specialty::of_origin`]): a corp grown from shops of consumer goods
//! runs depots and delivery, one from cars dealerships and car factories,
//! one from groceries farms, distribution yards and grocery stores.
//!
//! *Shops into corporations*: a shop doing well buys the plot next door
//! and rebuilds bigger, then opens stores across town; enough of them and
//! it is big enough to become a mega-corp ([`ShopGrowth`], the hook the
//! malls build on).
//!
//! Everything is saved with serde defaults.

use crate::market::BusinessKind;
use crate::zoning::LandUse;
use serde::{Deserialize, Serialize};

// ── tuning ───────────────────────────────────────────────────────────────────

/// People per local builder firm the town carries (at least one, at most
/// [`BUILDERS_MAX`]); a site further than [`BUILDER_REACH`] from every
/// builder's holdings founds a new one while there is room.
pub const BUILDER_PEOPLE_EACH: usize = 1500;
pub const BUILDERS_MAX: usize = 12;
pub const BUILDER_REACH: f32 = 900.0;
/// A builder with this many buildings counts as twice as far from a site
/// as its roots are: the big ones leave room for rivals.
pub const BUILDER_SIZE_REACH: f32 = 25.0;
/// Properties from which a builder takes a grown-up name.
pub const BUILDER_GROWN: usize = 8;
/// A firm this small (properties) may be bought out by a rival at least
/// [`MERGE_RATIO`] times its size whose holdings lie within
/// [`MERGE_REACH`] of its own.
pub const MERGE_MAX_SMALL: usize = 2;
pub const MERGE_RATIO: usize = 4;
pub const MERGE_REACH: f32 = 1200.0;
/// What a firm pays a founder it buys out, as a share of the value of the
/// firm's buildings (the rest is the bank's, as on any purchase).
pub const MERGE_PRICE_SHARE: f64 = 0.02;
/// Buildings an out-of-town developer of an old save sells to local firms
/// a day.
pub const RETIRE_PER_DAY: usize = 10;
/// A business of a corp's specialty counts as this much nearer when it
/// picks what to buy next.
pub const SPECIALTY_PULL: f32 = 0.55;
/// History lines kept per owner.
pub const HISTORY_MAX: usize = 16;
/// Rivals of the same trade this close (m) never share a livery colour.
pub const LIVERY_RIVAL_REACH: f32 = 2500.0;
/// A fleet company's yard grows to at most this many plots before it opens
/// a depot elsewhere.
pub const YARD_MAX_LOTS: usize = 2;
/// A new rival yard of the same trade this close (m) to an old one is
/// marked down when the market picks its site: rivals start in different
/// parts of town.
pub const RIVAL_SPACING: f32 = 700.0;

// ── names ────────────────────────────────────────────────────────────────────

const BUILDER_SUFFIX: &[&str] = &["Builders", "Building Co.", "Homes", "Construction", "& Sons Builders", "Brothers Construction"];
const GROWN_SUFFIX: &[&str] = &["Development Group", "Land & Homes", "Properties Group", "Holdings", "Estates"];
const TOWER_SUFFIX: &[&str] = &["Towers", "Tower Group", "Heights Development", "Skyline Co."];

fn pick<'a>(list: &[&'a str], h: u64) -> &'a str {
    list[(crate::hash::mix(h, 0x5F) % list.len() as u64) as usize]
}

/// A local builder's name from its founder's surname: "Okafor Builders".
pub fn builder_name(surname: &str, h: u64) -> String {
    format!("{surname} {}", pick(BUILDER_SUFFIX, h))
}

/// The name a builder takes once it has grown: "Okafor Development Group".
pub fn grown_name(surname: &str, h: u64) -> String {
    format!("{surname} {}", pick(GROWN_SUFFIX, h))
}

/// A local tower developer's name: "Okafor Towers".
pub fn tower_firm_name(surname: &str, h: u64) -> String {
    format!("{surname} {}", pick(TOWER_SUFFIX, h))
}

/// A name's first word (the founder's surname in a firm named after them).
pub fn surname_of(name: &str) -> &str {
    name.split_whitespace().next().unwrap_or(name)
}

// ── fleet liveries ───────────────────────────────────────────────────────────

/// Fleet colours, sRGB: bright enough to tell apart on the street at night,
/// none of them cab yellow.
pub const FLEET_COLOURS: [[u8; 3]; 16] = [
    [225, 40, 50],
    [255, 120, 20],
    [40, 200, 90],
    [20, 190, 210],
    [40, 110, 255],
    [150, 70, 255],
    [235, 50, 200],
    [250, 250, 245],
    [30, 30, 36],
    [120, 220, 40],
    [255, 90, 140],
    [0, 140, 120],
    [140, 30, 40],
    [190, 150, 90],
    [70, 80, 110],
    [255, 170, 120],
];

/// A fleet colour as linear 0..1 RGB for meshes.
pub fn fleet_rgb(i: u8) -> [f32; 3] {
    let c = FLEET_COLOURS[i as usize % FLEET_COLOURS.len()];
    let lin = |v: u8| {
        let x = v as f32 / 255.0;
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    };
    [lin(c[0]), lin(c[1]), lin(c[2])]
}

/// "#e1283a".
pub fn fleet_hex(i: u8) -> String {
    let [r, g, b] = FLEET_COLOURS[i as usize % FLEET_COLOURS.len()];
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Trades that run vehicles of their own and wear a company livery: cabs,
/// tow trucks, garbage trucks, trucking (lorry depots, container yards),
/// and the shops and makers whose vans deliver.
pub fn runs_fleet(kind: BusinessKind) -> bool {
    use BusinessKind as K;
    matches!(kind, K::TaxiCompany | K::TowYard | K::TrashCompany | K::FleetLot | K::ContainerYard) || crate::vehicle::Livery::for_business(kind) != crate::vehicle::Livery::None
}

/// The trades that grow a yard of vehicles: bigger yard first, then depots
/// across town (`World::expand_firms`).
pub fn yard_trade(kind: BusinessKind) -> bool {
    matches!(kind, BusinessKind::TowYard | BusinessKind::TrashCompany | BusinessKind::FleetLot)
}

// ── a corp's specialty ───────────────────────────────────────────────────────

/// What a mega-corp knows from where it started, and so what it builds and
/// buys next.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Specialty {
    /// Consumer products: shipping, depots, delivery vans and couriers.
    Logistics,
    /// Cars: dealerships, car factories, tow and repair.
    Motors,
    /// Groceries: farms, distribution yards and grocery stores.
    Food,
    /// Hotels, bars, cafés and driving ranges.
    Leisure,
    /// Oil and fuel: refineries and offices.
    Energy,
    /// Heavy industry: fabs, mills, smelters.
    Industry,
    /// Offices and homes.
    #[default]
    Property,
}

impl Specialty {
    pub const ALL: [Specialty; 7] = [Specialty::Logistics, Specialty::Motors, Specialty::Food, Specialty::Leisure, Specialty::Energy, Specialty::Industry, Specialty::Property];

    /// The specialty of a corp grown out of a company of `kind`.
    pub fn of_origin(kind: BusinessKind) -> Specialty {
        use BusinessKind as K;
        match kind {
            K::GadgetShop | K::Assembly | K::Bookshop | K::ContainerYard | K::FleetLot | K::CargoHub => Specialty::Logistics,
            K::CarDealer | K::Mechanic | K::TaxiCompany | K::TowYard | K::BikeShop | K::Parking => Specialty::Motors,
            K::Grocery | K::Bakery | K::Cafe | K::VertFarm | K::GrainFarm | K::Dairy | K::ProteinVat | K::Mill | K::CowFarm | K::ChickenFarm | K::Slaughterhouse | K::Feedlot | K::Fishery | K::FishPlant | K::FishMarket | K::FishingPort | K::FishGutting | K::FishStickFactory => Specialty::Food,
            K::Hotel | K::Venue | K::GolfRange | K::BeachResort | K::PierRestaurant | K::BeachBar | K::BoatRental | K::SeafoodRestaurant | K::Marina | K::Gallery | K::ArtStudio => Specialty::Leisure,
            K::OilWell | K::Refinery | K::ChemicalWorks => Specialty::Energy,
            K::Fab | K::Smelter | K::StripMine | K::TrashCompany => Specialty::Industry,
            K::Office => Specialty::Property,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Specialty::Logistics => "consumer products and delivery",
            Specialty::Motors => "cars",
            Specialty::Food => "groceries",
            Specialty::Leisure => "hospitality",
            Specialty::Energy => "energy",
            Specialty::Industry => "heavy industry",
            Specialty::Property => "offices and homes",
        }
    }

    /// What it builds on a plot of `lu` (and buys before anything else), in
    /// order of preference. Only kinds the market can open on a plain lot
    /// (the car dealer and repair shop want a forecourt: `min_area`).
    pub fn builds(self, lu: LandUse) -> &'static [BusinessKind] {
        use BusinessKind as K;
        match (self, lu) {
            (Specialty::Logistics, LandUse::Commercial) => &[K::GadgetShop, K::Office],
            (Specialty::Logistics, LandUse::Industrial) => &[K::FleetLot, K::ContainerYard, K::Assembly],
            (Specialty::Motors, LandUse::Commercial) => &[K::CarDealer, K::Mechanic],
            (Specialty::Motors, LandUse::Industrial) => &[K::Assembly, K::TowYard, K::Fab],
            (Specialty::Food, LandUse::Commercial) => &[K::Grocery, K::Bakery],
            (Specialty::Food, LandUse::Industrial) => &[K::VertFarm, K::ContainerYard, K::GrainFarm, K::Dairy],
            (Specialty::Leisure, LandUse::Commercial) => &[K::Hotel, K::Cafe],
            (Specialty::Leisure, LandUse::Entertainment) => &[K::Venue, K::GolfRange],
            (Specialty::Energy, LandUse::Commercial) => &[K::Office],
            (Specialty::Energy, LandUse::Industrial) => &[K::Refinery, K::Fab],
            (Specialty::Industry, LandUse::Commercial) => &[K::Office],
            (Specialty::Industry, LandUse::Industrial) => &[K::Fab, K::Mill, K::Assembly],
            (Specialty::Property, LandUse::Commercial) => &[K::Office, K::Hotel],
            _ => &[],
        }
    }

    /// Does a business of `kind` belong to the specialty (bought first)?
    pub fn wants(self, kind: BusinessKind) -> bool {
        [LandUse::Commercial, LandUse::Industrial, LandUse::Entertainment].iter().any(|lu| self.builds(*lu).contains(&kind)) || Specialty::of_origin(kind) == self
    }

    /// Does it want homes (staff housing) more than the others do?
    pub fn builds_homes(self) -> bool {
        self == Specialty::Property
    }
}

/// Floor area a kind's site needs at least (a forecourt of cars).
pub fn min_area(kind: BusinessKind) -> f32 {
    match kind {
        BusinessKind::CarDealer => 700.0,
        BusinessKind::Mechanic => 350.0,
        BusinessKind::GolfRange => 2800.0,
        BusinessKind::TowYard | BusinessKind::FleetLot | BusinessKind::ContainerYard => 500.0,
        _ => 0.0,
    }
}

// ── shops into corporations ──────────────────────────────────────────────────

/// The next step a growing shop takes (`World::shop_growth`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShopGrowth {
    /// Buy the plot next door and rebuild bigger across both.
    Neighbour,
    /// Open another store across town (the chain that becomes a corp).
    Branch,
    /// [malls] Rebuild as a mall on the land it has assembled
    /// (`World::build_mall`, once its plots reach `world::malls::MALL_MIN_AREA`).
    Mall,
}

/// The shops that grow a store into a chain: the general store (grocery)
/// and the gadget shop. (Bakeries already open branches; a café or a
/// bookshop closing to rebuild across the plot next door cost the small
/// demo towns jobs.)
pub fn shop_chain(kind: BusinessKind) -> bool {
    matches!(kind, BusinessKind::Grocery | BusinessKind::GadgetShop)
}

/// A line of an owner's history.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OwnerNote {
    pub day: u32,
    pub text: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_local_and_stable() {
        assert_eq!(builder_name("Okafor", 7), builder_name("Okafor", 7));
        assert!(builder_name("Okafor", 7).starts_with("Okafor "));
        assert!(grown_name("Okafor", 3).starts_with("Okafor "));
        assert_eq!(surname_of("Okafor Builders"), "Okafor");
    }

    #[test]
    fn every_origin_has_a_specialty_that_builds_something() {
        for k in BusinessKind::ALL {
            let s = Specialty::of_origin(k);
            let any = [LandUse::Commercial, LandUse::Industrial, LandUse::Entertainment].iter().any(|lu| !s.builds(*lu).is_empty());
            assert!(any, "{k:?} -> {s:?} builds nothing");
        }
        // The examples from the todo: goods to delivery, cars to dealers
        // and factories, groceries to farms, distribution and stores.
        assert!(Specialty::of_origin(BusinessKind::GadgetShop).builds(LandUse::Industrial).contains(&BusinessKind::FleetLot));
        assert!(Specialty::of_origin(BusinessKind::CarDealer).builds(LandUse::Commercial).contains(&BusinessKind::CarDealer));
        assert!(Specialty::of_origin(BusinessKind::CarDealer).builds(LandUse::Industrial).contains(&BusinessKind::Assembly));
        let food = Specialty::of_origin(BusinessKind::Grocery);
        assert!(food.builds(LandUse::Industrial).contains(&BusinessKind::VertFarm) && food.builds(LandUse::Industrial).contains(&BusinessKind::ContainerYard) && food.builds(LandUse::Commercial).contains(&BusinessKind::Grocery));
    }

    #[test]
    fn fleet_colours_are_distinct_and_not_cab_yellow() {
        for (i, a) in FLEET_COLOURS.iter().enumerate() {
            assert!(!FLEET_COLOURS[i + 1..].contains(a));
            assert!(!(a[0] > 200 && a[1] > 180 && a[2] < 60), "too close to cab yellow: {a:?}");
        }
        assert_eq!(fleet_hex(0), "#e12832");
    }
}
