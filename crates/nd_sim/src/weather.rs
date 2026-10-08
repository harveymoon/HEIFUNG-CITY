//! [grid-weather] Weather by climate, and the wind and sun the map offers.
//!
//! The weather is one city-wide state, stepped once a sim hour: cloud and
//! wind drift (the same two draws from the world's random stream as ever),
//! pulled toward what the map's [`Climate`] is like at that hour; the
//! temperature follows the climate's mean and its daily swing; and now and
//! then a *spell* (a heat wave, a cold snap, a storm) sets in for a day or
//! a few. Spells, the temperature's wander and everything else new are
//! decided by hashes of the landscape seed and the hour, never by the
//! random stream, so adding them shifted nothing else in the city.
//!
//! The resources are spatial and static: a wind field over the map (open
//! hills, ridges, coasts and wide water are windy; valleys and hollows are
//! sheltered) scaled by the climate's mean wind, and a sun factor per
//! climate (Tropical and Desert strong, Arctic weak) with a little more on
//! high open ground. Wind farms and solar fields read them at their lot
//! (`World::plant_output_mw`), and the land survey shows them as map
//! layers (`crate::survey::LandLayer::Wind`, `Solar`).

use crate::landscape::{fbm, Climate, Landscape};
use crate::pedestrians::hash01;
use glam::Vec2;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::f32::consts::TAU;

/// A run of unusual weather lasting a day or a few.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Spell {
    #[default]
    Normal,
    /// Hot days and warm nights: air conditioning everywhere, clear skies,
    /// solar panels a little less efficient in the heat, dear power.
    HeatWave,
    /// Bitter cold: heating load day and night, a stiff wind.
    ColdSnap,
    /// Gales and low cloud: wind farms on exposed ground shut down to
    /// save their blades (cut-out), sheltered ones run flat out.
    Storm,
}

impl Spell {
    pub fn label(self) -> &'static str {
        match self {
            Spell::Normal => "normal",
            Spell::HeatWave => "heat wave",
            Spell::ColdSnap => "cold snap",
            Spell::Storm => "storm",
        }
    }

    /// Degrees it puts on (or takes off) the day's mean temperature.
    pub fn temp_offset(self) -> f32 {
        match self {
            Spell::Normal => 0.0,
            Spell::HeatWave => 8.0,
            Spell::ColdSnap => -9.0,
            Spell::Storm => -3.0,
        }
    }
}

/// What a climate's weather is like.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ClimateProfile {
    /// Daily mean temperature, °C, and the swing either side of it from the
    /// cool of dawn to the heat of mid-afternoon (on a clear day).
    pub mean_temp_c: f32,
    pub daily_swing_c: f32,
    /// The cloud the sky drifts toward, `0..1`, and how much builds up in
    /// the afternoon on top (tropical thunderheads).
    pub cloud: f32,
    pub afternoon_cloud: f32,
    /// Mean wind at hub height over open, level ground, m/s.
    pub mean_wind_ms: f32,
    /// Strength of the sun on a clear day against the desert's, `0..1.1`:
    /// how high the sun climbs and how thin the air is.
    pub sun: f32,
    /// Chance per day a heat wave, a cold snap or a storm sets in.
    pub heat_wave: f32,
    pub cold_snap: f32,
    pub storm: f32,
}

