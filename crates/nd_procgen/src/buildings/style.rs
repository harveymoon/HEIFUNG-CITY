//! The architectural styles: one row per style in the [`styles!`] table,
//! which makes the [`Style`] enum, [`Style::ALL`] and [`STYLES`] (label,
//! adjacency family and cluster score, and the generator that draws it). A
//! new style is one row here: the compiler then asks for nothing else, and
//! it cannot be left without a label, a family or a generator.

use super::adjacency::Family;
use super::*;

/// Draws a style: geometry, its context, its random stream, the extras.
pub(crate) type Draw = fn(&mut Geometry, &Ctx, &mut nd_sim::SimRng, &Extras);

/// One style: what it is called, how it sits with its neighbours
/// ([`adjacency::traits`]) and how it is drawn.
pub struct StyleDef {
    pub style: Style,
    pub label: &'static str,
    /// Broad kind of building, for the affinity table.
    pub family: Family,
    /// Affinity for the same style next door: positive repeats happily (the
    /// Stacks, shophouse rows), negative wants to stand apart (spires).
    pub cluster: f32,
    pub(crate) draw: Draw,
}

impl std::fmt::Debug for StyleDef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StyleDef").field("style", &self.style).field("label", &self.label).field("family", &self.family).field("cluster", &self.cluster).finish()
    }
}

