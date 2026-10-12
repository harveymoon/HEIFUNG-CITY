//! Business pressure: what the city is short of, and which business would
//! fill the gap.
//!
//! Most businesses open the way they always have (`world/develop.rs`): a
//! vacant lot develops when its land use is in demand, and the business it
//! hosts is whatever the market can carry (a kind's cover, takings over
//! costs, and room on the shelves), or for industry whatever the ground and
//! the neighbours favour. Parking lots follow drivers who could not find a
//! space; taxi firms follow trips that wanted a cab; hotels follow visitors
//! turned away for want of a bed.
//!
//! Pressures add the trades a city needs for reasons the market alone does
//! not see. Each [`PressureKind`] watches a cause in the city (tows done by
//! companies from out of town, wrecks and cars to service, wealthy
//! households, bins the city's trucks never reach, people buying bikes and
//! walking far, livestock and fish with nobody to process them, a rich
//! audience and the artists to serve it) and turns it into a number of
//! businesses the cause would keep busy (`wanted`). Against what stands
//! (`have`, open or being built) that gives a `gap`: one business' worth of
//! unmet need is full strength. A pressure at or over [`OPEN_GAP`] for
//! [`OPEN_DAYS`] days in a row opens one more business of its kind on a
//! sensible lot (`world/pressure.rs`), then the gap shrinks by one; with
//! enough of them the pressure falls to nothing. A business that cannot pay
//! its way still closes as any other does (five days deep in debt), and its
//! kind then waits [`COOLDOWN_DAYS`] before the pressure may open another,
//! so a city that cannot carry one does not see it open and shut forever.
//!
//! Everything here is pure: the World gathers the [`Signals`] and the
//! [`Supply`], [`assess`] reads them.

use crate::market::BusinessKind;
use serde::{Deserialize, Serialize};

/// A gap of this many businesses opens one more.
pub const OPEN_GAP: f32 = 0.75;
/// Days in a row the gap must hold first.
pub const OPEN_DAYS: u8 = 2;
/// Days a kind waits after one of it closed broke.
pub const COOLDOWN_DAYS: u16 = 10;
/// Days after one of a kind opened (and while one is still being built)
/// before the pressure may open another: time to see the effect, as the
/// smoothed cause takes a few days to fall.
pub const SETTLE_DAYS: u64 = 4;
/// How fast the daily signals follow the day's counts (share of today).
pub const SMOOTHING: f32 = 0.4;
/// The same when the day's count is lower: needs fade faster than they grow.
pub const SMOOTHING_DOWN: f32 = 0.75;

/// Tows a day one local tow company takes off the out-of-town firms.
pub const TOWS_PER_COMPANY: f32 = 6.0;
/// Repairs and services a day a repair shop handles.
pub const REPAIRS_PER_SHOP: f32 = 9.0;
/// Cars in town per service a day (each car is serviced every few months).
pub const CARS_PER_SERVICE: f32 = 90.0;
/// Wealthy households one dealership lives on.
pub const RICH_PER_DEALER: f32 = 250.0;
/// Cars bought a day that keep a dealership busy.
pub const SALES_PER_DEALER: f32 = 25.0;
/// Buildings outside the city's trash rounds one trash company serves.
pub const CLIENTS_PER_TRASH_CO: f32 = 150.0;
/// Bike owners a bike shop keeps on the road.
pub const RIDERS_PER_BIKE_SHOP: f32 = 900.0;
/// Long walks a day that would sell a bike shop's worth of bikes.
pub const WALKS_PER_BIKE_SHOP: f32 = 2000.0;
/// Bikes bought a day a bike shop sells.
pub const SALES_PER_BIKE_SHOP: f32 = 60.0;
/// Livestock a day one slaughterhouse takes.
pub const LIVESTOCK_PER_SLAUGHTERHOUSE: f32 = 110.0;
/// Fish a day one gutting shed takes.
pub const FISH_PER_GUTTING: f32 = 90.0;
/// Fillets a day one fish-stick factory takes.
pub const FILLETS_PER_FACTORY: f32 = 80.0;
/// Comfortable and rich adults per driving range.
pub const GOLFERS_PER_RANGE: f32 = 650.0;
/// Artists per studio building.
pub const ARTISTS_PER_STUDIO: f32 = 25.0;
/// Rich adults per gallery.
pub const RICH_PER_GALLERY: f32 = 300.0;
/// The art scene needs this many artists of middle income ...
pub const SCENE_MIN_ARTISTS: f32 = 4.0;
/// ... and this many rich adults to buy.
pub const SCENE_MIN_AUDIENCE: f32 = 20.0;
/// Wealthy households near the water per small marina's worth of berths
/// (about one in eight or nine keeps a boat; a small marina has 10 berths
/// on the map, standing for a few dozen). Was 150: a 17k city on the
/// water then wanted 5-6 marinas with two built (H4).
pub const RICH_PER_MARINA: f32 = 250.0;
/// (H4) Residents per small marina's worth of berths at most, however rich:
/// about one berth per hundred people, a small marina standing for some
/// thirty. Harvey's 17k city, half its households wealthy and by the
/// water, wants about six, not the cap of eight.
pub const PEOPLE_PER_MARINA: f32 = 3000.0;
/// A large marina's berths, counted in small marinas (22 berths to 10).
pub const LARGE_MARINA_WORTH: f32 = 2.2;
/// [waterfront] Visitors in town per beach resort's worth of guests.
pub const TOURISTS_PER_RESORT: f32 = 60.0;
/// [waterfront] Comfortable adults, and visitors, per restaurant on a pier.
pub const DINERS_PER_PIER: f32 = 900.0;
pub const TOURISTS_PER_PIER: f32 = 80.0;
/// [waterfront] Bather-hours a day per beach bar.
pub const BATHERS_PER_BAR: f32 = 160.0;
/// [waterfront] Visitors, and wealthy households near the water, per boat rental.
pub const TOURISTS_PER_RENTAL: f32 = 90.0;
pub const RICH_PER_RENTAL: f32 = 300.0;
/// A pressure never asks for more than this many of a kind at once.
pub const MAX_WANTED: f32 = 8.0;

