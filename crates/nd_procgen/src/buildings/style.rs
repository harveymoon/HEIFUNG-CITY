//! The architectural styles.

use super::*;

/// Architectural styles. Each belongs to one land use; the tier decides which
/// are on the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Style {
    // residential
    Stacks,
    Tenement,
    Brutalist,
    BalconySlab,
    Courtyard,
    Crystal,
    Fortress,
    GardenTower,
    Villas,
    // commercial
    Shophouse,
    MarketHall,
    RetailBlock,
    PodiumOffice,
    SlabTower,
    TwistTower,
    CantileverMall,
    // industrial
    Junkyard,
    RustyWarehouse,
    Factory,
    TankFarm,
    Datacenter,
    Depot,
    // entertainment
    ShackBar,
    Club,
    Cinema,
    Casino,
    Arena,
    // civic
    Precinct,
    Ministry,
    Library,
    Courthouse,
    Monument,
    // park
    ScrubLot,
    Garden,
    Plaza,
    // parking businesses
    ParkingLot,
    CarPark,
    // power plants
    PowerCoal,
    PowerSolar,
    PowerWind,
    PowerNuclear,
    // transit
    BusDepot,
    Station,
    FreightYard,
    // -- variety: Hong Kong, Akira and Ghost-in-the-Shell forms. Added as
    // one block at the end so `ALL` keeps its order. --
    /// Kowloon walled block: abutting slabs of mismatched height, cages,
    /// AC units and an antenna forest.
    WalledCity,
    /// Hong Kong pencil tower on a shop podium.
    PencilTower,
    /// Shanghai twin slabs with crowned tops and a link bridge.
    TwinSlab,
    /// Akira megastructure: a heavy base, several towers, sky bridges.
    Megastructure,
    /// Ghost-in-the-Shell stack of offset glass boxes with planted terraces.
    GlassTerraces,
    /// Capsule hotel: pods plugged into a concrete core.
    CapsuleHotel,
    /// Terraced housing stepping up from the street like a hillside.
    HillsideSteps,
    /// Covered night market arcade with stalls, lanterns and string lights.
    NightMarket,
    /// Karaoke / pachinko tower wrapped in signs and screens.
    NeonTower,
    /// Row of garage workshops and chop shops.
    Workshops,
    /// Hong Kong flatted factory: stacked industrial floors and a goods lift.
    FlattedFactory,
    /// Clean semiconductor fab: white boxes, exhaust stacks and pipe racks.
    FabPlant,
    // landscape: water and hot-ground plants, docks
    PowerHydro,
    PowerTidal,
    PowerGeothermal,
    ShippingPort,
    FerryTerminal,
    // city services and attractions (buildings/services.rs), placed by the player
    PoliceStation,
    FireStation,
    Hospital,
    School,
    DispatchCenter,
    Landmark,
    Stadium,
    Prison,
    // -- industry sub-zones (`nd_sim::industry`): forced by the business on
    // the lot, never drawn from the land use's menu. One block at the end. --
    /// Fields in rows, a barn, silos and a greenhouse.
    Farmstead,
    /// Pumpjacks and a derrick over stained ground.
    OilField,
    /// Distillation columns, pipe racks, tanks and flares.
    Refinery,
    /// Container stacks with a gantry and a loading lane.
    ContainerYard,
    /// Processing sheds, an ice plant, crates and a boat on blocks.
    FishProcessing,
    /// Marked bays with parked lorries and buses, a fuel island.
    TruckLot,
    // [society] placed by the player; kept last so `ALL` keeps its order.
    TrashDepot,
    // air transport, placed by the player (buildings/air.rs)
    Vertiport,
    // [subway] metro entrances and the Central Station
    MetroEntrance,
    CentralStation,
    // [power] grid battery packs, placed by the player
    PowerBattery,
    // -- [economy] businesses and waterfront docks (`nd_sim::market`,
    // `nd_sim::ports`): forced by the business or the dock, never drawn
    // from a land use's menu. Appended so saved indices keep their order. --
    /// Garage bays with roll-up doors, a car on a lift, tyre stacks.
    Mechanic,
    /// Showroom and a forecourt of matching new cars, bunting and flags.
    CarDealer,
    /// Yard of garbage trucks, dumpsters and a shed.
    TrashCompany,
    /// Narrow shopfront with bikes in racks outside.
    BikeShop,
    /// Barn, silo and a fenced pasture of cows.
    CowFarm,
    /// Long low chicken sheds with vents and feed silos.
    ChickenFarm,
    /// Windowless block, holding pens and refrigerated trucks.
    Slaughterhouse,
    /// Terraced open pit, haul trucks, a conveyor and spoil heaps.
    StripMine,
    /// Gutting sheds, an ice plant, crates and a reefer van.
    FishGutting,
    /// Factory with a freezer block, a chimney and a fish-stick sign.
    FishStickFactory,
    /// Open-sided hall of stalls under awnings, crates of fish.
    FishMarket,
    /// Restaurant with a deck, string lights and a fish sign.
    SeafoodRestaurant,
    /// Two-storey tee bays, a green field, nets on poles and floodlights.
    GolfRange,
    /// Converted brick warehouse with big windows, murals and sculptures.
    ArtStudio,
    /// White cube with a glass front, a banner and a sculpture.
    Gallery,
    /// Land side of a small marina: boathouse, boat trailers, parking.
    MarinaSmall,
    /// Land side of a large marina: clubhouse terrace, yacht on a cradle, flagpoles.
    MarinaLarge,
    /// Land side of a small fishing port: quay shed, nets, crates.
    FishingPortSmall,
    /// Land side of a large fishing port: ice house, crane, big shed, crates.
    FishingPortLarge,
    // -- [land] homes by class and density, and the forms bought and merged
    // plots are built in (`buildings/estates.rs`). One block at the end so
    // `ALL` keeps its order. --
    /// One-storey shacks of tin and tarp: the poor on quiet land.
    Shacks,
    /// Mobile homes off a gravel lane.
    TrailerPark,
    /// Round thatched huts round a fire pit.
    Huts,
    /// Detached houses on lawns: the middle class's suburb.
    Suburb,
    /// Two plots, one big house and a pool (`estate::Form::McMansion`).
    McMansion,
    /// A billionaire's estate (`estate::Form::Mansion`).
    Mansion,
    /// Towers on a podium with valet parking, a roof pool and sky-bridges.
    MegaHotel,
    /// A gallery grown into a museum.
    Museum,
    // -- [dirty-industry] the new dirty trades (`buildings/dirty.rs`),
    // appended so `ALL` keeps its order. --
    /// Blast furnace, rusted casting hall, stacks, ore and slag heaps.
    Smelter,
    /// Battery sheds, packed mud pens and a manure lagoon.
    Feedlot,
    /// Cell houses, banded tank rows, a stack and a poisoned pond.
    ChemicalWorks,
    /// [supertall] A league tower on its podium (`buildings/supertall.rs`):
    /// its form comes from `nd_sim::towers::Tower::design`.
    Supertall,
    /// [megacorp] A university's main hall, portico, clock tower and quad.
    University,
    // -- [waterfront] the shore's looks (`buildings/shore.rs`), appended so
    // `ALL` keeps its order. --
    /// A public beach: sand, towels, umbrellas, a lifeguard tower, bathers.
    Beach,
    /// Lobby block, wings stepping down to the water round a lit pool, cabanas.
    BeachResort,
    /// A tower with a balcony per room facing the water and a roof pool.
    WaterfrontHotel,
    /// A glass pavilion at the water's edge; its deck is its pier.
    PierRestaurant,
    /// A thatched shack with a bar, torches and string lights on the sand.
    BeachBar,
    /// A kiosk, kayak racks and pedalos; its pontoon is its jetty.
    BoatRental,
}

