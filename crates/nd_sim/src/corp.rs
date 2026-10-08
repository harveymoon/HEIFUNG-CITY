//! [megacorp] Mega-corporations and universities: brands, logos and the
//! records the `World` keeps for them (`world/corp.rs`, `world/campus.rs`).
//!
//! *Brands* are procedural and fictional: a coined name from syllables
//! (never a real company), a colour from [`BRAND_COLOURS`], a logo shape and
//! a one-letter monogram. The logo is a small pixel mark
//! ([`Brand::logo_bitmap`]) that the renderer lights on rooftops, banners,
//! the HQ crown and the side of a fleet's trucks, and the UI paints beside
//! the owner's name, so all of them show the same mark.
//!
//! *Mega-corps* grow out of a company (an owner record or a lone business)
//! that passes [`PROMOTE_REVENUE`], [`PROMOTE_STAFF`] and [`PROMOTE_HOLDINGS`].
//! *Universities* are placed by the player (`ServiceKind::University`) and
//! act like a corp round their campus: dorms, annexes and plazas.
//!
//! Everything is saved with serde defaults: an older save loads with no
//! corporations, no campuses and the default rules.

use crate::buildings::BuildingId;
use crate::citizens::CitizenId;
use crate::estate::OwnerId;
use crate::market::BusinessKind;
use crate::zoning::LotId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── tuning ───────────────────────────────────────────────────────────────────

/// A company becomes a mega-corp when its businesses take this much a day
/// (the last three days' average, summed over its holdings)...
pub const PROMOTE_REVENUE: f32 = 2_500.0;
/// ...employ this many...
pub const PROMOTE_STAFF: usize = 40;
/// ...and it holds this many properties (or twice the staff and revenue
/// in one big building).
pub const PROMOTE_HOLDINGS: usize = 2;
/// People in town before any company is big enough to count.
pub const CORP_MIN_POP: usize = 600;
/// People per mega-corp the town carries, and the hard cap.
pub const CORP_PEOPLE_EACH: usize = 3000;
pub const CORP_MAX: usize = 4;
/// Chance a day the biggest qualifying company is promoted.
pub const PROMOTE_CHANCE: f64 = 0.25;

/// Default share of the town's ordinary buildings one corp may own.
pub const DEFAULT_MAX_SHARE: f32 = 0.08;
/// The player's cap on that share can go from here...
pub const MIN_MAX_SHARE: f32 = 0.02;
/// ...to here.
pub const MAX_MAX_SHARE: f32 = 0.25;
/// At most this share of a day's gross revenue taken as the levy.
pub const MAX_LEVY: f32 = 0.2;
/// A corp always may hold this many, whatever the share says.
pub const MIN_CAP: usize = 4;

/// Share of a holding's profit the corp takes into its own purse daily.
pub const SKIM: f32 = 0.4;
/// A business keeps at least this much cash after the skim.
pub const SKIM_FLOOR: f32 = 500.0;
/// The corp's deposit on a purchase: this share of the price at the
/// estate's scale (the rest is the bank's), and at least this much cash
/// in hand to buy at all.
pub const DEPOSIT_SHARE: f64 = 0.01;
pub const BUY_MIN_CASH: f64 = 400.0;
/// Purchases a day at most.
pub const BUYS_PER_DAY: usize = 3;
/// A corp buys homes for its staff until it has this many homes per
/// member of staff.
pub const HOUSING_PER_STAFF: f32 = 0.6;
/// Staff in company housing pay this much less rent.
pub const STAFF_RENT_DISCOUNT: f32 = 0.3;
/// Staff households moved into company housing a day, per corp.
pub const HOUSING_MOVES_PER_DAY: usize = 6;
/// Lots further than this by road from the HQ are not bought.
pub const MAX_DRIVE_M: f32 = 6_000.0;
/// A kind the corp does not own yet counts as this much nearer: the corp
/// spreads into every industry, the nearest first.
pub const NEW_KIND_PULL: f32 = 0.7;
/// HQ floors: at least this many, at most the pressure ceiling.
pub const HQ_MIN_FLOORS: u8 = 18;
pub const HQ_MAX_FLOORS: u8 = 40;
/// Plots an HQ site is assembled from, at most.
pub const HQ_MAX_PLOTS: usize = 3;

