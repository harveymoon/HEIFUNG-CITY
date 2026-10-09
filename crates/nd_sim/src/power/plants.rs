//! Power plants: their kinds, sites, sizes, output, costs and looks, and
//! the grid's statistics.

use super::*;

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
    /// the river's current as well). Every kind has one today; `None` would
    /// be a plant of fixed size.
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
    #[serde(default, deserialize_with = "crate::economy::padded")]
    pub supply_by_kind: [f32; PowerPlantKind::ALL.len()],
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
