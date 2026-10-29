//! The land survey: map layers that show what the ground is good for, so
//! the player can see where farms, wells, a geothermal plant, docks or each
//! kind of industry would do well before zoning anything.
//!
//! Two families of layer, both read from the sim's own figures so the map
//! cannot promise what the sim will not do:
//!
//! - **Resources**: the static ground ([`crate::ground::sample`]: fertility,
//!   oil, rock, sand, closeness to water), the geothermal hot spots a
//!   geothermal plant needs ([`crate::landscape::Landscape::hot_spot_at`]),
//!   and water deep enough for a shipping port (the port's own width rule,
//!   [`crate::ports::PortKind::min_width`]).
//! - **Industry suitability**: for each [`IndustryKind`], the chance a new
//!   industrial lot at that spot takes the kind, from the very weights the
//!   sim draws from ([`crate::industry::weights`] over an
//!   [`crate::industry::IndustrySite`] built the same way as for a lot, see
//!   `World::survey_industry_weights`). Plus "best industry", the kind the
//!   ground favours most at each spot.
//!
//! [`LandSurvey`] caches the static part on a grid (it only changes with the
//! landscape); the World side (`world/survey.rs`) adds what moves: the
//! contamination and the industry weights. Nothing here touches the RNG.

use crate::ground::{self, GroundSample};
use crate::industry::IndustryKind;
use crate::landscape::{Landscape, WaterKind};
use crate::ports::{PortKind, SHORE_REACH};
use glam::Vec2;

/// One map layer of the land survey.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LandLayer {
    /// Soil for fields: fertility, less where the ground is contaminated.
    Farmland,
    /// Oil under the ground.
    Oil,
    /// Hot ground a geothermal plant can tap.
    Geothermal,
    /// Rock and steep ground (no quarrying yet: it spoils farming).
    Rock,
    /// Beach sand by the sea.
    Sand,
    /// Closeness to open water: fishing and ferries.
    Water,
    /// Shore where a shipping port could go: the sea or a river wide enough.
    Shipping,
    /// [grid-weather] What a wind farm would make on an ordinary day: the
    /// map's wind field times the climate's mean wind, through the
    /// turbines' power curve.
    Wind,
    /// [grid-weather] The sun a solar field would get: the climate's, a
    /// little more on high open ground.
    Solar,
    /// The chance a new industrial lot here takes this kind.
    Industry(IndustryKind),
    /// The kind of industry the ground favours most, coloured by kind.
    BestIndustry,
    /// [dirty-industry] What industry has done to the ground: the saved
    /// contamination (`crate::ground::Contamination`), not a resource but
    /// shown with them.
    Contamination,
}

impl LandLayer {
    /// The resource layers, in picker order.
    pub const RESOURCES: [LandLayer; 10] = [LandLayer::Farmland, LandLayer::Oil, LandLayer::Geothermal, LandLayer::Rock, LandLayer::Sand, LandLayer::Water, LandLayer::Shipping, LandLayer::Wind, LandLayer::Solar, LandLayer::Contamination];

    /// Every layer, in the order the K key steps through them.
    pub const ALL: [LandLayer; 21] = [
        LandLayer::Farmland,
        LandLayer::Oil,
        LandLayer::Geothermal,
        LandLayer::Rock,
        LandLayer::Sand,
        LandLayer::Water,
        LandLayer::Shipping,
        LandLayer::Wind,
        LandLayer::Solar,
        LandLayer::Contamination,
        LandLayer::BestIndustry,
        LandLayer::Industry(IndustryKind::Farming),
        LandLayer::Industry(IndustryKind::OilExtraction),
        LandLayer::Industry(IndustryKind::Refinery),
        LandLayer::Industry(IndustryKind::Manufacturing),
        LandLayer::Industry(IndustryKind::Logistics),
        LandLayer::Industry(IndustryKind::Fishing),
        LandLayer::Industry(IndustryKind::Tech),
        LandLayer::Industry(IndustryKind::Mining),
        LandLayer::Industry(IndustryKind::Smelting),
        LandLayer::Industry(IndustryKind::FactoryFarming),
    ];

