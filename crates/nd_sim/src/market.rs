//! Resources, businesses and the market.
//!
//! Every non-residential building hosts a business with a recipe: inputs it
//! buys, outputs it makes, at a rate that scales with how many of its jobs
//! are filled. Stock, money and prices live on the business. The market is
//! just the set of businesses: a buyer asks for the cheapest seller of a
//! resource including haulage, strikes a deal, and a truck carries it. An
//! outer market imports and exports at worse prices so chains never deadlock.

use crate::buildings::BuildingId;
use crate::zoning::LandUse;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Resource {
    Produce,
    Grain,
    Flour,
    BakedGoods,
    Meat,
    Dairy,
    Groceries,
    Parts,
    Electronics,
    Goods,
    // -- industry block: oil and fish --
    /// Crude oil from the wells.
    Crude,
    /// Refined fuel: diesel for the lorries and boats.
    Fuel,
    /// Plastics and chemicals from the refinery.
    Plastics,
    /// The catch, landed by the fishing boats.
    Fish,
    // -- [economy] new supply chains, appended so saved maps keep their keys --
    /// Cattle and poultry from the cow and chicken farms, for the slaughterhouse.
    Livestock,
    /// Rock and ore dug out of a strip mine; sold out of town.
    Ore,
    /// Gutted, filleted fish: from the gutting sheds to the fish-stick
    /// factory, the fish markets and the seafood restaurants.
    Fillets,
    /// Paintings and sculpture from the artists' studios, hung and sold by
    /// the galleries.
    Art,
    /// [dirty-industry] Ingots and billets from the smelters; sold out of
    /// town. Appended.
    Metal,
}

impl Resource {
    pub const ALL: [Resource; 19] = [
        Resource::Produce,
        Resource::Grain,
        Resource::Flour,
        Resource::BakedGoods,
        Resource::Meat,
        Resource::Dairy,
        Resource::Groceries,
        Resource::Parts,
        Resource::Electronics,
        Resource::Goods,
        Resource::Crude,
        Resource::Fuel,
        Resource::Plastics,
        Resource::Fish,
        Resource::Livestock,
        Resource::Ore,
        Resource::Fillets,
        Resource::Art,
        Resource::Metal,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Resource::Produce => "produce",
            Resource::Grain => "grain",
            Resource::Flour => "flour",
            Resource::BakedGoods => "baked goods",
            Resource::Meat => "meat",
            Resource::Dairy => "dairy",
            Resource::Groceries => "groceries",
            Resource::Parts => "parts",
            Resource::Electronics => "electronics",
            Resource::Goods => "goods",
            Resource::Crude => "crude oil",
            Resource::Fuel => "fuel",
            Resource::Plastics => "plastics",
            Resource::Fish => "fish",
            Resource::Livestock => "livestock",
            Resource::Ore => "ore",
            Resource::Fillets => "fish fillets",
            Resource::Art => "art",
            Resource::Metal => "metal",
        }
    }

    /// Reference price per unit; the outer market trades around it.
    pub fn base_price(self) -> f32 {
        match self {
            Resource::Produce => 2.0,
            Resource::Grain => 1.0,
            Resource::Flour => 2.4,
            Resource::BakedGoods => 4.5,
            Resource::Meat => 7.0,
            Resource::Dairy => 3.0,
            Resource::Groceries => 6.0,
            Resource::Parts => 9.0,
            Resource::Electronics => 45.0,
            Resource::Goods => 60.0,
            Resource::Crude => 4.0,
            Resource::Fuel => 12.0,
            Resource::Plastics => 16.0,
            Resource::Fish => 3.0,
            Resource::Livestock => 3.5,
            Resource::Ore => 6.0,
            Resource::Fillets => 6.0,
            Resource::Art => 30.0,
            // Dear enough that a smelter on local ore pays its way shipping
            // ingots out of town at the export discount.
            Resource::Metal => 75.0,
        }
    }

    /// Hours of its own output a business keeps before shipping the rest
    /// out of town. [dirty-industry] Ingots have no local buyer: they go
    /// as soon as a lorryload stands in the yard.
    pub fn export_keep_hours(self) -> f32 {
        match self {
            Resource::Metal => 8.0,
            _ => 48.0,
        }
    }

    /// [dirty-industry] How much harder than an ordinary load a lorry full
    /// of this is on the road: ore, ingots and crude are dense, livestock
    /// lorries heavy and sloshing (`crate::road_wear::HEAVY` times this).
    pub fn road_wear_factor(self) -> f32 {
        match self {
            Resource::Ore | Resource::Metal => 2.5,
            Resource::Crude => 2.0,
            Resource::Livestock => 1.6,
            _ => 1.0,
        }
    }

    /// Sold to citizens directly, not to other businesses.
    pub fn is_retail(self) -> bool {
        matches!(self, Resource::Groceries | Resource::BakedGoods | Resource::Goods)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BusinessKind {
    VertFarm,
    GrainFarm,
    ProteinVat,
    Dairy,
    Mill,
    Fab,
    Assembly,
    Grocery,
    Bakery,
    GadgetShop,
    Venue,
    Office,
    /// Sells parking by the visit; no goods.
    Parking,
    // -- industry block (see `crate::industry`) --
    /// Pumpjacks over an oil deposit: crude, and a fouled ground.
    OilWell,
    /// Crude in, fuel and plastics out.
    Refinery,
    /// Container storage and loading: earns handling fees, burns fuel.
    ContainerYard,
    /// Truck and bus lot: parks idle fleets for a fee, burns fuel.
    FleetLot,
    /// Boats landing fish.
    Fishery,
    /// Fish processing: fish in, food (counted as meat) out.
    FishPlant,
    /// [parking] Tow company: tow trucks and a pound for towed cars; lives
    /// on tow and release fees (`crate::enforcement`).
    TowYard,
    /// [tourism] A hotel: tourists sleep here and pay by the night
    /// (`crate::tourism`). Opens on commercial land when visitors are being
    /// turned away for want of a bed. Last, so saved kinds keep their order.
    Hotel,
    /// [taxis] Taxi company: a yard of cabs that wait at ranks and carry
    /// riders for a fare (`crate::taxi`). Opened by a citizen when trips
    /// want cabs and there are none, never at random.
    TaxiCompany,
    /// Cargo hub the player places: a freight terminal that takes in the
    /// imports of the businesses around it, consolidated, and sends them
    /// on in its own lorries (`crate::cargo_hub`). Run by the city.
    CargoHub,
    // -- [economy] businesses opened by pressure (`crate::pressure`) and the
    // new supply chains. Appended so saved kinds keep their order. --
    /// Car repair shop: wrecks and servicing (`world/pressure.rs`).
    Mechanic,
    /// Car dealership: a showroom and a forecourt of new cars; sells the
    /// city's cars (`World::buy_car`).
    CarDealer,
    /// Private trash company: a yard of garbage trucks serving the
    /// buildings the city's depots do not reach, for a fee (`crate::trash`).
    TrashCompany,
    /// Bike shop: sells and mends bikes.
    BikeShop,
    /// Cattle on pasture, fed grain: livestock for the slaughterhouse.
    CowFarm,
    /// Long chicken sheds, fed grain: livestock for the slaughterhouse.
    ChickenFarm,
    /// Livestock in, meat out.
    Slaughterhouse,
    /// An open pit: ore for export; dust, noise and a scarred landscape.
    StripMine,
    /// A recreational marina on a player-placed dock: berth fees from boat
    /// owners (`world/waterfront.rs`).
    Marina,
    /// A fishing port on a player-placed dock: trawlers landing fish.
    FishingPort,
    /// Fish gutting and filleting sheds: fish in, fillets out.
    FishGutting,
    /// Fish-stick factory: fillets in, frozen food (counted as meat) out.
    FishStickFactory,
    /// Fish market: the catch sold to the public as groceries.
    FishMarket,
    /// Seafood restaurant: a place to go out, on fillets.
    SeafoodRestaurant,
    /// Golf driving range: bays, nets and floodlights; a place to go out.
    GolfRange,
    /// Artists' studios: art made in cheap space near the warehouses.
    ArtStudio,
    /// A small gallery: hangs and sells art, holds openings, takes donations.
    Gallery,
    // -- [dirty-industry] appended so saved kinds keep their order --
    /// Smelter and foundry: ore and fuel in, metal out; slag heaps, soot,
    /// heavy metals in the ground.
    Smelter,
    /// Factory farm: feedlot pens and battery sheds, grain in, livestock
    /// out by the lorryload; manure lagoons.
    Feedlot,
    /// Chemical works (chlor-alkali, mercury cells): crude in, chemicals
    /// (plastics) out; the dirtiest neighbour of all.
    ChemicalWorks,
    // -- [waterfront] trades that live on the water, opened by pressure on
    // waterfront lots (`crate::shore`). Appended so saved kinds keep their
    // order. --
    /// A beach resort: low wings round pools and gardens down to the sand,
    /// cabanas; tourists sleep here and pay well by the night.
    BeachResort,
    /// A fancy restaurant on a deck out over the water: a place to go out,
    /// dear.
    PierRestaurant,
    /// A beach bar: a shack, a deck, stools and string lights.
    BeachBar,
    /// Boat rental: kayaks, pedalos and small motorboats off a pontoon.
    BoatRental,
}

/// Opening hours on the clock, `open..close`; a close earlier than the open
/// runs past midnight, and equal hours mean round the clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hours {
    pub open: u8,
    pub close: u8,
}

