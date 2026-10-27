use crate::citizens::CitizenId;
use crate::citizens::HouseholdId;
use crate::market::{Business, Hours};
use crate::zoning::{LandUse, LotId};
use serde::{Deserialize, Serialize};
use slotmap::new_key_type;

new_key_type! {
    pub struct BuildingId;
}

/// Quality tier of a building, and the class of the people who can afford it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Tier {
    Low,
    Mid,
    High,
}

impl Tier {
    pub const ALL: [Tier; 3] = [Tier::Low, Tier::Mid, Tier::High];

    /// The tier's key in commands and saves of proposals: `low`, `mid`,
    /// `high`.
    pub fn key(self) -> &'static str {
        match self {
            Tier::Low => "low",
            Tier::Mid => "mid",
            Tier::High => "high",
        }
    }

    /// A tier from its key or a common synonym (`1`/`small`,
    /// `middle`/`2`/`medium`, `3`/`large`), any case.
    pub fn from_key(s: &str) -> Option<Tier> {
        match s.trim().to_ascii_lowercase().as_str() {
            "low" | "1" | "small" => Some(Tier::Low),
            "mid" | "middle" | "2" | "medium" => Some(Tier::Mid),
            "high" | "3" | "large" => Some(Tier::High),
            _ => None,
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            Tier::Low => "low",
            Tier::Mid => "middle",
            Tier::High => "high",
        }
    }

    pub fn up(self) -> Tier {
        match self {
            Tier::Low => Tier::Mid,
            _ => Tier::High,
        }
    }

    pub fn down(self) -> Tier {
        match self {
            Tier::High => Tier::Mid,
            _ => Tier::Low,
        }
    }

    /// Tier the land value at a lot supports.
    /// Land value from which a building is middle tier, and high tier.
    pub const MID_FROM: f32 = 0.38;
    pub const HIGH_FROM: f32 = 0.58;

    /// [pt-growth] On the compressed land value (`land_value::compress`):
    /// 0.38 and 0.58 are where the old thresholds, 0.38 and 0.66 on the
    /// plain sum, fall now.
    pub fn for_land_value(v: f32) -> Tier {
        if v < Self::MID_FROM {
            Tier::Low
        } else if v < Self::HIGH_FROM {
            Tier::Mid
        } else {
            Tier::High
        }
    }

    /// Daily rent per home before the land value multiplier.
    pub fn base_rent(self) -> f32 {
        match self {
            Tier::Low => 18.0,
            Tier::Mid => 42.0,
            Tier::High => 110.0,
        }
    }

    pub fn wage_multiplier(self) -> f32 {
        match self {
            Tier::Low => 0.85,
            Tier::Mid => 1.0,
            Tier::High => 1.8,
        }
    }

    /// Class of a citizen from their daily income.
    pub fn for_income(income: f32) -> Tier {
        if income < 75.0 {
            Tier::Low
        } else if income < 140.0 {
            Tier::Mid
        } else {
            Tier::High
        }
    }
}

/// A building site. Work advances every sim hour and needs a delivery of
/// parts at the start of each quarter; it waits at the quarter boundary
/// until the truck arrives.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Construction {
    /// Share of the work done, `0..1`.
    pub progress: f32,
    /// Total build time in days at full pace.
    pub days: f32,
    /// Deliveries of materials received so far (four are needed).
    pub deliveries: u8,
    /// A delivery has been ordered and is on its way.
    #[serde(default)]
    pub ordered: bool,
    /// [r2-traffic] No truck can get to the site (`SiteBlock`): the work
    /// waits, nothing is delivered by magic, and it looks again at
    /// `retry_at`.
    #[serde(default)]
    pub blocked: Option<SiteBlock>,
    #[serde(default)]
    pub retry_at: u64,
}

/// [r2-traffic] Why a building site cannot go on (K6): the trucks that
/// bring its materials, or the crew's vans, find no road there.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SiteBlock {
    /// No delivery truck could find a way to it.
    Deliveries,
    /// The plot is on no street the city's roads reach.
    Builders,
}

