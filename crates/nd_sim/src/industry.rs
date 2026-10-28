//! Industry kinds (the sub-zones of industrial land) and how a lot picks
//! one: the ground (fertile soil, oil, water), the neighbours (same kinds
//! cluster, farms keep away from oil and chemicals, logistics gathers at
//! ports, yards, depots and the highway), land value, and the player's
//! bias for the area, which overrides the rest.
//!
//! Every industrial business belongs to one kind; the kind decides its
//! nuisance on land value, whether it fouls the ground, and its look.

use crate::ground::GroundSample;
use crate::market::BusinessKind;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum IndustryKind {
    /// Fields, greenhouses, vats, dairies and mills.
    Farming,
    /// Pumpjacks and derricks over an oil deposit. Fouls the ground.
    OilExtraction,
    /// Refineries and chemical works.
    Refinery,
    /// Parts fabs and workshops.
    Manufacturing,
    /// Container storage and loading, truck and bus lots.
    Logistics,
    /// Fishing boats and fish processing.
    Fishing,
    /// Clean industry: electronics assembly.
    Tech,
    /// [economy] Strip mining over rocky ground: an open pit, ore for
    /// export, dust, noise and a heavy land value hit. Appended.
    Mining,
    /// [dirty-industry] Smelters and foundries: ore in, metal out; slag
    /// heaps, soot and heavy metals in the ground. Appended.
    Smelting,
    /// [dirty-industry] Factory farming: feedlots and battery sheds, grain
    /// in, livestock out by the thousand; manure lagoons and the smell.
    /// Still food. Appended.
    FactoryFarming,
}

