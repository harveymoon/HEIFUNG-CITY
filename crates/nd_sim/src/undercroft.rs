//! [r5-layered] Life under the raised roads: the ground under a deck is a
//! place, not a void.
//!
//! Every raised way the player drew stands on columns (`crate::pillars`).
//! Between two columns, under the deck, is a *bay*: a strip of sheltered
//! ground the width of the deck with the deck's soffit for a roof. A bay is
//! found wherever that ground is free (no road, path or rail runs under it,
//! no lot or building stands on it, no water) and the deck is high enough
//! to stand under (`MIN_CLEARANCE`).
//!
//! What a bay becomes follows its neighbourhood (`UnderUse`): a market of
//! stalls under tarps and bulbs where people live and work and the land is
//! worth something, hawker stalls with plastic stools where workers eat, a
//! row of lock-up workshops (scooter and car repair) by the industry and on
//! cheap land with a street to drive in from, a court for the young people
//! of the blocks around, and where people have nowhere to live, a camp
//! (`homeless`: they shelter under the raised ways, tents, tarps and a fire
//! in a barrel). A bay nobody has taken is bare: gravel, puddles, drums.
//!
//! The trades are run by stallholders, households from the blocks around
//! (those with nobody in work first): one of each stands at the stall in
//! its hours, the takings go to them (stock bought from out of town takes
//! its share). People out for a stroll stop at an open bay near them: they
//! eat at the hawkers (a meal), shop at the market, get a bike or car seen
//! to at the workshops, or shoot hoops; the bay counts its visits and
//! takings, by day.
//!
//! The bays are derived from the columns and the roads (found again when
//! they change); what each has become and its tallies are saved.
//! Everything random is a hash of the bay and the day, so the same city
//! makes the same choices.

use crate::citizens::{CitizenId, HouseholdId};
use std::collections::BTreeMap;
use glam::Vec2;
use serde::{Deserialize, Serialize};

/// Headroom (ground to the deck's soffit) a bay needs: a stall, a lock-up
/// or a tent fits under it.
pub const MIN_CLEARANCE: f32 = 3.0;
/// What a deck takes off its level for its slab (as `pillars::DECK_DEPTH`).
pub const SOFFIT_DROP: f32 = crate::pillars::DECK_DEPTH;
/// A bay keeps this far from the columns at its ends.
pub const COLUMN_CLEAR: f32 = 2.2;
/// The shortest bay worth having, and the longest (a longer span between
/// columns is two bays).
pub const MIN_BAY: f32 = 7.0;
pub const MAX_BAY: f32 = 22.0;
/// Bays no closer than this, centre to centre (two decks side by side, or
/// a deck crossing another, share their ground once).
pub const BAY_GAP: f32 = 6.0;
/// Most bays a city keeps track of.
pub const MAX_BAYS: usize = 600;
/// A day's chance that a bare bay with a use that suits it takes it.
pub const TAKE_CHANCE: f32 = 0.3;
/// A trade whose stalls took nobody for this many days in a row packs up.
pub const IDLE_DAYS: u8 = 4;
/// Stallholder households for a market or hawkers, at most.
pub const MAX_HOLDERS: usize = 3;
/// How far stallholders, custom and the neighbourhood are looked for.
pub const REACH: f32 = 380.0;
/// A stroller stops at an open bay this near (straight line).
pub const STROLL_REACH: f32 = 420.0;
/// Chance a stroll with an open bay in reach goes there instead.
pub const STROLL_CHANCE: f32 = 0.45;
/// Share of the takings that buys the stock (paid out of town).
pub const STOCK_SHARE: f32 = 0.4;

/// What the ground under a deck is used for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum UnderUse {
    /// Nothing yet: gravel, puddles and whatever was dumped there.
    #[default]
    Bare,
    /// Stalls under tarps: produce, clothes, phones, bric-a-brac.
    Market,
    /// Food stalls: woks, steam, plastic stools.
    Hawkers,
    /// Lock-ups: scooter and car repair, a welder.
    Workshop,
    /// A ball court painted on the concrete, a hoop each end.
    Court,
    /// A camp: people with nowhere to live shelter here.
    Shelter,
}

impl UnderUse {
    pub const ALL: [UnderUse; 6] = [UnderUse::Bare, UnderUse::Market, UnderUse::Hawkers, UnderUse::Workshop, UnderUse::Court, UnderUse::Shelter];

