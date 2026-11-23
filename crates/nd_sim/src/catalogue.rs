//! [r13-catalogue] The goods and business catalogue: the one place where
//! the facts about every good (`market::Resource`) and every kind of
//! business (`market::BusinessKind`) are written down. Everything else
//! reads them from here: `Resource::label`/`base_price`, the box a good
//! ships in (`rail::BoxKind::for_resource`), the lorry body
//! (`vehicle::VehicleModel::for_cargo`), what flies, recipes, opening
//! hours, the land uses a kind grows on, the inspector's line about it.
//!
//! Two tables, one row per variant, in the enums' order: [`GOODS`] and
//! [`BUSINESSES`]; and the building sites' bill of materials,
//! [`SITE_BILL`]. Adding a good or a kind of business means adding its
//! variant at the end of its enum and a row here; the exhaustive matches in
//! [`good_index`] and [`business_index`] fail to compile until the row is
//! there, and `tests` checks each row sits at its variant's index.
//! `docs/catalogue.md` has the checklist.
//!
//! Who makes and who uses a good is not written twice: it is read off the
//! recipes ([`makers`], [`users`]); flows outside the recipes (scrap steel
//! cut from old boxes, recovered plastics) are in the good's `note`.

use crate::clock::Hours;
use crate::market::{BusinessKind, Resource};
use crate::rail::BoxKind;
use crate::vehicle::BodyType;
use crate::zoning::LandUse;

use BusinessKind as K;
use Resource as R;

// ── goods ──────────────────────────────────────────────────────────────────

/// What sort of thing a good is, for the UI's grouping and the coming
/// supplies review (round 14 adds `BuildingMaterial`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GoodCategory {
    /// Crops and animals off the farms.
    Farm,
    /// Food made from them, for the shops and kitchens.
    Food,
    /// The catch and what is cut from it.
    Seafood,
    /// Oil and what is refined from it.
    Petrochemical,
    /// Rock, ore and metal.
    Minerals,
    /// Parts and electronics for the plants and shops.
    Manufactured,
    /// Finished goods sold to citizens.
    Consumer,
    /// Paintings and sculpture.
    Culture,
}

/// What one unit of a good stands for (the sim counts plain units; this is
/// what the inspector can call them).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Unit {
    Tonne,
    Crate,
    Pallet,
    Kilolitre,
    Head,
    Piece,
}

impl Unit {
    pub fn label(self) -> &'static str {
        match self {
            Unit::Tonne => "t",
            Unit::Crate => "crates",
            Unit::Pallet => "pallets",
            Unit::Kilolitre => "kL",
            Unit::Head => "head",
            Unit::Piece => "pieces",
        }
    }
}

/// One good: every fact about it.
#[derive(Clone, Copy, Debug)]
pub struct GoodRow {
    pub resource: Resource,
    /// Its name in the UI, the remote API and the reports.
    pub label: &'static str,
    pub category: GoodCategory,
    pub unit: Unit,
    /// Reference price per unit; the outer market trades around it.
    pub base_price: f32,
    /// The container it travels in by rail, sea and box lorry.
    pub ships: BoxKind,
    /// The lorry body a local delivery of it comes in (long hauls come
    /// by semi, building sites by the builders' trucks).
    pub truck: BodyType,
    /// Hours of its own output a business keeps before shipping the rest
    /// out of town.
    pub export_keep_hours: f32,
    /// How much harder than an ordinary load a lorry full of it is on the
    /// road (`crate::road_wear::HEAVY` times this).
    pub road_wear: f32,
    /// Sold to citizens directly, not to other businesses.
    pub retail: bool,
    /// Flown in when an airport can take it: dear and wanted fast.
    pub by_air: bool,
    /// Flows outside the recipes: where else it comes from and goes.
    pub note: &'static str,
}

const GOOD_BASE: GoodRow = GoodRow {
    resource: R::Produce,
    label: "",
    category: GoodCategory::Food,
    unit: Unit::Crate,
    base_price: 1.0,
    ships: BoxKind::Dry,
    truck: BodyType::BoxVan,
    export_keep_hours: 48.0,
    road_wear: 1.0,
    retail: false,
    by_air: false,
    note: "",
};