impl IndustryKind {
    pub const COUNT: usize = 10;
    pub const ALL: [IndustryKind; Self::COUNT] = [
        IndustryKind::Farming,
        IndustryKind::OilExtraction,
        IndustryKind::Refinery,
        IndustryKind::Manufacturing,
        IndustryKind::Logistics,
        IndustryKind::Fishing,
        IndustryKind::Tech,
        IndustryKind::Mining,
        IndustryKind::Smelting,
        IndustryKind::FactoryFarming,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            IndustryKind::Farming => "farming",
            IndustryKind::OilExtraction => "oil extraction",
            IndustryKind::Refinery => "refinery and chemicals",
            IndustryKind::Manufacturing => "manufacturing",
            IndustryKind::Logistics => "logistics",
            IndustryKind::Fishing => "fishing",
            IndustryKind::Tech => "clean tech",
            IndustryKind::Mining => "strip mining",
            IndustryKind::Smelting => "smelting",
            IndustryKind::FactoryFarming => "factory farming",
        }
    }

    /// Short key for the API and saves of settings.
    pub fn key(self) -> &'static str {
        match self {
            IndustryKind::Farming => "farming",
            IndustryKind::OilExtraction => "oil",
            IndustryKind::Refinery => "refinery",
            IndustryKind::Manufacturing => "manufacturing",
            IndustryKind::Logistics => "logistics",
            IndustryKind::Fishing => "fishing",
            IndustryKind::Tech => "tech",
            IndustryKind::Mining => "mining",
            IndustryKind::Smelting => "smelting",
            IndustryKind::FactoryFarming => "factory_farming",
        }
    }

    pub fn from_key(s: &str) -> Option<IndustryKind> {
        let s = s.trim().to_ascii_lowercase();
        Some(match s.as_str() {
            "farming" | "farm" | "agriculture" => IndustryKind::Farming,
            "oil" | "oil_extraction" | "extraction" | "oilextraction" => IndustryKind::OilExtraction,
            "refinery" | "chemical" | "chemicals" => IndustryKind::Refinery,
            "manufacturing" | "fab" | "factory" => IndustryKind::Manufacturing,
            "logistics" | "shipping" | "containers" => IndustryKind::Logistics,
            "fishing" | "fish" => IndustryKind::Fishing,
            "tech" | "clean" | "clean_tech" => IndustryKind::Tech,
            "mining" | "mine" | "strip_mining" | "quarry" => IndustryKind::Mining,
            "smelting" | "smelter" | "foundry" | "metal" | "metals" => IndustryKind::Smelting,
            "factory_farming" | "factoryfarming" | "factory_farm" | "feedlot" | "intensive" | "livestock" => IndustryKind::FactoryFarming,
            _ => return None,
        })
    }

    /// The businesses of this kind.
    pub fn businesses(self) -> &'static [BusinessKind] {
        match self {
            IndustryKind::Farming => &[BusinessKind::VertFarm, BusinessKind::GrainFarm, BusinessKind::ProteinVat, BusinessKind::Dairy, BusinessKind::Mill, BusinessKind::CowFarm, BusinessKind::ChickenFarm, BusinessKind::Slaughterhouse],
            IndustryKind::OilExtraction => &[BusinessKind::OilWell],
            IndustryKind::Refinery => &[BusinessKind::Refinery, BusinessKind::ChemicalWorks],
            IndustryKind::Manufacturing => &[BusinessKind::Fab],
            IndustryKind::Logistics => &[BusinessKind::ContainerYard, BusinessKind::FleetLot, BusinessKind::TowYard, BusinessKind::TrashCompany],
            IndustryKind::Fishing => &[BusinessKind::Fishery, BusinessKind::FishPlant, BusinessKind::FishGutting, BusinessKind::FishStickFactory],
            IndustryKind::Tech => &[BusinessKind::Assembly],
            IndustryKind::Mining => &[BusinessKind::StripMine],
            IndustryKind::Smelting => &[BusinessKind::Smelter],
            IndustryKind::FactoryFarming => &[BusinessKind::Feedlot],
        }
    }

    /// The kind a business belongs to; `None` for shops, offices and the rest.
    pub fn of_business(kind: BusinessKind) -> Option<IndustryKind> {
        IndustryKind::ALL.into_iter().find(|k| k.businesses().contains(&kind))
    }

    /// Nuisance on the land around it: (radius, weight). Homes feel all of
    /// it, shops most of it, other industry a little (see
    /// [`crate::land_value::LandValue::for_use`]).
    pub fn nuisance(self) -> (f32, f32) {
        match self {
            IndustryKind::Farming => (150.0, 0.08),
            IndustryKind::OilExtraction => (260.0, 0.32),
            IndustryKind::Refinery => (300.0, 0.40),
            IndustryKind::Manufacturing => (220.0, 0.30),
            IndustryKind::Logistics => (180.0, 0.18),
            IndustryKind::Fishing => (160.0, 0.14),
            IndustryKind::Tech => (120.0, 0.05),
            // Blasting, dust and a scarred hillside: the worst neighbour.
            IndustryKind::Mining => (340.0, 0.42),
            // [dirty-industry] Soot, fumes and the glow of the furnaces.
            IndustryKind::Smelting => (340.0, 0.42),
            // The smell carries.
            IndustryKind::FactoryFarming => (280.0, 0.3),
        }
    }

    /// Activity a working plant brings to run-down land: (radius, lift),
    /// applied only where the land is cheap.
    pub fn lift(self) -> (f32, f32) {
        match self {
            IndustryKind::OilExtraction | IndustryKind::Refinery => (200.0, 0.05),
            IndustryKind::Farming | IndustryKind::Fishing => (180.0, 0.05),
            IndustryKind::Manufacturing | IndustryKind::Logistics => (200.0, 0.07),
            IndustryKind::Tech => (200.0, 0.09),
            IndustryKind::Mining => (160.0, 0.03),
            IndustryKind::Smelting => (180.0, 0.04),
            IndustryKind::FactoryFarming => (150.0, 0.03),
        }
    }

    /// Contamination poured onto the ground per working hour at full
    /// staffing: (radius, amount).
    pub fn spill(self) -> Option<(f32, f32)> {
        match self {
            IndustryKind::OilExtraction => Some((60.0, 0.010)),
            IndustryKind::Refinery => Some((80.0, 0.004)),
            // Dust and run-off from the pit, tailings.
            IndustryKind::Mining => Some((120.0, 0.004)),
            // [dirty-industry] Heavy metals in the soot and the slag.
            IndustryKind::Smelting => Some((130.0, 0.006)),
            // Manure lagoons seeping nitrates.
            IndustryKind::FactoryFarming => Some((110.0, 0.004)),
            _ => None,
        }
    }

    /// [dirty-industry] How dirty a working plant of this kind is, `0..1`:
    /// what it does to the look of the place, the ground and the
    /// neighbours. Clean kinds are 0; see [`IndustryKind::is_dirty`].
    pub fn dirtiness(self) -> f32 {
        match self {
            IndustryKind::Farming | IndustryKind::Fishing | IndustryKind::Tech => 0.0,
            IndustryKind::Logistics => 0.05,
            IndustryKind::Manufacturing => 0.15,
            IndustryKind::OilExtraction => 0.55,
            IndustryKind::FactoryFarming => 0.6,
            IndustryKind::Mining => 0.75,
            IndustryKind::Refinery => 0.8,
            IndustryKind::Smelting => 0.9,
        }
    }

    /// [dirty-industry] Mining, oil, refining, smelting and factory farming:
    /// sooty looks, poisoned ground, heavy trucks and neighbours who hate it.
    pub fn is_dirty(self) -> bool {
        self.dirtiness() >= DIRTY
    }

    /// [dirty-industry] How far its neighbours feel a dirty plant, metres
    /// (zero for clean kinds).
    pub fn dirt_reach(self) -> f32 {
        dirt_reach(self.dirtiness())
    }

    /// Floors a building of this kind is built with, at most.
    pub fn max_floors(self) -> u8 {
        match self {
            IndustryKind::OilExtraction | IndustryKind::Farming | IndustryKind::Logistics | IndustryKind::Mining | IndustryKind::FactoryFarming => 1,
            IndustryKind::Fishing => 2,
            _ => 3,
        }
    }
}