impl Style {
    /// Position in `ALL`, for per-style tables.
    pub fn index(self) -> usize {
        self as usize
    }

    pub const ALL: [Style; 118] = [
        Style::Stacks,
        Style::Tenement,
        Style::Brutalist,
        Style::BalconySlab,
        Style::Courtyard,
        Style::Crystal,
        Style::Fortress,
        Style::GardenTower,
        Style::Villas,
        Style::Shophouse,
        Style::MarketHall,
        Style::RetailBlock,
        Style::PodiumOffice,
        Style::SlabTower,
        Style::TwistTower,
        Style::CantileverMall,
        Style::Junkyard,
        Style::RustyWarehouse,
        Style::Factory,
        Style::TankFarm,
        Style::Datacenter,
        Style::Depot,
        Style::ShackBar,
        Style::Club,
        Style::Cinema,
        Style::Casino,
        Style::Arena,
        Style::Precinct,
        Style::Ministry,
        Style::Library,
        Style::Courthouse,
        Style::Monument,
        Style::ScrubLot,
        Style::Garden,
        Style::Plaza,
        Style::ParkingLot,
        Style::CarPark,
        Style::PowerCoal,
        Style::PowerSolar,
        Style::PowerWind,
        Style::PowerNuclear,
        Style::BusDepot,
        Style::Station,
        Style::FreightYard,
        Style::WalledCity,
        Style::PencilTower,
        Style::TwinSlab,
        Style::Megastructure,
        Style::GlassTerraces,
        Style::CapsuleHotel,
        Style::HillsideSteps,
        Style::NightMarket,
        Style::NeonTower,
        Style::Workshops,
        Style::FlattedFactory,
        Style::FabPlant,
        Style::PowerHydro,
        Style::PowerTidal,
        Style::PowerGeothermal,
        Style::ShippingPort,
        Style::FerryTerminal,
        Style::PoliceStation,
        Style::FireStation,
        Style::Hospital,
        Style::School,
        Style::DispatchCenter,
        Style::Landmark,
        Style::Stadium,
        Style::Prison,
        Style::Farmstead,
        Style::OilField,
        Style::Refinery,
        Style::ContainerYard,
        Style::FishProcessing,
        Style::TruckLot,
        Style::TrashDepot,
        Style::Vertiport,
        Style::MetroEntrance,
        Style::CentralStation,
        Style::PowerBattery,
        Style::Mechanic,
        Style::CarDealer,
        Style::TrashCompany,
        Style::BikeShop,
        Style::CowFarm,
        Style::ChickenFarm,
        Style::Slaughterhouse,
        Style::StripMine,
        Style::FishGutting,
        Style::FishStickFactory,
        Style::FishMarket,
        Style::SeafoodRestaurant,
        Style::GolfRange,
        Style::ArtStudio,
        Style::Gallery,
        Style::MarinaSmall,
        Style::MarinaLarge,
        Style::FishingPortSmall,
        Style::FishingPortLarge,
        Style::Shacks,
        Style::TrailerPark,
        Style::Huts,
        Style::Suburb,
        Style::McMansion,
        Style::Mansion,
        Style::MegaHotel,
        Style::Museum,
        Style::Smelter,
        Style::Feedlot,
        Style::ChemicalWorks,
        Style::Supertall,
        Style::University,
        Style::Beach,
        Style::BeachResort,
        Style::WaterfrontHotel,
        Style::PierRestaurant,
        Style::BeachBar,
        Style::BoatRental,
    ];

