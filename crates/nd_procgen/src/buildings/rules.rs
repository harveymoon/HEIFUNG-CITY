//! The rules file: land-use and tier looks, and the style menus.

use super::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LandUseRules {
    pub floor_height: f32,
    /// Inset of the footprint from the parcel edges.
    pub inset: f32,
    pub window_spacing: f32,
    pub window_size: [f32; 2],
    pub sign_chance: f32,
    pub vertical_sign_chance: f32,
    pub roof_antenna_chance: f32,
}

/// Look of a quality tier, applied on top of the land-use rules.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TierStyle {
    pub wall_colors: Vec<[f32; 3]>,
    pub roof_color: [f32; 3],
    pub trim_color: [f32; 3],
    /// Probability a window is lit at night.
    pub lit_chance: f32,
    pub window_colors: Vec<[f32; 3]>,
    /// Stains, patches and rust streaks per facade.
    pub grime: f32,
    /// Air conditioners, dishes, water tanks.
    pub clutter: f32,
}


#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StyleWeight {
    pub land_use: LandUse,
    pub tier: Tier,
    pub style: Style,
    pub weight: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BuildingRules {
    pub residential: LandUseRules,
    pub commercial: LandUseRules,
    pub industrial: LandUseRules,
    pub entertainment: LandUseRules,
    pub civic: LandUseRules,
    pub park: LandUseRules,
    pub tiers: [TierStyle; 3],
    /// Which styles each (land use, tier) draws from, and how often.
    pub styles: Vec<StyleWeight>,
}

impl LandUseRules {
    /// Clamp the chances a rules file may have set outside `0..=1`; they
    /// feed `random_bool`, which panics on anything larger than one.
    pub fn validate(&mut self) {
        for p in [&mut self.sign_chance, &mut self.vertical_sign_chance, &mut self.roof_antenna_chance] {
            *p = if p.is_finite() { p.clamp(0.0, 1.0) } else { 0.0 };
        }
        self.floor_height = self.floor_height.clamp(2.2, 12.0);
        self.inset = self.inset.max(0.0);
        self.window_spacing = self.window_spacing.max(0.8);
        // A window wider than its spacing overlaps the next; one taller
        // than a floor reaches into the floors above and below (and under
        // the ground on the first).
        self.window_size = [self.window_size[0].max(0.2).min(self.window_spacing * 0.9), self.window_size[1].max(0.2).min(self.floor_height * 0.8)];
    }
}

impl TierStyle {
    pub fn validate(&mut self) {
        for p in [&mut self.lit_chance, &mut self.grime, &mut self.clutter] {
            *p = if p.is_finite() { p.clamp(0.0, 1.0) } else { 0.0 };
        }
        if self.wall_colors.is_empty() {
            self.wall_colors.push([0.5, 0.5, 0.5]);
        }
        if self.window_colors.is_empty() {
            self.window_colors.push([1.2, 1.1, 0.9]);
        }
    }

    /// Entry `i` of the lit-window colours, wrapping, never panicking.
    pub fn window_color(&self, i: usize) -> [f32; 3] {
        crate::palette::at(&self.window_colors, i)
    }
}

impl BuildingRules {
    /// Clamp every loaded chance and refill emptied colour lists. The
    /// generators also clamp at the point of use, so this is belt and
    /// braces for anything that reads the rules directly.
    pub fn validate(&mut self) {
        for r in [&mut self.residential, &mut self.commercial, &mut self.industrial, &mut self.entertainment, &mut self.civic, &mut self.park] {
            r.validate();
        }
        for t in &mut self.tiers {
            t.validate();
        }
        self.styles.retain(|w| w.weight.is_finite());
    }

    pub fn for_land_use(&self, lu: LandUse) -> &LandUseRules {
        match lu {
            LandUse::Residential => &self.residential,
            LandUse::Commercial => &self.commercial,
            LandUse::Industrial => &self.industrial,
            LandUse::Entertainment => &self.entertainment,
            LandUse::Civic => &self.civic,
            LandUse::Park => &self.park,
        }
    }

    pub fn style(&self, tier: Tier) -> &TierStyle {
        &self.tiers[tier.index()]
    }

    /// The weighted options for a land use and tier: the rules file's
    /// entries plus the built-in weights of any style the file never
    /// mentions, so styles added after a rules file was written still turn
    /// up (give a style weight 0 in the file to switch it off).
    pub fn options(&self, lu: LandUse, tier: Tier) -> Vec<StyleWeight> {
        let mut out: Vec<StyleWeight> = self.styles.iter().filter(|s| s.land_use == lu && s.tier == tier && s.weight > 0.0 && s.weight.is_finite()).cloned().collect();
        for d in default_style_weights() {
            if d.land_use == lu && d.tier == tier && !self.styles.iter().any(|s| s.style == d.style) {
                out.push(d.clone());
            }
        }
        out
    }

    /// Weighted pick among the styles listed for this land use and tier.
    pub fn pick_style(&self, lu: LandUse, tier: Tier, rng: &mut impl Rng) -> Style {
        self.pick_style_near(lu, tier, &adjacency::Site::default(), &[], rng)
    }

