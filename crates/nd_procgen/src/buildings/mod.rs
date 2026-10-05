//! Building grammar. Takes a parcel, land use, floors, tier, condition and a
//! seed and returns geometry bucketed by surface.
//!
//! Every (land use, tier) pair has its own family of styles, picked by weight
//! from the rules file, so an industrial slum reads as junkyards and rusted
//! sheds while a high-value industrial block gets datacentres and container
//! depots. Footprints follow the parcel polygon: a triangular corner lot gets
//! a triangular building.

mod civic;
mod commercial;
mod entertainment;
mod industrial;
mod park;
mod parking;
pub mod parts;
mod power;
pub mod transit;
mod residential;

use crate::palette::Palette;
use crate::surface::{Geometry, Surface};
use glam::{Vec2, Vec3};
use nd_sim::rng_for;
use nd_sim::zoning::{point_in_polygon, polygon_area};
use nd_sim::park::ParkLayout;
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
}

impl Style {
    pub const ALL: [Style; 42] = [
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
    ];

    pub fn label(self) -> &'static str {
        match self {
            Style::Stacks => "the Stacks",
            Style::Tenement => "walk-up tenement",
            Style::Brutalist => "brutalist block",
            Style::BalconySlab => "balcony slab",
            Style::Courtyard => "courtyard block",
            Style::Crystal => "crystal tower",
            Style::Fortress => "fortress compound",
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

impl BuildingRules {
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

    /// Weighted pick among the styles listed for this land use and tier.
    pub fn pick_style(&self, lu: LandUse, tier: Tier, rng: &mut impl Rng) -> Style {
        let options: Vec<&StyleWeight> = self.styles.iter().filter(|s| s.land_use == lu && s.tier == tier && s.weight > 0.0).collect();
        let total: f32 = options.iter().map(|s| s.weight).sum();
        if options.is_empty() || total <= 0.0 {
            return default_style(lu, tier);
        }
        let mut r = rng.random_range(0.0..total);
        for o in &options {
            if r < o.weight {
                return o.style;
            }
            r -= o.weight;
        }
        options.last().unwrap().style
    }
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
            ],
        }
    }
}

// ── geometry frame ───────────────────────────────────────────────────────────

/// Parcel-aligned coordinate frame: `u` runs along the frontage, `v` inward
/// from the road, origin at the centre of the parcel's bounding rectangle.
#[derive(Clone, Copy, Debug)]
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

    pub fn yaw(&self) -> f32 {
        self.along.y.atan2(self.along.x)
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
    pub fn from_edge(a: Vec2, b: Vec2) -> Self {
        let len = a.distance(b);
        let tangent = if len > 1e-4 { (b - a) / len } else { Vec2::X };
        let out = Vec2::new(tangent.y, -tangent.x);
        Self { a, b, tangent, out, len, is_front: out.dot(Vec2::new(0.0, -1.0)) > 0.6 }
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
}

impl Default for Extras<'_> {
    fn default() -> Self {
        Self { parking: None, plant: None, powered: 1.0, park: None, depot: false }
    }
}