/// [land-assembly] A strip mine opens only on a plot at least this big
/// (m², a little over a usual works yard): its pit then grows over the
/// land behind it.
pub const STRIP_MINE_MIN_AREA: f32 = 1500.0;

/// The player's industry sub-zone over an area: newer entries win. `kind:
/// None` hands the area back to the ground over an older bias.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct IndustryBias {
    pub polygon: Vec<glam::Vec2>,
    pub kind: Option<IndustryKind>,
}

/// Everything a lot's industry choice weighs. Proximities are `0..1`
/// (1 on the spot, 0 out of reach).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct IndustrySite {
    pub ground: GroundSample,
    pub contamination: f32,
    pub land_value: f32,
    /// A working shipping port.
    pub port: f32,
    /// A freight yard.
    pub yard: f32,
    /// A highway carriageway.
    pub highway: f32,
    /// A bus depot.
    pub depot: f32,
    /// Industry already standing within [`NEIGHBOUR_RADIUS`]: kind and distance.
    pub neighbours: Vec<(IndustryKind, f32)>,
    /// A truck and bus lot within reach already.
    pub fleet_lot_near: bool,
    /// Working oil wells within reach of a refinery here.
    pub crude_wells: u32,
    /// [economy] Cattle and chicken farms within [`LIVESTOCK_REACH`]: a
    /// slaughterhouse wants to be near them.
    pub livestock_farms: u32,
    /// [economy] Slaughterhouses already within [`LIVESTOCK_REACH`]: two
    /// farms keep one busy, no more.
    pub slaughterhouses: u32,
    /// [economy] A working fishing port (`0..1`, 1 on the quay): gutting
    /// sheds and the fish-stick factory gather round the catch.
    pub fishing_port: f32,
    /// [dirty-industry] Working strip mines within [`ORE_REACH`]: a smelter
    /// wants ore close by.
    pub ore_mines: u32,
    /// [dirty-industry] Smelters already within [`ORE_REACH`]: two to a
    /// mine, no more.
    pub smelters: u32,
    /// [dirty-industry] Dirty plants whose reach covers the site: kind and
    /// distance (see [`exposure`]). Unlike `neighbours` this reaches as far
    /// as each plant's [`IndustryKind::dirt_reach`].
    pub dirty: Vec<(IndustryKind, f32)>,
}

