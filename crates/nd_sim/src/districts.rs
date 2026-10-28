//! [districts] Named districts painted over the map, as in Cities: Skylines.
//!
//! A district is a name, a colour and a set of policies: a specialisation
//! for the homes in it (Kowloon stacks, suburban, eco), one for its shops
//! (neon strip, tech campus, night market, tourist quarter), an industry
//! sub-zone and the Historic freeze. Painting does not go through projects:
//! districts change no road, lot or zone, only what grows next and how it
//! looks, so they apply at once (as they do in Cities: Skylines) and the
//! plan's undo history stays about building.
//!
//! The map is a raster ([`DistrictGrid`]) of [`CELL`] metre cells holding a
//! district id (0 = none). A raster rather than polygons because the brush
//! paints and erases freely, painted areas merge and split, borders follow
//! the cells exactly, and "which district is this lot in" is one array read.
//! Saved run-length encoded, so an empty or blocky map costs a few bytes.
//!
//! Pure data and rules here; the world hooks live in `world/districts.rs`,
//! the looks (styles and palettes per [`Look`]) in `nd_procgen`.

use crate::buildings::Tier;
use crate::geom::point_in_polygon;
use crate::industry::IndustryKind;
use crate::market::BusinessKind;
use crate::zoning::LandUse;
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Side of a district cell in metres: fine enough for a brush to follow a
/// street, coarse enough that the whole map is ~48k cells.
pub const CELL: f32 = 16.0;
/// Half-size of the painted square: the map out to its edge.
pub const HALF: f32 = crate::landscape::MAP_EDGE;
/// At most this many districts at once (ids are `u16`, 0 is "none").
pub const MAX_DISTRICTS: usize = 64;

/// How the homes in a district grow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResidentialSpec {
    /// Cheap, dense, tall: walled blocks, stacks, pencil towers.
    Kowloon,
    /// Low and spread out: villas, terraces, courtyards, three floors at most.
    Suburban,
    /// Garden towers and planted terraces; lifts the land around.
    Eco,
    /// [capsule] White domes, saucers and pill towers ringed with round
    /// windows. Appended so saved specs keep their names and order.
    Capsule,
    /// [r4-brick] Red-brick worker rows, warehouse lofts with steel
    /// windows and water towers (the Foundry Row story).
    BrickSteel,
    /// [lantern] Lantern Alley: shophouses with flats over the shop and
    /// narrow walk-ups round lantern-strung alleys. Appended.
    Lantern,
}

/// How the shops in a district grow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CommercialSpec {
    /// Karaoke towers wrapped in signs; gadget shops; draws tourists at night.
    NeonStrip,
    /// Glass offices and campuses; pulls the tier up.
    TechCampus,
    /// Low arcades of stalls and lanterns: groceries and bakeries.
    NightMarket,
    /// Hotels first, then shops for visitors.
    Tourist,
    /// [capsule] High-tech domes: offices and gadget shops only (where the
    /// market can carry them), in white domes and pill towers.
    Capsule,
    /// [r4-brick] Brick workshops, the corner pub, the union hall.
    BrickSteel,
    /// [lantern] Lantern Alley's night market: stall rows under tarps, food
    /// courts, tea houses and market halls behind lantern gates; its small
    /// industry is workshops and back-alley kitchens. Appended.
    Lantern,
}

impl ResidentialSpec {
    pub const ALL: [ResidentialSpec; 6] = [ResidentialSpec::Kowloon, ResidentialSpec::Suburban, ResidentialSpec::Eco, ResidentialSpec::Capsule, ResidentialSpec::BrickSteel, ResidentialSpec::Lantern];

    pub fn label(self) -> &'static str {
        match self {
            ResidentialSpec::Kowloon => "Kowloon stacks",
            ResidentialSpec::Suburban => "Suburban",
            ResidentialSpec::Eco => "Eco",
            ResidentialSpec::Capsule => "Capsule",
            ResidentialSpec::BrickSteel => "Brick & Steel",
            ResidentialSpec::Lantern => "Lantern Alley",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            ResidentialSpec::Kowloon => "kowloon",
            ResidentialSpec::Suburban => "suburban",
            ResidentialSpec::Eco => "eco",
            ResidentialSpec::Capsule => "capsule",
            ResidentialSpec::BrickSteel => "brick_steel",
            ResidentialSpec::Lantern => "lantern_alley",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|x| x.key() == s.to_ascii_lowercase() || x.label().eq_ignore_ascii_case(s))
    }
}

impl CommercialSpec {
    pub const ALL: [CommercialSpec; 7] = [CommercialSpec::NeonStrip, CommercialSpec::TechCampus, CommercialSpec::NightMarket, CommercialSpec::Tourist, CommercialSpec::Capsule, CommercialSpec::BrickSteel, CommercialSpec::Lantern];

    pub fn label(self) -> &'static str {
        match self {
            CommercialSpec::NeonStrip => "Neon strip",
            CommercialSpec::TechCampus => "Tech campus",
            CommercialSpec::NightMarket => "Night market",
            CommercialSpec::Tourist => "Tourist quarter",
            CommercialSpec::Capsule => "Capsule",
            CommercialSpec::BrickSteel => "Brick & Steel",
            CommercialSpec::Lantern => "Lantern Alley",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            CommercialSpec::NeonStrip => "neon_strip",
            CommercialSpec::TechCampus => "tech_campus",
            CommercialSpec::NightMarket => "night_market",
            CommercialSpec::Tourist => "tourist",
            CommercialSpec::Capsule => "capsule",
            CommercialSpec::BrickSteel => "brick_steel",
            CommercialSpec::Lantern => "lantern_alley",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|x| x.key() == s.to_ascii_lowercase() || x.label().eq_ignore_ascii_case(s))
    }
}

