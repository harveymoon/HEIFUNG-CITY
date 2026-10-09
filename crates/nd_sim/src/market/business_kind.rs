//! Business kinds: the catalogue of businesses, their recipes, jobs, hours
//! and looks.

use super::*;

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
    // -- [sweep-sim] the trades students and downtown want. Appended so
    // saved kinds keep their order. --
    /// A café: coffee and pastries (baked goods on flour and milk), a
    /// place to sit; by campuses and on dear downtown streets.
    Cafe,
    /// A bookshop: books and stationery (goods printed on plastics-grade
    /// paper stock); by campuses and downtown.
    Bookshop,
}

/// What a business turns into what, per filled job per hour.
#[derive(Clone, Debug)]
pub struct Recipe {
    pub inputs: &'static [(Resource, f32)],
    pub outputs: &'static [(Resource, f32)],
}

impl BusinessKind {
    pub const COUNT: usize = 49;
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
        BusinessKind::Cafe,
        BusinessKind::Bookshop,
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
            BusinessKind::Cafe => "cafe",
            BusinessKind::Bookshop => "bookshop",
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
            // [sweep-sim] A job adds about 9 an hour at base prices, a
            // baker's 15 (the café sells fewer, sits people down), a
            // bookseller's about 17 against a gadget seller's 25.
            BusinessKind::Cafe => Recipe { inputs: &[(Resource::Flour, 0.6), (Resource::Dairy, 0.3)], outputs: &[(Resource::BakedGoods, 2.5)] },
            BusinessKind::Bookshop => Recipe { inputs: &[(Resource::Plastics, 0.25)], outputs: &[(Resource::Goods, 0.35)] },
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
            BusinessKind::Cafe => Hours::new(7, 21),
            BusinessKind::Bookshop => Hours::new(9, 20),
        }
    }

    /// Shops and venues that serve citizens at the door, and how many at once
    /// per job.
    pub fn customers_per_job(self) -> Option<u16> {
        match self {
            BusinessKind::Grocery | BusinessKind::Bakery | BusinessKind::GadgetShop | BusinessKind::Cafe | BusinessKind::Bookshop => Some(3),
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
                | BusinessKind::Cafe
                | BusinessKind::Bookshop
        )
    }

    /// [sweep-sim] Shops that sell what they make over the counter to
    /// whoever walks in: (what they sell). The café sells as a bakery
    /// does, the bookshop as a gadget shop.
    pub fn walk_in_sale(self) -> Option<Resource> {
        match self {
            BusinessKind::Grocery => Some(Resource::Groceries),
            BusinessKind::Bakery | BusinessKind::Cafe => Some(Resource::BakedGoods),
            BusinessKind::GadgetShop | BusinessKind::Bookshop => Some(Resource::Goods),
            _ => None,
        }
    }

    /// [megacorp] [sweep-sim] The trades students drop into by campus:
    /// pizza places (bakeries), cafés and bookshops.
    pub fn student_trade(self) -> bool {
        matches!(self, BusinessKind::Bakery | BusinessKind::GadgetShop | BusinessKind::Cafe | BusinessKind::Bookshop)
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
            BusinessKind::Grocery | BusinessKind::Bakery | BusinessKind::GadgetShop | BusinessKind::Cafe | BusinessKind::Bookshop | BusinessKind::Venue | BusinessKind::FishMarket | BusinessKind::SeafoodRestaurant | BusinessKind::Gallery | BusinessKind::PierRestaurant | BusinessKind::BeachBar => 36.0,
            _ => 24.0,
        }
    }
}
