//! [supertall] The skyscraper league: towers far above anything the
//! ordinary growth builds, only on the densest, dearest downtown land, put
//! up by developers with the money for them, on a site assembled by buying
//! the plots next door. Three classes by height: skyscrapers (150 m up),
//! supertalls (300 m up) and megatalls (600 m up). Developers compete for
//! the city's tallest: a new project may aim over the record when demand and
//! money allow, as long as the city's height limits let it.
//!
//! The data lives here; the `World` side is `world/towers.rs`. Everything is
//! saved with serde defaults: an older save loads with no towers, no limit
//! and an empty record book.

use crate::buildings::BuildingId;
use crate::estate::OwnerId;
use glam::Vec2;
use serde::{Deserialize, Serialize};

// ── tuning ───────────────────────────────────────────────────────────────────

/// Height (m) from which a tower is in the league: a skyscraper.
pub const SKYSCRAPER_M: f32 = 150.0;
/// ... a supertall.
pub const SUPERTALL_M: f32 = 300.0;
/// ... a megatall.
pub const MEGATALL_M: f32 = 600.0;
/// The tallest the sim will plan (`Building::floors` is a `u8`).
pub const TOWER_MAX_M: f32 = 1000.0;
/// Floor to floor (m) of a league tower: its sim floors are its height over
/// this.
pub const TOWER_FLOOR_M: f32 = 4.0;
/// Most plots one tower stands on (the primary included).
pub const TOWER_MAX_LOTS: usize = 8;
/// A tower site may be a little less compact than a firm's merge: the
/// podium fills the corners.
pub const TOWER_HULL_SLACK: f32 = 1.18;
/// People a city needs before anyone builds in the league.
pub const TOWER_MIN_POP: usize = 1200;
/// Land value a tower site needs at least.
pub const TOWER_MIN_VALUE: f32 = 0.6;
/// Floors the pressure on the site must justify (once pressure caps
/// heights at all).
pub const TOWER_MIN_PRESSURE_FLOORS: u8 = 9;
/// Floors of other buildings within `TOWER_DENSITY_RADIUS`: downtown, not a
/// field.
pub const TOWER_MIN_NEIGHBOUR_FLOORS: u32 = 50;
pub const TOWER_DENSITY_RADIUS: f32 = 160.0;
/// Days between two projects starting.
pub const TOWER_START_GAP_DAYS: u32 = 3;
/// Chance a day a developer starts a project when everything allows.
pub const TOWER_DAILY_CHANCE: f64 = 0.4;
/// Demand (for offices or homes) a project needs; a record attempt needs
/// the stronger one.
pub const TOWER_MIN_DEMAND: f32 = 0.05;
pub const RECORD_MIN_DEMAND: f32 = 0.2;
/// A record attempt beats the standing record by this much, at least and at
/// most (hashed between).
pub const RECORD_STEP: (f32, f32) = (1.06, 1.24);
/// Cost of the structure: height squared times this (dollars). A 150 m
/// tower costs $20M, 300 m $81M, 600 m $324M, 828 m $617M.
pub const COST_PER_M2: f64 = 900.0;
/// What one person in town brings the tower developers a day (investors
/// follow a growing city), before demand.
pub const CAPITAL_PER_PERSON: f64 = 900.0;
/// Around a player's vertiport nothing may rise over this (m): the
/// approach stays clear.
pub const VERTIPORT_CLEARANCE_M: f32 = 350.0;
pub const VERTIPORT_CLEARANCE_CAP_M: f32 = 120.0;
/// Floor heights the sim assumes for ordinary buildings, by land use
/// index (matching the renderer's rules), for heights and rankings.
pub const FLOOR_M: [f32; 6] = [3.1, 3.5, 5.0, 3.6, 4.2, 3.0];

/// Fictional tower developers: first and second words.
const DEV_A: &[&str] = &["Kestrel", "Solenne", "Vireo", "Corvane", "Halden", "Ostrava", "Marlowe", "Tamsin", "Lisle", "Brightwater", "Calloway", "Ardent"];
const DEV_B: &[&str] = &["Group", "Holdings", "Partners", "Vertical", "Capital", "Development", "Realty", "Land Co."];
/// Tower names.
const NAME_A: &[&str] = &["Meridian", "Aurora", "Helix", "Obsidian", "Lumen", "Vesper", "Cobalt", "Zenith", "Halcyon", "Sable", "Orchid", "Prism", "Solstice", "Nimbus", "Spectra", "Ion", "Argent", "Corona"];
const NAME_B: &[&str] = &["Spire", "Tower", "Needle", "Pinnacle", "Crown", "One", "Point", "Summit", "Sky Tower", "Centre"];

// ── classes and designs ──────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum League {
    /// 150 to 300 m: the Empire State class.
    Skyscraper,
    /// 300 to 600 m.
    Supertall,
    /// 600 m and up: the Burj class.
    Megatall,
}

