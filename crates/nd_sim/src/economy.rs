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
    /// Residential buildings per tier.
    pub tier_counts: [u32; 3],
    /// Citizens per income class.
    pub class_counts: [u32; 3],
    /// Average daily rent per residential tier.
    pub avg_rent: [f32; 3],
    /// Income inequality, 0 = equal, 1 = one person has everything.
    pub gini: f32,
    pub avg_land_value: f32,
    /// Citizens forced to move by rent, cumulative.
    pub displaced_total: u32,
    /// Citizens who gave up and left the city, cumulative.
    pub emigrated_total: u32,
    /// Immigrants turned away because nothing was affordable, cumulative.
    pub priced_out_total: u32,
    pub market: crate::market::MarketStats,
    #[serde(default)]
    pub parking_capacity: u32,
    #[serde(default)]
    pub parking_used: u32,
    #[serde(default)]
    pub parking_lots: u32,
    /// Drivers who found no space today and left the car on the street.
    #[serde(default)]
    pub parking_street_today: u32,
    #[serde(default)]
    pub parking_fees_today: f32,
    /// Where the last driver gave up looking for a space.
    #[serde(default)]
    pub parking_shortage_at: Option<glam::Vec2>,
    /// Pedestrian trips started today and sales to passers-by.
    #[serde(default)]
    pub walk_trips_today: u32,
    #[serde(default)]
    pub walk_in_sales_today: f32,
    /// City money: taxes in, plants and imported power out.
    #[serde(default = "default_treasury")]
    pub treasury: f32,
    #[serde(default)]
    pub tax_today: f32,
    #[serde(default)]
    pub power: crate::power::PowerStats,
    #[serde(default)]
    pub transit: crate::transit::TransitStats,
}

fn default_treasury() -> f32 {
    60_000.0
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

    /// Gini coefficient of a list of incomes.
    pub fn gini(incomes: &mut Vec<f32>) -> f32 {
        if incomes.len() < 2 {
            return 0.0;
        }
        incomes.sort_by(|a, b| a.total_cmp(b));
        let n = incomes.len() as f32;
        let total: f32 = incomes.iter().sum();
        if total <= 0.0 {
            return 0.0;
        }
        let weighted: f32 = incomes.iter().enumerate().map(|(i, &x)| (i as f32 + 1.0) * x).sum();
        ((2.0 * weighted) / (n * total) - (n + 1.0) / n).clamp(0.0, 1.0)
    }
}