/// The weather of each climate.
pub fn profile(c: Climate) -> ClimateProfile {
    match c {
        Climate::Temperate => ClimateProfile { mean_temp_c: 13.0, daily_swing_c: 5.0, cloud: 0.5, afternoon_cloud: 0.08, mean_wind_ms: 7.5, sun: 0.82, heat_wave: 0.04, cold_snap: 0.04, storm: 0.06 },
        Climate::Desert => ClimateProfile { mean_temp_c: 25.0, daily_swing_c: 8.0, cloud: 0.12, afternoon_cloud: 0.03, mean_wind_ms: 7.0, sun: 1.0, heat_wave: 0.07, cold_snap: 0.0, storm: 0.03 },
        Climate::Tropical => ClimateProfile { mean_temp_c: 28.0, daily_swing_c: 4.0, cloud: 0.22, afternoon_cloud: 0.42, mean_wind_ms: 6.0, sun: 1.08, heat_wave: 0.05, cold_snap: 0.0, storm: 0.06 },
        Climate::Arctic => ClimateProfile { mean_temp_c: -6.0, daily_swing_c: 3.5, cloud: 0.55, afternoon_cloud: 0.0, mean_wind_ms: 9.0, sun: 0.55, heat_wave: 0.0, cold_snap: 0.08, storm: 0.07 },
        Climate::Volcanic => ClimateProfile { mean_temp_c: 3.0, daily_swing_c: 3.0, cloud: 0.62, afternoon_cloud: 0.05, mean_wind_ms: 9.5, sun: 0.6, heat_wave: 0.0, cold_snap: 0.06, storm: 0.08 },
    }
}

/// Wind turbines start turning at this wind, m/s ...
pub const CUT_IN_MS: f32 = 3.0;
/// ... reach their nameplate at this one ...
pub const RATED_MS: f32 = 11.0;
/// ... and are feathered and braked above this one to save the blades.
pub const CUT_OUT_MS: f32 = 25.0;
/// What a storm adds to the wind, m/s, on top of the strongest the normal
/// drift reaches.
pub const STORM_GUST_MS: f32 = 7.0;

/// A turbine's power curve: share of nameplate at wind `v` m/s. Nothing
/// below cut-in, a ramp to rated, flat to cut-out, nothing above (a gentler
/// ramp than a real cube law: the game's turbines are good ones).
pub fn turbine_curve(v: f32) -> f32 {
    if !(CUT_IN_MS..CUT_OUT_MS).contains(&v) {
        return 0.0;
    }
    ((v - CUT_IN_MS) / (RATED_MS - CUT_IN_MS)).clamp(0.0, 1.0)
}

/// Solar panels lose this share of their output per °C above 25.
pub const PANEL_HEAT_LOSS: f32 = 0.004;

/// Share of a panel's output left at `temp_c` (heat wave afternoons cost a
/// few percent).
pub fn panel_derate(temp_c: f32) -> f32 {
    (1.0 - PANEL_HEAT_LOSS * (temp_c - 25.0).max(0.0)).clamp(0.7, 1.0)
}

/// Shape of the day's temperature: -1 at 05:00, +1 at 15:00.
pub fn diurnal(hour: f32) -> f32 {
    // A cosine peaking at 15:00, its trough nudged to 05:00 by warping the
    // morning half (the air warms faster than it cools).
    let h = hour.rem_euclid(24.0);
    let phase = if (5.0..15.0).contains(&h) { (h - 5.0) / 10.0 * 0.5 } else { 0.5 + ((h - 15.0).rem_euclid(24.0)) / 14.0 * 0.5 };
    -(phase * TAU).cos()
}

/// [dirty-industry] The way the wind blows (unit vector in the sim plane,
/// the direction the air moves): a prevailing direction that veers slowly
/// over the days. Smoke, smog and soot drift along it (`render/haze.rs`),
/// and so does contamination on the ground (`Contamination::drift`).
pub fn wind_dir(tick: u64) -> Vec2 {
    let a = 0.55 + 0.9 * ((tick as f32 / 1440.0) * 0.7).sin();
    Vec2::new(a.cos(), a.sin())
}

