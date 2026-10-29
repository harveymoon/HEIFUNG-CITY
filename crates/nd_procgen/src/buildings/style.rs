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
    // -- [attractions] the landmark attractions (`buildings/attractions.rs`),
    // placed by the player, one of each per city; appended so `ALL` keeps
    // its order. --
    /// White galleries round a sculpture court, a spiral drum ringed in neon.
    ArtMuseum { label: "art museum", family: Special, cluster: -1.5, draw: |g, c, r, _| attractions::art_museum(g, c, r) },
    /// A vaulted hall, a planetarium dome, a skeleton and a rocket outside.
    ScienceMuseum { label: "natural history and science museum", family: Special, cluster: -1.5, draw: |g, c, r, _| attractions::science_museum(g, c, r) },
    /// A colonnade under a pediment, a clock tower, a tram on show.
    HistoryMuseum { label: "city history and design museum", family: Special, cluster: -1.5, draw: |g, c, r, _| attractions::history_museum(g, c, r) },
    /// The pleasure pier: a gateway sign ashore, the deck out on piles.
    PleasurePier { label: "pleasure pier", family: Special, cluster: -1.5, draw: |g, c, r, _| attractions::pier(g, c, r) },
    /// A grand park: lakes, paths, a glass pavilion, a boathouse, a gate.
    GrandPark { label: "grand city park", family: Special, cluster: -1.5, draw: |g, c, r, _| attractions::grand_park(g, c, r) },
    /// Enclosures, animals, an aviary dome and a gate.
    Zoo { label: "zoo", family: Special, cluster: -1.5, draw: |g, c, r, _| attractions::zoo(g, c, r) },
    /// Telescope domes on a terrace over the city.
    Observatory { label: "observatory", family: Special, cluster: -1.5, draw: |g, c, r, _| attractions::observatory(g, c, r) },
    /// A wave-roofed hall glowing blue, a shark on the roof.
    Aquarium { label: "aquarium", family: Special, cluster: -1.5, draw: |g, c, r, _| attractions::aquarium(g, c, r) },
    /// Gardens round a great glass conservatory.
    BotanicalGarden { label: "botanical garden", family: Special, cluster: -1.5, draw: |g, c, r, _| attractions::botanical_garden(g, c, r) },
    /// White sails over a glass foyer on a podium.
    ConcertHall { label: "opera and concert hall", family: Special, cluster: -1.5, draw: |g, c, r, _| attractions::concert_hall(g, c, r) },
    /// (D4) A coaster, a ferris wheel, a drop tower and rides round a midway.
    AmusementPark { label: "amusement park", family: Special, cluster: -1.5, draw: |g, c, r, _| attractions::amusement_park(g, c, r) },
    /// (D4) Exhibition halls under a barrel roof, a glass front, flags.
    ConventionCentre { label: "convention centre", family: Special, cluster: -1.5, draw: |g, c, r, _| attractions::convention_centre(g, c, r) },
    /// (D4) A nave, twin spired towers, a rose window, a square.
    Cathedral { label: "cathedral", family: Special, cluster: -1.5, draw: |g, c, r, _| attractions::cathedral(g, c, r) },
    /// (D4) A crescent hotel tower over a neon casino and a pool deck.
    CasinoResort { label: "casino resort", family: Special, cluster: -1.5, draw: |g, c, r, _| attractions::casino_resort(g, c, r) },
    // -- [capsule] the Capsule district's white domes (`buildings/capsule.rs`),
    // drawn only from its look's menus; appended so `ALL` keeps its order. --
    // Low domes repeat happily along a street; the towers stand apart.
    /// A dome on a drum ringed with portholes, a round door, a garage dome or pad.
    DomeHouse { label: "dome house", family: Luxury, cluster: 0.8, draw: |g, c, r, _| capsule::dome_house(g, c, r) },
    /// A saucer raised on a core and four legs, portholes round its rim.
    SaucerHouse { label: "saucer house", family: Luxury, cluster: 0.6, draw: |g, c, r, _| capsule::saucer_house(g, c, r) },
    /// A ball on a single thick stilt, like a lolly, over a lobby dome.
    PopsicleDome { label: "stilt dome", family: Spire, cluster: 0.2, draw: |g, c, r, _| capsule::popsicle_dome(g, c, r) },
    /// Two or three domes of different sizes joined by glass tubes.
    TwinDomes { label: "twin domes", family: Luxury, cluster: 0.5, draw: |g, c, r, _| capsule::twin_domes(g, c, r) },
    /// A main dome with small pods round it on short glass tubes.
    DomePods { label: "dome with pods", family: Luxury, cluster: 0.5, draw: |g, c, r, _| capsule::dome_pods(g, c, r) },
    /// A pill-shaped tower: a porthole row every floor, a rounded top.
    CapsuleTower { label: "capsule tower", family: Spire, cluster: -0.4, draw: |g, c, r, e| capsule::capsule_tower(g, c, r, e.rooftop_pad) },
    /// Saucers stacked round a white core, a radar ball on top.
    SaucerStack { label: "saucer stack", family: Spire, cluster: -0.6, draw: |g, c, r, e| capsule::saucer_stack(g, c, r, e.rooftop_pad) },
    /// A tall stilt with a big domed pod holding the top floors.
    StiltTower { label: "stilt tower", family: Spire, cluster: -1.2, draw: |g, c, r, e| capsule::stilt_tower(g, c, r, e.rooftop_pad) },
    /// A big shallow dome over a glass drum, a curved glass canopy, a pad.
    DomeHall { label: "dome hall", family: Retail, cluster: -0.3, draw: |g, c, r, _| capsule::dome_hall(g, c, r) },
    /// A round office drum ribboned with glass, a radar ball, a lab dome.
    DomeCampus { label: "dome campus", family: Office, cluster: 0.1, draw: |g, c, r, _| capsule::dome_campus(g, c, r) },
    // -- [malls] strip malls, malls and mega-malls (`buildings/malls.rs`),
    // drawn only for their `Form`; appended so `ALL` keeps its order. --
    /// A row of shopfronts behind a small car park, a pylon sign by the street.
    StripMall { label: "strip mall", family: Retail, cluster: 0.3, draw: |g, c, r, e| malls::strip_mall(g, c, r, e) },
    /// A two-level block of shops round its car park, a glass atrium.
    Mall { label: "mall", family: Special, cluster: -1.0, draw: |g, c, r, e| malls::mall(g, c, r, e) },
    /// Anchor stores, a food court and a parking structure.
    MegaMall { label: "mega-mall", family: Special, cluster: -1.5, draw: |g, c, r, e| malls::mega_mall(g, c, r, e) },
    // -- [r4-brick] the Brick & Steel look of Foundry Row (`buildings/brick.rs`,
    // `brick_works.rs`), drawn only from its look's menus (and the loft
    // estate for a big house there); appended so `ALL` keeps its order. --
    /// A terrace of red-brick worker houses, chimney stacks, back yards.
    BrickRow { label: "brick worker row", family: Walkup, cluster: 0.9, draw: |g, c, r, _| brick::brick_row(g, c, r) },
    /// A brick warehouse of lofts: steel windows, a hoist, a water tower.
    WarehouseLoft { label: "warehouse loft", family: Walkup, cluster: 0.5, draw: |g, c, r, _| brick::warehouse_loft(g, c, r) },
    /// A converted mill: a long loft block, a stair tower with a clock, a chimney.
    MillLofts { label: "mill lofts", family: Slab, cluster: 0.1, draw: |g, c, r, _| brick::mill_lofts(g, c, r) },
    /// A brick workshop: big doors, a name board, a yard and a van.
    BrickWorkshop { label: "brick workshop", family: Retail, cluster: 0.4, draw: |g, c, r, _| brick::brick_workshop(g, c, r) },
    /// The corner pub: tiled front, warm windows, a hung sign, a beer garden.
    CornerPub { label: "corner pub", family: Night, cluster: -0.4, draw: |g, c, r, _| brick::corner_pub(g, c, r) },
    /// The union hall: a gabled hall, banners, a round window, a flag.
    UnionHall { label: "union hall", family: Special, cluster: -1.0, draw: |g, c, r, _| brick::union_hall(g, c, r) },
    /// A saw-tooth brick factory, its north lights glowing after dark.
    SawtoothFactory { label: "saw-tooth factory", family: Heavy, cluster: 0.6, draw: |g, c, r, _| brick_works::sawtooth_factory(g, c, r) },
    /// A foundry: casting shed, cupola, chimneys, a goliath gantry, a siding.
    FoundryWorks { label: "foundry", family: Heavy, cluster: 0.2, draw: |g, c, r, _| brick_works::foundry_works(g, c, r) },
    /// A gas works: holders in their guide frames, a brick retort house.
    GasWorks { label: "gas works", family: Heavy, cluster: -0.6, draw: |g, c, r, _| brick_works::gas_works(g, c, r) },
    /// A loft estate: a made-over warehouse, a glass penthouse, a pool.
    LoftEstate { label: "loft estate", family: Special, cluster: 0.0, draw: |g, c, r, _| brick::loft_estate(g, c, r) },
    // -- [lantern] Lantern Alley, the Lantern Lanes story's look
    // (`buildings/lantern.rs`), drawn only from its menus (the compound for
    // its estates' forms); appended so `ALL` keeps its order. --
    // Shophouse rows, alleys, stalls and workshops repeat along a lane; the
    // halls and the tea house stand apart.
    /// Narrow shophouses, flats over a shop behind an arcade, lanterns along it.
    LanternShophouse { label: "lantern shophouses", family: Walkup, cluster: 1.0, draw: |g, c, r, _| lantern::shophouses(g, c, r) },
    /// Two walk-ups either side of an alley strung with lanterns and washing.
    AlleyWalkups { label: "alley walk-ups", family: Walkup, cluster: 0.8, draw: |g, c, r, _| lantern::alley_walkups(g, c, r) },
    /// Stall rows under patchwork tarps, lanterns across the aisles.
    StallRows { label: "stall rows", family: Night, cluster: 0.9, draw: |g, c, r, _| lantern::stall_rows(g, c, r) },
    /// An open hall of kitchens and tables under a tin gable (a cooked-food hall upstairs on dear land).
    FoodCourt { label: "food court hall", family: Retail, cluster: -0.4, draw: |g, c, r, _| lantern::food_court(g, c, r) },
    /// Storeys under stacked tile skirts, lacquered columns, lanterns under the eaves.
    TeaHouse { label: "tea house", family: Night, cluster: -0.3, draw: |g, c, r, _| lantern::tea_house(g, c, r) },
    /// A painted gateway, a forecourt of stalls under lantern strings, a market hall.
    LanternMarket { label: "lantern night market", family: Night, cluster: 0.2, draw: |g, c, r, _| lantern::lantern_market(g, c, r) },
    /// Small workshops behind a work apron, roller doors, lean-tos, stock.
    AlleyWorkshops { label: "alley workshops", family: Heavy, cluster: 0.8, draw: |g, c, r, _| lantern::alley_workshops(g, c, r) },
    /// Kitchens behind a service alley: hoods, ducts, steam, gas bottles.
    BackKitchens { label: "back-alley kitchens", family: Heavy, cluster: 0.5, draw: |g, c, r, _| lantern::back_kitchens(g, c, r) },
    /// A family's or a billionaire's courtyard shophouse compound.
    LanternCompound { label: "courtyard shophouse compound", family: Special, cluster: 0.0, draw: |g, c, r, _| lantern::compound(g, c, r) },
    // -- [garden] the Garden Suburb look (`buildings/garden.rs`), drawn only
    // from its look's menus and for a family's or a billionaire's house in
    // it; appended so `ALL` keeps its order. Houses repeat along a street;
    // a garden centre stands apart. --
    /// Detached houses of many plans and roofs, gardens, fences, drives.
    GardenHouse { label: "garden house", family: Walkup, cluster: 0.9, draw: |g, c, r, _| garden::garden_house(g, c, r) },
    /// One-storey bungalows with carports.
    Bungalow { label: "bungalow", family: Walkup, cluster: 0.8, draw: |g, c, r, _| garden::bungalow(g, c, r) },
    /// A close of houses round a turning circle.
    CulDeSac { label: "cul-de-sac", family: Walkup, cluster: 0.3, draw: |g, c, r, _| garden::cul_de_sac(g, c, r) },
    /// A row of small shops with flats over them behind a forecourt.
    ShoppingParade { label: "shopping parade", family: Retail, cluster: 0.4, draw: |g, c, r, _| garden::shopping_parade(g, c, r) },
    /// A garden centre: shop, glasshouse, polytunnels, plant tables.
    GardenCentre { label: "garden centre", family: Retail, cluster: -1.0, draw: |g, c, r, _| garden::garden_centre(g, c, r) },
    /// A corner café with tables out under string lights.
    CornerCafe { label: "corner cafe", family: Retail, cluster: -0.3, draw: |g, c, r, _| garden::corner_cafe(g, c, r) },
    /// The big garden house behind tall hedges (`estate::Form::McMansion`
    /// and `Mansion` in the Garden Suburb).
    GardenManor { label: "garden manor", family: Special, cluster: 0.0, draw: |g, c, r, e| garden::garden_manor(g, c, r, e) },
    // -- [r4-harbour] Harbour Pastel, The Promenade's look
    // (`buildings/harbour.rs`), drawn only from its menus (the villa for its
    // estates' forms, the hotel, shack and hall for those trades there);
    // appended so `ALL` keeps its order. Terraces, sheds and arcades repeat
    // along the front; the corner tower, hotel and hall stand apart. --
    /// Narrow pastel houses, gables to the street, shutters, bulbs along the front.
    PastelTerrace { label: "pastel terrace", family: Walkup, cluster: 1.0, draw: |g, c, r, e| harbour::pastel_terrace(g, c, r, e) },
    /// Clapboard boathouses made homes: glazed boat doors, a slipway, a pier.
    Boathouses { label: "converted boathouses", family: Walkup, cluster: 0.6, draw: |g, c, r, e| harbour::boathouses(g, c, r, e) },
    /// Flats with a striped lighthouse tower on the corner, its lamp lit at night.
    LighthouseCorner { label: "lighthouse corner block", family: Walkup, cluster: -0.8, draw: |g, c, r, e| harbour::lighthouse_corner(g, c, r, e) },
    /// A café behind a raised boardwalk of tables, umbrellas and bulbs.
    BoardwalkCafe { label: "boardwalk café", family: Retail, cluster: 0.5, draw: |g, c, r, e| harbour::boardwalk_cafe(g, c, r, e) },
    /// A board shack with a lit hatch, a fish on the roof, picnic tables.
    SeafoodShack { label: "seafood shack", family: Night, cluster: 0.6, draw: |g, c, r, e| harbour::seafood_shack(g, c, r, e) },
    /// Shops behind white columns, each under its own striped awning.
    AwningArcade { label: "striped-awning arcade", family: Retail, cluster: 0.8, draw: |g, c, r, e| harbour::awning_arcade(g, c, r, e) },
    /// Balconies, a portico, flags out front and a mansard roof terrace.
    BoutiqueHotel { label: "boutique hotel", family: Luxury, cluster: -0.4, draw: |g, c, r, e| harbour::boutique_hotel(g, c, r, e) },
    /// A painted-iron hall with a clerestory, roller doors, crates of ice.
    FishMarketHall { label: "fish market hall", family: Retail, cluster: -0.6, draw: |g, c, r, e| harbour::fish_market_hall(g, c, r, e) },
    /// Tall tar-black net sheds, nets drying behind them.
    NetSheds { label: "net sheds", family: Heavy, cluster: 0.9, draw: |g, c, r, e| harbour::net_sheds(g, c, r, e) },
    /// A family's or a billionaire's pastel villa with its own boathouse.
    PastelVilla { label: "pastel waterfront villa", family: Special, cluster: 0.0, draw: |g, c, r, e| harbour::pastel_villa(g, c, r, e) },
    // -- [r4-oldstone] the Old Stone look of the Old Town Charter (the
    // Historic look's own buildings, `buildings/oldstone.rs`,
    // `oldstone_works.rs`), drawn only from its look's menus (and the stone
    // manor for a big house there); appended so `ALL` keeps its order. --
    /// Stone and jettied timber-frame town houses, a dormer, a front garden.
    StoneHouse { label: "stone house", family: Walkup, cluster: 0.7, draw: |g, c, r, _| oldstone::stone_house(g, c, r) },
    /// A row of narrow tall houses, gables to the street, crow steps.
    GabledRow { label: "gabled row", family: Walkup, cluster: 0.9, draw: |g, c, r, _| oldstone::gabled_row(g, c, r) },
    /// Ranges round a cobbled courtyard, an arched carriage gate, a well.
    CourtyardHouse { label: "courtyard house", family: Walkup, cluster: 0.3, draw: |g, c, r, _| oldstone::courtyard_house(g, c, r) },
    /// Storeys over the pavement on stone arches, shops in the arcade.
    MarketArcade { label: "market arcade", family: Retail, cluster: 0.8, draw: |g, c, r, _| oldstone::market_arcade(g, c, r) },
    /// A crow-stepped stone hall, a lit rose window, banners, a turret.
    GuildHall { label: "guild hall", family: Special, cluster: -1.2, draw: |g, c, r, _| oldstone::guild_hall(g, c, r) },
    /// A clock tower with lit faces and a spire, its church and churchyard.
    ClockTower { label: "clock tower", family: Special, cluster: -2.0, draw: |g, c, r, _| oldstone::clock_tower(g, c, r) },
    /// A coaching inn: jettied front, bay windows, the hung sign, the yard.
    OldInn { label: "old inn", family: Night, cluster: -0.3, draw: |g, c, r, _| oldstone::old_inn(g, c, r) },
    /// A stone mill with a lucam and a water wheel turning in its race.
    WaterMill { label: "old mill", family: Heavy, cluster: -0.8, draw: |g, c, r, _| oldstone_works::water_mill(g, c, r) },
    /// A tannery: a drying loft, pits of lime and bark liquor, hides on rails.
    Tannery { label: "tannery", family: Heavy, cluster: -0.2, draw: |g, c, r, _| oldstone_works::tannery(g, c, r) },
    /// A carter's or cooper's cobbled yard: workshops, a gateway, a cart.
    CobbledYard { label: "cobbled yard", family: Heavy, cluster: 0.5, draw: |g, c, r, _| oldstone_works::cobbled_yard(g, c, r) },
    /// A stone manor round a court: porch tower and clock, wings, a garden.
    StoneManor { label: "stone manor", family: Special, cluster: 0.0, draw: |g, c, r, _| oldstone::stone_manor(g, c, r) },
    // -- [r4-solarpunk] Solarpunk Green, Eco Village's look
    // (`buildings/solarpunk.rs`), drawn only from its menus (the villa for
    // its estates' forms, the vertical farm and the repair café for those
    // trades there); appended so `ALL` keeps its order. Terraces,
    // townhouses and greenhouses repeat; the tower, the hall and the depot
    // stand apart. --
    /// Narrow timber houses glazed floor by floor under planted roofs.
    TimberTerrace { label: "timber-and-glass terrace", family: Walkup, cluster: 1.0, draw: |g, c, r, _| solarpunk::timber_terrace(g, c, r) },
    /// Tall houses under steep roofs skinned in patched solar shingles.
    SolarTownhouses { label: "solar-shingle townhouses", family: Walkup, cluster: 0.7, draw: |g, c, r, _| solarpunk::solar_townhouses(g, c, r) },
    /// A concrete frame wrapped in balcony gardens, creepers, a roof greenhouse.
    VineTower { label: "vine-covered tower", family: Slab, cluster: -0.3, draw: |g, c, r, _| solarpunk::vine_tower(g, c, r) },
    /// An iron and dirty-glass hall, stalls out front, a farm on the roof behind.
    GlassMarketHall { label: "glass market hall", family: Retail, cluster: -0.6, draw: |g, c, r, _| solarpunk::glass_market_hall(g, c, r) },
    /// A timber shop behind a forecourt of crates under a solar canopy.
    CoopStore { label: "co-op store", family: Retail, cluster: 0.4, draw: |g, c, r, _| solarpunk::coop_store(g, c, r) },
    /// An old workshop with its door up on the benches, tables under bulbs.
    RepairCafe { label: "repair café", family: Night, cluster: 0.2, draw: |g, c, r, _| solarpunk::repair_cafe(g, c, r) },
    /// Growing floors behind green glass, magenta grow lights after dark.
    VerticalFarm { label: "vertical farm", family: Tech, cluster: -0.5, draw: |g, c, r, _| solarpunk::vertical_farm(g, c, r) },
    /// Long glasshouses on gravel, sodium-lit after dark, a packing shed.
    GreenhouseRows { label: "greenhouse rows", family: Farm, cluster: 0.8, draw: |g, c, r, _| solarpunk::greenhouse_rows(g, c, r) },
    /// Cargo bikes on charge under a solar canopy, wind trees in the yard.
    BikeDepot { label: "cargo-bike depot", family: Logistics, cluster: -0.4, draw: |g, c, r, _| solarpunk::bike_depot(g, c, r) },
    /// A family's or a billionaire's planted solar villa with a roof garden.
    SolarVilla { label: "planted solar villa", family: Special, cluster: 0.0, draw: |g, c, r, _| solarpunk::solar_villa(g, c, r) },
    // -- [r4-glass] Glass Campus, Founders' Campus's look
    // (`buildings/glasscampus.rs`), drawn only from its menus (the pavilion
    // for its estates' forms, the server hall for the assembly halls
    // there); appended so `ALL` keeps its order. Flats, sheds and halls
    // repeat; the tower, the atrium office and the quad stand apart. --
    /// A slab of student flats: bright panels by every window, galleries behind.
    StudentFlats { label: "student flats", family: Slab, cluster: 0.8, draw: |g, c, r, e| glasscampus::student_flats(g, c, r, e) },
    /// A tinted glass tower on a podium, lounges hung off it, a braced stair core.
    CoLivingTower { label: "co-living tower", family: Slab, cluster: -0.4, draw: |g, c, r, e| glasscampus::coliving_tower(g, c, r, e) },
    /// A glass lab in an exposed steel frame, fume stacks on the roof.
    GlassLab { label: "glass lab", family: Tech, cluster: 0.4, draw: |g, c, r, e| glasscampus::glass_lab(g, c, r, e) },
    /// A yard of cheap prefab sheds, some stacked, lit late, a shared deck.
    StartupSheds { label: "start-up sheds", family: Tech, cluster: 0.7, draw: |g, c, r, e| glasscampus::startup_sheds(g, c, r, e) },
    /// Tinted office slab with a full-height atrium lit all night, a screen in it.
    AtriumOffice { label: "atrium office", family: Office, cluster: -0.3, draw: |g, c, r, e| glasscampus::atrium_office(g, c, r, e) },
    /// Low glass wings round a lawn, a big screen, a cafe pavilion.
    CampusQuad { label: "shared quad", family: Office, cluster: -1.0, draw: |g, c, r, e| glasscampus::campus_quad(g, c, r, e) },
    /// A chamfered concrete research lab: ribbon windows, louvres, stacks.
    ResearchLab { label: "research lab", family: Tech, cluster: 0.3, draw: |g, c, r, e| glasscampus::research_lab(g, c, r, e) },
    /// A fenced windowless server hall, chillers on the roof, generators.
    ServerHall { label: "server hall", family: Tech, cluster: 0.6, draw: |g, c, r, e| glasscampus::server_hall(g, c, r, e) },
    /// A family's or a billionaire's glass pavilion house.
    GlassPavilion { label: "glass pavilion house", family: Special, cluster: 0.0, draw: |g, c, r, e| glasscampus::glass_pavilion(g, c, r, e) },
    // -- [r4-skyline] Skyline Luxe, Neo Tokyo Heights' look
    // (`buildings/skyline.rs`), drawn only from its menus (the sky villa for
    // its estates' forms, the helipad tower for its hotels); appended so
    // `ALL` keeps its order. The towers stand apart; the boutique row and
    // the mall line the street. --
    /// A slim glass residence tower on a podium of noisy shops, glass boxes hung off it.
    LuxeTower { label: "glass residence tower", family: Spire, cluster: -0.3, draw: |g, c, r, e| skyline::luxe_tower(g, c, r, e) },
    /// Two glass towers on one podium joined by a lit sky bridge.
    TwinTowers { label: "twin towers with a sky bridge", family: Spire, cluster: -0.8, draw: |g, c, r, e| skyline::twin_towers(g, c, r, e) },
    /// A glass tower with an infinity pool and cabanas on its roof.
    PoolTower { label: "rooftop pool tower", family: Spire, cluster: -0.5, draw: |g, c, r, e| skyline::pool_tower(g, c, r, e) },
    /// A hotel or office tower with a helipad over its roof, a brand sign under it.
    HelipadTower { label: "helipad tower", family: Spire, cluster: -0.7, draw: |g, c, r, e| skyline::helipad_tower(g, c, r, e) },
    /// A vertical mall: stacked levels of signs and screens, an atrium lit all night.
    MallPodium { label: "vertical mall", family: Retail, cluster: -0.4, draw: |g, c, r, e| skyline::mall_podium(g, c, r, e) },
    /// Luxury boutiques under neon, the street's vending machines and mopeds between.
    BoutiqueRow { label: "boutique row", family: Retail, cluster: 0.6, draw: |g, c, r, e| skyline::boutique_row(g, c, r, e) },
    /// A family's or a billionaire's sky villa: a glass penthouse and pool on a low block.
    SkyVilla { label: "sky villa", family: Special, cluster: 0.0, draw: |g, c, r, e| skyline::sky_villa(g, c, r, e) },
}