/// Every good, in `Resource` order.
pub const GOODS: [GoodRow; Resource::COUNT] = [
    GoodRow {
        resource: R::Produce,
        label: "produce",
        category: GoodCategory::Farm,
        unit: Unit::Crate,
        base_price: 2.0,
        ships: BoxKind::Reefer,
        truck: BodyType::PanelTruck,
        export_keep_hours: 48.0,
        road_wear: 1.0,
        retail: false,
        by_air: false,
        ..GOOD_BASE
    },
    GoodRow {
        resource: R::Grain,
        label: "grain",
        category: GoodCategory::Farm,
        unit: Unit::Tonne,
        base_price: 1.0,
        ships: BoxKind::OpenTop,
        truck: BodyType::PanelTruck,
        export_keep_hours: 48.0,
        road_wear: 1.0,
        retail: false,
        by_air: false,
        ..GOOD_BASE
    },
    GoodRow {
        resource: R::Flour,
        label: "flour",
        category: GoodCategory::Food,
        unit: Unit::Pallet,
        base_price: 2.4,
        ships: BoxKind::Dry,
        truck: BodyType::PanelTruck,
        export_keep_hours: 48.0,
        road_wear: 1.0,
        retail: false,
        by_air: false,
        ..GOOD_BASE
    },
    GoodRow {
        resource: R::BakedGoods,
        label: "baked goods",
        category: GoodCategory::Food,
        unit: Unit::Crate,
        base_price: 4.5,
        ships: BoxKind::Dry,
        truck: BodyType::DeliveryVan,
        export_keep_hours: 48.0,
        road_wear: 1.0,
        retail: true,
        by_air: false,
        note: "Also every meal the eateries cook and sell over the counter.",
        ..GOOD_BASE
    },
    GoodRow {
        resource: R::Meat,
        label: "meat",
        category: GoodCategory::Food,
        unit: Unit::Crate,
        base_price: 7.0,
        ships: BoxKind::Reefer,
        truck: BodyType::BoxVan,
        export_keep_hours: 48.0,
        road_wear: 1.0,
        retail: false,
        by_air: false,
        ..GOOD_BASE
    },
    GoodRow {
        resource: R::Dairy,
        label: "dairy",
        category: GoodCategory::Food,
        unit: Unit::Crate,
        base_price: 3.0,
        ships: BoxKind::Reefer,
        truck: BodyType::BoxVan,
        export_keep_hours: 48.0,
        road_wear: 1.0,
        retail: false,
        by_air: false,
        ..GOOD_BASE
    },
    GoodRow {
        resource: R::Groceries,
        label: "groceries",
        category: GoodCategory::Consumer,
        unit: Unit::Pallet,
        base_price: 6.0,
        ships: BoxKind::Dry,
        truck: BodyType::DeliveryVan,
        export_keep_hours: 48.0,
        road_wear: 1.0,
        retail: true,
        by_air: false,
        note: "Bought in from outside at no markup when the grocers' shelves run bare.",
        ..GOOD_BASE
    },
    GoodRow {
        resource: R::Parts,
        label: "parts",
        category: GoodCategory::Manufactured,
        unit: Unit::Pallet,
        base_price: 9.0,
        ships: BoxKind::Dry,
        truck: BodyType::Semi,
        export_keep_hours: 48.0,
        road_wear: 1.0,
        retail: false,
        by_air: false,
        note: "Also every building site's materials (`SITE_BILL`), and the stand-in cargo of rubble lorries.",
        ..GOOD_BASE
    },
    GoodRow {
        resource: R::Electronics,
        label: "electronics",
        category: GoodCategory::Manufactured,
        unit: Unit::Pallet,
        base_price: 45.0,
        ships: BoxKind::Dry,
        truck: BodyType::BoxVan,
        export_keep_hours: 48.0,
        road_wear: 1.0,
        retail: false,
        by_air: true,
        ..GOOD_BASE
    },
    GoodRow {
        resource: R::Goods,
        label: "goods",
        category: GoodCategory::Consumer,
        unit: Unit::Pallet,
        base_price: 60.0,
        ships: BoxKind::Dry,
        truck: BodyType::DeliveryVan,
        export_keep_hours: 48.0,
        road_wear: 1.0,
        retail: true,
        by_air: false,
        note: "Partly flown in at international airports (`airport::GOODS_BY_AIR`); the stand-in cargo of cargo-hub runs.",
        ..GOOD_BASE
    },
    GoodRow {
        resource: R::Crude,
        label: "crude oil",
        category: GoodCategory::Petrochemical,
        unit: Unit::Kilolitre,
        base_price: 4.0,
        ships: BoxKind::Tank,
        truck: BodyType::Semi,
        export_keep_hours: 48.0,
        road_wear: 2.0,
        retail: false,
        by_air: false,
        ..GOOD_BASE
    },
    GoodRow {
        resource: R::Fuel,
        label: "fuel",
        category: GoodCategory::Petrochemical,
        unit: Unit::Kilolitre,
        base_price: 12.0,
        ships: BoxKind::Tank,
        truck: BodyType::Semi,
        export_keep_hours: 48.0,
        road_wear: 1.0,
        retail: false,
        by_air: false,
        note: "The stand-in cargo of fuel runs to yards and lots (`world/industry.rs`).",
        ..GOOD_BASE
    },
    GoodRow {
        resource: R::Plastics,
        label: "plastics",
        category: GoodCategory::Petrochemical,
        unit: Unit::Tonne,
        base_price: 16.0,
        ships: BoxKind::Dry,
        truck: BodyType::BoxVan,
        export_keep_hours: 48.0,
        road_wear: 1.0,
        retail: false,
        by_air: false,
        note: "Pellets in bags, in a dry box (a tank container until r13). Also recovered from waste.",
        ..GOOD_BASE
    },
    GoodRow {
        resource: R::Fish,
        label: "fish",
        category: GoodCategory::Seafood,
        unit: Unit::Crate,
        base_price: 3.0,
        ships: BoxKind::Reefer,
        truck: BodyType::BoxVan,
        export_keep_hours: 48.0,
        road_wear: 1.0,
        retail: false,
        by_air: false,
        note: "Also landed by the fishing boats (`world/fishing.rs`).",
        ..GOOD_BASE
    },
    GoodRow {
        resource: R::Livestock,
        label: "livestock",
        category: GoodCategory::Farm,
        unit: Unit::Head,
        base_price: 3.5,
        ships: BoxKind::Dry,
        truck: BodyType::Semi,
        export_keep_hours: 48.0,
        road_wear: 1.6,
        retail: false,
        by_air: false,
        ..GOOD_BASE
    },
    GoodRow {
        resource: R::Ore,
        label: "ore",
        category: GoodCategory::Minerals,
        unit: Unit::Tonne,
        base_price: 6.0,
        ships: BoxKind::OpenTop,
        truck: BodyType::Semi,
        export_keep_hours: 48.0,
        road_wear: 2.5,
        retail: false,
        by_air: false,
        ..GOOD_BASE
    },
    GoodRow {
        resource: R::Fillets,
        label: "fish fillets",
        category: GoodCategory::Seafood,
        unit: Unit::Crate,
        base_price: 6.0,
        ships: BoxKind::Reefer,
        truck: BodyType::BoxVan,
        export_keep_hours: 48.0,
        road_wear: 1.0,
        retail: false,
        by_air: false,
        ..GOOD_BASE
    },
    GoodRow {
        resource: R::Art,
        label: "art",
        category: GoodCategory::Culture,
        unit: Unit::Piece,
        base_price: 30.0,
        ships: BoxKind::Dry,
        truck: BodyType::BoxVan,
        export_keep_hours: 48.0,
        road_wear: 1.0,
        retail: false,
        by_air: true,
        note: "Galleries sell it on to visitors (`world/pressure.rs`).",
        ..GOOD_BASE
    },
    // Dear enough that a smelter on local ore pays its way shipping ingots out
    // of town at the export discount; ingots have no local buyer, so they go as
    // soon as a lorryload stands in the yard.
    GoodRow {
        resource: R::Metal,
        label: "metal",
        category: GoodCategory::Minerals,
        unit: Unit::Tonne,
        base_price: 75.0,
        ships: BoxKind::Dry,
        truck: BodyType::Semi,
        export_keep_hours: 8.0,
        road_wear: 2.5,
        retail: false,
        by_air: false,
        note: "Also steel cut from worn-out containers at scrap yards (`World::cut_up_boxes`) and recovered from waste; scrap steel goes to building sites first (`SITE_BILL`).",
        ..GOOD_BASE
    },
];

// ── businesses ─────────────────────────────────────────────────────────────

/// One kind of business: every fact about it the economy, the UI and the
/// places to go out read.
#[derive(Clone, Copy, Debug)]
pub struct BusinessRow {
    pub kind: BusinessKind,
    /// Its name in the UI; the remote API's key is this in snake case.
    pub label: &'static str,
    /// What it buys and makes, per filled job per hour.
    pub inputs: &'static [(Resource, f32)],
    pub outputs: &'static [(Resource, f32)],
    /// When the doors are open (and the shifts run).
    pub hours: Hours,
    /// Serves citizens at the door: how many at once per job.
    pub customers_per_job: Option<u16>,
    /// Most floors a building of this kind goes up with.
    pub max_floors: Option<u8>,
    /// Stock it tries to keep of each input, in hours of production.
    pub stock_hours: f32,
    /// Sells what it makes over the counter to whoever walks in.
    pub walk_in: Option<Resource>,
    /// What a household's delivery bot orders from it, and how much.
    pub home_order: Option<(Resource, f32)>,
    /// Starting money against an ordinary business's (heavy industry
    /// starts with a bigger float).
    pub start_capital: f32,
    /// Part of the food chain, farm to shelf: staffed first, orders ahead.
    pub food: bool,
    /// A place people go out to and pay at the door by the hour, besides
    /// the places to eat and go out (`crate::leisure`), which always are.
    pub outing: bool,
    /// Sleeps visitors from out of town.
    pub lodging: bool,
    /// Opens only on a lot that runs down to the water.
    pub waterfront: bool,
    /// Opened only where its site or a pressure calls for it, never the
    /// market's pick for a plain lot.
    pub needs_site: bool,
    /// Students drop in by campus.
    pub student_trade: bool,
    /// Parks lorries in bays.
    pub truck_lot: bool,
    /// Fouls the air (the inspector says so).
    pub fouls_air: bool,
    /// What an hour's tab costs here against a nightclub round.
    pub tab: f32,
    /// [leisure] Share of an hour's takings paid out of town for what is
    /// on (the film, the touring company, the band, the comic).
    pub booking_share: f32,
    /// [leisure] Somewhere a parent takes the children before nine.
    pub family_friendly: bool,
    /// [leisure] Somewhere for a date.
    pub date_spot: bool,
    /// [leisure] How loud it is out on the street, `0..1`.
    pub loudness: f32,
    /// [leisure] Trash a worker's day leaves here against its land use's
    /// rate.
    pub trash: f32,
    /// [leisure] Land value it lifts round it beyond its land use's
    /// splat: `(radius, weight)`.
    pub land_value_lift: Option<(f32, f32)>,
    /// The word over its shopfront (a tenant's sign); `None` keeps the
    /// plain lit board.
    pub sign: Option<&'static [u8]>,
    /// The inspector's line about what it does for the town; `{dirt}`
    /// becomes the pollution clause for the dirty ones. `None`: the
    /// inspector says it by its industry.
    pub about: Option<&'static str>,
}