    /// Name for the API (`overlay {layer}`) and settings.
    pub fn key(self) -> &'static str {
        match self {
            LandLayer::Farmland => "farmland",
            LandLayer::Oil => "oil",
            LandLayer::Geothermal => "geothermal",
            LandLayer::Rock => "rock",
            LandLayer::Sand => "sand",
            LandLayer::Water => "water",
            LandLayer::Shipping => "shipping",
            LandLayer::Wind => "wind",
            LandLayer::Solar => "solar",
            LandLayer::BestIndustry => "best_industry",
            LandLayer::Contamination => "contamination",
            LandLayer::Industry(k) => match k {
                IndustryKind::Farming => "best_farming",
                IndustryKind::OilExtraction => "best_oil",
                IndustryKind::Refinery => "best_refinery",
                IndustryKind::Manufacturing => "best_manufacturing",
                IndustryKind::Logistics => "best_logistics",
                IndustryKind::Fishing => "best_fishing",
                IndustryKind::Tech => "best_tech",
                IndustryKind::Mining => "best_mining",
                IndustryKind::Smelting => "best_smelting",
                IndustryKind::FactoryFarming => "best_factory_farming",
            },
        }
    }

    pub fn from_key(s: &str) -> Option<LandLayer> {
        let s = s.trim().to_ascii_lowercase();
        let s = s.strip_prefix("land_").unwrap_or(&s);
        if let Some(l) = LandLayer::ALL.iter().find(|l| l.key() == s) {
            return Some(*l);
        }
        match s {
            "fertility" | "farming_soil" | "soil" => Some(LandLayer::Farmland),
            "hot_spots" | "hotspots" | "geo" | "geo_thermal" => Some(LandLayer::Geothermal),
            "quarry" => Some(LandLayer::Rock),
            "fishing" | "ferry" => Some(LandLayer::Water),
            "port" | "ports" | "docks" => Some(LandLayer::Shipping),
            "wind_resource" | "winds" | "wind_map" => Some(LandLayer::Wind),
            "sun" | "solar_resource" | "sunshine" => Some(LandLayer::Solar),
            "industry" | "best" => Some(LandLayer::BestIndustry),
            "toxic" | "toxicity" | "pollution_ground" | "soil_contamination" | "poison" => Some(LandLayer::Contamination),
            _ => s.strip_prefix("best_").and_then(IndustryKind::from_key).map(LandLayer::Industry),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LandLayer::Farmland => "Farmland (soil)",
            LandLayer::Oil => "Oil deposits",
            LandLayer::Geothermal => "Geothermal hot spots",
            LandLayer::Rock => "Rock and steep ground",
            LandLayer::Sand => "Beach sand",
            LandLayer::Water => "Water access",
            LandLayer::Shipping => "Deep water (shipping)",
            LandLayer::Wind => "Wind resource",
            LandLayer::Solar => "Solar resource",
            LandLayer::BestIndustry => "Best industry here",
            LandLayer::Contamination => "Contamination",
            LandLayer::Industry(k) => match k {
                IndustryKind::Farming => "Best for farming",
                IndustryKind::OilExtraction => "Best for oil wells",
                IndustryKind::Refinery => "Best for refineries",
                IndustryKind::Manufacturing => "Best for manufacturing",
                IndustryKind::Logistics => "Best for logistics",
                IndustryKind::Fishing => "Best for fishing",
                IndustryKind::Tech => "Best for clean tech",
                IndustryKind::Mining => "Best for strip mining",
                IndustryKind::Smelting => "Best for smelting",
                IndustryKind::FactoryFarming => "Best for factory farming",
            },
        }
    }

    /// One line for the legend: what the layer shows and who wants it.
    pub fn hint(self) -> &'static str {
        match self {
            LandLayer::Farmland => "deep soil by rivers and on the plains; farms yield by it, contamination spoils it",
            LandLayer::Oil => "oil wells only go over a deposit; refineries follow the wells",
            LandLayer::Geothermal => "steaming vents: a geothermal plant needs a lot over one (solid = workable)",
            LandLayer::Rock => "thin soil and steep slopes: poor farming; strip mines dig where the rock is near the surface",
            LandLayer::Sand => "beach and dune on the sea shore: poor farming",
            LandLayer::Water => "fishing wants the shore; ferries need water 16 m wide",
            LandLayer::Shipping => "a shipping port fits on the sea or a river 40 m wide; logistics and refineries follow ports",
            LandLayer::Wind => "what a wind farm makes on an ordinary day: windy on hills, ridges and by wide water, sheltered in valleys; storms shut exposed farms down",
            LandLayer::Solar => "the sun a solar field gets: set by the climate, a little more on high open ground",
            LandLayer::BestIndustry => "the kind a new industrial lot here is most likely to take",
            LandLayer::Contamination => "poisoned ground round mines, oil wells, refineries, smelters, chemical works, factory farms and coal plants: spoils crops, costs land value, makes residents ill; fades over months, faster under parks",
            LandLayer::Industry(k) => match k {
                IndustryKind::Farming => "chance a new industrial lot farms: good soil, clean and cheap ground",
                IndustryKind::OilExtraction => "chance of oil wells: over a deposit, on cheap land",
                IndustryKind::Refinery => "chance of a refinery: oil wells in reach or a shipping port",
                IndustryKind::Manufacturing => "chance of workshops and fabs: anywhere the ground offers nothing better",
                IndustryKind::Logistics => "chance of container yards and truck lots: near ports, freight yards, highways, depots",
                IndustryKind::Fishing => "chance of fisheries: on the shore, clean water, a port helps",
                IndustryKind::Tech => "chance of clean tech: dear land, away from heavy industry",
                IndustryKind::Mining => "chance of a strip mine: rock near the surface, on cheap land",
                IndustryKind::Smelting => "chance of a smelter: strip mines in reach or a shipping port for ore, on cheap land",
                IndustryKind::FactoryFarming => "chance of a factory farm: cheap open country, away from homes that would object",
            },
        }
    }

    /// Whether the layer reads the industry weights (heavier to build, and
    /// changes as the city grows) rather than the ground alone.
    pub fn is_industry(self) -> bool {
        matches!(self, LandLayer::Industry(_) | LandLayer::BestIndustry)
    }
}