impl Hours {
    pub const ALWAYS: Hours = Hours { open: 0, close: 0 };

    pub const fn new(open: u8, close: u8) -> Self {
        Self { open, close }
    }

    pub fn always(self) -> bool {
        self.open == self.close
    }

    /// Is the hour `h` (0..24) inside?
    pub fn contains(self, h: u8) -> bool {
        if self.always() {
            true
        } else if self.open < self.close {
            (self.open..self.close).contains(&h)
        } else {
            h >= self.open || h < self.close
        }
    }

    /// Hours open per day.
    pub fn span(self) -> u8 {
        if self.always() {
            24
        } else {
            (self.close + 24 - self.open) % 24
        }
    }

    /// The shift a worker with `seed` works: the whole day when it is ten
    /// hours or less, else one of two or three back-to-back shifts.
    pub fn shift(self, seed: u64) -> Hours {
        let span = self.span();
        if span <= 10 {
            return self;
        }
        let n = span.div_ceil(8);
        let len = span / n;
        let i = (seed % n as u64) as u8;
        let open = (self.open + i * len) % 24;
        let close = if i + 1 == n { self.close } else { (open + len) % 24 };
        Hours { open, close }
    }

    pub fn label(self) -> String {
        if self.always() {
            "24h".to_string()
        } else {
            format!("{:02}:00-{:02}:00", self.open, self.close)
        }
    }
}

/// What a business turns into what, per filled job per hour.
#[derive(Clone, Debug)]
pub struct Recipe {
    pub inputs: &'static [(Resource, f32)],
    pub outputs: &'static [(Resource, f32)],
}

