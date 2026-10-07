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
    /// Run-of-river turbines on a river or stream with a current.
    Hydro,
    /// Turbines in the tide race, on the sea shore.
    Tidal,
    /// Wells into the hot ground at a geothermal hot spot.
    Geothermal,
}

/// Where a kind of plant may stand, beyond its land use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlantSite {
    Anywhere,
    /// Beside a river or stream whose current reaches `MIN_HYDRO_CURRENT`.
    River,
    /// On the sea shore.
    Coast,
    /// Over a geothermal hot spot.
    HotSpot,
}

/// [clarity] The lot area solar and wind nameplates are quoted for.
pub const SOLAR_REFERENCE_AREA: f32 = 600.0;
pub const WIND_REFERENCE_AREA: f32 = 800.0;

impl PowerPlantKind {
    pub const ALL: [PowerPlantKind; 7] = [PowerPlantKind::Coal, PowerPlantKind::Solar, PowerPlantKind::Wind, PowerPlantKind::Nuclear, PowerPlantKind::Hydro, PowerPlantKind::Tidal, PowerPlantKind::Geothermal];

    /// The ground a plant needs: hydro a river with a current, tidal the
    /// sea, geothermal a hot spot; the rest go anywhere their land use allows.
    pub fn site(self) -> PlantSite {
        match self {
            PowerPlantKind::Hydro => PlantSite::River,
            PowerPlantKind::Tidal => PlantSite::Coast,
            PowerPlantKind::Geothermal => PlantSite::HotSpot,
            _ => PlantSite::Anywhere,
        }
    }

