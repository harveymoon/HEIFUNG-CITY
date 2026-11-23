//! Business kinds. Their facts (recipes, hours, flags, the inspector's
//! line) are rows of the catalogue (`crate::catalogue::BUSINESSES`).

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
    /// [K34] Scrap yard: worn-out shipping containers (and other scrap)
    /// cut up into steel, sold to building sites and out of town. Opened by
    /// pressure when old boxes pile up. Appended so saved kinds keep their
    /// order.
    ScrapYard,
    // -- [r4-leisure-places] places to eat and go out (`crate::leisure`),
    // grown on commercial land by sub-zone and on leisure land. Appended
    // so saved kinds keep their order. --
    /// A diner: booths, a counter and breakfast all day; near homes.
    Diner,
    /// A noodle or ramen bar: a counter, steam and a short menu.
    NoodleBar,
    /// A pizza place: a wood oven, slices to go, families in the booths.
    PizzaPlace,
    /// A fast-food joint with a drive-through lane, on the main roads.
    FastFood,
    /// A food hall: many counters under one roof, long shared tables.
    FoodHall,
    /// Street-food stalls and food trucks on an open lot.
    StreetFood,
    /// A fine-dining restaurant: few tables, a long evening, dear.
    FineDining,
    /// A bubble-tea and juice bar.
    BubbleTea,
    /// A bar or pub (the nightclub is the `Venue`).
    Bar,
    /// Karaoke boxes: private rooms by the hour.
    KaraokeBox,
    /// A cinema: films booked from the distributors.
    Cinema,
    /// A theatre: shows booked from touring companies.
    Theatre,
    /// A concert hall and live-music venue: bands booked by the night.
    ConcertHall,
    /// A bowling alley: lanes, shoes and a snack bar; families.
    BowlingAlley,
    /// An arcade and VR arcade.
    Arcade,
    /// A gym: early mornings and after work.
    Gym,
    /// A comedy club: a stage, a brick wall, a two-drink minimum.
    ComedyClub,
    /// [r8] A freight forwarder (a "shipping company" in town): leases
    /// empty containers, sends its lorries round the firms to fill them
    /// with their exports, takes the full boxes to the port or the freight
    /// yard and charges each firm a fee per load. Opened by pressure where
    /// the town exports. Appended so saved kinds keep their order.
    FreightForwarder,
    /// [r14-builders] A building contractor: a yard, crews of its own
    /// staff, vans and lorries; hired per building site (`world/builders.rs`),
    /// it supplies the crew and buys the site's materials. Opened by
    /// pressure when sites go to builders from out of town. Appended.
    Contractor,
    /// [r14-builders] A builders' merchant and hardware store: a big shed,
    /// a yard of timber, bricks and pallets, a trade counter; stocks the
    /// building materials and sells them to builders and industry, and DIY
    /// goods to residents. Opened by pressure. Appended.
    BuildersMerchant,
    /// [r14-foodtrucks] A food truck, van, cart or stand run by a household
    /// (`crate::food_trucks`): no building of its own; it drives out to
    /// where the crowds are. Never grown on a lot. Appended.
    FoodTruck,
}

/// What a business turns into what, per filled job per hour.
#[derive(Clone, Debug)]
pub struct Recipe {
    pub inputs: &'static [(Resource, f32)],
    pub outputs: &'static [(Resource, f32)],
}

impl BusinessKind {
    pub const COUNT: usize = 71;
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
        BusinessKind::ScrapYard,
        BusinessKind::Diner,
        BusinessKind::NoodleBar,
        BusinessKind::PizzaPlace,
        BusinessKind::FastFood,
        BusinessKind::FoodHall,
        BusinessKind::StreetFood,
        BusinessKind::FineDining,
        BusinessKind::BubbleTea,
        BusinessKind::Bar,
        BusinessKind::KaraokeBox,
        BusinessKind::Cinema,
        BusinessKind::Theatre,
        BusinessKind::ConcertHall,
        BusinessKind::BowlingAlley,
        BusinessKind::Arcade,
        BusinessKind::Gym,
        BusinessKind::ComedyClub,
        BusinessKind::FreightForwarder,
        BusinessKind::Contractor,
        BusinessKind::BuildersMerchant,
        BusinessKind::FoodTruck,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// [r13-catalogue] Its row in the catalogue: every fact below reads it.
    pub fn row(self) -> &'static crate::catalogue::BusinessRow {
        crate::catalogue::business(self)
    }

    pub fn label(self) -> &'static str {
        self.row().label
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
        let r = self.row();
        Recipe { inputs: r.inputs, outputs: r.outputs }
    }

    /// Candidate kinds for a land use, picked at construction.
    pub fn candidates(land_use: LandUse) -> &'static [BusinessKind] {
        crate::catalogue::candidates(land_use)
    }

    /// When the doors are open (and the shifts run).
    pub fn hours(self) -> Hours {
        self.row().hours
    }

    /// Shops and venues that serve citizens at the door, and how many at once
    /// per job.
    pub fn customers_per_job(self) -> Option<u16> {
        self.row().customers_per_job
    }

    /// [waterfront] Sleeps visitors from out of town (`crate::tourism`):
    /// hotels and beach resorts.
    pub fn is_lodging(self) -> bool {
        self.row().lodging
    }

    /// [waterfront] Opens only on a lot that runs down to the water
    /// (`Lot::shore`): the trades that live on the view.
    pub fn wants_waterfront(self) -> bool {
        self.row().waterfront
    }

    /// Part of the food chain, from farm to shelf: staffed first, and
    /// ordering ahead when the town is growing.
    pub fn is_food(self) -> bool {
        self.row().food
    }

    /// [economy] A place people go out to and pay at the door by the hour
    /// (`World::venue_tabs`): venues, and the restaurants, ranges and
    /// galleries that open by pressure on whatever land suits them.
    pub fn is_outing(self) -> bool {
        self.row().outing || self.is_eat_or_go_out()
    }

    /// [economy] What an hour's tab costs here against a venue's.
    pub fn tab_factor(self) -> f32 {
        self.row().tab
    }

    /// [economy] Opened only where its site or a pressure calls for it:
    /// never the market's pick for a plain lot (`World::business_kinds_viable`).
    pub fn needs_site(self) -> bool {
        self.row().needs_site
    }

    /// [sweep-sim] Shops that sell what they make over the counter to
    /// whoever walks in: (what they sell).
    pub fn walk_in_sale(self) -> Option<Resource> {
        self.row().walk_in
    }

    /// [megacorp] [sweep-sim] The trades students drop into by campus.
    pub fn student_trade(self) -> bool {
        self.row().student_trade
    }

    /// [economy] Most floors a building of this kind goes up with: sheds,
    /// forecourts and ranges stay low.
    pub fn max_floors(self) -> Option<u8> {
        self.row().max_floors
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
        self.row().truck_lot
    }

    /// Stock a business tries to keep of each input, in hours of production.
    pub fn target_hours(self) -> f32 {
        self.row().stock_hours
    }
}
