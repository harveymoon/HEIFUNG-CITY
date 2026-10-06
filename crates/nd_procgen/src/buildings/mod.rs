//! Building grammar. Takes a parcel, land use, floors, tier, condition and a
//! seed and returns geometry bucketed by surface.
//!
//! Every (land use, tier) pair has its own family of styles, picked by weight
//! from the rules file, so an industrial slum reads as junkyards and rusted
//! sheds while a high-value industrial block gets datacentres and container
//! depots. Footprints follow the parcel polygon: a triangular corner lot gets
//! a triangular building.

pub mod adjacency;
pub mod air;
mod civic;
mod commercial;
mod construction;
mod dense;
mod entertainment;
mod industrial;
pub mod kit;
mod park;
mod parking;
pub mod parts;
mod power;
mod rail;
pub mod waterworks;
pub mod transit;
mod residential;
mod towers;
mod works;

pub use adjacency::{Neighbour, StyleBook};
mod services;
/// [society] The trash depot.
mod sanitation;
pub use parts::WINDOW_LIGHT_MARK;
pub use power::{coal_stacks, wind_rotor, wind_turbines, Turbine, TURBINE_MAST, TURBINE_RADIUS, TURBINE_ROTOR_H};

use crate::palette::Palette;
use crate::surface::{Geometry, Surface};
use glam::{Vec2, Vec3};
use nd_sim::rng_for;
use nd_sim::zoning::{point_in_polygon, polygon_area};
use nd_sim::park::ParkLayout;
use nd_sim::rail::{RailFacility, RailLink};
use nd_sim::power::PowerPlantKind;
use nd_sim::{LandUse, Lot, Tier};
use rand::Rng;
use serde::{Deserialize, Serialize};

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

/// Architectural styles. Each belongs to one land use; the tier decides which
/// are on the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Style {
    // residential
    Stacks,
    Tenement,
    Brutalist,
    BalconySlab,
    Courtyard,
    Crystal,
    Fortress,
    GardenTower,
    Villas,
    // commercial
    Shophouse,
    MarketHall,
    RetailBlock,
    PodiumOffice,
    SlabTower,
    TwistTower,
    CantileverMall,
    // industrial
    Junkyard,
    RustyWarehouse,
    Factory,
    TankFarm,
    Datacenter,
    Depot,
    // entertainment
    ShackBar,
    Club,
    Cinema,
    Casino,
    Arena,
    // civic
    Precinct,
    Ministry,
    Library,
    Courthouse,
    Monument,
    // park
    ScrubLot,
    Garden,
    Plaza,
    // parking businesses
    ParkingLot,
    CarPark,
    // power plants
    PowerCoal,
    PowerSolar,
    PowerWind,
    PowerNuclear,
    // transit
    BusDepot,
    Station,
    FreightYard,
    // -- variety: Hong Kong, Akira and Ghost-in-the-Shell forms. Added as
    // one block at the end so `ALL` keeps its order. --
    /// Kowloon walled block: abutting slabs of mismatched height, cages,
    /// AC units and an antenna forest.
    WalledCity,
    /// Hong Kong pencil tower on a shop podium.
    PencilTower,
    /// Shanghai twin slabs with crowned tops and a link bridge.
    TwinSlab,
    /// Akira megastructure: a heavy base, several towers, sky bridges.
    Megastructure,
    /// Ghost-in-the-Shell stack of offset glass boxes with planted terraces.
    GlassTerraces,
    /// Capsule hotel: pods plugged into a concrete core.
    CapsuleHotel,
    /// Terraced housing stepping up from the street like a hillside.
    HillsideSteps,
    /// Covered night market arcade with stalls, lanterns and string lights.
    NightMarket,
    /// Karaoke / pachinko tower wrapped in signs and screens.
    NeonTower,
    /// Row of garage workshops and chop shops.
    Workshops,
    /// Hong Kong flatted factory: stacked industrial floors and a goods lift.
    FlattedFactory,
    /// Clean semiconductor fab: white boxes, exhaust stacks and pipe racks.
    FabPlant,
    // landscape: water and hot-ground plants, docks
    PowerHydro,
    PowerTidal,
    PowerGeothermal,
    ShippingPort,
    FerryTerminal,
    // city services and attractions (buildings/services.rs), placed by the player
    PoliceStation,
    FireStation,
    Hospital,
    School,
    DispatchCenter,
    Landmark,
    Stadium,
    Prison,
    // -- industry sub-zones (`nd_sim::industry`): forced by the business on
    // the lot, never drawn from the land use's menu. One block at the end. --
    /// Fields in rows, a barn, silos and a greenhouse.
    Farmstead,
    /// Pumpjacks and a derrick over stained ground.
    OilField,
    /// Distillation columns, pipe racks, tanks and flares.
    Refinery,
    /// Container stacks with a gantry and a loading lane.
    ContainerYard,
    /// Processing sheds, an ice plant, crates and a boat on blocks.
    FishProcessing,
    /// Marked bays with parked lorries and buses, a fuel island.
    TruckLot,
    // [society] placed by the player; kept last so `ALL` keeps its order.
    TrashDepot,
    // air transport, placed by the player (buildings/air.rs)
    Vertiport,
}

impl Style {
    /// Position in `ALL`, for per-style tables.
    pub fn index(self) -> usize {
        self as usize
    }

    pub const ALL: [Style; 77] = [
        Style::Stacks,
        Style::Tenement,
        Style::Brutalist,
        Style::BalconySlab,
        Style::Courtyard,
        Style::Crystal,
        Style::Fortress,
        Style::GardenTower,
        Style::Villas,
        Style::Shophouse,
        Style::MarketHall,
        Style::RetailBlock,
        Style::PodiumOffice,
        Style::SlabTower,
        Style::TwistTower,
        Style::CantileverMall,
        Style::Junkyard,
        Style::RustyWarehouse,
        Style::Factory,
        Style::TankFarm,
        Style::Datacenter,
        Style::Depot,
        Style::ShackBar,
        Style::Club,
        Style::Cinema,
        Style::Casino,
        Style::Arena,
        Style::Precinct,
        Style::Ministry,
        Style::Library,
        Style::Courthouse,
        Style::Monument,
        Style::ScrubLot,
        Style::Garden,
        Style::Plaza,
        Style::ParkingLot,
        Style::CarPark,
        Style::PowerCoal,
        Style::PowerSolar,
        Style::PowerWind,
        Style::PowerNuclear,
        Style::BusDepot,
        Style::Station,
        Style::FreightYard,
        Style::WalledCity,
        Style::PencilTower,
        Style::TwinSlab,
        Style::Megastructure,
        Style::GlassTerraces,
        Style::CapsuleHotel,
        Style::HillsideSteps,
        Style::NightMarket,
        Style::NeonTower,
        Style::Workshops,
        Style::FlattedFactory,
        Style::FabPlant,
        Style::PowerHydro,
        Style::PowerTidal,
        Style::PowerGeothermal,
        Style::ShippingPort,
        Style::FerryTerminal,
        Style::PoliceStation,
        Style::FireStation,
        Style::Hospital,
        Style::School,
        Style::DispatchCenter,
        Style::Landmark,
        Style::Stadium,
        Style::Prison,
        Style::Farmstead,
        Style::OilField,
        Style::Refinery,
        Style::ContainerYard,
        Style::FishProcessing,
        Style::TruckLot,
        Style::TrashDepot,
        Style::Vertiport,
    ];

    /// High-tier towers that take a rooftop helideck when the sim gives the
    /// building a pad (`buildings::air::rooftop_deck` finds the roof).
    pub fn pad_capable(self) -> bool {
        matches!(self, Style::Crystal | Style::Fortress | Style::GardenTower | Style::PencilTower | Style::TwinSlab | Style::GlassTerraces | Style::Megastructure | Style::SlabTower | Style::TwistTower)
    }

    /// The style a placed service or attraction is drawn in.
    pub fn for_service(kind: nd_sim::services::ServiceKind) -> Style {
        use nd_sim::services::ServiceKind as K;
        match kind {
            K::Police => Style::PoliceStation,
            K::Fire => Style::FireStation,
            K::Health => Style::Hospital,
            K::School => Style::School,
            K::Dispatch => Style::DispatchCenter,
            K::Monument => Style::Landmark,
            K::Stadium => Style::Stadium,
            K::Jail => Style::Prison,
            K::TrashDepot => Style::TrashDepot,
            K::Vertiport => Style::Vertiport,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Style::Stacks => "the Stacks",
            Style::Tenement => "walk-up tenement",
            Style::Brutalist => "brutalist block",
            Style::BalconySlab => "balcony slab",
            Style::Courtyard => "courtyard block",
            Style::Crystal => "crystal tower",
            Style::Fortress => "glass castle",
            Style::GardenTower => "garden tower",
            Style::Villas => "villa cluster",
            Style::Shophouse => "shophouses",
            Style::MarketHall => "market hall",
            Style::RetailBlock => "retail block",
            Style::PodiumOffice => "podium office",
            Style::SlabTower => "corporate slab",
            Style::TwistTower => "twist tower",
            Style::CantileverMall => "cantilever mall",
            Style::Junkyard => "junkyard",
            Style::RustyWarehouse => "rusted warehouse",
            Style::Factory => "factory",
            Style::TankFarm => "tank farm",
            Style::Datacenter => "datacentre",
            Style::Depot => "container depot",
            Style::ShackBar => "shack bar",
            Style::Club => "club",
            Style::Cinema => "cinema",
            Style::Casino => "casino",
            Style::Arena => "arena",
            Style::Precinct => "precinct house",
            Style::Ministry => "ministry",
            Style::Library => "library",
            Style::Courthouse => "courthouse",
            Style::Monument => "monument",
            Style::ScrubLot => "scrub lot",
            Style::Garden => "garden",
            Style::Plaza => "plaza",
            Style::ParkingLot => "parking lot",
            Style::CarPark => "car park",
            Style::PowerCoal => "coal plant",
            Style::PowerSolar => "solar farm",
            Style::PowerWind => "wind farm",
            Style::PowerNuclear => "nuclear plant",
            Style::BusDepot => "bus depot",
            Style::Station => "railway station",
            Style::FreightYard => "freight yard",
            Style::WalledCity => "walled block",
            Style::PencilTower => "pencil tower",
            Style::TwinSlab => "twin slabs",
            Style::Megastructure => "megastructure",
            Style::GlassTerraces => "glass terraces",
            Style::CapsuleHotel => "capsule hotel",
            Style::HillsideSteps => "stepped terraces",
            Style::NightMarket => "night market",
            Style::NeonTower => "neon tower",
            Style::Workshops => "workshops",
            Style::FlattedFactory => "flatted factory",
            Style::FabPlant => "fab plant",
            Style::PowerHydro => "hydro plant",
            Style::PowerTidal => "tidal plant",
            Style::PowerGeothermal => "geothermal plant",
            Style::ShippingPort => "shipping port",
            Style::FerryTerminal => "ferry terminal",
            Style::PoliceStation => "police station",
            Style::FireStation => "fire station",
            Style::Hospital => "hospital",
            Style::School => "school",
            Style::DispatchCenter => "911 dispatch centre",
            Style::Landmark => "landmark",
            Style::Stadium => "stadium",
            Style::Prison => "jail",
            Style::Farmstead => "farm",
            Style::OilField => "oil field",
            Style::Refinery => "refinery",
            Style::ContainerYard => "container yard",
            Style::FishProcessing => "fish processing",
            Style::TruckLot => "truck and bus lot",
            Style::TrashDepot => "trash depot",
            Style::Vertiport => "vertiport",
        }
    }
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
        self.window_size = [self.window_size[0].max(0.2), self.window_size[1].max(0.2)];
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