/// The architecture a building was put up in, saved on the building
/// (`Building::look`) so it keeps its look when the district's policy
/// changes until it is refitted. `nd_procgen` maps each to a style menu and
/// a colour and sign palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Look {
    Kowloon,
    Suburban,
    Eco,
    NeonStrip,
    TechCampus,
    NightMarket,
    Tourist,
    Historic,
    /// [capsule] White domes of many sizes, domes on stilts, pill towers,
    /// saucers, ringed with portholes: the high-tech dome district.
    /// Appended so saved looks keep their names and order.
    Capsule,
    /// [r4-brick] Brick & Steel (Foundry Row): worker rows, lofts, the pub
    /// and the union hall, and the district's works in brick and steel.
    BrickSteel,
    /// [lantern] Lantern Alley (the Lantern Lanes story): shophouses and
    /// alley walk-ups, stall rows, food courts, tea houses and market
    /// halls, workshops and back-alley kitchens, paper lanterns strung over
    /// the lanes. It also dresses the district's small industry. Appended.
    Lantern,
}

impl Look {
    pub const ALL: [Look; 11] = [Look::Kowloon, Look::Suburban, Look::Eco, Look::NeonStrip, Look::TechCampus, Look::NightMarket, Look::Tourist, Look::Historic, Look::Capsule, Look::BrickSteel, Look::Lantern];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            Look::Kowloon => "Kowloon stacks",
            Look::Suburban => "suburban",
            Look::Eco => "eco",
            Look::NeonStrip => "neon strip",
            Look::TechCampus => "tech campus",
            Look::NightMarket => "night market",
            Look::Tourist => "tourist quarter",
            Look::Historic => "historic",
            Look::Capsule => "capsule",
            Look::BrickSteel => "brick & steel",
            Look::Lantern => "lantern alley",
        }
    }

    /// Floors for a new building of this look, from what the land use and
    /// tier drew (`floors`) and the block's density cap (`cap`). Kowloon may
    /// overshoot the cap: the point of the stacks is that they rise anyway.
    pub fn floors(self, floors: u8, cap: u8) -> u8 {
        let f = match self {
            Look::Kowloon => (floors as u16 + 3).min(cap as u16 + 4) as u8,
            Look::Suburban => floors.min(3),
            Look::Eco => floors.min(cap.max(1)).min(10),
            Look::NeonStrip | Look::Tourist => (floors + 1).min(cap.max(1) + 1),
            Look::TechCampus => (floors + 1).min(cap.max(1) + 1),
            Look::NightMarket => floors.min(2),
            Look::Historic => floors.min(4),
            // Domes keep to the block's density, one floor over at most.
            Look::Capsule => floors.min(cap.max(1) + 1),
            // [r4-brick] Low-rise: rows of three or four, lofts to ten.
            Look::BrickSteel => floors.min(cap.max(1) + 1).min(10),
            // [lantern] Two to five storeys: shophouses, walk-ups, halls.
            Look::Lantern => floors.max(2).min(5).min(cap.max(2) + 1),
        };
        f.max(1)
    }

    /// The tier a new building of this look is built at, from the tier the
    /// land supports: the stacks and the market stalls are cheap whatever
    /// the land, a campus is built a step up, suburbs are never slums.
    pub fn tier(self, tier: Tier) -> Tier {
        match self {
            Look::Kowloon if tier == Tier::High => Tier::Mid,
            Look::Kowloon => Tier::Low,
            Look::NightMarket => tier.down(),
            Look::TechCampus => tier.up(),
            Look::Suburban if tier == Tier::Low => Tier::Mid,
            // Super high tech: never built cheap.
            Look::Capsule if tier == Tier::Low => Tier::Mid,
            // [lantern] Lanes, not towers: dear land builds a good
            // shophouse, never a glass tower.
            Look::Lantern if tier == Tier::High => Tier::Mid,
            _ => tier,
        }
    }

    /// Businesses this look favours among the ones the market says are
    /// viable, and by how much (others weigh 1).
    pub fn favours(self) -> &'static [(BusinessKind, f32)] {
        match self {
            Look::NeonStrip => &[(BusinessKind::GadgetShop, 4.0), (BusinessKind::Bakery, 2.0)],
            Look::TechCampus => &[(BusinessKind::Office, 5.0), (BusinessKind::GadgetShop, 2.0)],
            Look::NightMarket => &[(BusinessKind::Grocery, 3.0), (BusinessKind::Bakery, 4.0)],
            Look::Tourist => &[(BusinessKind::Hotel, 6.0), (BusinessKind::GadgetShop, 2.0), (BusinessKind::Bakery, 2.0)],
            Look::Capsule => &[(BusinessKind::Office, 4.0), (BusinessKind::GadgetShop, 3.0)],
            // [r4-brick] Workers' shops: the baker and the grocer.
            Look::BrickSteel => &[(BusinessKind::Bakery, 2.0), (BusinessKind::Grocery, 2.0)],
            // [lantern] Food first: grocers, bakers and kitchens open late.
            Look::Lantern => &[(BusinessKind::Bakery, 4.0), (BusinessKind::Grocery, 3.0), (BusinessKind::Venue, 2.0)],
            _ => &[],
        }
    }

    /// [capsule] Whether the favoured trades are the only ones this look
    /// opens (when the market can carry any of them): the Capsule district
    /// is high tech only. Hotels for a queue of visitors and the trades the
    /// economy opens by pressure (car dealers and the like) still come.
    pub fn favours_only(self) -> bool {
        self == Look::Capsule
    }

    /// Pull on the land value around each building of this look (radius m,
    /// weight), on top of what its land use already does.
    pub fn land_value(self) -> Option<(f32, f32)> {
        match self {
            Look::Eco => Some((140.0, 0.06)),
            Look::Historic => Some((120.0, 0.05)),
            Look::TechCampus => Some((120.0, 0.03)),
            Look::Capsule => Some((120.0, 0.04)),
            _ => None,
        }
    }
}