/// What a pressure asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PressureKind {
    TowCompany,
    Mechanic,
    CarDealer,
    TrashCompany,
    BikeShop,
    Slaughterhouse,
    FishGutting,
    FishStickFactory,
    FishMarket,
    SeafoodRestaurant,
    GolfRange,
    ArtStudio,
    Gallery,
    /// Advisory: the player places marinas (Build > Water).
    Marina,
    // -- [waterfront] trades on lots that run down to the water --
    BeachResort,
    PierRestaurant,
    BeachBar,
    BoatRental,
}

impl PressureKind {
    pub const ALL: [PressureKind; 18] = [
        PressureKind::TowCompany,
        PressureKind::Mechanic,
        PressureKind::CarDealer,
        PressureKind::TrashCompany,
        PressureKind::BikeShop,
        PressureKind::Slaughterhouse,
        PressureKind::FishGutting,
        PressureKind::FishStickFactory,
        PressureKind::FishMarket,
        PressureKind::SeafoodRestaurant,
        PressureKind::GolfRange,
        PressureKind::ArtStudio,
        PressureKind::Gallery,
        PressureKind::Marina,
        PressureKind::BeachResort,
        PressureKind::PierRestaurant,
        PressureKind::BeachBar,
        PressureKind::BoatRental,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    /// The business it opens.
    pub fn business(self) -> Option<BusinessKind> {
        Some(match self {
            PressureKind::TowCompany => BusinessKind::TowYard,
            PressureKind::Mechanic => BusinessKind::Mechanic,
            PressureKind::CarDealer => BusinessKind::CarDealer,
            PressureKind::TrashCompany => BusinessKind::TrashCompany,
            PressureKind::BikeShop => BusinessKind::BikeShop,
            PressureKind::Slaughterhouse => BusinessKind::Slaughterhouse,
            PressureKind::FishGutting => BusinessKind::FishGutting,
            PressureKind::FishStickFactory => BusinessKind::FishStickFactory,
            PressureKind::FishMarket => BusinessKind::FishMarket,
            PressureKind::SeafoodRestaurant => BusinessKind::SeafoodRestaurant,
            PressureKind::GolfRange => BusinessKind::GolfRange,
            PressureKind::ArtStudio => BusinessKind::ArtStudio,
            PressureKind::Gallery => BusinessKind::Gallery,
            PressureKind::Marina => BusinessKind::Marina,
            PressureKind::BeachResort => BusinessKind::BeachResort,
            PressureKind::PierRestaurant => BusinessKind::PierRestaurant,
            PressureKind::BeachBar => BusinessKind::BeachBar,
            PressureKind::BoatRental => BusinessKind::BoatRental,
        })
    }

    /// The pressure that opens businesses of `kind`, if one does.
    pub fn of_business(kind: BusinessKind) -> Option<PressureKind> {
        PressureKind::ALL.into_iter().find(|p| p.business() == Some(kind))
    }

    /// The player builds these; the pressure only says they would pay.
    pub fn player_placed(self) -> bool {
        matches!(self, PressureKind::Marina)
    }

    pub fn label(self) -> &'static str {
        match self {
            PressureKind::TowCompany => "tow company",
            PressureKind::Mechanic => "car repair shop",
            PressureKind::CarDealer => "car dealership",
            PressureKind::TrashCompany => "trash company",
            PressureKind::BikeShop => "bike shop",
            PressureKind::Slaughterhouse => "slaughterhouse",
            PressureKind::FishGutting => "fish gutting shed",
            PressureKind::FishStickFactory => "fish-stick factory",
            PressureKind::FishMarket => "fish market",
            PressureKind::SeafoodRestaurant => "seafood restaurant",
            PressureKind::GolfRange => "golf driving range",
            PressureKind::ArtStudio => "artists' studios",
            PressureKind::Gallery => "gallery",
            PressureKind::Marina => "marina",
            PressureKind::BeachResort => "beach resort",
            PressureKind::PierRestaurant => "pier restaurant",
            PressureKind::BeachBar => "beach bar",
            PressureKind::BoatRental => "boat rental",
        }
    }