/// Sky, wind and temperature, stepped once a sim hour. Cloud dims solar;
/// wind drives the turbines; heat and cold drive the city's demand.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Weather {
    pub cloud: f32,
    /// The city's wind on a `0..1` scale: about 0.45 on an ordinary day,
    /// 1 in a gale. [`Weather::wind_ms`] turns it into m/s for the climate.
    pub wind: f32,
    /// Air temperature now, °C.
    #[serde(default = "mild")]
    pub temp_c: f32,
    /// How far this stretch of days runs above or below the climate's
    /// mean, °C (drifts slowly).
    #[serde(default)]
    pub temp_anomaly: f32,
    #[serde(default)]
    pub spell: Spell,
    /// Hours the spell has left.
    #[serde(default)]
    pub spell_hours: u32,
    /// The map's climate, kept here so the weather reads on its own.
    #[serde(default)]
    pub climate: Climate,
}

fn mild() -> f32 {
    profile(Climate::default()).mean_temp_c
}

impl Default for Weather {
    fn default() -> Self {
        Self::for_climate(Climate::default())
    }
}

impl Weather {
    /// A fresh map's weather: the climate's ordinary day at midnight.
    pub fn for_climate(climate: Climate) -> Self {
        let p = profile(climate);
        Self { cloud: p.cloud.max(0.1), wind: 0.45, temp_c: p.mean_temp_c + p.daily_swing_c * diurnal(0.0), temp_anomaly: 0.0, spell: Spell::Normal, spell_hours: 0, climate }
    }

    pub fn profile(&self) -> ClimateProfile {
        profile(self.climate)
    }

    /// One hour on: `abs_hour` is the sim's hour count since the start
    /// (tick / 60), `seed` the landscape's seed. Draws exactly two numbers
    /// from `rng` (the drift of cloud and wind), as the old weather did;
    /// spells and the temperature's wander come from hashes of the seed
    /// and the hour.
    pub fn step(&mut self, rng: &mut impl Rng, climate: Climate, abs_hour: u64, seed: u64) {
        let dc: f32 = rng.random_range(-0.08..0.08);
        let dw: f32 = rng.random_range(-0.07..0.07);
        self.climate = climate;
        let p = profile(climate);
        let hour = (abs_hour % 24) as f32;
        // Spells: run down the one on, or maybe start one.
        if self.spell_hours > 0 {
            self.spell_hours -= 1;
        }
        if self.spell_hours == 0 {
            self.spell = Spell::Normal;
            let r = hash01(seed ^ 0x5be1, abs_hour, 1);
            let len = hash01(seed ^ 0x5be1, abs_hour, 2);
            let (h, c, s) = (p.heat_wave / 24.0, p.cold_snap / 24.0, p.storm / 24.0);
            let (spell, hours) = if r < h {
                (Spell::HeatWave, 48.0 + 48.0 * len)
            } else if r < h + c {
                (Spell::ColdSnap, 36.0 + 36.0 * len)
            } else if r < h + c + s {
                (Spell::Storm, 10.0 + 20.0 * len)
            } else {
                (Spell::Normal, 0.0)
            };
            self.spell = spell;
            self.spell_hours = hours as u32;
        }
        // Cloud drifts toward the climate's sky at this hour.
        let afternoon = (-((hour - 15.5) / 2.5).powi(2)).exp();
        let target_cloud = match self.spell {
            Spell::Storm => 0.95,
            Spell::HeatWave => p.cloud * 0.25,
            _ => p.cloud + p.afternoon_cloud * afternoon,
        };
        self.cloud = (self.cloud + dc + 0.12 * (target_cloud - self.cloud)).clamp(0.0, 1.0);
        // Wind likewise: a breeze picking up in the afternoon, a gale in a storm.
        let target_wind = match self.spell {
            Spell::Storm => 0.95,
            Spell::ColdSnap => 0.6,
            _ => 0.42 + 0.08 * afternoon,
        };
        let pull = if self.spell == Spell::Storm { 0.35 } else { 0.08 };
        self.wind = (self.wind + dw + pull * (target_wind - self.wind)).clamp(0.05, 1.0);
        // Temperature: the climate, this week's wander, the spell, the hour.
        let wander = (hash01(seed ^ 0x7e3a, abs_hour, 3) - 0.5) * 0.9;
        self.temp_anomaly = (self.temp_anomaly * 0.985 + wander).clamp(-6.0, 6.0);
        self.temp_c = self.temp_at(hour);
    }

