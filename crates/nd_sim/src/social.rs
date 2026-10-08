//! [social] People talking to each other: ties between citizens, the short
//! conversations that move them, and the crews and clubs that grow out of
//! friendships and make things in the city.
//!
//! This file holds the data and the pure rules (compatibility, labels,
//! interests, how a conversation goes); `world/social.rs` runs them: who
//! meets whom where, what friends and enemies then do, and what groups make.
//!
//! - A [`Tie`] is one side of a relationship: affinity (`-1..1`, liking),
//!   familiarity (`0..1`, how well they know each other), trust (`0..1`),
//!   when and how they met and the last contact. The label
//!   ([`Bond`]: stranger, acquaintance, friend, close friend, partner,
//!   rival, enemy) is derived. Each citizen keeps at most [`MAX_TIES`];
//!   the weakest goes first. Ties fade without contact.
//! - [`compatibility`] is fixed per pair: personalities, tastes, age, class,
//!   shared interests, plus a little luck from a hash of the pair.
//! - A [`Talk`] is one conversation: a [`Topic`] drawn from their lives and
//!   an [`Outcome`] that moves both ties.
//! - A [`Group`] (band, sports club, book club, street-art crew, startup,
//!   community garden, neighbourhood association) has members, momentum and
//!   a reputation; at thresholds it makes something ([`Work`]).
//!
//! Nothing here draws from the world's random stream: every roll is a hash
//! of the pair, the tick and a salt (`pedestrians::hash01`), so the same
//! seed makes the same friends.

use crate::buildings::{BuildingId, Tier};
use crate::citizens::CitizenId;
use crate::concerns::Concern;
use crate::zoning::LotId;
use glam::Vec2;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};

pub use crate::pedestrians::hash01;

/// Ties a citizen keeps at most; past this the weakest is dropped.
pub const MAX_TIES: usize = 16;
/// Conversations each citizen remembers (newest last).
pub const MAX_TALK_NOTES: usize = 6;
/// Conversations kept city-wide for the speech bubbles and the API.
pub const RECENT_TALKS: usize = 400;
/// News lines kept.
pub const NEWS_KEEP: usize = 60;
/// Days of social history kept.
pub const HISTORY_DAYS: usize = 60;
/// Members a group takes at most.
pub const MAX_MEMBERS: usize = 10;
/// Groups a citizen belongs to at most.
pub const MAX_GROUPS_EACH: usize = 2;
/// Minutes between two conversations of the same pair.
pub const PAIR_COOLDOWN: u64 = 45;

/// Affinity and familiarity thresholds of the labels.
pub const FRIEND_AFFINITY: f32 = 0.3;
pub const FRIEND_FAMILIARITY: f32 = 0.25;
pub const CLOSE_AFFINITY: f32 = 0.6;
pub const CLOSE_FAMILIARITY: f32 = 0.5;
pub const RIVAL_AFFINITY: f32 = -0.25;
pub const ENEMY_AFFINITY: f32 = -0.55;

/// What one person is to another, from the tie.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Bond {
    Stranger,
    Acquaintance,
    Friend,
    CloseFriend,
    Partner,
    Rival,
    Enemy,
}

impl Bond {
    pub fn label(self) -> &'static str {
        match self {
            Bond::Stranger => "stranger",
            Bond::Acquaintance => "acquaintance",
            Bond::Friend => "friend",
            Bond::CloseFriend => "close friend",
            Bond::Partner => "partner",
            Bond::Rival => "rival",
            Bond::Enemy => "enemy",
        }
    }

    pub fn is_friendly(self) -> bool {
        matches!(self, Bond::Friend | Bond::CloseFriend | Bond::Partner)
    }

    pub fn is_hostile(self) -> bool {
        matches!(self, Bond::Rival | Bond::Enemy)
    }

    /// Order for lists: partner, close friends, friends, enemies, rivals,
    /// acquaintances.
    pub fn rank(self) -> u8 {
        match self {
            Bond::Partner => 0,
            Bond::CloseFriend => 1,
            Bond::Friend => 2,
            Bond::Enemy => 3,
            Bond::Rival => 4,
            Bond::Acquaintance => 5,
            Bond::Stranger => 6,
        }
    }
}

/// Where two people first got talking.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Met {
    #[default]
    Around,
    /// Side by side on a park bench.
    Bench,
    /// At a bar, club, café or shop.
    Venue,
    Park,
    Work,
    /// On a bus, train or the metro.
    Transit,
    /// Passing in the street, or at a bus stop.
    Street,
    /// At someone's home.
    Home,
    /// In a crew or club.
    Group,
    /// Through a friend.
    Introduced,
}