    /// Short key for the API.
    pub fn key(self) -> &'static str {
        match self {
            PressureKind::TowCompany => "tow_company",
            PressureKind::Mechanic => "mechanic",
            PressureKind::CarDealer => "car_dealer",
            PressureKind::TrashCompany => "trash_company",
            PressureKind::BikeShop => "bike_shop",
            PressureKind::Slaughterhouse => "slaughterhouse",
            PressureKind::FishGutting => "fish_gutting",
            PressureKind::FishStickFactory => "fish_stick_factory",
            PressureKind::FishMarket => "fish_market",
            PressureKind::SeafoodRestaurant => "seafood_restaurant",
            PressureKind::GolfRange => "golf_range",
            PressureKind::ArtStudio => "art_studio",
            PressureKind::Gallery => "gallery",
            PressureKind::Marina => "marina",
            PressureKind::BeachResort => "beach_resort",
            PressureKind::PierRestaurant => "pier_restaurant",
            PressureKind::BeachBar => "beach_bar",
            PressureKind::BoatRental => "boat_rental",
        }
    }

    pub fn from_key(s: &str) -> Option<PressureKind> {
        let s = s.trim().to_ascii_lowercase().replace([' ', '-'], "_");
        PressureKind::ALL.into_iter().find(|p| p.key() == s)
    }
}

/// The causes, smoothed per day (`SMOOTHING`), and the counts read fresh
/// each day. Saved, so a loaded city keeps its pressure.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Signals {
    /// Tows a day done by the company from out of town.
    pub outside_tows: f32,
    /// Tows a day done by companies in town.
    pub local_tows: f32,
    /// Crashes a day.
    pub crashes: f32,
    /// Private cars in town.
    pub cars: f32,
    /// Households with a rich income.
    pub rich_households: f32,
    /// Cars bought a day, and of them bought out of town.
    pub cars_bought: f32,
    pub cars_bought_outside: f32,
    /// Buildings with bins outside every city trash round.
    pub trash_uncovered: f32,
    /// Buildings whose bins overflow.
    pub trash_overflowing: f32,
    /// City trash depots crewed.
    pub depots: f32,
    /// People who own a bike.
    pub bike_owners: f32,
    /// Bikes bought a day.
    pub bikes_bought: f32,
    /// Walks of 25 minutes or more a day.
    pub long_walks: f32,
    /// Livestock raised a day by the cattle and chicken farms.
    pub livestock: f32,
    /// Fish landed a day by fishing ports and fisheries.
    pub fish: f32,
    /// Fillets made a day by the gutting sheds.
    pub fillets: f32,
    /// Fishing ports working: small and large.
    pub fishing_small: f32,
    pub fishing_large: f32,
    /// Marinas working: small and large.
    pub marinas_small: f32,
    pub marinas_large: f32,
    /// Adults of middle or rich income (who might hit a bucket of balls).
    pub comfortable_adults: f32,
    /// Artists in town, and those of them of middle income or better.
    pub artists: f32,
    pub artists_mid: f32,
    /// Rich adults: the art audience.
    pub rich_adults: f32,
    /// Wealthy households within reach of the water.
    pub rich_near_water: f32,
    /// Shoreline the player could build on (any water at all).
    pub has_water: bool,
    /// [waterfront] Visitors from out of town in town now.
    pub tourists: f32,
    /// [waterfront] Bather-hours on the public beaches yesterday (a flow).
    pub beach_visits: f32,
    /// [waterfront] Public beaches open and reachable.
    pub beaches_open: f32,
    /// [waterfront] How inviting a beach is this week, `0..1`.
    pub beach_appeal: f32,
    /// [waterfront] Vacant lots on the water to build on.
    pub waterfront_sites: f32,
    /// (H4) Residents in town.
    #[serde(default)]
    pub people: f32,
}