/// [dirty-industry] A dirty plant as its neighbours feel it: where, how
/// dirty right now (its business's dirtiness, less when idle or shut) and
/// how far that carries. `as_kind` is the kind the compatibility table
/// reads it as (a coal plant reads as a refinery).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DirtySource {
    pub building: crate::buildings::BuildingId,
    pub pos: glam::Vec2,
    pub as_kind: IndustryKind,
    pub level: f32,
    pub reach: f32,
}

/// [dirty-industry] Strip mines this close feed a smelter.
pub const ORE_REACH: f32 = 1500.0;

/// [dirty-industry] A kind this dirty (see [`IndustryKind::dirtiness`])
/// counts as dirty industry.
pub const DIRTY: f32 = 0.5;

/// [dirty-industry] How far neighbours feel a plant of dirtiness `d`.
pub fn dirt_reach(d: f32) -> f32 {
    if d < DIRTY {
        0.0
    } else {
        150.0 + 250.0 * d
    }
}

/// [dirty-industry] Who is next door to dirty industry: an industry kind,
/// or a land use (homes, shops, a park) without one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Neighbour {
    Industry(IndustryKind),
    Land(crate::zoning::LandUse),
}

impl Neighbour {
    pub fn label(self) -> &'static str {
        match self {
            Neighbour::Industry(k) => k.label(),
            Neighbour::Land(lu) => lu.label(),
        }
    }
}

/// [dirty-industry] The compatibility table: how much `who` suffers from a
/// dirty plant of kind `source` next door, `0..1`. Farms, fishing, homes,
/// shops and clean tech hate it; manufacturing and logistics shrug; the
/// dirty kinds tolerate each other (a feedlot minds the chemicals a little).
pub fn sensitivity(who: Neighbour, source: IndustryKind) -> f32 {
    use crate::zoning::LandUse;
    use IndustryKind::*;
    if !source.is_dirty() {
        return 0.0;
    }
    match who {
        Neighbour::Industry(k) => match (k, source) {
            (Farming, FactoryFarming) => 0.6,
            (Farming, _) => 1.0,
            (Fishing, _) => 0.9,
            (Tech, _) => 0.8,
            (FactoryFarming, FactoryFarming) => 0.0,
            (FactoryFarming, _) => 0.3,
            (Manufacturing, _) => 0.25,
            (Logistics, _) => 0.1,
            (OilExtraction | Refinery | Mining | Smelting, _) => 0.0,
        },
        Neighbour::Land(lu) => match lu {
            LandUse::Residential => 1.0,
            LandUse::Commercial => 0.7,
            LandUse::Entertainment | LandUse::Civic => 0.6,
            LandUse::Park => 0.5,
            LandUse::Industrial => 0.25,
        },
    }
}

/// [dirty-industry] What one dirty plant of `level` (its dirtiness, scaled
/// by how hard it is working) and `reach` does to a neighbour of
/// sensitivity `sens` at distance `d`: `0..1`, falling off to nothing at
/// the reach.
pub fn hit(level: f32, reach: f32, sens: f32, d: f32) -> f32 {
    if reach <= 0.0 || d >= reach || !d.is_finite() {
        return 0.0;
    }
    (level * sens * (1.0 - d / reach).max(0.0).powf(1.5)).clamp(0.0, 1.0)
}

/// [dirty-industry] Several hits combined: each takes its share of what
/// the others left, capped at [`MAX_EXPOSURE`]. Order-independent.
pub fn combine(hits: impl IntoIterator<Item = f32>) -> f32 {
    let left: f32 = hits.into_iter().fold(1.0, |acc, h| acc * (1.0 - h.clamp(0.0, 1.0)));
    (1.0 - left).clamp(0.0, MAX_EXPOSURE)
}

