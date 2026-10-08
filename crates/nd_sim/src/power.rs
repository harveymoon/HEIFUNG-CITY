//! Electricity: plants the player places, a weather state that solar and
//! wind depend on, and an hourly allocation of supply to demand. The grid
//! itself is abstract (one city-wide pool); who goes dark first is not: the
//! rich are served first, so brownouts land on the cheap districts.

use crate::buildings::{Building, Tier};
use crate::zoning::LandUse;
use serde::{Deserialize, Serialize};

pub use crate::weather::{SiteResource, Spell, Weather};

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
/// [plant-groups] Most a single solar or wind lot makes of its nameplate.
pub const MAX_LOT_SCALE: f32 = 6.0;
/// [plant-scale] The site area the thermal plants' nameplates are quoted
/// for, and the most ground each puts to use: a bigger plant on a bigger
/// site, in a capped range (see [`PowerPlantKind::size_scale`]).
pub const COAL_REFERENCE_AREA: f32 = 1000.0;
pub const NUCLEAR_REFERENCE_AREA: f32 = 1500.0;
pub const GEOTHERMAL_REFERENCE_AREA: f32 = 700.0;
/// [freight] Hydro and tidal plants are sized to their site too: a lot of
/// this many m² holds the nameplate station (three turbine bays).
pub const HYDRO_REFERENCE_AREA: f32 = 700.0;
pub const TIDAL_REFERENCE_AREA: f32 = 700.0;

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
            // [free-placement] No zoning needed.
            PlantSite::Anywhere => "on any lot or unzoned ground by a road (not a park)",
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

    /// [plant-scale] The land-value cone of a plant of this size: a bigger
    /// coal plant smokes further and harder (by the square root of its
    /// size, so twice the plant is not twice the blight); 1 is the kind's.
    pub fn pollution_at(self, scale: f32) -> (f32, f32) {
        let (r, w) = self.pollution();
        let k = scale.clamp(0.25, 4.0).sqrt();
        (r * k, w * k)
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
    /// are for a lot of this many square metres. [plant-scale] Coal,
    /// nuclear and geothermal plants are sized to their site too, in a
    /// narrower range ([`Self::size_range`]), fixed when they are built
    /// ([`Self::sized_when_built`]); [freight] so are hydro and tidal
    /// (more turbine bays on a bigger site; hydro's output still follows
    /// the river's current as well). `None`: a plant of fixed size.
    pub fn reference_area(self) -> Option<f32> {
        match self {
            PowerPlantKind::Solar => Some(SOLAR_REFERENCE_AREA),
            PowerPlantKind::Wind => Some(WIND_REFERENCE_AREA),
            PowerPlantKind::Coal => Some(COAL_REFERENCE_AREA),
            PowerPlantKind::Nuclear => Some(NUCLEAR_REFERENCE_AREA),
            PowerPlantKind::Geothermal => Some(GEOTHERMAL_REFERENCE_AREA),
            PowerPlantKind::Hydro => Some(HYDRO_REFERENCE_AREA),
            PowerPlantKind::Tidal => Some(TIDAL_REFERENCE_AREA),
        }
    }

    /// [plant-scale] A thermal plant (coal, nuclear, geothermal): its size is
    /// worked out from its site when it is built and kept on the building
    /// (`Building::plant_scale`), so it never changes under it; a plant from
    /// an older save, built at the fixed size, keeps that (scale 1). Solar
    /// and wind follow their lot's area as they always have. [freight]
    /// Hydro and tidal stations too.
    pub fn sized_when_built(self) -> bool {
        matches!(self, PowerPlantKind::Coal | PowerPlantKind::Nuclear | PowerPlantKind::Geothermal | PowerPlantKind::Hydro | PowerPlantKind::Tidal)
    }

    /// [freight] Turbine bays in a hydro or tidal station of this size:
    /// three at the nameplate, one more for each further third of it.
    pub fn turbine_bays(scale: f32) -> u32 {
        ((scale * 3.0).round() as u32).clamp(1, 8)
    }

    /// [plant-scale] Most ground a plant of this kind puts to use, m²: a
    /// site bigger than this makes it no bigger (the plant stands on part
    /// of it), and free placement joins no more unzoned parcels than this.
    pub fn max_area(self) -> f32 {
        match self {
            PowerPlantKind::Coal => 2400.0,
            PowerPlantKind::Nuclear => 3000.0,
            PowerPlantKind::Geothermal => 1600.0,
            PowerPlantKind::Solar => SOLAR_REFERENCE_AREA * MAX_LOT_SCALE,
            PowerPlantKind::Wind => WIND_REFERENCE_AREA * MAX_LOT_SCALE,
            // [freight] Up to eight turbine bays (2.67× the nameplate).
            PowerPlantKind::Hydro | PowerPlantKind::Tidal => 1866.0,
        }
    }

    /// [plant-scale] How thick a site must be, m: twice its area over its
    /// perimeter (a 20 by 20 m square is 10, a 54 by 12 m strip 9.8), so a
    /// coal station is not put on a strip along the kerb. 0: any shape.
    pub fn min_thickness(self) -> f32 {
        match self {
            PowerPlantKind::Coal => 10.0,
            PowerPlantKind::Nuclear => 13.0,
            PowerPlantKind::Geothermal => 7.0,
            _ => 0.0,
        }
    }

    /// [plant-scale] The range of `size_scale`: (smallest, largest) share of
    /// the nameplate one plant can be. (1, 1) for a fixed-size kind.
    pub fn size_range(self) -> (f32, f32) {
        match self.reference_area() {
            Some(r) if self.sized_when_built() => (self.min_area() / r, self.max_area() / r),
            Some(_) => (0.05, MAX_LOT_SCALE),
            None => (1.0, 1.0),
        }
    }

    /// [clarity] What a lot of `area` square metres makes of the nameplate:
    /// area over the reference area, within [`Self::size_range`]; 1 for a
    /// fixed-size kind.
    pub fn size_scale(self, area: f32) -> f32 {
        match self.reference_area() {
            // [plant-groups] Capped: past six reference lots a single lot
            // gains nothing more (join lots into a park to go bigger).
            // [plant-scale] Thermal plants between their smallest and
            // largest size.
            Some(r) => {
                let (lo, hi) = self.size_range();
                (area.max(0.0) / r).clamp(lo, hi)
            }
            None => 1.0,
        }
    }

    /// [plant-scale] Nameplate, cost and upkeep at a given size.
    pub fn capacity_at(self, scale: f32) -> f32 {
        self.capacity_mw() * scale
    }

    pub fn cost_at(self, scale: f32) -> f32 {
        self.build_cost() * scale
    }

    pub fn upkeep_at(self, scale: f32) -> f32 {
        self.upkeep_per_day() * scale
    }

    /// [plant-scale] Jobs at a plant of this size: a bigger plant has more
    /// shifts on the boilers and the coal yard, or more turbines to mind
    /// (never fewer than a third).
    pub fn jobs_at(self, scale: f32) -> u16 {
        if !self.sized_when_built() {
            return self.jobs();
        }
        ((self.jobs() as f32 * scale).round() as u16).max(self.jobs() / 3).max(1)
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

    /// Share of nameplate output right now, for a plant on a site with
    /// these resources (`SiteResource::default()`: open, level ground).
    /// [grid-weather] Solar follows the sun's height, the cloud, the
    /// climate's sun and the site's, less a little in the heat; wind runs
    /// the turbines' power curve on the wind at the site (nothing above
    /// cut-out in a storm).
    pub fn availability(self, weather: &Weather, day_fraction: f32, site: SiteResource) -> f32 {
        match self {
            PowerPlantKind::Coal | PowerPlantKind::Nuclear | PowerPlantKind::Hydro | PowerPlantKind::Geothermal => 1.0,
            // Two tides a day; the race runs on the flood and the ebb.
            PowerPlantKind::Tidal => 0.2 + 0.8 * (day_fraction * 2.0 * std::f32::consts::TAU).sin().abs(),
            PowerPlantKind::Solar => {
                let elev = ((day_fraction - 0.25) * std::f32::consts::TAU).sin().max(0.0);
                let sky = 1.0 - 0.75 * weather.cloud;
                (elev * sky * weather.profile().sun * site.sun * crate::weather::panel_derate(weather.temp_c)).clamp(0.0, 1.0)
            }
            PowerPlantKind::Wind => crate::weather::turbine_curve(weather.wind_ms() * site.wind),
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
    /// [grid-weather] The regional grid's prices this hour, per MWh
    /// ([`import_price`], [`export_price`]).
    #[serde(default)]
    pub import_price: f32,
    #[serde(default)]
    pub export_price: f32,
    /// [batteries] The batteries' rule this hour and the day's price bands
    /// it was read from ([`PriceBands`]).
    #[serde(default)]
    pub battery_mode: BatteryMode,
    #[serde(default)]
    pub price_bands: PriceBands,
    /// [batteries] Of `imported_mw`, bought this hour to charge the batteries.
    #[serde(default)]
    pub battery_grid_charge_mw: f32,
    /// [batteries] What the batteries saved today and this hour
    /// ([`Dispatch::battery_savings`]).
    #[serde(default)]
    pub battery_savings_today: f32,
    #[serde(default)]
    pub battery_savings_hour: f32,
}

/// Power the regional grid will sell the city, in megawatts, and its mean
/// price per megawatt-hour (the price moves with the hour and the weather:
/// [`import_price`]). Enough for a village, not a city.
pub const GRID_IMPORT_MW: f32 = 60.0;
pub const GRID_IMPORT_PRICE: f32 = 2.5;
/// What the regional grid will take off the city's hands, in megawatts,
/// and what it pays per megawatt-hour on average ([`export_price`]): well
/// under the import price (the
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
    /// [batteries] Its share of what the batteries saved today and
    /// yesterday (by its share of the hour's flow).
    #[serde(default)]
    pub savings_today: f32,
    #[serde(default)]
    pub savings_yesterday: f32,
}

impl Battery {
    pub fn new(kind: BatteryKind) -> Self {
        Self { kind, charge_mwh: 0.0, flow_mw: 0.0, savings_today: 0.0, savings_yesterday: 0.0 }
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
    /// [batteries] Of `imported_mw`, what was bought to charge the
    /// batteries (part of `charge_mw` too).
    pub grid_charge_mw: f32,
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

/// [batteries] How many of the day's hours count as cheap (the batteries
/// buy from the regional grid) and as dear (they deliver first).
pub const ARBITRAGE_HOURS: usize = 6;
/// Buying cheap to give back dear must clear the round-trip losses and
/// this margin on top, or the batteries do not buy from the grid at all.
pub const ARBITRAGE_MARGIN: f32 = 0.9;

/// [batteries] What the batteries do with the grid this hour, read off the
/// day's price curve ([`PriceBands::mode`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BatteryMode {
    /// A cheap hour: take surplus, and buy from the regional grid to fill up.
    Charge,
    /// A middle hour: take surplus, keep the charge for the dear hours (give
    /// only what keeps the lights on past the import line).
    #[default]
    Hold,
    /// A dear hour: deliver before anything is imported.
    Deliver,
}

impl BatteryMode {
    pub fn label(self) -> &'static str {
        match self {
            BatteryMode::Charge => "cheap hour: charging from the grid",
            BatteryMode::Hold => "middle hour: holding for the peak",
            BatteryMode::Deliver => "dear hour: delivering first",
        }
    }
}

/// [batteries] The day's price thresholds: an hour at or under `cheap`
/// (the [`ARBITRAGE_HOURS`]th cheapest of the day) is for charging, one at
/// or over `dear` (the [`ARBITRAGE_HOURS`]th dearest) is for delivering.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PriceBands {
    pub cheap: f32,
    pub dear: f32,
}

impl PriceBands {
    /// The bands of a day's hourly prices (any length; 24 in the game).
    pub fn of(prices: &[f32]) -> Self {
        if prices.is_empty() {
            return Self::default();
        }
        let mut s = prices.to_vec();
        s.sort_by(f32::total_cmp);
        let k = ARBITRAGE_HOURS.min(s.len()).max(1);
        Self { cheap: s[k - 1], dear: s[s.len() - k] }
    }

    /// Whether power bought in the cheap hours is worth giving back in the
    /// dear ones, after the round trip's losses and a margin.
    pub fn worth_storing(&self) -> bool {
        self.cheap < self.dear * BATTERY_CHARGE_EFF * BATTERY_DISCHARGE_EFF * ARBITRAGE_MARGIN
    }

    /// The rule for an hour at `price`.
    pub fn mode(&self, price: f32) -> BatteryMode {
        if price >= self.dear && self.dear > self.cheap {
            BatteryMode::Deliver
        } else if price <= self.cheap && self.worth_storing() {
            BatteryMode::Charge
        } else if self.dear <= self.cheap {
            // A flat day: nothing to play, the old merit order.
            BatteryMode::Deliver
        } else {
            BatteryMode::Hold
        }
    }
}

/// [batteries] [`dispatch`] with the batteries playing the day's prices
/// (`mode`): a surplus always charges them first; in a cheap hour they
/// also buy from the regional grid (within its import line) to fill up;
/// in a shortfall they deliver first only in a dear hour, otherwise the
/// city imports first and they give only what the import line cannot
/// (reliability before money: nobody sits in the dark to save a charge).
pub fn dispatch_tou(supply: f32, demand: f32, room: f32, available: f32, mode: BatteryMode) -> Dispatch {
    if mode == BatteryMode::Deliver {
        return dispatch(supply, demand, room, available);
    }
    let mut d = Dispatch::default();
    let net = supply - demand;
    if net >= 0.0 {
        d = dispatch(supply, demand, room, available);
        if mode == BatteryMode::Charge {
            let buy = (room - d.charge_mw).max(0.0).min(GRID_IMPORT_MW);
            d.grid_charge_mw = buy;
            d.imported_mw += buy;
            d.charge_mw += buy;
        }
    } else {
        let need = -net;
        d.imported_mw = need.min(GRID_IMPORT_MW);
        d.discharge_mw = (need - d.imported_mw).min(available);
        if mode == BatteryMode::Charge && d.discharge_mw <= 0.0 {
            let buy = room.min(GRID_IMPORT_MW - d.imported_mw).max(0.0);
            d.grid_charge_mw = buy;
            d.imported_mw += buy;
            d.charge_mw = buy;
        }
    }
    d.charge_share = if room > 0.0 { (d.charge_mw / room).min(1.0) } else { 0.0 };
    d.discharge_share = if available > 0.0 { (d.discharge_mw / available).min(1.0) } else { 0.0 };
    d
}

impl Dispatch {
    /// [batteries] What the batteries saved this hour, in money: imports
    /// they spared (what they delivered, at the import price) less what
    /// was bought from the grid to charge them, less the exports they
    /// cost (surplus they took that the export line could have sold).
    pub fn battery_savings(&self, import_price: f32, export_price: f32) -> f32 {
        let from_surplus = (self.charge_mw - self.grid_charge_mw).max(0.0);
        let forgone = from_surplus.min((GRID_EXPORT_MW - self.exported_mw).max(0.0));
        self.discharge_mw * import_price - self.grid_charge_mw * import_price - forgone * export_price
    }
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
    // [supertall] Lifts, pumps and cooling up a league tower.
    let tall = if b.plot.tower.is_some() { 1.25 } else { 1.0 };
    per_floor * b.floors.max(1) as f32 * tier * tall
}

// ── [grid-weather] the daily load curve ─────────────────────────────────────

/// Share of a building's full draw (`demand_mw`) it uses at each hour of
/// the day, by land use: homes have a morning bump and the big evening
/// peak when people get home, cook and switch everything on; shops and
/// offices run through the working day; industry runs shifts round the
/// clock and dips at night; nightlife peaks late; parks are their lamps.
const PROFILE: [[f32; 24]; 6] = [
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

fn use_index(lu: LandUse) -> usize {
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
fn climate_shares(lu: LandUse) -> (f32, f32) {
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
    pub by_use: [f32; 6],
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

// ── [grid-weather] time-of-use prices ───────────────────────────────────────

/// The regional grid's load at `hour` in air of `temp_c`, against a mild
/// day's peak: the region is mostly homes and offices with some industry,
/// so its evening peak and its heat-wave afternoons are the city's too.
pub fn regional_load(hour: f32, temp_c: f32) -> f32 {
    0.45 * load_factor(LandUse::Residential, hour, temp_c) + 0.35 * load_factor(LandUse::Commercial, hour, temp_c) + 0.20 * load_factor(LandUse::Industrial, hour, temp_c)
}

/// The regional load the price is quoted at: the mean price of a mild day
/// comes out near [`GRID_IMPORT_PRICE`].
const PRICE_LOAD: f32 = 0.82;

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

    /// [batteries] The day's six cheapest hours charge, its six dearest
    /// deliver; a flat day plays nothing; buying cheap only when the round
    /// trip pays.
    #[test]
    fn price_bands_split_the_day_and_the_rule_is_explainable() {
        let mild = crate::weather::Weather::default();
        let prices: Vec<f32> = (0..24).map(|h| import_price(h as f32, mild.temp_at(h as f32))).collect();
        let b = PriceBands::of(&prices);
        assert!(b.cheap < b.dear && b.worth_storing(), "{b:?}");
        let modes: Vec<BatteryMode> = prices.iter().map(|p| b.mode(*p)).collect();
        assert_eq!(modes.iter().filter(|m| **m == BatteryMode::Charge).count(), ARBITRAGE_HOURS, "{prices:?}");
        assert_eq!(modes.iter().filter(|m| **m == BatteryMode::Deliver).count(), ARBITRAGE_HOURS, "{prices:?}");
        // Charging in the small hours, delivering in the evening.
        assert_eq!(modes[3], BatteryMode::Charge, "{modes:?}");
        assert_eq!(modes[19], BatteryMode::Deliver, "{modes:?}");
        // A flat price: the old merit order (deliver in any shortfall).
        let flat = PriceBands::of(&[2.0; 24]);
        assert_eq!(flat.mode(2.0), BatteryMode::Deliver);
        assert!(!flat.worth_storing());
        // Barely different prices do not pay the losses: no buying.
        let tight = PriceBands { cheap: 2.0, dear: 2.1 };
        assert_ne!(tight.mode(2.0), BatteryMode::Charge);
    }

    #[test]
    fn the_hour_rule_charges_holds_and_delivers() {
        // A shortfall of 30 MW with 20 MW of battery room and output.
        let hold = dispatch_tou(100.0, 130.0, 20.0, 20.0, BatteryMode::Hold);
        assert_eq!((hold.imported_mw, hold.discharge_mw, hold.charge_mw), (30.0, 0.0, 0.0), "holding imports first");
        let deliver = dispatch_tou(100.0, 130.0, 20.0, 20.0, BatteryMode::Deliver);
        assert_eq!((deliver.discharge_mw, deliver.imported_mw), (20.0, 10.0), "a dear hour delivers first");
        let cheap = dispatch_tou(100.0, 130.0, 20.0, 20.0, BatteryMode::Charge);
        assert_eq!((cheap.imported_mw, cheap.grid_charge_mw, cheap.charge_mw, cheap.discharge_mw), (50.0, 20.0, 20.0, 0.0), "a cheap hour buys to fill up");
        // Past the import line the battery keeps the lights on, whatever the hour.
        let short = dispatch_tou(0.0, GRID_IMPORT_MW + 15.0, 20.0, 20.0, BatteryMode::Hold);
        assert_eq!((short.imported_mw, short.discharge_mw), (GRID_IMPORT_MW, 15.0));
        let short = dispatch_tou(0.0, GRID_IMPORT_MW + 15.0, 20.0, 20.0, BatteryMode::Charge);
        assert_eq!((short.discharge_mw, short.grid_charge_mw), (15.0, 0.0), "never buys to charge while it must give");
        // A surplus always charges first; a cheap hour tops up from the grid.
        let sun = dispatch_tou(130.0, 100.0, 50.0, 20.0, BatteryMode::Charge);
        assert_eq!((sun.charge_mw, sun.grid_charge_mw, sun.imported_mw, sun.exported_mw), (50.0, 20.0, 20.0, 0.0));
        // Savings: imports spared at the price, less what was bought, less
        // the exports the surplus would have earned.
        assert_eq!(deliver.battery_savings(4.0, 0.8), 20.0 * 4.0);
        assert_eq!(cheap.battery_savings(1.0, 0.2), -20.0);
        let s = dispatch_tou(130.0, 100.0, 10.0, 0.0, BatteryMode::Hold);
        assert!((s.battery_savings(2.0, 0.4) + 10.0 * 0.4).abs() < 1e-4);
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

    /// A city's mix of full draw: homes, shops, some industry, a little
    /// nightlife and civic.
    fn city() -> DemandBase {
        DemandBase { by_use: [40.0, 25.0, 25.0, 5.0, 5.0, 0.5], sites: 0.0 }
    }

    fn day(base: &DemandBase, temp: impl Fn(f32) -> f32) -> Vec<f32> {
        (0..24).map(|h| base.at(h as f32, temp(h as f32))).collect()
    }

    #[test]
    fn demand_peaks_in_the_evening_and_dips_at_night() {
        let mild = |_| 18.0;
        // Homes: a morning bump, a midday lull and the big evening peak.
        let res = |h: f32| hourly_load(LandUse::Residential, h);
        assert!(res(7.0) > res(3.0) && res(7.0) > res(11.0), "morning bump");
        assert!(res(19.0) > res(7.0) && res(19.0) > res(12.0), "evening peak");
        let peak_hour = (0..24).max_by(|a, b| res(*a as f32).total_cmp(&res(*b as f32))).unwrap();
        assert!((18..=21).contains(&peak_hour), "homes peak when people get home: {peak_hour}");
        // Offices run in the day, nightlife at night, industry flat.
        assert!(hourly_load(LandUse::Commercial, 12.0) > 2.5 * hourly_load(LandUse::Commercial, 3.0));
        assert!(hourly_load(LandUse::Entertainment, 21.0) > 2.5 * hourly_load(LandUse::Entertainment, 6.0));
        let ind: Vec<f32> = (0..24).map(|h| hourly_load(LandUse::Industrial, h as f32)).collect();
        let (lo, hi) = ind.iter().fold((9.0f32, 0.0f32), |(a, b), v| (a.min(*v), b.max(*v)));
        assert!(hi / lo < 1.6, "industry runs shifts round the clock");
        assert!(hourly_load(LandUse::Park, 23.0) > 2.0 * hourly_load(LandUse::Park, 12.0), "park lamps at night");
        // Interpolated smoothly between the hours, wrapping at midnight.
        assert!((hourly_load(LandUse::Residential, 23.5) - (PROFILE[0][23] + PROFILE[0][0]) / 2.0).abs() < 1e-5);
        // The city: evening peak over the night trough, and no more than a
        // touch above the old flat draw, so a city that had power has it.
        let base = city();
        let d = day(&base, mild);
        let (lo, hi) = d.iter().fold((f32::MAX, 0.0f32), |(a, b), v| (a.min(*v), b.max(*v)));
        let mean = d.iter().sum::<f32>() / 24.0;
        let peak = d.iter().position(|v| *v == hi).unwrap();
        assert!(hi / lo > 1.6 && hi / lo < 2.4, "peak/trough {}", hi / lo);
        assert!((10..=20).contains(&peak), "the city peaks in the day or evening: {peak}");
        assert!(hi <= base.full() * 1.05, "a mild day's peak stays near the old flat draw: {hi} vs {}", base.full());
        assert!(mean < base.full() * 0.9, "the day uses less than running flat out: {mean}");
    }

    #[test]
    fn heat_and_cold_add_load() {
        let base = city();
        let mild = base.at(15.0, 20.0);
        assert!(base.at(15.0, 40.0) > mild * 1.12, "air conditioning on a hot afternoon");
        assert!(base.at(15.0, -15.0) > mild * 1.12, "heating in the cold");
        assert_eq!(weather_load(LandUse::Park, 45.0), 1.0);
        // Desert, ordinary day: the evening peak with its AC still within
        // a sixth of the old flat draw (no blackouts every evening for a
        // city that had enough before).
        let w = Weather::for_climate(crate::landscape::Climate::Desert);
        let d = day(&base, |h| w.temp_at(h));
        let hi = d.iter().cloned().fold(0.0f32, f32::max);
        assert!(hi < base.full() * 1.15, "desert peak {hi} vs flat {}", base.full());
        // A heat wave pushes it up.
        let hot = Weather { spell: Spell::HeatWave, ..w.clone() };
        let dh = day(&base, |h| hot.temp_at(h));
        assert!(dh.iter().cloned().fold(0.0f32, f32::max) > hi * 1.05);
        // A demand_at for one building follows the same curve.
        let mut b = Building::new(crate::LotId::default(), LandUse::Residential, 10, Tier::Mid, 1, "test".into(), 0, 0.5);
        assert!((demand_at(&b, 19.0, 20.0) - demand_mw(&b) * load_factor(LandUse::Residential, 19.0, 20.0)).abs() < 1e-6);
        b.construction = Some(crate::buildings::Construction::new(1.0));
        assert_eq!(demand_at(&b, 19.0, 40.0), demand_mw(&b), "a site draws its flat figure");
    }

    #[test]
    fn power_is_cheap_at_night_dear_in_the_evening_and_dearer_in_a_heat_wave() {
        let mild = Weather::for_climate(crate::landscape::Climate::Temperate);
        let prices: Vec<f32> = (0..24).map(|h| import_price(h as f32, mild.temp_at(h as f32))).collect();
        let mean = prices.iter().sum::<f32>() / 24.0;
        let (lo, hi) = prices.iter().fold((f32::MAX, 0.0f32), |(a, b), v| (a.min(*v), b.max(*v)));
        let cheapest = prices.iter().position(|v| *v == lo).unwrap();
        let dearest = prices.iter().position(|v| *v == hi).unwrap();
        assert!(cheapest <= 5, "cheapest in the small hours: {cheapest}");
        assert!((16..=21).contains(&dearest), "dearest late afternoon or evening: {dearest}");
        assert!(hi > 2.0 * lo, "a real spread: {lo}..{hi}");
        assert!((mean - GRID_IMPORT_PRICE).abs() < 0.5, "a mild day averages the old price: {mean}");
        // Heat wave evenings cost more.
        let hot = Weather { spell: Spell::HeatWave, ..Weather::for_climate(crate::landscape::Climate::Desert) };
        assert!(import_price(18.0, hot.temp_at(18.0)) > hi * 1.15);
        // Exports always earn less than imports cost.
        for h in 0..24 {
            for t in [-20.0, 10.0, 30.0, 45.0] {
                assert!(export_price(h as f32, t) < import_price(h as f32, t));
            }
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