    /// [megacorp] The style a campus annex is drawn in (`None`: a dorm
    /// keeps the block it was bought as).
    pub fn for_annex(a: nd_sim::corp::Annex) -> Option<Style> {
        use nd_sim::corp::Annex;
        match a {
            Annex::Admin => Some(Style::Ministry),
            Annex::Theatre => Some(Style::Cinema),
            Annex::Sports => Some(Style::Arena),
            Annex::Frat => Some(Style::McMansion),
            Annex::Plaza => Some(Style::Plaza),
            Annex::Dorm => None,
        }
    }

    /// [land] The style a building's form forces (`nd_sim::estate::Form`);
    /// `None` for merged plots, which keep their kind's own looks.
    pub fn for_form(form: Option<nd_sim::estate::Form>) -> Option<Style> {
        use nd_sim::estate::Form;
        match form? {
            Form::McMansion => Some(Style::McMansion),
            Form::Mansion => Some(Style::Mansion),
            Form::MegaHotel => Some(Style::MegaHotel),
            Form::Museum => Some(Style::Museum),
            Form::Tower => Some(Style::Supertall),
            Form::Merged => None,
        }
    }

    /// The railway piece a style is drawn beside, if it is one.
    pub fn rail_kind(self) -> Option<RailFacility> {
        match self {
            Style::Station => Some(RailFacility::Station),
            Style::FreightYard => Some(RailFacility::FreightYard),
            Style::CentralStation => Some(RailFacility::Central),
            _ => None,
        }
    }

    /// High-tier towers that take a rooftop helideck when the sim gives the
    /// building a pad (`buildings::air::rooftop_deck` finds the roof).
    pub fn pad_capable(self) -> bool {
        matches!(self, Style::Crystal | Style::Fortress | Style::GardenTower | Style::PencilTower | Style::TwinSlab | Style::GlassTerraces | Style::Megastructure | Style::SlabTower | Style::TwistTower)
    }

