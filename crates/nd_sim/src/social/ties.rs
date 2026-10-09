//! Ties between people: bonds, how they met, and the circles they make.

use super::*;

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

/// One side of a relationship. Saved as a tuple (`TieRepr`): a big city
/// has a hundred thousand of them, and a pretty-printed struct each made
/// the save a third larger.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "TieRepr", into = "TieRepr")]
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
    pub partner: bool,
    /// Romance building up from flirting, `0..1`.
    pub spark: f32,
    /// Money lent to them and not paid back.
    pub lent: f32,
}

/// A tie as saved: a tuple, so a new field breaks every old save. Never
/// add one here; add it to a new container with `#[serde(default)]`.
pub(super) type TieRepr = (u64, f32, f32, f32, u64, u64, Met, Option<u64>, u16, bool, f32, f32);

impl From<TieRepr> for Tie {
    fn from(t: TieRepr) -> Self {
        let (who, affinity, familiarity, trust, since, last, met, met_at, talks, partner, spark, lent) = t;
        Tie { who: from_ffi(who), affinity, familiarity, trust, since, last, met, met_at: met_at.map(from_ffi), talks, partner, spark, lent }
    }
}

impl From<Tie> for TieRepr {
    fn from(t: Tie) -> Self {
        (to_ffi(t.who), t.affinity, t.familiarity, t.trust, t.since, t.last, t.met, t.met_at.map(to_ffi), t.talks, t.partner, t.spark, t.lent)
    }
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
        if self.affinity <= RIVAL_AFFINITY && self.familiarity >= 0.15 {
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