impl SiteBlock {
    pub fn text(self) -> &'static str {
        match self {
            SiteBlock::Deliveries => "delivery trucks can't get here",
            SiteBlock::Builders => "builders can't reach this plot",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            SiteBlock::Deliveries => "no_deliveries",
            SiteBlock::Builders => "no_builders",
        }
    }
}

impl Construction {
    /// Deliveries a site needs before it can start each quarter.
    pub const DELIVERIES: u8 = 4;

    pub fn new(days: f32) -> Self {
        Self { progress: 0.0, days: days.clamp(1.0, 12.0), deliveries: 0, ordered: false, blocked: None, retry_at: 0 }
    }

    /// Build time for a building of this size and quality.
    pub fn days_for(floors: u8, tier: Tier) -> f32 {
        let tier_factor = match tier {
            Tier::Low => 0.8,
            Tier::Mid => 1.0,
            Tier::High => 1.3,
        };
        ((1.0 + floors as f32 * 0.4) * tier_factor).clamp(1.0, 12.0)
    }

    /// Quarter of the work the site is in, `0..=3`.
    pub fn stage(&self) -> u8 {
        ((self.progress / 0.25).floor() as u8).min(3)
    }

    /// Materials for the current quarter have not arrived yet.
    pub fn waiting_for_materials(&self) -> bool {
        self.progress < 1.0 && self.deliveries <= self.stage()
    }

    /// Progress made in one sim hour at full pace.
    pub fn hourly_step(&self) -> f32 {
        1.0 / (self.days * 24.0)
    }

    /// [pt-growth] Progress made in one hour of the working day: sites work
    /// only those hours, so a day's work is done in them and `days` stays
    /// the calendar time.
    pub fn work_hour_step(&self) -> f32 {
        1.0 / (self.days * crate::clock::WORK_HOURS_PER_DAY)
    }

    /// Days of work left, counting stalls as if the truck came now.
    pub fn days_left(&self) -> f32 {
        ((1.0 - self.progress) * self.days).max(0.0)
    }

    /// [pt-growth] Hours of the working day still to go (stalls aside).
    pub fn work_hours_left(&self) -> f32 {
        (1.0 - self.progress).max(0.0) / self.work_hour_step()
    }

    /// Site crew for a building of this size.
    pub fn crew(floors: u8) -> u16 {
        3 + floors as u16 / 2
    }
}

/// [pt-growth] Why a building is coming down, for the inspector and the
/// API ("being demolished: new development planned").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DemolitionReason {
    /// Older saves and anything not told.
    #[default]
    Unknown,
    /// A developer bought it to build bigger (densification, a richer
    /// rebuild of a cheap building on dear land).
    Redevelopment,
    /// One of several small plots a developer bought together for one
    /// bigger building.
    Assembly,
    /// Shut down and empty: a new owner is taking it over.
    Abandoned,
    /// A road is being built through it.
    RoadWorks,
    /// The city bought it out for one of its own buildings.
    CityTakeover,
    /// The player bulldozed it (or its zone).
    Bulldozed,
    /// A neighbour bought the plot to expand.
    Expansion,
    /// What is left after a fire.
    Fire,
    /// Bought for a private house (a McMansion, a billionaire's estate).
    Estate,
    /// Cleared for a skyscraper, a corporate headquarters or a campus.
    Landmark,
    /// Rebuilt for richer tenants (priced out by a mansion nearby).
    PricedOut,
}

impl DemolitionReason {
    /// "new development planned", for "being demolished: ...".
    pub fn label(self) -> &'static str {
        match self {
            DemolitionReason::Unknown => "being pulled down",
            DemolitionReason::Redevelopment => "new development planned",
            DemolitionReason::Assembly => "plots bought together for a new development",
            DemolitionReason::Abandoned => "abandoned: a new owner is taking over",
            DemolitionReason::RoadWorks => "road works",
            DemolitionReason::CityTakeover => "bought out by the city",
            DemolitionReason::Bulldozed => "bulldozed: the zone was cleared",
            DemolitionReason::Expansion => "a neighbour is expanding onto the plot",
            DemolitionReason::Fire => "burned down: clearing the ruins",
            DemolitionReason::Estate => "bought for a private estate",
            DemolitionReason::Landmark => "cleared for a landmark building",
            DemolitionReason::PricedOut => "rebuilt for richer tenants",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            DemolitionReason::Unknown => "unknown",
            DemolitionReason::Redevelopment => "redevelopment",
            DemolitionReason::Assembly => "assembly",
            DemolitionReason::Abandoned => "abandoned",
            DemolitionReason::RoadWorks => "road_works",
            DemolitionReason::CityTakeover => "city_takeover",
            DemolitionReason::Bulldozed => "bulldozed",
            DemolitionReason::Expansion => "expansion",
            DemolitionReason::Fire => "fire",
            DemolitionReason::Estate => "estate",
            DemolitionReason::Landmark => "landmark",
            DemolitionReason::PricedOut => "priced_out",
        }
    }
}

