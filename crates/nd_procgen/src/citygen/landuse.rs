//! Which use each block gets: a simple land-use model. Shops and offices
//! crowd the centres and line the arterials, works yards go to the edge of
//! town, by the railway and the highway, parks take the waterfronts, the
//! plazas and odd corners, and homes fill the rest. Each use takes a share
//! of the city's street frontage (what lots are cut along), the blocks that
//! suit it best first.

use glam::Vec2;
use nd_sim::LandUse;

/// A block (or a strip along the outer streets) to zone.
#[derive(Clone, Debug)]
pub struct Patch {
    pub polygon: Vec<Vec2>,
    pub centroid: Vec2,
    /// Street frontage, metres: how much it counts towards a use's share.
    pub frontage: f32,
    pub area: f32,
    /// 0..1, 1 at a centre.
    pub core: f32,
    pub on_arterial: bool,
    pub on_collector: bool,
    pub waterfront: bool,
    /// 0..1: 1 on the edge of town.
    pub edge: f32,
    /// 0..1: near the railway, the highway or a works district.
    pub industry: f32,
    /// 0..1: inside a works district or the farms the plan names.
    pub works: f32,
    /// The square at a centre.
    pub plaza: bool,
    /// 0..1, the block's own throw of the dice.
    pub luck: f32,
}

/// Each use's share of the frontage, in the order they are handed out.
const SHARES: [(LandUse, f32); 5] = [(LandUse::Industrial, 0.15), (LandUse::Commercial, 0.19), (LandUse::Entertainment, 0.06), (LandUse::Civic, 0.04), (LandUse::Park, 0.07)];
/// [perfect-city] The planned city: its works and farms are zoned whole
/// on top of this (`zone`), and its services stand on lots of their own,
/// so fewer shops and offices than a grown town: more of the frontage is
/// homes (a jobs surplus brings people who then have nowhere to live).
const PERFECT_SHARES: [(LandUse, f32); 5] = [(LandUse::Industrial, 0.10), (LandUse::Commercial, 0.14), (LandUse::Entertainment, 0.04), (LandUse::Civic, 0.02), (LandUse::Park, 0.08)];

/// Each use's share of the frontage for a preset.
pub fn shares(preset: super::CityPreset) -> &'static [(LandUse, f32)] {
    match preset {
        super::CityPreset::PerfectCity => &PERFECT_SHARES,
        _ => &SHARES,
    }
}

fn score(lu: LandUse, p: &Patch) -> f32 {
    let art = if p.on_arterial { 1.0 } else if p.on_collector { 0.4 } else { 0.0 };
    match lu {
        LandUse::Industrial => {
            if p.core > 0.45 || p.plaza {
                f32::NEG_INFINITY
            } else {
                p.industry * (1.0 - p.core).powi(2) + 0.3 * p.edge + 0.1 * p.luck + 0.05 * art
            }
        }
        LandUse::Commercial => p.core + 0.3 * art + 0.15 * p.luck,
        LandUse::Entertainment => p.core * 0.9 + 0.2 * art + 0.3 * p.luck + if p.waterfront { 0.15 } else { 0.0 },
        LandUse::Civic => p.core * 0.6 + 0.45 * p.luck,
        LandUse::Park => {
            let small = (1.0 - p.area / 6000.0).clamp(0.0, 1.0);
            (if p.waterfront { 0.6 } else { 0.0 }) + 0.45 * p.luck + 0.3 * small - 0.25 * p.core - 0.2 * p.edge
        }
        LandUse::Residential => 0.0,
    }
}

/// A use for every patch.
pub fn allocate(patches: &[Patch], shares: &[(LandUse, f32)]) -> Vec<LandUse> {
    let total: f32 = patches.iter().map(|p| p.frontage).sum();
    let mut out: Vec<Option<LandUse>> = patches.iter().map(|p| p.plaza.then_some(LandUse::Park)).collect();
    for &(lu, share) in shares {
        let mut taken: f32 = out.iter().zip(patches).filter(|(u, _)| **u == Some(lu)).map(|(_, p)| p.frontage).sum();
        let mut order: Vec<(usize, f32)> = (0..patches.len()).filter(|&i| out[i].is_none()).map(|i| (i, score(lu, &patches[i]))).filter(|(_, s)| s.is_finite()).collect();
        order.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        for (i, _) in order {
            if taken >= total * share {
                break;
            }
            out[i] = Some(lu);
            taken += patches[i].frontage;
        }
    }
    out.into_iter().map(|u| u.unwrap_or(LandUse::Residential)).collect()
}
