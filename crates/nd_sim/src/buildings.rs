use crate::citizens::CitizenId;
use crate::market::Business;
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
    pub residents: Vec<CitizenId>,
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
    /// The business operating here, for non-residential buildings.
    pub business: Option<Business>,
}

impl Building {
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
            workers: Vec::new(),
            visitors: 0,
            built_tick,
            condition: 1.0,
            rent: 0.0,
            land_value,
            business: None,
        };
        b.recompute_capacity();
        b.update_rent(land_value);
        b
    }

    pub fn recompute_capacity(&mut self) {
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
    }

    pub fn update_rent(&mut self, land_value: f32) {
        self.land_value = land_value;
        self.rent = self.tier.base_rent() * (0.6 + land_value) * (0.7 + 0.3 * self.condition);
    }

    /// Daily wage for a job here.
    pub fn wage(&self) -> f32 {
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

    pub fn free_homes(&self) -> u16 {
        self.homes.saturating_sub(self.residents.len() as u16)
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