/// A building coming down. Residents and staff are gone the moment it
/// starts; a crew works the structure from the top down over 1–3 days,
/// dump trucks haul the rubble away, and the lot is left clear (or, when
/// the building is being replaced, its successor's construction starts).
/// The look of what is being pulled down is kept here, because during a
/// redevelopment the building's own fields already describe its successor.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Demolition {
    /// Share of the work done, `0..1`.
    pub progress: f32,
    /// Total time in days.
    pub days: f32,
    /// Truckloads of rubble hauled away so far.
    #[serde(default)]
    pub loads: u8,
    /// What is coming down: floors, tier, land use, seed and condition of
    /// the old building, for the renderer.
    pub floors: u8,
    pub tier: Tier,
    pub land_use: LandUse,
    pub seed: u64,
    #[serde(default = "one")]
    pub condition: f32,
    /// Nothing follows: the building is removed and the lot left vacant
    /// when the work is done. False when a successor waits (redevelopment,
    /// or a civic structure that took the lot over).
    #[serde(default)]
    pub clear_lot: bool,
    /// The lot was made for the site because the parcel itself was erased
    /// (zone bulldozed); it goes with the rubble.
    #[serde(default)]
    pub own_lot: bool,
    /// [districts] The old building's district look, so it comes down
    /// looking as it stood.
    #[serde(default)]
    pub look: Option<crate::districts::Look>,
    /// [pt-growth] Why it is coming down.
    #[serde(default)]
    pub reason: DemolitionReason,
}

impl Demolition {
    /// Time to pull down a building of this height: one day for a house,
    /// three for a tower.
    pub fn days_for(floors: u8) -> f32 {
        (0.9 + floors as f32 * 0.1).clamp(1.0, 3.0)
    }

    /// Dump-truck loads of rubble a building of this height makes.
    pub fn loads_for(floors: u8) -> u8 {
        (2 + floors / 3).min(10)
    }

    /// Crew pulling a building of this height down.
    pub fn crew(floors: u8) -> u16 {
        2 + floors as u16 / 3
    }

    /// Start pulling down `b` as it stands now.
    pub fn of(b: &Building, clear_lot: bool) -> Self {
        Self {
            progress: 0.0,
            days: Self::days_for(b.floors),
            loads: 0,
            floors: b.floors.max(1),
            tier: b.tier,
            land_use: b.land_use,
            seed: b.seed,
            condition: b.condition,
            clear_lot,
            own_lot: false,
            look: b.look,
            reason: DemolitionReason::Unknown,
        }
    }

    /// Progress made in one sim hour.
    pub fn hourly_step(&self) -> f32 {
        1.0 / (self.days.max(0.05) * 24.0)
    }

    /// [pt-growth] ... in one hour of the working day (the crews work only
    /// those, `days` stays the calendar time).
    pub fn work_hour_step(&self) -> f32 {
        1.0 / (self.days.max(0.05) * crate::clock::WORK_HOURS_PER_DAY)
    }

    pub fn days_left(&self) -> f32 {
        ((1.0 - self.progress) * self.days).max(0.0)
    }

    /// [pt-growth] Hours of the working day still to go.
    pub fn work_hours_left(&self) -> f32 {
        (1.0 - self.progress).max(0.0) / self.work_hour_step()
    }