    /// A short note on where the plant may go, for tool tips.
    pub fn site_hint(self) -> &'static str {
        match self.site() {
            PlantSite::River => "on a lot beside a river or stream with a current",
            PlantSite::Coast => "on a lot on the sea shore",
            PlantSite::HotSpot => "over a geothermal hot spot (steaming vents)",
            PlantSite::Anywhere => match self {
                PowerPlantKind::Coal | PowerPlantKind::Nuclear => "on industrial land",
                _ => "on any land but parks and civic",
            },
        }
    }

    /// Position in `ALL`: the slot in per-kind arrays.
    pub fn index(self) -> usize {
        self as usize
    }

    /// The kind's name without "plant" or "farm", for chart legends.
    pub fn short(self) -> &'static str {
        match self {
            PowerPlantKind::Coal => "coal",
            PowerPlantKind::Solar => "solar",
            PowerPlantKind::Wind => "wind",
            PowerPlantKind::Nuclear => "nuclear",
            PowerPlantKind::Hydro => "hydro",
            PowerPlantKind::Tidal => "tidal",
            PowerPlantKind::Geothermal => "geothermal",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            PowerPlantKind::Coal => "coal plant",
            PowerPlantKind::Solar => "solar farm",
            PowerPlantKind::Wind => "wind farm",
            PowerPlantKind::Nuclear => "nuclear plant",
            PowerPlantKind::Hydro => "hydro plant",
            PowerPlantKind::Tidal => "tidal plant",
            PowerPlantKind::Geothermal => "geothermal plant",
        }
    }

    /// Nameplate output in megawatts.
    pub fn capacity_mw(self) -> f32 {
        match self {
            PowerPlantKind::Coal => 120.0,
            PowerPlantKind::Solar => 25.0,
            PowerPlantKind::Wind => 40.0,
            PowerPlantKind::Nuclear => 400.0,
            PowerPlantKind::Hydro => 70.0,
            PowerPlantKind::Tidal => 60.0,
            PowerPlantKind::Geothermal => 90.0,
        }
    }

    pub fn build_cost(self) -> f32 {
        match self {
            PowerPlantKind::Coal => 20_000.0,
            PowerPlantKind::Solar => 8_000.0,
            PowerPlantKind::Wind => 10_000.0,
            PowerPlantKind::Nuclear => 150_000.0,
            PowerPlantKind::Hydro => 40_000.0,
            PowerPlantKind::Tidal => 55_000.0,
            PowerPlantKind::Geothermal => 50_000.0,
        }
    }

    pub fn upkeep_per_day(self) -> f32 {
        match self {
            PowerPlantKind::Coal => 900.0,
            PowerPlantKind::Solar => 60.0,
            PowerPlantKind::Wind => 90.0,
            PowerPlantKind::Nuclear => 2_500.0,
            PowerPlantKind::Hydro => 220.0,
            PowerPlantKind::Tidal => 320.0,
            PowerPlantKind::Geothermal => 380.0,
        }
    }

    pub fn jobs(self) -> u16 {
        match self {
            PowerPlantKind::Coal => 30,
            PowerPlantKind::Solar => 3,
            PowerPlantKind::Wind => 4,
            PowerPlantKind::Nuclear => 60,
            PowerPlantKind::Hydro => 10,
            PowerPlantKind::Tidal => 8,
            PowerPlantKind::Geothermal => 16,
        }
    }

    /// Effect on land value around the plant: (radius, weight). Every
    /// plant costs its neighbours something: coal most and furthest (smoke
    /// and ash), nuclear next (what people fear more than what it does),
    /// geothermal (the smell of sulphur), then hydro, wind and tidal
    /// (noise, the view), and solar least. The penalty is a nuisance (see
    /// [`crate::land_value::LandValue::for_use`]): homes feel it most,
    /// industry hardly at all.
    pub fn pollution(self) -> (f32, f32) {
        match self {
            PowerPlantKind::Coal => (360.0, -0.40),
            PowerPlantKind::Nuclear => (260.0, -0.18),
            PowerPlantKind::Geothermal => (180.0, -0.09),
            PowerPlantKind::Hydro => (130.0, -0.05),
            PowerPlantKind::Wind => (130.0, -0.045),
            PowerPlantKind::Tidal => (120.0, -0.045),
            PowerPlantKind::Solar => (90.0, -0.03),
        }
    }

    /// How much land value the plant takes overall: weight times the area
    /// of its cone. Used to order the plants for the UI and tests.
    pub fn pollution_load(self) -> f32 {
        let (r, w) = self.pollution();
        -w * r * r
    }

    /// Ash and spills on the ground around the plant per working hour:
    /// (radius, contamination). Coal only.
    pub fn ground_spill(self) -> Option<(f32, f32)> {
        match self {
            PowerPlantKind::Coal => Some((120.0, 0.003)),
            _ => None,
        }
    }

    pub fn makes_haze(self) -> bool {
        self == PowerPlantKind::Coal
    }

    /// Whether neighbouring plants of this kind read as one field: solar
    /// arrays and wind farms on parcels that touch are drawn as a single
    /// installation spanning them (see `World::plant_cluster`). Everything
    /// else is a building with its own fence.
    pub fn merges_with_neighbours(self) -> bool {
        matches!(self, PowerPlantKind::Solar | PowerPlantKind::Wind)
    }

    /// [clarity] Panels and turbines fill the lot they stand on: a solar
    /// field or wind farm's output, build cost and upkeep scale with the
    /// lot's area, so every megawatt costs the same whatever the lot. The
    /// nameplate figures (`capacity_mw`, `build_cost`, `upkeep_per_day`)
    /// are for a lot of this many square metres. `None`: a plant of fixed
    /// size.
    pub fn reference_area(self) -> Option<f32> {
        match self {
            PowerPlantKind::Solar => Some(SOLAR_REFERENCE_AREA),
            PowerPlantKind::Wind => Some(WIND_REFERENCE_AREA),
            _ => None,
        }
    }

    /// [clarity] What a lot of `area` square metres makes of the nameplate:
    /// area over the reference area for solar and wind, 1 for the rest.
    pub fn size_scale(self, area: f32) -> f32 {
        match self.reference_area() {
            Some(r) => (area.max(0.0) / r).max(0.05),
            None => 1.0,
        }
    }

    /// Smallest lot it fits on.
    pub fn min_area(self) -> f32 {
        match self {
            PowerPlantKind::Coal => 600.0,
            PowerPlantKind::Solar => 300.0,
            PowerPlantKind::Wind => 400.0,
            PowerPlantKind::Nuclear => 900.0,
            PowerPlantKind::Hydro | PowerPlantKind::Tidal => 300.0,
            PowerPlantKind::Geothermal => 400.0,
        }
    }

    pub fn allowed_on(self, lu: LandUse) -> bool {
        match self {
            PowerPlantKind::Coal | PowerPlantKind::Nuclear => lu == LandUse::Industrial,
            PowerPlantKind::Solar | PowerPlantKind::Wind => lu != LandUse::Park && lu != LandUse::Civic,
            // The site rule does the work for these.
            PowerPlantKind::Hydro | PowerPlantKind::Tidal | PowerPlantKind::Geothermal => lu != LandUse::Park,
        }
    }

    /// Air pollution per megawatt-hour generated: tonnes of CO2 (with the
    /// soot and sulphur that come with it). Burning coal is the one big
    /// emitter; geothermal wells vent some CO2 and hydrogen sulphide; the
    /// rest emit nothing while running (their footprint is in building
    /// them, which the city does not book here). The Power window
    /// multiplies this by output, so an idle coal plant is clean.
    pub fn co2_t_per_mwh(self) -> f32 {
        match self {
            PowerPlantKind::Coal => 0.95,
            PowerPlantKind::Geothermal => 0.08,
            PowerPlantKind::Nuclear | PowerPlantKind::Hydro | PowerPlantKind::Tidal | PowerPlantKind::Solar | PowerPlantKind::Wind => 0.0,
        }
    }

    /// Smog at the fence while running at full output, `0..1`: what the
    /// neighbours breathe (coal 1, geothermal's sulphur a little, the rest
    /// nothing).
    pub fn air_index(self) -> f32 {
        match self {
            PowerPlantKind::Coal => 1.0,
            PowerPlantKind::Geothermal => 0.15,
            _ => 0.0,
        }
    }

    /// Noise at the fence while running, dB(A): coal's mills, fans and
    /// conveyors loudest, then the geothermal wells' steam, the hydro and
    /// nuclear turbine halls and cooling, the tidal race, the swish of the
    /// wind farm; a solar farm only hums at its inverters.
    pub fn noise_db(self) -> f32 {
        match self {
            PowerPlantKind::Coal => 68.0,
            PowerPlantKind::Geothermal => 64.0,
            PowerPlantKind::Hydro => 60.0,
            PowerPlantKind::Nuclear => 58.0,
            PowerPlantKind::Tidal => 55.0,
            PowerPlantKind::Wind => 52.0,
            PowerPlantKind::Solar => 38.0,
        }
    }

    /// Share of nameplate output right now.
    pub fn availability(self, weather: &Weather, day_fraction: f32) -> f32 {
        match self {
            PowerPlantKind::Coal | PowerPlantKind::Nuclear | PowerPlantKind::Hydro | PowerPlantKind::Geothermal => 1.0,
            // Two tides a day; the race runs on the flood and the ebb.
            PowerPlantKind::Tidal => 0.2 + 0.8 * (day_fraction * 2.0 * std::f32::consts::TAU).sin().abs(),
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
    /// This hour's supply by kind of plant (`PowerPlantKind::index`), for
    /// the Power window's chart of where the megawatts come from.
    #[serde(default)]
    pub supply_by_kind: [f32; 7],
    /// Megawatts sold to the regional grid this hour: surplus the
    /// batteries had no room for.
    #[serde(default)]
    pub exported_mw: f32,
    /// What the regional grid owes for today's exports; paid at midnight
    /// under `Flow::GridExport`.
    #[serde(default)]
    pub export_income_today: f32,
    /// Surplus nobody would take this hour (batteries full, export line
    /// full): the plants are turned down.
    #[serde(default)]
    pub curtailed_mw: f32,
    /// Battery packs standing and working.
    #[serde(default)]
    pub batteries: u32,
    /// Energy stored in every battery, and what they could hold, MWh.
    #[serde(default)]
    pub battery_charge_mwh: f32,
    #[serde(default)]
    pub battery_capacity_mwh: f32,
    /// This hour, at the grid side: drawn into the batteries, and
    /// delivered from them, MW.
    #[serde(default)]
    pub battery_in_mw: f32,
    #[serde(default)]
    pub battery_out_mw: f32,
}

/// Power the regional grid will sell the city, in megawatts, and its price
/// per megawatt-hour. Enough for a village, not a city.
pub const GRID_IMPORT_MW: f32 = 60.0;
pub const GRID_IMPORT_PRICE: f32 = 2.5;
/// What the regional grid will take off the city's hands, in megawatts,
/// and what it pays per megawatt-hour: well under the import price (the
/// region buys at wholesale and sells at retail), so exporting pays for a
/// surplus but buying to sell back never does.
pub const GRID_EXPORT_MW: f32 = 120.0;
pub const GRID_EXPORT_PRICE: f32 = 0.5;

// ── batteries ──────────────────────────────────────────────────────────────

/// Share of the energy drawn into a battery that is stored, and of the
/// stored energy that reaches the grid again: about 90% round trip, as for
/// a lithium-iron pack with its inverters and cooling.
pub const BATTERY_CHARGE_EFF: f32 = 0.95;
pub const BATTERY_DISCHARGE_EFF: f32 = 0.95;

/// Grid battery packs the player places under Buildings, Power. They make
/// no power: they soak up surplus (before it is exported) and give it back
/// in a shortfall (before power is imported), so a solar noon can carry
/// into the evening peak and a calm hour need not go to the regional grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BatteryKind {
    Small,
    Medium,
    Large,
}