impl Met {
    pub fn label(self) -> &'static str {
        match self {
            Met::Around => "around town",
            Met::Bench => "on a park bench",
            Met::Venue => "at a bar",
            Met::Park => "in the park",
            Met::Work => "at work",
            Met::Transit => "on the bus",
            Met::Street => "in the street",
            Met::Home => "at a friend's place",
            Met::Group => "in a club",
            Met::Introduced => "through a friend",
        }
    }
}

/// One side of a relationship.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tie {
    pub who: CitizenId,
    /// Liking, `-1..1`.
    pub affinity: f32,
    /// How well they know each other, `0..1`.
    pub familiarity: f32,
    /// `0..1`: lend money, put in a word.
    pub trust: f32,
    /// Tick they first talked, and the last contact.
    pub since: u64,
    pub last: u64,
    pub met: Met,
    /// Where they met (a building), if anywhere.
    pub met_at: Option<BuildingId>,
    pub talks: u16,
    /// A couple.
    #[serde(default)]
    pub partner: bool,
    /// Romance building up from flirting, `0..1`.
    #[serde(default)]
    pub spark: f32,
    /// Money lent to them and not paid back.
    #[serde(default)]
    pub lent: f32,
}

impl Tie {
    pub fn new(who: CitizenId, tick: u64, met: Met, met_at: Option<BuildingId>) -> Self {
        Self { who, affinity: 0.0, familiarity: 0.0, trust: 0.2, since: tick, last: tick, met, met_at, talks: 0, partner: false, spark: 0.0, lent: 0.0 }
    }

    pub fn bond(&self) -> Bond {
        if self.partner {
            return Bond::Partner;
        }
        if self.affinity <= ENEMY_AFFINITY {
            return Bond::Enemy;
        }
        if self.affinity <= RIVAL_AFFINITY && self.familiarity >= 0.1 {
            return Bond::Rival;
        }
        if self.affinity >= CLOSE_AFFINITY && self.familiarity >= CLOSE_FAMILIARITY {
            return Bond::CloseFriend;
        }
        if self.affinity >= FRIEND_AFFINITY && self.familiarity >= FRIEND_FAMILIARITY {
            return Bond::Friend;
        }
        if self.familiarity >= 0.08 {
            return Bond::Acquaintance;
        }
        Bond::Stranger
    }

    /// How much the tie is worth keeping: strong feelings either way and
    /// long acquaintance stay; a passing word goes.
    pub fn strength(&self) -> f32 {
        let p = if self.partner { 2.0 } else { 0.0 };
        self.affinity.abs() * 1.2 + self.familiarity + p
    }
}

/// A citizen's people: ties, the last few conversations and their groups.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Circle {
    pub ties: Vec<Tie>,
    pub talks: Vec<TalkNote>,
    pub groups: Vec<u32>,
}

impl Circle {
    pub fn tie(&self, who: CitizenId) -> Option<&Tie> {
        self.ties.iter().find(|t| t.who == who)
    }

    pub fn tie_mut(&mut self, who: CitizenId) -> Option<&mut Tie> {
        self.ties.iter_mut().find(|t| t.who == who)
    }

    pub fn bond(&self, who: CitizenId) -> Bond {
        self.tie(who).map_or(Bond::Stranger, |t| t.bond())
    }

    pub fn partner(&self) -> Option<CitizenId> {
        self.ties.iter().find(|t| t.partner).map(|t| t.who)
    }

    /// The tie with `who`, made if new (dropping the weakest past the cap,
    /// never a partner).
    pub fn tie_or_new(&mut self, who: CitizenId, tick: u64, met: Met, at: Option<BuildingId>) -> &mut Tie {
        if let Some(i) = self.ties.iter().position(|t| t.who == who) {
            return &mut self.ties[i];
        }
        if self.ties.len() >= MAX_TIES {
            if let Some(k) = (0..self.ties.len()).filter(|&k| !self.ties[k].partner).min_by(|&a, &b| self.ties[a].strength().total_cmp(&self.ties[b].strength()).then(self.ties[a].last.cmp(&self.ties[b].last))) {
                self.ties.remove(k);
            }
        }
        self.ties.push(Tie::new(who, tick, met, at));
        self.ties.last_mut().unwrap()
    }