    /// Loads that should have left by now: spread evenly over the work,
    /// the last one before the lot is handed over.
    pub fn loads_due(&self) -> u8 {
        let n = Self::loads_for(self.floors);
        ((self.progress * (n as f32 + 1.0)).floor() as u8).min(n)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Building {
    pub lot: LotId,
    pub land_use: LandUse,
    pub floors: u8,
    pub tier: Tier,
    /// Seed for procedural geometry and naming. Saves stay small because the
    /// geometry is regenerated from this.
    pub seed: u64,
    pub name: String,
    pub homes: u16,
    pub jobs: u16,
    /// Everyone living here, of every household.
    pub residents: Vec<CitizenId>,
    /// The households living here, one per home.
    #[serde(default)]
    pub households: Vec<HouseholdId>,
    pub workers: Vec<CitizenId>,
    /// Citizens currently inside who neither live nor work here.
    pub visitors: u16,
    pub built_tick: u64,
    /// Physical state in `0..1`; cheap buildings wear down, upgrades restore.
    pub condition: f32,
    /// Daily rent per home, derived from tier and land value.
    pub rent: f32,
    /// Land value at the lot when last evaluated.
    pub land_value: f32,
    /// Consecutive days a low-tier building has stood on high-value land;
    /// three in a row and it is redeveloped at the land's tier.
    #[serde(default)]
    pub high_land_days: u8,
    /// [pt-growth] Consecutive days a block of homes has stood mostly
    /// empty (`estate::FORECLOSE_OCCUPANCY`).
    #[serde(default)]
    pub empty_days: u8,
    /// The business operating here, for non-residential buildings.
    pub business: Option<Business>,
    /// Car spaces: integrated into towers, a few under middle-tier blocks,
    /// the whole point of a parking lot.
    #[serde(default)]
    pub parking_capacity: u16,
    #[serde(default)]
    pub parking_used: u16,
    /// [parking-market] The most spaces taken at once today (reset at
    /// midnight, when a parking business sets its fee by it).
    #[serde(default)]
    pub parking_peak: u16,
    /// Fee per visit for paying customers (parking businesses).
    #[serde(default)]
    pub parking_price: f32,
    /// A parking lot the player placed (Build, Transit): the city's own, so
    /// a road never buys it out; it has to be bulldozed first.
    #[serde(default)]
    pub city_parking: bool,
    /// A power plant placed by the player.
    #[serde(default)]
    pub plant: Option<crate::power::PowerPlantKind>,
    /// [plant-scale] A coal, nuclear or geothermal plant's size, as a share
    /// of its kind's nameplate, fixed from its site when it was built.
    /// `None` on plants from older saves (built at the fixed size: 1) and
    /// on solar and wind, which follow their lot.
    #[serde(default)]
    pub plant_scale: Option<f32>,
    /// A grid battery pack placed by the player, with its charge.
    #[serde(default)]
    pub battery: Option<crate::power::Battery>,
    /// Share of its electricity demand met this hour.
    #[serde(default = "one")]
    pub powered: f32,
    #[serde(default)]
    pub unpowered_hours_today: u16,
    /// A bus depot placed by the player.
    #[serde(default)]
    pub depot: bool,
    /// A station or freight yard on the railway.
    #[serde(default)]
    pub rail: Option<crate::rail::RailLink>,
    /// Goods ordered by rail, waiting for the next freight train (yards).
    /// [freight] Legacy: only read from old saves, whose orders move to
    /// `Rail::orders` on load (`World::validate`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rail_inbound: Vec<crate::traffic::Cargo>,
    /// Export units waiting on the apron (yards).
    #[serde(default)]
    pub rail_outbound: f32,
    /// Still being built: no homes, no permanent jobs, a site crew instead.
    #[serde(default)]
    pub construction: Option<Construction>,
    /// A shipping port or ferry terminal.
    #[serde(default)]
    pub port: Option<crate::ports::PortKind>,
    /// A police, fire, health, school or dispatch building, or an
    /// attraction, placed by the player (`nd_sim::services`).
    #[serde(default)]
    pub service: Option<crate::services::ServiceSite>,
    /// Being pulled down (see [`Demolition`]). A building that is only
    /// coming down also carries a `construction` placeholder, so every
    /// system that skips building sites skips it too.
    #[serde(default)]
    pub demolition: Option<Demolition>,
    /// [districts] The district architecture it was built or refitted in
    /// (`crate::districts::Look`); `None` draws the land use's own menu.
    #[serde(default)]
    pub look: Option<crate::districts::Look>,
    /// [subway] A metro station: an entrance kiosk on its plaza here, the
    /// platform underground (`nd_sim::subway`). A Central Station has it
    /// too, beside its railway platforms and bus terminal.
    #[serde(default)]
    pub metro: bool,
    /// [megacorp] A campus building beyond a university's main one (a dorm,
    /// offices, a theatre, a sports centre, a frat house, a plaza). Offices,
    /// theatres and sports centres are the campus's own (`is_special`).
    #[serde(default)]
    pub annex: Option<crate::corp::Annex>,
    /// [land] Extra plots it spans, the form it was built in and who owns
    /// it (`crate::estate`). Default: one plot, independently owned.
    #[serde(default)]
    pub plot: crate::estate::Plot,
    /// [tenants] The units it is let in: shopfronts on the ground floor and
    /// ranges of upper floors, each empty, the building's own business's
    /// (`business`) or another business's (`World::plan_units`). Empty for
    /// buildings that are one business or none (houses, sheds, the city's
    /// structures), and in saves from before tenants (planned on load).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub units: Vec<Unit>,
    /// [tenants] Set on a tenant: a business renting units in this other
    /// building. It is a record of its own (its own staff, books, account,
    /// hours, sign and rent) but no structure: it stands on its host's lot,
    /// is drawn as part of the host and goes when the host goes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit_of: Option<BuildingId>,
}

/// [tenants] Who has a unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Occupant {
    /// To let.
    Vacant,
    /// The building's own business (`Building::business`).
    Main,
    /// Another business renting it (a record with `unit_of` this building).
    Tenant(BuildingId),
}

/// [tenants] A unit of a building that can be let on its own.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Unit {
    /// Floors it spans, ground floor 0, inclusive.
    pub lo: u8,
    pub hi: u8,
    /// Its share of each floor's footprint, `0..=1`.
    pub share: f32,
    /// Floor area, m².
    pub area: f32,
    /// A shop on the street, with its own door and sign.
    pub shopfront: bool,
    /// Jobs it holds (`Building::recompute_capacity`).
    pub jobs: u16,
    pub occupant: Occupant,
    /// Tick it last fell empty (or was planned empty).
    #[serde(default)]
    pub vacant_since: u64,
}