impl BatteryKind {
    pub const ALL: [BatteryKind; 3] = [BatteryKind::Small, BatteryKind::Medium, BatteryKind::Large];

    pub fn label(self) -> &'static str {
        match self {
            BatteryKind::Small => "small battery pack",
            BatteryKind::Medium => "battery pack",
            BatteryKind::Large => "battery farm",
        }
    }

    /// The remote API's name for it.
    pub fn key(self) -> &'static str {
        match self {
            BatteryKind::Small => "battery_small",
            BatteryKind::Medium => "battery_medium",
            BatteryKind::Large => "battery_large",
        }
    }

    pub fn from_key(s: &str) -> Option<BatteryKind> {
        match s {
            "battery_small" | "small_battery" => Some(BatteryKind::Small),
            "battery_medium" | "battery" | "medium_battery" => Some(BatteryKind::Medium),
            "battery_large" | "large_battery" | "battery_farm" => Some(BatteryKind::Large),
            _ => None,
        }
    }

    /// Energy it holds when full, MWh.
    pub fn capacity_mwh(self) -> f32 {
        match self {
            BatteryKind::Small => 20.0,
            BatteryKind::Medium => 80.0,
            BatteryKind::Large => 240.0,
        }
    }

    /// Fastest it charges or discharges, MW at the grid side: four hours
    /// from full to empty at full power for the packs, three for the farm.
    pub fn max_mw(self) -> f32 {
        match self {
            BatteryKind::Small => 5.0,
            BatteryKind::Medium => 20.0,
            BatteryKind::Large => 80.0,
        }
    }

    pub fn build_cost(self) -> f32 {
        match self {
            BatteryKind::Small => 6_000.0,
            BatteryKind::Medium => 20_000.0,
            BatteryKind::Large => 55_000.0,
        }
    }

    pub fn upkeep_per_day(self) -> f32 {
        match self {
            BatteryKind::Small => 30.0,
            BatteryKind::Medium => 90.0,
            BatteryKind::Large => 240.0,
        }
    }

    pub fn jobs(self) -> u16 {
        match self {
            BatteryKind::Small => 1,
            BatteryKind::Medium => 2,
            BatteryKind::Large => 4,
        }
    }

    pub fn min_area(self) -> f32 {
        match self {
            BatteryKind::Small => 150.0,
            BatteryKind::Medium => 350.0,
            BatteryKind::Large => 800.0,
        }
    }

    /// Anywhere but parks: a fenced yard of cabinets sits as well beside
    /// homes as beside a factory.
    pub fn allowed_on(self, lu: LandUse) -> bool {
        lu != LandUse::Park
    }

    /// Hum of the inverters and the cooling fans at the fence, dB(A).
    pub fn noise_db(self) -> f32 {
        match self {
            BatteryKind::Small => 40.0,
            BatteryKind::Medium => 44.0,
            BatteryKind::Large => 48.0,
        }
    }
}