impl BusinessKind {
    pub const COUNT: usize = 47;
    /// Every kind, in `index` order.
    pub const ALL: [BusinessKind; Self::COUNT] = [
        BusinessKind::VertFarm,
        BusinessKind::GrainFarm,
        BusinessKind::ProteinVat,
        BusinessKind::Dairy,
        BusinessKind::Mill,
        BusinessKind::Fab,
        BusinessKind::Assembly,
        BusinessKind::Grocery,
        BusinessKind::Bakery,
        BusinessKind::GadgetShop,
        BusinessKind::Venue,
        BusinessKind::Office,
        BusinessKind::Parking,
        BusinessKind::OilWell,
        BusinessKind::Refinery,
        BusinessKind::ContainerYard,
        BusinessKind::FleetLot,
        BusinessKind::Fishery,
        BusinessKind::FishPlant,
        BusinessKind::TowYard,
        BusinessKind::Hotel,
        BusinessKind::TaxiCompany,
        BusinessKind::CargoHub,
        BusinessKind::Mechanic,
        BusinessKind::CarDealer,
        BusinessKind::TrashCompany,
        BusinessKind::BikeShop,
        BusinessKind::CowFarm,
        BusinessKind::ChickenFarm,
        BusinessKind::Slaughterhouse,
        BusinessKind::StripMine,
        BusinessKind::Marina,
        BusinessKind::FishingPort,
        BusinessKind::FishGutting,
        BusinessKind::FishStickFactory,
        BusinessKind::FishMarket,
        BusinessKind::SeafoodRestaurant,
        BusinessKind::GolfRange,
        BusinessKind::ArtStudio,
        BusinessKind::Gallery,
        BusinessKind::Smelter,
        BusinessKind::Feedlot,
        BusinessKind::ChemicalWorks,
        BusinessKind::BeachResort,
        BusinessKind::PierRestaurant,
        BusinessKind::BeachBar,
        BusinessKind::BoatRental,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            BusinessKind::VertFarm => "vertical farm",
            BusinessKind::GrainFarm => "grain farm",
            BusinessKind::ProteinVat => "protein vat",
            BusinessKind::Dairy => "dairy",
            BusinessKind::Mill => "mill",
            BusinessKind::Fab => "parts fab",
            BusinessKind::Assembly => "assembly plant",
            BusinessKind::Grocery => "grocery",
            BusinessKind::Bakery => "bakery",
            BusinessKind::GadgetShop => "gadget shop",
            BusinessKind::Venue => "venue",
            BusinessKind::Office => "office",
            BusinessKind::Parking => "parking",
            BusinessKind::OilWell => "oil well",
            BusinessKind::Refinery => "refinery",
            BusinessKind::ContainerYard => "container yard",
            BusinessKind::FleetLot => "truck and bus lot",
            BusinessKind::Fishery => "fishery",
            BusinessKind::FishPlant => "fish processing plant",
            BusinessKind::TowYard => "tow company",
            BusinessKind::Hotel => "hotel",
            BusinessKind::TaxiCompany => "taxi company",
            BusinessKind::CargoHub => "cargo hub",
            BusinessKind::Mechanic => "car repair shop",
            BusinessKind::CarDealer => "car dealership",
            BusinessKind::TrashCompany => "trash company",
            BusinessKind::BikeShop => "bike shop",
            BusinessKind::CowFarm => "cattle farm",
            BusinessKind::ChickenFarm => "chicken farm",
            BusinessKind::Slaughterhouse => "slaughterhouse",
            BusinessKind::StripMine => "strip mine",
            BusinessKind::Marina => "marina",
            BusinessKind::FishingPort => "fishing port",
            BusinessKind::FishGutting => "fish gutting shed",
            BusinessKind::FishStickFactory => "fish-stick factory",
            BusinessKind::FishMarket => "fish market",
            BusinessKind::SeafoodRestaurant => "seafood restaurant",
            BusinessKind::GolfRange => "golf driving range",
            BusinessKind::ArtStudio => "artists' studios",
            BusinessKind::Gallery => "gallery",
            BusinessKind::Smelter => "smelter",
            BusinessKind::Feedlot => "factory farm",
            BusinessKind::ChemicalWorks => "chemical works",
            BusinessKind::BeachResort => "beach resort",
            BusinessKind::PierRestaurant => "pier restaurant",
            BusinessKind::BeachBar => "beach bar",
            BusinessKind::BoatRental => "boat rental",
        }
    }

    /// [economy] Short key for the remote API: the label in snake case.
    pub fn key(self) -> String {
        self.label().replace(['\'', '-'], "").replace(' ', "_")
    }

    /// [economy] The kind with this key, label or variant name (case,
    /// spaces, dashes and underscores ignored).
    pub fn from_key(s: &str) -> Option<BusinessKind> {
        let norm = |x: &str| x.to_ascii_lowercase().replace(['\'', '-', '_', ' '], "");
        let want = norm(s);
        Self::ALL.into_iter().find(|k| norm(k.label()) == want || norm(&format!("{k:?}")) == want)
    }

    pub fn recipe(self) -> Recipe {
        match self {
            BusinessKind::VertFarm => Recipe { inputs: &[], outputs: &[(Resource::Produce, 3.0)] },
            BusinessKind::GrainFarm => Recipe { inputs: &[], outputs: &[(Resource::Grain, 4.0)] },
            BusinessKind::ProteinVat => Recipe { inputs: &[(Resource::Grain, 1.0)], outputs: &[(Resource::Meat, 1.0)] },
            BusinessKind::Dairy => Recipe { inputs: &[(Resource::Grain, 0.5)], outputs: &[(Resource::Dairy, 1.5)] },
            // Milling is automated: a hand runs six of grain an hour. At two
            // the mill added too little to what it bought to pay its wages.
            BusinessKind::Mill => Recipe { inputs: &[(Resource::Grain, 6.0)], outputs: &[(Resource::Flour, 6.0)] },
            BusinessKind::Fab => Recipe { inputs: &[], outputs: &[(Resource::Parts, 1.0)] },
            // Two parts (18 at base) make 0.8 electronics (36); at 0.4 the
            // plant added nothing to what it bought and could never pay wages.
            BusinessKind::Assembly => Recipe { inputs: &[(Resource::Parts, 2.0)], outputs: &[(Resource::Electronics, 0.8)] },
            // Shops pay town wages, so what a job adds to its inputs at base
            // price covers its share of them: a grocer's job adds about 80 a day
            // against 54, a baker's about 59, a gadget seller's about 84.
            BusinessKind::Grocery => Recipe {
                inputs: &[(Resource::Produce, 1.0), (Resource::Flour, 0.4), (Resource::Meat, 0.3), (Resource::Dairy, 0.4)],
                outputs: &[(Resource::Groceries, 4.0)],
            },
            BusinessKind::Bakery => Recipe { inputs: &[(Resource::Flour, 1.2)], outputs: &[(Resource::BakedGoods, 4.0)] },
            BusinessKind::GadgetShop => Recipe { inputs: &[(Resource::Electronics, 0.5)], outputs: &[(Resource::Goods, 0.8)] },
            BusinessKind::Venue => Recipe { inputs: &[(Resource::Groceries, 0.3)], outputs: &[] },
            BusinessKind::Office => Recipe { inputs: &[], outputs: &[] },
            BusinessKind::Parking => Recipe { inputs: &[], outputs: &[] },
            BusinessKind::OilWell => Recipe { inputs: &[], outputs: &[(Resource::Crude, 2.5)] },
            BusinessKind::Refinery => Recipe { inputs: &[(Resource::Crude, 1.5)], outputs: &[(Resource::Fuel, 0.8), (Resource::Plastics, 0.4)] },
            // Fuel is burnt by the hour in `World::industry_hourly`, which
            // also pays the handling and bay fees.
            BusinessKind::ContainerYard => Recipe { inputs: &[(Resource::Fuel, 0.15)], outputs: &[] },
            BusinessKind::FleetLot => Recipe { inputs: &[(Resource::Fuel, 0.1)], outputs: &[] },
            BusinessKind::Fishery => Recipe { inputs: &[], outputs: &[(Resource::Fish, 2.5)] },
            BusinessKind::FishPlant => Recipe { inputs: &[(Resource::Fish, 1.5)], outputs: &[(Resource::Meat, 1.0)] },
            BusinessKind::TowYard => Recipe { inputs: &[], outputs: &[] },
            // [tourism] Breakfast: a little of the town's groceries. Rooms
            // are paid for by the night (`World::tourism_nightly`).
            BusinessKind::Hotel => Recipe { inputs: &[(Resource::Groceries, 0.15)], outputs: &[] },
            // Lives on fares (`world/taxis.rs`).
            BusinessKind::TaxiCompany => Recipe { inputs: &[], outputs: &[] },
            // Handling fees (`world/cargo_hubs.rs`).
            BusinessKind::CargoHub => Recipe { inputs: &[], outputs: &[] },
            // [economy] Services paid by the job (`world/pressure.rs`), car
            // and bike sales (`world/cars.rs`) and contracts (`trash.rs`).
            BusinessKind::Mechanic | BusinessKind::CarDealer | BusinessKind::TrashCompany | BusinessKind::BikeShop => Recipe { inputs: &[], outputs: &[] },
            // A job adds about 50 a day at base prices (7 an hour of value
            // added at `JOB_RATE`), like the farms and plants it sits with.
            BusinessKind::CowFarm => Recipe { inputs: &[(Resource::Grain, 1.0)], outputs: &[(Resource::Livestock, 2.2)] },
            BusinessKind::ChickenFarm => Recipe { inputs: &[(Resource::Grain, 0.8)], outputs: &[(Resource::Livestock, 1.8)] },
            BusinessKind::Slaughterhouse => Recipe { inputs: &[(Resource::Livestock, 2.0)], outputs: &[(Resource::Meat, 2.0)] },
            // Sold out of town at the export discount.
            BusinessKind::StripMine => Recipe { inputs: &[], outputs: &[(Resource::Ore, 2.0)] },
            // Berth fees (`world/waterfront.rs`).
            BusinessKind::Marina => Recipe { inputs: &[], outputs: &[] },
            BusinessKind::FishingPort => Recipe { inputs: &[], outputs: &[(Resource::Fish, 3.0)] },
            BusinessKind::FishGutting => Recipe { inputs: &[(Resource::Fish, 2.0)], outputs: &[(Resource::Fillets, 2.0)] },
            BusinessKind::FishStickFactory => Recipe { inputs: &[(Resource::Fillets, 1.5)], outputs: &[(Resource::Meat, 2.2)] },
            BusinessKind::FishMarket => Recipe { inputs: &[(Resource::Fish, 2.0)], outputs: &[(Resource::Groceries, 3.0)] },
            // Diners and golfers pay at the door (`World::venue_tabs`).
            BusinessKind::SeafoodRestaurant => Recipe { inputs: &[(Resource::Fillets, 0.3)], outputs: &[] },
            BusinessKind::GolfRange => Recipe { inputs: &[], outputs: &[] },
            BusinessKind::ArtStudio => Recipe { inputs: &[], outputs: &[(Resource::Art, 0.5)] },
            // Hangs art bought from the studios; sells it to visitors.
            BusinessKind::Gallery => Recipe { inputs: &[(Resource::Art, 0.1)], outputs: &[] },
            // [dirty-industry] One and a half of ore (9 at base) and a little
            // fuel make an ingot (75): exported at the discount (34) it
            // clears local ore and the wages, as the mine's ore does; on
            // imported ore the margin is thin, so smelters stay by the mines.
            BusinessKind::Smelter => Recipe { inputs: &[(Resource::Ore, 1.5), (Resource::Fuel, 0.1)], outputs: &[(Resource::Metal, 1.0)] },
            // Intensive: five times the livestock per hand of a cattle
            // farm (the whole point of it), so it pays its way even
            // selling out of town.
            BusinessKind::Feedlot => Recipe { inputs: &[(Resource::Grain, 2.0)], outputs: &[(Resource::Livestock, 12.0)] },
            BusinessKind::ChemicalWorks => Recipe { inputs: &[(Resource::Crude, 1.2)], outputs: &[(Resource::Plastics, 0.7)] },
            // [waterfront] Breakfasts and the bar; rooms are paid by the night
            // (`World::daily_tourism`).
            BusinessKind::BeachResort => Recipe { inputs: &[(Resource::Groceries, 0.25)], outputs: &[] },
            // Diners, drinkers and renters pay at the door (`World::venue_tabs`).
            BusinessKind::PierRestaurant => Recipe { inputs: &[(Resource::Groceries, 0.35)], outputs: &[] },
            BusinessKind::BeachBar => Recipe { inputs: &[(Resource::Groceries, 0.2)], outputs: &[] },
            BusinessKind::BoatRental => Recipe { inputs: &[(Resource::Fuel, 0.05)], outputs: &[] },
        }
    }

    /// Candidate kinds for a land use, picked at construction.
    pub fn candidates(land_use: LandUse) -> &'static [BusinessKind] {
        match land_use {
            // Industrial lots choose through `crate::industry` (ground,
            // neighbours, the player's bias); this is the full menu.
            LandUse::Industrial => &[
                BusinessKind::VertFarm,
                BusinessKind::GrainFarm,
                BusinessKind::ProteinVat,
                BusinessKind::Dairy,
                BusinessKind::Mill,
                BusinessKind::Fab,
                BusinessKind::Assembly,
                BusinessKind::OilWell,
                BusinessKind::Refinery,
                BusinessKind::ContainerYard,
                BusinessKind::FleetLot,
                BusinessKind::Fishery,
                BusinessKind::FishPlant,
                BusinessKind::TowYard,
                BusinessKind::CowFarm,
                BusinessKind::ChickenFarm,
                BusinessKind::Slaughterhouse,
                BusinessKind::StripMine,
                BusinessKind::FishGutting,
                BusinessKind::FishStickFactory,
                BusinessKind::TrashCompany,
                BusinessKind::Smelter,
                BusinessKind::Feedlot,
                BusinessKind::ChemicalWorks,
            ],
            LandUse::Commercial => &[BusinessKind::Grocery, BusinessKind::Grocery, BusinessKind::Bakery, BusinessKind::GadgetShop, BusinessKind::Office],
            LandUse::Entertainment => &[BusinessKind::Venue],
            LandUse::Civic => &[BusinessKind::Office],
            _ => &[],
        }
    }

    /// When the doors are open (and the shifts run).
    pub fn hours(self) -> Hours {
        match self {
            BusinessKind::Grocery => Hours::new(7, 22),
            BusinessKind::Bakery => Hours::new(6, 19),
            BusinessKind::GadgetShop => Hours::new(9, 20),
            BusinessKind::Venue => Hours::new(18, 3),
            BusinessKind::Office => Hours::new(8, 18),
            BusinessKind::Parking | BusinessKind::FleetLot | BusinessKind::ContainerYard | BusinessKind::OilWell | BusinessKind::TowYard | BusinessKind::Hotel | BusinessKind::TaxiCompany | BusinessKind::CargoHub | BusinessKind::Marina => Hours::ALWAYS,
            BusinessKind::Mechanic => Hours::new(8, 18),
            BusinessKind::CarDealer | BusinessKind::BikeShop => Hours::new(9, 19),
            BusinessKind::TrashCompany => Hours::new(6, 18),
            BusinessKind::FishingPort => Hours::new(4, 16),
            BusinessKind::FishGutting => Hours::new(5, 17),
            BusinessKind::FishMarket => Hours::new(5, 14),
            BusinessKind::SeafoodRestaurant => Hours::new(11, 23),
            // Floodlit until late.
            BusinessKind::GolfRange => Hours::new(8, 23),
            BusinessKind::ArtStudio => Hours::new(10, 20),
            BusinessKind::Gallery => Hours::new(11, 22),
            BusinessKind::CowFarm | BusinessKind::ChickenFarm | BusinessKind::Slaughterhouse | BusinessKind::StripMine | BusinessKind::FishStickFactory | BusinessKind::Feedlot => Hours::new(6, 22),
            BusinessKind::Smelter | BusinessKind::ChemicalWorks => Hours::new(6, 22),
            BusinessKind::Fishery => Hours::new(4, 16),
            BusinessKind::Refinery | BusinessKind::FishPlant => Hours::new(6, 22),
            // Plants run two shifts.
            BusinessKind::VertFarm | BusinessKind::GrainFarm | BusinessKind::ProteinVat | BusinessKind::Dairy | BusinessKind::Mill | BusinessKind::Fab | BusinessKind::Assembly => Hours::new(6, 22),
            // [waterfront]
            BusinessKind::BeachResort => Hours::ALWAYS,
            BusinessKind::PierRestaurant => Hours::new(12, 24),
            BusinessKind::BeachBar => Hours::new(10, 2),
            BusinessKind::BoatRental => Hours::new(8, 20),
        }
    }

    /// Shops and venues that serve citizens at the door, and how many at once
    /// per job.
    pub fn customers_per_job(self) -> Option<u16> {
        match self {
            BusinessKind::Grocery | BusinessKind::Bakery | BusinessKind::GadgetShop => Some(3),
            BusinessKind::Venue => Some(8),
            BusinessKind::FishMarket | BusinessKind::BikeShop => Some(3),
            BusinessKind::SeafoodRestaurant | BusinessKind::GolfRange => Some(6),
            BusinessKind::Gallery => Some(8),
            BusinessKind::PierRestaurant => Some(6),
            BusinessKind::BeachBar => Some(10),
            BusinessKind::BoatRental => Some(5),
            _ => None,
        }
    }

    /// [waterfront] Sleeps visitors from out of town (`crate::tourism`):
    /// hotels and beach resorts.
    pub fn is_lodging(self) -> bool {
        matches!(self, BusinessKind::Hotel | BusinessKind::BeachResort)
    }

    /// [waterfront] Opens only on a lot that runs down to the water
    /// (`Lot::shore`): the trades that live on the view.
    pub fn wants_waterfront(self) -> bool {
        matches!(self, BusinessKind::BeachResort | BusinessKind::PierRestaurant | BusinessKind::BeachBar | BusinessKind::BoatRental)
    }

    /// Part of the food chain, from farm to shelf: staffed first, and
    /// ordering ahead when the town is growing.
    pub fn is_food(self) -> bool {
        matches!(
            self,
            BusinessKind::VertFarm
                | BusinessKind::GrainFarm
                | BusinessKind::ProteinVat
                | BusinessKind::Dairy
                | BusinessKind::Mill
                | BusinessKind::Grocery
                | BusinessKind::Bakery
                | BusinessKind::Fishery
                | BusinessKind::FishPlant
                | BusinessKind::CowFarm
                | BusinessKind::ChickenFarm
                | BusinessKind::Slaughterhouse
                | BusinessKind::FishingPort
                | BusinessKind::FishGutting
                | BusinessKind::FishStickFactory
                | BusinessKind::FishMarket
                | BusinessKind::Feedlot
        )
    }

    /// [economy] A place people go out to and pay at the door by the hour
    /// (`World::venue_tabs`): venues, and the restaurants, ranges and
    /// galleries that open by pressure on whatever land suits them.
    pub fn is_outing(self) -> bool {
        matches!(self, BusinessKind::Venue | BusinessKind::SeafoodRestaurant | BusinessKind::GolfRange | BusinessKind::Gallery | BusinessKind::Marina | BusinessKind::PierRestaurant | BusinessKind::BeachBar | BusinessKind::BoatRental)
    }

    /// [economy] What an hour's tab costs here against a venue's.
    pub fn tab_factor(self) -> f32 {
        match self {
            BusinessKind::SeafoodRestaurant => 1.5,
            // [waterfront] White tablecloths over the water; a drink in the
            // sand; an hour in a pedalo.
            BusinessKind::PierRestaurant => 2.4,
            BusinessKind::BeachBar => 1.1,
            BusinessKind::BoatRental => 1.8,
            BusinessKind::GolfRange => 1.7,
            // A drink at the clubhouse or the marina café.
            BusinessKind::Marina => 1.3,
            // Entry is free; sales and donations pay (`world/pressure.rs`).
            BusinessKind::Gallery => 0.4,
            _ => 1.0,
        }
    }

    /// [economy] Opened only where its site or a pressure calls for it:
    /// never the market's pick for a plain lot (`World::business_kinds_viable`).
    pub fn needs_site(self) -> bool {
        matches!(
            self,
            BusinessKind::Mechanic
                | BusinessKind::CarDealer
                | BusinessKind::TrashCompany
                | BusinessKind::BikeShop
                | BusinessKind::CowFarm
                | BusinessKind::ChickenFarm
                | BusinessKind::Slaughterhouse
                | BusinessKind::StripMine
                | BusinessKind::Marina
                | BusinessKind::FishingPort
                | BusinessKind::FishGutting
                | BusinessKind::FishStickFactory
                | BusinessKind::FishMarket
                | BusinessKind::SeafoodRestaurant
                | BusinessKind::GolfRange
                | BusinessKind::ArtStudio
                | BusinessKind::Gallery
                | BusinessKind::Smelter
                | BusinessKind::Feedlot
                | BusinessKind::ChemicalWorks
                | BusinessKind::BeachResort
                | BusinessKind::PierRestaurant
                | BusinessKind::BeachBar
                | BusinessKind::BoatRental
        )
    }

    /// [economy] Most floors a building of this kind goes up with: sheds,
    /// forecourts and ranges stay low.
    pub fn max_floors(self) -> Option<u8> {
        Some(match self {
            BusinessKind::Mechanic | BusinessKind::TrashCompany | BusinessKind::GolfRange | BusinessKind::StripMine | BusinessKind::CowFarm | BusinessKind::ChickenFarm | BusinessKind::CarDealer | BusinessKind::BikeShop | BusinessKind::Feedlot => 1,
            BusinessKind::Smelter | BusinessKind::ChemicalWorks => 2,
            BusinessKind::FishMarket | BusinessKind::SeafoodRestaurant | BusinessKind::ArtStudio | BusinessKind::Gallery | BusinessKind::FishGutting => 2,
            BusinessKind::Slaughterhouse | BusinessKind::FishStickFactory => 3,
            // [waterfront] Low on the shore: a resort spreads, it does not climb.
            BusinessKind::BeachBar | BusinessKind::BoatRental => 1,
            BusinessKind::PierRestaurant => 2,
            BusinessKind::BeachResort => 5,
            _ => return None,
        })
    }

    /// The industry sub-zone this business belongs to, if any.
    pub fn industry(self) -> Option<crate::industry::IndustryKind> {
        crate::industry::IndustryKind::of_business(self)
    }

    /// [dirty-industry] How dirty this business is, `0..1`: its kind's
    /// dirtiness, the chemical works (mercury cells) dirtier still.
    pub fn dirtiness(self) -> f32 {
        match self {
            BusinessKind::ChemicalWorks => 1.0,
            _ => self.industry().map_or(0.0, |k| k.dirtiness()),
        }
    }

    /// [dirty-industry] Contamination it pours on the ground per working
    /// hour at full staff: (radius, amount). The chemical works leaks more
    /// than its kind.
    pub fn spill(self) -> Option<(f32, f32)> {
        match self {
            BusinessKind::ChemicalWorks => Some((140.0, 0.008)),
            _ => self.industry().and_then(|k| k.spill()),
        }
    }

    /// Parks lorries in bays: a truck and bus lot, or a cargo hub's own
    /// fleet bays.
    pub fn is_truck_lot(self) -> bool {
        matches!(self, BusinessKind::FleetLot | BusinessKind::CargoHub)
    }

    /// Stock a business tries to keep of each input, in hours of production.
    pub fn target_hours(self) -> f32 {
        match self {
            BusinessKind::Grocery | BusinessKind::Bakery | BusinessKind::GadgetShop | BusinessKind::Venue | BusinessKind::FishMarket | BusinessKind::SeafoodRestaurant | BusinessKind::Gallery | BusinessKind::PierRestaurant | BusinessKind::BeachBar => 36.0,
            _ => 24.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Business {
    pub kind: BusinessKind,
    pub stock: BTreeMap<Resource, f32>,
    pub money: f32,
    /// Revenue and spending today, for the inspector.
    pub revenue_today: f32,
    pub spend_today: f32,
    pub deals: u32,
    /// Days in a row the business could not pay its way.
    pub broke_days: u8,
    pub closed: bool,
    /// Deliveries on their way, so the business does not reorder.
    #[serde(default)]
    pub inbound: BTreeMap<Resource, f32>,
    /// Books closed since opening; the market reading
    /// (`Economy::business_cover`) skips the stocking-up first day.
    #[serde(default)]
    pub days_open: u16,
    /// A truck and bus lot: lorries parked here between jobs.
    #[serde(default)]
    pub fleet_trucks: u16,
    /// The lorries parked here, longest first: whose they are and since when.
    #[serde(default)]
    pub fleet_log: Vec<ParkedLorry>,
    /// The last [`Business::BOOK_DAYS`] days' books, oldest first: what it
    /// took and what it paid out each day (wages, inputs, extra tax), for
    /// the inspector's cash-flow sparkline. Written when the day closes.
    #[serde(default)]
    pub books: Vec<DayBook>,
    /// [economy] Why it opened, in words, when something in the city asked
    /// for it (a pressure, a dock the player built); `None` for businesses
    /// the market picked (`World::why_business` explains those).
    #[serde(default)]
    pub origin: Option<String>,
    /// [economy] Money given by visitors who loved the place (galleries).
    #[serde(default)]
    pub donations: f32,
    /// [economy] Doing well enough to want the plots next door: the land
    /// agent's hook for growth (a gallery into a museum). Nothing acts on
    /// it yet.
    #[serde(default)]
    pub wants_expand: bool,
    /// [camera] The resident who owns it, when one put their money in (a
    /// taxi firm's founder). The inspector pins it among their places.
    #[serde(default)]
    pub owner: Option<crate::citizens::CitizenId>,
}

/// One closed day of a business's books.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DayBook {
    pub revenue: f32,
    pub spend: f32,
}

/// A lorry parked at a truck and bus lot.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ParkedLorry {
    /// The business it works for (null when unknown).
    pub home: BuildingId,
    pub since: u64,
    #[serde(default)]
    pub model: crate::vehicle::VehicleModel,
}