impl Ctx<'_> {
    pub fn fh(&self) -> f32 {
        self.rules.floor_height
    }

    pub fn height(&self) -> f32 {
        self.floors.max(1) as f32 * self.fh()
    }

    pub fn lit_chance(&self) -> f32 {
        self.style.lit_chance * (1.0 - 0.8 * self.wear) * self.powered.clamp(0.08, 1.0)
    }

    pub fn inside(&self, p: Vec2) -> bool {
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

    /// The face of a polygon that looks at the road.
    pub fn front_face(&self, poly: &[Vec2]) -> Face {
        let faces = self.faces(poly);
        faces
            .iter()
            .copied()
            .max_by(|a, b| {
                let sa = a.out.dot(Vec2::new(0.0, -1.0)) * a.len;
                let sb = b.out.dot(Vec2::new(0.0, -1.0)) * b.len;
                sa.total_cmp(&sb)
            })
            .unwrap_or(Face::from_edge(Vec2::new(-1.0, -1.0), Vec2::new(1.0, -1.0)))
    }

    /// Random point inside the parcel at least `margin` from its edges.
    pub fn random_inside(&self, rng: &mut impl Rng, margin: f32) -> Option<Vec2> {
        let poly = parts::offset_poly(&self.lot_poly, margin);
        if poly.len() < 3 {
            return None;
        }
        let (lo, hi) = parts::bounds(&poly);
        for _ in 0..24 {
            let p = Vec2::new(rng.random_range(lo.x..=hi.x), rng.random_range(lo.y..=hi.y));
            if point_in_polygon(p, &poly) {
                return Some(p);
            }
        }
        None
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
    let mut rng = rng_for(seed, 11);
    let style = match (extras.plant, extras.parking) {
        _ if extras.depot => Style::BusDepot,
        (Some(PowerPlantKind::Coal), _) => Style::PowerCoal,
        (Some(PowerPlantKind::Solar), _) => Style::PowerSolar,
        (Some(PowerPlantKind::Wind), _) => Style::PowerWind,
        (Some(PowerPlantKind::Nuclear), _) => Style::PowerNuclear,
        (None, Some(_)) if tier == Tier::Low || floors < 2 => Style::ParkingLot,
        (None, Some(_)) => Style::CarPark,
        (None, None) => rules.pick_style(land_use, tier, &mut rng),
    };
    generate_styled_extras(lot, land_use, floors, tier, condition, seed, style, rules, palette, extras)
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
    let parking = extras.parking;
    let mut g = Geometry::default();
    let mut rng = rng_for(seed, 13);
    let frame = Frame::from_lot(lot);
    let r = rules.for_land_use(land_use);
    let mut lot_poly: Vec<Vec2> = lot.polygon.iter().map(|p| frame.to_local(*p)).collect();
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
        park: if land_use == LandUse::Park || matches!(style, Style::ScrubLot | Style::Garden | Style::Plaza) { Some(extras.park.cloned().unwrap_or_else(|| nd_sim::park::plan_single(lot, tier))) } else { None },
    };
    match style {
        Style::Stacks => residential::stacks(&mut g, &ctx, &mut rng),
        Style::Tenement => residential::tenement(&mut g, &ctx, &mut rng),
        Style::Brutalist => residential::brutalist(&mut g, &ctx, &mut rng),
        Style::BalconySlab => residential::balcony_slab(&mut g, &ctx, &mut rng),
        Style::Courtyard => residential::courtyard(&mut g, &ctx, &mut rng),
        Style::Crystal => residential::crystal(&mut g, &ctx, &mut rng),
        Style::Fortress => residential::fortress(&mut g, &ctx, &mut rng),
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
    }
    g
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
                        for (s, m) in g.iter() {
                            assert_eq!(m.positions.len(), m.uvs.len(), "{style:?} {s:?} uv count");
                            assert!(m.positions.iter().all(|p| p.iter().all(|v| v.is_finite())), "{style:?} {s:?} has NaN");
                            assert!(m.indices.iter().all(|&i| (i as usize) < m.positions.len()), "{style:?} {s:?} bad index");
                        }
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

    /// Nothing a building generates may reach far beyond its own lot.
    #[test]
    fn geometry_stays_near_its_lot() {
        let rules = BuildingRules::default();
        let pal = Palette::default();
        let mut worst: Vec<(f32, String)> = Vec::new();
        for lu in LandUse::ALL {
            let lots = many_lots(lu);
            assert!(lots.len() > 10, "{lu:?}: {} lots", lots.len());
            for (li, lot) in lots.iter().enumerate() {
                let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
                for p in &lot.polygon {
                    lo = lo.min(*p);
                    hi = hi.max(*p);
                }
                let span = (hi - lo).max_element();
                for style in Style::ALL {
                    for (seed, floors, tier) in [(1u64, 2u8, Tier::Low), (7, 9, Tier::Mid), (13, 30, Tier::High)] {
                        let g = generate_styled(lot, lu, floors, tier, 0.6, seed + li as u64, style, &rules, &pal);
                        let mut over = 0.0f32;
                        let mut top = 0.0f32;
                        let mut low = 0.0f32;
                        let mut culprit = String::new();
                        let frame = Frame::from_lot(lot);
                        for (surface, m) in g.iter() {
                            for p in &m.positions {
                                let q = Vec2::new(p[0], p[2]);
                                let d = (lo - q).max(q - hi).max_element();
                                if d > over {
                                    over = d;
                                    let l = frame.to_local(q);
                                    culprit = format!("{surface:?} at local ({:.1}, {:.1}) y {:.1}", l.x, l.y, p[1]);
                                }
                                if p[1] < low {
                                    low = p[1];
                                    if over <= 4.0 {
                                        let l = frame.to_local(q);
                                        culprit = format!("{surface:?} at local ({:.1}, {:.1}) y {:.1}", l.x, l.y, p[1]);
                                    }
                                }
                                top = top.max(p[1]);
                            }
                        }
                        if over > 4.0 || top > (60.0 + floors as f32 * 5.0).max(90.0) || low < -2.0 {
                            let (plo, phi) = parts::bounds(&frame_poly(&frame, lot));
                            worst.push((over.max(-low), format!("{style:?} {lu:?} lot {li} ({} verts, span {span:.0}, local {:.0}..{:.0} x {:.0}..{:.0}) overshoots {over:.1} m, top {top:.0} m, low {low:.1}, floors {floors}: {culprit}", lot.polygon.len(), plo.x, phi.x, plo.y, phi.y)));
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
        assert!(worst.is_empty(), "{} style/lot combinations reach beyond their lot", worst.len());
    }

    #[test]
    fn style_weights_respect_land_use_and_tier() {
        let rules = BuildingRules::default();
        let mut rng = rng_for(1, 2);
        for _ in 0..50 {
            let s = rules.pick_style(LandUse::Industrial, Tier::Low, &mut rng);
            assert!(matches!(s, Style::Junkyard | Style::RustyWarehouse));
            let s = rules.pick_style(LandUse::Industrial, Tier::High, &mut rng);
            assert!(matches!(s, Style::Datacenter | Style::Depot));
        }
    }
}
