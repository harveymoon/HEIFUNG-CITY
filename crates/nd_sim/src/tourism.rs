//! [tourism] Visitors from outside: drawn by what the city has to see, they
//! come in by road, rail, ferry or air, sleep in hotels, spend in shops,
//! venues and on the city's tourist tax, and go home after a few days.
//!
//! Tourists are not citizens. They have no home, job, household, pantry,
//! rent, mood or mind, and nothing that reads citizens (demand, classes,
//! crime, the report's people section) should count them, so they are kept
//! apart in [`Tourism`] with a small state machine of their own. They walk
//! the pedestrian network like everyone else (and their footsteps count as
//! footfall for the shops they pass), and are drawn with the citizens'
//! figure generator with a backpack and camera so they read as visitors.
//!
//! Pure data and the attractiveness rules here; the world side (arrivals,
//! days out, spending, hotels opening) is in `world/tourism.rs`. Tourism
//! draws on its own random stream (`rng_for` by tick), so turning it on
//! does not change what anything else in the city rolls.

use crate::buildings::{BuildingId, Tier};
use glam::Vec2;
use serde::{Deserialize, Serialize};
use slotmap::{new_key_type, SlotMap};

new_key_type! {
    pub struct TouristId;
}

/// Beds per floor in a hotel, by tier: capsule pods, rooms, suites.
pub fn beds_per_floor(tier: Tier) -> u16 {
    match tier {
        Tier::Low => 12,
        Tier::Mid => 8,
        Tier::High => 6,
    }
}

/// What a night costs, by hotel tier. Set so a hotel about two thirds full
/// covers its wages (a capsule floor of three staff, a suite floor of four
/// well-paid ones).
pub fn night_price(tier: Tier) -> f32 {
    match tier {
        Tier::Low => 18.0,
        Tier::Mid => 45.0,
        Tier::High => 110.0,
    }
}

/// The city's tax on a hotel night, as a share of its price.
pub const TOURIST_TAX: f32 = 0.12;
/// Tourists who would come per day for each point of attractiveness.
pub const ARRIVALS_PER_POINT: f32 = 0.35;
/// At most this many tourists in town at once (a bound on the work).
pub const MAX_TOURISTS: usize = 1500;
/// [ferries] Minutes a visitor sits aboard a ferry that never comes in
/// (the terminal shut) before stepping ashore at the terminal anyway.
pub const ABOARD_GIVE_UP: u64 = 300;
/// Days a visitor stays: 1 + up to this many more.
pub const MAX_EXTRA_NIGHTS: u32 = 3;
/// Beyond this a leg is taken by taxi rather than walked.
pub const WALK_MAX_M: f32 = 1100.0;
/// Hotel demand (smoothed tourists turned away per day) above which a new
/// commercial building may open as a hotel.
pub const HOTEL_DEMAND_TO_BUILD: f32 = 4.0;

/// How a visitor came, and so how they leave.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Arrival {
    /// The highway or a road off the map: set down at the hotel door.
    Road,
    /// A train to a station.
    Rail,
    /// A ferry to a terminal.
    Ferry,
    /// A drone taxi to a vertiport.
    Air,
}

impl Arrival {
    pub const ALL: [Arrival; 4] = [Arrival::Road, Arrival::Rail, Arrival::Ferry, Arrival::Air];

    pub fn label(self) -> &'static str {
        match self {
            Arrival::Road => "road",
            Arrival::Rail => "rail",
            Arrival::Ferry => "ferry",
            Arrival::Air => "air",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// What draws visitors.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AttractionKind {
    Monument,
    Stadium,
    Park,
    Nightlife,
    /// A historic, neon strip, night market or tourist quarter district.
    District,
    /// The waterfront: built lots along the sea, a lake or a river.
    Coast,
}

impl AttractionKind {
    pub const ALL: [AttractionKind; 6] = [AttractionKind::Monument, AttractionKind::Stadium, AttractionKind::Park, AttractionKind::Nightlife, AttractionKind::District, AttractionKind::Coast];