// Universities.

/// Lots within this distance of the campus (centre to centre) are "near
/// campus": dorms, annexes and plazas, and the student trade.
pub const CAMPUS_REACH: f32 = 420.0;
/// Shops and bars this close feel the students (full at the gate, none at
/// the edge).
pub const STUDENT_TRADE_REACH: f32 = 600.0;
/// Students' age band.
pub const STUDENT_AGES: std::ops::RangeInclusive<u8> = 18..=24;
/// Annexes a campus may build, by tier.
pub const ANNEX_CAP: [usize; 3] = [4, 7, 12];
/// Daily tuition a student pays the campus (from their pocket).
pub const TUITION: f32 = 6.0;
/// Dorm rent discount on the building's rent.
pub const DORM_RENT_DISCOUNT: f32 = 0.45;
/// Students arriving to live in the dorms, a day at most.
pub const DORM_ARRIVALS_PER_DAY: usize = 4;
/// Minute of the day students head to class, and the hour classes end.
pub const CLASS_MINUTE: u64 = 9 * 60;
pub const CLASS_END_HOUR: u8 = 15;
/// Minute of the evening students go out, and the share of them that do.
pub const NIGHT_OUT_MINUTE: u64 = 20 * 60 + 30;
pub const NIGHT_OUT_SHARE: f32 = 0.35;
/// Extra walk-in shoppers a pizza place, café or bookshop by campus sees per
/// enrolled student an hour while open (at the gate).
pub const STUDENT_FEET_PER_HOUR: f32 = 0.05;

// ── brands ───────────────────────────────────────────────────────────────────

/// Brand colours, sRGB: vivid enough to read on a rooftop at night.
pub const BRAND_COLOURS: [[u8; 3]; 12] = [
    [235, 35, 65],
    [255, 125, 20],
    [255, 196, 10],
    [150, 232, 40],
    [25, 205, 115],
    [10, 205, 195],
    [35, 180, 255],
    [55, 95, 255],
    [140, 70, 255],
    [232, 50, 220],
    [255, 95, 165],
    [222, 228, 238],
];

/// The shape under a logo's monogram.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LogoShape {
    #[default]
    Circle,
    Ring,
    Hexagon,
    Diamond,
    Triangle,
    Square,
    Chevron,
    /// A crest: universities.
    Shield,
}

impl LogoShape {
    pub const CORP: [LogoShape; 7] = [LogoShape::Circle, LogoShape::Ring, LogoShape::Hexagon, LogoShape::Diamond, LogoShape::Triangle, LogoShape::Square, LogoShape::Chevron];

    pub fn key(self) -> &'static str {
        match self {
            LogoShape::Circle => "circle",
            LogoShape::Ring => "ring",
            LogoShape::Hexagon => "hexagon",
            LogoShape::Diamond => "diamond",
            LogoShape::Triangle => "triangle",
            LogoShape::Square => "square",
            LogoShape::Chevron => "chevron",
            LogoShape::Shield => "shield",
        }
    }

    /// Is the point (`x`, `y` in -1..1, `y` up) on the shape?
    pub fn covers(self, x: f32, y: f32) -> bool {
        let r = (x * x + y * y).sqrt();
        match self {
            LogoShape::Circle => r <= 1.0,
            LogoShape::Ring => (0.58..=1.0).contains(&r),
            LogoShape::Hexagon => (x.abs() * 0.866 + y.abs() * 0.5).max(y.abs() * 0.98) <= 0.95,
            LogoShape::Diamond => x.abs() + y.abs() <= 1.08,
            LogoShape::Triangle => y >= -0.8 && x.abs() <= (1.0 - (y + 0.8) / 1.85) * 1.08,
            LogoShape::Square => x.abs() <= 0.9 && y.abs() <= 0.9,
            LogoShape::Chevron => {
                let d = (y - (x.abs() * 1.0 - 0.45)).abs();
                d <= 0.42 && y <= 0.95 && y >= -0.95
            }
            LogoShape::Shield => x.abs() <= 0.88 && (y >= -0.15 && y <= 0.95 || y < -0.15 && x.abs() <= 0.88 * (1.0 - (-0.15 - y) / 0.85)),
        }
    }
}

