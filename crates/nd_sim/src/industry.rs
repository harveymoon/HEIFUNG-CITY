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
}

impl IndustryKind {
    pub const ALL: [IndustryKind; 7] = [
        IndustryKind::Farming,
        IndustryKind::OilExtraction,
        IndustryKind::Refinery,
        IndustryKind::Manufacturing,
        IndustryKind::Logistics,
        IndustryKind::Fishing,
        IndustryKind::Tech,
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
            _ => return None,
        })
    }

    /// The businesses of this kind.
    pub fn businesses(self) -> &'static [BusinessKind] {
        match self {
            IndustryKind::Farming => &[BusinessKind::VertFarm, BusinessKind::GrainFarm, BusinessKind::ProteinVat, BusinessKind::Dairy, BusinessKind::Mill],
            IndustryKind::OilExtraction => &[BusinessKind::OilWell],
            IndustryKind::Refinery => &[BusinessKind::Refinery],
            IndustryKind::Manufacturing => &[BusinessKind::Fab],
            IndustryKind::Logistics => &[BusinessKind::ContainerYard, BusinessKind::FleetLot, BusinessKind::TowYard],
            IndustryKind::Fishing => &[BusinessKind::Fishery, BusinessKind::FishPlant],
            IndustryKind::Tech => &[BusinessKind::Assembly],
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
        }
    }

    /// Contamination poured onto the ground per working hour at full
    /// staffing: (radius, amount).
    pub fn spill(self) -> Option<(f32, f32)> {
        match self {
            IndustryKind::OilExtraction => Some((60.0, 0.010)),
            IndustryKind::Refinery => Some((80.0, 0.004)),
            _ => None,
        }
    }

    /// Floors a building of this kind is built with, at most.
    pub fn max_floors(self) -> u8 {
        match self {
            IndustryKind::OilExtraction | IndustryKind::Farming | IndustryKind::Logistics => 1,
            IndustryKind::Fishing => 2,
            _ => 3,
        }
    }
}

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
}

/// Industry further than this does not count as a neighbour.
pub const NEIGHBOUR_RADIUS: f32 = 160.0;

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

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
        _ => 0.0,
    }
}

/// Unnormalised weight of each industry kind on a site, in
/// [`IndustryKind::ALL`] order. Deterministic.
pub fn weights(site: &IndustrySite) -> [f32; 7] {
    use IndustryKind::*;
    let g = &site.ground;
    let lv = site.land_value.clamp(0.0, 1.0);
    let clean = (1.0 - site.contamination).clamp(0.0, 1.0);
    let mut w = [0.0f32; 7];
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
    let cheap = 1.3 - lv;
    for k in [OilExtraction, Refinery, Farming] {
        w[k.index()] *= cheap;
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
pub fn favourite(w: &[f32; 7]) -> IndustryKind {
    let mut best = 0;
    for i in 1..7 {
        if w[i] > w[best] {
            best = i;
        }
    }
    IndustryKind::ALL[best]
}

/// Weighted pick with `roll` in `0..1`.
pub fn pick(w: &[f32; 7], roll: f32) -> IndustryKind {
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
            vec![(BusinessKind::GrainFarm, field), (BusinessKind::Dairy, field * 0.8), (BusinessKind::VertFarm, 0.9), (BusinessKind::ProteinVat, 0.8), (BusinessKind::Mill, 0.7)]
        }
        IndustryKind::Logistics => {
            let fleet = if site.fleet_lot_near { 0.15 } else { 0.5 + 1.5 * site.depot + 0.8 * site.port.max(site.yard) };
            // [parking] A tow company now and then: towing pays wherever cars are.
            vec![(BusinessKind::ContainerYard, 1.0 + 1.0 * site.port + 0.6 * site.yard + 0.4 * site.highway), (BusinessKind::FleetLot, fleet), (BusinessKind::TowYard, 0.4)]
        }
        IndustryKind::Fishing => {
            let processors = site.neighbours.iter().filter(|(k, _)| *k == IndustryKind::Fishing).count() as f32;
            vec![(BusinessKind::Fishery, 1.2 + g.water), (BusinessKind::FishPlant, 0.6 + 0.4 * processors.min(3.0))]
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
        let w = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0];
        assert_eq!(pick(&w, 0.1), IndustryKind::Farming);
        assert_eq!(pick(&w, 0.3), IndustryKind::Manufacturing);
        assert_eq!(pick(&w, 0.9), IndustryKind::Tech);
        assert_eq!(pick(&[0.0; 7], 0.5), IndustryKind::Manufacturing);
        let near_depot = IndustrySite { depot: 1.0, ..Default::default() };
        let opts = business_weights(IndustryKind::Logistics, &near_depot);
        let fleet = opts.iter().find(|o| o.0 == BusinessKind::FleetLot).unwrap().1;
        let crowded = business_weights(IndustryKind::Logistics, &IndustrySite { fleet_lot_near: true, ..near_depot });
        assert!(crowded.iter().find(|o| o.0 == BusinessKind::FleetLot).unwrap().1 < fleet * 0.2);
    }
}