/// A battery pack's state, on its building.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Battery {
    pub kind: BatteryKind,
    /// Energy stored, MWh, `0..=capacity`.
    #[serde(default)]
    pub charge_mwh: f32,
    /// This hour's flow at the grid side, MW: positive discharging into the
    /// grid, negative charging from it.
    #[serde(default)]
    pub flow_mw: f32,
}

impl Battery {
    pub fn new(kind: BatteryKind) -> Self {
        Self { kind, charge_mwh: 0.0, flow_mw: 0.0 }
    }

    /// Share full, `0..1`.
    pub fn level(&self) -> f32 {
        (self.charge_mwh / self.kind.capacity_mwh()).clamp(0.0, 1.0)
    }

    /// What its status lights show, coarsely: fifths of charge (rounded
    /// up) and whether it is charging (-1), holding (0) or delivering (1).
    /// The renderer redraws the pack when this changes.
    pub fn status(&self) -> (u8, i8) {
        let fifths = ((self.level() * 5.0) - 1e-3).ceil().max(0.0) as u8;
        let flow = if self.flow_mw < -0.05 { -1 } else if self.flow_mw > 0.05 { 1 } else { 0 };
        (fifths, flow)
    }

    /// Most it can take from the grid this hour, MW.
    pub fn room_mw(&self) -> f32 {
        ((self.kind.capacity_mwh() - self.charge_mwh).max(0.0) / BATTERY_CHARGE_EFF).min(self.kind.max_mw())
    }