impl Signals {
    /// Blend today's reading into the smoothed one: the flows (per day)
    /// smooth, the stocks (counts) are taken as they are.
    pub fn blend(&mut self, today: &Signals) {
        // A need builds up over days and fades fast once it is met, so a
        // trade that just opened is not followed by another on old news.
        let mix = |old: &mut f32, new: f32| *old += (new - *old) * if new < *old { SMOOTHING_DOWN } else { SMOOTHING };
        mix(&mut self.outside_tows, today.outside_tows);
        mix(&mut self.local_tows, today.local_tows);
        mix(&mut self.crashes, today.crashes);
        mix(&mut self.cars_bought, today.cars_bought);
        mix(&mut self.cars_bought_outside, today.cars_bought_outside);
        mix(&mut self.bikes_bought, today.bikes_bought);
        mix(&mut self.long_walks, today.long_walks);
        mix(&mut self.livestock, today.livestock);
        mix(&mut self.fish, today.fish);
        mix(&mut self.fillets, today.fillets);
        mix(&mut self.beach_visits, today.beach_visits);
        self.cars = today.cars;
        self.rich_households = today.rich_households;
        self.trash_uncovered = today.trash_uncovered;
        self.trash_overflowing = today.trash_overflowing;
        self.depots = today.depots;
        self.bike_owners = today.bike_owners;
        self.fishing_small = today.fishing_small;
        self.fishing_large = today.fishing_large;
        self.marinas_small = today.marinas_small;
        self.marinas_large = today.marinas_large;
        self.comfortable_adults = today.comfortable_adults;
        self.artists = today.artists;
        self.artists_mid = today.artists_mid;
        self.rich_adults = today.rich_adults;
        self.rich_near_water = today.rich_near_water;
        self.has_water = today.has_water;
        self.tourists = today.tourists;
        self.beaches_open = today.beaches_open;
        self.beach_appeal = today.beach_appeal;
        self.waterfront_sites = today.waterfront_sites;
        self.people = today.people;
    }

    /// Is there an art scene: enough artists getting by and enough money
    /// to buy what they make?
    pub fn art_scene(&self) -> bool {
        self.artists_mid >= SCENE_MIN_ARTISTS && self.rich_adults >= SCENE_MIN_AUDIENCE
    }
}

/// What stands of each kind: open, being built, and closed broke lately.
#[derive(Clone, Debug, PartialEq)]
pub struct Supply {
    pub open: [u32; BusinessKind::COUNT],
    pub building: [u32; BusinessKind::COUNT],
    /// Of the open ones, those losing money lately.
    pub struggling: [u32; BusinessKind::COUNT],
}

impl Default for Supply {
    fn default() -> Self {
        Supply { open: [0; BusinessKind::COUNT], building: [0; BusinessKind::COUNT], struggling: [0; BusinessKind::COUNT] }
    }
}

impl Supply {
    pub fn have(&self, k: BusinessKind) -> u32 {
        self.open[k.index()] + self.building[k.index()]
    }
}

/// One pressure, read for the day.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pressure {
    pub kind: PressureKind,
    /// Businesses of the kind the cause would keep busy.
    pub wanted: f32,
    /// Open and being built.
    pub have: u32,
    /// Open but losing money lately.
    #[serde(default)]
    pub struggling: u32,
    /// `wanted - have`: unmet need in businesses.
    pub gap: f32,
    /// `gap` as `0..1`: one business of unmet need is full strength.
    pub strength: f32,
    /// Why, in a sentence: the cause and what stands.
    pub why: String,
    /// Why nothing opens although the gap is there (cooling down, a
    /// player-placed kind, no lot), if so.
    #[serde(default)]
    pub held: Option<String>,
}

impl Pressure {
    fn new(kind: PressureKind, wanted: f32, supply: &Supply, why: String) -> Pressure {
        let wanted = wanted.clamp(0.0, MAX_WANTED);
        let (have, struggling) = match kind.business() {
            Some(b) => (supply.have(b), supply.struggling[b.index()]),
            None => (0, 0),
        };
        let gap = wanted - have as f32;
        Pressure { kind, wanted, have, struggling, gap, strength: gap.clamp(0.0, 1.0), why, held: None }
    }

    /// Strong enough to open one more.
    pub fn wants_more(&self) -> bool {
        self.gap >= OPEN_GAP
    }
}

fn plural(n: f32, one: &str, many: &str) -> String {
    let r = n.round() as i64;
    format!("{r} {}", if r == 1 { one } else { many })
}

fn have_text(have: u32, label: &str) -> String {
    match have {
        0 => format!("no {label} in town"),
        1 => format!("one {label} in town"),
        n => format!("{n} of them in town"),
    }
}