impl League {
    pub fn for_height(h: f32) -> Option<League> {
        if h >= MEGATALL_M {
            Some(League::Megatall)
        } else if h >= SUPERTALL_M {
            Some(League::Supertall)
        } else if h >= SKYSCRAPER_M {
            Some(League::Skyscraper)
        } else {
            None
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            League::Skyscraper => "skyscraper",
            League::Supertall => "supertall",
            League::Megatall => "megatall",
        }
    }

    /// Site a tower of this class needs: plots (the primary included) and
    /// area (m²), both at least.
    pub fn min_lots(self) -> usize {
        match self {
            League::Skyscraper => 1,
            League::Supertall => 2,
            League::Megatall => 3,
        }
    }

    pub fn min_area(self) -> f32 {
        match self {
            League::Skyscraper => 700.0,
            League::Supertall => 1500.0,
            League::Megatall => 2600.0,
        }
    }
}

/// The tower's form, drawn by `nd_procgen::buildings::supertall`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TowerDesign {
    /// Stepped Art Deco setbacks and a spire (the Empire State type).
    DecoSpire,
    /// Three wings round a core stepping back to a needle (the Burj type).
    Bundled,
    /// Floor plates turning as they rise.
    Twist,
    /// A slim tapering shaft with a long lit needle.
    Needle,
}

impl TowerDesign {
    pub const ALL: [TowerDesign; 4] = [TowerDesign::DecoSpire, TowerDesign::Bundled, TowerDesign::Twist, TowerDesign::Needle];

    pub fn label(self) -> &'static str {
        match self {
            TowerDesign::DecoSpire => "Art Deco spire",
            TowerDesign::Bundled => "bundled tubes",
            TowerDesign::Twist => "twisting tower",
            TowerDesign::Needle => "needle tower",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            TowerDesign::DecoSpire => "deco_spire",
            TowerDesign::Bundled => "bundled",
            TowerDesign::Twist => "twist",
            TowerDesign::Needle => "needle",
        }
    }

    pub fn parse(s: &str) -> Option<TowerDesign> {
        let s = s.trim().to_ascii_lowercase();
        TowerDesign::ALL.into_iter().find(|d| d.key() == s || d.label() == s)
    }

    /// The design for a tower of `height` from its seed: the Deco spire
    /// suits the shorter class, the bundled tubes the tallest.
    pub fn for_seed(seed: u64, height: f32) -> TowerDesign {
        let w: [f32; 4] = match League::for_height(height) {
            Some(League::Megatall) => [0.5, 3.0, 1.2, 1.3],
            Some(League::Supertall) => [1.0, 1.6, 1.5, 1.4],
            _ => [2.4, 0.6, 1.2, 1.0],
        };
        let r = unit(seed ^ 0xDE51_6A) * w.iter().sum::<f32>();
        let mut acc = 0.0;
        for (i, x) in w.iter().enumerate() {
            acc += x;
            if r < acc {
                return TowerDesign::ALL[i];
            }
        }
        TowerDesign::Needle
    }
}

/// A league tower: on a building's `plot.tower`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tower {
    /// To the top of the spire (m).
    pub height: f32,
    pub design: TowerDesign,
    /// Seed of the form (stays when the building's own seed changes on a
    /// refit, so the tower keeps its shape).
    pub seed: u64,
    #[serde(default)]
    pub started_day: u32,
    #[serde(default)]
    pub topped_out_day: Option<u32>,
    /// Planned to beat the city's record when it broke ground.
    #[serde(default)]
    pub record_bid: bool,
}

impl Tower {
    pub fn league(&self) -> League {
        League::for_height(self.height).unwrap_or(League::Skyscraper)
    }

    /// Floors the sim counts (homes, jobs, power, build time).
    pub fn floors(&self) -> u8 {
        floors_for(self.height)
    }
}

pub fn floors_for(height: f32) -> u8 {
    (height.clamp(0.0, TOWER_MAX_M) / TOWER_FLOOR_M * 0.88).round().clamp(1.0, 250.0) as u8
}

/// Floors that count for homes and jobs: all of the first forty, half of
/// the next eighty, a quarter above (sky lobbies, plant floors, the thin
/// top), so one megatall does not swamp a city's balance.
pub fn effective_floors(floors: u8) -> f32 {
    let f = floors as f32;
    f.min(40.0) + 0.5 * (f.min(120.0) - 40.0).max(0.0) + 0.25 * (f - 120.0).max(0.0)
}

/// Build time (days) of a tower of `height`: a month for a skyscraper,
/// two for the tallest.
pub fn build_days(height: f32) -> f32 {
    (12.0 + height / 15.0).clamp(12.0, 80.0)
}

/// What the structure of a tower of `height` costs (dollars).
pub fn structure_cost(height: f32) -> f64 {
    (height as f64).powi(2) * COST_PER_M2
}

/// The height a fortune buys (m).
pub fn affordable_height(money: f64) -> f32 {
    (money.max(0.0) / COST_PER_M2).sqrt() as f32
}

/// The tallest a town of `pop` people can carry (m): 170 m at 1,200
/// people, 410 m at 6,000, 610 m at 10,000, 860 m at 15,000.
pub fn height_for_population(pop: usize) -> f32 {
    (110.0 + 0.05 * pop as f32).min(TOWER_MAX_M)
}