const BASE: BusinessRow = BusinessRow {
    kind: K::Office,
    label: "",
    inputs: &[],
    outputs: &[],
    hours: Hours::ALWAYS,
    customers_per_job: None,
    max_floors: None,
    stock_hours: 24.0,
    walk_in: None,
    home_order: None,
    start_capital: 1.0,
    food: false,
    outing: false,
    lodging: false,
    waterfront: false,
    needs_site: false,
    student_trade: false,
    truck_lot: false,
    fouls_air: false,
    tab: 1.0,
    booking_share: 0.0,
    family_friendly: false,
    date_spot: false,
    loudness: 0.1,
    trash: 1.0,
    land_value_lift: None,
    sign: None,
    about: None,
};

// Inspector lines shared by several kinds.
const FARMING: &str = "Farming: grows food for the mills, dairies and shops, with lorries in and out.";
const FISH_PROCESSING: &str = "Fish processing: takes in the catch and sends food on to the shops, with lorries in and out{dirt}.";
const FOOD_PROCESSING: &str = "Food processing: farm produce in, food out to the bakeries and shops{dirt}.";
const CORNER_SHOP: &str = "Feeds the neighbourhood: people on foot drop in to shop, and a street of shops draws more.";
const GOING_OUT: &str = "Somewhere to go out: draws crowds (and some noise) in the evenings.";