impl Business {
    pub const PARKING_KIND: BusinessKind = BusinessKind::Parking;

    pub fn new(kind: BusinessKind, jobs: u16) -> Self {
        Self {
            kind,
            stock: BTreeMap::new(),
            // [dirty-industry] Heavy plant that sells out of town opens
            // with an investor's money: its first loads only leave once two
            // days of output stand in the yard.
            money: (150.0 + jobs as f32 * 40.0) * if matches!(kind, BusinessKind::Smelter | BusinessKind::Feedlot | BusinessKind::ChemicalWorks) { 5.0 } else { 1.0 },
            revenue_today: 0.0,
            spend_today: 0.0,
            deals: 0,
            broke_days: 0,
            closed: false,
            inbound: BTreeMap::new(),
            days_open: 0,
            fleet_trucks: 0,
            fleet_log: Vec::new(),
            books: Vec::new(),
            origin: None,
            donations: 0.0,
            wants_expand: false,
            owner: None,
        }
    }

    /// [economy] Days of the last `n` closed that took less than they paid out.
    pub fn losing_days(&self, n: usize) -> usize {
        self.books.iter().rev().take(n).filter(|d| d.revenue < d.spend).count()
    }

    /// Days of books kept: a month is enough to read a trend, and the list
    /// stays small in the save.
    pub const BOOK_DAYS: usize = 30;