/// A district: what the player painted and named, and its policies.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct District {
    /// Its value in the grid; never 0.
    pub id: u16,
    pub name: String,
    /// Index into [`COLORS`]: border and label colour.
    pub color: u8,
    #[serde(default)]
    pub residential: Option<ResidentialSpec>,
    #[serde(default)]
    pub commercial: Option<CommercialSpec>,
    /// Industry sub-zone the district's industrial land takes (an
    /// `industry_bias` that follows the district's cells).
    #[serde(default)]
    pub industry: Option<IndustryKind>,
    /// No upgrades, no decay of tier, no redevelopment: the district stays
    /// as it stands, and draws visitors for it.
    #[serde(default)]
    pub historic: bool,
    #[serde(default)]
    pub created_tick: u64,
    /// [supertall] Nothing new rises over this (m) here; `None` follows
    /// the city's limit.
    #[serde(default)]
    pub height_limit: Option<f32>,
    /// [stories] The district's story, which set the dials above; `None`
    /// is no story (nothing set) or, with dials set, a custom district
    /// from an older save whose dials no story has.
    #[serde(default)]
    pub story: Option<crate::stories::Story>,
    /// [stories] Its own taxes and levers (the story's to start with; the
    /// player fine-tunes them).
    #[serde(default)]
    pub policies: crate::stories::Policies,
    /// [stories] What the policies moved, today and yesterday.
    #[serde(default)]
    pub books: DistrictBooks,
}

/// [stories] A district's money from its policies, by policy (positive:
/// to the city; negative: the city paid), today and the whole of yesterday.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DistrictBooks {
    #[serde(default)]
    pub today: [f32; crate::stories::POLICIES],
    #[serde(default)]
    pub yesterday: [f32; crate::stories::POLICIES],
}

impl DistrictBooks {
    /// Levied (to the city) yesterday.
    pub fn levied_yesterday(&self) -> f32 {
        self.yesterday.iter().filter(|x| **x > 0.0).sum()
    }

    /// Paid out by the city yesterday.
    pub fn spent_yesterday(&self) -> f32 {
        -self.yesterday.iter().filter(|x| **x < 0.0).sum::<f32>()
    }

    pub fn roll(&mut self) {
        self.yesterday = self.today;
        self.today = [0.0; crate::stories::POLICIES];
    }
}

impl District {
    /// The look a new building of `lu` gets here, if the district gives one.
    pub fn look_for(&self, lu: LandUse) -> Option<Look> {
        if self.historic && matches!(lu, LandUse::Residential | LandUse::Commercial) {
            return Some(Look::Historic);
        }
        match lu {
            LandUse::Residential => self.residential.map(|s| match s {
                ResidentialSpec::Kowloon => Look::Kowloon,
                ResidentialSpec::Suburban => Look::Suburban,
                ResidentialSpec::Eco => Look::Eco,
                ResidentialSpec::Capsule => Look::Capsule,
                ResidentialSpec::BrickSteel => Look::BrickSteel,
                ResidentialSpec::Lantern => Look::Lantern,
            }),
            // Nightlife on a strip or in a market takes the street's look.
            LandUse::Commercial | LandUse::Entertainment => self.commercial.and_then(|s| match s {
                CommercialSpec::NeonStrip => Some(Look::NeonStrip),
                CommercialSpec::NightMarket => Some(Look::NightMarket),
                CommercialSpec::TechCampus if lu == LandUse::Commercial => Some(Look::TechCampus),
                CommercialSpec::Tourist if lu == LandUse::Commercial => Some(Look::Tourist),
                // Nights out under a dome too.
                CommercialSpec::Capsule => Some(Look::Capsule),
                // [r4-brick] The pub and the union hall are the nights out.
                CommercialSpec::BrickSteel => Some(Look::BrickSteel),
                CommercialSpec::Lantern => Some(Look::Lantern),
                _ => None,
            }),
            // [r4-brick] Brick & Steel builds its works in brick too.
            LandUse::Industrial if self.residential == Some(ResidentialSpec::BrickSteel) || self.commercial == Some(CommercialSpec::BrickSteel) => Some(Look::BrickSteel),
            // [lantern] The lanes' small industry: workshops and kitchens.
            LandUse::Industrial if self.commercial == Some(CommercialSpec::Lantern) => Some(Look::Lantern),
            _ => None,
        }
    }