    /// Weighted pick that also weighs how each option suits the site
    /// (floors, frontage) and the styles already standing within
    /// [`adjacency::RADIUS`]: some styles cluster, spires want to differ
    /// from their neighbours. One random draw, like `pick_style`.
    pub fn pick_style_near(&self, lu: LandUse, tier: Tier, site: &adjacency::Site, near: &[Neighbour], rng: &mut impl Rng) -> Style {
        let options = self.options(lu, tier);
        let weights: Vec<f32> = options.iter().map(|o| o.weight * adjacency::site_fit(o.style, site) * adjacency::neighbour_factor(o.style, near)).collect();
        weighted_index(&weights, rng).map_or_else(|| default_style(lu, tier), |i| options[i].style)
    }
}

/// The built-in style weights, built once.
pub(crate) fn default_style_weights() -> &'static [StyleWeight] {
    static W: std::sync::OnceLock<Vec<StyleWeight>> = std::sync::OnceLock::new();
    W.get_or_init(|| BuildingRules::default().styles)
}

pub(crate) fn default_style(lu: LandUse, tier: Tier) -> Style {
    match (lu, tier) {
        (LandUse::Residential, Tier::Low) => Style::Stacks,
        (LandUse::Residential, Tier::Mid) => Style::Brutalist,
        (LandUse::Residential, Tier::High) => Style::Crystal,
        (LandUse::Commercial, Tier::Low) => Style::Shophouse,
        (LandUse::Commercial, Tier::Mid) => Style::RetailBlock,
        (LandUse::Commercial, Tier::High) => Style::SlabTower,
        (LandUse::Industrial, Tier::Low) => Style::RustyWarehouse,
        (LandUse::Industrial, Tier::Mid) => Style::Factory,
        (LandUse::Industrial, Tier::High) => Style::Datacenter,
        (LandUse::Entertainment, Tier::Low) => Style::ShackBar,
        (LandUse::Entertainment, Tier::Mid) => Style::Club,
        (LandUse::Entertainment, Tier::High) => Style::Casino,
        (LandUse::Civic, Tier::Low) => Style::Precinct,
        (LandUse::Civic, Tier::Mid) => Style::Ministry,
        (LandUse::Civic, Tier::High) => Style::Courthouse,
        (LandUse::Park, Tier::Low) => Style::ScrubLot,
        (LandUse::Park, Tier::Mid) => Style::Garden,
        (LandUse::Park, Tier::High) => Style::Plaza,
    }
}