impl Unit {
    pub fn floors(&self) -> u8 {
        self.hi.saturating_sub(self.lo) + 1
    }

    pub fn vacant(&self) -> bool {
        self.occupant == Occupant::Vacant
    }

    /// "ground floor", "floor 3", "floors 4–9".
    pub fn place(&self) -> String {
        match (self.lo, self.hi) {
            (0, 0) => "ground floor".into(),
            (0, h) => format!("ground floor–{h}"),
            (l, h) if l == h => format!("floor {l}"),
            (l, h) => format!("floors {l}–{h}"),
        }
    }
}

fn one() -> f32 {
    1.0
}

/// [tenants] A shopfront's staff against an office floor of the same area:
/// a shop floor is busier (tills, stockroom, the door).
pub const SHOPFRONT_STAFFING: f32 = 1.5;

impl Building {
    /// Daily pay for a labourer on a building site.
    pub const SITE_WAGE: f32 = 70.0;

    pub fn new(lot: LotId, land_use: LandUse, floors: u8, tier: Tier, seed: u64, name: String, built_tick: u64, land_value: f32) -> Self {
        let mut b = Self {
            lot,
            land_use,
            floors,
            tier,
            seed,
            name,
            homes: 0,
            jobs: 0,
            residents: Vec::new(),
            households: Vec::new(),
            workers: Vec::new(),
            visitors: 0,
            built_tick,
            condition: 1.0,
            rent: 0.0,
            land_value,
            high_land_days: 0,
            empty_days: 0,
            business: None,
            parking_capacity: 0,
            parking_used: 0,
            parking_peak: 0,
            parking_price: 0.0,
            city_parking: false,
            plant: None,
            plant_scale: None,
            battery: None,
            powered: 1.0,
            unpowered_hours_today: 0,
            depot: false,
            rail: None,
            rail_inbound: Vec::new(),
            rail_outbound: 0.0,
            port: None,
            construction: None,
            service: None,
            demolition: None,
            look: None,
            metro: false,
            annex: None,
            plot: Default::default(),
            units: Vec::new(),
            unit_of: None,
        };
        b.recompute_capacity();
        b.update_rent(land_value);
        b
    }