/// Every kind of business, in `BusinessKind` order.
pub const BUSINESSES: [BusinessRow; BusinessKind::COUNT] = [
    // hours: Plants run two shifts.
    BusinessRow {
        kind: K::VertFarm,
        label: "vertical farm",
        outputs: &[(R::Produce, 3.0)],
        hours: Hours::new(6, 22),
        food: true,
        about: Some(FARMING),
        ..BASE
    },
    BusinessRow {
        kind: K::GrainFarm,
        label: "grain farm",
        outputs: &[(R::Grain, 4.0)],
        hours: Hours::new(6, 22),
        food: true,
        about: Some(FARMING),
        ..BASE
    },
    BusinessRow {
        kind: K::ProteinVat,
        label: "protein vat",
        inputs: &[(R::Grain, 1.0)],
        outputs: &[(R::Meat, 1.0)],
        hours: Hours::new(6, 22),
        food: true,
        about: Some(FARMING),
        ..BASE
    },
    BusinessRow {
        kind: K::Dairy,
        label: "dairy",
        inputs: &[(R::Grain, 0.5)],
        outputs: &[(R::Dairy, 1.5)],
        hours: Hours::new(6, 22),
        food: true,
        about: Some(FARMING),
        ..BASE
    },
    // Milling is automated: a hand runs six of grain an hour. At two the mill added too little to
    // what it bought to pay its wages.
    BusinessRow {
        kind: K::Mill,
        label: "mill",
        inputs: &[(R::Grain, 6.0)],
        outputs: &[(R::Flour, 6.0)],
        hours: Hours::new(6, 22),
        food: true,
        about: Some(FOOD_PROCESSING),
        ..BASE
    },
    BusinessRow {
        kind: K::Fab,
        label: "parts fab",
        outputs: &[(R::Parts, 1.0)],
        hours: Hours::new(6, 22),
        ..BASE
    },
    // Two parts (18 at base) make 0.8 electronics (36); at 0.4 the plant added nothing to what it
    // bought and could never pay wages.
    BusinessRow {
        kind: K::Assembly,
        label: "assembly plant",
        inputs: &[(R::Parts, 2.0)],
        outputs: &[(R::Electronics, 0.8)],
        hours: Hours::new(6, 22),
        ..BASE
    },
    // Shops pay town wages, so what a job adds to its inputs at base price covers its share of
    // them: a grocer's job adds about 80 a day against 54, a baker's about 59, a gadget seller's
    // about 84.
    BusinessRow {
        kind: K::Grocery,
        label: "grocery",
        inputs: &[(R::Produce, 1.0), (R::Flour, 0.4), (R::Meat, 0.3), (R::Dairy, 0.4)],
        outputs: &[(R::Groceries, 4.0)],
        hours: Hours::new(7, 22),
        customers_per_job: Some(3),
        stock_hours: 36.0,
        walk_in: Some(R::Groceries),
        food: true,
        home_order: Some((R::Groceries, 4.0)),
        about: Some(CORNER_SHOP),
        ..BASE
    },
    BusinessRow {
        kind: K::Bakery,
        label: "bakery",
        inputs: &[(R::Flour, 1.2)],
        outputs: &[(R::BakedGoods, 4.0)],
        hours: Hours::new(6, 19),
        customers_per_job: Some(3),
        stock_hours: 36.0,
        walk_in: Some(R::BakedGoods),
        food: true,
        student_trade: true,
        home_order: Some((R::BakedGoods, 2.0)),
        about: Some(CORNER_SHOP),
        ..BASE
    },
    BusinessRow {
        kind: K::GadgetShop,
        label: "gadget shop",
        inputs: &[(R::Electronics, 0.5)],
        outputs: &[(R::Goods, 0.8)],
        hours: Hours::new(9, 20),
        customers_per_job: Some(3),
        stock_hours: 36.0,
        walk_in: Some(R::Goods),
        student_trade: true,
        home_order: Some((R::Goods, 1.0)),
        about: Some("Sells to the people round about; its customers come on foot and by car."),
        ..BASE
    },
    BusinessRow {
        kind: K::Venue,
        label: "venue",
        inputs: &[(R::Groceries, 0.3)],
        hours: Hours::new(18, 3),
        customers_per_job: Some(8),
        stock_hours: 36.0,
        outing: true,
        loudness: 0.9,
        about: Some(GOING_OUT),
        ..BASE
    },
    BusinessRow {
        kind: K::Office,
        label: "office",
        hours: Hours::new(8, 18),
        about: Some("Office work: steady, well-paid jobs. Its staff buy lunch and coffee nearby."),
        ..BASE
    },
    BusinessRow {
        kind: K::Parking,
        label: "parking",
        hours: Hours::ALWAYS,
        about: Some("Drivers coming to this street pay to park here instead of circling the block."),
        ..BASE
    },
    BusinessRow {
        kind: K::OilWell,
        label: "oil well",
        outputs: &[(R::Crude, 2.5)],
        hours: Hours::ALWAYS,
        fouls_air: true,
        ..BASE
    },
    BusinessRow {
        kind: K::Refinery,
        label: "refinery",
        inputs: &[(R::Crude, 1.5)],
        outputs: &[(R::Fuel, 0.8), (R::Plastics, 0.4)],
        hours: Hours::new(6, 22),
        fouls_air: true,
        ..BASE
    },
    // Fuel is burnt by the hour in `World::industry_hourly`, which also pays the handling and bay
    // fees.
    BusinessRow {
        kind: K::ContainerYard,
        label: "container yard",
        inputs: &[(R::Fuel, 0.15)],
        hours: Hours::ALWAYS,
        about: Some("Stores and loads containers for the freight trade: handling fees, and lorries in and out."),
        ..BASE
    },
    BusinessRow {
        kind: K::FleetLot,
        label: "truck and bus lot",
        inputs: &[(R::Fuel, 0.1)],
        hours: Hours::ALWAYS,
        truck_lot: true,
        about: Some("Parks idle lorries and buses for a fee, with fuel and noise from the yard."),
        ..BASE
    },
    BusinessRow {
        kind: K::Fishery,
        label: "fishery",
        outputs: &[(R::Fish, 2.5)],
        hours: Hours::new(4, 16),
        food: true,
        about: Some("Boats landing fish for the processing plants and markets: part of the local fishing economy."),
        ..BASE
    },
    BusinessRow {
        kind: K::FishPlant,
        label: "fish processing plant",
        inputs: &[(R::Fish, 1.5)],
        outputs: &[(R::Meat, 1.0)],
        hours: Hours::new(6, 22),
        food: true,
        about: Some(FISH_PROCESSING),
        ..BASE
    },
    BusinessRow {
        kind: K::TowYard,
        label: "tow company",
        hours: Hours::ALWAYS,
        about: Some("Tows broken-down and illegally parked cars to its pound; owners pay to get them back."),
        ..BASE
    },
    // [tourism] Breakfast: a little of the town's groceries. Rooms are paid for by the night
    // (`World::tourism_nightly`).
    BusinessRow {
        kind: K::Hotel,
        label: "hotel",
        inputs: &[(R::Groceries, 0.15)],
        hours: Hours::ALWAYS,
        lodging: true,
        about: Some("Rooms for visitors: tourists sleep here and spend in the shops, bars and sights round it."),
        ..BASE
    },
    // Lives on fares (`world/taxis.rs`).
    BusinessRow {
        kind: K::TaxiCompany,
        label: "taxi company",
        hours: Hours::ALWAYS,
        about: Some("Runs taxis: people without a car get a ride, for a fare."),
        ..BASE
    },
    // Handling fees (`world/cargo_hubs.rs`).
    BusinessRow {
        kind: K::CargoHub,
        label: "cargo hub",
        hours: Hours::ALWAYS,
        truck_lot: true,
        about: Some("Freight terminal: businesses nearby import through it in shared trucks instead of one lorry each."),
        ..BASE
    },
    // [economy] Services paid by the job (`world/pressure.rs`), car and bike sales
    // (`world/cars.rs`) and contracts (`trash.rs`).
    BusinessRow {
        kind: K::Mechanic,
        label: "car repair shop",
        hours: Hours::new(8, 18),
        max_floors: Some(1),
        needs_site: true,
        about: Some("Fixes the town's cars."),
        ..BASE
    },
    BusinessRow {
        kind: K::CarDealer,
        label: "car dealership",
        hours: Hours::new(9, 19),
        max_floors: Some(1),
        needs_site: true,
        about: Some("Sells cars: more people drive, and more cars need parking."),
        ..BASE
    },
    BusinessRow {
        kind: K::TrashCompany,
        label: "trash company",
        hours: Hours::new(6, 18),
        max_floors: Some(1),
        needs_site: true,
        about: Some("A private hauler: empties the bins of whoever pays."),
        ..BASE
    },
    BusinessRow {
        kind: K::BikeShop,
        label: "bike shop",
        hours: Hours::new(9, 19),
        customers_per_job: Some(3),
        max_floors: Some(1),
        needs_site: true,
        about: Some("Sells and mends bikes: more people ride."),
        ..BASE
    },
    // A job adds about 50 a day at base prices (7 an hour of value added at `JOB_RATE`), like the
    // farms and plants it sits with.
    BusinessRow {
        kind: K::CowFarm,
        label: "cattle farm",
        inputs: &[(R::Grain, 1.0)],
        outputs: &[(R::Livestock, 2.2)],
        hours: Hours::new(6, 22),
        max_floors: Some(1),
        food: true,
        needs_site: true,
        about: Some(FARMING),
        ..BASE
    },
    BusinessRow {
        kind: K::ChickenFarm,
        label: "chicken farm",
        inputs: &[(R::Grain, 0.8)],
        outputs: &[(R::Livestock, 1.8)],
        hours: Hours::new(6, 22),
        max_floors: Some(1),
        food: true,
        needs_site: true,
        about: Some(FARMING),
        ..BASE
    },
    BusinessRow {
        kind: K::Slaughterhouse,
        label: "slaughterhouse",
        inputs: &[(R::Livestock, 2.0)],
        outputs: &[(R::Meat, 2.0)],
        hours: Hours::new(6, 22),
        max_floors: Some(3),
        food: true,
        needs_site: true,
        fouls_air: true,
        about: Some(FOOD_PROCESSING),
        ..BASE
    },
    // Sold out of town at the export discount.
    BusinessRow {
        kind: K::StripMine,
        label: "strip mine",
        outputs: &[(R::Ore, 2.0)],
        hours: Hours::new(6, 22),
        max_floors: Some(1),
        needs_site: true,
        fouls_air: true,
        ..BASE
    },
    // Berth fees (`world/waterfront.rs`).
    // tab_factor: A drink at the clubhouse or the marina café.
    BusinessRow {
        kind: K::Marina,
        label: "marina",
        hours: Hours::ALWAYS,
        outing: true,
        needs_site: true,
        tab: 1.3,
        about: Some("Lets berths to boat owners and keeps a cafe on the water: a place to go out."),
        ..BASE
    },
    BusinessRow {
        kind: K::FishingPort,
        label: "fishing port",
        outputs: &[(R::Fish, 3.0)],
        hours: Hours::new(4, 16),
        food: true,
        needs_site: true,
        about: Some("Adds to the local fishing economy and provides a dock for the fishing fleet: its trawlers land fish for the gutting sheds and markets."),
        ..BASE
    },
    BusinessRow {
        kind: K::FishGutting,
        label: "fish gutting shed",
        inputs: &[(R::Fish, 2.0)],
        outputs: &[(R::Fillets, 2.0)],
        hours: Hours::new(5, 17),
        max_floors: Some(2),
        food: true,
        needs_site: true,
        about: Some(FISH_PROCESSING),
        ..BASE
    },
    BusinessRow {
        kind: K::FishStickFactory,
        label: "fish-stick factory",
        inputs: &[(R::Fillets, 1.5)],
        outputs: &[(R::Meat, 2.2)],
        hours: Hours::new(6, 22),
        max_floors: Some(3),
        food: true,
        needs_site: true,
        about: Some(FISH_PROCESSING),
        ..BASE
    },
    BusinessRow {
        kind: K::FishMarket,
        label: "fish market",
        inputs: &[(R::Fish, 2.0)],
        outputs: &[(R::Groceries, 3.0)],
        hours: Hours::new(5, 14),
        customers_per_job: Some(3),
        max_floors: Some(2),
        stock_hours: 36.0,
        food: true,
        needs_site: true,
        about: Some("Sells the catch to the public: fresh fish for the neighbourhood."),
        ..BASE
    },
    // Diners and golfers pay at the door (`World::venue_tabs`).
    BusinessRow {
        kind: K::SeafoodRestaurant,
        label: "seafood restaurant",
        inputs: &[(R::Fillets, 0.3)],
        hours: Hours::new(11, 23),
        customers_per_job: Some(6),
        max_floors: Some(2),
        stock_hours: 36.0,
        outing: true,
        needs_site: true,
        tab: 1.5,
        about: Some(GOING_OUT),
        ..BASE
    },
    // hours: Floodlit until late.
    BusinessRow {
        kind: K::GolfRange,
        label: "golf driving range",
        hours: Hours::new(8, 23),
        customers_per_job: Some(6),
        max_floors: Some(1),
        outing: true,
        needs_site: true,
        tab: 1.7,
        about: Some(GOING_OUT),
        ..BASE
    },
    BusinessRow {
        kind: K::ArtStudio,
        label: "artists' studios",
        outputs: &[(R::Art, 0.5)],
        hours: Hours::new(10, 20),
        max_floors: Some(2),
        needs_site: true,
        about: Some("Artists make art in cheap space here; the galleries nearby sell it."),
        ..BASE
    },
    // Hangs art bought from the studios; sells it to visitors.
    // tab_factor: Entry is free; sales and donations pay (`world/pressure.rs`).
    BusinessRow {
        kind: K::Gallery,
        label: "gallery",
        inputs: &[(R::Art, 0.1)],
        hours: Hours::new(11, 22),
        customers_per_job: Some(8),
        max_floors: Some(2),
        stock_hours: 36.0,
        outing: true,
        needs_site: true,
        tab: 0.4,
        about: Some("Hangs and sells art and holds openings: a place to go out, free at the door."),
        ..BASE
    },
    // [dirty-industry] One and a half of ore (9 at base) and a little fuel make an ingot (75):
    // exported at the discount (34) it clears local ore and the wages, as the mine's ore does; on
    // imported ore the margin is thin, so smelters stay by the mines.
    BusinessRow {
        kind: K::Smelter,
        label: "smelter",
        inputs: &[(R::Ore, 1.5), (R::Fuel, 0.1)],
        outputs: &[(R::Metal, 1.0)],
        hours: Hours::new(6, 22),
        max_floors: Some(2),
        needs_site: true,
        start_capital: 5.0,
        fouls_air: true,
        ..BASE
    },
    // Intensive: five times the livestock per hand of a cattle farm (the whole point of it), so it
    // pays its way even selling out of town.
    BusinessRow {
        kind: K::Feedlot,
        label: "factory farm",
        inputs: &[(R::Grain, 2.0)],
        outputs: &[(R::Livestock, 12.0)],
        hours: Hours::new(6, 22),
        max_floors: Some(1),
        food: true,
        needs_site: true,
        start_capital: 5.0,
        fouls_air: true,
        about: Some("Factory farming: livestock out by the lorryload for the slaughterhouses{dirt}."),
        ..BASE
    },
    BusinessRow {
        kind: K::ChemicalWorks,
        label: "chemical works",
        inputs: &[(R::Crude, 1.2)],
        outputs: &[(R::Plastics, 0.7)],
        hours: Hours::new(6, 22),
        max_floors: Some(2),
        needs_site: true,
        start_capital: 5.0,
        fouls_air: true,
        ..BASE
    },
    // [waterfront] Breakfasts and the bar; rooms are paid by the night (`World::daily_tourism`).
    BusinessRow {
        kind: K::BeachResort,
        label: "beach resort",
        inputs: &[(R::Groceries, 0.25)],
        hours: Hours::ALWAYS,
        max_floors: Some(5),
        lodging: true,
        waterfront: true,
        needs_site: true,
        about: Some("Rooms down to the sand: tourists sleep here, pay well by the night and spend in town."),
        ..BASE
    },
    // Diners, drinkers and renters pay at the door (`World::venue_tabs`).
    // tab_factor: [waterfront] White tablecloths over the water; a drink in the sand; an hour in a
    // pedalo.
    BusinessRow {
        kind: K::PierRestaurant,
        label: "pier restaurant",
        inputs: &[(R::Groceries, 0.35)],
        hours: Hours::new(12, 24),
        customers_per_job: Some(6),
        max_floors: Some(2),
        stock_hours: 36.0,
        outing: true,
        waterfront: true,
        needs_site: true,
        tab: 2.4,
        about: Some(GOING_OUT),
        ..BASE
    },
    // max_floors: [waterfront] Low on the shore: a resort spreads, it does not climb.
    BusinessRow {
        kind: K::BeachBar,
        label: "beach bar",
        inputs: &[(R::Groceries, 0.2)],
        hours: Hours::new(10, 2),
        customers_per_job: Some(10),
        max_floors: Some(1),
        stock_hours: 36.0,
        outing: true,
        waterfront: true,
        needs_site: true,
        tab: 1.1,
        about: Some(GOING_OUT),
        ..BASE
    },
    BusinessRow {
        kind: K::BoatRental,
        label: "boat rental",
        inputs: &[(R::Fuel, 0.05)],
        hours: Hours::new(8, 20),
        customers_per_job: Some(5),
        max_floors: Some(1),
        outing: true,
        waterfront: true,
        needs_site: true,
        tab: 1.8,
        about: Some(GOING_OUT),
        ..BASE
    },
    // [sweep-sim] A job adds about 9 an hour at base prices, a baker's 15 (the café sells fewer,
    // sits people down), a bookseller's about 17 against a gadget seller's 25.
    BusinessRow {
        kind: K::Cafe,
        label: "cafe",
        inputs: &[(R::Flour, 0.6), (R::Dairy, 0.3)],
        outputs: &[(R::BakedGoods, 2.5)],
        hours: Hours::new(7, 21),
        customers_per_job: Some(3),
        stock_hours: 36.0,
        walk_in: Some(R::BakedGoods),
        needs_site: true,
        student_trade: true,
        sign: Some(b"CAFE"),
        home_order: Some((R::BakedGoods, 1.0)),
        about: Some("Coffee and pastries, a place to sit: students and office workers drop in."),
        ..BASE
    },
    BusinessRow {
        kind: K::Bookshop,
        label: "bookshop",
        inputs: &[(R::Plastics, 0.25)],
        outputs: &[(R::Goods, 0.35)],
        hours: Hours::new(9, 20),
        customers_per_job: Some(3),
        stock_hours: 36.0,
        walk_in: Some(R::Goods),
        needs_site: true,
        student_trade: true,
        home_order: Some((R::Goods, 1.0)),
        about: Some("Books and stationery: students and downtown browsers drop in."),
        ..BASE
    },
    // [K34] Its steel comes from the boxes it cuts up (`World::cut_up_boxes`); the trickle here is
    // the other scrap brought in over the gate (cars, sheet, appliances).
    BusinessRow {
        kind: K::ScrapYard,
        label: "scrap yard",
        outputs: &[(R::Metal, 0.04)],
        hours: Hours::new(7, 18),
        max_floors: Some(1),
        needs_site: true,
        about: Some("Cuts worn-out shipping containers (and other scrap) into steel: sold to building sites in town and out of town."),
        ..BASE
    },
    // [r4-leisure-places] Kitchens cook meals (sold as baked goods: over the counter, at lunch and
    // to diners in) from the farms' and mills' produce, meat, flour and milk; a job adds about
    // 7-10 an hour at base prices, as a cafe's. The rest take their stock (drinks and snacks from
    // the grocers, a fine kitchen's produce) by the visitor-hour (`World::venue_tabs`).
    // max_floors: [r4-leisure-places] A diner and a drive-through are one storey; halls and alleys
    // stay low; karaoke stacks its boxes.
    BusinessRow {
        kind: K::Diner,
        label: "diner",
        inputs: &[(R::Meat, 0.3), (R::Dairy, 0.3), (R::Flour, 0.4), (R::Produce, 0.4)],
        outputs: &[(R::BakedGoods, 2.5)],
        hours: Hours::new(6, 23),
        customers_per_job: Some(4),
        max_floors: Some(1),
        stock_hours: 36.0,
        walk_in: Some(R::BakedGoods),
        tab: 0.9,
        family_friendly: true,
        trash: 1.3,
        land_value_lift: Some((80.0, 0.02)),
        sign: Some(b"DINER"),
        about: Some("Breakfast all day and a booth for the family: the neighbourhood drops in morning, noon and night."),
        ..BASE
    },
    BusinessRow {
        kind: K::NoodleBar,
        label: "noodle bar",
        inputs: &[(R::Flour, 0.5), (R::Meat, 0.25), (R::Produce, 0.4)],
        outputs: &[(R::BakedGoods, 2.6)],
        hours: Hours::new(11, 2),
        customers_per_job: Some(5),
        max_floors: Some(3),
        stock_hours: 36.0,
        walk_in: Some(R::BakedGoods),
        student_trade: true,
        tab: 0.8,
        date_spot: true,
        trash: 1.3,
        land_value_lift: Some((80.0, 0.02)),
        sign: Some(b"NOODLES"),
        about: Some("Noodles at the counter: lunch for the office crowd, a late bowl after a night out. Cooks with the mills' flour and the farms' meat and greens."),
        ..BASE
    },
    BusinessRow {
        kind: K::PizzaPlace,
        label: "pizza place",
        inputs: &[(R::Flour, 0.6), (R::Dairy, 0.4), (R::Meat, 0.15), (R::Produce, 0.2)],
        outputs: &[(R::BakedGoods, 2.6)],
        hours: Hours::new(11, 23),
        customers_per_job: Some(4),
        max_floors: Some(2),
        stock_hours: 36.0,
        walk_in: Some(R::BakedGoods),
        student_trade: true,
        tab: 0.9,
        family_friendly: true,
        trash: 1.6,
        sign: Some(b"PIZZA"),
        about: Some("Pizza by the slice and by the pie: families and students. Buys flour, milk and toppings from the mills and farms."),
        ..BASE
    },
    BusinessRow {
        kind: K::FastFood,
        label: "fast-food drive-through",
        inputs: &[(R::Meat, 0.4), (R::Flour, 0.4), (R::Produce, 0.3)],
        outputs: &[(R::BakedGoods, 3.2)],
        hours: Hours::new(6, 2),
        customers_per_job: Some(6),
        max_floors: Some(1),
        stock_hours: 36.0,
        walk_in: Some(R::BakedGoods),
        student_trade: true,
        tab: 0.6,
        family_friendly: true,
        loudness: 0.5,
        trash: 2.0,
        sign: Some(b"BURGERS"),
        about: Some("A drive-through on the main road: cheap and quick, open till late, and its takeaway boxes fill the bins."),
        ..BASE
    },
    BusinessRow {
        kind: K::FoodHall,
        label: "food hall",
        inputs: &[(R::Groceries, 0.3), (R::Meat, 0.2), (R::Produce, 0.4)],
        outputs: &[(R::BakedGoods, 2.4)],
        hours: Hours::new(10, 22),
        customers_per_job: Some(8),
        max_floors: Some(2),
        stock_hours: 36.0,
        walk_in: Some(R::BakedGoods),
        tab: 0.9,
        family_friendly: true,
        loudness: 0.2,
        trash: 1.6,
        land_value_lift: Some((120.0, 0.035)),
        sign: Some(b"FOOD HALL"),
        about: Some("Many kitchens under one roof: the lunch crowd from the offices and visitors in from out of town."),
        ..BASE
    },
    BusinessRow {
        kind: K::StreetFood,
        label: "street-food stalls",
        inputs: &[(R::Meat, 0.3), (R::Produce, 0.4), (R::Flour, 0.3)],
        outputs: &[(R::BakedGoods, 2.4)],
        hours: Hours::new(11, 2),
        customers_per_job: Some(8),
        max_floors: Some(1),
        stock_hours: 36.0,
        walk_in: Some(R::BakedGoods),
        tab: 0.6,
        loudness: 0.5,
        trash: 2.0,
        sign: Some(b"EATS"),
        about: Some("Food trucks and stalls on an open lot, busiest after dark: cheap eats, visitors love it, the bins overflow."),
        ..BASE
    },
    BusinessRow {
        kind: K::FineDining,
        label: "fine-dining restaurant",
        inputs: &[(R::Meat, 0.3), (R::Produce, 0.3), (R::Dairy, 0.2)],
        hours: Hours::new(18, 24),
        customers_per_job: Some(3),
        max_floors: Some(2),
        stock_hours: 36.0,
        tab: 3.0,
        date_spot: true,
        land_value_lift: Some((110.0, 0.05)),
        sign: Some(b"DINING"),
        about: Some("A long dinner for those who can pay: dates and anniversaries, rich regulars. Lifts the street's land value."),
        ..BASE
    },
    BusinessRow {
        kind: K::BubbleTea,
        label: "bubble-tea bar",
        inputs: &[(R::Dairy, 0.4), (R::Produce, 0.3)],
        outputs: &[(R::BakedGoods, 2.2)],
        hours: Hours::new(10, 22),
        customers_per_job: Some(6),
        max_floors: Some(2),
        stock_hours: 36.0,
        walk_in: Some(R::BakedGoods),
        student_trade: true,
        tab: 0.5,
        family_friendly: true,
        trash: 1.6,
        land_value_lift: Some((80.0, 0.02)),
        sign: Some(b"BOBA"),
        about: Some("Bubble tea and juice: the young, the afternoon, the walk home from school."),
        ..BASE
    },
    BusinessRow {
        kind: K::Bar,
        label: "bar",
        inputs: &[(R::Groceries, 0.3)],
        hours: Hours::new(16, 2),
        customers_per_job: Some(8),
        max_floors: Some(3),
        stock_hours: 36.0,
        date_spot: true,
        loudness: 0.6,
        trash: 1.3,
        sign: Some(b"BAR"),
        about: Some("A drink after work and into the night: drinks bought from the grocers, regulars at the bar, some noise."),
        ..BASE
    },
    BusinessRow {
        kind: K::KaraokeBox,
        label: "karaoke box",
        inputs: &[(R::Groceries, 0.3)],
        hours: Hours::new(19, 5),
        customers_per_job: Some(6),
        max_floors: Some(4),
        stock_hours: 36.0,
        tab: 1.2,
        loudness: 0.7,
        trash: 1.3,
        sign: Some(b"KARAOKE"),
        about: Some("Private rooms to sing in by the hour, late into the night: friends in groups, and loud."),
        ..BASE
    },
    BusinessRow {
        kind: K::Cinema,
        label: "cinema",
        inputs: &[(R::Groceries, 0.12)],
        hours: Hours::new(12, 24),
        customers_per_job: Some(25),
        max_floors: Some(4),
        stock_hours: 36.0,
        tab: 1.3,
        booking_share: 0.45,
        family_friendly: true,
        date_spot: true,
        loudness: 0.2,
        trash: 1.2,
        land_value_lift: Some((120.0, 0.035)),
        sign: Some(b"CINEMA"),
        about: Some("Films in the evening and matinees by day: families, dates. A share of each ticket goes to the film's distributor."),
        ..BASE
    },
    BusinessRow {
        kind: K::Theatre,
        label: "theatre",
        inputs: &[(R::Groceries, 0.12)],
        hours: Hours::new(14, 23),
        customers_per_job: Some(20),
        max_floors: Some(5),
        stock_hours: 36.0,
        tab: 2.2,
        booking_share: 0.3,
        date_spot: true,
        land_value_lift: Some((160.0, 0.06)),
        sign: Some(b"THEATRE"),
        about: Some("Shows booked from touring companies: older and richer theatregoers and visitors. Lifts the street round it."),
        ..BASE
    },
    BusinessRow {
        kind: K::ConcertHall,
        label: "live-music hall",
        inputs: &[(R::Groceries, 0.12)],
        hours: Hours::new(19, 1),
        customers_per_job: Some(25),
        max_floors: Some(5),
        stock_hours: 36.0,
        tab: 2.0,
        booking_share: 0.4,
        date_spot: true,
        loudness: 0.8,
        trash: 1.2,
        land_value_lift: Some((160.0, 0.06)),
        sign: Some(b"LIVE"),
        about: Some("Bands every night: the young, loud, the band's share of the door paid out of town."),
        ..BASE
    },
    BusinessRow {
        kind: K::BowlingAlley,
        label: "bowling alley",
        inputs: &[(R::Groceries, 0.12)],
        hours: Hours::new(10, 24),
        customers_per_job: Some(8),
        max_floors: Some(2),
        stock_hours: 36.0,
        tab: 1.2,
        family_friendly: true,
        loudness: 0.4,
        trash: 1.2,
        sign: Some(b"BOWL"),
        about: Some("Lanes for families and friends, afternoon to late, with a snack bar."),
        ..BASE
    },
    // Machines wear out: a little electronics for spares.
    BusinessRow {
        kind: K::Arcade,
        label: "arcade",
        inputs: &[(R::Electronics, 0.02)],
        hours: Hours::new(10, 24),
        customers_per_job: Some(10),
        max_floors: Some(3),
        stock_hours: 36.0,
        tab: 0.8,
        family_friendly: true,
        loudness: 0.4,
        trash: 1.2,
        sign: Some(b"ARCADE"),
        about: Some("Game cabinets and VR booths: teenagers and the young after school and at night."),
        ..BASE
    },
    BusinessRow {
        kind: K::Gym,
        label: "gym",
        hours: Hours::new(6, 23),
        customers_per_job: Some(12),
        max_floors: Some(3),
        stock_hours: 36.0,
        tab: 0.7,
        loudness: 0.2,
        trash: 0.6,
        land_value_lift: Some((80.0, 0.02)),
        sign: Some(b"GYM"),
        about: Some("Early mornings before work and evenings after: its regulars come most days."),
        ..BASE
    },
    BusinessRow {
        kind: K::ComedyClub,
        label: "comedy club",
        inputs: &[(R::Groceries, 0.12)],
        hours: Hours::new(19, 1),
        customers_per_job: Some(12),
        max_floors: Some(3),
        stock_hours: 36.0,
        tab: 1.5,
        booking_share: 0.25,
        date_spot: true,
        loudness: 0.4,
        sign: Some(b"COMEDY"),
        about: Some("Stand-up in a basement: couples and friends, late evenings. The comics take a share of the door."),
        ..BASE
    },
    // [r8] Its lorries burn fuel; its trade is the fees.
    BusinessRow {
        kind: K::FreightForwarder,
        label: "freight forwarder",
        inputs: &[(R::Fuel, 0.1)],
        hours: Hours::new(6, 22),
        about: Some("A shipping company in town: leases empty containers, sends its lorries round the firms to fill each box with their exports, and ships the full boxes from the port or the freight yard, charging each firm a fee per load."),
        ..BASE
    },
    // [r14-builders] A builder's yard: no recipe, it lives on its contracts (`world/builders.rs`):
    // paid by the developer in instalments, it pays its crews and buys each site's materials.
    BusinessRow {
        kind: K::Contractor,
        label: "building contractor",
        hours: Hours::new(7, 18),
        max_floors: Some(2),
        needs_site: true,
        about: Some("A builder: crews, vans and lorries out of a yard. Hired per job to put up the city's buildings; buys each site's materials from the builders' merchant, the makers in town or imports, and fetches them in its own lorries or hires a haulier."),
        ..BASE
    },
    // [r14-builders] The builders' merchant: its trade stock (every good a site bill names,
    // `site_materials`) is bought and sold outside the recipe (`World::merchants_hourly`); the
    // recipe is the DIY floor: paint, tools and garden things (goods) for residents.
    BusinessRow {
        kind: K::BuildersMerchant,
        label: "builders' merchant",
        inputs: &[(R::Plastics, 0.3)],
        outputs: &[(R::Goods, 0.45)],
        hours: Hours::new(7, 20),
        customers_per_job: Some(4),
        max_floors: Some(2),
        stock_hours: 36.0,
        walk_in: Some(R::Goods),
        needs_site: true,
        sign: Some(b"HARDWARE"),
        about: Some("A builders' merchant and hardware store: timber, bricks, steel and parts by the pallet for the builders and the trades over the trade counter, delivered by its own lorries; paint, tools and garden things for everyone else."),
        ..BASE
    },
];