    pub fn note(&mut self, n: TalkNote) {
        self.talks.push(n);
        if self.talks.len() > MAX_TALK_NOTES {
            self.talks.remove(0);
        }
    }

    pub fn friends(&self) -> impl Iterator<Item = &Tie> {
        self.ties.iter().filter(|t| t.bond().is_friendly())
    }

    pub fn foes(&self) -> impl Iterator<Item = &Tie> {
        self.ties.iter().filter(|t| t.bond().is_hostile())
    }

    pub fn is_empty(&self) -> bool {
        self.ties.is_empty() && self.talks.is_empty() && self.groups.is_empty()
    }
}

/// What two people talked about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Topic {
    Work,
    Rent,
    /// One of the city's concerns.
    City(Concern),
    /// Something that happened lately (a crime, a fire, a race).
    News,
    /// The game at the stadium.
    Sports,
    /// A place they both go to.
    Place,
    /// A third person they both know.
    Gossip,
    Music,
    Art,
    Books,
    Weather,
    Family,
}

impl Topic {
    pub fn label(self) -> &'static str {
        match self {
            Topic::Work => "work",
            Topic::Rent => "rent",
            Topic::City(c) => match c {
                Concern::Housing => "housing",
                Concern::Food => "food prices",
                Concern::Jobs => "jobs",
                Concern::Safety => "crime",
                Concern::Trash => "the trash",
                Concern::Noise => "the noise",
                Concern::Commute => "traffic",
                Concern::Parking => "parking",
                Concern::Power => "the power cuts",
                Concern::Services => "the city",
                Concern::Nightlife => "nightlife",
                Concern::Displacement => "people being pushed out",
                Concern::DangerousStreets => "dangerous streets",
                Concern::Healthcare => "hospitals",
                Concern::Other => "the city",
            },
            Topic::News => "the news",
            Topic::Sports => "the game",
            Topic::Place => "a place they like",
            Topic::Gossip => "gossip",
            Topic::Music => "music",
            Topic::Art => "art",
            Topic::Books => "books",
            Topic::Weather => "the weather",
            Topic::Family => "family",
        }
    }

    /// Stable key for the remote API.
    pub fn key(self) -> &'static str {
        match self {
            Topic::Work => "work",
            Topic::Rent => "rent",
            Topic::City(_) => "city",
            Topic::News => "news",
            Topic::Sports => "sports",
            Topic::Place => "place",
            Topic::Gossip => "gossip",
            Topic::Music => "music",
            Topic::Art => "art",
            Topic::Books => "books",
            Topic::Weather => "weather",
            Topic::Family => "family",
        }
    }

    /// How touchy the subject is: rent and the city's troubles split
    /// people more than the weather does. `0..1`.
    pub fn heat(self) -> f32 {
        match self {
            Topic::Rent | Topic::City(_) => 0.55,
            Topic::Gossip => 0.45,
            Topic::Sports | Topic::Work => 0.35,
            Topic::News => 0.3,
            Topic::Music | Topic::Art | Topic::Books | Topic::Place => 0.2,
            Topic::Family => 0.15,
            Topic::Weather => 0.05,
        }
    }
}

/// How a conversation went.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Outcome {
    Agreed,
    Laughed,
    Flirted,
    /// One did the other a favour (a tip, a hand, a word put in).
    Favour,
    Argued,
    Insulted,
    /// Came to blows.
    Fight,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Outcome::Agreed => "agreed",
            Outcome::Laughed => "laughed",
            Outcome::Flirted => "flirted",
            Outcome::Favour => "a favour",
            Outcome::Argued => "argued",
            Outcome::Insulted => "insults",
            Outcome::Fight => "a fight",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Outcome::Agreed => "agreed",
            Outcome::Laughed => "laughed",
            Outcome::Flirted => "flirted",
            Outcome::Favour => "favour",
            Outcome::Argued => "argued",
            Outcome::Insulted => "insulted",
            Outcome::Fight => "fight",
        }
    }

    pub fn is_good(self) -> bool {
        matches!(self, Outcome::Agreed | Outcome::Laughed | Outcome::Flirted | Outcome::Favour)
    }

    /// Affinity change for both.
    pub fn affinity(self) -> f32 {
        match self {
            Outcome::Agreed => 0.07,
            Outcome::Laughed => 0.1,
            Outcome::Flirted => 0.11,
            Outcome::Favour => 0.13,
            Outcome::Argued => -0.1,
            Outcome::Insulted => -0.18,
            Outcome::Fight => -0.35,
        }
    }

    /// Trust change for both.
    pub fn trust(self) -> f32 {
        match self {
            Outcome::Favour => 0.12,
            Outcome::Agreed => 0.04,
            Outcome::Laughed | Outcome::Flirted => 0.03,
            Outcome::Argued => -0.04,
            Outcome::Insulted => -0.1,
            Outcome::Fight => -0.25,
        }
    }

    /// The bubble's icon.
    pub fn icon(self) -> &'static str {
        match self {
            Outcome::Agreed => "💬",
            Outcome::Laughed => "😂",
            Outcome::Flirted => "❤",
            Outcome::Favour => "🤝",
            Outcome::Argued => "😠",
            Outcome::Insulted => "💢",
            Outcome::Fight => "💥",
        }
    }
}