    /// The temperature at `hour` of the day if nothing changes but the sun:
    /// the climate's mean, the wander and the spell, with the daily swing
    /// (damped by cloud).
    pub fn temp_at(&self, hour: f32) -> f32 {
        let p = self.profile();
        let swing = p.daily_swing_c * (1.0 - 0.5 * self.cloud);
        p.mean_temp_c + self.temp_anomaly + self.spell.temp_offset() + swing * diurnal(hour)
    }

    /// The wind over open, level ground, m/s: the climate's mean scaled by
    /// the `0..1` drift, plus the gusts of a storm.
    pub fn wind_ms(&self) -> f32 {
        let gust = if self.spell == Spell::Storm { STORM_GUST_MS } else { 0.0 };
        self.profile().mean_wind_ms * (0.15 + 1.9 * self.wind) + gust
    }

    pub fn label(&self) -> &'static str {
        match self.spell {
            Spell::Storm => return "storm",
            Spell::HeatWave => return "heat wave",
            Spell::ColdSnap => return "cold snap",
            Spell::Normal => {}
        }
        match (self.cloud, self.wind) {
            (c, _) if c > 0.75 => "overcast",
            (c, w) if c > 0.45 && w > 0.6 => "blustery",
            (c, _) if c > 0.45 => "cloudy",
            (_, w) if w > 0.7 => "windy",
            _ => "clear",
        }
    }
}

// ── the resources on the map ────────────────────────────────────────────────

/// How far around a point the wind field looks for higher and lower ground.
const EXPOSURE_RING: f32 = 240.0;

/// The wind at `p` against open, level ground, `0.35..1.8`: windier on
/// hills and ridges (above the ground around), on high ground, by the
/// shores of wide water (a long fetch); sheltered in valleys and hollows;
/// plus a broad, gentle pattern from the seed so even a flat map has its
/// windier and calmer stretches. Static: the same for a map forever.
pub fn wind_site(land: &Landscape, p: Vec2) -> f32 {
    let seed = land.params.seed;
    let h = land.height(p);
    let ring = (0..8).map(|k| land.height(p + Vec2::from_angle(k as f32 * TAU / 8.0) * EXPOSURE_RING)).sum::<f32>() / 8.0;
    // Up on a rise or down in a hollow, against the ground around.
    let exposure = ((h - ring) / 12.0).tanh();
    // High ground catches the stronger wind aloft.
    let altitude = (h.max(0.0) / 90.0).min(1.0);
    // Open water: nothing in the wind's way. Rivers and lakes count less
    // than wide water simply by being narrow (the raster's distance).
    let wd = land.water_distance(p);
    let water = if wd <= 0.0 { 1.0 } else { (1.0 - wd / 160.0).clamp(0.0, 1.0) };
    let broad = fbm(seed ^ 0x3f1d_57a9, p * 0.0011, 3) - 0.5;
    (1.0 + 0.32 * exposure + 0.25 * altitude + 0.22 * water + 0.75 * broad).clamp(0.35, 1.8)
}

/// The sun at `p` against the climate's open ground, `0.9..1.06`: a
/// little more on high open ground, a little less down in a valley (the
/// sun comes later over the ridge and goes earlier).
pub fn solar_site(land: &Landscape, p: Vec2) -> f32 {
    let h = land.height(p);
    let ring = (0..6).map(|k| land.height(p + Vec2::from_angle(k as f32 * TAU / 6.0) * EXPOSURE_RING)).sum::<f32>() / 6.0;
    let exposure = ((h - ring) / 15.0).tanh();
    (1.0 + 0.06 * exposure).clamp(0.9, 1.06)
}

/// The mean wind at `p` on an ordinary day, m/s (what the wind layer shows).
pub fn mean_wind_ms_at(land: &Landscape, p: Vec2) -> f32 {
    let ordinary = Weather { wind: 0.45, ..Weather::for_climate(land.params.climate) };
    ordinary.wind_ms() * wind_site(land, p)
}