impl Style {
    /// [malls] The mall styles, which draw their own shop signs.
    pub fn is_mall(self) -> bool {
        matches!(self, Style::StripMall | Style::Mall | Style::MegaMall)
    }

    /// [p2-look] A look dug below the ground (a strip mine's pit: the
    /// renderer cuts its hole out of the ground, `holes`); the only styles
    /// the tests let reach under it.
    pub fn digs(self) -> bool {
        self == Style::StripMine
    }

    /// [r4-brick] The Brick & Steel look's styles (Foundry Row).
    pub const BRICK_STEEL: [Style; 10] = [Style::BrickRow, Style::WarehouseLoft, Style::MillLofts, Style::BrickWorkshop, Style::CornerPub, Style::UnionHall, Style::SawtoothFactory, Style::FoundryWorks, Style::GasWorks, Style::LoftEstate];

    /// [r4-oldstone] The Old Stone look's styles (the Historic look; the
    /// Old Town Charter).
    pub const OLD_STONE: [Style; 11] = [Style::StoneHouse, Style::GabledRow, Style::CourtyardHouse, Style::MarketArcade, Style::GuildHall, Style::ClockTower, Style::OldInn, Style::WaterMill, Style::Tannery, Style::CobbledYard, Style::StoneManor];

    /// [capsule] The Capsule district's styles.
    pub const CAPSULE: [Style; 10] = [Style::DomeHouse, Style::SaucerHouse, Style::PopsicleDome, Style::TwinDomes, Style::DomePods, Style::CapsuleTower, Style::SaucerStack, Style::StiltTower, Style::DomeHall, Style::DomeCampus];