// ── where kinds grow ───────────────────────────────────────────────────────

/// Industrial lots choose through `crate::industry` (ground, neighbours,
/// the player's bias); this is the full menu.
pub const INDUSTRIAL: &[BusinessKind] = &[K::VertFarm, K::GrainFarm, K::ProteinVat, K::Dairy, K::Mill, K::Fab, K::Assembly, K::OilWell, K::Refinery, K::ContainerYard, K::FleetLot, K::Fishery, K::FishPlant, K::TowYard, K::CowFarm, K::ChickenFarm, K::Slaughterhouse, K::StripMine, K::FishGutting, K::FishStickFactory, K::TrashCompany, K::Smelter, K::Feedlot, K::ChemicalWorks];
/// [r4-leisure-places] Every commercial sub-zone's menu but leisure's
/// (`CommercialSub::menu`): neighbourhood, retail, offices. The grocer is
/// listed twice: twice as likely.
pub const COMMERCIAL: &[BusinessKind] = &[K::Grocery, K::Grocery, K::Bakery, K::GadgetShop, K::Office, K::Diner, K::NoodleBar, K::PizzaPlace, K::BubbleTea, K::FastFood, K::FoodHall, K::Gym];
/// Leisure land: the nightclub and the rest of the nights out.
pub const ENTERTAINMENT: &[BusinessKind] = &[K::Venue, K::Bar, K::KaraokeBox, K::Cinema, K::Theatre, K::ConcertHall, K::BowlingAlley, K::Arcade, K::ComedyClub, K::FineDining, K::StreetFood, K::NoodleBar];
pub const CIVIC: &[BusinessKind] = &[K::Office];