    /// [stories] Set the story: its architecture, height limit and
    /// policies replace the district's (`None`: no story, nothing set but
    /// the height limit, which stays).
    pub fn apply_story(&mut self, story: Option<crate::stories::Story>) {
        self.story = story;
        match story {
            Some(s) => {
                let p = s.preset();
                self.residential = p.residential;
                self.commercial = p.commercial;
                self.industry = p.industry;
                self.historic = p.historic;
                self.height_limit = p.height_limit;
                self.policies = p.policies;
            }
            None => {
                self.residential = None;
                self.commercial = None;
                self.industry = None;
                self.historic = false;
                self.policies = Default::default();
            }
        }
    }

    /// [stories] A custom district: building-type dials no story has (an
    /// older save's, or set through the API).
    pub fn is_custom(&self) -> bool {
        self.story.is_none() && (self.residential.is_some() || self.commercial.is_some() || self.industry.is_some() || self.historic)
    }

    /// [stories] The story's name, "Custom" or "No story".
    pub fn story_label(&self) -> &'static str {
        match self.story {
            Some(s) => s.label(),
            None if self.is_custom() => "Custom",
            None => "No story",
        }
    }

    /// One line of what the district's policies are, for lists.
    pub fn policy_label(&self) -> String {
        if let Some(s) = self.story {
            return s.label().to_string();
        }
        if !self.is_custom() {
            return "No story".to_string();
        }
        let mut parts: Vec<String> = Vec::new();
        if let Some(r) = self.residential {
            parts.push(r.label().to_string());
        }
        if let Some(c) = self.commercial {
            parts.push(c.label().to_string());
        }
        if let Some(i) = self.industry {
            parts.push(i.label().to_string());
        }
        if self.historic {
            parts.push("Historic".to_string());
        }
        format!("Custom: {}", parts.join(", "))
    }

    /// Draws visitors in its own right: a landmark district.
    pub fn is_landmark(&self) -> bool {
        self.historic || matches!(self.commercial, Some(CommercialSpec::NeonStrip | CommercialSpec::NightMarket | CommercialSpec::Tourist | CommercialSpec::Lantern))
    }
}

/// Border and label colours (linear RGB, a little over 1 to glow).
pub const COLORS: [[f32; 3]; 10] = [
    [1.4, 0.25, 0.9],
    [0.2, 1.2, 1.4],
    [1.4, 0.9, 0.15],
    [0.4, 1.4, 0.35],
    [1.4, 0.35, 0.3],
    [0.55, 0.5, 1.5],
    [1.3, 1.3, 1.3],
    [1.4, 0.55, 0.15],
    [0.2, 1.4, 0.8],
    [1.2, 0.3, 0.55],
];

/// District names drawn from when the player does not give one.
const NAMES_A: [&str; 16] = ["Lantern", "Chrome", "Neon", "Jade", "Copper", "Signal", "Static", "Velvet", "Iron", "Paper", "Glass", "Harbour", "Ember", "Silk", "Cobalt", "Monsoon"];
const NAMES_B: [&str; 12] = ["Row", "Heights", "Quarter", "Flats", "Gardens", "Market", "Reach", "Commons", "Wharf", "Terraces", "Cross", "Hollow"];