/// [dirty-industry] The most a neighbour can suffer.
pub const MAX_EXPOSURE: f32 = 0.9;

/// [dirty-industry] How exposed `who` is to the dirty plants in `dirty`
/// (kind and distance, each at its kind's dirtiness and reach).
pub fn exposure(who: Neighbour, dirty: &[(IndustryKind, f32)]) -> f32 {
    combine(dirty.iter().map(|(k, d)| hit(k.dirtiness(), k.dirt_reach(), sensitivity(who, *k), *d)))
}

/// [economy] Livestock farms this close count for a slaughterhouse.
pub const LIVESTOCK_REACH: f32 = 700.0;

/// Industry further than this does not count as a neighbour.
pub const NEIGHBOUR_RADIUS: f32 = 160.0;

use crate::geom::smoothstep;

/// How much `kind` wants to stand next to `other`: positive clusters,
/// negative keeps apart.
pub fn affinity(kind: IndustryKind, other: IndustryKind) -> f32 {
    use IndustryKind::*;
    if kind == other {
        return match kind {
            Farming => 0.45,
            OilExtraction => 0.5,
            Refinery => 0.3,
            Logistics => 0.4,
            Fishing => 0.4,
            Manufacturing => 0.3,
            Tech => 0.35,
            Mining => 0.5,
            Smelting => 0.1,
            FactoryFarming => 0.2,
        };
    }
    match (kind, other) {
        // Nobody wants their crops or their catch next to an oil field.
        (Farming, OilExtraction) | (Farming, Refinery) => -1.3,
        (Fishing, OilExtraction) | (Fishing, Refinery) => -0.7,
        (Tech, OilExtraction) | (Tech, Refinery) => -0.6,
        (Farming, Manufacturing) => -0.2,
        // Refineries follow the wells; logistics follows everything heavy.
        (Refinery, OilExtraction) => 0.6,
        (OilExtraction, Refinery) => 0.3,
        (Logistics, Refinery) | (Logistics, Manufacturing) | (Logistics, Fishing) => 0.25,
        (Fishing, Logistics) => 0.2,
        (Tech, Manufacturing) | (Manufacturing, Tech) => 0.2,
        // [economy] Nobody wants a blasting pit next door but the hauliers.
        (Farming, Mining) => -1.0,
        (Fishing, Mining) | (Tech, Mining) => -0.5,
        (Logistics, Mining) => 0.2,
        (Mining, Logistics) => 0.1,
        // [dirty-industry] Smelters follow the mines; the hauliers follow both.
        (Smelting, Mining) => 0.6,
        (Mining, Smelting) => 0.3,
        (Logistics, Smelting) => 0.25,
        (Smelting, Logistics) => 0.1,
        (Farming, Smelting) | (Farming, FactoryFarming) => -1.0,
        (Fishing, Smelting) | (Tech, Smelting) | (Fishing, FactoryFarming) => -0.6,
        (FactoryFarming, Farming) => 0.15,
        _ => 0.0,
    }
}