    pub fn label(self) -> &'static str {
        match self {
            AttractionKind::Monument => "monument",
            AttractionKind::Stadium => "stadium",
            AttractionKind::Park => "park",
            AttractionKind::Nightlife => "nightlife",
            AttractionKind::District => "landmark district",
            AttractionKind::Coast => "waterfront",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    /// Open to sightseers at this hour: nightlife after dark, everything
    /// else by day, stadiums from noon to late.
    pub fn open_at(self, hour: u32) -> bool {
        match self {
            AttractionKind::Nightlife => hour >= 18 || hour < 2,
            AttractionKind::Stadium => (12..23).contains(&hour),
            AttractionKind::District => (9..24).contains(&hour),
            _ => (8..21).contains(&hour),
        }
    }
}

/// One thing worth seeing, with its pull.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attraction {
    pub kind: AttractionKind,
    pub name: String,
    pub pos: Vec2,
    /// The building to stand outside (or go into), if any.
    pub building: Option<BuildingId>,
    pub score: f32,
}

/// Where a tourist is going on foot (or by taxi).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Goal {
    /// Check in (or back to bed).
    Hotel,
    /// Stand and look at an attraction of this kind (the place is the
    /// end of the leg, so a refreshed attraction list does not matter).
    Sight(AttractionKind),
    /// Into a shop or venue to spend.
    Shop(BuildingId),
    /// Out of town by the way they came.
    Depart,
    /// [ferries] A commuter off the boat, to the shift at this business.
    Work(BuildingId),
}

/// What a tourist is doing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum TouristState {
    /// In the hotel: asleep, resting, or between outings.
    InHotel,
    /// On foot along `path` to `goal`.
    Walking { goal: Goal, path: Vec<Vec2>, step: usize },
    /// In a taxi, out of sight, to `goal` at `to`, there at `until`.
    Riding { goal: Goal, to: Vec2, until: u64 },
    /// Taking photos at a sight until `until`.
    Sightseeing { kind: AttractionKind, until: u64 },
    /// Inside a shop or venue until `until`.
    Inside { building: BuildingId, until: u64 },
    /// [ferries] On a ferry in from a town off the map, out of sight, to
    /// step off at `terminal` when the boat comes in.
    Aboard { terminal: BuildingId, since: u64 },
}

/// A visitor.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tourist {
    pub seed: u64,
    pub age: u8,
    /// Backpacker, ordinary visitor, or well off: their budget and the
    /// hotels they look for.
    pub class: Tier,
    pub pos: Vec2,
    pub prev_pos: Vec2,
    pub heading: f32,
    pub prev_heading: f32,
    pub hotel: BuildingId,
    pub via: Arrival,
    /// Where they arrived (a station, terminal or pad), to leave from.
    pub port: Option<BuildingId>,
    pub arrived_tick: u64,
    /// The day (`SimClock::day`) they go home.
    pub leave_day: u64,
    pub money: f32,
    pub spent: f32,
    pub state: TouristState,
    /// Next tick a decision is due while in the hotel or out.
    pub next_tick: u64,
    /// Sights seen, for the inspector.
    pub sights: u16,
    /// [ferries] Not a visitor but a commuter from over the water, here for
    /// a shift at this business (`hotel` is the same building: no bed, no
    /// night's bill); back to the boat when the shift is over.
    #[serde(default)]
    pub commuter: Option<BuildingId>,
}

impl Tourist {
    /// Out on the street where the renderer should draw them.
    pub fn is_visible(&self) -> bool {
        matches!(self.state, TouristState::Walking { .. } | TouristState::Sightseeing { .. })
    }

