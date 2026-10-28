//! [districts] District architecture: what each district look
//! (`nd_sim::districts::Look`) builds and how it is coloured.
//!
//! A look replaces the land use's style menu with its own (still weighed by
//! the site and the neighbours, so the adjacency families keep working
//! inside a district), and re-tints the tier's walls, trim and windows, the
//! sign, LED and awning colours and how much signage goes up. That is what
//! makes a district read as one place from far off: a neon strip is dark
//! walls under walls of pink and cyan signs, a tech campus white and glass,
//! a historic quarter brick and amber.
//!
//! Hotels (`BusinessKind::Hotel`) have their own menu too: capsule hotels,
//! slabs and glass towers.

use super::adjacency::{self, Neighbour, Site};
use super::{LandUseRules, Style, TierStyle};
use crate::palette::Palette;
use nd_sim::districts::Look;
use nd_sim::{LandUse, Tier};
use rand::Rng;

type Menu = &'static [(Style, f32)];

/// The styles a look builds for a land use and tier, or `None` to keep the
/// land use's own menu (industry, civic and parks have none).
pub fn look_styles(look: Look, lu: LandUse, tier: Tier) -> Option<Menu> {
    use LandUse::*;
    use Style as S;
    use Tier::*;
    Some(match (look, lu, tier) {
        (Look::Kowloon, Residential, Low) => &[(S::WalledCity, 0.5), (S::Stacks, 0.35), (S::CapsuleHotel, 0.15)],
        (Look::Kowloon, Residential, Mid) => &[(S::WalledCity, 0.35), (S::PencilTower, 0.35), (S::TwinSlab, 0.3)],
        (Look::Kowloon, Residential, High) => &[(S::PencilTower, 0.4), (S::TwinSlab, 0.35), (S::Megastructure, 0.25)],
        (Look::Suburban, Residential, Low) => &[(S::Tenement, 0.3), (S::HillsideSteps, 0.4), (S::Courtyard, 0.3)],
        (Look::Suburban, Residential, Mid) => &[(S::Villas, 0.4), (S::HillsideSteps, 0.3), (S::Courtyard, 0.3)],
        (Look::Suburban, Residential, High) => &[(S::Villas, 0.6), (S::HillsideSteps, 0.25), (S::GardenTower, 0.15)],
        (Look::Eco, Residential, Low) => &[(S::HillsideSteps, 0.5), (S::Courtyard, 0.5)],
        (Look::Eco, Residential, Mid) => &[(S::GardenTower, 0.45), (S::HillsideSteps, 0.3), (S::GlassTerraces, 0.25)],
        (Look::Eco, Residential, High) => &[(S::GardenTower, 0.5), (S::GlassTerraces, 0.5)],
        (Look::NeonStrip, Commercial, Low) => &[(S::NightMarket, 0.35), (S::Shophouse, 0.3), (S::NeonTower, 0.35)],
        (Look::NeonStrip, Commercial, Mid) => &[(S::NeonTower, 0.6), (S::PencilTower, 0.4)],
        (Look::NeonStrip, Commercial, High) => &[(S::NeonTower, 0.5), (S::Megastructure, 0.3), (S::TwistTower, 0.2)],
        (Look::NeonStrip, Entertainment, Low) => &[(S::NightMarket, 0.5), (S::ShackBar, 0.5)],
        (Look::NeonStrip, Entertainment, Mid) => &[(S::NeonTower, 0.6), (S::Club, 0.4)],
        (Look::NeonStrip, Entertainment, High) => &[(S::Casino, 0.5), (S::NeonTower, 0.5)],
        (Look::TechCampus, Commercial, Low) => &[(S::RetailBlock, 0.5), (S::PodiumOffice, 0.5)],
        (Look::TechCampus, Commercial, Mid) => &[(S::GlassTerraces, 0.5), (S::PodiumOffice, 0.5)],
        (Look::TechCampus, Commercial, High) => &[(S::GlassTerraces, 0.35), (S::TwistTower, 0.3), (S::Crystal, 0.2), (S::CantileverMall, 0.15)],
        (Look::NightMarket, Commercial, Low) => &[(S::NightMarket, 0.6), (S::MarketHall, 0.25), (S::Shophouse, 0.15)],
        (Look::NightMarket, Commercial, Mid) => &[(S::MarketHall, 0.5), (S::NightMarket, 0.5)],
        (Look::NightMarket, Commercial, High) => &[(S::MarketHall, 0.6), (S::CantileverMall, 0.4)],
        (Look::NightMarket, Entertainment, Low) => &[(S::NightMarket, 0.7), (S::ShackBar, 0.3)],
        (Look::NightMarket, Entertainment, Mid) => &[(S::NightMarket, 0.5), (S::Club, 0.5)],
        (Look::NightMarket, Entertainment, High) => &[(S::NightMarket, 0.5), (S::Casino, 0.5)],
        (Look::Tourist, Commercial, Low) => &[(S::CapsuleHotel, 0.5), (S::Shophouse, 0.25), (S::NightMarket, 0.25)],
        (Look::Tourist, Commercial, Mid) => &[(S::TwinSlab, 0.35), (S::GlassTerraces, 0.35), (S::CapsuleHotel, 0.3)],
        (Look::Tourist, Commercial, High) => &[(S::SlabTower, 0.35), (S::GlassTerraces, 0.35), (S::TwistTower, 0.3)],
        (Look::Historic, Residential, Low) => &[(S::Tenement, 0.6), (S::Courtyard, 0.4)],
        (Look::Historic, Residential, Mid) => &[(S::Courtyard, 0.55), (S::Tenement, 0.45)],
        (Look::Historic, Residential, High) => &[(S::Courtyard, 0.5), (S::Villas, 0.5)],
        (Look::Historic, Commercial, _) => &[(S::Shophouse, 0.6), (S::MarketHall, 0.4)],
        // [capsule] White domes for homes, shops, offices and nights out.
        (Look::Capsule, Residential, Low) => &[(S::DomeHouse, 0.45), (S::SaucerHouse, 0.3), (S::TwinDomes, 0.25)],
        (Look::Capsule, Residential, Mid) => &[(S::DomePods, 0.2), (S::DomeHouse, 0.15), (S::SaucerHouse, 0.15), (S::PopsicleDome, 0.25), (S::CapsuleTower, 0.15), (S::SaucerStack, 0.1), (S::StiltTower, 0.08)],
        (Look::Capsule, Residential, High) => &[(S::CapsuleTower, 0.35), (S::StiltTower, 0.25), (S::PopsicleDome, 0.2), (S::SaucerStack, 0.2)],
        (Look::Capsule, Commercial, Low) => &[(S::DomeHall, 0.45), (S::TwinDomes, 0.35), (S::PopsicleDome, 0.2)],
        (Look::Capsule, Commercial, Mid) => &[(S::DomeCampus, 0.35), (S::SaucerStack, 0.25), (S::DomeHall, 0.25), (S::CapsuleTower, 0.15)],
        (Look::Capsule, Commercial, High) => &[(S::SaucerStack, 0.3), (S::CapsuleTower, 0.25), (S::StiltTower, 0.2), (S::DomeCampus, 0.25)],
        (Look::Capsule, Entertainment, Low) => &[(S::DomeHall, 0.6), (S::TwinDomes, 0.4)],
        (Look::Capsule, Entertainment, _) => &[(S::DomeHall, 0.5), (S::SaucerStack, 0.3), (S::PopsicleDome, 0.2)],
        // [r4-brick] Brick & Steel: worker rows and lofts, workshops, the
        // pub and the union hall, and the works.
        (Look::BrickSteel, Residential, Low) => &[(S::BrickRow, 0.7), (S::WarehouseLoft, 0.3)],
        (Look::BrickSteel, Residential, Mid) => &[(S::BrickRow, 0.4), (S::WarehouseLoft, 0.45), (S::MillLofts, 0.15)],
        (Look::BrickSteel, Residential, High) => &[(S::WarehouseLoft, 0.5), (S::MillLofts, 0.5)],
        (Look::BrickSteel, Commercial, Low) => &[(S::BrickWorkshop, 0.55), (S::CornerPub, 0.3), (S::WarehouseLoft, 0.15)],
        (Look::BrickSteel, Commercial, Mid) => &[(S::BrickWorkshop, 0.35), (S::WarehouseLoft, 0.3), (S::CornerPub, 0.2), (S::UnionHall, 0.15)],
        (Look::BrickSteel, Commercial, High) => &[(S::WarehouseLoft, 0.4), (S::MillLofts, 0.35), (S::UnionHall, 0.25)],
        (Look::BrickSteel, Entertainment, Low) => &[(S::CornerPub, 0.7), (S::BrickWorkshop, 0.3)],
        (Look::BrickSteel, Entertainment, _) => &[(S::CornerPub, 0.55), (S::UnionHall, 0.45)],
        (Look::BrickSteel, Industrial, Low) => &[(S::SawtoothFactory, 0.55), (S::FoundryWorks, 0.25), (S::GasWorks, 0.2)],
        (Look::BrickSteel, Industrial, Mid) => &[(S::SawtoothFactory, 0.45), (S::FoundryWorks, 0.35), (S::GasWorks, 0.2)],
        (Look::BrickSteel, Industrial, High) => &[(S::FoundryWorks, 0.4), (S::SawtoothFactory, 0.35), (S::GasWorks, 0.25)],
        // [lantern] Shophouses and alleys for homes; stalls, halls and tea
        // houses for shops and nights out; workshops and kitchens for its
        // small industry.
        (Look::Lantern, Residential, Low) => &[(S::AlleyWalkups, 0.55), (S::LanternShophouse, 0.45)],
        (Look::Lantern, Residential, _) => &[(S::LanternShophouse, 0.55), (S::AlleyWalkups, 0.45)],
        (Look::Lantern, Commercial, Low) => &[(S::StallRows, 0.35), (S::LanternShophouse, 0.3), (S::FoodCourt, 0.15), (S::TeaHouse, 0.1), (S::LanternMarket, 0.1)],
        (Look::Lantern, Commercial, _) => &[(S::LanternShophouse, 0.3), (S::FoodCourt, 0.2), (S::TeaHouse, 0.2), (S::LanternMarket, 0.2), (S::StallRows, 0.1)],
        (Look::Lantern, Entertainment, _) => &[(S::LanternMarket, 0.35), (S::TeaHouse, 0.3), (S::StallRows, 0.2), (S::FoodCourt, 0.15)],
        (Look::Lantern, Industrial, _) => &[(S::AlleyWorkshops, 0.55), (S::BackKitchens, 0.45)],
        // [garden] Houses, bungalows and closes; a parade, a café, a
        // garden centre.
        (Look::Garden, Residential, Low) => &[(S::Bungalow, 0.5), (S::GardenHouse, 0.38), (S::CulDeSac, 0.12)],
        (Look::Garden, Residential, Mid) => &[(S::GardenHouse, 0.5), (S::Bungalow, 0.25), (S::CulDeSac, 0.25)],
        (Look::Garden, Residential, High) => &[(S::GardenHouse, 0.55), (S::CulDeSac, 0.3), (S::Bungalow, 0.15)],
        (Look::Garden, Commercial, Low) => &[(S::ShoppingParade, 0.45), (S::CornerCafe, 0.35), (S::GardenCentre, 0.2)],
        (Look::Garden, Commercial, _) => &[(S::ShoppingParade, 0.45), (S::GardenCentre, 0.3), (S::CornerCafe, 0.25)],
        // [r4-harbour] Terraces, boathouses and corner towers for homes;
        // the arcade, cafés, shacks and boutique hotels on the front; the
        // fish market and net sheds for its small industry.
        (Look::HarbourPastel, Residential, Low) => &[(S::PastelTerrace, 0.6), (S::Boathouses, 0.3), (S::LighthouseCorner, 0.1)],
        (Look::HarbourPastel, Residential, Mid) => &[(S::PastelTerrace, 0.5), (S::Boathouses, 0.25), (S::LighthouseCorner, 0.25)],
        (Look::HarbourPastel, Residential, High) => &[(S::PastelTerrace, 0.4), (S::LighthouseCorner, 0.35), (S::Boathouses, 0.25)],
        (Look::HarbourPastel, Commercial, Low) => &[(S::AwningArcade, 0.35), (S::BoardwalkCafe, 0.3), (S::SeafoodShack, 0.25), (S::BoutiqueHotel, 0.1)],
        (Look::HarbourPastel, Commercial, _) => &[(S::AwningArcade, 0.35), (S::BoardwalkCafe, 0.25), (S::BoutiqueHotel, 0.25), (S::SeafoodShack, 0.15)],
        (Look::HarbourPastel, Entertainment, _) => &[(S::BoardwalkCafe, 0.4), (S::SeafoodShack, 0.35), (S::AwningArcade, 0.25)],
        (Look::HarbourPastel, Industrial, _) => &[(S::NetSheds, 0.55), (S::FishMarketHall, 0.45)],
        _ => return None,
    })
}