/// Candidate kinds for a land use, picked at construction.
pub fn candidates(land_use: LandUse) -> &'static [BusinessKind] {
    match land_use {
        LandUse::Industrial => INDUSTRIAL,
        LandUse::Commercial => COMMERCIAL,
        LandUse::Entertainment => ENTERTAINMENT,
        LandUse::Civic => CIVIC,
        _ => &[],
    }
}

// ── building sites ─────────────────────────────────────────────────────────

/// What a building site orders, one truckload when the site opens
/// (`World::order_materials`): `base + per_floor * floors` units of
/// `material`, from the cheapest local maker or else imported; but first,
/// when a scrap yard in town has the stock, `scrap.1` of that load as
/// `scrap.0` (steel cut from old boxes) instead.
///
/// Round 14 (the supplies review) turns this into a bill of materials per
/// building style and size, with several materials delivered by stage;
/// those rows go here.
#[derive(Clone, Copy, Debug)]
pub struct SiteBill {
    pub base: f32,
    pub per_floor: f32,
    pub material: Resource,
    pub scrap: Option<(Resource, f32)>,
}

impl SiteBill {
    /// Units the site orders for `floors` floors.
    pub fn amount(&self, floors: u8) -> f32 {
        self.base + floors as f32 * self.per_floor
    }
}