    pub fn activity_label(&self) -> &'static str {
        match &self.state {
            TouristState::InHotel => "at the hotel",
            TouristState::Walking { goal: Goal::Hotel, .. } => "walking to the hotel",
            TouristState::Walking { goal: Goal::Sight(_), .. } => "walking to a sight",
            TouristState::Walking { goal: Goal::Shop(_), .. } => "walking to a shop",
            TouristState::Walking { goal: Goal::Depart, .. } => "leaving town",
            TouristState::Walking { goal: Goal::Work(_), .. } => "walking to work",
            TouristState::Riding { .. } => "in a taxi",
            TouristState::Sightseeing { .. } => "sightseeing",
            TouristState::Inside { .. } if self.commuter.is_some() => "at work",
            TouristState::Inside { .. } => "shopping",
            TouristState::Aboard { .. } => "on the ferry in",
        }
    }

    /// Daily spending money by class.
    pub fn daily_budget(class: Tier) -> f32 {
        match class {
            Tier::Low => 55.0,
            Tier::Mid => 140.0,
            Tier::High => 380.0,
        }
    }
}

/// Today's and running tourism figures.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TourismStats {
    pub arrivals_today: u32,
    pub arrivals_yesterday: u32,
    /// Arrivals today by `Arrival` index.
    pub arrivals_by_mode_today: [u32; 4],
    pub departures_today: u32,
    pub turned_away_today: u32,
    pub turned_away_yesterday: u32,
    pub nights_today: u32,
    pub nights_yesterday: u32,
    /// What visitors paid to hotels, shops and venues today.
    pub spend_hotels_today: f32,
    pub spend_shops_today: f32,
    pub spend_venues_today: f32,
    pub spend_yesterday: f32,
    /// The city's tourist tax today and yesterday (also on the ledger).
    pub tax_today: f32,
    pub tax_yesterday: f32,
    pub sights_today: u32,
    pub arrivals_total: u64,
    pub spend_total: f64,
}

impl TourismStats {
    pub fn spend_today(&self) -> f32 {
        self.spend_hotels_today + self.spend_shops_today + self.spend_venues_today
    }

    /// Close the day.
    pub fn roll(&mut self) {
        self.arrivals_yesterday = self.arrivals_today;
        self.turned_away_yesterday = self.turned_away_today;
        self.nights_yesterday = self.nights_today;
        self.spend_yesterday = self.spend_today();
        self.tax_yesterday = self.tax_today;
        self.arrivals_today = 0;
        self.arrivals_by_mode_today = [0; 4];
        self.departures_today = 0;
        self.turned_away_today = 0;
        self.nights_today = 0;
        self.spend_hotels_today = 0.0;
        self.spend_shops_today = 0.0;
        self.spend_venues_today = 0.0;
        self.tax_today = 0.0;
        self.sights_today = 0;
    }
}

/// Everything about visitors.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Tourism {
    pub tourists: SlotMap<TouristId, Tourist>,
    /// What there is to see, refreshed every two hours.
    pub attractions: Vec<Attraction>,
    /// Sum of the attractions' pull: the city's attractiveness.
    pub attractiveness: f32,
    /// Ways in that work now, by `Arrival` index (refreshed with the
    /// attractions).
    pub ways_in: [bool; 4],
    /// Tourists who would come per day at this attractiveness.
    pub demand_per_day: f32,
    /// Smoothed visitors turned away for want of a bed per day: opens hotels.
    pub hotel_demand: f32,
    /// Fractional arrivals carried to the next hour, so a slow trickle
    /// still arrives.
    pub carry: f32,
    pub stats: TourismStats,
}

impl Tourism {
    /// Tourists in town now (in a hotel, out or on their way); commuters
    /// off the ferry are not visitors.
    pub fn count(&self) -> usize {
        self.tourists.values().filter(|t| t.commuter.is_none()).count()
    }

    /// [ferries] Commuters from over the water in town now.
    pub fn commuters(&self) -> usize {
        self.tourists.values().filter(|t| t.commuter.is_some()).count()
    }

    pub fn visible(&self) -> impl Iterator<Item = (TouristId, &Tourist)> {
        self.tourists.iter().filter(|(_, t)| t.is_visible())
    }