/// Where a conversation happened.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Setting {
    Venue,
    Park,
    Bench,
    Work,
    Home,
    Transit,
    Street,
    Stop,
    Stadium,
}

impl Setting {
    pub fn met(self) -> Met {
        match self {
            Setting::Venue | Setting::Stadium => Met::Venue,
            Setting::Park => Met::Park,
            Setting::Bench => Met::Bench,
            Setting::Work => Met::Work,
            Setting::Home => Met::Home,
            Setting::Transit => Met::Transit,
            Setting::Street | Setting::Stop => Met::Street,
        }
    }

    /// How likely a pair sharing it gets talking per look, before
    /// personality: bars are for talking, a bus mostly is not.
    pub fn chattiness(self) -> f32 {
        match self {
            Setting::Venue => 0.16,
            Setting::Stadium => 0.12,
            Setting::Bench => 0.12,
            Setting::Park => 0.07,
            Setting::Home => 0.25,
            Setting::Work => 0.035,
            Setting::Transit => 0.025,
            Setting::Stop => 0.04,
            Setting::Street => 0.012,
        }
    }

    pub fn is_open_air(self) -> bool {
        matches!(self, Setting::Park | Setting::Bench | Setting::Street | Setting::Stop)
    }
}

/// One conversation, city-wide (for the bubbles and the API).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Talk {
    /// Increasing number, so a viewer can tell new from seen.
    pub serial: u64,
    pub tick: u64,
    pub a: CitizenId,
    pub b: CitizenId,
    pub topic: Topic,
    pub outcome: Outcome,
    pub setting: Setting,
    pub place: Option<BuildingId>,
    /// Where it happened (between the two, or the door of the place).
    pub pos: Vec2,
    /// A short line of what was said.
    pub line: String,
}

/// One conversation as a citizen remembers it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TalkNote {
    pub tick: u64,
    pub with: CitizenId,
    pub topic: Topic,
    pub outcome: Outcome,
    pub place: Option<BuildingId>,
    pub setting: Setting,
}

/// What a group is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum GroupKind {
    Band,
    SportsClub,
    BookClub,
    StreetArt,
    Startup,
    Garden,
    Neighbourhood,
}

impl GroupKind {
    pub const ALL: [GroupKind; 7] = [GroupKind::Band, GroupKind::SportsClub, GroupKind::BookClub, GroupKind::StreetArt, GroupKind::Startup, GroupKind::Garden, GroupKind::Neighbourhood];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            GroupKind::Band => "band",
            GroupKind::SportsClub => "sports club",
            GroupKind::BookClub => "book club",
            GroupKind::StreetArt => "street-art crew",
            GroupKind::Startup => "startup",
            GroupKind::Garden => "community garden group",
            GroupKind::Neighbourhood => "neighbourhood association",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            GroupKind::Band => "band",
            GroupKind::SportsClub => "sports_club",
            GroupKind::BookClub => "book_club",
            GroupKind::StreetArt => "street_art",
            GroupKind::Startup => "startup",
            GroupKind::Garden => "garden",
            GroupKind::Neighbourhood => "neighbourhood",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            GroupKind::Band => "♪",
            GroupKind::SportsClub => "⚽",
            GroupKind::BookClub => "📖",
            GroupKind::StreetArt => "🎨",
            GroupKind::Startup => "💡",
            GroupKind::Garden => "🌱",
            GroupKind::Neighbourhood => "🏠",
        }
    }

    /// The topic its members talk about.
    pub fn topic(self) -> Topic {
        match self {
            GroupKind::Band => Topic::Music,
            GroupKind::SportsClub => Topic::Sports,
            GroupKind::BookClub => Topic::Books,
            GroupKind::StreetArt => Topic::Art,
            GroupKind::Startup => Topic::Work,
            GroupKind::Garden => Topic::Weather,
            GroupKind::Neighbourhood => Topic::City(Concern::Other),
        }
    }

    /// Momentum it takes to make something.
    pub fn threshold(self) -> f32 {
        match self {
            GroupKind::Band => 1.2,
            GroupKind::SportsClub => 1.5,
            GroupKind::BookClub => 1.5,
            GroupKind::StreetArt => 1.6,
            GroupKind::Startup => 3.0,
            GroupKind::Garden => 2.0,
            GroupKind::Neighbourhood => 2.0,
        }
    }

    /// Members it takes to start.
    pub fn min_members(self) -> usize {
        match self {
            GroupKind::Startup => 2,
            _ => 3,
        }
    }
}