    pub fn label(self) -> &'static str {
        match self {
            UnderUse::Bare => "bare ground",
            UnderUse::Market => "market",
            UnderUse::Hawkers => "hawker stalls",
            UnderUse::Workshop => "workshops",
            UnderUse::Court => "ball court",
            UnderUse::Shelter => "camp",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            UnderUse::Bare => "bare",
            UnderUse::Market => "market",
            UnderUse::Hawkers => "hawkers",
            UnderUse::Workshop => "workshop",
            UnderUse::Court => "court",
            UnderUse::Shelter => "camp",
        }
    }

    /// Run by stallholders, open for custom in its hours.
    pub fn is_trade(self) -> bool {
        matches!(self, UnderUse::Market | UnderUse::Hawkers | UnderUse::Workshop)
    }

    /// People come to it (a trade, or the court).
    pub fn draws_visitors(self) -> bool {
        self.is_trade() || self == UnderUse::Court
    }

    /// Opening hours `[open, close)`, the hour of the day. The hawkers run
    /// from the lunch rush to late, the market into the evening (a night
    /// market under the lights), the workshops in the day.
    pub fn hours(self) -> (u8, u8) {
        match self {
            UnderUse::Market => (10, 23),
            UnderUse::Hawkers => (11, 24),
            UnderUse::Workshop => (8, 19),
            UnderUse::Court => (7, 23),
            UnderUse::Bare | UnderUse::Shelter => (0, 0),
        }
    }

    pub fn open_at(self, hour: u8) -> bool {
        let (a, b) = self.hours();
        hour >= a && hour < b
    }

    /// What a visit costs (none at the court), before the visitor's means.
    pub fn spend(self) -> f32 {
        match self {
            UnderUse::Market => 11.0,
            UnderUse::Hawkers => 7.0,
            UnderUse::Workshop => 24.0,
            _ => 0.0,
        }
    }

    /// Minutes a visitor stays.
    pub fn stay(self) -> u64 {
        match self {
            UnderUse::Market => 35,
            UnderUse::Hawkers => 40,
            UnderUse::Workshop => 45,
            UnderUse::Court => 60,
            _ => 20,
        }
    }

    /// Most people at (or on the way to) a bay of `units` at once.
    pub fn capacity(self, units: u8) -> usize {
        match self {
            UnderUse::Court => 10,
            UnderUse::Workshop => units as usize * 2,
            _ => units as usize * 4,
        }
    }

    /// Stalls (or lock-ups, or hoops) drawn for a bay `len` long.
    pub fn units(self, len: f32) -> u8 {
        match self {
            UnderUse::Market => (len / 3.2).floor().clamp(2.0, 7.0) as u8,
            UnderUse::Hawkers => (len / 4.5).floor().clamp(1.0, 4.0) as u8,
            UnderUse::Workshop => (len / 4.2).floor().clamp(1.0, 5.0) as u8,
            UnderUse::Court => 2,
            _ => 0,
        }
    }
}

/// The city's bays and who is at them (`World::undercroft`).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Undercroft {
    pub bays: Vec<Bay>,
    /// Who is on the way to (or at) which bay, by its id.
    pub trips: BTreeMap<CitizenId, u64>,
    /// The day each visitor last stopped at a bay: once a day at most.
    pub last_visit: BTreeMap<CitizenId, u64>,
    /// Signature of the ground the bays were found on (found again when
    /// it changes).
    #[serde(skip)]
    pub key: u64,
    pub visits_total: u64,
    pub takings_total: f64,
}

impl Undercroft {
    /// Bays by use, in `UnderUse::ALL` order.
    pub fn counts(&self) -> [usize; 6] {
        let mut n = [0; 6];
        for b in &self.bays {
            n[b.use_ as usize] += 1;
        }
        n
    }
}

/// One bay of ground under a deck, between two columns.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Bay {
    /// Stable: a hash of where it is (to the half metre).
    pub id: u64,
    /// Its centre, on the ground.
    pub pos: Vec2,
    /// Along the deck.
    pub dir: Vec2,
    /// Length along the deck, and width across it (the deck's).
    pub len: f32,
    pub width: f32,
    /// The deck's level over the ground here (its top), and the headroom
    /// under its soffit.
    pub level: f32,
    pub clearance: f32,
    /// A raised footpath over it (else a street or highway).
    pub walkway: bool,
    /// On a park's ground (a plaza under the deck: no lock-ups, no camp).
    pub park: bool,
    /// The way over it, by name ("" unnamed).
    pub under: String,
    pub use_: UnderUse,
    /// The day it took its use.
    pub since_day: u64,
    /// Who runs the stalls.
    pub holders: Vec<HouseholdId>,
    /// Days in a row the stalls took nobody.
    pub idle_days: u8,
    pub visits_today: u32,
    pub visits_yesterday: u32,
    pub takings_today: f32,
    pub takings_yesterday: f32,
    pub visits_total: u64,
}

impl Bay {
    /// A point on the bay's ground: `u` along it, `v` across (metres from
    /// the centre).
    pub fn at(&self, u: f32, v: f32) -> Vec2 {
        self.pos + self.dir * u + self.dir.perp() * v
    }

    /// Is `p` on the bay's ground (within `margin` of it)?
    pub fn contains(&self, p: Vec2, margin: f32) -> bool {
        let d = p - self.pos;
        d.dot(self.dir).abs() <= self.len * 0.5 + margin && d.dot(self.dir.perp()).abs() <= self.width * 0.5 + margin
    }