/// Every pressure, read from the signals against what stands.
pub fn assess(s: &Signals, supply: &Supply) -> Vec<Pressure> {
    let mut out = Vec::with_capacity(PressureKind::ALL.len());
    for kind in PressureKind::ALL {
        let have = kind.business().map_or(0, |b| supply.have(b));
        let ht = have_text(have, kind.label());
        let (wanted, why) = match kind {
            PressureKind::TowCompany => {
                // The open local companies take the jobs they can; what the
                // firm from out of town still does is the unmet part (a
                // company still being built takes none yet).
                let wanted = supply.open[BusinessKind::TowYard.index()] as f32 + s.outside_tows / TOWS_PER_COMPANY;
                (wanted, format!("{} a day towed by the company from out of town ({} local); {ht}", plural(s.outside_tows, "car", "cars"), plural(s.local_tows, "tow", "tows")))
            }
            PressureKind::Mechanic => {
                let services = s.cars / CARS_PER_SERVICE;
                let work = s.crashes + services;
                (work / REPAIRS_PER_SHOP, format!("{} a day to mend and {} cars to keep on the road ({} services a day); {ht}", plural(s.crashes, "wreck", "wrecks"), s.cars.round(), services.round()))
            }
            PressureKind::CarDealer => {
                let wanted = s.rich_households / RICH_PER_DEALER + s.cars_bought / SALES_PER_DEALER;
                let outside = if s.cars_bought_outside >= 0.5 { format!(", {} of them from out of town", s.cars_bought_outside.round()) } else { String::new() };
                (wanted, format!("{} wealthy households and {} cars bought a day{outside}; {ht}", s.rich_households.round(), s.cars_bought.round()))
            }
            PressureKind::TrashCompany => {
                let wanted = s.trash_uncovered / CLIENTS_PER_TRASH_CO;
                let depots = match s.depots.round() as i64 {
                    0 => "any city trash round (the city runs no depot)".to_string(),
                    1 => "the rounds of the city's one depot".to_string(),
                    n => format!("the rounds of the city's {n} depots"),
                };
                (wanted, format!("{} buildings beyond {depots}, {} overflowing; {ht}", s.trash_uncovered.round(), s.trash_overflowing.round()))
            }
            PressureKind::BikeShop => {
                let wanted = s.bike_owners / RIDERS_PER_BIKE_SHOP + s.long_walks / WALKS_PER_BIKE_SHOP + s.bikes_bought / SALES_PER_BIKE_SHOP;
                (wanted, format!("{} bike owners, {} bought a day and {} long walks a day; {ht}", s.bike_owners.round(), s.bikes_bought.round(), s.long_walks.round()))
            }
            PressureKind::Slaughterhouse => {
                (s.livestock / LIVESTOCK_PER_SLAUGHTERHOUSE, format!("{} livestock a day from the cattle and chicken farms; {ht}", s.livestock.round()))
            }
            PressureKind::FishGutting => (s.fish / FISH_PER_GUTTING, format!("{} fish landed a day; {ht}", s.fish.round())),
            PressureKind::FishStickFactory => {
                // A gutting shed's fillets need a buyer before they pile up:
                // a factory follows the sheds as well as their output.
                let sheds = supply.open[BusinessKind::FishGutting.index()] as f32;
                (s.fillets / FILLETS_PER_FACTORY + 0.6 * sheds, format!("{} fillets a day from {} gutting sheds; {ht}", s.fillets.round(), sheds))
            }
            PressureKind::FishMarket => {
                let ports = s.fishing_small + s.fishing_large;
                (0.8 * s.fishing_small + 1.6 * s.fishing_large, format!("{} fishing {} landing a catch; {ht}", ports.round(), if ports.round() == 1.0 { "port" } else { "ports" }))
            }
            PressureKind::SeafoodRestaurant => {
                let wanted = 0.8 * s.marinas_small + 1.8 * s.marinas_large + 0.5 * (s.fishing_small + s.fishing_large);
                (wanted, format!("{} marinas and {} fishing ports draw diners to the water; {ht}", (s.marinas_small + s.marinas_large).round(), (s.fishing_small + s.fishing_large).round()))
            }
            PressureKind::GolfRange => (s.comfortable_adults / GOLFERS_PER_RANGE, format!("{} comfortable and rich adults with an evening to fill; {ht}", s.comfortable_adults.round())),
            PressureKind::ArtStudio => {
                let wanted = if s.art_scene() { s.artists / ARTISTS_PER_STUDIO } else { 0.0 };
                (wanted, format!("{} artists ({} getting by), {} rich adults to buy{}; {ht}", s.artists.round(), s.artists_mid.round(), s.rich_adults.round(), scene_note(s)))
            }
            PressureKind::Gallery => {
                let studios = supply.open[BusinessKind::ArtStudio.index()] as f32;
                let wanted = if s.art_scene() { (s.rich_adults / RICH_PER_GALLERY).min(studios * 0.75) } else { 0.0 };
                (wanted, format!("{} rich adults and {} studios open{}; {ht}", s.rich_adults.round(), studios, scene_note(s)))
            }
            PressureKind::Marina => {
                let by_people = if s.people > 0.0 { s.people / PEOPLE_PER_MARINA } else { f32::MAX };
                let wanted = if s.has_water { (s.rich_near_water / RICH_PER_MARINA).min(by_people) } else { 0.0 };
                let wanted = wanted.max(0.0);
                let why = if s.has_water {
                    format!("{} wealthy households near the water would keep a boat: berths for about {wanted:.1} small marinas ({}); a large marina counts as {LARGE_MARINA_WORTH}", s.rich_near_water.round(), marinas_built(s))
                } else {
                    "no water to build a marina on".to_string()
                };
                (wanted, why)
            }
            // [waterfront] The trades of the shore, only where there is one.
            PressureKind::BeachResort => {
                let wanted = if s.has_water { (s.tourists / TOURISTS_PER_RESORT + 0.4 * s.beaches_open) * (0.25 + 0.75 * s.beach_appeal) } else { 0.0 };
                (wanted, format!("{} visitors in town, {} public beaches open, beach weather {:.0}%; {ht}", s.tourists.round(), s.beaches_open, s.beach_appeal * 100.0))
            }
            PressureKind::PierRestaurant => {
                let wanted = if s.has_water { s.comfortable_adults / DINERS_PER_PIER + s.tourists / TOURISTS_PER_PIER } else { 0.0 };
                (wanted, format!("{} comfortable adults and {} visitors would dine over the water; {ht}", s.comfortable_adults.round(), s.tourists.round()))
            }
            PressureKind::BeachBar => {
                let wanted = if s.has_water { s.beach_visits / BATHERS_PER_BAR + 0.5 * s.beaches_open * s.beach_appeal } else { 0.0 };
                (wanted, format!("{} bather-hours a day on {} public beaches; {ht}", s.beach_visits.round(), s.beaches_open))
            }
            PressureKind::BoatRental => {
                let wanted = if s.has_water { (s.tourists / TOURISTS_PER_RENTAL + s.rich_near_water / RICH_PER_RENTAL) * (0.4 + 0.6 * s.beach_appeal) } else { 0.0 };
                (wanted, format!("{} visitors and {} wealthy households near the water want to get out on it; {ht}", s.tourists.round(), s.rich_near_water.round()))
            }
        };
        let mut p = Pressure::new(kind, wanted, supply, why);
        if kind == PressureKind::Marina {
            // The player's docks are not businesses the supply counts. A
            // large marina's berths count as more than one small one's.
            p.have = (s.marinas_small + s.marinas_large).round() as u32;
            let worth = s.marinas_small + LARGE_MARINA_WORTH * s.marinas_large;
            p.gap = p.wanted - worth;
            p.strength = p.gap.clamp(0.0, 1.0);
            if p.wants_more() {
                // H4: say what stands, not just that it is held.
                let want = p.wanted.round().max(1.0) as u32;
                p.held = Some(match p.have {
                    0 => format!("wants about {want} marina{}, you have none: Build > Water", if want == 1 { "" } else { "s" }),
                    _ => format!("wants berths for about {want} small marinas, you have {} (worth {worth:.1}): Build > Water", marinas_built(s)),
                });
            }
        }
        out.push(p);
    }
    out
}