/// A typical wind farm's share of nameplate at `p` on an ordinary day.
pub fn wind_yield_at(land: &Landscape, p: Vec2) -> f32 {
    turbine_curve(mean_wind_ms_at(land, p))
}

/// The sun at `p` with its climate, `0..~1.1` (1: the desert's open ground).
pub fn sun_at(land: &Landscape, p: Vec2) -> f32 {
    profile(land.params.climate).sun * solar_site(land, p)
}

/// What a wind or solar plant at a lot reads off the map: its climate and
/// the site factors at the lot's centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SiteResource {
    pub wind: f32,
    pub sun: f32,
}

impl Default for SiteResource {
    fn default() -> Self {
        Self { wind: 1.0, sun: 1.0 }
    }
}

impl SiteResource {
    pub fn at(land: &Landscape, p: Vec2) -> Self {
        Self { wind: wind_site(land, p), sun: solar_site(land, p) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::landscape::{LandscapeParams, LandscapePreset};
    use rand::SeedableRng;

    fn land(preset: LandscapePreset, seed: u64, climate: Climate) -> Landscape {
        Landscape::new(&LandscapeParams { climate, ..LandscapeParams::preset(preset, seed) })
    }

    #[test]
    fn the_wind_field_is_fixed_varies_over_the_map_and_follows_the_ground() {
        let l = land(LandscapePreset::RiverValley, 7, Climate::Temperate);
        let again = land(LandscapePreset::RiverValley, 7, Climate::Temperate);
        let pts: Vec<Vec2> = (0..15).flat_map(|i| (0..15).map(move |j| Vec2::new(-1400.0 + i as f32 * 200.0, -1400.0 + j as f32 * 200.0))).collect();
        let a: Vec<f32> = pts.iter().map(|p| wind_site(&l, *p)).collect();
        let b: Vec<f32> = pts.iter().map(|p| wind_site(&again, *p)).collect();
        assert_eq!(a, b, "the same map has the same wind");
        let (lo, hi) = a.iter().fold((f32::MAX, f32::MIN), |(lo, hi), v| (lo.min(*v), hi.max(*v)));
        assert!(hi - lo > 0.4, "windier and calmer places: {lo}..{hi}");
        assert!(a.iter().all(|v| (0.35..=1.8).contains(v)));
        let mean = a.iter().sum::<f32>() / a.len() as f32;
        assert!((0.8..1.35).contains(&mean), "about open ground on the whole: {mean}");
        // A different seed is a different pattern.
        let other = land(LandscapePreset::RiverValley, 8, Climate::Temperate);
        assert!(pts.iter().any(|p| (wind_site(&other, *p) - wind_site(&l, *p)).abs() > 0.05));
        // The outer hills are windier than the basin floor on the whole.
        let flat = land(LandscapePreset::Flat, 7, Climate::Temperate);
        let ring = |r: f32| (0..16).map(|k| wind_site(&flat, Vec2::from_angle(k as f32 * TAU / 16.0) * r)).sum::<f32>() / 16.0;
        assert!(ring(1650.0) > ring(300.0), "hills {} vs basin {}", ring(1650.0), ring(300.0));
    }

    #[test]
    fn climates_scale_the_wind_and_the_sun() {
        let p = Vec2::new(150.0, -80.0);
        let at = |c| land(LandscapePreset::Flat, 3, c);
        let calm = mean_wind_ms_at(&at(Climate::Tropical), p);
        let windy = mean_wind_ms_at(&at(Climate::Volcanic), p);
        assert!(windy > calm * 1.4, "Iceland-style maps blow harder: {windy} vs {calm}");
        assert!(wind_yield_at(&at(Climate::Volcanic), p) > wind_yield_at(&at(Climate::Tropical), p));
        assert!(sun_at(&at(Climate::Tropical), p) > sun_at(&at(Climate::Temperate), p));
        assert!(sun_at(&at(Climate::Desert), p) > 1.5 * sun_at(&at(Climate::Arctic), p));
        // The site factor alone does not depend on the climate.
        assert_eq!(wind_site(&at(Climate::Arctic), p), wind_site(&at(Climate::Desert), p));
    }

    #[test]
    fn turbines_cut_in_reach_rated_and_cut_out_in_a_storm() {
        assert_eq!(turbine_curve(2.0), 0.0);
        assert!(turbine_curve(7.0) > 0.3 && turbine_curve(7.0) < 0.7);
        assert_eq!(turbine_curve(RATED_MS), 1.0);
        assert_eq!(turbine_curve(20.0), 1.0);
        assert_eq!(turbine_curve(CUT_OUT_MS + 0.1), 0.0, "braked above cut-out");
        let mut w = Weather::for_climate(Climate::Volcanic);
        w.spell = Spell::Storm;
        w.wind = 0.95;
        let gale = w.wind_ms();
        assert!(gale * 1.2 > CUT_OUT_MS, "an exposed site cuts out in a storm: {gale} m/s");
        assert!(gale * 0.8 < CUT_OUT_MS, "a sheltered one keeps running: {gale} m/s");
        assert_eq!(turbine_curve(gale * 1.2), 0.0);
        assert_eq!(turbine_curve(gale * 0.8), 1.0);
    }

    #[test]
    fn the_weather_keeps_to_its_climate_and_brings_spells() {
        let run = |c: Climate| {
            let mut rng = rand_chacha::ChaCha8Rng::seed_from_u64(5);
            let mut w = Weather::for_climate(c);
            let (mut temp, mut cloud, mut wind, mut n) = (0.0, 0.0, 0.0, 0.0);
            let mut spells = std::collections::BTreeMap::new();
            for h in 0..24 * 400u64 {
                w.step(&mut rng, c, h, 99);
                temp += w.temp_c;
                cloud += w.cloud;
                wind += w.wind;
                n += 1.0;
                *spells.entry(w.spell.label()).or_insert(0u32) += 1;
            }
            (temp / n, cloud / n, wind / n, spells)
        };
        let desert = run(Climate::Desert);
        let arctic = run(Climate::Arctic);
        let tropical = run(Climate::Tropical);
        let temperate = run(Climate::Temperate);
        assert!(desert.0 > 20.0 && arctic.0 < 0.0, "desert {} arctic {}", desert.0, arctic.0);
        assert!(desert.1 < temperate.1 && desert.1 < 0.3, "desert skies are clear: {}", desert.1);
        assert!(tropical.1 > desert.1);
        assert!((0.3..0.65).contains(&desert.2), "ordinary wind about 0.45: {}", desert.2);
        assert!(desert.3.contains_key("heat wave") && !desert.3.contains_key("cold snap"));
        assert!(arctic.3.contains_key("cold snap") && !arctic.3.contains_key("heat wave"));
        assert!(temperate.3.contains_key("storm"));
        assert!(desert.3["normal"] > 24 * 250, "spells are the exception");
        // The afternoon is hotter than the dawn.
        let w = Weather::for_climate(Climate::Desert);
        assert!(w.temp_at(15.0) > w.temp_at(5.0) + 10.0);
        assert!((diurnal(15.0) - 1.0).abs() < 1e-4 && (diurnal(5.0) + 1.0).abs() < 1e-4);
        assert!(panel_derate(44.0) < 0.95 && panel_derate(20.0) == 1.0);
    }

    #[test]
    fn an_old_save_weather_loads() {
        let w: Weather = ron::from_str("(cloud: 0.4, wind: 0.6)").expect("old weather loads");
        assert_eq!(w.spell, Spell::Normal);
        assert_eq!(w.climate, Climate::Desert);
        assert!(w.temp_c > 10.0);
    }
}