    /// Close a day's books into the bounded history.
    pub fn close_books(&mut self, revenue: f32, spend: f32) {
        if self.books.len() >= Self::BOOK_DAYS {
            self.books.remove(0);
        }
        self.books.push(DayBook { revenue, spend });
    }

    pub fn inbound_of(&self, r: Resource) -> f32 {
        self.inbound.get(&r).copied().unwrap_or(0.0)
    }

    pub fn stock_of(&self, r: Resource) -> f32 {
        self.stock.get(&r).copied().unwrap_or(0.0)
    }

    pub fn add(&mut self, r: Resource, amount: f32) {
        *self.stock.entry(r).or_insert(0.0) += amount;
    }

    pub fn take(&mut self, r: Resource, amount: f32) -> f32 {
        let have = self.stock_of(r);
        let taken = amount.min(have);
        self.stock.insert(r, have - taken);
        taken
    }

    /// Units of each output the business can make per hour at `staffing` (0..1).
    pub fn hourly_capacity(&self, jobs: u16, staffing: f32) -> f32 {
        jobs as f32 * staffing * JOB_RATE
    }

    /// Target stock of an input: enough for `target_hours` at full staffing.
    pub fn target_input(&self, r: Resource, jobs: u16) -> f32 {
        let per_hour = self.kind.recipe().inputs.iter().find(|(x, _)| *x == r).map(|(_, a)| *a).unwrap_or(0.0);
        per_hour * jobs as f32 * JOB_RATE * self.kind.target_hours()
    }