    pub fn is_parking(&self) -> bool {
        self.business.as_ref().map_or(false, |b| b.kind == Business::PARKING_KIND)
    }

    /// A power plant or a grid battery: part of the grid, not a place
    /// anyone lives, shops or visits.
    pub fn is_power(&self) -> bool {
        self.plant.is_some() || self.battery.is_some()
    }

    /// [review] One of the city's own structures, put down by the player
    /// with the Build window: a plant or battery, bus depot, rail yard or
    /// station, dock, service or attraction, metro station, city car park
    /// or cargo hub. Its ground is the city's; the market never buys it,
    /// and only the player removes it. The one source of truth behind
    /// `World::is_civic_structure` and `World::is_city_building`, which
    /// disagreed with each other and with [`Building::is_special`] (city
    /// car parks and cargo hubs were missing here, annexes there).
    pub fn is_city_structure(&self) -> bool {
        self.is_power() || self.depot || self.rail.is_some() || self.port.is_some() || self.service.is_some() || self.metro || self.city_parking || self.business.as_ref().is_some_and(|z| z.kind == crate::market::BusinessKind::CargoHub)
    }

    /// Not an ordinary grown building: a city structure
    /// ([`Building::is_city_structure`]) or a campus's special annex. Never
    /// upgraded, redeveloped or counted towards density. (A dock was
    /// missing here once: the daily market update retiered shipping ports
    /// and recomputed their jobs as an ordinary industrial building; city
    /// car parks and cargo hubs were, until the review.)
    pub fn is_special(&self) -> bool {
        self.is_city_structure() || self.annex.is_some_and(|a| a.special()) || self.is_tenant()
    }

    /// [tenants] A business renting units in another building
    /// (`unit_of`), not a structure of its own: special (never upgraded,
    /// redeveloped, bought or counted as a building), drawn as its host.
    pub fn is_tenant(&self) -> bool {
        self.unit_of.is_some()
    }

    /// [tenants] Units to let.
    pub fn vacant_units(&self) -> usize {
        self.units.iter().filter(|u| u.vacant()).count()
    }

    pub fn parking_free(&self) -> u16 {
        self.parking_capacity.saturating_sub(self.parking_used)
    }

    /// Share of spaces in use, for parking lots whose look should show it.
    pub fn parking_visual(&self) -> Option<f32> {
        if self.is_parking() && self.parking_capacity > 0 {
            Some(self.parking_used as f32 / self.parking_capacity as f32)
        } else {
            None
        }
    }

    pub fn under_construction(&self) -> bool {
        self.construction.is_some()
    }

    /// [under-construction] Open as its finished self: not a building site
    /// and not coming down. A site has its crew and nothing else: no
    /// visitors or customers, no homes or permanent jobs, no service cover,
    /// no power, no parking, no transit, no pull on tourists or land value.
    /// Every system that picks a building as a destination or lets it act
    /// on the city asks this. (Derelict buildings still operate, badly;
    /// callers that shut them out too check `is_derelict` beside it.)
    pub fn operating(&self) -> bool {
        self.construction.is_none() && self.demolition.is_none()
    }

    /// Coming down, with nothing to follow on the lot.
    pub fn being_demolished(&self) -> bool {
        self.demolition.as_ref().is_some_and(|d| d.clear_lot)
    }