pub fn unit(h: u64) -> f32 {
    let x = h.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ (h >> 29);
    let x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    ((x >> 40) as f32) / ((1u64 << 24) as f32)
}

pub fn developer_name(h: u64) -> String {
    format!("{} {}", DEV_A[(h % DEV_A.len() as u64) as usize], DEV_B[((h >> 17) % DEV_B.len() as u64) as usize])
}

pub fn tower_name(h: u64) -> String {
    format!("{} {}", NAME_A[(h % NAME_A.len() as u64) as usize], NAME_B[((h >> 13) % NAME_B.len() as u64) as usize])
}

/// "1st", "2nd", "3rd", "11th".
pub fn ordinal(n: usize) -> String {
    let s = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{s}")
}

// ── the record book ──────────────────────────────────────────────────────────

/// A line of skyline news (a groundbreaking, a topping out, a record).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TowerNews {
    pub seq: u32,
    pub tick: u64,
    pub headline: String,
    pub pos: Vec2,
    #[serde(default)]
    pub building: Option<BuildingId>,
}

/// A record set: the city's tallest at the time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RecordEntry {
    pub day: u32,
    pub name: String,
    pub developer: String,
    pub height: f32,
    #[serde(default)]
    pub building: Option<BuildingId>,
}

/// A tower developer and its war chest. Its owner record (`estate`) is
/// made with its first project and dropped by the estate's pruning while
/// it owns nothing; the name and the money stay here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TowerDeveloper {
    pub name: String,
    #[serde(default)]
    pub owner: Option<OwnerId>,
    /// Dollars to build with.
    pub capital: f64,
    /// How hard investors back it, `0.7..1.3`.
    pub pull: f32,
}

/// The league's state on the `World` (`World::skyline`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Skyline {
    /// The city's height limit (m); `None` lets them rise as high as the
    /// market will. Districts can set their own (`District::height_limit`).
    pub limit: Option<f32>,
    /// Keep the approach round the player's vertiports clear
    /// (`VERTIPORT_CLEARANCE_M`, `VERTIPORT_CLEARANCE_CAP_M`).
    pub vertiport_clearance: bool,
    /// Ignore the population gate (tests and the dev command).
    pub always: bool,
    pub developers: Vec<TowerDeveloper>,
    pub news: Vec<TowerNews>,
    pub news_seq: u32,
    pub records: Vec<RecordEntry>,
    pub projects_total: u32,
    pub topped_total: u32,
    pub plots_bought_total: u32,
    pub last_start_day: Option<u32>,
}

impl Default for Skyline {
    fn default() -> Self {
        Self { limit: None, vertiport_clearance: true, always: false, developers: Vec::new(), news: Vec::new(), news_seq: 0, records: Vec::new(), projects_total: 0, topped_total: 0, plots_bought_total: 0, last_start_day: None }
    }
}

impl Skyline {
    pub fn push_news(&mut self, tick: u64, headline: String, pos: Vec2, building: Option<BuildingId>) -> u32 {
        self.news_seq += 1;
        let seq = self.news_seq;
        self.news.push(TowerNews { seq, tick, headline, pos, building });
        if self.news.len() > 40 {
            self.news.remove(0);
        }
        seq
    }

    pub fn developer(&self, owner: OwnerId) -> Option<&TowerDeveloper> {
        self.developers.iter().find(|d| d.owner == Some(owner))
    }

    /// The record as last set (m), 0 before the first.
    pub fn record(&self) -> f32 {
        self.records.last().map_or(0.0, |r| r.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classes_by_height() {
        assert_eq!(League::for_height(120.0), None);
        assert_eq!(League::for_height(150.0), Some(League::Skyscraper));
        assert_eq!(League::for_height(381.0), Some(League::Supertall));
        assert_eq!(League::for_height(828.0), Some(League::Megatall));
        assert!(League::Megatall.min_area() > League::Supertall.min_area());
    }

    #[test]
    fn floors_and_capacity_scale_with_diminishing_returns() {
        assert!(floors_for(150.0) >= 30 && floors_for(150.0) <= 40);
        assert!(floors_for(828.0) > 150 && floors_for(828.0) <= 250);
        assert_eq!(floors_for(5000.0), floors_for(TOWER_MAX_M));
        let (a, b) = (effective_floors(40), effective_floors(200));
        assert_eq!(a, 40.0);
        assert!(b < 120.0 && b > 80.0, "{b}");
    }

    #[test]
    fn names_and_ordinals() {
        assert_eq!(ordinal(1), "1st");
        assert_eq!(ordinal(2), "2nd");
        assert_eq!(ordinal(3), "3rd");
        assert_eq!(ordinal(11), "11th");
        assert_eq!(ordinal(22), "22nd");
        assert_ne!(tower_name(1), "");
        assert_eq!(developer_name(7), developer_name(7));
        for s in [0u64, 1, 99, 12345] {
            let u = unit(s);
            assert!((0.0..1.0).contains(&u));
        }
    }
}