/// Every site's bill today: generic parts, scrap steel first.
pub const SITE_BILL: SiteBill = SiteBill { base: 4.0, per_floor: 2.0, material: R::Parts, scrap: Some((R::Metal, 0.5)) };

/// The stand-in cargo a rubble lorry carries (nothing is sold or stocked).
pub const RUBBLE: Resource = R::Parts;

/// [r14-builders] Every good a building site's bill can name: what the
/// builders' merchant stocks for the trade and a builder keeps in its yard.
/// Read off the bill (today `SITE_BILL`'s material and its scrap stand-in),
/// so a bill that names more goods is stocked with no other change.
pub fn site_materials() -> Vec<Resource> {
    let mut v = vec![SITE_BILL.material];
    if let Some((scrap, _)) = SITE_BILL.scrap {
        if !v.contains(&scrap) {
            v.push(scrap);
        }
    }
    v
}

// ── lookups ────────────────────────────────────────────────────────────────

/// The row index of a good. Exhaustive on purpose: a new `Resource`
/// variant does not compile until it is given its place (and its row).
pub const fn good_index(r: Resource) -> usize {
    match r {
        R::Produce => 0,
        R::Grain => 1,
        R::Flour => 2,
        R::BakedGoods => 3,
        R::Meat => 4,
        R::Dairy => 5,
        R::Groceries => 6,
        R::Parts => 7,
        R::Electronics => 8,
        R::Goods => 9,
        R::Crude => 10,
        R::Fuel => 11,
        R::Plastics => 12,
        R::Fish => 13,
        R::Livestock => 14,
        R::Ore => 15,
        R::Fillets => 16,
        R::Art => 17,
        R::Metal => 18,
    }
}