    pub fn recompute_capacity(&mut self) {
        if let Some(d) = self.demolition.as_ref().filter(|_| self.construction.is_some()) {
            // A demolition site: the only work is the wrecking crew's.
            self.homes = 0;
            self.jobs = Demolition::crew(d.floors);
            self.parking_capacity = 0;
            return;
        }
        if self.construction.is_some() {
            // A building site: nobody lives here yet, and the only work is
            // for the crew putting it up.
            self.homes = 0;
            self.jobs = Construction::crew(self.floors);
            self.parking_capacity = 0;
            return;
        }
        // [megacorp] A campus's offices, theatre or sports centre.
        if let Some(a) = self.annex.filter(|a| a.special()) {
            self.homes = 0;
            self.jobs = a.jobs();
            self.parking_capacity = 6;
            return;
        }
        if self.is_parking() || self.is_special() {
            return;
        }
        let f = self.floors as u16;
        let (homes, jobs) = match self.land_use {
            LandUse::Residential => {
                let per_floor = match self.tier {
                    Tier::Low => 3,
                    Tier::Mid => 2,
                    Tier::High => 1,
                };
                (f * per_floor, 0)
            }
            LandUse::Commercial => (0, f * if self.tier == Tier::High { 4 } else { 3 }),
            LandUse::Industrial => (0, f * 8),
            LandUse::Entertainment => (0, f * 2 + 2),
            // [pt-growth] Civic halls are two to five storeys at any tier
            // (what their styles draw); a grand one holds more staff a floor.
            LandUse::Civic => (0, f * match self.tier {
                Tier::Low => 4,
                Tier::Mid => 5,
                Tier::High => 8,
            }),
            LandUse::Park => (0, 1),
        };
        // [land] A one-storey row of cheap homes (shacks, a trailer park,
        // huts) houses as many as the two-storey walk-up it replaces; a
        // building over merged plots holds as much per plot; a McMansion or
        // a mansion is one household's.
        let (homes, jobs) = match self.plot.form {
            Some(form) if form.owner_occupied() => (1, 0),
            // [supertall] A league tower: homes or offices by its floors,
            // with diminishing returns up the shaft and a hard cap, plus a
            // little for the podium on each extra plot; one tower should
            // not swamp a city's balance.
            Some(crate::estate::Form::Tower) if self.plot.tower.is_some() => {
                let eff = crate::towers::effective_floors(self.floors);
                let podium = self.plot.extra_lots.len() as f32;
                match self.land_use {
                    LandUse::Residential => ((eff * 2.0 + podium * 6.0).min(900.0) as u16, 0),
                    LandUse::Commercial => (0, (eff * 6.0 + podium * 12.0).min(1500.0) as u16),
                    _ => (homes, jobs),
                }
            }
            _ => {
                let homes = if self.land_use == LandUse::Residential && self.tier == Tier::Low && self.floors == 1 { 6 } else { homes };
                let k = 1 + self.plot.extra_lots.len() as u16;
                (homes * k, jobs * k)
            }
        };
        self.homes = homes;
        self.jobs = jobs;
        // [tenants] Jobs split over the units by floor area; the building's
        // own business has those of its own units only.
        if !self.units.is_empty() {
            self.jobs = self.split_unit_jobs(jobs);
        }
        // Towers bring their own parking; middle blocks squeeze in a few spaces.
        self.parking_capacity = match self.tier {
            Tier::High => (homes + jobs) / 2 + 2,
            Tier::Mid => (homes + jobs) / 4,
            Tier::Low => 0,
        };
    }

    /// [tenants] Each unit's jobs from the building's (`jobs`, all floors):
    /// an office or shop building's spread by floor area (its jobs as
    /// before, however it is let), a shopfront under homes at a shop's
    /// staffing for its share of the ground floor
    /// ([`SHOPFRONT_STAFFING`] times a shop floor). Returns the jobs of the
    /// units the building's own business has.
    fn split_unit_jobs(&mut self, jobs: u16) -> u16 {
        let floors = self.floors.max(1) as f32;
        let k = 1 + self.plot.extra_lots.len() as u16;
        let shop_floor = (if self.tier == Tier::High { 4 } else { 3 }) * k;
        let homes_above = self.land_use == LandUse::Residential;
        let mut own = 0u16;
        for u in self.units.iter_mut() {
            let n = if homes_above {
                shop_floor as f32 * SHOPFRONT_STAFFING * u.share * u.floors() as f32
            } else {
                jobs as f32 / floors * u.share * u.floors() as f32
            };
            u.jobs = (n.round() as u16).max(if u.shopfront { 2 } else { 1 });
            if u.occupant == Occupant::Main {
                own += u.jobs;
            }
        }
        own
    }

