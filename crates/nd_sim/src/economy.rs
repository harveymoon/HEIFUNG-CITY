use crate::zoning::LandUse;
use serde::{Deserialize, Serialize};

/// Deliberately simple demand model. Demand per land use is in `0..1` and
/// drives how quickly vacant lots develop.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Economy {
    pub demand: [f32; 6],
    pub population: u32,
    pub unhoused: u32,
    pub unemployed: u32,
    pub homes_total: u32,
    pub homes_free: u32,
    pub jobs_total: u32,
    pub jobs_free: u32,
    /// People who want to move in but found no home yet.
    pub waiting_immigrants: u32,
}

impl Economy {
    pub fn demand_for(&self, lu: LandUse) -> f32 {
        self.demand[lu.index()]
    }

    pub fn recompute(&mut self) {
        let pop = self.population.max(1) as f32;
        let housing_pressure = (self.waiting_immigrants as f32 + self.unhoused as f32) / 6.0;
        let job_pressure = self.unemployed as f32 / pop;
        let entertainment_pressure = (pop / 40.0).min(1.0);

        let mut d = [0.0f32; 6];
        // Residential demand: always a trickle so the city starts, then driven by
        // people waiting for homes.
        d[LandUse::Residential.index()] = (0.35 + housing_pressure).min(1.0);
        // Jobs: unemployment pulls commercial and industrial development.
        d[LandUse::Commercial.index()] = (0.2 + job_pressure * 1.2).min(1.0);
        d[LandUse::Industrial.index()] = (0.15 + job_pressure).min(1.0);
        d[LandUse::Entertainment.index()] = (0.1 + entertainment_pressure * 0.8).min(1.0);
        d[LandUse::Civic.index()] = (0.1 + (pop / 150.0)).min(1.0);
        d[LandUse::Park.index()] = 0.5;
        self.demand = d;
    }
}