/// "2 (1 large)" / "1 small": the marinas the player has built.
fn marinas_built(s: &Signals) -> String {
    let (small, large) = (s.marinas_small.round() as u32, s.marinas_large.round() as u32);
    match (small, large) {
        (0, 0) => "none built".to_string(),
        (n, 0) => format!("{n} small"),
        (0, n) => format!("{n} large"),
        (a, b) => format!("{} ({b} large)", a + b),
    }
}

fn scene_note(s: &Signals) -> String {
    if s.art_scene() {
        String::new()
    } else if s.artists_mid < SCENE_MIN_ARTISTS {
        format!(" (no scene yet: needs {SCENE_MIN_ARTISTS:.0} artists of middle income)")
    } else {
        format!(" (no scene yet: needs {SCENE_MIN_AUDIENCE:.0} rich adults to buy)")
    }
}

/// The saved state of the pressures: smoothed signals, the last reading,
/// days each has held, cooldowns, and the counters behind the flows.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PressureState {
    pub signals: Signals,
    pub readings: Vec<Pressure>,
    /// Days in a row each pressure has wanted more (`PressureKind::index`).
    pub held_days: Vec<u8>,
    /// Days left before a kind may open again after one closed broke.
    pub cooldown: Vec<u16>,
    /// Businesses the pressures have opened, by kind key.
    pub opened_total: std::collections::BTreeMap<String, u32>,
    /// Totals read yesterday, to turn running totals into a day's count.
    pub last_outside_tows: u32,
    pub last_tows: u32,
    pub last_crashes: u32,
    pub last_bought: u32,
    pub last_bought_outside: u32,
    /// The day one of each kind last opened (`PressureKind::index`).
    pub last_opened_day: Vec<u64>,
    /// Walks of 25 minutes or more today (`World::end_trip`).
    pub long_walks_today: u32,
    /// Set once the running totals have been read: the first reading of a
    /// city (or an old save) counts no flows, not everything since day one.
    pub primed: bool,
    /// What the repair and bike shops charged today, billed at midnight.
    pub car_bill_today: f32,
    pub bike_bill_today: f32,
}