/// Unnormalised weight of each industry kind on a site, in
/// [`IndustryKind::ALL`] order. Deterministic.
pub fn weights(site: &IndustrySite) -> [f32; IndustryKind::COUNT] {
    use IndustryKind::*;
    let g = &site.ground;
    let lv = site.land_value.clamp(0.0, 1.0);
    let clean = (1.0 - site.contamination).clamp(0.0, 1.0);
    let mut w = [0.0f32; IndustryKind::COUNT];
    // Farming wants deep soil and clean ground; it is the bread and butter
    // of new industrial land, so it starts high.
    w[Farming.index()] = 2.0 * (0.25 + 1.75 * g.fertility) * clean * clean * (1.0 - 0.6 * g.rock);
    // Only where there is oil, and then very keen.
    w[OilExtraction.index()] = 9.0 * smoothstep(0.25, 0.6, g.oil);
    // Refineries are rare without wells nearby.
    let wells_near = site.neighbours.iter().filter(|(k, _)| *k == OilExtraction).count() as f32;
    // A refinery on imported crude loses money, so none is built without
    // wells in reach or a shipping port to land crude.
    w[Refinery.index()] = if site.crude_wells == 0 && site.port <= 0.0 { 0.0 } else { 0.04 + 0.6 * g.oil + 0.4 * wells_near.min(3.0) + 0.25 * site.port };
    w[Manufacturing.index()] = 0.45;
    w[Logistics.index()] = 0.25 + 2.0 * site.port + 1.2 * site.yard + 0.8 * site.highway + 0.6 * site.depot;
    // Fishing needs the water, and a port helps.
    w[Fishing.index()] = 2.5 * g.water * (0.5 + 0.5 * clean) + 0.8 * site.port * g.water;
    // Clean industry wants dear land; heavy industry the cheap stuff.
    w[Tech.index()] = 0.35 + 1.0 * smoothstep(0.3, 0.7, lv);
    // [economy] A strip mine only where the rock is near the surface; it
    // takes the cheap land like the wells do.
    w[Mining.index()] = 2.4 * smoothstep(0.62, 0.9, g.rock);
    // [dirty-industry] A smelter only with ore to hand: mines in reach or
    // a port to land it.
    // One smelter or two per mine: imported ore costs more than the
    // ingots fetch out of town.
    w[Smelting.index()] = if site.ore_mines == 0 || site.smelters >= 2 * site.ore_mines { 0.0 } else { 0.05 + 0.5 * (site.ore_mines.min(3) as f32) + 0.2 * site.port };
    // Feedlots: cheap open country, near the grain; a sideline next to
    // ordinary farming, never the bread and butter.
    w[FactoryFarming.index()] = 0.12 + 0.3 * g.fertility * clean;
    let cheap = 1.3 - lv;
    for k in [OilExtraction, Refinery, Farming, Mining, Smelting, FactoryFarming] {
        w[k.index()] *= cheap;
    }
    // [dirty-industry] Dirty plants within their reach keep the kinds that
    // hate them away (`sensitivity`), much further out than neighbours.
    if !site.dirty.is_empty() {
        for k in IndustryKind::ALL {
            let e = exposure(Neighbour::Industry(k), &site.dirty);
            w[k.index()] *= (1.0 - e).powi(2).max(0.02);
        }
    }
    // Neighbours: exp of the summed affinities, nearer counting more.
    for k in IndustryKind::ALL {
        let mut terms: Vec<f32> = site
            .neighbours
            .iter()
            .filter(|(_, d)| d.is_finite() && *d < NEIGHBOUR_RADIUS)
            .map(|(o, d)| affinity(k, *o) * (1.0 - d / NEIGHBOUR_RADIUS).clamp(0.0, 1.0).powf(0.7))
            .collect();
        terms.sort_by(f32::total_cmp);
        let s: f32 = terms.iter().sum();
        w[k.index()] *= (s * 1.2).exp().clamp(0.02, 12.0);
    }
    for x in &mut w {
        if !x.is_finite() || *x < 0.0 {
            *x = 0.0;
        }
    }
    w
}

/// The kind with the most weight (ties to the earlier kind).
pub fn favourite(w: &[f32; IndustryKind::COUNT]) -> IndustryKind {
    let mut best = 0;
    for i in 1..IndustryKind::COUNT {
        if w[i] > w[best] {
            best = i;
        }
    }
    IndustryKind::ALL[best]
}

/// Weighted pick with `roll` in `0..1`.
pub fn pick(w: &[f32; IndustryKind::COUNT], roll: f32) -> IndustryKind {
    let total: f32 = w.iter().sum();
    if total <= 0.0 || !total.is_finite() {
        return IndustryKind::Manufacturing;
    }
    let mut r = roll.clamp(0.0, 0.9999) * total;
    for (i, x) in w.iter().enumerate() {
        if r < *x {
            return IndustryKind::ALL[i];
        }
        r -= x;
    }
    IndustryKind::Manufacturing
}