    pub fn wall_color(&self, i: usize) -> [f32; 3] {
        crate::palette::at(&self.wall_colors, i)
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
        let total: f32 = weights.iter().sum();
        if options.is_empty() || total.is_nan() || total <= 0.0 {
            return default_style(lu, tier);
        }
        let mut r = rng.random_range(0.0..total);
        for (o, w) in options.iter().zip(&weights) {
            if r < *w {
                return o.style;
            }
            r -= w;
        }
        options.last().unwrap().style
    }
}

/// The built-in style weights, built once.
fn default_style_weights() -> &'static [StyleWeight] {
    static W: std::sync::OnceLock<Vec<StyleWeight>> = std::sync::OnceLock::new();
    W.get_or_init(|| BuildingRules::default().styles)
}

fn default_style(lu: LandUse, tier: Tier) -> Style {
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
            ],
        }
    }
}

// ── geometry frame ───────────────────────────────────────────────────────────

/// Parcel-aligned coordinate frame: `u` runs along the frontage, `v` inward
/// from the road, origin at the centre of the parcel's bounding rectangle.
///
/// Generators work in two coordinate systems: lot-local (`Ctx::lot_poly`,
/// `Face`, `Frame::bx`) and world (`ParkLayout`, `RailLink`, the `*_world`
/// helpers). The identity frame from [`Frame::world`] lets a frame-taking
/// helper draw in world space; [`Frame::is_world`] tells which one a frame is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub center: Vec2,
    pub along: Vec2,
    pub inward: Vec2,
    pub width: f32,
    pub depth: f32,
}

impl Frame {
    pub fn from_lot(lot: &Lot) -> Self {
        let along = (lot.corners[1] - lot.corners[0]).normalize_or_zero();
        let inward = Vec2::new(-along.y, along.x);
        let center = (lot.corners[0] + lot.corners[1] + lot.corners[2] + lot.corners[3]) * 0.25;
        Self { center, along, inward, width: lot.width(), depth: lot.depth() }
    }

    /// The identity frame: lot-local and world coordinates coincide, so the
    /// frame-taking helpers draw straight into world space (parks, rail and
    /// vacant lots lay out from world polygons).
    pub fn world() -> Self {
        Self { center: Vec2::ZERO, along: Vec2::X, inward: Vec2::Y, width: 0.0, depth: 0.0 }
    }

    pub fn is_world(&self) -> bool {
        self.center == Vec2::ZERO && self.along == Vec2::X && self.inward == Vec2::Y
    }

    pub fn yaw(&self) -> f32 {
        self.along.y.atan2(self.along.x)
    }

    /// Lot-local direction to world, without translation.
    pub fn dir3(&self, d: Vec2) -> Vec3 {
        let w = self.along * d.x + self.inward * d.y;
        Vec3::new(w.x, 0.0, w.y)
    }

    pub fn to_world(&self, u: f32, v: f32) -> Vec2 {
        self.center + self.along * u + self.inward * v
    }

    pub fn to_local(&self, p: Vec2) -> Vec2 {
        let d = p - self.center;
        Vec2::new(d.dot(self.along), d.dot(self.inward))
    }

    pub fn at(&self, u: f32, y: f32, v: f32) -> Vec3 {
        let p = self.to_world(u, v);
        Vec3::new(p.x, y, p.y)
    }

    pub fn at2(&self, p: Vec2, y: f32) -> Vec3 {
        self.at(p.x, y, p.y)
    }

    /// Box in parcel coordinates: centre (u, y, v) and size (along, up, inward).
    pub fn bx(&self, g: &mut Geometry, s: Surface, u: f32, y: f32, v: f32, size: Vec3, color: [f32; 3]) {
        let p = self.to_world(u, v);
        g.part(s).boxed(Vec3::new(p.x, y, p.y), size, self.yaw(), parts::rgba(color));
    }

    /// Box rotated by `extra` radians relative to the frame.
    pub fn bx_yaw(&self, g: &mut Geometry, s: Surface, u: f32, y: f32, v: f32, size: Vec3, color: [f32; 3], extra: f32) {
        let p = self.to_world(u, v);
        g.part(s).boxed(Vec3::new(p.x, y, p.y), size, self.yaw() + extra, parts::rgba(color));
    }

    /// Flat painted mark lying on the ground at height `y`: `w` along the
    /// frame (plus `extra` yaw) and `d` across. One upward quad.
    #[allow(clippy::too_many_arguments)]
    pub fn flat(&self, g: &mut Geometry, s: Surface, u: f32, y: f32, v: f32, w: f32, d: f32, extra: f32, color: [f32; 3]) {
        let yaw = self.yaw() + extra;
        let along = Vec3::new(yaw.cos(), 0.0, yaw.sin());
        let across = Vec3::new(-yaw.sin(), 0.0, yaw.cos());
        g.part(s).decal(self.at(u, y, v), along, across, w, d, Vec3::Y, parts::rgba(color));
    }

    /// Lot-local polygon to world points at height `y`.
    pub fn poly3(&self, poly: &[Vec2], y: f32) -> Vec<Vec3> {
        poly.iter().map(|p| self.at2(*p, y)).collect()
    }
}

/// One edge of a footprint, with helpers to place things on or in front of it.
#[derive(Clone, Copy, Debug)]
pub struct Face {
    pub a: Vec2,
    pub b: Vec2,
    pub tangent: Vec2,
    /// Outward normal (lot-local).
    pub out: Vec2,
    pub len: f32,
    /// Faces the road.
    pub is_front: bool,
}

impl Face {
    /// The edge a→b with its outside on the right of travel, as the edges
    /// of a counter-clockwise footprint have.
    pub fn from_edge(a: Vec2, b: Vec2) -> Self {
        let len = a.distance(b);
        let tangent = if len > 1e-4 { (b - a) / len } else { Vec2::X };
        let out = Vec2::new(tangent.y, -tangent.x);
        Self { a, b, tangent, out, len, is_front: out.dot(Vec2::new(0.0, -1.0)) > 0.6 }
    }

    /// The edge a→b facing the side `out` points to. The endpoints are
    /// swapped when needed so `tangent`, `out`, `a` and `b` always agree,
    /// which a struct literal override of one field cannot promise.
    pub fn new(a: Vec2, b: Vec2, out: Vec2) -> Self {
        let f = Self::from_edge(a, b);
        if f.out.dot(out) < 0.0 { f.reversed() } else { f }
    }

    /// The same edge seen from the other side.
    pub fn reversed(&self) -> Self {
        Self::from_edge(self.b, self.a)
    }

    pub fn mid(&self) -> Vec2 {
        (self.a + self.b) * 0.5
    }

    /// Point `t` along the face (0 at the middle) and `d` outward.
    pub fn at(&self, t: f32, d: f32) -> Vec2 {
        self.mid() + self.tangent * t + self.out * d
    }

    pub fn yaw(&self) -> f32 {
        self.tangent.y.atan2(self.tangent.x)
    }

    /// Box centred `t` along the face, `d` outward, at height `y`; `size.x`
    /// runs along the face and `size.z` through it.
    #[allow(clippy::too_many_arguments)]
    pub fn bx(&self, g: &mut Geometry, f: &Frame, s: Surface, t: f32, y: f32, d: f32, size: Vec3, color: [f32; 3]) {
        let p = self.at(t, d);
        f.bx_yaw(g, s, p.x, y, p.y, size, color, self.yaw());
    }

    /// One quad `w` wide and `h` high, `d` proud of the face and facing the
    /// way the face does: panes, frames, signs, stains, anything flat that
    /// is only ever seen from the front. See [`parts::decal`].
    #[allow(clippy::too_many_arguments)]
    pub fn decal(&self, g: &mut Geometry, f: &Frame, s: Surface, t: f32, y: f32, d: f32, w: f32, h: f32, color: [f32; 3]) {
        parts::decal(g, f, s, self, t, y, d, w, h, color);
    }
}

/// Everything a style generator needs.
pub struct Ctx<'a> {
    pub frame: Frame,
    pub rules: &'a LandUseRules,
    pub style: &'a TierStyle,
    pub palette: &'a Palette,
    pub land_use: LandUse,
    pub tier: Tier,
    pub floors: u8,
    /// 0 = pristine, 1 = ruined.
    pub wear: f32,
    /// Bounding-rectangle footprint size after inset.
    pub fw: f32,
    pub fd: f32,
    /// Exact parcel outline in lot coordinates, counter-clockwise.
    pub lot_poly: Vec<Vec2>,
    /// Buildable footprint: the parcel inset by the land use's setback.
    pub poly: Vec<Vec2>,
    pub seed: u64,
    /// For parking lots: share of bays holding a car.
    pub parking: Option<f32>,
    /// Share of the building's electricity demand met: dark windows when low.
    pub powered: f32,
    /// For parks: the sim's plan of paths, benches and features.
    pub park: Option<ParkLayout>,
    /// For stations and yards: the track and platform beside the lot.
    pub rail: Option<RailLink>,
    /// Wind farm rotors are left out for the renderer to spin.
    pub animated_rotors: bool,
    /// Window lights are switched by the renderer from occupancy: nearly
    /// every window is drawn as a marked light over its glass (see
    /// `parts::window_light`), and `lit_chance` ignores power.
    pub dynamic_lights: bool,
}