    /// Pull of attractions by kind.
    pub fn score_by_kind(&self) -> [f32; 6] {
        let mut out = [0.0; 6];
        for a in &self.attractions {
            out[a.kind.index()] += a.score;
        }
        out
    }
}

/// Pull of a monument or stadium by tier.
pub fn landmark_score(stadium: bool, tier: Tier) -> f32 {
    let base = if stadium { 18.0 } else { 14.0 };
    base * match tier {
        Tier::Low => 0.6,
        Tier::Mid => 1.0,
        Tier::High => 1.9,
    }
}

/// Pull of a park lot by tier and area: dear land gets lakes, fountains
/// and ferris wheels worth a visit; scrub lots are not.
pub fn park_score(tier: Tier, area_m2: f32) -> f32 {
    let per = match tier {
        Tier::Low => 0.0,
        Tier::Mid => 0.5,
        Tier::High => 1.2,
    };
    (per * (0.5 + area_m2 / 3000.0)).min(5.0)
}

/// Pull of an open entertainment venue by tier.
pub fn nightlife_score(tier: Tier) -> f32 {
    match tier {
        Tier::Low => 0.4,
        Tier::Mid => 0.9,
        Tier::High => 2.0,
    }
}

/// Pull of a landmark district with `buildings` standing in it.
pub fn district_score(buildings: usize, historic: bool) -> f32 {
    let per = if historic { 0.45 } else { 0.3 };
    (2.0 + per * buildings as f32).min(if historic { 30.0 } else { 22.0 })
}

/// Pull of the waterfront from built lots along the water.
pub fn coast_score(waterfront_lots: usize) -> f32 {
    (waterfront_lots as f32 * 0.25).min(16.0)
}

/// Tourists per day the city would draw: attractiveness times the share
/// of the world that can reach it (a road in matters most; rail, ferries
/// and air add their own visitors).
pub fn demand_per_day(attractiveness: f32, ways_in: [bool; 4]) -> f32 {
    let reach = [1.0, 0.45, 0.3, 0.15];
    let r: f32 = ways_in.iter().zip(reach).filter(|(w, _)| **w).map(|(_, r)| r).sum();
    // The first sights matter most; a city of wonders is not ten times as
    // visited as a city with one.
    let a = attractiveness.max(0.0);
    let pull = if a <= 60.0 { a } else { 60.0 + (a - 60.0).sqrt() * 6.0 };
    pull * ARRIVALS_PER_POINT * r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demand_needs_a_way_in_and_grows_with_sights() {
        assert_eq!(demand_per_day(50.0, [false; 4]), 0.0);
        let road = demand_per_day(50.0, [true, false, false, false]);
        let all = demand_per_day(50.0, [true; 4]);
        assert!(road > 10.0 && all > road);
        // Diminishing: twice the sights is less than twice the visitors past 60.
        let a = demand_per_day(100.0, [true, false, false, false]);
        let b = demand_per_day(200.0, [true, false, false, false]);
        assert!(b > a && b < 2.0 * a);
    }

    #[test]
    fn scores_rank_sensibly() {
        assert!(landmark_score(false, Tier::High) > landmark_score(false, Tier::Low));
        assert!(park_score(Tier::High, 4000.0) > park_score(Tier::Mid, 4000.0));
        assert_eq!(park_score(Tier::Low, 9000.0), 0.0);
        assert!(district_score(40, true) > district_score(40, false));
        assert!(coast_score(1000) <= 16.0);
    }

    #[test]
    fn stats_roll_over() {
        let mut s = TourismStats { arrivals_today: 5, spend_shops_today: 20.0, tax_today: 3.0, ..Default::default() };
        s.roll();
        assert_eq!((s.arrivals_yesterday, s.arrivals_today), (5, 0));
        assert_eq!(s.spend_yesterday, 20.0);
        assert_eq!(s.tax_yesterday, 3.0);
    }
}