impl PressureState {
    pub fn reading(&self, k: PressureKind) -> Option<&Pressure> {
        self.readings.iter().find(|p| p.kind == k)
    }

    pub fn cooling(&self, k: PressureKind) -> u16 {
        self.cooldown.get(k.index()).copied().unwrap_or(0)
    }

    pub(crate) fn pad(&mut self) {
        let n = PressureKind::ALL.len();
        self.held_days.resize(n, 0);
        self.cooldown.resize(n, 0);
        self.last_opened_day.resize(n, 0);
    }

    /// Still waiting to see what the last one it opened does.
    pub fn settling(&self, k: PressureKind, day: u64, sites: u32) -> bool {
        let last = self.last_opened_day.get(k.index()).copied().unwrap_or(0);
        sites > 0 || (last > 0 && day < last + SETTLE_DAYS)
    }
}

/// [economy] A gallery opening: an evening event that invites the town.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Opening {
    pub gallery: crate::buildings::BuildingId,
    /// Doors open and close, in ticks.
    pub from: u64,
    pub until: u64,
    /// People who came, and what the wealthy among them gave.
    pub visitors: u32,
    pub donations: f32,
    pub sales: u32,
    /// The guest list.
    #[serde(default)]
    pub invited: Vec<crate::citizens::CitizenId>,
}

impl Opening {
    pub fn on_at(&self, tick: u64) -> bool {
        (self.from..self.until).contains(&tick)
    }
}

/// Hours an opening runs, from 18:00.
pub const OPENING_START_HOUR: u64 = 18;
pub const OPENING_HOURS: u64 = 4;
/// Days between a gallery's openings, about.
pub const OPENING_EVERY_DAYS: u64 = 6;
/// Lifetime donations past which a gallery that pays its way wants to grow
/// (the land agent's hook, `Business::wants_expand`).
pub const EXPAND_DONATIONS: f32 = 1500.0;
/// The most a delighted rich visitor gives at once.
pub const MAX_DONATION: f32 = 60.0;

/// [economy] The art scene and the waterfront's running state.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Scene {
    /// Openings tonight and lately (the last few), oldest first.
    pub openings: Vec<Opening>,
    pub openings_total: u32,
    pub donations_total: f32,
    pub art_sold_total: u32,
    /// Boats out of the marinas and fishing ports now, and the catch.
    pub boats_out: u32,
    pub fish_landed_today: f32,
    pub berth_fees_today: f32,
    /// Repairs done today by the repair shops, and bikes mended.
    pub repairs_today: f32,
    pub bike_repairs_today: f32,
}