/// Per-building state that changes the look beyond style and condition.
#[derive(Clone, Copy, Debug)]
pub struct Extras<'a> {
    pub parking: Option<f32>,
    pub plant: Option<PowerPlantKind>,
    pub powered: f32,
    pub park: Option<&'a ParkLayout>,
    /// A bus depot.
    pub depot: bool,
    pub rail: Option<&'a RailLink>,
    /// A shipping port or ferry terminal.
    pub port: Option<nd_sim::ports::PortKind>,
    /// Still being built: progress in `0..1`. The site is drawn instead of
    /// the finished style.
    pub construction: Option<f32>,
    /// Coming down: progress in `0..1`. The building (as described by the
    /// other arguments) is drawn cut down to what is left, with the
    /// demolition site around it.
    pub demolition: Option<f32>,
    /// Styles of the buildings already standing nearby (see
    /// [`StyleBook`]); the style pick clusters or contrasts with them.
    pub neighbours: &'a [Neighbour],
    /// A city service or attraction: its own style, at the placed tier.
    pub service: Option<nd_sim::services::ServiceKind>,
    /// Leave wind farm rotors out: the renderer draws and spins them.
    pub animated_rotors: bool,
    /// Draw window lights for the renderer to switch by occupancy (see
    /// [`Ctx::dynamic_lights`]).
    pub dynamic_lights: bool,
    /// The sim gave this tower a rooftop pad (`nd_sim::air`): a pad-capable
    /// style is picked and a helideck drawn on its roof.
    pub rooftop_pad: bool,
    /// The business on the lot: industry sub-zones have their own looks.
    pub business: Option<nd_sim::BusinessKind>,
    /// A truck and bus lot: share of its bays in use.
    pub fleet: f32,
}

impl Default for Extras<'_> {
    fn default() -> Self {
        Self { parking: None, plant: None, powered: 1.0, park: None, depot: false, rail: None, port: None, construction: None, demolition: None, neighbours: &[], service: None, animated_rotors: false, dynamic_lights: false, business: None, fleet: 0.5, rooftop_pad: false }
    }
}

impl Ctx<'_> {
    pub fn fh(&self) -> f32 {
        self.rules.floor_height
    }

    pub fn height(&self) -> f32 {
        self.floors.max(1) as f32 * self.fh()
    }

    /// Chance a window is lit, clamped so rule files cannot push it past 1.
    pub fn lit_chance(&self) -> f32 {
        if self.dynamic_lights {
            // Most windows can light; which ones do is the renderer's call.
            return (0.92 * (1.0 - 0.3 * self.wear)).clamp(0.0, 1.0);
        }
        (self.style.lit_chance * (1.0 - 0.8 * self.wear) * self.powered.clamp(0.08, 1.0)).clamp(0.0, 1.0)
    }

    /// Whether a lot-local point lies on the parcel. World points go through
    /// `Frame::to_local` first (or `park::Park::inside_world`).
    pub fn inside_local(&self, p: Vec2) -> bool {
        point_in_polygon(p, &self.lot_poly)
    }

    /// Distance from the middle of a face, outward, to the lot boundary: the
    /// room a canopy, porch or forecourt has before it spills onto the street.
    pub fn reach_out(&self, face: &Face) -> f32 {
        let o = face.mid();
        let d = face.out;
        let n = self.lot_poly.len();
        let mut best = f32::MAX;
        for i in 0..n {
            let a = self.lot_poly[i];
            let b = self.lot_poly[(i + 1) % n];
            let e = b - a;
            let denom = d.perp_dot(e);
            if denom.abs() < 1e-6 {
                continue;
            }
            let t = (a - o).perp_dot(e) / denom;
            let u = (a - o).perp_dot(d) / denom;
            if t >= 0.0 && (0.0..=1.0).contains(&u) {
                best = best.min(t);
            }
        }
        if best == f32::MAX { 0.0 } else { best }
    }

    /// Faces of a lot-local polygon.
    pub fn faces(&self, poly: &[Vec2]) -> Vec<Face> {
        let n = poly.len();
        (0..n).map(|i| Face::from_edge(poly[i], poly[(i + 1) % n])).collect()
    }

    /// Index of the face of a polygon that looks at the road: the longest
    /// edge facing -v. Fences and hoardings leave that edge for the gate.
    pub fn front_face_index(&self, poly: &[Vec2]) -> Option<usize> {
        let faces = self.faces(poly);
        (0..faces.len()).max_by(|&i, &j| {
            let si = faces[i].out.dot(Vec2::new(0.0, -1.0)) * faces[i].len;
            let sj = faces[j].out.dot(Vec2::new(0.0, -1.0)) * faces[j].len;
            si.total_cmp(&sj)
        })
    }

    /// The face of a polygon that looks at the road.
    pub fn front_face(&self, poly: &[Vec2]) -> Face {
        let faces = self.faces(poly);
        self.front_face_index(poly).map(|i| faces[i]).unwrap_or(Face::from_edge(Vec2::new(-1.0, -1.0), Vec2::new(1.0, -1.0)))
    }

    /// Random point inside the parcel at least `margin` from its edges.
    pub fn random_inside(&self, rng: &mut impl Rng, margin: f32) -> Option<Vec2> {
        parts::random_in_poly(rng, &self.lot_poly, margin)
    }
}

#[allow(clippy::too_many_arguments)]
pub fn generate_building(
    lot: &Lot,
    land_use: LandUse,
    floors: u8,
    tier: Tier,
    condition: f32,
    seed: u64,
    rules: &BuildingRules,
    palette: &Palette,
    extras: &Extras,
) -> Geometry {
    let style = choose_style(lot, land_use, floors, tier, seed, rules, extras);
    generate_styled_extras(lot, land_use, floors, tier, condition, seed, style, rules, palette, extras)
}

/// The style `generate_building` draws for these inputs: forced by the
/// extras for plants, car parks, depots and rail, otherwise picked by weight,
/// site and neighbours. The [`StyleBook`] calls this too, so the style it
/// records for a building is the one that is drawn.
#[allow(clippy::too_many_arguments)]
pub fn choose_style(lot: &Lot, land_use: LandUse, floors: u8, tier: Tier, seed: u64, rules: &BuildingRules, extras: &Extras) -> Style {
    let mut rng = rng_for(seed, 11);
    match (extras.plant, extras.parking) {
        _ if extras.service.is_some() => Style::for_service(extras.service.unwrap()),
        _ if extras.depot => Style::BusDepot,
        _ if extras.port == Some(nd_sim::ports::PortKind::ShippingPort) => Style::ShippingPort,
        _ if extras.port == Some(nd_sim::ports::PortKind::FerryTerminal) => Style::FerryTerminal,
        _ if extras.rail.is_some_and(|r| r.kind == RailFacility::Station) => Style::Station,
        _ if extras.rail.is_some_and(|r| r.kind == RailFacility::FreightYard) => Style::FreightYard,
        (Some(PowerPlantKind::Coal), _) => Style::PowerCoal,
        (Some(PowerPlantKind::Solar), _) => Style::PowerSolar,
        (Some(PowerPlantKind::Wind), _) => Style::PowerWind,
        (Some(PowerPlantKind::Nuclear), _) => Style::PowerNuclear,
        (Some(PowerPlantKind::Hydro), _) => Style::PowerHydro,
        (Some(PowerPlantKind::Tidal), _) => Style::PowerTidal,
        (Some(PowerPlantKind::Geothermal), _) => Style::PowerGeothermal,
        (None, Some(_)) if tier == Tier::Low || floors < 2 => Style::ParkingLot,
        (None, Some(_)) => Style::CarPark,
        (None, None) if extras.business.is_some_and(|k| industry_styles(k, tier).is_some()) => {
            let options = industry_styles(extras.business.unwrap(), tier).unwrap();
            pick_industry_style(options, extras.neighbours, &mut rng)
        }
        (None, None) => {
            let site = adjacency::Site { floors, frontage: lot.width() };
            let mut style = rules.pick_style_near(land_use, tier, &site, extras.neighbours, &mut rng);
            // A tower with a pad is one that can carry a helideck.
            let mut tries = 0;
            while extras.rooftop_pad && !style.pad_capable() && tries < 12 {
                style = rules.pick_style_near(land_use, tier, &site, extras.neighbours, &mut rng);
                tries += 1;
            }
            if extras.rooftop_pad && !style.pad_capable() {
                style = if land_use == LandUse::Commercial { Style::SlabTower } else { Style::Crystal };
            }
            style
        }
    }
}

/// The looks of an industry business, weighted, or `None` to draw from the
/// land use's own menu (parts fabs keep the generic industrial styles).
pub fn industry_styles(kind: nd_sim::BusinessKind, tier: Tier) -> Option<&'static [(Style, f32)]> {
    use nd_sim::BusinessKind as B;
    Some(match kind {
        B::GrainFarm | B::Dairy | B::Mill => &[(Style::Farmstead, 1.0)],
        B::VertFarm => &[(Style::Farmstead, 0.6), (Style::FlattedFactory, 0.4)],
        B::ProteinVat => &[(Style::TankFarm, 0.6), (Style::Farmstead, 0.4)],
        B::OilWell => &[(Style::OilField, 1.0)],
        B::Refinery => &[(Style::Refinery, 0.8), (Style::TankFarm, 0.2)],
        B::ContainerYard => &[(Style::ContainerYard, 0.7), (Style::Depot, 0.3)],
        B::FleetLot => &[(Style::TruckLot, 1.0)],
        B::Fishery | B::FishPlant => &[(Style::FishProcessing, 1.0)],
        B::Assembly => match tier {
            Tier::Low => &[(Style::FlattedFactory, 0.6), (Style::Workshops, 0.4)],
            _ => &[(Style::FabPlant, 0.6), (Style::Datacenter, 0.4)],
        },
        _ => return None,
    })
}

/// Weighted pick among an industry's looks, clustering by neighbours.
fn pick_industry_style(options: &[(Style, f32)], near: &[Neighbour], rng: &mut impl Rng) -> Style {
    let weights: Vec<f32> = options.iter().map(|(s, w)| w * adjacency::neighbour_factor(*s, near)).collect();
    let total: f32 = weights.iter().sum();
    if total <= 0.0 || !total.is_finite() {
        return options[0].0;
    }
    let mut r = rng.random_range(0.0..total);
    for ((s, _), w) in options.iter().zip(&weights) {
        if r < *w {
            return *s;
        }
        r -= w;
    }
    options[options.len() - 1].0
}

/// Generate a specific style; used by the tests and the style browser.
#[allow(clippy::too_many_arguments)]
pub fn generate_styled(
    lot: &Lot,
    land_use: LandUse,
    floors: u8,
    tier: Tier,
    condition: f32,
    seed: u64,
    style: Style,
    rules: &BuildingRules,
    palette: &Palette,
) -> Geometry {
    generate_styled_extras(lot, land_use, floors, tier, condition, seed, style, rules, palette, &Extras::default())
}