/// [garden] The house a family (`Form::McMansion`) or a billionaire
/// (`Form::Mansion`) builds in a look that has its own: `None` keeps the
/// city-wide McMansion and mansion.
pub fn look_mansion(look: Look) -> Option<Style> {
    match look {
        Look::Garden => Some(Style::GardenManor),
        // [r4-harbour] A pastel villa with a boathouse on The Promenade.
        Look::HarbourPastel => Some(Style::PastelVilla),
        _ => None,
    }
}

/// What a hotel looks like by tier.
pub fn hotel_styles(tier: Tier) -> Menu {
    match tier {
        Tier::Low => &[(Style::CapsuleHotel, 1.0)],
        Tier::Mid => &[(Style::CapsuleHotel, 0.3), (Style::TwinSlab, 0.35), (Style::PencilTower, 0.35)],
        Tier::High => &[(Style::SlabTower, 0.4), (Style::GlassTerraces, 0.3), (Style::TwistTower, 0.3)],
    }
}

/// Weighted pick from a menu, weighed by the site (towers need floors) and
/// the neighbours (families cluster or contrast), as the land use menus are.
pub fn pick(options: Menu, site: &Site, near: &[Neighbour], rng: &mut impl Rng) -> Style {
    let weights: Vec<f32> = options.iter().map(|(s, w)| w * adjacency::site_fit(*s, site) * adjacency::neighbour_factor(*s, near)).collect();
    super::choose::weighted_index(&weights, rng).map_or(options[0].0, |i| options[i].0)
}