/// The row index of a kind of business. Exhaustive on purpose, like
/// [`good_index`].
pub const fn business_index(k: BusinessKind) -> usize {
    match k {
        K::VertFarm => 0,
        K::GrainFarm => 1,
        K::ProteinVat => 2,
        K::Dairy => 3,
        K::Mill => 4,
        K::Fab => 5,
        K::Assembly => 6,
        K::Grocery => 7,
        K::Bakery => 8,
        K::GadgetShop => 9,
        K::Venue => 10,
        K::Office => 11,
        K::Parking => 12,
        K::OilWell => 13,
        K::Refinery => 14,
        K::ContainerYard => 15,
        K::FleetLot => 16,
        K::Fishery => 17,
        K::FishPlant => 18,
        K::TowYard => 19,
        K::Hotel => 20,
        K::TaxiCompany => 21,
        K::CargoHub => 22,
        K::Mechanic => 23,
        K::CarDealer => 24,
        K::TrashCompany => 25,
        K::BikeShop => 26,
        K::CowFarm => 27,
        K::ChickenFarm => 28,
        K::Slaughterhouse => 29,
        K::StripMine => 30,
        K::Marina => 31,
        K::FishingPort => 32,
        K::FishGutting => 33,
        K::FishStickFactory => 34,
        K::FishMarket => 35,
        K::SeafoodRestaurant => 36,
        K::GolfRange => 37,
        K::ArtStudio => 38,
        K::Gallery => 39,
        K::Smelter => 40,
        K::Feedlot => 41,
        K::ChemicalWorks => 42,
        K::BeachResort => 43,
        K::PierRestaurant => 44,
        K::BeachBar => 45,
        K::BoatRental => 46,
        K::Cafe => 47,
        K::Bookshop => 48,
        K::ScrapYard => 49,
        K::Diner => 50,
        K::NoodleBar => 51,
        K::PizzaPlace => 52,
        K::FastFood => 53,
        K::FoodHall => 54,
        K::StreetFood => 55,
        K::FineDining => 56,
        K::BubbleTea => 57,
        K::Bar => 58,
        K::KaraokeBox => 59,
        K::Cinema => 60,
        K::Theatre => 61,
        K::ConcertHall => 62,
        K::BowlingAlley => 63,
        K::Arcade => 64,
        K::Gym => 65,
        K::ComedyClub => 66,
        K::FreightForwarder => 67,
        K::Contractor => 68,
        K::BuildersMerchant => 69,
    }
}

/// A good's row.
pub fn good(r: Resource) -> &'static GoodRow {
    &GOODS[good_index(r)]
}

/// A kind of business's row.
pub fn business(k: BusinessKind) -> &'static BusinessRow {
    &BUSINESSES[business_index(k)]
}

/// The kinds that make `r` (it is among their recipe's outputs).
pub fn makers(r: Resource) -> impl Iterator<Item = BusinessKind> {
    BUSINESSES.iter().filter(move |b| b.outputs.iter().any(|(x, _)| *x == r)).map(|b| b.kind)
}

/// The kinds that use `r` (it is among their recipe's inputs).
pub fn users(r: Resource) -> impl Iterator<Item = BusinessKind> {
    BUSINESSES.iter().filter(move |b| b.inputs.iter().any(|(x, _)| *x == r)).map(|b| b.kind)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every good has its row, at its own index, with a name, a price and
    /// a sensible place in the chains.
    #[test]
    fn every_good_has_a_complete_row() {
        assert_eq!(GOODS.len(), Resource::ALL.len());
        for (i, r) in Resource::ALL.into_iter().enumerate() {
            assert_eq!(good_index(r), i, "{r:?} out of order in Resource::ALL");
            let g = good(r);
            assert_eq!(g.resource, r, "row {i} is {:?}, not {r:?}", g.resource);
            assert!(!g.label.is_empty(), "{r:?} has no label");
            assert!(g.base_price > 0.0, "{r:?} has no price");
            assert!(g.export_keep_hours > 0.0 && g.road_wear >= 1.0, "{r:?}");
            // Someone makes it, or it comes from outside the recipes.
            assert!(makers(r).next().is_some() || !g.note.is_empty(), "{r:?}: nobody makes it and no note says where it comes from");
        }
        let mut labels: Vec<&str> = GOODS.iter().map(|g| g.label).collect();
        labels.sort();
        labels.dedup();
        assert_eq!(labels.len(), GOODS.len(), "two goods share a label");
    }

    /// Every kind of business has its row, at its own index, with a name.
    #[test]
    fn every_business_has_a_complete_row() {
        assert_eq!(BUSINESSES.len(), BusinessKind::ALL.len());
        for (i, k) in BusinessKind::ALL.into_iter().enumerate() {
            assert_eq!(business_index(k), i, "{k:?} out of order in BusinessKind::ALL");
            assert_eq!(k.index(), i, "{k:?}: ALL is not in declaration order");
            let b = business(k);
            assert_eq!(b.kind, k, "row {i} is {:?}, not {k:?}", b.kind);
            assert!(!b.label.is_empty(), "{k:?} has no label");
            assert!(b.stock_hours > 0.0 && b.tab > 0.0 && b.start_capital > 0.0, "{k:?}");
            if let Some(r) = b.walk_in {
                assert!(b.outputs.iter().any(|(x, _)| *x == r), "{k:?} sells {r:?} over the counter but does not make it");
            }
            if let Some((r, n)) = b.home_order {
                assert!(n > 0.0 && b.walk_in == Some(r), "{k:?} delivers {r:?} it does not sell");
            }
        }
        let mut labels: Vec<&str> = BUSINESSES.iter().map(|b| b.label).collect();
        labels.sort();
        labels.dedup();
        assert_eq!(labels.len(), BUSINESSES.len(), "two kinds share a label");
    }

    /// [r13-catalogue] Plastics travel as pellets in bags: a dry box, not
    /// the round tank container (Harvey: "some containers are round like
    /// fluid ones"). Only liquids go in tanks.
    #[test]
    fn only_liquids_ship_in_tanks() {
        let tanks: Vec<Resource> = Resource::ALL.into_iter().filter(|r| good(*r).ships == BoxKind::Tank).collect();
        assert_eq!(tanks, vec![R::Crude, R::Fuel]);
        assert!(tanks.iter().all(|r| good(*r).unit == Unit::Kilolitre));
        assert_eq!(BoxKind::for_resource(Some(R::Plastics)), BoxKind::Dry);
    }

    /// Every good someone uses can be had: made in town, or it is a site
    /// material or comes from outside the recipes.
    #[test]
    fn every_used_good_has_a_source() {
        for r in Resource::ALL {
            if users(r).next().is_some() {
                assert!(makers(r).next().is_some() || !good(r).note.is_empty(), "{r:?} is used but never made");
            }
        }
        assert!(makers(SITE_BILL.material).next().is_some());
    }
}
