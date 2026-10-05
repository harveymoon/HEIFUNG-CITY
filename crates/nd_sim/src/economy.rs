use crate::zoning::LandUse;
use serde::{Deserialize, Serialize};

/// How far into the red a citizen's savings may go. Below this, shops,
/// venues and car parks refuse them: without a floor, consumer debt grows
/// without limit and every purchase is money printed on the spot.
pub const OVERDRAFT: f32 = -200.0;
/// Daily gig income of someone without a job.
pub const GIG_INCOME: f32 = 40.0;

/// A named source or sink of treasury money.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Flow {
    /// 12% of every citizen's daily income.
    IncomeTax,
    /// 5% of every open business's daily revenue.
    BusinessTax,
    BusFares,
    RailFares,
    PlantUpkeep,
    /// Electricity bought from the regional grid.
    GridImport,
    /// Buses and depots.
    TransitUpkeep,
    /// Stations and yards.
    RailUpkeep,
    /// Plants, depots, stations and yards placed by the player.
    Construction,
}

impl Flow {
    pub const ALL: [Flow; 9] = [Flow::IncomeTax, Flow::BusinessTax, Flow::BusFares, Flow::RailFares, Flow::PlantUpkeep, Flow::GridImport, Flow::TransitUpkeep, Flow::RailUpkeep, Flow::Construction];

    pub fn label(self) -> &'static str {
        match self {
            Flow::IncomeTax => "income tax",
            Flow::BusinessTax => "business tax",
            Flow::BusFares => "bus fares",
            Flow::RailFares => "rail fares",
            Flow::PlantUpkeep => "plant upkeep",
            Flow::GridImport => "grid import",
            Flow::TransitUpkeep => "transit upkeep",
            Flow::RailUpkeep => "rail upkeep",
            Flow::Construction => "construction",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// Where the treasury's money came from and went, by source and sink, for
/// today so far and for the whole of yesterday. Positive is income.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Ledger {
    pub today: [f32; 9],
    pub yesterday: [f32; 9],
}

impl Ledger {
    pub fn today_of(&self, f: Flow) -> f32 {
        self.today[f.index()]
    }

    pub fn yesterday_of(&self, f: Flow) -> f32 {
        self.yesterday[f.index()]
    }

    /// Close the day's books.
    pub fn roll(&mut self) {
        self.yesterday = self.today;
        self.today = [0.0; 9];
    }

    /// Yesterday's entries, largest first, for a report.
    pub fn yesterday_sorted(&self) -> Vec<(Flow, f32)> {
        let mut v: Vec<(Flow, f32)> = Flow::ALL.iter().map(|&f| (f, self.yesterday[f.index()])).filter(|(_, x)| *x != 0.0).collect();
        v.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
        v
    }
}

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
    /// Building sites, and the homes and jobs they will add when they open.
    #[serde(default)]
    pub sites: u32,
    #[serde(default)]
    pub homes_building: u32,
    #[serde(default)]
    pub jobs_building: u32,
    /// Building materials bought from outside today, at import prices.
    #[serde(default)]
    pub materials_today: f32,
    /// Treasury sources and sinks by name.
    #[serde(default)]
    pub ledger: Ledger,
}

fn default_treasury() -> f32 {
    60_000.0
}

impl Economy {
    /// Money into the treasury, booked under `flow`.
    pub fn credit(&mut self, flow: Flow, amount: f32) {
        self.treasury += amount;
        self.ledger.today[flow.index()] += amount;
    }

    /// Money out of the treasury, booked under `flow`.
    pub fn debit(&mut self, flow: Flow, amount: f32) {
        self.credit(flow, -amount);
    }

    pub fn demand_for(&self, lu: LandUse) -> f32 {
        self.demand[lu.index()]
    }

    pub fn recompute(&mut self) {
        let pop = self.population.max(1) as f32;
        // Homes and jobs already going up count against the pressure, so a
        // boom does not start a site on every vacant lot while the first ones
        // are still scaffolding.
        let housing_pressure = ((self.waiting_immigrants as f32 + self.unhoused as f32) - self.homes_building as f32 * 0.5).max(0.0) / 6.0;
        let job_pressure = (self.unemployed as f32 - self.jobs_building as f32 * 0.5).max(0.0) / pop;
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