    /// The style a placed service or attraction is drawn in.
    pub fn for_service(kind: nd_sim::services::ServiceKind) -> Style {
        use nd_sim::services::ServiceKind as K;
        match kind {
            K::Police => Style::PoliceStation,
            K::Fire => Style::FireStation,
            K::Health => Style::Hospital,
            K::School => Style::School,
            K::Dispatch => Style::DispatchCenter,
            K::Monument => Style::Landmark,
            K::Stadium => Style::Stadium,
            K::Jail => Style::Prison,
            K::TrashDepot => Style::TrashDepot,
            K::Vertiport => Style::Vertiport,
            // [parking] A parking enforcement office: a small station.
            K::ParkingEnforcement => Style::PoliceStation,
            K::University => Style::University,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Style::Stacks => "the Stacks",
            Style::Tenement => "walk-up tenement",
            Style::Brutalist => "brutalist block",
            Style::BalconySlab => "balcony slab",
            Style::Courtyard => "courtyard block",
            Style::Crystal => "crystal tower",
            Style::Fortress => "glass castle",
            Style::GardenTower => "garden tower",
            Style::Villas => "villa cluster",
            Style::Shophouse => "shophouses",
            Style::MarketHall => "market hall",
            Style::RetailBlock => "retail block",
            Style::PodiumOffice => "podium office",
            Style::SlabTower => "corporate slab",
            Style::TwistTower => "twist tower",
            Style::CantileverMall => "cantilever mall",
            Style::Junkyard => "junkyard",
            Style::RustyWarehouse => "rusted warehouse",
            Style::Factory => "factory",
            Style::TankFarm => "tank farm",
            Style::Datacenter => "datacentre",
            Style::Depot => "container depot",
            Style::ShackBar => "shack bar",
            Style::Club => "club",
            Style::Cinema => "cinema",
            Style::Casino => "casino",
            Style::Arena => "arena",
            Style::Precinct => "precinct house",
            Style::Ministry => "ministry",
            Style::Library => "library",
            Style::Courthouse => "courthouse",
            Style::Monument => "monument",
            Style::ScrubLot => "scrub lot",
            Style::Garden => "garden",
            Style::Plaza => "plaza",
            Style::ParkingLot => "parking lot",
            Style::CarPark => "car park",
            Style::PowerCoal => "coal plant",
            Style::PowerSolar => "solar farm",
            Style::PowerWind => "wind farm",
            Style::PowerNuclear => "nuclear plant",
            Style::PowerBattery => "battery pack",
            Style::BusDepot => "bus depot",
            Style::Station => "railway station",
            Style::MetroEntrance => "metro entrance",
            Style::CentralStation => "Central Station",
            Style::FreightYard => "freight yard",
            Style::WalledCity => "walled block",
            Style::PencilTower => "pencil tower",
            Style::TwinSlab => "twin slabs",
            Style::Megastructure => "megastructure",
            Style::GlassTerraces => "glass terraces",
            Style::CapsuleHotel => "capsule hotel",
            Style::HillsideSteps => "stepped terraces",
            Style::NightMarket => "night market",
            Style::NeonTower => "neon tower",
            Style::Workshops => "workshops",
            Style::FlattedFactory => "flatted factory",
            Style::FabPlant => "fab plant",
            Style::PowerHydro => "hydro plant",
            Style::PowerTidal => "tidal plant",
            Style::PowerGeothermal => "geothermal plant",
            Style::ShippingPort => "shipping port",
            Style::FerryTerminal => "ferry terminal",
            Style::PoliceStation => "police station",
            Style::FireStation => "fire station",
            Style::Hospital => "hospital",
            Style::School => "school",
            Style::DispatchCenter => "911 dispatch centre",
            Style::Landmark => "landmark",
            Style::Stadium => "stadium",
            Style::Prison => "jail",
            Style::University => "university hall",
            Style::Farmstead => "farm",
            Style::OilField => "oil field",
            Style::Refinery => "refinery",
            Style::ContainerYard => "container yard",
            Style::FishProcessing => "fish processing",
            Style::TruckLot => "truck and bus lot",
            Style::TrashDepot => "trash depot",
            Style::Vertiport => "vertiport",
            Style::Mechanic => "car repair shop",
            Style::CarDealer => "car dealership",
            Style::TrashCompany => "trash company",
            Style::BikeShop => "bike shop",
            Style::CowFarm => "cattle farm",
            Style::ChickenFarm => "chicken farm",
            Style::Slaughterhouse => "slaughterhouse",
            Style::StripMine => "strip mine",
            Style::FishGutting => "fish gutting sheds",
            Style::FishStickFactory => "fish-stick factory",
            Style::FishMarket => "fish market",
            Style::SeafoodRestaurant => "seafood restaurant",
            Style::GolfRange => "driving range",
            Style::ArtStudio => "artists' studios",
            Style::Gallery => "gallery",
            Style::MarinaSmall => "small marina",
            Style::MarinaLarge => "large marina",
            Style::FishingPortSmall => "small fishing port",
            Style::FishingPortLarge => "large fishing port",
            Style::Shacks => "shacks",
            Style::TrailerPark => "trailer park",
            Style::Huts => "huts",
            Style::Suburb => "suburban houses",
            Style::McMansion => "McMansion",
            Style::Mansion => "mansion",
            Style::MegaHotel => "mega hotel",
            Style::Museum => "museum",
            Style::Smelter => "smelter",
            Style::Feedlot => "factory farm",
            Style::ChemicalWorks => "chemical works",
            Style::Supertall => "league tower",
            Style::Beach => "public beach",
            Style::BeachResort => "beach resort",
            Style::WaterfrontHotel => "waterfront hotel",
            Style::PierRestaurant => "pier restaurant",
            Style::BeachBar => "beach bar",
            Style::BoatRental => "boat rental",
        }
    }
}
