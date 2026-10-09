//! Conversations: topics, settings and outcomes, the notes people keep,
//! and who gets on with whom.

use super::*;

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
            Outcome::Favour => "✔",
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
            Setting::Work => 0.02,
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

/// One conversation as a citizen remembers it (saved as a tuple).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "NoteRepr", into = "NoteRepr")]
pub struct TalkNote {
    pub tick: u64,
    pub with: CitizenId,
    pub topic: Topic,
    pub outcome: Outcome,
    pub place: Option<BuildingId>,
    pub setting: Setting,
}

/// A talk note as saved: a tuple, so a new field breaks every old save
/// (see `TieRepr`).
pub(super) type NoteRepr = (u64, u64, Topic, Outcome, Option<u64>, Setting);

impl From<NoteRepr> for TalkNote {
    fn from(n: NoteRepr) -> Self {
        let (tick, with, topic, outcome, place, setting) = n;
        TalkNote { tick, with: from_ffi(with), topic, outcome, place: place.map(from_ffi), setting }
    }
}

impl From<TalkNote> for NoteRepr {
    fn from(n: TalkNote) -> Self {
        (n.tick, to_ffi(n.with), n.topic, n.outcome, n.place.map(to_ffi), n.setting)
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
    let band = 0.45 * hed + 0.35 * young + 0.25 * soc + 0.15 * cur - 0.3 * old - 0.2 + q(0);
    let sports = 0.4 * amb + 0.3 * soc + 0.3 * young - 0.15 * cur - 0.25 * old + 0.06 + q(1);
    let books = 0.5 * cur + 0.3 * cau + 0.25 * old - 0.25 * hed + 0.08 + q(2);
    let art = 0.45 * cur + 0.35 * hed + 0.25 * young - 0.35 * cau - 0.2 * rich + 0.12 + q(3);
    let startup = (0.6 * amb + 0.25 * cur + 0.2 * rich - 0.15 * old - 0.2 * poor - 0.04 + q(4)) * adult;
    let garden = 0.4 * cau + 0.3 * old + 0.2 * soc - 0.2 * hed + 0.1 * poor + 0.08 + q(5);
    let hood = (0.35 * cau + 0.3 * old + 0.25 * soc + 0.15 * amb - 0.04 + q(6)) * adult;
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
    let temper = 0.45 - gap * 1.4;
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