    /// Most it can deliver to the grid this hour, MW.
    pub fn available_mw(&self) -> f32 {
        (self.charge_mwh.max(0.0) * BATTERY_DISCHARGE_EFF).min(self.kind.max_mw())
    }

    /// Run one hour at `flow` MW (grid side; positive discharging): the
    /// stored energy moves by the flow less the conversion losses.
    pub fn run_hour(&mut self, flow: f32) {
        self.flow_mw = flow;
        let stored = if flow < 0.0 { -flow * BATTERY_CHARGE_EFF } else { -flow / BATTERY_DISCHARGE_EFF };
        self.charge_mwh = (self.charge_mwh + stored).clamp(0.0, self.kind.capacity_mwh());
    }
}

/// How an hour's grid balance is met, worked out by [`dispatch`] from the
/// supply, the demand and the batteries' headroom; the world applies it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Dispatch {
    /// Into the batteries (grid side), MW, and the share of every
    /// battery's room that is used (`0..1`).
    pub charge_mw: f32,
    pub charge_share: f32,
    /// Out of the batteries, MW, and the share of every battery's
    /// available output that is used.
    pub discharge_mw: f32,
    pub discharge_share: f32,
    pub exported_mw: f32,
    pub imported_mw: f32,
    pub curtailed_mw: f32,
}

/// The merit order: a surplus charges the batteries first, is exported
/// with what is left (up to the export line) and curtailed beyond that; a
/// shortfall drains the batteries first and is imported with what is left
/// (up to the import line). Every battery takes or gives the same share of
/// its own headroom, so packs fill and empty together. `room` and
/// `available` are the batteries' summed headroom in MW.
pub fn dispatch(supply: f32, demand: f32, room: f32, available: f32) -> Dispatch {
    let mut d = Dispatch::default();
    let net = supply - demand;
    if net > 0.0 {
        d.charge_mw = net.min(room);
        d.charge_share = if room > 0.0 { d.charge_mw / room } else { 0.0 };
        let left = net - d.charge_mw;
        d.exported_mw = left.min(GRID_EXPORT_MW);
        d.curtailed_mw = left - d.exported_mw;
    } else if net < 0.0 {
        let need = -net;
        d.discharge_mw = need.min(available);
        d.discharge_share = if available > 0.0 { d.discharge_mw / available } else { 0.0 };
        d.imported_mw = (need - d.discharge_mw).min(GRID_IMPORT_MW);
    }
    d
}