/// Heat of the geothermal ground at `p`, `0..1`: a hot spot's full heat
/// where a geothermal plant would work (the same reach as
/// [`Landscape::hot_spot_at`]), fading to nothing over the next 60 m so the
/// vents read on the map as a halo.
pub fn geothermal_at(land: &Landscape, p: Vec2) -> f32 {
    let mut best = 0.0f32;
    for h in &land.hot_spots {
        let d = h.pos.distance(p);
        let reach = h.radius + 25.0;
        let v = if d <= reach { h.heat } else { h.heat * 0.45 * (1.0 - ((d - reach) / 60.0).clamp(0.0, 1.0)) };
        best = best.max(v);
    }
    best
}

/// Closeness to water a shipping port can use, `0..1`: 1 at the shore of
/// the sea or a river at least [`PortKind::min_width`] wide, 0 beyond the
/// port's reach of the shore. Lakes and streams never count (ships cannot
/// reach them).
pub fn shipping_at(land: &Landscape, p: Vec2) -> f32 {
    if land.is_flat() || land.water_distance(p) > SHORE_REACH {
        return 0.0;
    }
    let Some(s) = land.nearest_shore(p, SHORE_REACH) else { return 0.0 };
    let ok = match s.water.kind {
        WaterKind::Sea => true,
        WaterKind::River => s.water.width >= PortKind::ShippingPort.min_width(),
        WaterKind::Lake | WaterKind::Stream => false,
    };
    if !ok {
        return 0.0;
    }
    1.0 - ((s.distance - 25.0) / (SHORE_REACH - 25.0)).clamp(0.0, 1.0)
}

/// The static part of the survey on a grid of corners over the land: the
/// ground, the geothermal heat and the shipping reach at every corner. It
/// depends only on the landscape, so build it once per map.
#[derive(Clone, Debug)]
pub struct LandSurvey {
    pub origin: Vec2,
    pub cell: f32,
    /// Cells across and down; there is one more corner each way.
    pub cols: usize,
    pub rows: usize,
    pub ground: Vec<GroundSample>,
    pub heat: Vec<f32>,
    pub shipping: Vec<f32>,
    /// [grid-weather] The wind and solar layers' values (`wind_value`,
    /// `solar_value`).
    pub wind: Vec<f32>,
    pub solar: Vec<f32>,
    /// Corners in the water (nothing is built there; the layers stay clear).
    pub wet: Vec<bool>,
}

