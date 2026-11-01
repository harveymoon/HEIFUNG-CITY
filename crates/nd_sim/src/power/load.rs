//! Demand: each building's load through the day and with the weather, and
//! the regional load and prices.

use super::*;

/// Megawatts a building draws when fully powered.
pub fn demand_mw(b: &Building) -> f32 {
    // Plants feed the grid; batteries charge through the dispatch in
    // `World::allocate_power`, not as a building's load.
    if b.plant.is_some() || b.battery.is_some() {
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
    // [supertall] Lifts, pumps and cooling up a league tower.
    // [megablocks] An arcology's too, and its malls and car decks.
    let tall = if b.plot.tower.is_some() || b.plot.megablock.is_some() { 1.25 } else { 1.0 };
    per_floor * b.floors.max(1) as f32 * tier * tall
}

/// Share of a building's full draw (`demand_mw`) it uses at each hour of
/// the day, by land use: homes have a morning bump and the big evening
/// peak when people get home, cook and switch everything on; shops and
/// offices run through the working day; industry runs shifts round the
/// clock and dips at night; nightlife peaks late; parks are their lamps.
pub(super) const PROFILE: [[f32; 24]; 6] = [
    // Residential
    [0.48, 0.44, 0.42, 0.42, 0.44, 0.52, 0.72, 0.88, 0.80, 0.66, 0.60, 0.60, 0.62, 0.60, 0.60, 0.64, 0.76, 0.92, 1.06, 1.12, 1.10, 0.98, 0.80, 0.60],
    // Commercial
    [0.32, 0.30, 0.30, 0.30, 0.32, 0.36, 0.50, 0.74, 0.94, 1.04, 1.08, 1.10, 1.10, 1.10, 1.08, 1.06, 1.04, 0.98, 0.88, 0.74, 0.60, 0.50, 0.42, 0.36],
    // Industrial
    [0.76, 0.74, 0.73, 0.73, 0.76, 0.82, 0.94, 1.03, 1.06, 1.06, 1.06, 1.05, 1.02, 1.05, 1.06, 1.04, 1.00, 0.95, 0.90, 0.86, 0.83, 0.81, 0.79, 0.77],
    // Entertainment
    [0.90, 0.74, 0.54, 0.38, 0.32, 0.30, 0.32, 0.38, 0.45, 0.54, 0.62, 0.70, 0.76, 0.76, 0.76, 0.78, 0.84, 0.94, 1.05, 1.12, 1.16, 1.14, 1.08, 1.00],
    // Civic
    [0.45, 0.44, 0.44, 0.44, 0.45, 0.50, 0.62, 0.80, 0.96, 1.02, 1.04, 1.04, 1.04, 1.04, 1.02, 1.00, 0.96, 0.88, 0.76, 0.64, 0.56, 0.50, 0.47, 0.46],
    // Park
    [1.00, 1.00, 1.00, 1.00, 1.00, 0.90, 0.55, 0.30, 0.30, 0.30, 0.30, 0.30, 0.30, 0.30, 0.30, 0.30, 0.30, 0.40, 0.70, 0.95, 1.00, 1.00, 1.00, 1.00],
];

pub(super) fn use_index(lu: LandUse) -> usize {
    match lu {
        LandUse::Residential => 0,
        LandUse::Commercial => 1,
        LandUse::Industrial => 2,
        LandUse::Entertainment => 3,
        LandUse::Civic => 4,
        LandUse::Park => 5,
    }
}

/// The hour-of-day share of a building's full draw, interpolated between
/// the hours (`hour` in `0..24`, wrapping).
pub fn hourly_load(lu: LandUse, hour: f32) -> f32 {
    let row = &PROFILE[use_index(lu)];
    let h = hour.rem_euclid(24.0);
    let i = h.floor() as usize % 24;
    let t = h - h.floor();
    row[i] + (row[(i + 1) % 24] - row[i]) * t
}

/// Share of a building's draw that goes on air conditioning at full blast,
/// and on electric heating at full blast.
pub(super) fn climate_shares(lu: LandUse) -> (f32, f32) {
    match lu {
        LandUse::Residential => (0.22, 0.18),
        LandUse::Commercial => (0.28, 0.12),
        LandUse::Industrial => (0.06, 0.05),
        LandUse::Entertainment => (0.22, 0.10),
        LandUse::Civic => (0.22, 0.14),
        LandUse::Park => (0.0, 0.0),
    }
}

/// How hard air conditioning runs at `temp_c`, `0..1.4`: from 24 °C up.
pub fn cooling(temp_c: f32) -> f32 {
    ((temp_c - 24.0) / 14.0).clamp(0.0, 1.4)
}

/// How hard heating runs at `temp_c`, `0..1.4`: from 15 °C down.
pub fn heating(temp_c: f32) -> f32 {
    ((15.0 - temp_c) / 22.0).clamp(0.0, 1.4)
}

/// What the weather puts on a building's draw: `1` in mild air, more for
/// cooling in the heat and heating in the cold.
pub fn weather_load(lu: LandUse, temp_c: f32) -> f32 {
    let (ac, heat) = climate_shares(lu);
    1.0 + ac * cooling(temp_c) + heat * heating(temp_c)
}

/// The full load factor for a land use at an hour and temperature.
pub fn load_factor(lu: LandUse, hour: f32, temp_c: f32) -> f32 {
    hourly_load(lu, hour) * weather_load(lu, temp_c)
}

/// Megawatts a building draws at `hour` in air of `temp_c`: its full draw
/// shaped by the day and the weather. A building site draws its flat
/// floodlights-and-mixer figure; plants and batteries nothing.
pub fn demand_at(b: &Building, hour: f32, temp_c: f32) -> f32 {
    let full = demand_mw(b);
    if full <= 0.0 || b.construction.is_some() {
        return full;
    }
    full * load_factor(b.land_use, hour, temp_c)
}

/// Full draw per land use (`LandUse::ALL` order) and of the building sites:
/// what [`demand_at`] summed over the city multiplies, so a whole day's
/// curve is 24 multiplications, not 24 passes over the buildings.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DemandBase {
    pub by_use: [f32; crate::zoning::LandUse::ALL.len()],
    pub sites: f32,
}