/// A name for the `n`th district of a city with `seed`, deterministic.
pub fn default_name(seed: u64, n: u16) -> String {
    let h = (seed ^ (n as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let a = NAMES_A[((h >> 7) % NAMES_A.len() as u64) as usize];
    let b = NAMES_B[((h >> 29) % NAMES_B.len() as u64) as usize];
    format!("{a} {b}")
}

/// The district raster. `ids[row * cols + col]`, row 0 at the smallest y.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(into = "SavedGrid", from = "SavedGrid")]
pub struct DistrictGrid {
    pub cols: u32,
    pub rows: u32,
    pub ids: Vec<u16>,
}

/// The grid as saved: runs of (id, count).
#[derive(Clone, Serialize, Deserialize)]
struct SavedGrid {
    cols: u32,
    rows: u32,
    runs: Vec<(u16, u32)>,
}

impl From<DistrictGrid> for SavedGrid {
    fn from(g: DistrictGrid) -> Self {
        let mut runs: Vec<(u16, u32)> = Vec::new();
        for &id in &g.ids {
            match runs.last_mut() {
                Some((last, n)) if *last == id => *n += 1,
                _ => runs.push((id, 1)),
            }
        }
        SavedGrid { cols: g.cols, rows: g.rows, runs }
    }
}

impl From<SavedGrid> for DistrictGrid {
    fn from(s: SavedGrid) -> Self {
        let mut g = DistrictGrid::default();
        // A save with another grid size (or a corrupt one) loads empty
        // rather than misplacing every district.
        if s.cols != g.cols || s.rows != g.rows {
            return g;
        }
        let mut i = 0usize;
        for (id, n) in s.runs {
            for _ in 0..n {
                if i < g.ids.len() {
                    g.ids[i] = id;
                }
                i += 1;
            }
        }
        g
    }
}

impl Default for DistrictGrid {
    fn default() -> Self {
        let n = ((2.0 * HALF) / CELL).ceil() as u32;
        Self { cols: n, rows: n, ids: vec![0; (n * n) as usize] }
    }
}

impl DistrictGrid {
    /// Cell holding `p`, if on the map.
    pub fn cell_of(&self, p: Vec2) -> Option<(u32, u32)> {
        let c = ((p.x + HALF) / CELL).floor();
        let r = ((p.y + HALF) / CELL).floor();
        (c >= 0.0 && r >= 0.0 && (c as u32) < self.cols && (r as u32) < self.rows).then_some((c as u32, r as u32))
    }

    /// Centre of cell (col, row).
    pub fn center(&self, col: u32, row: u32) -> Vec2 {
        Vec2::new(-HALF + (col as f32 + 0.5) * CELL, -HALF + (row as f32 + 0.5) * CELL)
    }

    pub fn get(&self, col: u32, row: u32) -> u16 {
        self.ids[(row * self.cols + col) as usize]
    }

    /// District id at a point (0 = none).
    pub fn at(&self, p: Vec2) -> u16 {
        self.cell_of(p).map_or(0, |(c, r)| self.get(c, r))
    }

    /// Set every cell whose centre is within `radius` of `center` to `id`
    /// (0 erases). Returns the cells changed.
    pub fn paint_circle(&mut self, center: Vec2, radius: f32, id: u16) -> usize {
        let r = radius.max(CELL * 0.5);
        self.paint_where(center - Vec2::splat(r), center + Vec2::splat(r), id, |p| p.distance_squared(center) <= r * r)
    }

    /// Set every cell whose centre is inside `poly` to `id`.
    pub fn paint_polygon(&mut self, poly: &[Vec2], id: u16) -> usize {
        if poly.len() < 3 {
            return 0;
        }
        let lo = poly.iter().fold(Vec2::splat(f32::MAX), |a, p| a.min(*p));
        let hi = poly.iter().fold(Vec2::splat(f32::MIN), |a, p| a.max(*p));
        self.paint_where(lo, hi, id, |p| point_in_polygon(p, poly))
    }

    fn paint_where(&mut self, lo: Vec2, hi: Vec2, id: u16, inside: impl Fn(Vec2) -> bool) -> usize {
        let to_c = |x: f32| (((x + HALF) / CELL).floor().max(0.0) as u32).min(self.cols - 1);
        let to_r = |y: f32| (((y + HALF) / CELL).floor().max(0.0) as u32).min(self.rows - 1);
        let (c0, c1, r0, r1) = (to_c(lo.x), to_c(hi.x), to_r(lo.y), to_r(hi.y));
        let mut changed = 0;
        for r in r0..=r1 {
            for c in c0..=c1 {
                let i = (r * self.cols + c) as usize;
                if self.ids[i] != id && inside(self.center(c, r)) {
                    self.ids[i] = id;
                    changed += 1;
                }
            }
        }
        changed
    }

    /// Replace every cell of `id` (removing a district).
    pub fn clear_id(&mut self, id: u16) -> usize {
        let mut n = 0;
        for x in self.ids.iter_mut().filter(|x| **x == id) {
            *x = 0;
            n += 1;
        }
        n
    }

    /// Cells, summed centre and bounding box of each id, in one pass.
    pub fn extents(&self) -> std::collections::BTreeMap<u16, Extent> {
        let mut out: std::collections::BTreeMap<u16, Extent> = std::collections::BTreeMap::new();
        for r in 0..self.rows {
            for c in 0..self.cols {
                let id = self.get(c, r);
                if id == 0 {
                    continue;
                }
                let p = self.center(c, r);
                let e = out.entry(id).or_insert(Extent { cells: 0, sum: Vec2::ZERO, lo: p, hi: p });
                e.cells += 1;
                e.sum += p;
                e.lo = e.lo.min(p);
                e.hi = e.hi.max(p);
            }
        }
        out
    }

    /// Every cell edge with a district on at least one side and something
    /// else on the other: the borders, as (a, b, id inside). An edge between
    /// two districts is listed once for each, so each draws its own colour.
    pub fn borders(&self) -> Vec<(Vec2, Vec2, u16)> {
        let mut out = Vec::new();
        let corner = |c: u32, r: u32| Vec2::new(-HALF + c as f32 * CELL, -HALF + r as f32 * CELL);
        for r in 0..self.rows {
            for c in 0..self.cols {
                let id = self.get(c, r);
                if id == 0 {
                    continue;
                }
                let left = if c == 0 { 0 } else { self.get(c - 1, r) };
                let right = if c + 1 >= self.cols { 0 } else { self.get(c + 1, r) };
                let down = if r == 0 { 0 } else { self.get(c, r - 1) };
                let up = if r + 1 >= self.rows { 0 } else { self.get(c, r + 1) };
                if left != id {
                    out.push((corner(c, r), corner(c, r + 1), id));
                }
                if right != id {
                    out.push((corner(c + 1, r), corner(c + 1, r + 1), id));
                }
                if down != id {
                    out.push((corner(c, r), corner(c + 1, r), id));
                }
                if up != id {
                    out.push((corner(c, r + 1), corner(c + 1, r + 1), id));
                }
            }
        }
        out
    }
}

/// Where a district lies: its cells, their centre and bounds.
#[derive(Clone, Copy, Debug)]
pub struct Extent {
    pub cells: u32,
    pub sum: Vec2,
    pub lo: Vec2,
    pub hi: Vec2,
}

impl Extent {
    pub fn centroid(&self) -> Vec2 {
        self.sum / self.cells.max(1) as f32
    }

    pub fn area_m2(&self) -> f32 {
        self.cells as f32 * CELL * CELL
    }
}

/// Every district and the map of where they are.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Districts {
    pub list: Vec<District>,
    pub grid: DistrictGrid,
    #[serde(default)]
    pub next_id: u16,
    /// Bumped on every change, so the renderer redraws the layer only then.
    #[serde(skip)]
    pub version: u64,
}

impl Districts {
    pub fn get(&self, id: u16) -> Option<&District> {
        self.list.iter().find(|d| d.id == id)
    }

    pub fn get_mut(&mut self, id: u16) -> Option<&mut District> {
        self.list.iter_mut().find(|d| d.id == id)
    }

    /// The district at a point.
    pub fn at(&self, p: Vec2) -> Option<&District> {
        match self.grid.at(p) {
            0 => None,
            id => self.get(id),
        }
    }

    /// By id (as a number or string) or by name, case-insensitive.
    pub fn find(&self, key: &str) -> Option<u16> {
        if let Ok(n) = key.trim().parse::<u16>() {
            if self.get(n).is_some() {
                return Some(n);
            }
        }
        self.list.iter().find(|d| d.name.eq_ignore_ascii_case(key.trim())).map(|d| d.id)
    }

    /// A new, empty district. `None` once [`MAX_DISTRICTS`] exist.
    pub fn create(&mut self, name: Option<String>, seed: u64, tick: u64) -> Option<u16> {
        if self.list.len() >= MAX_DISTRICTS {
            return None;
        }
        self.next_id = self.next_id.max(self.list.iter().map(|d| d.id).max().unwrap_or(0)) + 1;
        let id = self.next_id;
        let name = name.filter(|n| !n.trim().is_empty()).unwrap_or_else(|| default_name(seed, id));
        self.list.push(District { id, name, color: ((id - 1) as usize % COLORS.len()) as u8, residential: None, commercial: None, industry: None, historic: false, created_tick: tick, height_limit: None, story: None, policies: Default::default(), books: Default::default() });
        self.version += 1;
        Some(id)
    }

    /// Remove a district and erase its cells.
    pub fn remove(&mut self, id: u16) -> bool {
        let before = self.list.len();
        self.list.retain(|d| d.id != id);
        self.grid.clear_id(id);
        self.version += 1;
        self.list.len() != before
    }

    /// Paint by brush (`id` 0 erases). Districts left with no cells are
    /// kept: the player may paint them again; `prune` drops them.
    pub fn paint_circle(&mut self, center: Vec2, radius: f32, id: u16) -> usize {
        let n = self.grid.paint_circle(center, radius, id);
        if n > 0 {
            self.version += 1;
        }
        n
    }

    pub fn paint_polygon(&mut self, poly: &[Vec2], id: u16) -> usize {
        let n = self.grid.paint_polygon(poly, id);
        if n > 0 {
            self.version += 1;
        }
        n
    }

    /// [stories] Districts from a save before stories: those whose dials
    /// are exactly a story's take it (their height limit kept, the story's
    /// policies to start with); the rest stay custom, or no story when
    /// nothing was set. Run on every load: it only touches districts with
    /// no story whose dials match one, which a loaded custom one never does.
    /// [r4-brick] A district with a story whose looks changed since the
    /// save (a story that got its own look) takes the story's new ones.
    pub fn adopt_stories(&mut self) -> usize {
        let mut n = 0;
        for d in self.list.iter_mut().filter(|d| d.story.is_none()) {
            if let Some(s) = crate::stories::Story::matching(d) {
                let height = d.height_limit;
                d.apply_story(Some(s));
                d.height_limit = height;
                n += 1;
            }
        }
        // [r4-brick] A story that got its own look since the save (Foundry
        // Row's Brick & Steel, Lantern Lanes' Lantern Alley): the district takes the story's building
        // types now; its policies and height limit stay the player's.
        for d in self.list.iter_mut() {
            if let Some(s) = d.story.filter(|s| !s.fits(d)) {
                let p = s.preset();
                d.residential = p.residential;
                d.commercial = p.commercial;
                d.industry = p.industry;
                d.historic = p.historic;
                n += 1;
            }
        }
        for d in self.list.iter_mut() {
            d.policies = d.policies.clamped();
        }
        if n > 0 {
            self.version += 1;
        }
        n
    }

    /// [stories] The policies of the district at `p`, when any is set.
    pub fn policies_at(&self, p: Vec2) -> Option<(u16, crate::stories::Policies)> {
        self.at(p).filter(|d| !d.policies.is_none()).map(|d| (d.id, d.policies))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brush_paints_and_erases() {
        let mut d = Districts::default();
        let id = d.create(None, 7, 0).unwrap();
        let n = d.paint_circle(Vec2::new(100.0, 50.0), 40.0, id);
        assert!(n > 15, "a 40 m brush covers some cells: {n}");
        assert_eq!(d.at(Vec2::new(100.0, 50.0)).map(|x| x.id), Some(id));
        assert!(d.at(Vec2::new(300.0, 50.0)).is_none());
        // Painting the same again changes nothing.
        assert_eq!(d.paint_circle(Vec2::new(100.0, 50.0), 40.0, id), 0);
        d.paint_circle(Vec2::new(100.0, 50.0), 10.0, 0);
        assert!(d.at(Vec2::new(100.0, 50.0)).is_none());
    }

    #[test]
    fn polygon_fill_and_area() {
        let mut d = Districts::default();
        let id = d.create(Some("Old Town".into()), 1, 0).unwrap();
        let sq = [Vec2::new(0.0, 0.0), Vec2::new(160.0, 0.0), Vec2::new(160.0, 160.0), Vec2::new(0.0, 160.0)];
        assert_eq!(d.paint_polygon(&sq, id), 100);
        let ext = d.grid.extents();
        let e = ext[&id];
        assert_eq!(e.cells, 100);
        assert!((e.area_m2() - 160.0 * 160.0).abs() < 1.0);
        assert!(e.centroid().distance(Vec2::new(80.0, 80.0)) < 1.0);
        // A closed square has 4 * 10 border edges.
        assert_eq!(d.grid.borders().len(), 40);
        assert_eq!(d.find("old town"), Some(id));
    }

    #[test]
    fn grid_round_trips_through_ron_compactly() {
        let mut d = Districts::default();
        let a = d.create(None, 3, 0).unwrap();
        let b = d.create(None, 3, 0).unwrap();
        d.paint_circle(Vec2::new(-200.0, 0.0), 60.0, a);
        d.paint_circle(Vec2::new(200.0, 30.0), 90.0, b);
        let text = ron::to_string(&d).unwrap();
        assert!(text.len() < 8_000, "run-length encoded: {} bytes", text.len());
        let back: Districts = ron::from_str(&text).unwrap();
        assert_eq!(back.grid, d.grid);
        assert_eq!(back.list.len(), 2);
        assert_ne!(back.list[0].name, "");
    }

    #[test]
    fn looks_follow_policies() {
        let mut d = Districts::default();
        let id = d.create(None, 1, 0).unwrap();
        let dist = d.get_mut(id).unwrap();
        dist.residential = Some(ResidentialSpec::Kowloon);
        dist.commercial = Some(CommercialSpec::TechCampus);
        assert_eq!(dist.look_for(LandUse::Residential), Some(Look::Kowloon));
        assert_eq!(dist.look_for(LandUse::Commercial), Some(Look::TechCampus));
        assert_eq!(dist.look_for(LandUse::Entertainment), None);
        assert_eq!(dist.look_for(LandUse::Industrial), None);
        dist.historic = true;
        assert_eq!(dist.look_for(LandUse::Residential), Some(Look::Historic));
        // Kowloon builds cheap and tall, suburbs low, night markets low.
        assert_eq!(Look::Kowloon.tier(Tier::High), Tier::Mid);
        assert!(Look::Kowloon.floors(4, 3) > 4);
        assert!(Look::Suburban.floors(9, 20) <= 3);
        assert!(Look::NightMarket.floors(6, 20) <= 2);
        assert_eq!(Look::TechCampus.tier(Tier::Mid), Tier::High);
    }

    /// [capsule] The Capsule spec gives the Capsule look to homes, shops and
    /// nights out; it is never cheap, keeps tall buildings tall, parses from
    /// the API's keys, and a district saved before it existed still loads.
    #[test]
    fn capsule_look_and_old_saves() {
        let mut d = Districts::default();
        let id = d.create(None, 1, 0).unwrap();
        let dist = d.get_mut(id).unwrap();
        dist.residential = Some(ResidentialSpec::Capsule);
        dist.commercial = Some(CommercialSpec::Capsule);
        for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment] {
            assert_eq!(dist.look_for(lu), Some(Look::Capsule));
        }
        assert_eq!(dist.look_for(LandUse::Industrial), None);
        assert_eq!(Look::Capsule.tier(Tier::Low), Tier::Mid);
        assert_eq!(Look::Capsule.tier(Tier::High), Tier::High);
        assert_eq!(Look::Capsule.floors(30, 40), 30);
        assert!(Look::Capsule.favours_only() && !Look::TechCampus.favours_only());
        assert_eq!(ResidentialSpec::parse("capsule"), Some(ResidentialSpec::Capsule));
        assert_eq!(CommercialSpec::parse("Capsule"), Some(CommercialSpec::Capsule));
        assert_eq!(Look::ALL[Look::Capsule.index()], Look::Capsule);
        // An old district, as saved, round trips; a new one too.
        let old = "(id: 3, name: \"Jade Row\", color: 2, residential: Some(Eco), commercial: Some(Tourist), industry: None, historic: false)";
        let back: District = ron::from_str(old).expect("old district loads");
        assert_eq!(back.residential, Some(ResidentialSpec::Eco));
        let text = ron::to_string(&*dist).unwrap();
        let again: District = ron::from_str(&text).unwrap();
        assert_eq!(again.residential, Some(ResidentialSpec::Capsule));
        assert_eq!(ron::from_str::<Look>("Historic").unwrap(), Look::Historic);
        assert_eq!(ron::from_str::<Look>("Capsule").unwrap(), Look::Capsule);
    }

    /// [r4-brick] Brick & Steel gives its look to homes, shops, nights out
    /// and the works; it keeps the district low; and a Foundry Row district
    /// saved before the look (dials: manufacturing only) takes it on load,
    /// keeping the player's policies and height limit.
    #[test]
    fn brick_steel_look_and_old_foundry_row() {
        use crate::stories::Story;
        let mut d = Districts::default();
        let id = d.create(None, 1, 0).unwrap();
        let dist = d.get_mut(id).unwrap();
        dist.apply_story(Some(Story::FoundryRow));
        for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment, LandUse::Industrial] {
            assert_eq!(dist.look_for(lu), Some(Look::BrickSteel), "{lu:?}");
        }
        assert_eq!(dist.industry, Some(IndustryKind::Manufacturing));
        assert_eq!(Look::BrickSteel.floors(30, 40), 10);
        assert_eq!(Look::BrickSteel.floors(3, 2), 3);
        assert_eq!(ResidentialSpec::parse("brick_steel"), Some(ResidentialSpec::BrickSteel));
        assert_eq!(CommercialSpec::parse("Brick & Steel"), Some(CommercialSpec::BrickSteel));
        assert_eq!(Look::ALL[Look::BrickSteel.index()], Look::BrickSteel);
        // Other districts' works keep no look.
        let other = d.create(None, 2, 0).unwrap();
        d.get_mut(other).unwrap().apply_story(Some(Story::Capsule));
        assert_eq!(d.get(other).unwrap().look_for(LandUse::Industrial), None);
        // As saved before the look: Foundry Row with manufacturing alone.
        let old = "(id: 3, name: \"Eastworks\", color: 2, residential: None, commercial: None, industry: Some(Manufacturing), historic: false, height_limit: Some(30.0), story: Some(FoundryRow), policies: (pollution_levy: 0.12, rent_support: 0.1))";
        let back: District = ron::from_str(old).expect("old district loads");
        let mut ds = Districts::default();
        ds.list.push(back);
        assert_eq!(ds.adopt_stories(), 1);
        let got = &ds.list[0];
        assert_eq!(got.story, Some(Story::FoundryRow));
        assert!(Story::FoundryRow.fits(got));
        assert_eq!(got.residential, Some(ResidentialSpec::BrickSteel));
        assert_eq!(got.height_limit, Some(30.0));
        assert_eq!(got.policies.pollution_levy, 0.12);
        // Run again (every load): nothing more to do.
        assert_eq!(ds.adopt_stories(), 0);
    }

    /// [lantern] The Lantern spec gives Lantern Alley to homes, shops,
    /// nights out and (the one look that does) small industry; it keeps to
    /// two to five storeys and never builds a high-tier tower. A Lantern
    /// Lanes district saved while the story still borrowed the night
    /// market's look takes its own on load, keeping its height limit and
    /// policies; other stories' districts are left as they were.
    #[test]
    fn lantern_look_and_old_lantern_lanes_saves() {
        use crate::stories::Story;
        let mut d = Districts::default();
        let id = d.create(None, 1, 0).unwrap();
        let dist = d.get_mut(id).unwrap();
        dist.apply_story(Some(Story::LanternLanes));
        for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment, LandUse::Industrial] {
            assert_eq!(dist.look_for(lu), Some(Look::Lantern), "{lu:?}");
        }
        assert_eq!(dist.look_for(LandUse::Civic), None);
        assert_eq!(Look::Lantern.tier(Tier::High), Tier::Mid);
        assert_eq!(Look::Lantern.tier(Tier::Low), Tier::Low);
        assert_eq!(Look::Lantern.floors(1, 1), 2);
        assert_eq!(Look::Lantern.floors(12, 30), 5);
        assert_eq!(Look::ALL[Look::Lantern.index()], Look::Lantern);
        assert_eq!(ResidentialSpec::parse("lantern_alley"), Some(ResidentialSpec::Lantern));
        assert_eq!(CommercialSpec::parse("Lantern Alley"), Some(CommercialSpec::Lantern));
        // Only a Lantern district dresses its industry.
        let mut wq = District { id: 9, ..dist.clone() };
        wq.apply_story(Some(Story::WalledQuarter));
        assert_eq!(wq.look_for(LandUse::Industrial), None);
        // As saved before Lantern Alley: the story, the night market's
        // dials, a height limit and a tuned policy.
        let old = "(id: 3, name: \"Paper Row\", color: 2, residential: None, commercial: Some(NightMarket), industry: None, historic: false, height_limit: Some(30.0), story: Some(LanternLanes), policies: (business_rate: -0.04, late_hours: true))";
        let mut saved = Districts::default();
        saved.list.push(ron::from_str(old).expect("old district loads"));
        saved.list.push(District { id: 4, ..wq.clone() });
        assert!(saved.adopt_stories() >= 1);
        let lanes = &saved.list[0];
        assert_eq!((lanes.residential, lanes.commercial), (Some(ResidentialSpec::Lantern), Some(CommercialSpec::Lantern)));
        assert_eq!(lanes.story, Some(Story::LanternLanes));
        assert_eq!(lanes.height_limit, Some(30.0));
        assert_eq!(lanes.policies.business_rate, -0.04);
        assert!(Story::LanternLanes.fits(lanes));
        // The Walled Quarter keeps the night market.
        assert_eq!(saved.list[1].commercial, Some(CommercialSpec::NightMarket));
        assert_eq!(saved.list[1].residential, Some(ResidentialSpec::Kowloon));
    }
}