impl LandSurvey {
    /// Survey `lo..hi` with cells of at least `cell` metres, grown so there
    /// are at most `max_cells`.
    pub fn new(land: &Landscape, lo: Vec2, hi: Vec2, cell: f32, max_cells: usize) -> Self {
        let size = (hi - lo).max(Vec2::splat(1.0));
        let mut cell = cell.max(1.0);
        while ((size.x / cell).ceil() * (size.y / cell).ceil()) as usize > max_cells.max(1) {
            cell *= 1.25;
        }
        let cols = (size.x / cell).ceil() as usize;
        let rows = (size.y / cell).ceil() as usize;
        let n = (cols + 1) * (rows + 1);
        let mut s = Self { origin: lo, cell, cols, rows, ground: Vec::with_capacity(n), heat: Vec::with_capacity(n), shipping: Vec::with_capacity(n), wind: Vec::with_capacity(n), solar: Vec::with_capacity(n), wet: Vec::with_capacity(n) };
        for j in 0..=rows {
            for i in 0..=cols {
                let p = s.corner(i, j);
                let wet = land.is_water(p);
                s.wet.push(wet);
                s.ground.push(if wet { GroundSample::default() } else { ground::sample(land, p) });
                s.heat.push(if wet { 0.0 } else { geothermal_at(land, p) });
                s.shipping.push(if wet { 0.0 } else { shipping_at(land, p) });
                s.wind.push(if wet { 0.0 } else { wind_value(land, p) });
                s.solar.push(if wet { 0.0 } else { solar_value(land, p) });
            }
        }
        s
    }

    /// The square `-half..half` both ways (clamped to the land), in cells of
    /// 16 m or a little more: fine enough that a hot spot or a river bank
    /// reads, coarse enough to build in a blink.
    pub fn square(land: &Landscape, half: f32) -> Self {
        let h = half.min(land.land_half());
        Self::new(land, Vec2::splat(-h), Vec2::splat(h), 16.0, 24_000)
    }

    pub fn corner(&self, i: usize, j: usize) -> Vec2 {
        self.origin + Vec2::new(i as f32, j as f32) * self.cell
    }

    pub fn len(&self) -> usize {
        (self.cols + 1) * (self.rows + 1)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every corner's position, row by row (the order of the value vectors).
    pub fn corners(&self) -> impl Iterator<Item = Vec2> + '_ {
        (0..=self.rows).flat_map(move |j| (0..=self.cols).map(move |i| self.corner(i, j)))
    }
}

/// [grid-weather] The wind layer at `p`, `0..1`: a typical wind farm's
/// share of nameplate there on an ordinary day.
pub fn wind_value(land: &Landscape, p: Vec2) -> f32 {
    crate::weather::wind_yield_at(land, p)
}

/// [grid-weather] The solar layer at `p`, `0..1`: the sun with its climate
/// (1 for the desert's open ground).
pub fn solar_value(land: &Landscape, p: Vec2) -> f32 {
    crate::weather::sun_at(land, p).clamp(0.0, 1.0)
}

/// The figures at a point the resource layers read beyond the ground:
/// [`geothermal_at`], [`shipping_at`], [`wind_value`], [`solar_value`] and
/// the contamination.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointResources {
    pub heat: f32,
    pub shipping: f32,
    pub wind: f32,
    pub solar: f32,
    pub contamination: f32,
}

/// A resource layer's value from the ground at a point, `0..1`, given the
/// other figures there (`r`). Industry layers are not resource layers and
/// read 0 here.
pub fn resource_value(layer: LandLayer, g: &GroundSample, r: PointResources) -> f32 {
    let PointResources { heat, shipping, wind, solar, contamination } = r;
    let clean = (1.0 - contamination).clamp(0.0, 1.0);
    match layer {
        // Farms yield by fertility and lose it to contamination, the same
        // two figures the farming weight multiplies.
        LandLayer::Farmland => (g.fertility * clean).clamp(0.0, 1.0),
        LandLayer::Oil => g.oil,
        LandLayer::Geothermal => heat,
        LandLayer::Rock => g.rock,
        LandLayer::Sand => g.sand,
        LandLayer::Water => g.water,
        LandLayer::Shipping => shipping,
        LandLayer::Wind => wind,
        LandLayer::Solar => solar,
        LandLayer::Contamination => contamination.clamp(0.0, 1.0),
        LandLayer::Industry(_) | LandLayer::BestIndustry => 0.0,
    }
}