impl DemandBase {
    pub fn of<'a>(buildings: impl Iterator<Item = &'a Building>) -> Self {
        let mut d = DemandBase::default();
        for b in buildings {
            let full = demand_mw(b);
            if full <= 0.0 {
                continue;
            }
            if b.construction.is_some() {
                d.sites += full;
            } else {
                d.by_use[use_index(b.land_use)] += full;
            }
        }
        d
    }

    /// The city's demand at `hour` in air of `temp_c`, MW.
    pub fn at(&self, hour: f32, temp_c: f32) -> f32 {
        LandUse::ALL.iter().map(|lu| self.by_use[use_index(*lu)] * load_factor(*lu, hour, temp_c)).sum::<f32>() + self.sites
    }

    /// Full draw of everything, MW (the old flat figure).
    pub fn full(&self) -> f32 {
        self.by_use.iter().sum::<f32>() + self.sites
    }
}

/// The regional grid's load at `hour` in air of `temp_c`, against a mild
/// day's peak: the region is mostly homes and offices with some industry,
/// so its evening peak and its heat-wave afternoons are the city's too.
pub fn regional_load(hour: f32, temp_c: f32) -> f32 {
    0.45 * load_factor(LandUse::Residential, hour, temp_c) + 0.35 * load_factor(LandUse::Commercial, hour, temp_c) + 0.20 * load_factor(LandUse::Industrial, hour, temp_c)
}

/// The regional load the price is quoted at: the mean price of a mild day
/// comes out near [`GRID_IMPORT_PRICE`].
pub(super) const PRICE_LOAD: f32 = 0.82;

/// What the regional grid charges per MWh at `hour` in air of `temp_c`:
/// with the square of its load, so cheap at night, dear in the evening
/// peak, dearest on a heat-wave evening (or a cold snap's).
pub fn import_price(hour: f32, temp_c: f32) -> f32 {
    let l = regional_load(hour, temp_c) / PRICE_LOAD;
    (GRID_IMPORT_PRICE * l * l).clamp(GRID_IMPORT_PRICE * 0.3, GRID_IMPORT_PRICE * 3.5)
}

/// What the regional grid pays per MWh at `hour`: the wholesale price, a
/// fixed share of the retail one, so selling back never pays for buying.
pub fn export_price(hour: f32, temp_c: f32) -> f32 {
    import_price(hour, temp_c) * (GRID_EXPORT_PRICE / GRID_IMPORT_PRICE)
}