/// Side of the logo bitmap, in pixels.
pub const LOGO_N: usize = 11;

/// One pixel of a logo bitmap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogoPixel {
    Empty,
    /// The shape, in the brand colour.
    Shape,
    /// The monogram cut into the shape (dark).
    LetterOnShape,
    /// The monogram where there is no shape (light).
    LetterAlone,
}

/// A 5x7 pixel font for the monogram, rows top to bottom, bit 4 the left
/// column.
fn glyph(c: char) -> [u8; 7] {
    match c.to_ascii_uppercase() {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0F],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x02, 0x12, 0x0C],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x11, 0x19, 0x15, 0x13, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        _ => [0x00, 0x0E, 0x1F, 0x1F, 0x1F, 0x0E, 0x00],
    }
}

/// A brand: a fictional name, a colour, a logo shape and a monogram.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Brand {
    pub name: String,
    /// Index into [`BRAND_COLOURS`].
    pub colour: u8,
    pub shape: LogoShape,
    /// One capital letter.
    pub monogram: char,
}

impl Brand {
    pub fn rgb(&self) -> [u8; 3] {
        BRAND_COLOURS[self.colour as usize % BRAND_COLOURS.len()]
    }

    /// "#e8233f".
    pub fn hex(&self) -> String {
        let [r, g, b] = self.rgb();
        format!("#{r:02x}{g:02x}{b:02x}")
    }

    /// Linear-ish 0..1 colour for meshes.
    pub fn rgb_f32(&self) -> [f32; 3] {
        let c = self.rgb();
        [srgb(c[0]), srgb(c[1]), srgb(c[2])]
    }

    /// The logo as pixels, row 0 at the top: the shape filled, the
    /// monogram over it (cut dark into the shape, light where it is not).
    pub fn logo_bitmap(&self) -> [[LogoPixel; LOGO_N]; LOGO_N] {
        let mut out = [[LogoPixel::Empty; LOGO_N]; LOGO_N];
        let h = (LOGO_N as f32 - 1.0) * 0.5;
        for (row, line) in out.iter_mut().enumerate() {
            for (col, px) in line.iter_mut().enumerate() {
                let x = (col as f32 - h) / h;
                let y = (h - row as f32) / h;
                if self.shape.covers(x, y) {
                    *px = LogoPixel::Shape;
                }
            }
        }
        let g = glyph(self.monogram);
        let (r0, c0) = ((LOGO_N - 7) / 2, (LOGO_N - 5) / 2);
        for (r, bits) in g.iter().enumerate() {
            for c in 0..5 {
                if bits & (0x10 >> c) != 0 {
                    let px = &mut out[r0 + r][c0 + c];
                    *px = if *px == LogoPixel::Shape { LogoPixel::LetterOnShape } else { LogoPixel::LetterAlone };
                }
            }
        }
        out
    }

    /// The bitmap as text rows (`#` shape, `x` letter on shape, `o` letter
    /// alone, `.` empty), for the API and tests.
    pub fn logo_rows(&self) -> Vec<String> {
        self.logo_bitmap()
            .iter()
            .map(|r| {
                r.iter()
                    .map(|p| match p {
                        LogoPixel::Empty => '.',
                        LogoPixel::Shape => '#',
                        LogoPixel::LetterOnShape => 'x',
                        LogoPixel::LetterAlone => 'o',
                    })
                    .collect()
            })
            .collect()
    }

    /// What the renderer needs to draw the mark.
    pub fn mark(&self, role: BrandRole) -> BrandMark {
        BrandMark { colour: self.rgb_f32(), shape: self.shape, monogram: self.monogram, role, colour_index: self.colour }
    }
}