    /// Selling price of an output: base, up when stock is scarce, down when it
    /// piles up.
    pub fn sell_price(&self, r: Resource, jobs: u16) -> f32 {
        let per_hour = self.kind.recipe().outputs.iter().find(|(x, _)| *x == r).map(|(_, a)| *a).unwrap_or(1.0);
        let target = (per_hour * jobs as f32 * JOB_RATE * 24.0).max(1.0);
        let ratio = (self.stock_of(r) / target).clamp(0.0, 3.0);
        let k = 1.5 - 0.4 * ratio;
        r.base_price() * k.clamp(0.6, 1.5)
    }

    pub fn sells(&self, r: Resource) -> bool {
        self.kind.recipe().outputs.iter().any(|(x, _)| *x == r)
    }

    pub fn buys(&self, r: Resource) -> bool {
        self.kind.recipe().inputs.iter().any(|(x, _)| *x == r)
    }

    /// Run one hour of production, limited by inputs on hand.
    pub fn produce(&mut self, jobs: u16, staffing: f32) -> f32 {
        let recipe = self.kind.recipe();
        if recipe.outputs.is_empty() || jobs == 0 {
            return 0.0;
        }
        let cap = self.hourly_capacity(jobs, staffing);
        // Scale by the scarcest input, and by the room left for the output:
        // with full shelves the business stops working its inputs instead of
        // burning stock it bought into output that has nowhere to go.
        let mut scale = 1.0f32;
        for (r, per) in recipe.inputs {
            let need = per * cap;
            if need > 0.0 {
                scale = scale.min(self.stock_of(*r) / need);
            }
        }
        for (r, per) in recipe.outputs {
            let make = per * cap;
            if make > 0.0 {
                let cap_stock = per * jobs as f32 * JOB_RATE * 96.0;
                scale = scale.min((cap_stock - self.stock_of(*r)).max(0.0) / make);
            }
        }
        let scale = scale.clamp(0.0, 1.0);
        if scale <= 0.0 {
            return 0.0;
        }
        for (r, per) in recipe.inputs {
            self.take(*r, per * cap * scale);
        }
        let mut made = 0.0;
        for (r, per) in recipe.outputs {
            // Outputs cap at a few days of production so stock does not grow forever.
            let cap_stock = per * jobs as f32 * JOB_RATE * 96.0;
            let amount = (per * cap * scale).min((cap_stock - self.stock_of(*r)).max(0.0));
            self.add(*r, amount);
            made += amount;
        }
        made
    }
}