#[allow(clippy::too_many_arguments)]
fn generate_styled_extras(
    lot: &Lot,
    land_use: LandUse,
    floors: u8,
    tier: Tier,
    condition: f32,
    seed: u64,
    style: Style,
    rules: &BuildingRules,
    palette: &Palette,
    extras: &Extras,
) -> Geometry {
    // A truck lot shows its fleet the way a car park shows its cars.
    let parking = extras.parking.or((style == Style::TruckLot).then_some(extras.fleet));
    let mut g = Geometry::default();
    let mut rng = rng_for(seed, 13);
    let frame = Frame::from_lot(lot);
    let r = rules.for_land_use(land_use);
    let mut lot_poly: Vec<Vec2> = parts::clean_poly(&lot.polygon.iter().map(|p| frame.to_local(*p)).collect::<Vec<_>>(), 0.3);
    if lot_poly.len() < 3 {
        let (hw, hd) = (frame.width * 0.5, frame.depth * 0.5);
        lot_poly = vec![Vec2::new(-hw, -hd), Vec2::new(hw, -hd), Vec2::new(hw, hd), Vec2::new(-hw, hd)];
    }
    if polygon_area(&lot_poly) < 0.0 {
        lot_poly.reverse();
    }
    let poly = parts::offset_poly(&lot_poly, r.inset);
    let ctx = Ctx {
        fw: (frame.width - 2.0 * r.inset).max(4.0),
        fd: (frame.depth - 2.0 * r.inset).max(4.0),
        frame,
        rules: r,
        style: rules.style(tier),
        palette,
        land_use,
        tier,
        floors: floors.max(1),
        wear: (1.0 - condition).clamp(0.0, 1.0),
        lot_poly,
        poly,
        seed,
        parking,
        powered: extras.powered,
        rail: extras.rail.cloned().or_else(|| matches!(style, Style::Station | Style::FreightYard).then(|| back_edge_link(lot, if style == Style::Station { RailFacility::Station } else { RailFacility::FreightYard }))),
        park: if land_use == LandUse::Park || matches!(style, Style::ScrubLot | Style::Garden | Style::Plaza) { Some(extras.park.cloned().unwrap_or_else(|| nd_sim::park::plan_single(lot, tier))) } else { None },
        animated_rotors: extras.animated_rotors,
        dynamic_lights: extras.dynamic_lights,
    };
    if let Some(progress) = extras.demolition {
        let mut g = generate_styled_extras(lot, land_use, floors, tier, condition, seed, style, rules, palette, &Extras { demolition: None, construction: None, ..*extras });
        construction::demolition_site(&mut g, &ctx, &mut rng, progress);
        return g;
    }
    if let Some(progress) = extras.construction {
        construction::construction_site(&mut g, &ctx, &mut rng, progress);
        if weathered(tier, ctx.wear) {
            g.weather();
        }
        return g;
    }
    match style {
        Style::Stacks => residential::stacks(&mut g, &ctx, &mut rng),
        Style::Tenement => residential::tenement(&mut g, &ctx, &mut rng),
        Style::Brutalist => residential::brutalist(&mut g, &ctx, &mut rng),
        Style::BalconySlab => residential::balcony_slab(&mut g, &ctx, &mut rng),
        Style::Courtyard => residential::courtyard(&mut g, &ctx, &mut rng),
        Style::Crystal => residential::crystal(&mut g, &ctx, &mut rng),
        Style::Fortress => towers::glass_castle(&mut g, &ctx, &mut rng),
        Style::GardenTower => residential::garden_tower(&mut g, &ctx, &mut rng),
        Style::Villas => residential::villas(&mut g, &ctx, &mut rng),
        Style::Shophouse => commercial::shophouse(&mut g, &ctx, &mut rng),
        Style::MarketHall => commercial::market_hall(&mut g, &ctx, &mut rng),
        Style::RetailBlock => commercial::retail_block(&mut g, &ctx, &mut rng),
        Style::PodiumOffice => commercial::podium_office(&mut g, &ctx, &mut rng),
        Style::SlabTower => commercial::slab_tower(&mut g, &ctx, &mut rng),
        Style::TwistTower => commercial::twist_tower(&mut g, &ctx, &mut rng),
        Style::CantileverMall => commercial::cantilever_mall(&mut g, &ctx, &mut rng),
        Style::Junkyard => industrial::junkyard(&mut g, &ctx, &mut rng),
        Style::RustyWarehouse => industrial::rusty_warehouse(&mut g, &ctx, &mut rng),
        Style::Factory => industrial::factory(&mut g, &ctx, &mut rng),
        Style::TankFarm => industrial::tank_farm(&mut g, &ctx, &mut rng),
        Style::Datacenter => industrial::datacenter(&mut g, &ctx, &mut rng),
        Style::Depot => industrial::depot(&mut g, &ctx, &mut rng),
        Style::ShackBar => entertainment::shack_bar(&mut g, &ctx, &mut rng),
        Style::Club => entertainment::club(&mut g, &ctx, &mut rng),
        Style::Cinema => entertainment::cinema(&mut g, &ctx, &mut rng),
        Style::Casino => entertainment::casino(&mut g, &ctx, &mut rng),
        Style::Arena => entertainment::arena(&mut g, &ctx, &mut rng),
        Style::Precinct => civic::precinct(&mut g, &ctx, &mut rng),
        Style::Ministry => civic::ministry(&mut g, &ctx, &mut rng),
        Style::Library => civic::library(&mut g, &ctx, &mut rng),
        Style::Courthouse => civic::courthouse(&mut g, &ctx, &mut rng),
        Style::Monument => civic::monument(&mut g, &ctx, &mut rng),
        Style::ScrubLot => park::scrub_lot(&mut g, &ctx, &mut rng),
        Style::Garden => park::garden(&mut g, &ctx, &mut rng),
        Style::Plaza => park::plaza(&mut g, &ctx, &mut rng),
        Style::ParkingLot => parking::parking_lot(&mut g, &ctx, &mut rng),
        Style::CarPark => parking::car_park(&mut g, &ctx, &mut rng),
        Style::PowerCoal => power::coal(&mut g, &ctx, &mut rng),
        Style::PowerSolar => power::solar(&mut g, &ctx, &mut rng),
        Style::PowerWind => power::wind(&mut g, &ctx, &mut rng),
        Style::PowerNuclear => power::nuclear(&mut g, &ctx, &mut rng),
        Style::BusDepot => transit::bus_depot(&mut g, &ctx, &mut rng),
        Style::Station => rail::station(&mut g, &ctx, &mut rng),
        Style::FreightYard => rail::freight_yard(&mut g, &ctx, &mut rng),
        Style::WalledCity => dense::walled_city(&mut g, &ctx, &mut rng),
        Style::PencilTower => towers::pencil_tower(&mut g, &ctx, &mut rng),
        Style::TwinSlab => towers::twin_slab(&mut g, &ctx, &mut rng),
        Style::Megastructure => towers::megastructure(&mut g, &ctx, &mut rng),
        Style::GlassTerraces => towers::glass_terraces(&mut g, &ctx, &mut rng),
        Style::CapsuleHotel => dense::capsule_hotel(&mut g, &ctx, &mut rng),
        Style::HillsideSteps => dense::hillside_steps(&mut g, &ctx, &mut rng),
        Style::NightMarket => dense::night_market(&mut g, &ctx, &mut rng),
        Style::NeonTower => dense::neon_tower(&mut g, &ctx, &mut rng),
        Style::Workshops => works::workshops(&mut g, &ctx, &mut rng),
        Style::FlattedFactory => works::flatted_factory(&mut g, &ctx, &mut rng),
        Style::FabPlant => works::fab_plant(&mut g, &ctx, &mut rng),
        Style::PowerHydro => waterworks::hydro(&mut g, &ctx, &mut rng),
        Style::PowerTidal => waterworks::tidal(&mut g, &ctx, &mut rng),
        Style::PowerGeothermal => waterworks::geothermal(&mut g, &ctx, &mut rng),
        Style::ShippingPort => waterworks::shipping_port(&mut g, &ctx, &mut rng),
        Style::FerryTerminal => waterworks::ferry_terminal(&mut g, &ctx, &mut rng),
        Style::PoliceStation => services::police_station(&mut g, &ctx, &mut rng),
        Style::FireStation => services::fire_station(&mut g, &ctx, &mut rng),
        Style::Hospital => services::hospital(&mut g, &ctx, &mut rng),
        Style::School => services::school(&mut g, &ctx, &mut rng),
        Style::DispatchCenter => services::dispatch_center(&mut g, &ctx, &mut rng),
        Style::Landmark => services::landmark(&mut g, &ctx, &mut rng),
        Style::Stadium => services::stadium(&mut g, &ctx, &mut rng),
        Style::Prison => services::prison(&mut g, &ctx, &mut rng),
        Style::Farmstead => industrial::farmstead(&mut g, &ctx, &mut rng),
        Style::OilField => industrial::oil_field(&mut g, &ctx, &mut rng),
        Style::Refinery => industrial::refinery(&mut g, &ctx, &mut rng),
        Style::ContainerYard => industrial::container_yard(&mut g, &ctx, &mut rng),
        Style::FishProcessing => industrial::fish_processing(&mut g, &ctx, &mut rng),
        Style::TruckLot => industrial::truck_lot(&mut g, &ctx, &mut rng),
        Style::TrashDepot => sanitation::trash_depot(&mut g, &ctx, &mut rng),
        Style::Vertiport => air::vertiport(&mut g, &ctx, &mut rng),
    }
    if extras.rooftop_pad && style.pad_capable() && g.pads.is_empty() {
        air::rooftop_deck(&mut g, &ctx);
    }
    if weathered(tier, ctx.wear) {
        g.weather();
    }
    g
}

/// Whether a building is drawn with the weathered materials: everything
/// cheap (low land value builds low tier), and anything left to decay.
pub fn weathered(tier: Tier, wear: f32) -> bool {
    match tier {
        Tier::Low => true,
        Tier::Mid => wear > 0.45,
        Tier::High => wear > 0.7,
    }
}