fn srgb(c: u8) -> f32 {
    let x = c as f32 / 255.0;
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

/// A stable 64-bit mix.
pub fn mix(a: u64, b: u64) -> u64 {
    let mut x = a ^ b.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// `0..1` from a hash.
pub fn unit(h: u64) -> f32 {
    (h >> 40) as f32 / (1u64 << 24) as f32
}

const SYL_A: &[&str] = &["Vor", "Kal", "Thes", "Quen", "Oru", "Zan", "Ilm", "Dra", "Syl", "Mor", "Pyr", "Cael", "Brix", "Nym", "Tov", "Jur", "Ferr", "Ost", "Hal", "Xan"];
const SYL_B: &[&str] = &["ion", "ari", "exa", "ora", "ynth", "eth", "ux", "ivo", "arc", "enne", "ova", "ith", "ane", "eon", "ilo", "ark"];
const SECTOR: &[&str] = &["Dynamics", "Holdings", "Systems", "Industries", "Group", "Consolidated", "Works", "Combine", "Syndicate", "Corporation"];

/// A coined corporation name: two syllables nobody trades under, and what
/// it does. Fictional by construction.
pub fn corp_name(h: u64) -> String {
    let a = SYL_A[(mix(h, 1) % SYL_A.len() as u64) as usize];
    let b = SYL_B[(mix(h, 2) % SYL_B.len() as u64) as usize];
    let s = SECTOR[(mix(h, 3) % SECTOR.len() as u64) as usize];
    format!("{a}{b} {s}")
}

/// A brand for a new corp (or campus) from a hash: a colour and shape no
/// other brand in `taken` has, if any is left, and its initial.
pub fn new_brand(h: u64, name: String, shapes: &[LogoShape], taken: &[&Brand]) -> Brand {
    let n = BRAND_COLOURS.len() as u64;
    let start = mix(h, 11) % n;
    let colour = (0..n).map(|k| ((start + k * 5) % n) as u8).find(|c| !taken.iter().any(|b| b.colour == *c)).unwrap_or(start as u8);
    let s0 = mix(h, 12) as usize % shapes.len();
    let shape = (0..shapes.len()).map(|k| shapes[(s0 + k) % shapes.len()]).find(|s| !taken.iter().any(|b| b.shape == *s && b.colour == colour)).unwrap_or(shapes[s0]);
    let monogram = name.chars().find(|c| c.is_ascii_alphabetic()).map(|c| c.to_ascii_uppercase()).unwrap_or('X');
    Brand { name, colour, shape, monogram }
}

/// Where a brand is shown, for the renderer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BrandRole {
    /// The headquarters tower: a lit crown with the logo on every side.
    Headquarters,
    /// Any other corp property: a rooftop sign, a facade banner.
    Holding,
    /// Staff housing: a smaller banner.
    Housing,
    /// The university's main building.
    Campus,
    /// A campus annex (dorm, admin, theatre, sports, frat, plaza).
    Annex(Annex),
}

/// A brand's mark as the renderer draws it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrandMark {
    pub colour: [f32; 3],
    pub shape: LogoShape,
    pub monogram: char,
    pub role: BrandRole,
    pub colour_index: u8,
}

impl BrandMark {
    /// The bitmap of the mark (as [`Brand::logo_bitmap`]).
    pub fn bitmap(&self) -> [[LogoPixel; LOGO_N]; LOGO_N] {
        Brand { name: String::new(), colour: self.colour_index, shape: self.shape, monogram: self.monogram }.logo_bitmap()
    }
}

// ── mega-corps ───────────────────────────────────────────────────────────────

/// A mega-corporation: an owner record (`OwnerKind::MegaCorp`) with a
/// brand, a headquarters and a purse.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MegaCorp {
    pub owner: OwnerId,
    pub brand: Brand,
    /// What the company was before it grew.
    pub origin: BusinessKind,
    pub origin_name: String,
    /// The building it started from: buys are measured from here until the
    /// HQ stands.
    pub flagship: Option<BuildingId>,
    pub hq: Option<BuildingId>,
    /// Cash skimmed from its holdings' profits, for deposits.
    #[serde(default)]
    pub cash: f64,
    pub founded_day: u32,
    #[serde(default)]
    pub bought_total: u32,
    #[serde(default)]
    pub housed_total: u32,
    /// The levy paid, all time.
    #[serde(default)]
    pub levy_total: f64,
    /// Yesterday's gross revenue over its holdings.
    #[serde(default)]
    pub revenue_yesterday: f32,
}