    /// [lantern] Lantern Alley's styles.
    pub const LANTERN: [Style; 9] = [Style::LanternShophouse, Style::AlleyWalkups, Style::StallRows, Style::FoodCourt, Style::TeaHouse, Style::LanternMarket, Style::AlleyWorkshops, Style::BackKitchens, Style::LanternCompound];

    /// [garden] The Garden Suburb's styles.
    pub const GARDEN: [Style; 7] = [Style::GardenHouse, Style::Bungalow, Style::CulDeSac, Style::ShoppingParade, Style::GardenCentre, Style::CornerCafe, Style::GardenManor];

    /// [r4-harbour] Harbour Pastel's styles (The Promenade).
    pub const HARBOUR: [Style; 10] = [Style::PastelTerrace, Style::Boathouses, Style::LighthouseCorner, Style::BoardwalkCafe, Style::SeafoodShack, Style::AwningArcade, Style::BoutiqueHotel, Style::FishMarketHall, Style::NetSheds, Style::PastelVilla];

    /// [r4-solarpunk] Solarpunk Green's styles (Eco Village).
    pub const SOLARPUNK: [Style; 10] = [Style::TimberTerrace, Style::SolarTownhouses, Style::VineTower, Style::GlassMarketHall, Style::CoopStore, Style::RepairCafe, Style::VerticalFarm, Style::GreenhouseRows, Style::BikeDepot, Style::SolarVilla];
    /// [r4-glass] Glass Campus's styles (Founders' Campus).
    pub const GLASS_CAMPUS: [Style; 9] = [Style::StudentFlats, Style::CoLivingTower, Style::GlassLab, Style::StartupSheds, Style::AtriumOffice, Style::CampusQuad, Style::ResearchLab, Style::ServerHall, Style::GlassPavilion];
    /// [r4-skyline] Skyline Luxe's styles (Neo Tokyo Heights).
    pub const SKYLINE_LUXE: [Style; 7] = [Style::LuxeTower, Style::TwinTowers, Style::PoolTower, Style::HelipadTower, Style::MallPodium, Style::BoutiqueRow, Style::SkyVilla];
}