/// Share of each kind in a set of industry weights (they sum to 1; all
/// zero when nothing has any weight).
pub fn shares(w: &[f32; crate::industry::IndustryKind::COUNT]) -> [f32; crate::industry::IndustryKind::COUNT] {
    let total: f32 = w.iter().sum();
    if total <= 0.0 || !total.is_finite() {
        return [0.0; crate::industry::IndustryKind::COUNT];
    }
    w.map(|x| x / total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::landscape::{LandscapeParams, LandscapePreset};

    #[test]
    fn keys_round_trip_and_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for l in LandLayer::ALL {
            assert_eq!(LandLayer::from_key(l.key()), Some(l), "{}", l.key());
            assert!(seen.insert(l.key()), "duplicate key {}", l.key());
            assert!(!l.label().is_empty() && !l.hint().is_empty());
        }
        assert_eq!(LandLayer::from_key("land_oil"), Some(LandLayer::Oil));
        assert_eq!(LandLayer::from_key("hot_spots"), Some(LandLayer::Geothermal));
        assert_eq!(LandLayer::from_key("best_logistics"), Some(LandLayer::Industry(IndustryKind::Logistics)));
        assert_eq!(LandLayer::from_key("nonsense"), None);
        for k in IndustryKind::ALL {
            assert!(LandLayer::ALL.contains(&LandLayer::Industry(k)), "every industry kind has a layer");
        }
    }

    #[test]
    fn survey_finds_the_hot_spots_and_the_shipping_water() {
        let land = Landscape::generate(&LandscapeParams::preset(LandscapePreset::RiverValley, 7));
        let s = LandSurvey::new(&land, Vec2::splat(-1200.0), Vec2::splat(1200.0), 30.0, 10_000);
        assert_eq!(s.ground.len(), s.len());
        assert_eq!(s.corners().count(), s.len());
        for h in &land.hot_spots {
            assert!(geothermal_at(&land, h.pos) >= h.heat - 1e-6, "full heat over the vent");
            assert!(land.hot_spot_at(h.pos).is_some());
            let far = h.pos + Vec2::new(h.radius + 200.0, 0.0);
            if land.hot_spots.iter().all(|o| o.pos.distance(far) > o.radius + 100.0) {
                assert_eq!(geothermal_at(&land, far), 0.0, "cold away from the vents");
            }
        }
        // Wherever the layer says "workable", a geothermal plant is allowed.
        for p in s.corners() {
            let h = geothermal_at(&land, p);
            if h > 0.0 && land.hot_spot_at(p).is_none() {
                assert!(h < 0.5, "the halo outside a plant's reach stays faint: {h}");
            }
        }
        let ship = s.shipping.iter().filter(|v| **v > 0.5).count();
        let wet = s.wet.iter().filter(|w| **w).count();
        if land.rivers.iter().any(|r| r.width >= 40.0) || land.sea.is_some() {
            assert!(ship > 0, "a valley with a wide river has somewhere to dock");
        }
        assert!(wet > 0, "a river valley has water");
        for (k, g) in s.ground.iter().enumerate() {
            if s.wet[k] {
                assert_eq!(s.shipping[k], 0.0);
                assert_eq!(g.fertility, 0.0);
            }
        }
    }

    #[test]
    fn resource_values_follow_the_ground() {
        let g = GroundSample { fertility: 0.8, oil: 0.3, rock: 0.1, sand: 0.0, water: 0.5 };
        let r = |heat, shipping, contamination| PointResources { heat, shipping, contamination, ..Default::default() };
        assert!((resource_value(LandLayer::Farmland, &g, r(0.0, 0.0, 0.0)) - 0.8).abs() < 1e-6);
        assert!(resource_value(LandLayer::Farmland, &g, r(0.0, 0.0, 0.5)) < 0.5, "contamination spoils farmland");
        assert_eq!(resource_value(LandLayer::Oil, &g, r(0.0, 0.0, 0.0)), 0.3);
        assert_eq!(resource_value(LandLayer::Geothermal, &g, r(0.9, 0.0, 0.0)), 0.9);
        assert_eq!(resource_value(LandLayer::Shipping, &g, r(0.0, 0.7, 0.0)), 0.7);
        assert_eq!(resource_value(LandLayer::BestIndustry, &g, r(1.0, 1.0, 0.0)), 0.0);
        assert_eq!(resource_value(LandLayer::Wind, &g, PointResources { wind: 0.6, ..Default::default() }), 0.6);
        assert_eq!(resource_value(LandLayer::Solar, &g, PointResources { solar: 0.9, ..Default::default() }), 0.9);
        let sh = shares(&[1.0, 3.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert!((sh[1] - 0.75).abs() < 1e-6 && (sh.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        assert_eq!(shares(&[0.0; crate::industry::IndustryKind::COUNT]), [0.0; crate::industry::IndustryKind::COUNT]);
    }
}