/// What a person cares about, by group kind, `0..1`: from personality,
/// age and class. Fixed per citizen.
pub fn interests(seed: u64, age: u8, class: Tier) -> [f32; 7] {
    let [soc, amb, hed, cau, cur] = crate::places::traits(seed);
    let young = ((40.0 - age as f32) / 22.0).clamp(0.0, 1.0);
    let old = ((age as f32 - 50.0) / 25.0).clamp(0.0, 1.0);
    let adult = if age >= 18 { 1.0 } else { 0.0 };
    let (rich, poor) = match class {
        Tier::High => (1.0, 0.0),
        Tier::Mid => (0.4, 0.0),
        Tier::Low => (0.0, 1.0),
    };
    // A personal quirk per kind, from the seed.
    let q = |k: u64| hash01(seed, 0x1A7E_4E57, k) * 0.3 - 0.15;
    let band = 0.45 * hed + 0.35 * young + 0.25 * soc + 0.15 * cur - 0.3 * old + q(0);
    let sports = 0.4 * amb + 0.3 * soc + 0.3 * young - 0.15 * cur - 0.25 * old + q(1);
    let books = 0.5 * cur + 0.3 * cau + 0.25 * old - 0.25 * hed + q(2);
    let art = 0.45 * cur + 0.35 * hed + 0.25 * young - 0.35 * cau - 0.2 * rich + q(3);
    let startup = (0.6 * amb + 0.25 * cur + 0.2 * rich - 0.15 * old - 0.2 * poor + q(4)) * adult;
    let garden = 0.4 * cau + 0.3 * old + 0.2 * soc - 0.2 * hed + 0.1 * poor + q(5);
    let hood = (0.35 * cau + 0.3 * old + 0.25 * soc + 0.15 * amb + q(6)) * adult;
    [band, sports, books, art, startup, garden, hood].map(|x| x.clamp(0.0, 1.0))
}

/// What a pair has going for them, fixed for the pair, about `-1..1`:
/// similar temperaments get on, so do people of an age and a class, the
/// shared interests count, opposite temperaments rub, and a little luck
/// (chemistry) from a hash of the pair.
pub fn compatibility(a: (u64, u8, Tier), b: (u64, u8, Tier)) -> f32 {
    let ta = crate::places::traits(a.0);
    let tb = crate::places::traits(b.0);
    let ia = interests(a.0, a.1, a.2);
    let ib = interests(b.0, b.1, b.2);
    // Temperament: the gap on each trait (sociability and hedonism most).
    let w = [0.3, 0.15, 0.3, 0.15, 0.1];
    let gap: f32 = (0..5).map(|k| w[k] * (ta[k] - tb[k]).abs()).sum();
    let temper = 0.35 - gap * 1.4;
    // Age: a decade apart is fine, three are not.
    let years = (a.1 as f32 - b.1 as f32).abs();
    let age = 0.15 - (years / 30.0).min(1.0) * 0.4;
    // Class: rich and poor rarely see eye to eye.
    let class = 0.08 - 0.18 * (a.2.index() as f32 - b.2.index() as f32).abs();
    // Shared passions.
    let shared: f32 = (0..7).map(|k| ia[k].min(ib[k])).fold(0.0, f32::max);
    let interest = 0.5 * (shared - 0.35);
    // Chemistry.
    let (lo, hi) = if a.0 < b.0 { (a.0, b.0) } else { (b.0, a.0) };
    let luck = (hash01(lo, hi, 0xC4E3_15) - 0.5) * 0.6;
    (temper + age + class + interest + luck).clamp(-1.0, 1.0)
}

