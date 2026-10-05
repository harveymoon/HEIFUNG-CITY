//! Electricity: plants the player places, a weather state that solar and
//! wind depend on, and an hourly allocation of supply to demand. The grid
//! itself is abstract (one city-wide pool); who goes dark first is not: the
//! rich are served first, so brownouts land on the cheap districts.

use crate::buildings::{Building, Tier};
use crate::zoning::LandUse;
use rand::Rng;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PowerPlantKind {
    Coal,
    Solar,
    Wind,
    Nuclear,
}

impl PowerPlantKind {
    pub const ALL: [PowerPlantKind; 4] = [PowerPlantKind::Coal, PowerPlantKind::Solar, PowerPlantKind::Wind, PowerPlantKind::Nuclear];

    pub fn label(self) -> &'static str {
        match self {
            PowerPlantKind::Coal => "coal plant",
            PowerPlantKind::Solar => "solar farm",
            PowerPlantKind::Wind => "wind farm",
            PowerPlantKind::Nuclear => "nuclear plant",
        }
    }

    /// Nameplate output in megawatts.
    pub fn capacity_mw(self) -> f32 {
        match self {
            PowerPlantKind::Coal => 120.0,
            PowerPlantKind::Solar => 25.0,
            PowerPlantKind::Wind => 40.0,
            PowerPlantKind::Nuclear => 400.0,
        }
    }

    pub fn build_cost(self) -> f32 {
        match self {
            PowerPlantKind::Coal => 20_000.0,
            PowerPlantKind::Solar => 8_000.0,
            PowerPlantKind::Wind => 10_000.0,
            PowerPlantKind::Nuclear => 150_000.0,
        }
    }

    pub fn upkeep_per_day(self) -> f32 {
        match self {
            PowerPlantKind::Coal => 900.0,
            PowerPlantKind::Solar => 60.0,
            PowerPlantKind::Wind => 90.0,
            PowerPlantKind::Nuclear => 2_500.0,
        }
    }

    pub fn jobs(self) -> u16 {
        match self {
            PowerPlantKind::Coal => 30,
            PowerPlantKind::Solar => 3,
            PowerPlantKind::Wind => 4,
            PowerPlantKind::Nuclear => 60,
        }
    }

    /// Effect on land value around the plant: (radius, weight).
    pub fn pollution(self) -> (f32, f32) {
        match self {
            PowerPlantKind::Coal => (300.0, -0.35),
            PowerPlantKind::Solar => (0.0, 0.0),
            PowerPlantKind::Wind => (120.0, -0.04),
            PowerPlantKind::Nuclear => (200.0, -0.10),
        }
    }

    pub fn makes_haze(self) -> bool {
        self == PowerPlantKind::Coal
    }

    /// Smallest lot it fits on.
    pub fn min_area(self) -> f32 {
        match self {
            PowerPlantKind::Coal => 600.0,
            PowerPlantKind::Solar => 300.0,
            PowerPlantKind::Wind => 400.0,
            PowerPlantKind::Nuclear => 900.0,
        }
    }

    pub fn allowed_on(self, lu: LandUse) -> bool {
        match self {
            PowerPlantKind::Coal | PowerPlantKind::Nuclear => lu == LandUse::Industrial,
            PowerPlantKind::Solar | PowerPlantKind::Wind => lu != LandUse::Park && lu != LandUse::Civic,
        }
    }

    /// Share of nameplate output right now.
    pub fn availability(self, weather: &Weather, day_fraction: f32) -> f32 {
        match self {
            PowerPlantKind::Coal | PowerPlantKind::Nuclear => 1.0,
            PowerPlantKind::Solar => {
                let elev = ((day_fraction - 0.25) * std::f32::consts::TAU).sin().max(0.0);
                elev * (1.0 - 0.75 * weather.cloud)
            }
            PowerPlantKind::Wind => 0.12 + 0.88 * weather.wind,
        }
    }
}

/// Sky and wind, drifting slowly. Cloud dims solar; wind drives turbines.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Weather {
    pub cloud: f32,
    pub wind: f32,
}

impl Default for Weather {
    fn default() -> Self {
        Self { cloud: 0.3, wind: 0.5 }
    }
}

impl Weather {
    /// One hour of drift.
    pub fn step(&mut self, rng: &mut impl Rng) {
        self.cloud = (self.cloud + rng.random_range(-0.08..0.08)).clamp(0.0, 1.0);
        self.wind = (self.wind + rng.random_range(-0.07..0.07)).clamp(0.05, 1.0);
    }

    pub fn label(&self) -> &'static str {
        match (self.cloud, self.wind) {
            (c, _) if c > 0.75 => "overcast",
            (c, w) if c > 0.45 && w > 0.6 => "blustery",
            (c, _) if c > 0.45 => "cloudy",
            (_, w) if w > 0.7 => "windy",
            _ => "clear",
        }
    }
}

/// Grid figures for the panel and the headless runner.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PowerStats {
    pub supply_mw: f32,
    pub demand_mw: f32,
    /// Share of demand met this hour.
    pub served: f32,
    pub plants: u32,
    pub unpowered_buildings: u32,
    pub brownout_hours_today: u32,
    /// Megawatts bought from the regional grid this hour.
    pub imported_mw: f32,
    pub import_cost_today: f32,
    pub upkeep_today: f32,
}

/// Power the regional grid will sell the city, in megawatts, and its price
/// per megawatt-hour. Enough for a village, not a city.
pub const GRID_IMPORT_MW: f32 = 60.0;
pub const GRID_IMPORT_PRICE: f32 = 2.5;

/// Megawatts a building draws when fully powered.
pub fn demand_mw(b: &Building) -> f32 {
    if b.plant.is_some() {
        return 0.0;
    }
    // A site runs a few floodlights and a mixer, not a building's worth.
    if b.construction.is_some() {
        return 0.05;
    }
    let per_floor = match b.land_use {
        LandUse::Residential => 0.08,
        LandUse::Commercial => 0.12,
        LandUse::Industrial => 0.5,
        LandUse::Entertainment => 0.2,
        LandUse::Civic => 0.15,
        LandUse::Park => 0.02,
    };
    let tier = match b.tier {
        Tier::Low => 0.8,
        Tier::Mid => 1.0,
        Tier::High => 1.4,
    };
    per_floor * b.floors.max(1) as f32 * tier
}