impl Default for BuildingRules {
    fn default() -> Self {
        let base = LandUseRules {
            floor_height: 3.1,
            inset: 1.5,
            window_spacing: 2.6,
            window_size: [1.1, 1.4],
            sign_chance: 0.05,
            vertical_sign_chance: 0.0,
            roof_antenna_chance: 0.35,
        };
        let w = |lu: LandUse, tier: Tier, style: Style, weight: f32| StyleWeight { land_use: lu, tier, style, weight };
        use LandUse::*;
        use Tier::*;
        Self {
            residential: base.clone(),
            commercial: LandUseRules { floor_height: 3.5, window_spacing: 2.2, window_size: [1.5, 1.8], sign_chance: 0.6, vertical_sign_chance: 0.12, ..base.clone() },
            industrial: LandUseRules { floor_height: 5.0, inset: 2.5, window_spacing: 4.0, window_size: [2.5, 0.9], sign_chance: 0.3, roof_antenna_chance: 0.9, ..base.clone() },
            entertainment: LandUseRules { floor_height: 3.6, sign_chance: 0.9, vertical_sign_chance: 0.5, ..base.clone() },
            civic: LandUseRules { floor_height: 4.2, inset: 3.0, window_spacing: 3.0, window_size: [1.6, 2.4], sign_chance: 0.3, ..base.clone() },
            park: LandUseRules { inset: 2.0, sign_chance: 0.0, roof_antenna_chance: 0.0, ..base },
            tiers: [
                TierStyle {
                    wall_colors: vec![[0.42, 0.37, 0.31], [0.36, 0.34, 0.32], [0.46, 0.40, 0.34], [0.33, 0.31, 0.29]],
                    roof_color: [0.18, 0.17, 0.16],
                    trim_color: [0.26, 0.22, 0.18],
                    lit_chance: 0.35,
                    window_colors: vec![[1.3, 0.85, 0.45], [1.1, 0.95, 0.75], [0.7, 0.8, 0.9]],
                    grime: 0.9,
                    clutter: 0.8,
                },
                TierStyle {
                    wall_colors: vec![[0.58, 0.56, 0.54], [0.50, 0.51, 0.53], [0.62, 0.58, 0.52], [0.46, 0.48, 0.50]],
                    roof_color: [0.28, 0.28, 0.29],
                    trim_color: [0.36, 0.36, 0.37],
                    lit_chance: 0.45,
                    window_colors: vec![[1.2, 1.1, 0.9], [1.0, 1.05, 1.15], [1.3, 0.95, 0.6]],
                    grime: 0.3,
                    clutter: 0.35,
                },
                TierStyle {
                    wall_colors: vec![[0.40, 0.46, 0.54], [0.34, 0.40, 0.48], [0.46, 0.50, 0.56], [0.70, 0.70, 0.68]],
                    roof_color: [0.32, 0.34, 0.38],
                    trim_color: [0.72, 0.74, 0.78],
                    lit_chance: 0.55,
                    window_colors: vec![[1.0, 1.15, 1.4], [1.2, 1.2, 1.25], [0.9, 1.0, 1.3]],
                    grime: 0.0,
                    clutter: 0.1,
                },
            ],
            styles: vec![
                w(Residential, Low, Style::Stacks, 0.55),
                w(Residential, Low, Style::Tenement, 0.45),
                w(Residential, Mid, Style::Brutalist, 0.4),
                w(Residential, Mid, Style::BalconySlab, 0.4),
                w(Residential, Mid, Style::Courtyard, 0.2),
                w(Residential, High, Style::Crystal, 0.3),
                w(Residential, High, Style::Fortress, 0.25),
                w(Residential, High, Style::GardenTower, 0.3),
                w(Residential, High, Style::Villas, 0.15),
                w(Commercial, Low, Style::Shophouse, 0.65),
                w(Commercial, Low, Style::MarketHall, 0.35),
                w(Commercial, Mid, Style::RetailBlock, 0.5),
                w(Commercial, Mid, Style::PodiumOffice, 0.5),
                w(Commercial, High, Style::SlabTower, 0.35),
                w(Commercial, High, Style::TwistTower, 0.3),
                w(Commercial, High, Style::CantileverMall, 0.2),
                w(Commercial, High, Style::Crystal, 0.15),
                w(Industrial, Low, Style::Junkyard, 0.45),
                w(Industrial, Low, Style::RustyWarehouse, 0.55),
                w(Industrial, Mid, Style::Factory, 0.6),
                w(Industrial, Mid, Style::TankFarm, 0.4),
                w(Industrial, High, Style::Datacenter, 0.5),
                w(Industrial, High, Style::Depot, 0.5),
                w(Entertainment, Low, Style::ShackBar, 1.0),
                w(Entertainment, Mid, Style::Club, 0.6),
                w(Entertainment, Mid, Style::Cinema, 0.4),
                w(Entertainment, High, Style::Casino, 0.85),
                w(Entertainment, High, Style::Arena, 0.15),
                w(Civic, Low, Style::Precinct, 1.0),
                w(Civic, Mid, Style::Ministry, 0.6),
                w(Civic, Mid, Style::Library, 0.4),
                w(Civic, High, Style::Courthouse, 0.6),
                w(Civic, High, Style::Monument, 0.4),
                w(Park, Low, Style::ScrubLot, 1.0),
                w(Park, Mid, Style::Garden, 1.0),
                w(Park, High, Style::Plaza, 1.0),
                // variety block
                w(Residential, Low, Style::WalledCity, 0.4),
                w(Residential, Low, Style::CapsuleHotel, 0.1),
                w(Residential, Mid, Style::PencilTower, 0.3),
                w(Residential, Mid, Style::TwinSlab, 0.3),
                w(Residential, Mid, Style::HillsideSteps, 0.2),
                w(Residential, High, Style::PencilTower, 0.15),
                w(Residential, High, Style::TwinSlab, 0.2),
                w(Residential, High, Style::GlassTerraces, 0.2),
                w(Residential, High, Style::Megastructure, 0.12),
                w(Residential, High, Style::HillsideSteps, 0.1),
                w(Commercial, Low, Style::NightMarket, 0.25),
                w(Commercial, Low, Style::CapsuleHotel, 0.15),
                w(Commercial, Low, Style::WalledCity, 0.15),
                w(Commercial, Mid, Style::PencilTower, 0.25),
                w(Commercial, Mid, Style::CapsuleHotel, 0.15),
                w(Commercial, Mid, Style::GlassTerraces, 0.15),
                w(Commercial, Mid, Style::NeonTower, 0.15),
                w(Commercial, High, Style::Megastructure, 0.2),
                w(Commercial, High, Style::GlassTerraces, 0.25),
                w(Industrial, Low, Style::Workshops, 0.45),
                w(Industrial, Mid, Style::FlattedFactory, 0.45),
                w(Industrial, High, Style::FabPlant, 0.45),
                w(Entertainment, Low, Style::NightMarket, 0.7),
                w(Entertainment, Mid, Style::NeonTower, 0.5),
                w(Entertainment, High, Style::NeonTower, 0.3),
                w(Entertainment, High, Style::Megastructure, 0.15),
                // [land] low density by class: only fitting one- and
                // two-storey sites (`adjacency::site_fit`)
                w(Residential, Low, Style::Shacks, 0.9),
                w(Residential, Low, Style::TrailerPark, 0.7),
                w(Residential, Low, Style::Huts, 0.35),
                w(Residential, Low, Style::Suburb, 0.25),
                w(Residential, Mid, Style::Suburb, 1.2),
                w(Residential, High, Style::Suburb, 0.5),
            ],
        }
    }
}