/// Something a group made.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Work {
    Gig { venue: BuildingId, day: u64 },
    Match { park: BuildingId, day: u64 },
    Reading { place: BuildingId, day: u64 },
    Mural { lot: LotId, day: u64 },
    Garden { lot: LotId, day: u64 },
    Business { building: BuildingId, day: u64 },
    Petition { id: u32, day: u64 },
    Protest { petition: u32, day: u64 },
}

impl Work {
    pub fn day(&self) -> u64 {
        match self {
            Work::Gig { day, .. }
            | Work::Match { day, .. }
            | Work::Reading { day, .. }
            | Work::Mural { day, .. }
            | Work::Garden { day, .. }
            | Work::Business { day, .. }
            | Work::Petition { day, .. }
            | Work::Protest { day, .. } => *day,
        }
    }

    pub fn key(&self) -> &'static str {
        match self {
            Work::Gig { .. } => "gig",
            Work::Match { .. } => "match",
            Work::Reading { .. } => "reading",
            Work::Mural { .. } => "mural",
            Work::Garden { .. } => "garden",
            Work::Business { .. } => "business",
            Work::Petition { .. } => "petition",
            Work::Protest { .. } => "protest",
        }
    }
}

/// A crew or a club.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group {
    pub id: u32,
    pub kind: GroupKind,
    pub name: String,
    pub founded: u64,
    pub founder: CitizenId,
    pub members: Vec<CitizenId>,
    /// Builds with meetings and members, spent on making things.
    pub momentum: f32,
    /// What the town thinks of it, `-1..1`.
    pub reputation: f32,
    /// Where it meets (a place its members like), if anywhere.
    pub base: Option<BuildingId>,
    pub works: Vec<Work>,
    /// Broke away from this group.
    #[serde(default)]
    pub split_from: Option<u32>,
    /// Day it last met, and how many times.
    pub last_met: u64,
    pub meetings: u32,
    /// Days running with almost no momentum (it folds after a while).
    #[serde(default)]
    pub idle_days: u16,
    /// Folded (kept for the record a while).
    #[serde(default)]
    pub folded: Option<u64>,
}

impl Group {
    pub fn active(&self) -> bool {
        self.folded.is_none()
    }
}

/// A painted wall.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mural {
    pub group: u32,
    pub lot: LotId,
    pub building: Option<BuildingId>,
    pub seed: u64,
    pub day: u64,
    /// The crew's reputation when it was painted: art or a tag, `-1..1`.
    pub quality: f32,
    /// The painted wall: its middle at the lot front, along the front,
    /// the way it faces (to the street).
    pub pos: Vec2,
    pub along: Vec2,
    pub facing: Vec2,
    pub width: f32,
    pub height: f32,
}

/// A community garden on a vacant lot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Garden {
    pub group: u32,
    pub lot: LotId,
    pub seed: u64,
    pub day: u64,
    pub pos: Vec2,
}

/// A band playing a venue.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gig {
    pub group: u32,
    pub venue: BuildingId,
    pub from: u64,
    pub until: u64,
    pub crowd: u32,
}

impl Gig {
    pub fn on_at(&self, tick: u64) -> bool {
        tick >= self.from && tick < self.until
    }
}

/// A neighbourhood association's request to the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Ask {
    MorePolice,
    LowerRent,
    Crossing,
    CleanStreets,
    APark,
    Jobs,
    Quiet,
}