/// Throughput of one filled job per hour, in recipe units.
pub const JOB_RATE: f32 = 0.3;
/// Trucks allowed on the road at once; beyond this deliveries are instant.
pub const MAX_TRUCKS: usize = 60;

/// Import and export with the world outside the city. Imports cost more than
/// local goods, exports pay less, so a local supply chain is worth building.
pub const IMPORT_MARKUP: f32 = 1.7;
pub const EXPORT_DISCOUNT: f32 = 0.45;
/// Haulage cost per unit per metre, added to a seller's price when comparing.
pub const HAULAGE: f32 = 0.0012;

/// Best local seller of `r` for a buyer at `at`: cheapest delivered price.
pub fn best_seller<'a>(sellers: impl Iterator<Item = (BuildingId, &'a Business, u16, Vec2)>, r: Resource, at: Vec2, amount: f32) -> Option<(BuildingId, f32)> {
    let mut best: Option<(BuildingId, f32)> = None;
    for (id, b, jobs, pos) in sellers {
        if b.closed || !b.sells(r) || b.stock_of(r) < amount * 0.5 {
            continue;
        }
        let price = b.sell_price(r, jobs) + HAULAGE * pos.distance(at) * r.base_price();
        if best.map_or(true, |(_, p)| price < p) {
            best = Some((id, price));
        }
    }
    best
}