    /// Homes and jobs the building will have once construction is over.
    pub fn planned_capacity(&self) -> (u16, u16) {
        if self.being_demolished() {
            return (0, 0);
        }
        if self.construction.is_none() {
            return (self.homes, self.jobs);
        }
        let mut done = self.clone();
        done.construction = None;
        done.demolition = None;
        done.recompute_capacity();
        (done.homes, done.jobs)
    }

    pub fn update_rent(&mut self, land_value: f32) {
        self.land_value = land_value;
        self.rent = self.tier.base_rent() * (0.6 + land_value) * (0.7 + 0.3 * self.condition);
        // [land] Bought outright: the household that owns it pays no rent.
        if self.plot.form.is_some_and(|f| f.owner_occupied()) {
            self.rent = 0.0;
        }
    }

    /// Daily wage for a job here.
    pub fn wage(&self) -> f32 {
        if self.construction.is_some() {
            return Self::SITE_WAGE;
        }
        let base = match self.land_use {
            LandUse::Industrial => 65.0,
            LandUse::Commercial => 90.0,
            LandUse::Entertainment => 80.0,
            LandUse::Civic => 125.0,
            LandUse::Park => 50.0,
            LandUse::Residential => 0.0,
        };
        base * self.tier.wage_multiplier()
    }

    /// Homes not taken by a household.
    pub fn free_homes(&self) -> u16 {
        self.homes.saturating_sub(self.households.len() as u16)
    }

    /// When the doors are open: the business's hours, round the clock for
    /// everything else (parks, plants, homes).
    pub fn opening_hours(&self) -> Hours {
        if let Some(s) = self.service {
            return s.kind.hours();
        }
        // [megacorp] A campus's offices, theatre or sports centre.
        if let Some(a) = self.annex.filter(|a| a.special()) {
            return a.hours();
        }
        // [stories] A late licence keeps it open longer.
        self.business.as_ref().map_or(Hours::ALWAYS, |z| z.kind.hours().later(z.late_hours))
    }

    /// Hours a worker with `seed` works here. Building sites work days.
    pub fn shift_for(&self, seed: u64) -> Hours {
        if self.construction.is_some() {
            return Hours::new(7, 17);
        }
        // Stations, hospitals and dispatch run 24 h in three shifts.
        if let Some(s) = self.service {
            return s.kind.hours().shift(seed);
        }
        match &self.business {
            Some(z) => z.kind.hours().shift(seed),
            None => Hours::new(8, 17),
        }
    }

    /// Customers it can serve at once; unlimited for places without a door
    /// count (parks, offices).
    pub fn customer_capacity(&self) -> u16 {
        match self.business.as_ref().and_then(|z| z.kind.customers_per_job()) {
            Some(per) => (self.jobs * per).max(8),
            None => u16::MAX,
        }
    }

    /// Built, not derelict or shut down, and with room inside.
    pub fn takes_customers(&self) -> bool {
        self.operating() && !self.is_derelict() && self.business.as_ref().is_none_or(|z| !z.closed) && self.visitors < self.customer_capacity()
    }

    /// Open to a customer at `hour`: `takes_customers` and inside its
    /// opening hours.
    pub fn serves_at(&self, hour: u8) -> bool {
        self.takes_customers() && self.opening_hours().contains(hour)
    }

    pub fn free_jobs(&self) -> u16 {
        self.jobs.saturating_sub(self.workers.len() as u16)
    }

    pub fn occupancy(&self) -> usize {
        self.residents.len() + self.workers.len() + self.visitors as usize
    }

    pub fn is_derelict(&self) -> bool {
        self.condition < 0.3
    }

    /// Fraction of jobs filled.
    pub fn staffing(&self) -> f32 {
        if self.jobs == 0 {
            0.0
        } else {
            (self.workers.len() as f32 / self.jobs as f32).clamp(0.0, 1.0)
        }
    }
}