/// The player's levers on the corps (Economy › Corporations).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CorpRules {
    /// Companies may grow into mega-corps at all.
    pub allowed: bool,
    /// Most of the town's ordinary buildings one corp may own.
    pub max_share: f32,
    /// Share of a corp's daily gross revenue the city takes (an antitrust
    /// levy), booked as business tax.
    pub levy: f32,
}

impl Default for CorpRules {
    fn default() -> Self {
        Self { allowed: true, max_share: DEFAULT_MAX_SHARE, levy: 0.0 }
    }
}

// ── universities ─────────────────────────────────────────────────────────────

/// What a campus building beyond the main one is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Annex {
    /// Student housing: a residential block the campus bought or built.
    Dorm,
    /// Administration offices.
    Admin,
    /// A theatre: shows at night.
    Theatre,
    /// A sports hall and field.
    Sports,
    /// A fraternity house.
    Frat,
    /// A park claimed as campus plaza.
    Plaza,
}

impl Annex {
    pub const ALL: [Annex; 6] = [Annex::Dorm, Annex::Admin, Annex::Theatre, Annex::Sports, Annex::Frat, Annex::Plaza];

    pub fn key(self) -> &'static str {
        match self {
            Annex::Dorm => "dorm",
            Annex::Admin => "admin",
            Annex::Theatre => "theatre",
            Annex::Sports => "sports",
            Annex::Frat => "frat",
            Annex::Plaza => "plaza",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Annex::Dorm => "dorm",
            Annex::Admin => "administration",
            Annex::Theatre => "theatre",
            Annex::Sports => "sports centre",
            Annex::Frat => "frat house",
            Annex::Plaza => "campus plaza",
        }
    }

    /// The name of the building, from the university's.
    pub fn name(self, campus: &str, n: usize) -> String {
        let short = campus.split_whitespace().next().unwrap_or(campus);
        match self {
            Annex::Dorm => format!("{short} Hall {}", ["North", "East", "West", "South", "Commons", "Tower", "Court"][n % 7]),
            Annex::Admin => format!("{short} Administration"),
            Annex::Theatre => format!("{short} Playhouse"),
            Annex::Sports => format!("{short} Athletics Centre"),
            Annex::Frat => format!("{} House", ["Alpha Rho", "Kappa Sigma", "Delta Nu", "Theta Pi", "Omega Chi", "Zeta Mu"][n % 6]),
            Annex::Plaza => format!("{short} Quad"),
        }
    }

    /// The campus's own building (offices, theatre, sports centre): never
    /// sold, refitted or redeveloped, staffed like a service.
    pub fn special(self) -> bool {
        matches!(self, Annex::Admin | Annex::Theatre | Annex::Sports)
    }

    /// Land use the annex is built as.
    pub fn land_use(self) -> crate::zoning::LandUse {
        use crate::zoning::LandUse;
        match self {
            Annex::Dorm | Annex::Frat => LandUse::Residential,
            Annex::Admin | Annex::Sports => LandUse::Civic,
            Annex::Theatre => LandUse::Entertainment,
            Annex::Plaza => LandUse::Park,
        }
    }

    /// When it is open and staffed.
    pub fn hours(self) -> crate::market::Hours {
        use crate::market::Hours;
        match self {
            Annex::Admin => Hours::new(8, 18),
            Annex::Theatre => Hours::new(16, 0),
            Annex::Sports => Hours::new(7, 22),
            _ => Hours::ALWAYS,
        }
    }

    /// Jobs it brings.
    pub fn jobs(self) -> u16 {
        match self {
            Annex::Admin => 14,
            Annex::Theatre => 6,
            Annex::Sports => 8,
            _ => 0,
        }
    }
}