/// A stand-in track along the back of a lot, for previews and tests of the
/// station and yard styles when no railway is near.
fn back_edge_link(lot: &Lot, kind: RailFacility) -> RailLink {
    let a = lot.corners[3];
    let b = lot.corners[2];
    let d = (b - a).normalize_or_zero();
    let back = (a + b) * 0.5 + lot.front_dir * -4.0;
    let half = (a.distance(b) * 0.45).min(40.0);
    // `perp(along) * side` is the direction from the track to the building:
    // here, from behind the lot towards the road.
    let side = if Vec2::new(-d.y, d.x).dot(lot.front_dir) >= 0.0 { 1.0 } else { -1.0 };
    RailLink { kind, segment: nd_sim::SegmentId(0), platform: [back - d * half, back + d * half], side, rail_connected: true, road_connected: true }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nd_sim::roads::RoadNetwork;
    use nd_sim::zoning::{generate_lots, Zone, ZoneId};
    use slotmap::SlotMap;

    fn lots(poly: Vec<Vec2>, lu: LandUse) -> Vec<Lot> {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-300.0, -40.0), Vec2::new(300.0, -40.0)], 1, 1);
        roads.add_polyline(&[Vec2::new(-120.0, -300.0), Vec2::new(60.0, 300.0)], 1, 1);
        let zone = Zone { polygon: poly, land_use: lu };
        let mut lots = SlotMap::with_key();
        generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
        lots.values().cloned().collect()
    }

    fn check_mesh(g: &Geometry, what: &str) {
        for (s, m) in g.iter() {
            assert_eq!(m.positions.len(), m.uvs.len(), "{what} {s:?} uv count");
            assert_eq!(m.positions.len(), m.tangents.len(), "{what} {s:?} tangent count");
            assert!(m.positions.iter().all(|p| p.iter().all(|v| v.is_finite())), "{what} {s:?} has NaN");
            assert!(m.normals.iter().all(|p| p.iter().all(|v| v.is_finite())), "{what} {s:?} has NaN normals");
            assert!(m.uvs.iter().all(|p| p.iter().all(|v| v.is_finite())), "{what} {s:?} has NaN uvs");
            assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()), "{what} {s:?} bad index");
        }
    }

    #[test]
    fn every_style_builds_on_odd_parcels_without_nans() {
        let rules = BuildingRules::default();
        let pal = Palette::default();
        let zone = vec![Vec2::new(-100.0, -20.0), Vec2::new(100.0, -20.0), Vec2::new(100.0, 120.0), Vec2::new(-100.0, 120.0)];
        for lu in LandUse::ALL {
            let lots = lots(zone.clone(), lu);
            assert!(!lots.is_empty());
            let odd = lots.iter().find(|l| l.polygon.len() != 4).unwrap_or(&lots[0]);
            for style in Style::ALL {
                for (lot, floors) in [(&lots[0], 6u8), (odd, 2u8)] {
                    for tier in [Tier::Low, Tier::Mid, Tier::High] {
                        let g = generate_styled(lot, lu, floors, tier, 0.7, 42, style, &rules, &pal);
                        assert!(g.triangle_count() > 0, "{style:?} produced nothing");
                        check_mesh(&g, &format!("{style:?}"));
                    }
                }
            }
        }
    }

    /// Lots from a zone cut by straight, diagonal and curved roads.
    fn many_lots(lu: LandUse) -> Vec<Lot> {
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-400.0, -40.0), Vec2::new(400.0, -40.0)], 1, 1);
        roads.add_polyline(&[Vec2::new(-120.0, -300.0), Vec2::new(60.0, 300.0)], 1, 1);
        roads.add_polyline(&[Vec2::new(-400.0, 200.0), Vec2::new(400.0, 200.0)], 1, 1);
        let curve: Vec<Vec2> = (0..=20).map(|i| {
            let t = i as f32 / 20.0;
            Vec2::new(-300.0 + 600.0 * t, 80.0 + 60.0 * (t * std::f32::consts::TAU).sin())
        }).collect();
        roads.add_polyline(&curve, 1, 1);
        let zone = Zone { polygon: vec![Vec2::new(-300.0, -20.0), Vec2::new(300.0, -20.0), Vec2::new(300.0, 180.0), Vec2::new(-300.0, 180.0)], land_use: lu };
        let mut lots = SlotMap::with_key();
        generate_lots(ZoneId::default(), &zone, &roads, &mut lots, &[]);
        lots.values().filter(|l| l.usable()).cloned().collect()
    }

    fn frame_poly(f: &Frame, lot: &Lot) -> Vec<Vec2> {
        lot.polygon.iter().map(|p| f.to_local(*p)).collect()
    }

    /// How far a world point lies outside the lot's outline (zero inside).
    /// The outline, not its bounding box: a triangular corner lot has a lot
    /// of box that is somebody else's land.
    fn outside_by(q: Vec2, lot: &Lot) -> f32 {
        if point_in_polygon(q, &lot.polygon) { 0.0 } else { parts::distance_to_boundary(q, &lot.polygon) }
    }

    /// Distance from a point to the track corridor a station or yard was
    /// given: the platform segment widened by `half`.
    fn outside_corridor(q: Vec2, link: &RailLink, half: f32) -> f32 {
        let (_, p) = nd_sim::roads::project_on_segment(q, link.platform[0], link.platform[1]);
        (q.distance(p) - half).max(0.0)
    }

    /// Worst overshoot, lowest and highest vertex of a building, with a
    /// description of the worst offender.
    fn extent(g: &Geometry, lot: &Lot, link: Option<&RailLink>) -> (f32, f32, f32, String) {
        let frame = Frame::from_lot(lot);
        let (mut over, mut top, mut low) = (0.0f32, 0.0f32, 0.0f32);
        let mut culprit = String::new();
        for (surface, m) in g.iter() {
            for p in &m.positions {
                let q = Vec2::new(p[0], p[2]);
                let mut d = outside_by(q, lot);
                if let Some(l) = link {
                    // Platforms, sidings, apron and the gantry reach out to
                    // the track by design; they must stay in its corridor.
                    let half = if l.kind == RailFacility::FreightYard { 19.0 } else { 10.0 };
                    d = d.min(outside_corridor(q, l, half));
                }
                if d > over {
                    over = d;
                    let l = frame.to_local(q);
                    culprit = format!("{surface:?} at local ({:.1}, {:.1}) y {:.1}", l.x, l.y, p[1]);
                }
                if p[1] < low {
                    low = p[1];
                    if over <= 0.0 {
                        let l = frame.to_local(q);
                        culprit = format!("{surface:?} at local ({:.1}, {:.1}) y {:.1}", l.x, l.y, p[1]);
                    }
                }
                top = top.max(p[1]);
            }
        }
        (over, top, low, culprit)
    }

    /// Triangles a style may spend on a lot: a base, a share per floor for
    /// facades, and a share per square metre for yards and parks.
    fn budget(floors: u8, lot: &Lot) -> usize {
        4000 + 300 * floors as usize + 2 * polygon_area(&lot.polygon).abs() as usize
    }

    /// Building sites at every stage stay on their lot and are well formed.
    #[test]
    fn construction_sites_stay_on_their_lot() {
        let rules = BuildingRules::default();
        let pal = Palette::default();
        let mut worst: Vec<String> = Vec::new();
        for lu in LandUse::ALL {
            let lots = many_lots(lu);
            for (li, lot) in lots.iter().enumerate() {
                for (seed, floors, tier) in [(1u64, 2u8, Tier::Low), (7, 9, Tier::Mid), (13, 30, Tier::High)] {
                    for progress in [0.0f32, 0.05, 0.3, 0.55, 0.8, 0.96, 1.0] {
                        let extras = Extras { construction: Some(progress), ..Extras::default() };
                        let g = generate_building(lot, lu, floors, tier, 1.0, seed + li as u64, &rules, &pal, &extras);
                        assert!(g.triangle_count() > 0, "site at {progress} on {lu:?} lot {li} drew nothing");
                        check_mesh(&g, &format!("site at {progress} on {lu:?} lot {li}"));
                        let (over, top, low, culprit) = extent(&g, lot, None);
                        // Scaffolding and hoarding are dense, and a site is temporary: it
                        // gets twice the finished building's budget.
                        let site_budget = budget(floors, lot) * 2;
                        if over > 1.0 || top > (60.0 + floors as f32 * 5.0).max(90.0) || low < -0.3 || g.triangle_count() > site_budget {
                            let frame = Frame::from_lot(lot);
                            let (plo, phi) = parts::bounds(&frame_poly(&frame, lot));
                            worst.push(format!("{lu:?} lot {li} floors {floors} at {progress}: overshoots {over:.1} m, top {top:.0} m, low {low:.1}, {} tris ({} verts, local {:.0}..{:.0} x {:.0}..{:.0}): {culprit}", g.triangle_count(), lot.polygon.len(), plo.x, phi.x, plo.y, phi.y));
                        }
                    }
                }
            }
        }
        assert!(worst.is_empty(), "{} sites leave their lot or budget:\n{}", worst.len(), worst.join("\n"));
    }

    /// Demolition sites at every stage stay on their lot, are well formed,
    /// and come down: by the end nothing taller than the rubble is left.
    #[test]
    fn demolition_sites_stay_on_their_lot() {
        let rules = BuildingRules::default();
        let pal = Palette::default();
        let mut worst: Vec<String> = Vec::new();
        for lu in LandUse::ALL {
            let lots = many_lots(lu);
            for (li, lot) in lots.iter().enumerate() {
                for (seed, floors, tier) in [(1u64, 2u8, Tier::Low), (7, 9, Tier::Mid), (13, 30, Tier::High)] {
                    for progress in [0.0f32, 0.05, 0.2, 0.45, 0.7, 0.85, 0.99, 1.0] {
                        let extras = Extras { demolition: Some(progress), ..Extras::default() };
                        let g = generate_building(lot, lu, floors, tier, 1.0, seed + li as u64, &rules, &pal, &extras);
                        let what = format!("demolition at {progress} on {lu:?} lot {li} floors {floors}");
                        assert!(g.triangle_count() > 0, "{what} drew nothing");
                        check_mesh(&g, &what);
                        let (over, top, low, culprit) = extent(&g, lot, None);
                        if over > 1.0 || top > (60.0 + floors as f32 * 5.0).max(90.0) || low < -0.3 || g.triangle_count() > budget(floors, lot) * 2 {
                            worst.push(format!("{what}: overshoots {over:.1} m, top {top:.0} m, low {low:.1}, {} tris: {culprit}", g.triangle_count()));
                        }
                        // Machines and cranes come and go, but once the
                        // structure is down only the site is left.
                        if progress >= 0.85 && top > 8.0 {
                            worst.push(format!("{what}: still {top:.1} m tall after the structure came down"));
                        }
                    }
                }
            }
        }
        assert!(worst.is_empty(), "{} demolition sites misbehave:
{}", worst.len(), worst.join("
"));
    }

    /// A building cut part way down keeps no triangle above the cut, and
    /// what is below is untouched.
    #[test]
    fn clipping_drops_everything_above_the_cut() {
        let rules = BuildingRules::default();
        let pal = Palette::default();
        let lot = &many_lots(LandUse::Commercial)[0];
        let mut g = generate_building(lot, LandUse::Commercial, 9, Tier::Mid, 1.0, 7, &rules, &pal, &Extras::default());
        let below = |g: &Geometry, y: f32| g.iter().flat_map(|(_, m)| m.indices.chunks_exact(3).filter(move |t| t.iter().all(|&i| m.positions[i as usize][1] <= y)).map(|_| ()).collect::<Vec<_>>()).count();
        let low_before = below(&g, 5.0);
        let points = construction::clip_above(&mut g, 12.0);
        assert!(!points.is_empty(), "walls were cut");
        let top = g.iter().flat_map(|(_, m)| m.positions.iter().map(|p| p[1])).fold(0.0f32, f32::max);
        assert!(top <= 12.0 + 1e-3, "nothing above the cut: {top}");
        assert_eq!(below(&g, 5.0), low_before, "the lower floors are untouched");
        check_mesh(&g, "clipped building");
    }

    /// Nothing a building generates may reach beyond its own lot outline
    /// (a short way for canopies and kerbside clutter), dig into the
    /// ground, or blow its triangle budget.
    #[test]
    fn geometry_stays_near_its_lot() {
        let rules = BuildingRules::default();
        let pal = Palette::default();
        let mut worst: Vec<(f32, String)> = Vec::new();
        for lu in LandUse::ALL {
            let lots = many_lots(lu);
            assert!(lots.len() > 10, "{lu:?}: {} lots", lots.len());
            for (li, lot) in lots.iter().enumerate() {
                let span = { let (lo, hi) = parts::bounds(&lot.polygon); (hi - lo).max_element() };
                for style in Style::ALL {
                    for (seed, floors, tier) in [(1u64, 2u8, Tier::Low), (7, 9, Tier::Mid), (13, 30, Tier::High)] {
                        let g = generate_styled(lot, lu, floors, tier, 0.6, seed + li as u64, style, &rules, &pal);
                        let link = match style {
                            Style::Station => Some(back_edge_link(lot, RailFacility::Station)),
                            Style::FreightYard => Some(back_edge_link(lot, RailFacility::FreightYard)),
                            _ => None,
                        };
                        let (over, top, low, culprit) = extent(&g, lot, link.as_ref());
                        let tris = g.triangle_count();
                        let allow = 1.0;
                        if over > allow || top > (60.0 + floors as f32 * 5.0).max(90.0) || low < -0.3 || tris > budget(floors, lot) {
                            let frame = Frame::from_lot(lot);
                            let (plo, phi) = parts::bounds(&frame_poly(&frame, lot));
                            worst.push((over.max(-low).max((tris as f32 - budget(floors, lot) as f32) / 1000.0), format!("{style:?} {lu:?} lot {li} ({} verts, span {span:.0}, local {:.0}..{:.0} x {:.0}..{:.0}) overshoots {over:.1} m, top {top:.0} m, low {low:.1}, {tris} tris of {}, floors {floors}: {culprit}", lot.polygon.len(), plo.x, phi.x, plo.y, phi.y, budget(floors, lot))));
                        }
                    }
                }
            }
        }
        worst.sort_by(|a, b| b.0.total_cmp(&a.0));
        // One line per style: its worst case and how many lots it fails on.
        let mut seen: Vec<String> = Vec::new();
        for (_, w) in &worst {
            let style = w.split(' ').next().unwrap().to_string();
            if !seen.contains(&style) {
                let n = worst.iter().filter(|x| x.1.starts_with(&format!("{style} "))).count();
                eprintln!("{n:4} x {w}");
                seen.push(style);
            }
        }
        assert!(worst.is_empty(), "{} style/lot combinations reach beyond their lot or budget", worst.len());
    }

    /// A tower the sim gives a rooftop pad draws a helideck on its highest
    /// roof, clear of everything on it, without leaving its lot or budget;
    /// an unflagged tower draws none, and a flagged one always picks a
    /// style that can carry a deck.
    #[test]
    fn rooftop_pads_sit_on_tower_roofs_and_stay_on_the_lot() {
        let rules = BuildingRules::default();
        let pal = Palette::default();
        let (mut tried, mut drawn) = (0usize, 0usize);
        let mut misses: Vec<String> = Vec::new();
        let mut bad: Vec<String> = Vec::new();
        for lu in [LandUse::Residential, LandUse::Commercial] {
            let lots = many_lots(lu);
            for (li, lot) in lots.iter().enumerate().take(36) {
                for style in Style::ALL.into_iter().filter(|s| s.pad_capable()) {
                    let floors = 14u8;
                    let seed = 3 + li as u64;
                    let plain = generate_styled(lot, lu, floors, Tier::High, 1.0, seed, style, &rules, &pal);
                    // Some towers carry a helipad of their own (corporate slabs).
                    let built_in = plain.pads.len();
                    let padded = generate_styled_extras(lot, lu, floors, Tier::High, 1.0, seed, style, &rules, &pal, &Extras { rooftop_pad: true, ..Extras::default() });
                    check_mesh(&padded, "rooftop pad");
                    tried += 1;
                    let (over, top, low, culprit) = extent(&padded, lot, None);
                    if over > 1.0 || low < -0.3 || top > (60.0 + floors as f32 * 5.0).max(90.0) || padded.triangle_count() > budget(floors, lot) {
                        bad.push(format!("{style:?} lot {li}: over {over:.1} top {top:.0} low {low:.1} tris {}: {culprit}", padded.triangle_count()));
                    }
                    assert!(padded.pads.len() <= built_in + 1, "{style:?}: one deck at most");
                    let Some(p) = padded.pads.last().copied() else {
                        misses.push(format!("{style:?} lot {li}"));
                        continue;
                    };
                    drawn += 1;
                    assert!(p.y > 8.0, "{style:?} lot {li}: the deck is up on a roof ({:.1} m)", p.y);
                    let through = plain.iter().flat_map(|(_, m)| m.positions.iter()).filter(|q| Vec2::new(q[0], q[2]).distance(Vec2::new(p.x, p.z)) < 2.5 && q[1] > p.y + 0.3).count();
                    assert_eq!(through, 0, "{style:?} lot {li}: the tower rises through its deck at {:.1} m", p.y);
                }
            }
        }
        assert!(bad.is_empty(), "{} rooftop pads leave their lot or budget:
{}", bad.len(), bad.join("
"));
        eprintln!("rooftop decks: {drawn} of {tried}; no room on {}", misses.len());
        assert!(drawn * 10 >= tried * 8, "{drawn} of {tried} towers drew a deck; missed {misses:?}");
        // The pick: a flagged tower always gets a deck-carrying style.
        let lots = many_lots(LandUse::Residential);
        for (li, lot) in lots.iter().enumerate().take(30) {
            for lu in [LandUse::Residential, LandUse::Commercial] {
                let s = choose_style(lot, lu, 14, Tier::High, 50 + li as u64, &rules, &Extras { rooftop_pad: true, ..Extras::default() });
                assert!(s.pad_capable(), "{lu:?} lot {li} picked {s:?}");
            }
        }
        // Vertiports draw a pad per slot where the lot has room, at the deck height.
        for tier in Tier::ALL {
            let lot = &lots[0];
            let g = generate_building(lot, LandUse::Civic, 1, tier, 1.0, 5, &rules, &pal, &Extras { service: Some(nd_sim::ServiceKind::Vertiport), ..Extras::default() });
            assert!(!g.pads.is_empty(), "{tier:?} vertiport has pads");
            assert!(g.pads.len() <= nd_sim::air::vertiport_slots(tier) as usize);
            let deck = nd_sim::air::vertiport_deck(tier);
            assert!(g.pads.iter().all(|p| (p.y - deck).abs() < 0.6), "{tier:?} pads at the deck height {deck}: {:?}", g.pads);
        }
    }

    /// The variety styles change form with height: a tall site draws a
    /// taller building than a low one, and the glass castle ends in
    /// crystals, not a needle (its top is broad).
    #[test]
    fn styles_change_form_with_height() {
        let rules = BuildingRules::default();
        let pal = Palette::default();
        let lots = many_lots(LandUse::Residential);
        let lot = lots.iter().max_by(|a, b| polygon_area(&a.polygon).abs().total_cmp(&polygon_area(&b.polygon).abs())).unwrap();
        let top = |g: &Geometry| g.iter().filter(|(s, _)| *s != Surface::Glow && *s != Surface::Matte).flat_map(|(_, m)| m.positions.iter().map(|p| p[1])).fold(0.0f32, f32::max);
        for style in [Style::WalledCity, Style::PencilTower, Style::TwinSlab, Style::Megastructure, Style::GlassTerraces, Style::CapsuleHotel, Style::HillsideSteps, Style::NeonTower, Style::Fortress] {
            let low = generate_styled(lot, LandUse::Residential, 3, Tier::Mid, 0.8, 9, style, &rules, &pal);
            let high = generate_styled(lot, LandUse::Residential, 16, Tier::Mid, 0.8, 9, style, &rules, &pal);
            assert!(top(&high) > top(&low) * 1.6, "{style:?}: 16 floors ({:.0} m) should rise well above 3 ({:.0} m)", top(&high), top(&low));
        }
        // No spire: within the top 15% of the glass castle's height there is
        // still a crystal at least a few metres across.
        let g = generate_styled(lot, LandUse::Residential, 16, Tier::High, 1.0, 4, Style::Fortress, &rules, &pal);
        let h = top(&g);
        let crown: Vec<Vec2> = g.get(Surface::Glass).positions.iter().filter(|p| p[1] > h * 0.85).map(|p| Vec2::new(p[0], p[2])).collect();
        let (lo, hi) = parts::bounds(&crown);
        assert!((hi - lo).min_element() > 3.0, "the glass castle should not taper to a point: crown {:?}", hi - lo);
    }

    /// Cheap buildings use the weathered materials, smart ones do not.
    #[test]
    fn low_value_buildings_are_weathered() {
        let rules = BuildingRules::default();
        let pal = Palette::default();
        let lots = many_lots(LandUse::Residential);
        let worn = |g: &Geometry| g.iter().filter(|(s, _)| s.is_worn()).count();
        let low = generate_styled(&lots[0], LandUse::Residential, 4, Tier::Low, 0.5, 3, Style::Tenement, &rules, &pal);
        let high = generate_styled(&lots[0], LandUse::Residential, 4, Tier::High, 1.0, 3, Style::Tenement, &rules, &pal);
        assert!(worn(&low) > 0 && low.get(Surface::Brick).is_empty(), "a low-tier tenement should be weathered brick");
        assert_eq!(worn(&high), 0, "a well-kept high-tier building stays clean");
    }

    #[test]
    fn style_weights_respect_land_use_and_tier() {
        let rules = BuildingRules::default();
        let mut rng = rng_for(1, 2);
        for _ in 0..50 {
            let s = rules.pick_style(LandUse::Industrial, Tier::Low, &mut rng);
            assert!(matches!(s, Style::Junkyard | Style::RustyWarehouse | Style::Workshops));
            let s = rules.pick_style(LandUse::Industrial, Tier::High, &mut rng);
            assert!(matches!(s, Style::Datacenter | Style::Depot | Style::FabPlant));
        }
    }

    /// The same inputs draw the same building, byte for byte: the renderer
    /// keys caches on (lot, seed) and a save must look the same on reload.
    #[test]
    fn generation_is_deterministic() {
        let rules = BuildingRules::default();
        let pal = Palette::default();
        let lots = many_lots(LandUse::Commercial);
        for (li, lot) in lots.iter().take(6).enumerate() {
            for style in [Style::Shophouse, Style::Stacks, Style::Junkyard, Style::Plaza, Style::Crystal] {
                let a = generate_styled(lot, LandUse::Commercial, 5, Tier::Mid, 0.6, 99 + li as u64, style, &rules, &pal);
                let b = generate_styled(lot, LandUse::Commercial, 5, Tier::Mid, 0.6, 99 + li as u64, style, &rules, &pal);
                assert!(a == b, "{style:?} on lot {li} differs between two runs");
            }
            let extras = Extras { powered: 0.4, ..Extras::default() };
            let a = generate_building(lot, LandUse::Residential, 7, Tier::Low, 0.5, 3 + li as u64, &rules, &pal, &extras);
            let b = generate_building(lot, LandUse::Residential, 7, Tier::Low, 0.5, 3 + li as u64, &rules, &pal, &extras);
            assert!(a == b, "generate_building on lot {li} differs between two runs");
        }
    }

    /// A truck lot draws more lorries the fuller the sim says it is.
    #[test]
    fn a_truck_lot_shows_its_fleet() {
        let rules = BuildingRules::default();
        let pal = Palette::default();
        let lots = many_lots(LandUse::Industrial);
        let lot = lots.iter().max_by(|a, b| a.area().total_cmp(&b.area())).unwrap();
        let tris = |fleet: f32| {
            let e = Extras { business: Some(nd_sim::BusinessKind::FleetLot), fleet, ..Extras::default() };
            generate_building(lot, LandUse::Industrial, 1, Tier::Low, 0.8, 9, &rules, &pal, &e).triangle_count()
        };
        assert!(tris(1.0) > tris(0.0) + 200, "{} vs {}", tris(1.0), tris(0.0));
    }

    #[test]
    fn enum_tables_match_their_indices() {
        for (i, s) in Surface::ALL.iter().enumerate() {
            assert_eq!(s.index(), i, "{s:?}");
        }
        for (i, s) in Style::ALL.iter().enumerate() {
            assert_eq!(s.index(), i, "{s:?}");
        }
        assert_eq!(Geometry::default().parts.len(), Surface::ALL.len());
    }

    /// Rule files may hold chances above one; nothing downstream may panic.
    #[test]
    fn out_of_range_rules_are_clamped() {
        let mut rules = BuildingRules::default();
        rules.commercial.sign_chance = 7.0;
        rules.entertainment.vertical_sign_chance = f32::NAN;
        rules.tiers[0].lit_chance = 3.0;
        rules.tiers[1].window_colors.clear();
        rules.validate();
        assert_eq!(rules.commercial.sign_chance, 1.0);
        assert_eq!(rules.entertainment.vertical_sign_chance, 0.0);
        assert_eq!(rules.tiers[0].lit_chance, 1.0);
        assert!(!rules.tiers[1].window_colors.is_empty());
        // Even without validate() the generators clamp at the point of use.
        let mut raw = BuildingRules::default();
        raw.tiers[2].lit_chance = 4.0;
        raw.commercial.sign_chance = 2.0;
        let pal = Palette::default();
        let lots = many_lots(LandUse::Commercial);
        for style in [Style::Shophouse, Style::RetailBlock, Style::Tenement] {
            let g = generate_styled(&lots[0], LandUse::Commercial, 5, Tier::High, 0.3, 1, style, &raw, &pal);
            assert!(g.triangle_count() > 0);
        }
    }

    /// Extras change the look without breaking the lot: brown-outs, car
    /// park occupancy, a bus depot, a planned multi-lot park and a real
    /// railway link on either side of the track.
    #[test]
    fn extras_paths_stay_on_their_lot() {
        let rules = BuildingRules::default();
        let pal = Palette::default();
        let lots = many_lots(LandUse::Commercial);
        let lot = &lots[0];
        // Brown-out: fewer lit windows than full power, same footprint.
        let lit = |g: &Geometry| g.get(Surface::Glow).indices.len();
        let bright = generate_building(lot, LandUse::Residential, 8, Tier::Mid, 0.9, 5, &rules, &pal, &Extras::default());
        let dim = generate_building(lot, LandUse::Residential, 8, Tier::Mid, 0.9, 5, &rules, &pal, &Extras { powered: 0.1, ..Extras::default() });
        assert!(lit(&dim) < lit(&bright), "a brown-out should put windows out ({} vs {})", lit(&dim), lit(&bright));
        for g in [&bright, &dim] {
            check_mesh(g, "powered");
            assert!(extent(g, lot, None).0 <= 1.0);
        }
        // Occupancy lighting: nearly every window becomes a marked light over
        // glass, ranked so the renderer can switch them by share.
        let dynamic = generate_building(lot, LandUse::Residential, 8, Tier::Mid, 0.9, 5, &rules, &pal, &Extras { dynamic_lights: true, ..Extras::default() });
        check_mesh(&dynamic, "dynamic lights");
        let marks: Vec<f32> = dynamic.get(Surface::Glow).colors.iter().map(|c| c[3]).filter(|a| *a >= WINDOW_LIGHT_MARK).collect();
        assert!(marks.len() >= 4 * lit(&bright) / 6, "most windows are switchable lights: {} marked vertices", marks.len());
        assert!(marks.iter().all(|a| *a < WINDOW_LIGHT_MARK + 1.0), "ranks stay in 0..1");
        let ranks: std::collections::HashSet<u32> = marks.iter().map(|a| ((a - WINDOW_LIGHT_MARK) * 1000.0) as u32).collect();
        assert!(ranks.len() > 10, "windows get spread-out ranks, got {}", ranks.len());
        assert!(dynamic.get(Surface::Glass).positions.len() > bright.get(Surface::Glass).positions.len(), "glass behind every switchable light");
        assert!(dynamic.get(Surface::Glow).colors.iter().any(|c| c[3] == 1.0) || dynamic.get(Surface::Glow).colors.iter().all(|c| c[3] >= WINDOW_LIGHT_MARK), "other glow untouched");
        // Parking: a full lot has more car than an empty one.
        let empty = generate_building(lot, LandUse::Commercial, 1, Tier::Low, 0.8, 5, &rules, &pal, &Extras { parking: Some(0.0), ..Extras::default() });
        let full = generate_building(lot, LandUse::Commercial, 1, Tier::Low, 0.8, 5, &rules, &pal, &Extras { parking: Some(1.0), ..Extras::default() });
        assert!(full.triangle_count() > empty.triangle_count() + 100, "cars should show up in a full car park");
        let deck = generate_building(lot, LandUse::Commercial, 4, Tier::Mid, 0.8, 5, &rules, &pal, &Extras { parking: Some(0.7), ..Extras::default() });
        for g in [&empty, &full, &deck] {
            check_mesh(g, "parking");
            let (over, _, low, culprit) = extent(g, lot, None);
            assert!(over <= 1.0 && low >= -0.3, "parking leaves its lot: {over:.1} m / {low:.1}: {culprit}");
        }
        // Services and attractions, every tier, drawn through their extras.
        for kind in nd_sim::services::ServiceKind::ALL {
            for tier in Tier::ALL {
                let g = generate_building(lot, LandUse::Civic, 1, tier, 0.9, 9, &rules, &pal, &Extras { service: Some(kind), ..Extras::default() });
                check_mesh(&g, "service");
                let (over, _, low, culprit) = extent(&g, lot, None);
                assert!(g.triangle_count() > 50 && over <= 1.0 && low >= -0.3, "{kind:?} {tier:?}: {over:.1} m / {low:.1}: {culprit}");
            }
        }
        // Depot.
        let depot = generate_building(lot, LandUse::Civic, 2, Tier::Mid, 0.8, 5, &rules, &pal, &Extras { depot: true, ..Extras::default() });
        check_mesh(&depot, "depot");
        assert!(extent(&depot, lot, None).0 <= 1.0);

        // A real park plan over several adjoining lots, drawn lot by lot.
        let mut roads = RoadNetwork::default();
        roads.add_polyline(&[Vec2::new(-400.0, -40.0), Vec2::new(400.0, -40.0)], 1, 1);
        roads.add_polyline(&[Vec2::new(-400.0, 200.0), Vec2::new(400.0, 200.0)], 1, 1);
        roads.add_polyline(&[Vec2::new(-120.0, -300.0), Vec2::new(60.0, 300.0)], 1, 1);
        let zone = Zone { polygon: vec![Vec2::new(-200.0, -20.0), Vec2::new(200.0, -20.0), Vec2::new(200.0, 180.0), Vec2::new(-200.0, 180.0)], land_use: LandUse::Park };
        let mut park_lots = SlotMap::with_key();
        generate_lots(ZoneId::default(), &zone, &roads, &mut park_lots, &[]);
        let mut lv = nd_sim::land_value::LandValue::new(400.0, 50.0);
        for v in &mut lv.values {
            *v = 0.8;
        }
        nd_sim::park::plan_parks(&mut park_lots, &lv);
        let planned: Vec<&Lot> = park_lots.values().filter(|l| l.usable() && l.park.is_some()).collect();
        assert!(planned.iter().any(|l| l.park.as_ref().unwrap().cluster_lots >= 2), "the park zone should plan at least one multi-lot park");
        assert!(planned.iter().any(|l| !l.park.as_ref().unwrap().shared_edges.is_empty()), "adjoining park lots should share an edge");
        let mut drawn = 0;
        for lot in planned {
            let layout = lot.park.as_ref().unwrap();
            let g = generate_building(lot, LandUse::Park, 1, layout.tier, 1.0, 11, &rules, &pal, &Extras { park: Some(layout), ..Extras::default() });
            check_mesh(&g, "park");
            let (over, _, low, culprit) = extent(&g, lot, None);
            assert!(over <= 1.0 && low >= -0.3, "park lot ({} lots, tier {:?}) leaves its lot: {over:.1} m / {low:.1}: {culprit}", layout.cluster_lots, layout.tier);
            drawn += 1;
        }
        assert!(drawn > 3);

        // A real railway link along the back of the lot, on both sides of
        // the track, for the station and the yard.
        for kind in [RailFacility::Station, RailFacility::FreightYard] {
            for side in [1.0f32, -1.0] {
                for (li, lot) in lots.iter().take(12).enumerate() {
                    let a = lot.corners[3];
                    let b = lot.corners[2];
                    let d = (b - a).normalize_or_zero();
                    // `out` must point from the track towards the lot, so pick the
                    // platform direction that makes perp(along) * side do that.
                    let along = if Vec2::new(-d.y, d.x).dot(lot.front_dir) * side > 0.0 { d } else { -d };
                    let centre = (a + b) * 0.5 - lot.front_dir * 4.0;
                    let half = (a.distance(b) * 0.45).min(40.0);
                    let link = RailLink { kind, segment: nd_sim::SegmentId(0), platform: [centre - along * half, centre + along * half], side, rail_connected: true, road_connected: true };
                    let g = generate_building(lot, LandUse::Industrial, 2, Tier::Mid, 0.8, 5 + li as u64, &rules, &pal, &Extras { rail: Some(&link), ..Extras::default() });
                    check_mesh(&g, "rail");
                    let (over, _, low, culprit) = extent(&g, lot, Some(&link));
                    assert!(over <= 1.0 && low >= -0.3, "{kind:?} side {side} on lot {li} leaves its lot and track: {over:.1} m / {low:.1}: {culprit}");
                    // The platform furniture must land on the lot's side of the track.
                    let out = Vec2::new(-along.y, along.x) * side;
                    let platform_side = g.get(Surface::Concrete).positions.iter().filter(|p| (Vec2::new(p[0], p[2]) - centre).dot(out) > 0.5).count();
                    let far_side = g.get(Surface::Concrete).positions.iter().filter(|p| (Vec2::new(p[0], p[2]) - centre).dot(out) < -8.0).count();
                    assert!(platform_side > far_side, "{kind:?} side {side} on lot {li} built across the track");
                }
            }
        }
    }

    /// A bare context on a lot, for driving one part generator directly.
    fn ctx_on<'a>(lot: &Lot, rules: &'a BuildingRules, pal: &'a Palette, tier: Tier, wear: f32) -> Ctx<'a> {
        let frame = Frame::from_lot(lot);
        let r = rules.for_land_use(LandUse::Residential);
        let mut lot_poly: Vec<Vec2> = lot.polygon.iter().map(|p| frame.to_local(*p)).collect();
        if polygon_area(&lot_poly) < 0.0 {
            lot_poly.reverse();
        }
        let poly = parts::offset_poly(&lot_poly, r.inset);
        Ctx {
            fw: frame.width,
            fd: frame.depth,
            frame,
            rules: r,
            style: rules.style(tier),
            palette: pal,
            land_use: LandUse::Residential,
            tier,
            floors: 3,
            wear,
            lot_poly,
            poly,
            seed: 1,
            parking: None,
            powered: 1.0,
            park: None,
            rail: None,
            animated_rotors: false,
            dynamic_lights: false,
        }
    }

    /// Triangles per shipping container and per Stacks building, printed
    /// for the record (run with --nocapture). Every unit stays within its
    /// budget, worn places spend more of it on rust and dents.
    #[test]
    fn container_triangle_counts() {
        let rules = BuildingRules::default();
        let pal = Palette::default();
        let lots = many_lots(LandUse::Residential);
        let mut counts = Vec::new();
        let mut by_wear = Vec::new();
        for (i, (wear, tier)) in [(0.0f32, Tier::High), (0.3, Tier::Mid), (0.6, Tier::Low), (1.0, Tier::Low)].iter().enumerate() {
            let c = ctx_on(&lots[0], &rules, &pal, *tier, *wear);
            let mut rng = rng_for(5, i as u64);
            let mut sum = 0;
            for k in 0..60 {
                let mut g = Geometry::default();
                parts::container_unit(&mut g, &c, &mut rng, 0.0, 0.0, 0.0, if k % 3 == 0 { 12.1 } else { 6.05 }, 2.44, 2.6, (k % 3) as f32 * 2.6, pal.container(k));
                check_mesh(&g, "container");
                assert!(g.triangle_count() <= parts::CONTAINER_TRIS, "a container spent {} triangles", g.triangle_count());
                counts.push(g.triangle_count());
                sum += g.triangle_count();
            }
            by_wear.push(sum);
        }
        assert!(by_wear[3] > by_wear[0], "worn containers should carry more rust and dents: {by_wear:?}");
        let (lo, hi) = (*counts.iter().min().unwrap(), *counts.iter().max().unwrap());
        let avg = counts.iter().sum::<usize>() as f32 / counts.len() as f32;
        let stacks: Vec<usize> = lots.iter().take(40).enumerate().map(|(li, lot)| generate_styled(lot, LandUse::Residential, 3, Tier::Low, 0.5, 11 + li as u64, Style::Stacks, &rules, &pal).triangle_count()).collect();
        let savg = stacks.iter().sum::<usize>() as f32 / stacks.len() as f32;
        eprintln!("container tris: min {lo} avg {avg:.1} max {hi}; Stacks building avg {savg:.0} (max {})", stacks.iter().max().unwrap());
    }

    /// Units painted from the same palette entry come out different (paint
    /// jitter, fading, livery, wear), and the same seed draws the same unit.
    #[test]
    fn containers_vary_per_unit_deterministically() {
        let rules = BuildingRules::default();
        let pal = Palette::default();
        let lots = many_lots(LandUse::Residential);
        let c = ctx_on(&lots[0], &rules, &pal, Tier::Low, 0.5);
        let draw = |seed: u64| {
            let mut g = Geometry::default();
            let mut rng = rng_for(seed, 77);
            parts::container_unit(&mut g, &c, &mut rng, 0.0, 0.0, 0.0, 6.05, 2.44, 2.6, 0.0, pal.container(0));
            g
        };
        let mut paints: Vec<[u32; 3]> = Vec::new();
        let mut shapes: Vec<usize> = Vec::new();
        for seed in 0..40 {
            let g = draw(seed);
            assert!(g == draw(seed), "seed {seed} draws a different container twice");
            let sheet = g.get(Surface::Container);
            // The first sheet vertex carries the unit's body paint.
            let p = sheet.colors[0];
            let key = [(p[0] * 1000.0) as u32, (p[1] * 1000.0) as u32, (p[2] * 1000.0) as u32];
            if !paints.contains(&key) {
                paints.push(key);
            }
            if !shapes.contains(&g.triangle_count()) {
                shapes.push(g.triangle_count());
            }
            // Walls, roof and the decals painted on them all use the sheet.
            assert!(sheet.indices.len() / 3 >= 10, "the corrugated sheet covers walls, ends and roof");
        }
        assert!(paints.len() >= 35, "only {} distinct paints in 40 units", paints.len());
        assert!(shapes.len() >= 3, "livery and wear should vary the details: {shapes:?}");
    }

    /// A prop is an I-beam: two flanges and a web (two faces each, 12 rust
    /// triangles), 0.2 m square in plan around its point, on a base plate
    /// and a concrete pad on the ground, under a cap plate at the top.
    #[test]
    fn supports_are_ibeams_and_stay_put() {
        let rules = BuildingRules::default();
        let pal = Palette::default();
        let lots = many_lots(LandUse::Residential);
        let c = ctx_on(&lots[0], &rules, &pal, Tier::Low, 0.5);
        let mut rng = rng_for(3, 3);
        let p = Vec2::new(1.0, -2.0);
        for (base, on_ground) in [(0.0f32, true), (2.6, false)] {
            let mut g = Geometry::default();
            parts::ibeam_support(&mut g, &c, &mut rng, p, 0.4, base, base + 2.6, on_ground);
            check_mesh(&g, "prop");
            let steel = g.get(Surface::Rust);
            assert_eq!(steel.indices.len() / 3, 12, "two flanges and a web, two faces each");
            let w = c.frame.to_world(p.x, p.y);
            let half = parts::IBEAM_FLANGE * 0.5 * std::f32::consts::SQRT_2 + 1e-3;
            assert!(steel.positions.iter().all(|q| Vec2::new(q[0], q[2]).distance(w) <= half), "the beam stays within its section");
            // The web is thinner than the flanges: some rust vertices sit
            // close to the beam's axis plane, others out at the flange tips.
            let a = c.frame.yaw() + 0.4;
            let axis = Vec2::new(a.cos(), a.sin());
            let xs: Vec<f32> = steel.positions.iter().map(|q| (Vec2::new(q[0], q[2]) - w).dot(axis).abs()).collect();
            assert!(xs.iter().any(|x| *x < 0.02) && xs.iter().any(|x| *x > 0.09), "an I section: {xs:?}");
            assert_eq!(!g.get(Surface::Concrete).is_empty(), on_ground, "a pad only on the ground");
            let all = g.iter().flat_map(|(_, m)| m.positions.iter()).map(|q| q[1]);
            let (lo, hi) = all.fold((f32::MAX, f32::MIN), |(lo, hi), y| (lo.min(y), hi.max(y)));
            assert!(lo >= base - 1e-3 && hi <= base + 2.6 + 1e-3, "the prop fills the gap and no more: {lo}..{hi}");
            assert!(g.iter().flat_map(|(_, m)| m.positions.iter()).all(|q| Vec2::new(q[0], q[2]).distance(w) <= 0.45), "plates and pad stay round the prop");
        }
        // Stacks draw them, on their lots.
        let mut props = 0;
        for (li, lot) in lots.iter().take(30).enumerate() {
            let g = generate_styled(lot, LandUse::Residential, 4, Tier::Low, 0.4, 21 + li as u64, Style::Stacks, &rules, &pal);
            props += g.get(Surface::Rust).indices.len() / 3;
            for q in &g.get(Surface::Rust).positions {
                assert!(outside_by(Vec2::new(q[0], q[2]), lot) <= 1.0 && q[1] >= -0.01, "lot {li}: a prop outside its lot at {q:?}");
            }
        }
        assert!(props >= 20 * 5, "stacks should stand on I-beam props ({props} rust triangles)");
    }
}
