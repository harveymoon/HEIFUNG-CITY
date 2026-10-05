use crate::citizens::CitizenId;
use crate::zoning::{LandUse, LotId};
use serde::{Deserialize, Serialize};
use slotmap::new_key_type;

new_key_type! {
    pub struct BuildingId;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Building {
    pub lot: LotId,
    pub land_use: LandUse,
    pub floors: u8,
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
}

impl Building {
    pub fn new(lot: LotId, land_use: LandUse, floors: u8, seed: u64, name: String, built_tick: u64) -> Self {
        let f = floors as u16;
        let (homes, jobs) = match land_use {
            LandUse::Residential => (f * 2, 0),
            LandUse::Commercial => (0, f * 3),
            LandUse::Industrial => (0, f * 8),
            LandUse::Entertainment => (0, f * 2 + 2),
            LandUse::Civic => (0, f * 4),
            LandUse::Park => (0, 1),
        };
        Self {
            lot,
            land_use,
            floors,
            seed,
            name,
            homes,
            jobs,
            residents: Vec::new(),
            workers: Vec::new(),
            visitors: 0,
            built_tick,
        }
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
}