/// A university and what it has grown round its campus.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Campus {
    /// The main building (`ServiceKind::University`).
    pub building: BuildingId,
    pub owner: OwnerId,
    pub brand: Brand,
    pub founded_day: u32,
    /// Buildings it owns beyond the main one, and what each is.
    #[serde(default)]
    pub annexes: Vec<(BuildingId, Annex)>,
    /// Plots bought and being cleared for an annex.
    #[serde(default)]
    pub pending: Vec<(LotId, Annex)>,
    /// Enrolled students.
    #[serde(default)]
    pub students: Vec<CitizenId>,
    /// Tuition and gifts, for buying plots.
    #[serde(default)]
    pub funds: f64,
    #[serde(default)]
    pub expansions_total: u32,
}

impl Campus {
    pub fn annex_of(&self, b: BuildingId) -> Option<Annex> {
        self.annexes.iter().find(|(x, _)| *x == b).map(|(_, a)| *a)
    }

    pub fn count(&self, a: Annex) -> usize {
        self.annexes.iter().filter(|(_, x)| *x == a).count() + self.pending.iter().filter(|(_, x)| *x == a).count()
    }
}

/// [megacorp] Running totals.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CorpStats {
    pub promotions_total: u32,
    pub purchases_total: u32,
    pub purchases_today: u32,
    pub levy_today: f64,
    pub levy_total: f64,
    pub housed_today: u32,
    pub students_out_today: u32,
    pub students_to_class_today: u32,
    pub student_trade_today: f32,
    pub campus_expansions_total: u32,
    pub divested_total: u32,
}

/// Everything about corps and campuses (`World::corps`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Corporations {
    pub corps: Vec<MegaCorp>,
    pub campuses: Vec<Campus>,
    pub rules: CorpRules,
    pub stats: CorpStats,
    /// Road distance (metres) from each corp's HQ (or flagship) to every
    /// road node, keyed by the origin building and the road network's
    /// version: rebuilt when either changes.
    #[serde(skip)]
    pub drive_cache: HashMap<BuildingId, (u64, Vec<f32>)>,
}

impl Corporations {
    pub fn corp_of_owner(&self, o: OwnerId) -> Option<&MegaCorp> {
        self.corps.iter().find(|c| c.owner == o)
    }

    pub fn campus_of_owner(&self, o: OwnerId) -> Option<&Campus> {
        self.campuses.iter().find(|c| c.owner == o)
    }

    pub fn campus_of_building(&self, b: BuildingId) -> Option<&Campus> {
        self.campuses.iter().find(|c| c.building == b || c.annexes.iter().any(|(x, _)| *x == b))
    }

    pub fn is_student(&self, c: CitizenId) -> bool {
        self.campuses.iter().any(|k| k.students.contains(&c))
    }

    pub fn brands(&self) -> Vec<&Brand> {
        self.corps.iter().map(|c| &c.brand).chain(self.campuses.iter().map(|c| &c.brand)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_names_are_coined_and_logos_deterministic() {
        let a = corp_name(42);
        assert_eq!(a, corp_name(42));
        assert!(a.split_whitespace().count() == 2);
        let b = new_brand(7, a.clone(), &LogoShape::CORP, &[]);
        let c = new_brand(7, a.clone(), &LogoShape::CORP, &[]);
        assert_eq!(b, c);
        assert_eq!(b.logo_rows(), c.logo_rows());
        // The monogram is drawn.
        assert!(b.logo_rows().iter().any(|r| r.contains('x') || r.contains('o')));
        // A second brand avoids the first one's colour.
        let d = new_brand(7, "Other".into(), &LogoShape::CORP, &[&b]);
        assert_ne!(d.colour, b.colour);
        assert_eq!(d.monogram, 'O');
    }

    #[test]
    fn every_shape_fills_part_of_the_mark() {
        for s in LogoShape::CORP.into_iter().chain([LogoShape::Shield]) {
            let b = Brand { name: "Q".into(), colour: 0, shape: s, monogram: 'Q' };
            let filled = b.logo_bitmap().iter().flatten().filter(|p| **p != LogoPixel::Empty).count();
            assert!(filled > 25 && filled < LOGO_N * LOGO_N, "{s:?}: {filled}");
        }
    }
}