/// Per-resource market statistics for the UI.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct MarketStats {
    pub avg_price: BTreeMap<Resource, f32>,
    pub stock: BTreeMap<Resource, f32>,
    pub deals_today: u32,
    pub imports_today: f32,
    pub exports_today: f32,
    pub businesses: u32,
    pub closed_total: u32,
    pub meals_sold_today: u32,
    pub hungry: u32,
    /// Groceries bought from outside today because local shelves ran short.
    #[serde(default)]
    pub grocery_imports_today: f32,
    /// Shoppers who found a shop shut or full today.
    #[serde(default)]
    pub turned_away_today: u32,
}

#[cfg(test)]
mod tests {

    /// [inspector] The books keep a bounded month, oldest first.
    #[test]
    fn books_keep_the_last_month() {
        let mut b = Business::new(BusinessKind::Grocery, 4);
        for d in 0..(Business::BOOK_DAYS + 5) {
            b.close_books(d as f32, 1.0);
        }
        assert_eq!(b.books.len(), Business::BOOK_DAYS);
        assert_eq!(b.books[0].revenue, 5.0);
        assert_eq!(b.books.last().unwrap().revenue, (Business::BOOK_DAYS + 4) as f32);
    }

    use super::*;

    #[test]
    fn hours_wrap_midnight_and_split_into_shifts() {
        let venue = BusinessKind::Venue.hours();
        assert!(venue.contains(23) && venue.contains(2) && !venue.contains(3) && !venue.contains(12));
        assert_eq!(venue.span(), 9);
        assert_eq!(venue.shift(1), venue, "a short day is one shift");
        let plant = BusinessKind::Mill.hours();
        assert_eq!(plant.shift(0), Hours::new(6, 14));
        assert_eq!(plant.shift(1), Hours::new(14, 22));
        let park = BusinessKind::Parking.hours();
        assert!((0..24).all(|h| park.contains(h)));
        let shifts: Vec<Hours> = (0..3).map(|s| park.shift(s)).collect();
        assert_eq!(shifts, vec![Hours::new(0, 8), Hours::new(8, 16), Hours::new(16, 0)]);
        // Every hour of a long day is covered by some shift.
        let groc = BusinessKind::Grocery.hours();
        for h in 0..24 {
            assert_eq!(groc.contains(h), (0..4).any(|s| groc.shift(s).contains(h)), "hour {h}");
        }
    }
}