/// Weights of the businesses within a kind on a site.
pub fn business_weights(kind: IndustryKind, site: &IndustrySite) -> Vec<(BusinessKind, f32)> {
    let g = &site.ground;
    match kind {
        IndustryKind::Farming => {
            // Open fields on good soil; vats and vertical farms anywhere.
            let field = 0.3 + 1.4 * g.fertility;
            // [economy] Pasture and chicken sheds on the open land; a
            // slaughterhouse where the livestock farms are.
            let farms = site.livestock_farms.min(4) as f32;
            vec![
                (BusinessKind::GrainFarm, field),
                (BusinessKind::Dairy, field * 0.8),
                (BusinessKind::VertFarm, 0.9),
                (BusinessKind::ProteinVat, 0.8),
                (BusinessKind::Mill, 0.7),
                (BusinessKind::CowFarm, field * 0.12),
                (BusinessKind::ChickenFarm, 0.1),
                (BusinessKind::Slaughterhouse, if site.livestock_farms >= 2 * (site.slaughterhouses + 1) { 0.15 + 0.35 * farms } else { 0.0 }),
            ]
        }
        IndustryKind::Logistics => {
            let fleet = if site.fleet_lot_near { 0.15 } else { 0.5 + 1.5 * site.depot + 0.8 * site.port.max(site.yard) };
            // [parking] A tow company now and then: towing pays wherever cars are.
            vec![(BusinessKind::ContainerYard, 1.0 + 1.0 * site.port + 0.6 * site.yard + 0.4 * site.highway), (BusinessKind::FleetLot, fleet), (BusinessKind::TowYard, 0.4)]
        }
        IndustryKind::Fishing => {
            let processors = site.neighbours.iter().filter(|(k, _)| *k == IndustryKind::Fishing).count() as f32;
            vec![
                (BusinessKind::Fishery, 1.2 + g.water),
                (BusinessKind::FishPlant, 0.6 + 0.4 * processors.min(3.0)),
                // [economy] Gutting and the fish-stick factory follow a fishing port.
                (BusinessKind::FishGutting, 1.2 * site.fishing_port),
                (BusinessKind::FishStickFactory, 0.6 * site.fishing_port),
            ]
        }
        IndustryKind::Refinery => {
            // [dirty-industry] A chemical works now and then, more where
            // refineries already stand.
            let refineries = site.neighbours.iter().filter(|(k, _)| *k == IndustryKind::Refinery).count() as f32;
            vec![(BusinessKind::Refinery, 1.0), (BusinessKind::ChemicalWorks, 0.2 + 0.2 * refineries.min(2.0))]
        }
        k => k.businesses().iter().map(|b| (*b, 1.0)).collect(),
    }
}