macro_rules! styles {
    ($( $(#[$m:meta])* $v:ident { label: $label:literal, family: $fam:ident, cluster: $cluster:expr, draw: $draw:expr $(,)? }, )*) => {
        /// Architectural styles. Each belongs to one land use; the tier
        /// decides which are on the menu. The order is the save order:
        /// new styles are appended.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum Style {
            $( $(#[$m])* $v, )*
        }

        impl Style {
            /// Every style, in declaration (and save) order.
            pub const ALL: [Style; [$(stringify!($v)),*].len()] = [$(Style::$v,)*];
        }

        /// One row per style, in [`Style::ALL`] order.
        pub static STYLES: [StyleDef; Style::ALL.len()] = [
            $( StyleDef { style: Style::$v, label: $label, family: Family::$fam, cluster: $cluster, draw: $draw }, )*
        ];
    };
}

styles! {
    // residential
    Stacks { label: "the Stacks", family: Slum, cluster: 1.4, draw: |g, c, r, _| residential::stacks(g, c, r) },
    Tenement { label: "walk-up tenement", family: Walkup, cluster: 0.6, draw: |g, c, r, _| residential::tenement(g, c, r) },
    Brutalist { label: "brutalist block", family: Slab, cluster: 0.4, draw: |g, c, r, _| residential::brutalist(g, c, r) },
    BalconySlab { label: "balcony slab", family: Slab, cluster: 0.7, draw: |g, c, r, _| residential::balcony_slab(g, c, r) },
    Courtyard { label: "courtyard block", family: Walkup, cluster: 0.2, draw: |g, c, r, _| residential::courtyard(g, c, r) },
    Crystal { label: "crystal tower", family: Spire, cluster: -2.0, draw: |g, c, r, _| residential::crystal(g, c, r) },
    Fortress { label: "glass castle", family: Luxury, cluster: -1.8, draw: |g, c, r, _| towers::glass_castle(g, c, r) },
    GardenTower { label: "garden tower", family: Spire, cluster: -0.8, draw: |g, c, r, _| residential::garden_tower(g, c, r) },
    Villas { label: "villa cluster", family: Luxury, cluster: 0.5, draw: |g, c, r, _| residential::villas(g, c, r) },
    // commercial
    Shophouse { label: "shophouses", family: Retail, cluster: 0.8, draw: |g, c, r, _| commercial::shophouse(g, c, r) },
    MarketHall { label: "market hall", family: Retail, cluster: -0.4, draw: |g, c, r, _| commercial::market_hall(g, c, r) },
    RetailBlock { label: "retail block", family: Retail, cluster: 0.2, draw: |g, c, r, _| commercial::retail_block(g, c, r) },
    PodiumOffice { label: "podium office", family: Office, cluster: 0.1, draw: |g, c, r, _| commercial::podium_office(g, c, r) },
    SlabTower { label: "corporate slab", family: Spire, cluster: -0.8, draw: |g, c, r, _| commercial::slab_tower(g, c, r) },
    TwistTower { label: "twist tower", family: Spire, cluster: -2.0, draw: |g, c, r, _| commercial::twist_tower(g, c, r) },
    CantileverMall { label: "cantilever mall", family: Spire, cluster: -1.0, draw: |g, c, r, _| commercial::cantilever_mall(g, c, r) },
    // industrial
    Junkyard { label: "junkyard", family: Heavy, cluster: 0.8, draw: |g, c, r, _| industrial::junkyard(g, c, r) },
    RustyWarehouse { label: "rusted warehouse", family: Heavy, cluster: 0.6, draw: |g, c, r, _| industrial::rusty_warehouse(g, c, r) },
    Factory { label: "factory", family: Heavy, cluster: 0.4, draw: |g, c, r, _| industrial::factory(g, c, r) },
    TankFarm { label: "tank farm", family: Heavy, cluster: 0.5, draw: |g, c, r, _| industrial::tank_farm(g, c, r) },
    Datacenter { label: "datacentre", family: Tech, cluster: 0.3, draw: |g, c, r, _| industrial::datacenter(g, c, r) },
    Depot { label: "container depot", family: Tech, cluster: 0.3, draw: |g, c, r, _| industrial::depot(g, c, r) },
    // entertainment
    ShackBar { label: "shack bar", family: Night, cluster: 0.6, draw: |g, c, r, _| entertainment::shack_bar(g, c, r) },
    Club { label: "club", family: Night, cluster: 0.0, draw: |g, c, r, _| entertainment::club(g, c, r) },
    Cinema { label: "cinema", family: Night, cluster: -0.6, draw: |g, c, r, _| entertainment::cinema(g, c, r) },
    Casino { label: "casino", family: Night, cluster: -1.0, draw: |g, c, r, _| entertainment::casino(g, c, r) },
    Arena { label: "arena", family: Night, cluster: -2.0, draw: |g, c, r, _| entertainment::arena(g, c, r) },
    // civic
    Precinct { label: "precinct house", family: Special, cluster: 0.0, draw: |g, c, r, _| civic::precinct(g, c, r) },
    Ministry { label: "ministry", family: Special, cluster: 0.0, draw: |g, c, r, _| civic::ministry(g, c, r) },
    Library { label: "library", family: Special, cluster: 0.0, draw: |g, c, r, _| civic::library(g, c, r) },
    Courthouse { label: "courthouse", family: Special, cluster: 0.0, draw: |g, c, r, _| civic::courthouse(g, c, r) },
    Monument { label: "monument", family: Special, cluster: 0.0, draw: |g, c, r, _| civic::monument(g, c, r) },
    // park
    ScrubLot { label: "scrub lot", family: Special, cluster: 0.0, draw: |g, c, r, _| park::scrub_lot(g, c, r) },
    Garden { label: "garden", family: Special, cluster: 0.0, draw: |g, c, r, _| park::garden(g, c, r) },
    Plaza { label: "plaza", family: Special, cluster: 0.0, draw: |g, c, r, _| park::plaza(g, c, r) },
    // parking businesses
    ParkingLot { label: "parking lot", family: Special, cluster: 0.0, draw: |g, c, r, _| parking::parking_lot(g, c, r) },
    CarPark { label: "car park", family: Special, cluster: 0.0, draw: |g, c, r, _| parking::car_park(g, c, r) },
    // power plants
    PowerCoal { label: "coal plant", family: Special, cluster: 0.0, draw: |g, c, r, _| power::coal(g, c, r) },
    PowerSolar { label: "solar farm", family: Special, cluster: 0.0, draw: |g, c, r, _| power::solar(g, c, r) },
    PowerWind { label: "wind farm", family: Special, cluster: 0.0, draw: |g, c, r, _| power::wind(g, c, r) },
    PowerNuclear { label: "nuclear plant", family: Special, cluster: 0.0, draw: |g, c, r, _| power::nuclear(g, c, r) },
    // transit
    BusDepot { label: "bus depot", family: Special, cluster: 0.0, draw: |g, c, r, _| transit::bus_depot(g, c, r) },
    Station { label: "railway station", family: Special, cluster: 0.0, draw: |g, c, r, _| rail::station(g, c, r) },
    FreightYard { label: "freight yard", family: Special, cluster: 0.0, draw: |g, c, r, _| rail::freight_yard(g, c, r) },
    // -- variety: Hong Kong, Akira and Ghost-in-the-Shell forms. Added as
    // one block, appended so `ALL` keeps its order. --
    /// Kowloon walled block: abutting slabs of mismatched height, cages,
    /// AC units and an antenna forest.
    WalledCity { label: "walled block", family: Slum, cluster: 1.1, draw: |g, c, r, _| dense::walled_city(g, c, r) },
    /// Hong Kong pencil tower on a shop podium.
    PencilTower { label: "pencil tower", family: Slab, cluster: 0.6, draw: |g, c, r, _| towers::pencil_tower(g, c, r) },
    /// Shanghai twin slabs with crowned tops and a link bridge.
    TwinSlab { label: "twin slabs", family: Slab, cluster: 0.9, draw: |g, c, r, _| towers::twin_slab(g, c, r) },
    /// Akira megastructure: a heavy base, several towers, sky bridges.
    Megastructure { label: "megastructure", family: Spire, cluster: -2.2, draw: |g, c, r, _| towers::megastructure(g, c, r) },
    /// Ghost-in-the-Shell stack of offset glass boxes with planted terraces.
    GlassTerraces { label: "glass terraces", family: Spire, cluster: -1.0, draw: |g, c, r, _| towers::glass_terraces(g, c, r) },
    /// Capsule hotel: pods plugged into a concrete core.
    CapsuleHotel { label: "capsule hotel", family: Slum, cluster: 0.3, draw: |g, c, r, _| dense::capsule_hotel(g, c, r) },
    /// Terraced housing stepping up from the street like a hillside.
    HillsideSteps { label: "stepped terraces", family: Walkup, cluster: 0.7, draw: |g, c, r, _| dense::hillside_steps(g, c, r) },
    /// Covered night market arcade with stalls, lanterns and string lights.
    NightMarket { label: "night market", family: Night, cluster: 0.7, draw: |g, c, r, _| dense::night_market(g, c, r) },
    /// Karaoke / pachinko tower wrapped in signs and screens.
    NeonTower { label: "neon tower", family: Night, cluster: 0.2, draw: |g, c, r, _| dense::neon_tower(g, c, r) },
    /// Row of garage workshops and chop shops.
    Workshops { label: "workshops", family: Heavy, cluster: 0.9, draw: |g, c, r, _| works::workshops(g, c, r) },
    /// Hong Kong flatted factory: stacked industrial floors and a goods lift.
    FlattedFactory { label: "flatted factory", family: Heavy, cluster: 0.5, draw: |g, c, r, _| works::flatted_factory(g, c, r) },
    /// Clean semiconductor fab: white boxes, exhaust stacks and pipe racks.
    FabPlant { label: "fab plant", family: Tech, cluster: -0.3, draw: |g, c, r, _| works::fab_plant(g, c, r) },
    // landscape: water and hot-ground plants, docks
    // Water and hot-ground plants and docks: the city's own, neutral;
    // a container port sits with heavy industry.
    PowerHydro { label: "hydro plant", family: Special, cluster: 0.0, draw: |g, c, r, _| waterworks::hydro(g, c, r) },
    PowerTidal { label: "tidal plant", family: Special, cluster: 0.0, draw: |g, c, r, _| waterworks::tidal(g, c, r) },
    PowerGeothermal { label: "geothermal plant", family: Special, cluster: 0.0, draw: |g, c, r, _| waterworks::geothermal(g, c, r) },
    ShippingPort { label: "shipping port", family: Heavy, cluster: -0.5, draw: |g, c, r, _| waterworks::shipping_port(g, c, r) },
    FerryTerminal { label: "ferry terminal", family: Special, cluster: 0.0, draw: |g, c, r, _| waterworks::ferry_terminal(g, c, r) },
    // city services and attractions (buildings/services.rs), placed by the player
    // City services and attractions are placed by the player, never
    // picked by weight: neutral neighbours, and a landmark or stadium
    // should not repeat next door.
    PoliceStation { label: "police station", family: Special, cluster: 0.0, draw: |g, c, r, _| services::police_station(g, c, r) },
    FireStation { label: "fire station", family: Special, cluster: 0.0, draw: |g, c, r, _| services::fire_station(g, c, r) },
    Hospital { label: "hospital", family: Special, cluster: 0.0, draw: |g, c, r, _| services::hospital(g, c, r) },
    School { label: "school", family: Special, cluster: 0.0, draw: |g, c, r, _| services::school(g, c, r) },
    DispatchCenter { label: "911 dispatch centre", family: Special, cluster: 0.0, draw: |g, c, r, _| services::dispatch_center(g, c, r) },
    Landmark { label: "landmark", family: Special, cluster: -1.5, draw: |g, c, r, _| monuments::landmark(g, c, r) },
    Stadium { label: "stadium", family: Special, cluster: -1.5, draw: |g, c, r, _| services::stadium(g, c, r) },
    Prison { label: "jail", family: Special, cluster: -1.0, draw: |g, c, r, _| services::prison(g, c, r) },
    // -- industry sub-zones (`nd_sim::industry`): forced by the business on
    // the lot, never drawn from the land use's menu. Appended as one block. --
    // Industry sub-zones: farms spread in big patches, oil and
    // refineries are heavy, yards and lots line up together.
    /// Fields in rows, a barn, silos and a greenhouse.
    Farmstead { label: "farm", family: Farm, cluster: 0.9, draw: |g, c, r, _| industrial::farmstead(g, c, r) },
    /// Pumpjacks and a derrick over stained ground.
    OilField { label: "oil field", family: Heavy, cluster: 0.9, draw: |g, c, r, _| industrial::oil_field(g, c, r) },
    /// Distillation columns, pipe racks, tanks and flares.
    Refinery { label: "refinery", family: Heavy, cluster: 0.3, draw: |g, c, r, _| industrial::refinery(g, c, r) },
    /// Container stacks with a gantry and a loading lane.
    ContainerYard { label: "container yard", family: Logistics, cluster: 0.8, draw: |g, c, r, _| industrial::container_yard(g, c, r) },
    /// Processing sheds, an ice plant, crates and a boat on blocks.
    FishProcessing { label: "fish processing", family: Logistics, cluster: 0.6, draw: |g, c, r, _| industrial::fish_processing(g, c, r) },
    /// Marked bays with parked lorries and buses, a fuel island.
    TruckLot { label: "truck and bus lot", family: Logistics, cluster: 0.2, draw: |g, c, r, _| industrial::truck_lot(g, c, r) },
    // [society] placed by the player; appended so `ALL` keeps its order.
    // [society] Lorries and bins: it sits with the dirty industry.
    TrashDepot { label: "trash depot", family: Heavy, cluster: -0.5, draw: |g, c, r, _| sanitation::trash_depot(g, c, r) },
    // air transport, placed by the player (buildings/air.rs)
    // A vertiport serves the towers: an office-district neighbour that
    // should not repeat next door.
    Vertiport { label: "vertiport", family: Office, cluster: -1.2, draw: |g, c, r, _| air::vertiport(g, c, r) },
    // [subway] metro entrances and the Central Station
    MetroEntrance { label: "metro entrance", family: Special, cluster: 0.0, draw: |g, c, r, _| subway::metro_entrance(g, c, r) },
    CentralStation { label: "Central Station", family: Special, cluster: 0.0, draw: |g, c, r, _| subway::central_station(g, c, r) },
    // [power] grid battery packs, placed by the player
    PowerBattery { label: "battery pack", family: Special, cluster: 0.0, draw: |g, c, r, _| power::battery(g, c, r) },
    // -- [economy] businesses and waterfront docks (`nd_sim::market`,
    // `nd_sim::ports`): forced by the business or the dock, never drawn
    // from a land use's menu. Appended so saved indices keep their order. --
    /// Garage bays with roll-up doors, a car on a lift, tyre stacks.
    Mechanic { label: "car repair shop", family: Retail, cluster: 0.3, draw: |g, c, r, _| trades::mechanic(g, c, r) },
    /// Showroom and a forecourt of matching new cars, bunting and flags.
    CarDealer { label: "car dealership", family: Retail, cluster: 0.5, draw: |g, c, r, _| trades::car_dealer(g, c, r) },
    /// Yard of garbage trucks, dumpsters and a shed.
    TrashCompany { label: "trash company", family: Logistics, cluster: -0.2, draw: |g, c, r, _| rural::trash_company(g, c, r) },
    /// Narrow shopfront with bikes in racks outside.
    BikeShop { label: "bike shop", family: Retail, cluster: 0.2, draw: |g, c, r, _| trades::bike_shop(g, c, r) },
    // [economy] The new trades. Farms spread, the dirty trades sit with
    // heavy industry, yards with logistics, shops and nights out with
    // their kind; docks are the city's own.
    /// Barn, silo and a fenced pasture of cows.
    CowFarm { label: "cattle farm", family: Farm, cluster: 0.8, draw: |g, c, r, _| rural::cow_farm(g, c, r) },
    /// Long low chicken sheds with vents and feed silos.
    ChickenFarm { label: "chicken farm", family: Farm, cluster: 0.6, draw: |g, c, r, _| rural::chicken_farm(g, c, r) },
    /// Windowless block, holding pens and refrigerated trucks.
    Slaughterhouse { label: "slaughterhouse", family: Heavy, cluster: -0.3, draw: |g, c, r, _| rural::slaughterhouse(g, c, r) },
    /// Terraced open pit, haul trucks, a conveyor and spoil heaps.
    StripMine { label: "strip mine", family: Heavy, cluster: 0.4, draw: |g, c, r, _| rural::strip_mine(g, c, r) },
    /// Gutting sheds, an ice plant, crates and a reefer van.
    FishGutting { label: "fish gutting sheds", family: Logistics, cluster: 0.5, draw: |g, c, r, _| rural::fish_gutting(g, c, r) },
    /// Factory with a freezer block, a chimney and a fish-stick sign.
    FishStickFactory { label: "fish-stick factory", family: Heavy, cluster: 0.0, draw: |g, c, r, _| rural::fish_stick_factory(g, c, r) },
    /// Open-sided hall of stalls under awnings, crates of fish.
    FishMarket { label: "fish market", family: Retail, cluster: -0.5, draw: |g, c, r, _| trades::fish_market(g, c, r) },
    /// Restaurant with a deck, string lights and a fish sign.
    SeafoodRestaurant { label: "seafood restaurant", family: Night, cluster: 0.3, draw: |g, c, r, _| trades::seafood_restaurant(g, c, r) },
    /// Two-storey tee bays, a green field, nets on poles and floodlights.
    GolfRange { label: "driving range", family: Night, cluster: -1.5, draw: |g, c, r, _| trades::golf_range(g, c, r) },
    /// Converted brick warehouse with big windows, murals and sculptures.
    ArtStudio { label: "artists' studios", family: Night, cluster: 0.6, draw: |g, c, r, _| trades::art_studio(g, c, r) },
    /// White cube with a glass front, a banner and a sculpture.
    Gallery { label: "gallery", family: Night, cluster: 0.4, draw: |g, c, r, _| trades::gallery(g, c, r) },
    /// Land side of a small marina: boathouse, boat trailers, parking.
    MarinaSmall { label: "small marina", family: Special, cluster: 0.0, draw: |g, c, r, _| docks::marina_small(g, c, r) },
    /// Land side of a large marina: clubhouse terrace, yacht on a cradle, flagpoles.
    MarinaLarge { label: "large marina", family: Special, cluster: 0.0, draw: |g, c, r, _| docks::marina_large(g, c, r) },
    /// Land side of a small fishing port: quay shed, nets, crates.
    FishingPortSmall { label: "small fishing port", family: Special, cluster: 0.0, draw: |g, c, r, _| docks::fishing_port_small(g, c, r) },
    /// Land side of a large fishing port: ice house, crane, big shed, crates.
    FishingPortLarge { label: "large fishing port", family: Special, cluster: 0.0, draw: |g, c, r, _| docks::fishing_port_large(g, c, r) },
    // -- [land] homes by class and density, and the forms bought and merged
    // plots are built in (`buildings/estates.rs`). Appended as one block so
    // `ALL` keeps its order. --
    /// One-storey shacks of tin and tarp: the poor on quiet land.
    Shacks { label: "shacks", family: Special, cluster: 0.0, draw: |g, c, r, _| estates::shacks(g, c, r) },
    /// Mobile homes off a gravel lane.
    TrailerPark { label: "trailer park", family: Special, cluster: 0.0, draw: |g, c, r, _| estates::trailer_park(g, c, r) },
    /// Round thatched huts round a fire pit.
    Huts { label: "huts", family: Special, cluster: 0.0, draw: |g, c, r, _| estates::huts(g, c, r) },
    /// Detached houses on lawns: the middle class's suburb.
    Suburb { label: "suburban houses", family: Special, cluster: 0.0, draw: |g, c, r, _| estates::suburb(g, c, r) },
    /// Two plots, one big house and a pool (`estate::Form::McMansion`).
    McMansion { label: "McMansion", family: Special, cluster: 0.0, draw: |g, c, r, _| estates::mcmansion(g, c, r) },
    /// A billionaire's estate (`estate::Form::Mansion`).
    Mansion { label: "mansion", family: Special, cluster: 0.0, draw: |g, c, r, _| estates::mansion(g, c, r) },
    /// Towers on a podium with valet parking, a roof pool and sky-bridges.
    MegaHotel { label: "mega hotel", family: Special, cluster: 0.0, draw: |g, c, r, _| estates::mega_hotel(g, c, r) },
    /// A gallery grown into a museum.
    Museum { label: "museum", family: Special, cluster: 0.0, draw: |g, c, r, _| estates::museum(g, c, r) },
    // -- [dirty-industry] the new dirty trades (`buildings/dirty.rs`),
    // appended so `ALL` keeps its order. --
    // [dirty-industry] with the heavy industry; feedlots with the farms
    // they feed off, but never repeating in a row of looks.
    /// Blast furnace, rusted casting hall, stacks, ore and slag heaps.
    Smelter { label: "smelter", family: Heavy, cluster: 0.3, draw: |g, c, r, _| dirty::smelter(g, c, r) },
    /// Battery sheds, packed mud pens and a manure lagoon.
    Feedlot { label: "factory farm", family: Farm, cluster: -0.2, draw: |g, c, r, _| dirty::feedlot(g, c, r) },
    /// Cell houses, banded tank rows, a stack and a poisoned pond.
    ChemicalWorks { label: "chemical works", family: Heavy, cluster: 0.2, draw: |g, c, r, _| dirty::chemical_works(g, c, r) },
    /// [supertall] A league tower on its podium (`buildings/supertall.rs`):
    /// its form comes from `nd_sim::towers::Tower::design`.
    Supertall { label: "league tower", family: Special, cluster: 0.0, draw: |g, c, r, e| match &e.tower {
            Some(t) => supertall::tower(g, c, t),
            // A tower without its record (a preview): a plain glass slab.
            None => commercial::slab_tower(g, c, r),
        } },
    /// [megacorp] A university's main hall, portico, clock tower and quad.
    University { label: "university hall", family: Special, cluster: -1.0, draw: |g, c, r, _| brand::university(g, c, r) },
    // -- [waterfront] the shore's looks (`buildings/shore.rs`), appended so
    // `ALL` keeps its order. --
    /// A public beach: sand, towels, umbrellas, a lifeguard tower, bathers.
    Beach { label: "public beach", family: Special, cluster: 0.0, draw: |g, c, _, _| shore::beach(g, c, c.shore) },
    // [waterfront] The shore's trades line the water together.
    /// Lobby block, wings stepping down to the water round a lit pool, cabanas.
    BeachResort { label: "beach resort", family: Night, cluster: 0.4, draw: |g, c, r, _| shore::beach_resort(g, c, r, c.shore) },
    /// A tower with a balcony per room facing the water and a roof pool.
    WaterfrontHotel { label: "waterfront hotel", family: Night, cluster: 0.4, draw: |g, c, r, _| shore::waterfront_hotel(g, c, r, c.shore) },
    /// A glass pavilion at the water's edge; its deck is its pier.
    PierRestaurant { label: "pier restaurant", family: Night, cluster: 0.6, draw: |g, c, r, _| shore::pier_restaurant(g, c, r, c.shore) },
    /// A thatched shack with a bar, torches and string lights on the sand.
    BeachBar { label: "beach bar", family: Night, cluster: 0.6, draw: |g, c, r, _| shore::beach_bar(g, c, r, c.shore) },
    /// A kiosk, kayak racks and pedalos; its pontoon is its jetty.
    BoatRental { label: "boat rental", family: Night, cluster: 0.6, draw: |g, c, r, _| shore::boat_rental(g, c, r, c.shore) },
    // -- [pt-visual] brutalist civic buildings, appended so `ALL` keeps its order. --
    /// Battered concrete, slit windows, blast doors under a visor, searchlights.
    CivicBunker { label: "civil defence bunker", family: Special, cluster: -0.3, draw: |g, c, r, _| civic::bunker(g, c, r) },
    /// An inverted ziggurat of concrete fins over a columned lobby, the emblem on the crown.
    CivicTower { label: "ministry tower", family: Special, cluster: -0.6, draw: |g, c, r, _| civic::civic_tower(g, c, r) },
    /// A near windowless monolith with lit slits, cameras and an eye over the door.
    Panopticon { label: "surveillance bureau", family: Special, cluster: -0.8, draw: |g, c, r, _| civic::panopticon(g, c, r) },
}

impl Style {
    /// Position in `ALL` (and in [`STYLES`]), for per-style tables.
    pub fn index(self) -> usize {
        self as usize
    }

    /// This style's row of [`STYLES`].
    pub fn def(self) -> &'static StyleDef {
        &STYLES[self as usize]
    }

    /// What the inspector and the remote API call it.
    pub fn label(self) -> &'static str {
        self.def().label
    }

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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// The table is the whole enum, in order: row `i` is the style whose
    /// index is `i`, every style once, every label distinct.
    #[test]
    fn the_style_table_is_complete_and_in_order() {
        assert_eq!(STYLES.len(), Style::ALL.len());
        let mut labels = HashSet::new();
        for (i, d) in STYLES.iter().enumerate() {
            assert_eq!(d.style.index(), i, "{:?} sits at row {i}", d.style);
            assert_eq!(Style::ALL[i], d.style);
            assert!(!d.label.is_empty() && labels.insert(d.label), "{:?}: label {:?} empty or used twice", d.style, d.label);
            assert!(d.cluster.is_finite());
        }
    }
}
