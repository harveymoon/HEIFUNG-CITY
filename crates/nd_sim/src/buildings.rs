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
    pub fn for_land_value(v: f32) -> Tier {
        if v < 0.38 {
            Tier::Low
        } else if v < 0.66 {
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
}

impl Construction {
    /// Deliveries a site needs before it can start each quarter.
    pub const DELIVERIES: u8 = 4;

    pub fn new(days: f32) -> Self {
        Self { progress: 0.0, days: days.clamp(1.0, 12.0), deliveries: 0, ordered: false }
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

    /// Days of work left, counting stalls as if the truck came now.
    pub fn days_left(&self) -> f32 {
        ((1.0 - self.progress) * self.days).max(0.0)
    }

    /// Site crew for a building of this size.
    pub fn crew(floors: u8) -> u16 {
        3 + floors as u16 / 2
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
    /// The business operating here, for non-residential buildings.
    pub business: Option<Business>,
    /// Car spaces: integrated into towers, a few under middle-tier blocks,
    /// the whole point of a parking lot.
    #[serde(default)]
    pub parking_capacity: u16,
    #[serde(default)]
    pub parking_used: u16,
    /// Fee per visit for paying customers (parking businesses).
    #[serde(default)]
    pub parking_price: f32,
    /// A power plant placed by the player.
    #[serde(default)]
    pub plant: Option<crate::power::PowerPlantKind>,
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
    #[serde(default)]
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
}

fn one() -> f32 {
    1.0
}

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
            business: None,
            parking_capacity: 0,
            parking_used: 0,
            parking_price: 0.0,
            plant: None,
            powered: 1.0,
            unpowered_hours_today: 0,
            depot: false,
            rail: None,
            rail_inbound: Vec::new(),
            rail_outbound: 0.0,
            port: None,
            construction: None,
        };
        b.recompute_capacity();
        b.update_rent(land_value);
        b
    }

    pub fn is_parking(&self) -> bool {
        self.business.as_ref().map_or(false, |b| b.kind == Business::PARKING_KIND)
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

    pub fn recompute_capacity(&mut self) {
        if self.construction.is_some() {
            // A building site: nobody lives here yet, and the only work is
            // for the crew putting it up.
            self.homes = 0;
            self.jobs = Construction::crew(self.floors);
            self.parking_capacity = 0;
            return;
        }
        if self.is_parking() || self.plant.is_some() || self.depot || self.rail.is_some() {
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
            LandUse::Civic => (0, f * 4),
            LandUse::Park => (0, 1),
        };
        self.homes = homes;
        self.jobs = jobs;
        // Towers bring their own parking; middle blocks squeeze in a few spaces.
        self.parking_capacity = match self.tier {
            Tier::High => (homes + jobs) / 2 + 2,
            Tier::Mid => (homes + jobs) / 4,
            Tier::Low => 0,
        };
    }

    /// Homes and jobs the building will have once construction is over.
    pub fn planned_capacity(&self) -> (u16, u16) {
        if self.construction.is_none() {
            return (self.homes, self.jobs);
        }
        let mut done = self.clone();
        done.construction = None;
        done.recompute_capacity();
        (done.homes, done.jobs)
    }

    pub fn update_rent(&mut self, land_value: f32) {
        self.land_value = land_value;
        self.rent = self.tier.base_rent() * (0.6 + land_value) * (0.7 + 0.3 * self.condition);
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
        self.business.as_ref().map_or(Hours::ALWAYS, |z| z.kind.hours())
    }

    /// Hours a worker with `seed` works here. Building sites work days.
    pub fn shift_for(&self, seed: u64) -> Hours {
        if self.construction.is_some() {
            return Hours::new(7, 17);
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
        self.construction.is_none() && !self.is_derelict() && self.business.as_ref().is_none_or(|z| !z.closed) && self.visitors < self.customer_capacity()
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