/// Where the sun is at `day_fraction` (0 midnight, 0.5 noon), as a unit
/// vector in the renderer's frame (x east, y up, z the sim's y): it rises
/// in the east at 06:00, stands overhead at noon and sets in the west at
/// 18:00, the same path the app lights the city by and solar output
/// follows ([`PowerPlantKind::availability`]). `None` while it is down.
pub fn sun_direction(day_fraction: f32) -> Option<[f32; 3]> {
    let a = (day_fraction - 0.25) * std::f32::consts::TAU;
    let elev = a.sin();
    if elev <= 0.0 {
        return None;
    }
    let r = (1.0 - elev * elev).max(0.0).sqrt();
    Some([a.cos() * r, elev, a.sin() * r])
}

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
    per_floor * b.floors.max(1) as f32 * tier
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surplus_charges_then_exports_and_shortfall_drains_then_imports() {
        // Surplus of 50: 30 into the batteries, 20 exported.
        let d = dispatch(150.0, 100.0, 30.0, 0.0);
        assert_eq!((d.charge_mw, d.exported_mw, d.imported_mw, d.curtailed_mw), (30.0, 20.0, 0.0, 0.0));
        assert_eq!(d.charge_share, 1.0);
        // A surplus beyond the batteries and the export line is curtailed.
        let d = dispatch(400.0, 100.0, 10.0, 0.0);
        assert_eq!((d.charge_mw, d.exported_mw), (10.0, GRID_EXPORT_MW));
        assert!((d.curtailed_mw - (290.0 - GRID_EXPORT_MW)).abs() < 1e-3);
        // Shortfall of 40: the batteries give 25, 15 is imported.
        let d = dispatch(60.0, 100.0, 0.0, 25.0);
        assert_eq!((d.discharge_mw, d.imported_mw, d.exported_mw), (25.0, 15.0, 0.0));
        // Batteries with more than enough give a share of what they could.
        let d = dispatch(90.0, 100.0, 0.0, 40.0);
        assert_eq!((d.discharge_mw, d.imported_mw), (10.0, 0.0));
        assert!((d.discharge_share - 0.25).abs() < 1e-6);
        // The import line still caps what is bought.
        assert_eq!(dispatch(0.0, 500.0, 0.0, 0.0).imported_mw, GRID_IMPORT_MW);
        assert!(GRID_EXPORT_PRICE < GRID_IMPORT_PRICE, "selling back never pays for buying");
    }

    #[test]
    fn a_battery_loses_a_tenth_on_the_round_trip_and_stays_in_bounds() {
        let mut b = Battery::new(BatteryKind::Medium);
        assert_eq!(b.available_mw(), 0.0);
        assert_eq!(b.room_mw(), BatteryKind::Medium.max_mw());
        let mut drawn = 0.0;
        while b.room_mw() > 1e-4 {
            let f = b.room_mw();
            drawn += f;
            b.run_hour(-f);
        }
        assert!((b.charge_mwh - BatteryKind::Medium.capacity_mwh()).abs() < 1e-3, "full: {}", b.charge_mwh);
        let mut given = 0.0;
        while b.available_mw() > 1e-4 {
            let f = b.available_mw();
            given += f;
            b.run_hour(f);
        }
        assert!(b.charge_mwh.abs() < 1e-3);
        let eff = given / drawn;
        assert!((eff - BATTERY_CHARGE_EFF * BATTERY_DISCHARGE_EFF).abs() < 1e-3, "round trip {eff}");
        for k in BatteryKind::ALL {
            assert!(k.capacity_mwh() / k.max_mw() >= 3.0, "{k:?} holds at least three hours at full power");
            assert_eq!(BatteryKind::from_key(k.key()), Some(k));
        }
    }

    #[test]
    fn the_sun_rises_east_stands_high_at_noon_and_is_down_at_night() {
        assert!(sun_direction(0.0).is_none() && sun_direction(0.9).is_none());
        let morning = sun_direction(0.3).unwrap();
        assert!(morning[0] > 0.0 && morning[1] > 0.0);
        let noon = sun_direction(0.5).unwrap();
        assert!(noon[1] > 0.99);
        let evening = sun_direction(0.7).unwrap();
        assert!(evening[0] < 0.0);
        for t in [0.3, 0.42, 0.5, 0.61, 0.7] {
            let d = sun_direction(t).unwrap();
            assert!(((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() - 1.0).abs() < 1e-4);
        }
        assert!(PowerPlantKind::ALL.iter().all(|k| k.noise_db() > 30.0 && k.co2_t_per_mwh() >= 0.0));
        assert!(PowerPlantKind::Coal.co2_t_per_mwh() > PowerPlantKind::Geothermal.co2_t_per_mwh());
    }
}