impl Ask {
    pub fn concern(self) -> Concern {
        match self {
            Ask::MorePolice => Concern::Safety,
            Ask::LowerRent => Concern::Housing,
            Ask::Crossing => Concern::DangerousStreets,
            Ask::CleanStreets => Concern::Trash,
            Ask::APark => Concern::Other,
            Ask::Jobs => Concern::Jobs,
            Ask::Quiet => Concern::Noise,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Ask::MorePolice => "more_police",
            Ask::LowerRent => "lower_rent",
            Ask::Crossing => "crossing",
            Ask::CleanStreets => "clean_streets",
            Ask::APark => "park",
            Ask::Jobs => "jobs",
            Ask::Quiet => "quiet",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PetitionState {
    Open,
    /// The city did something about it.
    Answered,
    /// Ignored long enough: they took to the street.
    Protested,
    /// Gave up.
    Lapsed,
}

impl PetitionState {
    pub fn label(self) -> &'static str {
        match self {
            PetitionState::Open => "open",
            PetitionState::Answered => "answered",
            PetitionState::Protested => "protested",
            PetitionState::Lapsed => "lapsed",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Petition {
    pub id: u32,
    pub group: u32,
    pub day: u64,
    pub ask: Ask,
    /// What it says, for the player.
    pub text: String,
    /// The neighbourhood it is about.
    pub at: Vec2,
    pub signatures: u32,
    pub state: PetitionState,
    /// Where they would protest (a civic building or a park).
    #[serde(default)]
    pub rally: Option<BuildingId>,
    /// The day the state last changed.
    #[serde(default)]
    pub settled: Option<u64>,
}

/// A planned meet-up of two friends.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Outing {
    /// The one calling round, and the one called on.
    pub caller: CitizenId,
    pub friend: CitizenId,
    pub place: BuildingId,
    /// Calling at the friend's home first, to walk there together.
    pub pickup: Option<BuildingId>,
    pub tick: u64,
}

/// A line of social news.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct News {
    pub seq: u32,
    pub tick: u64,
    pub text: String,
    pub pos: Vec2,
    pub group: Option<u32>,
}

/// The day's counts.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SocialDay {
    pub day: u64,
    pub talks: u32,
    pub good_talks: u32,
    pub bad_talks: u32,
    pub friendships: u32,
    pub close_friendships: u32,
    pub couples: u32,
    pub feuds: u32,
    pub fights: u32,
    pub meetups: u32,
    pub home_visits: u32,
    pub favours: u32,
    pub loans: u32,
    pub jobs_found: u32,
    pub homes_found: u32,
    pub moved_in: u32,
    pub avoided: u32,
    pub gossip: u32,
    pub groups_formed: u32,
    pub groups_folded: u32,
    pub splits: u32,
    pub works: u32,
}

/// Everything social, saved with the world.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Social {
    pub circles: BTreeMap<CitizenId, Circle>,
    pub groups: Vec<Group>,
    pub next_group: u32,
    pub murals: Vec<Mural>,
    pub gardens: Vec<Garden>,
    pub gigs: Vec<Gig>,
    pub petitions: Vec<Petition>,
    pub next_petition: u32,
    pub outings: Vec<Outing>,
    /// Two friends walking together: each keeps the slower pace until then.
    pub walking_with: BTreeMap<CitizenId, (CitizenId, u64)>,
    pub news: Vec<News>,
    pub news_seq: u32,
    pub today: SocialDay,
    pub history: VecDeque<SocialDay>,
    pub talks_total: u64,
    pub friendships_total: u32,
    pub feuds_total: u32,
    pub fights_total: u32,
    pub couples_total: u32,
    /// Recent conversations, for the bubbles (not saved).
    #[serde(skip)]
    pub recent: VecDeque<Talk>,
    #[serde(skip)]
    pub serial: u64,
    /// Place choices an enemy steered today (counted by `social_pull`).
    #[serde(skip)]
    pub avoid_hits: std::cell::Cell<u32>,
}

impl Social {
    pub fn circle(&self, c: CitizenId) -> Option<&Circle> {
        self.circles.get(&c)
    }

    pub fn bond(&self, a: CitizenId, b: CitizenId) -> Bond {
        self.circles.get(&a).map_or(Bond::Stranger, |c| c.bond(b))
    }

    pub fn group(&self, id: u32) -> Option<&Group> {
        self.groups.iter().find(|g| g.id == id)
    }

    pub fn group_mut(&mut self, id: u32) -> Option<&mut Group> {
        self.groups.iter_mut().find(|g| g.id == id)
    }

    pub fn active_groups(&self) -> impl Iterator<Item = &Group> {
        self.groups.iter().filter(|g| g.active())
    }

    pub fn gig_at(&self, venue: BuildingId, tick: u64) -> Option<&Gig> {
        self.gigs.iter().find(|g| g.venue == venue && g.on_at(tick))
    }

    pub fn push_news(&mut self, tick: u64, text: String, pos: Vec2, group: Option<u32>) {
        self.news_seq += 1;
        self.news.push(News { seq: self.news_seq, tick, text, pos, group });
        if self.news.len() > NEWS_KEEP {
            self.news.remove(0);
        }
    }

    pub fn push_talk(&mut self, mut t: Talk) {
        self.serial += 1;
        t.serial = self.serial;
        self.recent.push_back(t);
        while self.recent.len() > RECENT_TALKS {
            self.recent.pop_front();
        }
    }

    /// Ties of each label city-wide (each pair counted from both sides).
    pub fn bond_counts(&self) -> BTreeMap<Bond, u32> {
        let mut out = BTreeMap::new();
        for c in self.circles.values() {
            for t in &c.ties {
                *out.entry(t.bond()).or_insert(0) += 1;
            }
        }
        out
    }