impl Scene {
    /// The opening at `gallery` on now, if any.
    pub fn opening_at(&self, gallery: crate::buildings::BuildingId, tick: u64) -> Option<&Opening> {
        self.openings.iter().rev().find(|o| o.gallery == gallery && o.on_at(tick))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn supply_with(k: BusinessKind, n: u32) -> Supply {
        let mut s = Supply::default();
        s.open[k.index()] = n;
        s
    }

    fn get(v: &[Pressure], k: PressureKind) -> Pressure {
        v.iter().find(|p| p.kind == k).unwrap().clone()
    }

    #[test]
    fn every_pressure_names_its_business_and_round_trips_its_key() {
        for k in PressureKind::ALL {
            assert_eq!(PressureKind::from_key(k.key()), Some(k));
            let b = k.business().unwrap();
            assert_eq!(PressureKind::of_business(b), Some(k));
        }
    }

    #[test]
    fn outside_tows_ask_for_a_tow_company_until_the_locals_take_them() {
        let s = Signals { outside_tows: 12.0, ..Default::default() };
        let p = get(&assess(&s, &Supply::default()), PressureKind::TowCompany);
        assert!(p.wants_more() && p.strength == 1.0, "{p:?}");
        assert!(p.why.contains("out of town"));
        // With a company in town the out-of-town tows dry up: no gap.
        let s = Signals { outside_tows: 0.5, local_tows: 9.0, ..Default::default() };
        let p = get(&assess(&s, &supply_with(BusinessKind::TowYard, 1)), PressureKind::TowCompany);
        assert!(!p.wants_more() && p.strength < 0.2, "{p:?}");
    }

    #[test]
    fn each_cause_raises_its_pressure_and_supply_meets_it() {
        let cases: Vec<(Signals, PressureKind)> = vec![
            (Signals { crashes: 10.0, cars: 900.0, ..Default::default() }, PressureKind::Mechanic),
            (Signals { rich_households: 400.0, cars_bought: 10.0, ..Default::default() }, PressureKind::CarDealer),
            (Signals { trash_uncovered: 300.0, ..Default::default() }, PressureKind::TrashCompany),
            (Signals { bike_owners: 1200.0, long_walks: 1000.0, ..Default::default() }, PressureKind::BikeShop),
            (Signals { livestock: 160.0, ..Default::default() }, PressureKind::Slaughterhouse),
            (Signals { fish: 140.0, ..Default::default() }, PressureKind::FishGutting),
            (Signals { fillets: 120.0, ..Default::default() }, PressureKind::FishStickFactory),
            (Signals { fishing_large: 1.0, ..Default::default() }, PressureKind::FishMarket),
            (Signals { marinas_large: 1.0, ..Default::default() }, PressureKind::SeafoodRestaurant),
            (Signals { comfortable_adults: 1500.0, ..Default::default() }, PressureKind::GolfRange),
            (Signals { artists: 40.0, artists_mid: 8.0, rich_adults: 60.0, ..Default::default() }, PressureKind::ArtStudio),
        ];
        for (s, k) in cases {
            let quiet = get(&assess(&Signals::default(), &Supply::default()), k);
            assert!(!quiet.wants_more(), "{k:?} without a cause: {quiet:?}");
            let p = get(&assess(&s, &Supply::default()), k);
            assert!(p.wants_more(), "{k:?} with its cause: {p:?}");
            // Enough of them, and the pressure is gone.
            let n = p.wanted.ceil() as u32;
            let met = get(&assess(&s, &supply_with(k.business().unwrap(), n)), k);
            assert!(!met.wants_more() && met.strength == 0.0, "{k:?} met by {n}: {met:?}");
        }
    }

    #[test]
    fn the_art_scene_needs_artists_and_an_audience() {
        let poor = Signals { artists: 30.0, artists_mid: 2.0, rich_adults: 100.0, ..Default::default() };
        assert!(!get(&assess(&poor, &Supply::default()), PressureKind::ArtStudio).wants_more());
        let no_buyers = Signals { artists: 30.0, artists_mid: 10.0, rich_adults: 5.0, ..Default::default() };
        let p = get(&assess(&no_buyers, &Supply::default()), PressureKind::ArtStudio);
        assert!(!p.wants_more() && p.why.contains("no scene yet"), "{p:?}");
        let scene = Signals { artists: 60.0, artists_mid: 10.0, rich_adults: 700.0, ..Default::default() };
        assert!(get(&assess(&scene, &Supply::default()), PressureKind::ArtStudio).wants_more());
        // Galleries follow the studios.
        assert!(!get(&assess(&scene, &Supply::default()), PressureKind::Gallery).wants_more());
        assert!(get(&assess(&scene, &supply_with(BusinessKind::ArtStudio, 3)), PressureKind::Gallery).wants_more());
    }

    #[test]
    fn marinas_are_advice_for_the_player() {
        let s = Signals { rich_near_water: 400.0, has_water: true, ..Default::default() };
        let p = get(&assess(&s, &Supply::default()), PressureKind::Marina);
        assert!(p.wants_more() && p.held.as_deref().is_some_and(|h| h.contains("Build")));
        let dry = Signals { rich_near_water: 200.0, ..Default::default() };
        assert!(!get(&assess(&dry, &Supply::default()), PressureKind::Marina).wants_more());
    }

    /// H4: with marinas built the advice says how many stand, and a large
    /// marina's berths count for more than a small one's.
    #[test]
    fn marina_advice_counts_what_is_built() {
        let s = Signals { rich_near_water: 1500.0, has_water: true, marinas_small: 2.0, ..Default::default() };
        let p = get(&assess(&s, &Supply::default()), PressureKind::Marina);
        assert_eq!(p.have, 2);
        let held = p.held.expect("wants more");
        assert!(held.contains("wants berths for about 6") && held.contains("you have 2"), "{held}");
        // A city of 17,600 with 3,468 wealthy households by the water (Harvey's
        // day 20) wants about six, not the cap.
        let rich = Signals { rich_near_water: 3468.0, people: 17_627.0, has_water: true, marinas_small: 1.0, marinas_large: 1.0, ..Default::default() };
        let p = get(&assess(&rich, &Supply::default()), PressureKind::Marina);
        assert!((p.wanted - 5.9).abs() < 0.1, "{p:?}");
        assert!(p.held.as_deref().is_some_and(|h| h.contains("about 6") && h.contains("you have 2")), "{:?}", p.held);
        // Harvey's 17k city on day 11: ~840 wealthy households by the water,
        // one small and one large marina: met.
        let h4 = Signals { rich_near_water: 840.0, has_water: true, marinas_small: 1.0, marinas_large: 1.0, ..Default::default() };
        let p = get(&assess(&h4, &Supply::default()), PressureKind::Marina);
        assert!(!p.wants_more(), "{p:?}");
        assert!(p.held.is_none());
    }

    #[test]
    fn signals_smooth_flows_and_take_counts() {
        let mut s = Signals::default();
        s.blend(&Signals { outside_tows: 10.0, cars: 50.0, ..Default::default() });
        assert!((s.outside_tows - 10.0 * SMOOTHING).abs() < 1e-4);
        assert_eq!(s.cars, 50.0);
    }
}
