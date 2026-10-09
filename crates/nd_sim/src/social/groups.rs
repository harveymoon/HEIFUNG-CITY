//! Groups and what they do: murals, gardens, gigs, petitions and outings.

use super::*;

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