/// A look's colours and signage, applied over the tier's and the land
/// use's. The palette is cloned only for a building with a look.
pub struct Tint {
    pub walls: &'static [[f32; 3]],
    pub trim: Option<[f32; 3]>,
    pub windows: &'static [[f32; 3]],
    pub signs: &'static [[f32; 3]],
    pub awnings: Option<&'static [[f32; 3]]>,
    pub brick: Option<&'static [[f32; 3]]>,
    pub plaster: Option<&'static [[f32; 3]]>,
    /// Sign chance and vertical sign chance: `None` keeps the land use's.
    pub signage: Option<(f32, f32)>,
    /// Multiplier on the land use's sign chances when `signage` is unset.
    pub sign_scale: f32,
    /// Grime and clutter override (Kowloon is cluttered whatever its tier).
    pub grime: Option<(f32, f32)>,
}

pub fn tint(look: Look) -> Tint {
    match look {
        Look::Kowloon => Tint {
            walls: &[[0.50, 0.47, 0.40], [0.40, 0.48, 0.46], [0.52, 0.42, 0.42], [0.44, 0.44, 0.36], [0.36, 0.40, 0.48]],
            trim: Some([0.30, 0.34, 0.32]),
            windows: &[[1.3, 0.9, 0.5], [0.8, 1.25, 0.9], [1.2, 1.1, 0.8], [1.3, 0.7, 0.9]],
            signs: &[[2.2, 0.35, 1.0], [0.3, 1.8, 2.2], [2.2, 1.7, 0.3], [2.2, 0.4, 0.3]],
            awnings: None,
            brick: None,
            plaster: None,
            signage: Some((0.8, 0.7)),
            sign_scale: 1.0,
            grime: Some((0.9, 1.0)),
        },
        Look::Suburban => Tint {
            walls: &[[0.78, 0.70, 0.58], [0.72, 0.56, 0.44], [0.80, 0.76, 0.68], [0.62, 0.66, 0.60]],
            trim: Some([0.86, 0.84, 0.78]),
            windows: &[[1.3, 1.0, 0.65], [1.2, 1.05, 0.8]],
            signs: &[[1.2, 1.0, 0.7], [0.9, 1.1, 1.0]],
            awnings: Some(&[[0.55, 0.30, 0.22], [0.30, 0.42, 0.34], [0.70, 0.66, 0.56]]),
            brick: None,
            plaster: Some(&[[0.80, 0.72, 0.60], [0.74, 0.60, 0.48], [0.84, 0.80, 0.72]]),
            signage: None,
            sign_scale: 0.25,
            grime: Some((0.1, 0.15)),
        },
        Look::Eco => Tint {
            walls: &[[0.52, 0.60, 0.50], [0.60, 0.56, 0.46], [0.46, 0.54, 0.52], [0.70, 0.72, 0.66]],
            trim: Some([0.46, 0.34, 0.22]),
            windows: &[[1.1, 1.2, 0.9], [1.25, 1.1, 0.75]],
            signs: &[[0.5, 1.9, 0.6], [1.2, 1.9, 0.3], [0.3, 1.6, 1.4]],
            awnings: Some(&[[0.30, 0.46, 0.28], [0.50, 0.40, 0.26]]),
            brick: None,
            plaster: None,
            signage: None,
            sign_scale: 0.5,
            grime: Some((0.05, 0.1)),
        },
        Look::NeonStrip => Tint {
            walls: &[[0.16, 0.15, 0.20], [0.20, 0.17, 0.24], [0.14, 0.16, 0.20], [0.24, 0.20, 0.22]],
            trim: Some([0.10, 0.10, 0.12]),
            windows: &[[1.4, 0.5, 1.2], [0.5, 1.2, 1.5], [1.4, 1.1, 0.5]],
            signs: &[[2.8, 0.3, 1.6], [0.3, 2.4, 2.8], [1.4, 0.4, 2.8], [2.6, 2.4, 0.3], [2.8, 0.5, 0.4]],
            awnings: Some(&[[0.6, 0.1, 0.4], [0.1, 0.4, 0.6], [0.2, 0.1, 0.4]]),
            brick: None,
            plaster: None,
            signage: Some((0.98, 0.85)),
            sign_scale: 1.0,
            grime: None,
        },
        Look::TechCampus => Tint {
            walls: &[[0.82, 0.84, 0.86], [0.70, 0.74, 0.78], [0.60, 0.66, 0.72], [0.88, 0.88, 0.86]],
            trim: Some([0.90, 0.92, 0.95]),
            windows: &[[0.9, 1.1, 1.45], [1.0, 1.2, 1.3]],
            signs: &[[0.4, 1.6, 2.4], [1.8, 1.9, 2.2], [0.4, 0.8, 2.4]],
            awnings: None,
            brick: None,
            plaster: None,
            signage: None,
            sign_scale: 0.4,
            grime: Some((0.0, 0.05)),
        },
        Look::NightMarket => Tint {
            walls: &[[0.50, 0.30, 0.24], [0.56, 0.40, 0.28], [0.44, 0.34, 0.30], [0.60, 0.48, 0.36]],
            trim: Some([0.36, 0.16, 0.10]),
            windows: &[[1.4, 0.8, 0.4], [1.4, 0.6, 0.35]],
            signs: &[[2.6, 0.4, 0.2], [2.6, 1.2, 0.2], [2.4, 1.9, 0.4], [2.4, 0.3, 0.5]],
            awnings: Some(&[[0.7, 0.12, 0.08], [0.8, 0.4, 0.08], [0.7, 0.55, 0.12]]),
            brick: Some(&[[0.50, 0.24, 0.16], [0.56, 0.30, 0.20]]),
            plaster: None,
            signage: Some((0.92, 0.6)),
            sign_scale: 1.0,
            grime: None,
        },
        Look::Tourist => Tint {
            walls: &[[0.80, 0.78, 0.72], [0.40, 0.58, 0.60], [0.74, 0.64, 0.46], [0.56, 0.62, 0.70]],
            trim: Some([0.80, 0.66, 0.34]),
            windows: &[[1.4, 1.15, 0.7], [1.2, 1.2, 1.25]],
            signs: &[[2.4, 1.8, 0.5], [0.3, 2.0, 2.0], [2.2, 2.2, 2.2]],
            awnings: Some(&[[0.10, 0.45, 0.48], [0.75, 0.58, 0.20], [0.85, 0.85, 0.82]]),
            brick: None,
            plaster: None,
            signage: Some((0.75, 0.45)),
            sign_scale: 1.0,
            grime: Some((0.1, 0.2)),
        },
        // [capsule] Warm white, soft warm portholes, pastel signs and few of
        // them, spotless. The shells carry their own whites and stripes
        // (`buildings/capsule.rs`); the tint lights their windows.
        Look::Capsule => Tint {
            walls: &[[0.93, 0.91, 0.86], [0.95, 0.94, 0.90], [0.91, 0.90, 0.87]],
            trim: Some([0.66, 0.68, 0.72]),
            windows: &[[1.35, 1.12, 0.72], [1.25, 1.18, 0.95], [1.1, 1.2, 1.25]],
            signs: &[[1.7, 1.35, 0.45], [0.45, 1.5, 1.5], [1.7, 0.85, 0.45], [0.7, 1.0, 1.7]],
            awnings: Some(&[[0.96, 0.76, 0.24], [0.22, 0.66, 0.68], [0.95, 0.50, 0.20]]),
            brick: None,
            plaster: Some(&[[0.93, 0.91, 0.86], [0.95, 0.94, 0.90]]),
            signage: None,
            sign_scale: 0.35,
            grime: Some((0.0, 0.0)),
        },
        // [r4-brick] Sooty red brick, amber windows, sodium-orange and cream
        // signs and few of them; grimy whatever the tier. The styles carry
        // their own bricks and steel (`buildings/brick.rs`).
        Look::BrickSteel => Tint {
            walls: &[[0.50, 0.22, 0.15], [0.44, 0.19, 0.14], [0.52, 0.30, 0.21]],
            trim: Some([0.62, 0.58, 0.50]),
            windows: &[[1.35, 0.85, 0.40], [1.25, 0.95, 0.55], [1.40, 0.72, 0.30]],
            signs: &[[2.4, 1.1, 0.25], [2.2, 1.8, 0.9], [2.3, 0.35, 0.2], [0.3, 1.4, 1.1]],
            awnings: Some(&[[0.14, 0.26, 0.18], [0.36, 0.08, 0.07], [0.30, 0.24, 0.16]]),
            brick: Some(&[[0.50, 0.22, 0.15], [0.56, 0.27, 0.18], [0.44, 0.19, 0.14]]),
            plaster: None,
            signage: None,
            sign_scale: 0.5,
            grime: Some((0.6, 0.5)),
        },
        // [lantern] Stained render and faded tile (cream, jade, ochre,
        // dusty pink), dark red trim, warm windows; red, amber and jade
        // signs everywhere; red, green, blue and tan awnings.
        Look::Lantern => Tint {
            walls: &[[0.66, 0.60, 0.50], [0.46, 0.56, 0.50], [0.64, 0.52, 0.34], [0.64, 0.48, 0.46], [0.52, 0.52, 0.50]],
            trim: Some([0.36, 0.12, 0.09]),
            windows: &[[1.4, 0.95, 0.5], [1.35, 0.8, 0.45], [1.2, 1.05, 0.7]],
            signs: &[[2.6, 0.35, 0.2], [2.5, 1.5, 0.3], [0.4, 2.0, 1.2], [2.4, 0.5, 0.6], [2.2, 2.0, 1.4]],
            awnings: Some(&[[0.62, 0.12, 0.09], [0.16, 0.36, 0.26], [0.16, 0.28, 0.48], [0.58, 0.46, 0.20]]),
            brick: Some(&[[0.48, 0.25, 0.18], [0.40, 0.30, 0.24]]),
            plaster: Some(&[[0.70, 0.64, 0.52], [0.56, 0.62, 0.54], [0.68, 0.56, 0.40]]),
            signage: Some((0.9, 0.65)),
            sign_scale: 1.0,
            grime: None,
        },
        // [garden] Brick, render and painted boards under tile and slate,
        // warm lamplit windows, few signs and quiet ones, a little worn.
        Look::Garden => Tint {
            walls: &[[0.80, 0.76, 0.66], [0.62, 0.40, 0.30], [0.74, 0.74, 0.70], [0.70, 0.72, 0.64]],
            trim: Some([0.86, 0.85, 0.80]),
            windows: &[[1.3, 1.0, 0.62], [1.25, 1.05, 0.75]],
            signs: &[[1.5, 1.2, 0.6], [1.2, 1.4, 1.1], [1.6, 0.9, 0.5]],
            awnings: Some(&[[0.20, 0.36, 0.26], [0.52, 0.16, 0.16], [0.18, 0.24, 0.44], [0.70, 0.62, 0.40]]),
            brick: Some(&[[0.56, 0.30, 0.22], [0.50, 0.28, 0.21], [0.62, 0.40, 0.28], [0.46, 0.30, 0.24]]),
            plaster: Some(&[[0.84, 0.80, 0.70], [0.80, 0.78, 0.72], [0.74, 0.70, 0.60]]),
            signage: None,
            sign_scale: 0.2,
            grime: Some((0.15, 0.2)),
        },
        // [r4-harbour] Salt-faded pastels and white trim, warm windows;
        // few signs, in sea-front reds, yellows and teals; striped canvas.
        // The styles paint their own pastels (`buildings/harbour.rs`).
        Look::HarbourPastel => Tint {
            walls: &[[0.60, 0.74, 0.64], [0.58, 0.68, 0.78], [0.86, 0.78, 0.50], [0.84, 0.60, 0.58], [0.68, 0.62, 0.76], [0.86, 0.82, 0.70]],
            trim: Some([0.86, 0.85, 0.80]),
            windows: &[[1.4, 1.0, 0.55], [1.3, 1.1, 0.7], [1.35, 0.9, 0.5]],
            signs: &[[2.4, 0.4, 0.3], [2.4, 1.9, 0.5], [0.3, 1.8, 1.7], [2.2, 2.1, 1.8]],
            awnings: Some(&[[0.66, 0.14, 0.12], [0.14, 0.20, 0.40], [0.10, 0.44, 0.46], [0.88, 0.70, 0.18]]),
            brick: Some(&[[0.50, 0.30, 0.24], [0.44, 0.34, 0.30]]),
            plaster: Some(&[[0.86, 0.82, 0.72], [0.80, 0.84, 0.80], [0.86, 0.78, 0.70]]),
            signage: None,
            sign_scale: 0.5,
            grime: Some((0.35, 0.3)),
        },
        Look::Historic => Tint {
            walls: &[[0.55, 0.32, 0.24], [0.62, 0.56, 0.46], [0.48, 0.30, 0.22], [0.66, 0.60, 0.50]],
            trim: Some([0.74, 0.70, 0.60]),
            windows: &[[1.3, 0.95, 0.55], [1.25, 1.05, 0.7]],
            signs: &[[1.8, 1.2, 0.5], [1.6, 1.5, 1.2]],
            awnings: Some(&[[0.34, 0.14, 0.10], [0.20, 0.30, 0.22], [0.40, 0.32, 0.18]]),
            brick: Some(&[[0.52, 0.28, 0.20], [0.58, 0.34, 0.24], [0.46, 0.26, 0.18]]),
            plaster: Some(&[[0.70, 0.64, 0.52], [0.64, 0.58, 0.48]]),
            signage: None,
            sign_scale: 0.6,
            grime: None,
        },
    }
}