impl Style {
    /// The landmark attractions (`buildings/attractions.rs`).
    pub fn is_attraction(self) -> bool {
        matches!(
            self,
            Style::ArtMuseum | Style::ScienceMuseum | Style::HistoryMuseum | Style::PleasurePier | Style::GrandPark | Style::Zoo | Style::Observatory | Style::Aquarium | Style::BotanicalGarden | Style::ConcertHall
        )
    }

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
            Form::StripMall => Some(Style::StripMall),
            Form::Mall => Some(Style::Mall),
            Form::MegaMall => Some(Style::MegaMall),
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
        matches!(
            self,
            Style::Crystal | Style::Fortress | Style::GardenTower | Style::PencilTower | Style::TwinSlab | Style::GlassTerraces | Style::Megastructure | Style::SlabTower | Style::TwistTower
                // [capsule] Their own round deck on a mast over the crown.
                | Style::CapsuleTower | Style::SaucerStack | Style::StiltTower
                // [r4-skyline] Its own helipad over the roof.
                | Style::HelipadTower
        )
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
            K::ArtMuseum => Style::ArtMuseum,
            K::ScienceMuseum => Style::ScienceMuseum,
            K::HistoryMuseum => Style::HistoryMuseum,
            K::Pier => Style::PleasurePier,
            K::GrandPark => Style::GrandPark,
            K::Zoo => Style::Zoo,
            K::Observatory => Style::Observatory,
            K::Aquarium => Style::Aquarium,
            K::BotanicalGarden => Style::BotanicalGarden,
            K::ConcertHall => Style::ConcertHall,
            K::AmusementPark => Style::AmusementPark,
            K::ConventionCentre => Style::ConventionCentre,
            K::Cathedral => Style::Cathedral,
            K::CasinoResort => Style::CasinoResort,
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