/// Weighted business pick with `roll` in `0..1`.
pub fn pick_business(options: &[(BusinessKind, f32)], roll: f32) -> Option<BusinessKind> {
    let total: f32 = options.iter().map(|o| o.1.max(0.0)).sum();
    if options.is_empty() || total <= 0.0 {
        return options.first().map(|o| o.0);
    }
    let mut r = roll.clamp(0.0, 0.9999) * total;
    for (b, w) in options {
        let w = w.max(0.0);
        if r < w {
            return Some(*b);
        }
        r -= w;
    }
    options.last().map(|o| o.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn share(site: &IndustrySite, k: IndustryKind) -> f32 {
        let w = weights(site);
        w[k.index()] / w.iter().sum::<f32>()
    }

    #[test]
    fn every_business_belongs_to_its_kind() {
        for k in IndustryKind::ALL {
            assert_eq!(IndustryKind::from_key(k.key()), Some(k));
            for b in k.businesses() {
                assert_eq!(IndustryKind::of_business(*b), Some(k));
                assert_eq!(BusinessKind::candidates(crate::zoning::LandUse::Industrial).contains(b), true, "{b:?} grows on industrial land");
            }
        }
        assert_eq!(IndustryKind::of_business(BusinessKind::Grocery), None);
    }

    #[test]
    fn the_ground_steers_the_choice() {
        let plain = IndustrySite { land_value: 0.3, ..Default::default() };
        let fertile = IndustrySite { ground: GroundSample { fertility: 0.9, ..Default::default() }, ..plain.clone() };
        assert_eq!(favourite(&weights(&fertile)), IndustryKind::Farming);
        assert!(share(&fertile, IndustryKind::Farming) > share(&plain, IndustryKind::Farming) + 0.15);
        let oily = IndustrySite { ground: GroundSample { oil: 0.9, fertility: 0.2, ..Default::default() }, ..plain.clone() };
        assert_eq!(favourite(&weights(&oily)), IndustryKind::OilExtraction);
        assert_eq!(weights(&plain)[IndustryKind::OilExtraction.index()], 0.0, "no oil, no wells");
        let port = IndustrySite { ground: GroundSample { water: 1.0, fertility: 0.3, ..Default::default() }, port: 1.0, ..plain.clone() };
        let f = favourite(&weights(&port));
        assert!(matches!(f, IndustryKind::Logistics | IndustryKind::Fishing), "{f:?}");
        assert!(share(&port, IndustryKind::Logistics) + share(&port, IndustryKind::Fishing) > 0.6);
        assert_eq!(weights(&plain)[IndustryKind::Fishing.index()], 0.0, "no water, no fishing");
        assert_eq!(weights(&plain)[IndustryKind::Refinery.index()], 0.0, "no crude in reach, no refinery");
        let fed = IndustrySite { crude_wells: 2, ..plain.clone() };
        assert!(weights(&fed)[IndustryKind::Refinery.index()] > 0.0);
        let dear = IndustrySite { land_value: 0.85, ..plain.clone() };
        assert!(share(&dear, IndustryKind::Tech) > share(&plain, IndustryKind::Tech) * 2.0);
    }

    #[test]
    fn farms_shun_oil_and_kinds_cluster() {
        let fertile = IndustrySite { ground: GroundSample { fertility: 0.8, oil: 0.3, ..Default::default() }, land_value: 0.3, ..Default::default() };
        let by_wells = IndustrySite { neighbours: vec![(IndustryKind::OilExtraction, 30.0), (IndustryKind::OilExtraction, 70.0)], ..fertile.clone() };
        assert!(share(&by_wells, IndustryKind::Farming) < share(&fertile, IndustryKind::Farming) * 0.4, "{} vs {}", share(&by_wells, IndustryKind::Farming), share(&fertile, IndustryKind::Farming));
        let by_farms = IndustrySite { neighbours: vec![(IndustryKind::Farming, 30.0), (IndustryKind::Farming, 70.0)], ..fertile.clone() };
        assert!(share(&by_farms, IndustryKind::Farming) > share(&fertile, IndustryKind::Farming));
        let contaminated = IndustrySite { contamination: 0.8, ..fertile.clone() };
        assert!(share(&contaminated, IndustryKind::Farming) < share(&fertile, IndustryKind::Farming) * 0.3);
    }

    #[test]
    fn picks_are_deterministic_and_cover_the_weights() {
        let w = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0];
        assert_eq!(pick(&w, 0.1), IndustryKind::Farming);
        assert_eq!(pick(&w, 0.3), IndustryKind::Manufacturing);
        assert_eq!(pick(&w, 0.9), IndustryKind::Tech);
        assert_eq!(pick(&[0.0; IndustryKind::COUNT], 0.5), IndustryKind::Manufacturing);
        let near_depot = IndustrySite { depot: 1.0, ..Default::default() };
        let opts = business_weights(IndustryKind::Logistics, &near_depot);
        let fleet = opts.iter().find(|o| o.0 == BusinessKind::FleetLot).unwrap().1;
        let crowded = business_weights(IndustryKind::Logistics, &IndustrySite { fleet_lot_near: true, ..near_depot });
        assert!(crowded.iter().find(|o| o.0 == BusinessKind::FleetLot).unwrap().1 < fleet * 0.2);
    }
}