    /// Where a visitor walks in from: just out from under the deck's edge
    /// on the side `side` (+1 / -1), on the ground (a walk route there ends
    /// on the ground, not up on the deck).
    pub fn approach(&self, side: f32) -> Vec2 {
        self.at(0.0, side * (self.width * 0.5 + 2.5))
    }

    /// Where the visitor `k` of the hour stands or sits, in the bay.
    pub fn spot(&self, k: u64) -> Vec2 {
        let h = crate::hash::mix(self.id, k);
        let u = (crate::hash::unit(h) - 0.5) * (self.len - 2.0).max(1.0);
        let v = (crate::hash::unit(h >> 17) - 0.5) * (self.width - 2.0).max(0.5) * 0.8;
        self.at(u, v)
    }

    /// Where stallholder `k` stands, behind the stalls on the bay's spine.
    pub fn holder_spot(&self, k: usize) -> Vec2 {
        let n = MAX_HOLDERS as f32;
        let u = ((k as f32 + 0.5) / n - 0.5) * (self.len - 2.0).max(1.0);
        self.at(u, -self.width * 0.18)
    }

    pub fn open_at(&self, hour: u8) -> bool {
        self.use_.draws_visitors() && self.use_.open_at(hour) && (!self.use_.is_trade() || !self.holders.is_empty())
    }
}

/// Stable id of a bay at `p`.
pub fn bay_id(p: Vec2) -> u64 {
    crate::hash::mix2_salted((p.x * 2.0).round() as i64 as u64, (p.y * 2.0).round() as i64 as u64, 0x0BDE_C4)
}

/// The bays between consecutive column stations `ts` (distances along a
/// piece `len` long) where the deck stands over the ground: each span
/// between two columns, kept `COLUMN_CLEAR` from them, split in two when
/// longer than `MAX_BAY`. Returns `(centre, length)` along the piece.
pub fn spans(ts: &[f32], len: f32) -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    for w in ts.windows(2) {
        let (a, b) = (w[0].max(0.0), w[1].min(len));
        let free = b - a - 2.0 * COLUMN_CLEAR;
        if free < MIN_BAY {
            continue;
        }
        let n = (free / MAX_BAY).ceil().max(1.0);
        let each = free / n;
        for k in 0..n as usize {
            let c = a + COLUMN_CLEAR + each * (k as f32 + 0.5);
            out.push((c, each - if n > 1.0 { 0.6 } else { 0.0 }));
        }
    }
    out
}

/// What the blocks round a bay are like, for what it becomes.
#[derive(Clone, Copy, Debug, Default)]
pub struct Surroundings {
    /// Homes and jobs within `REACH`.
    pub homes: u32,
    pub shop_jobs: u32,
    pub office_jobs: u32,
    pub industry_jobs: u32,
    /// Land value at the bay.
    pub land_value: f32,
    /// A street at grade within reach of a van.
    pub street: bool,
    /// The same use in bays within 60 m, and within `REACH`.
    pub same_near: [u8; 6],
    pub same_reach: [u8; 6],
}

/// How well each use suits a bay; 0 does not.
pub fn suitability(u: UnderUse, s: &Surroundings, clearance: f32, width: f32, len: f32) -> f32 {
    let homes = s.homes as f32;
    let shops = s.shop_jobs as f32;
    let offices = s.office_jobs as f32;
    let industry = s.industry_jobs as f32;
    let lv = s.land_value;
    let crowd = s.same_near[u as usize] as f32;
    let rivals = s.same_reach[u as usize] as f32;
    // The custom each one needs: a market for every few hundred homes, a
    // hawker row for the workers as well, a court for the young people of
    // the blocks; the more there are in reach, the more each needs.
    let (base, per) = match u {
        UnderUse::Market if homes >= 60.0 && len >= 9.0 => ((homes / 400.0).min(1.5) + (shops / 120.0).min(0.6) + if (0.2..0.75).contains(&lv) { 0.3 } else { 0.0 }, 0.3),
        UnderUse::Hawkers if homes + offices + shops >= 80.0 => (((offices + shops) / 200.0).min(1.4) + (homes / 600.0).min(0.7), 0.3),
        UnderUse::Workshop if s.street && clearance >= 3.4 => ((industry / 150.0).min(1.2) + if lv < 0.4 { 0.7 } else { 0.2 } + (homes / 1500.0).min(0.3), 0.35),
        UnderUse::Court if homes >= 120.0 && width >= 9.0 && len >= 14.0 && clearance >= 4.5 => (0.5 + (homes / 800.0).min(1.0), 0.45),
        _ => (0.0, 0.0),
    };
    // Variety: a second of the same use beside one is much less likely.
    (base - 0.6 * crowd - per * rivals).max(0.0)
}