/// The tier style, land use rules and palette a building of `look` is
/// generated with.
pub fn apply(look: Look, tier_style: &TierStyle, rules: &LandUseRules, palette: &Palette) -> (TierStyle, LandUseRules, Palette) {
    let t = tint(look);
    let mut ts = tier_style.clone();
    ts.wall_colors = t.walls.to_vec();
    ts.window_colors = t.windows.to_vec();
    if let Some(trim) = t.trim {
        ts.trim_color = trim;
    }
    if let Some((grime, clutter)) = t.grime {
        ts.grime = grime;
        ts.clutter = clutter;
    }
    let mut r = rules.clone();
    match t.signage {
        Some((s, v)) => {
            r.sign_chance = s;
            r.vertical_sign_chance = v;
        }
        None => {
            r.sign_chance = (r.sign_chance * t.sign_scale).clamp(0.0, 1.0);
            r.vertical_sign_chance = (r.vertical_sign_chance * t.sign_scale).clamp(0.0, 1.0);
        }
    }
    let mut p = palette.clone();
    p.sign_colors = t.signs.to_vec();
    p.led_colors = t.signs.iter().map(|c| c.map(|x| x * 1.1)).collect();
    if let Some(a) = t.awnings {
        p.awning_colors = a.to_vec();
    }
    if let Some(b) = t.brick {
        p.brick = b.to_vec();
    }
    if let Some(pl) = t.plaster {
        p.plaster = pl.to_vec();
    }
    (ts, r, p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nd_sim::rng_for;

    #[test]
    fn every_look_has_a_menu_for_its_land_uses() {
        for look in Look::ALL {
            let uses: &[LandUse] = match look {
                Look::Kowloon | Look::Suburban | Look::Eco => &[LandUse::Residential],
                Look::NeonStrip | Look::NightMarket => &[LandUse::Commercial, LandUse::Entertainment],
                Look::TechCampus | Look::Tourist => &[LandUse::Commercial],
                Look::Historic => &[LandUse::Residential, LandUse::Commercial],
                Look::Capsule => &[LandUse::Residential, LandUse::Commercial, LandUse::Entertainment],
                Look::BrickSteel => &[LandUse::Residential, LandUse::Commercial, LandUse::Entertainment, LandUse::Industrial],
                Look::Lantern => &[LandUse::Residential, LandUse::Commercial, LandUse::Entertainment, LandUse::Industrial],
                Look::Garden => &[LandUse::Residential, LandUse::Commercial],
                Look::HarbourPastel => &[LandUse::Residential, LandUse::Commercial, LandUse::Entertainment, LandUse::Industrial],
            };
            for &lu in uses {
                for tier in Tier::ALL {
                    let menu = look_styles(look, lu, tier).unwrap_or_else(|| panic!("{look:?} {lu:?} {tier:?}"));
                    assert!(!menu.is_empty() && menu.iter().all(|(_, w)| *w > 0.0));
                }
            }
            // [r4-brick] Brick & Steel builds the works in its look; [lantern]
            // Lantern Alley its small trades. No other look dresses industry.
            // [r4-harbour] Harbour Pastel its fish market and net sheds.
            assert_eq!(look_styles(look, LandUse::Industrial, Tier::Mid).is_some(), matches!(look, Look::BrickSteel | Look::Lantern | Look::HarbourPastel));
        }
    }

    #[test]
    fn looks_read_differently() {
        let base = crate::buildings::BuildingRules::default();
        let pal = Palette::default();
        let (neon, neon_r, neon_p) = apply(Look::NeonStrip, base.style(Tier::Mid), &base.commercial, &pal);
        let (tech, tech_r, _) = apply(Look::TechCampus, base.style(Tier::Mid), &base.commercial, &pal);
        let lum = |c: &[f32; 3]| c[0] + c[1] + c[2];
        let avg = |v: &[[f32; 3]]| v.iter().map(lum).sum::<f32>() / v.len() as f32;
        assert!(avg(&neon.wall_colors) * 2.0 < avg(&tech.wall_colors), "the strip is dark, the campus pale");
        assert!(neon_r.sign_chance > tech_r.sign_chance);
        assert_ne!(neon_p.sign_colors, pal.sign_colors);
    }

    #[test]
    fn a_look_builds_from_its_menu_in_its_colours() {
        use crate::buildings::{choose_style, generate_building, BuildingRules, Extras};
        use nd_sim::{World, WorldConfig};
        let mut w = World::new(WorldConfig { landscape: nd_sim::LandscapeParams::flat(), ..WorldConfig::default() });
        w.add_road(&[glam::Vec2::new(-300.0, 0.0), glam::Vec2::new(300.0, 0.0)], 1, 1);
        let sq = vec![glam::Vec2::new(0.0, 5.0), glam::Vec2::new(200.0, 5.0), glam::Vec2::new(200.0, 200.0), glam::Vec2::new(0.0, 200.0)];
        w.add_zone(sq, LandUse::Commercial);
        let lot = w.lots.values().find(|l| l.usable() && l.land_use == LandUse::Commercial).expect("a lot").clone();
        let rules = BuildingRules::default();
        let pal = Palette::default();
        let menu = look_styles(Look::NeonStrip, LandUse::Commercial, Tier::Mid).unwrap();
        for seed in 0..24u64 {
            let ex = Extras { look: Some(Look::NeonStrip), ..Extras::default() };
            let style = choose_style(&lot, LandUse::Commercial, 5, Tier::Mid, seed, &rules, &ex);
            assert!(menu.iter().any(|(m, _)| *m == style), "{style:?}");
            let g = generate_building(&lot, LandUse::Commercial, 5, Tier::Mid, 1.0, seed, &rules, &pal, &ex);
            assert!(g.triangle_count() > 50);
        }
        // A hotel without a district look is drawn as a hotel.
        let ex = Extras { business: Some(nd_sim::BusinessKind::Hotel), ..Extras::default() };
        assert_eq!(choose_style(&lot, LandUse::Commercial, 3, Tier::Low, 1, &rules, &ex), Style::CapsuleHotel);
    }

    #[test]
    fn picks_stay_on_the_menu() {
        let mut rng = rng_for(3, 4);
        let menu = look_styles(Look::Kowloon, LandUse::Residential, Tier::Low).unwrap();
        for _ in 0..200 {
            let s = pick(menu, &Site { floors: 6, frontage: 20.0 }, &[], &mut rng);
            assert!(menu.iter().any(|(m, _)| *m == s));
        }
    }
}