    /// Close a day: push it on the history and start a new one.
    pub fn roll_day(&mut self, day: u64) {
        let mut d = std::mem::take(&mut self.today);
        d.day = day.saturating_sub(1);
        self.history.push_back(d);
        while self.history.len() > HISTORY_DAYS {
            self.history.pop_front();
        }
        self.today.day = day;
    }

    pub fn yesterday(&self) -> Option<&SocialDay> {
        self.history.back()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(n: usize) -> Vec<CitizenId> {
        let mut m: slotmap::SlotMap<CitizenId, ()> = slotmap::SlotMap::with_key();
        (0..n).map(|_| m.insert(())).collect()
    }

    #[test]
    fn labels_follow_affinity_and_familiarity() {
        let who = ids(1)[0];
        let mut t = Tie::new(who, 0, Met::Venue, None);
        assert_eq!(t.bond(), Bond::Stranger);
        t.familiarity = 0.1;
        assert_eq!(t.bond(), Bond::Acquaintance);
        t.affinity = 0.35;
        t.familiarity = 0.3;
        assert_eq!(t.bond(), Bond::Friend);
        t.affinity = 0.7;
        t.familiarity = 0.6;
        assert_eq!(t.bond(), Bond::CloseFriend);
        t.partner = true;
        assert_eq!(t.bond(), Bond::Partner);
        t.partner = false;
        t.affinity = -0.3;
        assert_eq!(t.bond(), Bond::Rival);
        t.affinity = -0.6;
        assert_eq!(t.bond(), Bond::Enemy);
    }

    #[test]
    fn the_weakest_tie_goes_past_the_cap_never_a_partner() {
        let people = ids(MAX_TIES + 3);
        let mut c = Circle::default();
        {
            let p = c.tie_or_new(people[0], 0, Met::Venue, None);
            p.partner = true;
        }
        for (k, &w) in people.iter().enumerate().skip(1) {
            let t = c.tie_or_new(w, k as u64, Met::Venue, None);
            t.familiarity = 0.1 + k as f32 * 0.01;
        }
        assert_eq!(c.ties.len(), MAX_TIES);
        assert!(c.tie(people[0]).is_some(), "the partner stays");
        assert!(c.tie(people[1]).is_none(), "the faintest went");
        assert!(c.tie(*people.last().unwrap()).is_some());
    }

    #[test]
    fn like_minds_of_an_age_get_on_better_than_opposites() {
        // Two near-identical temperaments against two opposite ones.
        let seeds: Vec<u64> = (0..4000u64).collect();
        let t = |s: u64| crate::places::traits(s);
        let mut pairs_close = Vec::new();
        let mut pairs_far = Vec::new();
        for &a in seeds.iter().take(300) {
            for &b in seeds.iter().skip(300).take(300) {
                let (ta, tb) = (t(a), t(b));
                let gap: f32 = (0..5).map(|k| (ta[k] - tb[k]).abs()).sum();
                if gap < 0.6 {
                    pairs_close.push(compatibility((a, 30, Tier::Mid), (b, 32, Tier::Mid)));
                } else if gap > 2.6 {
                    pairs_far.push(compatibility((a, 25, Tier::Low), (b, 70, Tier::High)));
                }
            }
        }
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32;
        assert!(pairs_close.len() > 20 && pairs_far.len() > 20, "{} {}", pairs_close.len(), pairs_far.len());
        assert!(mean(&pairs_close) > 0.3, "like minds: {}", mean(&pairs_close));
        assert!(mean(&pairs_far) < -0.3, "opposites: {}", mean(&pairs_far));
        // Symmetric.
        assert_eq!(compatibility((5, 30, Tier::Mid), (9, 40, Tier::Low)), compatibility((9, 40, Tier::Low), (5, 30, Tier::Mid)));
    }

    #[test]
    fn interests_follow_temperament_and_age() {
        let young_wild = (0..3000u64).find(|&s| {
            let t = crate::places::traits(s);
            t[2] > 0.8 && t[0] > 0.6
        });
        let s = young_wild.unwrap();
        let i = interests(s, 22, Tier::Low);
        let o = interests(s, 75, Tier::Low);
        assert!(i[GroupKind::Band.index()] > o[GroupKind::Band.index()]);
        assert_eq!(interests(s, 12, Tier::Low)[GroupKind::Startup.index()], 0.0, "no children's startups");
    }
}
