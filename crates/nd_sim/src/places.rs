//! Places and taste: what a venue, shop or park is like (`PlaceCharacter`),
//! what a citizen likes (`Taste`), and what they remember of the places
//! they have been (`PlaceBook`). The world side (`world/places.rs`) rates
//! each visit, picks where to go and keeps the venues' reputations.
//!
//! Everything random here takes the caller's seeded RNG, so a replay of
//! the same city makes the same choices.

use crate::buildings::{Building, BuildingId, Tier};
use crate::citizens::CitizenId;
use crate::market::BusinessKind;
use crate::zoning::LandUse;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Places a citizen remembers; past this the least valued is forgotten.
pub const MAX_PLACES: usize = 12;
/// People a citizen knows from their regular haunts.
pub const MAX_ACQUAINTANCES: usize = 8;
/// An experience at or below this, at a place that is not a habit yet,
/// and they never go back.
pub const NEVER_AGAIN: f32 = -0.5;
/// An experience at or above this is a great one (and a first one worth
/// writing down).
pub const GREAT: f32 = 0.5;
/// Visits and average experience that make someone a regular.
pub const REGULAR_VISITS: u16 = 3;
pub const REGULAR_SCORE: f32 = 0.2;
/// A regular whose average sinks below this stops being one.
pub const REGULAR_LAPSE: f32 = 0.0;
/// Weight of a new experience in the running average after the first.
pub const SCORE_LEARN: f32 = 0.35;
/// Round of drinks bonus a regular leaves at the bar (share of the tab).
pub const REGULAR_TIP: f32 = 0.2;

/// The kind of place, by business and by what the building calls itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Vibe {
    DiveBar,
    Club,
    Karaoke,
    Lounge,
    Arcade,
    Cabaret,
    NightMarket,
    Cafe,
    Grocer,
    Boutique,
    Park,
}

impl Vibe {
    pub const COUNT: usize = 11;
    pub const ALL: [Vibe; Self::COUNT] = [Vibe::DiveBar, Vibe::Club, Vibe::Karaoke, Vibe::Lounge, Vibe::Arcade, Vibe::Cabaret, Vibe::NightMarket, Vibe::Cafe, Vibe::Grocer, Vibe::Boutique, Vibe::Park];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn label(self) -> &'static str {
        match self {
            Vibe::DiveBar => "dive bar",
            Vibe::Club => "club",
            Vibe::Karaoke => "karaoke bar",
            Vibe::Lounge => "lounge",
            Vibe::Arcade => "arcade",
            Vibe::Cabaret => "cabaret",
            Vibe::NightMarket => "night market",
            Vibe::Cafe => "café-bakery",
            Vibe::Grocer => "grocer",
            Vibe::Boutique => "gadget boutique",
            Vibe::Park => "park",
        }
    }

    /// How loud the place is by nature, `0..1`.
    fn noise(self) -> f32 {
        match self {
            Vibe::Club => 0.95,
            Vibe::Karaoke => 0.85,
            Vibe::DiveBar => 0.7,
            Vibe::Arcade => 0.75,
            Vibe::NightMarket => 0.6,
            Vibe::Cabaret => 0.55,
            Vibe::Lounge => 0.35,
            Vibe::Cafe => 0.3,
            Vibe::Grocer => 0.25,
            Vibe::Boutique => 0.2,
            Vibe::Park => 0.1,
        }
    }

    /// A night out (venues), rather than an errand or a stroll.
    pub fn is_night_out(self) -> bool {
        matches!(self, Vibe::DiveBar | Vibe::Club | Vibe::Karaoke | Vibe::Lounge | Vibe::Arcade | Vibe::Cabaret | Vibe::NightMarket)
    }
}

/// Is `b` a place people pick and remember: a venue, a shop that serves
/// at the door, or a park (not a service, school or stadium)?
pub fn is_place(b: &Building) -> bool {
    if b.service.is_some() || b.is_power() || b.rail.is_some() || b.port.is_some() {
        return false;
    }
    b.land_use == LandUse::Park || b.business.as_ref().is_some_and(|z| matches!(z.kind, BusinessKind::Venue | BusinessKind::Grocery | BusinessKind::Bakery | BusinessKind::GadgetShop | BusinessKind::Cafe | BusinessKind::Bookshop))
}

/// The vibe of a place: venues by the word in their name and their tier
/// (a cheap bar is a dive, a dear club a lounge), shops by their trade.
pub fn vibe_of(b: &Building) -> Vibe {
    if b.land_use == LandUse::Park {
        return Vibe::Park;
    }
    match b.business.as_ref().map(|z| z.kind) {
        Some(BusinessKind::Grocery) => return Vibe::Grocer,
        Some(BusinessKind::Bakery | BusinessKind::Cafe) => return Vibe::Cafe,
        Some(BusinessKind::GadgetShop | BusinessKind::Bookshop) => return Vibe::Boutique,
        _ => {}
    }
    let n = b.name.as_str();
    let word = |w: &str| n.split_whitespace().any(|x| x == w);
    let by_name = if word("Karaoke") {
        Vibe::Karaoke
    } else if word("Club") {
        Vibe::Club
    } else if word("Lounge") {
        Vibe::Lounge
    } else if word("Arcade") {
        Vibe::Arcade
    } else if word("Cabaret") {
        Vibe::Cabaret
    } else if word("Bar") || word("Den") {
        Vibe::DiveBar
    } else {
        Vibe::NightMarket
    };
    match (b.tier, by_name) {
        // Cheap and low: the shack bars and the night market stalls.
        (Tier::Low, Vibe::Lounge | Vibe::Cabaret) => Vibe::DiveBar,
        (Tier::Low, Vibe::Club) if b.seed % 3 == 0 => Vibe::NightMarket,
        // Nobody calls a high-end place a dive.
        (Tier::High, Vibe::DiveBar) => Vibe::Lounge,
        (Tier::High, Vibe::NightMarket) => Vibe::Cabaret,
        (_, v) => v,
    }
}

/// What a place is like right now. All in `0..1`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaceCharacter {
    pub vibe: Vibe,
    pub tier: Tier,
    /// Shoddy (0) to polished (1): tier and upkeep.
    pub polish: f32,
    /// Cheap (0) to dear (1).
    pub price: f32,
    pub noise: f32,
    /// Empty (0) to packed (1).
    pub crowd: f32,
    /// Class of the people inside: 0 all poor, 1 all rich (0.5 when empty).
    pub crowd_class: f32,
    /// Crime nearby: 1 safe, 0 a hot spot.
    pub safety: f32,
    /// Trash: 1 clean, 0 bins overflowing.
    pub clean: f32,
}

impl PlaceCharacter {
    /// Shoddy, unsafe and dirty all at once, `0..1`.
    pub fn sketchiness(&self) -> f32 {
        ((1.0 - self.polish) * 0.5 + (1.0 - self.safety) * 0.3 + (1.0 - self.clean) * 0.2).clamp(0.0, 1.0)
    }

    /// One or two words for the inspector.
    pub fn summary(&self) -> String {
        let mut words: Vec<&str> = Vec::new();
        if self.sketchiness() > 0.55 {
            words.push("sketchy");
        } else if self.polish < 0.35 {
            words.push("shoddy");
        } else if self.polish > 0.75 {
            words.push("polished");
        }
        if self.price < 0.3 {
            words.push("cheap");
        } else if self.price > 0.7 {
            words.push("pricey");
        }
        if self.noise > 0.75 {
            words.push("loud");
        } else if self.noise < 0.3 {
            words.push("quiet");
        }
        if self.crowd > 0.8 {
            words.push("packed");
        }
        words.push(self.vibe.label());
        words.join(" ")
    }
}

/// Inputs for `character` the building does not carry: the people inside
/// (`count`, mean class index 0..2), crime heat around it and how far its
/// bin is past full (`trash::Bin::overflow`).
#[derive(Clone, Copy, Debug, Default)]
pub struct Surroundings {
    pub visitors: u16,
    pub mean_class: Option<f32>,
    pub heat: f32,
    pub overflow: f32,
}

pub fn character(b: &Building, s: Surroundings) -> PlaceCharacter {
    let vibe = vibe_of(b);
    let tier_polish = match b.tier {
        Tier::Low => 0.2,
        Tier::Mid => 0.55,
        Tier::High => 0.9,
    };
    let polish = (tier_polish * (0.4 + 0.6 * b.condition)).clamp(0.0, 1.0);
    let price = match (vibe, b.tier) {
        (Vibe::Park, _) => 0.0,
        (_, Tier::Low) => 0.15,
        (_, Tier::Mid) => 0.5,
        (_, Tier::High) => 0.9,
    };
    let cap = b.customer_capacity();
    let crowd = if vibe == Vibe::Park {
        (s.visitors as f32 / 30.0).min(1.0)
    } else if cap == u16::MAX || cap == 0 {
        0.0
    } else {
        (s.visitors as f32 / cap as f32).min(1.0)
    };
    // A packed room is louder than an empty one.
    let noise = (vibe.noise() * (0.75 + 0.35 * crowd)).clamp(0.0, 1.0);
    let crowd_class = s.mean_class.map_or(0.5, |m| (m / 2.0).clamp(0.0, 1.0));
    // A cheap block feels rougher even before anything happens.
    let base_safety = match b.tier {
        Tier::Low => 0.8,
        Tier::Mid => 0.92,
        Tier::High => 1.0,
    };
    let safety = (base_safety - (s.heat / crate::crime::FEAR_CAP).min(1.0) * 0.7).clamp(0.0, 1.0);
    let clean = (1.0 - s.overflow.min(1.0) * 0.8 - (1.0 - b.condition) * 0.3).clamp(0.0, 1.0);
    PlaceCharacter { vibe, tier: b.tier, polish, price, noise, crowd, crowd_class, safety, clean }
}

/// The five personality traits, `[sociability, ambition, hedonism,
/// caution, curiosity]`, drawn from the seed exactly as
/// `nd_minds::Personality::from_seed` does.
pub fn traits(seed: u64) -> [f32; 5] {
    let mut rng = crate::SimRng::seed_from_u64(seed);
    [rng.random(), rng.random(), rng.random(), rng.random(), rng.random()]
}

/// What a citizen likes in a place. The likings run `-1..1` (negative is
/// a dislike), the needs `0..1.5` (how much a shortfall hurts).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Taste {
    pub loud: f32,
    pub crowded: f32,
    /// Likes cheap (positive) or wants dear (negative).
    pub cheap: f32,
    /// Liking for an upscale crowd.
    pub posh: f32,
    pub polish: f32,
    pub safety: f32,
    pub clean: f32,
    /// Chance to try somewhere new rather than a favourite, `0..1`.
    pub novelty: f32,
    pub sociability: f32,
    /// A personal leaning towards each `Vibe`.
    pub vibes: [f32; Vibe::COUNT],
}

/// Taste from personality, age, class and money to spend (`budget`: the
/// pocket plus a share of the household purse).
pub fn taste(seed: u64, age: u8, class: Tier, budget: f32) -> Taste {
    let [sociability, _ambition, hedonism, caution, curiosity] = traits(seed);
    let young = ((40.0 - age as f32) / 22.0).clamp(0.0, 1.0);
    let old = ((age as f32 - 50.0) / 25.0).clamp(0.0, 1.0);
    let (rich, poor) = match class {
        Tier::High => (1.0, 0.0),
        Tier::Mid => (0.3, 0.0),
        Tier::Low => (0.0, 1.0),
    };
    let skint = if budget < 40.0 { 0.3 } else { 0.0 };
    let loud = 0.9 * hedonism + 0.45 * young + 0.3 * sociability - 0.9 * old - 0.4 * caution - 0.25 * rich - 0.3;
    let crowded = 0.8 * sociability + 0.3 * hedonism + 0.3 * young - 0.6 * old - 0.2 * caution - 0.35;
    let cheap = 0.6 * poor - 0.7 * rich + 0.1 + 0.25 * young - 0.2 * old + skint;
    let posh = 0.6 * rich - 0.25 * poor + 0.1 * old;
    let polish = 0.15 + 0.9 * rich + 0.6 * old + 0.3 * caution - 0.45 * hedonism * young;
    let safety = 0.25 + 0.9 * caution + 0.45 * old + 0.3 * rich - 0.3 * hedonism;
    let clean = 0.2 + 0.45 * old + 0.35 * rich + 0.2 * caution;
    let novelty = 0.03 + 0.5 * curiosity + 0.12 * young + 0.08 * hedonism - 0.1 * old;
    // A little personal quirk per vibe, then the obvious leanings.
    let mut rng = crate::rng_for(seed, 0x7A57E);
    let mut vibes = [0.0f32; Vibe::COUNT];
    for v in vibes.iter_mut() {
        *v = rng.random_range(-0.12..0.12);
    }
    vibes[Vibe::Club.index()] += 0.35 * young * hedonism - 0.3 * old;
    vibes[Vibe::Karaoke.index()] += 0.25 * sociability - 0.1 * caution;
    vibes[Vibe::DiveBar.index()] += 0.25 * hedonism - 0.2 * rich;
    vibes[Vibe::Lounge.index()] += 0.2 * old + 0.2 * rich;
    vibes[Vibe::Arcade.index()] += 0.25 * young - 0.15 * old;
    vibes[Vibe::NightMarket.index()] += 0.2 * curiosity;
    vibes[Vibe::Cafe.index()] += 0.15 * old;
    vibes[Vibe::Park.index()] += 0.15 * caution + 0.15 * old;
    Taste {
        loud: loud.clamp(-1.0, 1.0),
        crowded: crowded.clamp(-1.0, 1.0),
        cheap: cheap.clamp(-1.0, 1.0),
        posh: posh.clamp(-1.0, 1.0),
        polish: polish.clamp(-0.3, 1.5),
        safety: safety.clamp(0.0, 1.5),
        clean: clean.clamp(0.0, 1.5),
        novelty: novelty.clamp(0.03, 0.8),
        sociability,
        vibes,
    }
}

/// How well a place suits a taste, about `-1..1`.
pub fn fit(t: &Taste, ch: &PlaceCharacter) -> f32 {
    let mut f = 0.0;
    f += 0.35 * t.loud * (ch.noise - 0.5) * 2.0;
    f += 0.2 * t.crowded * (ch.crowd - 0.4) * 2.0;
    if ch.vibe != Vibe::Park {
        f += 0.25 * t.cheap * (0.5 - ch.price) * 2.0;
    }
    if ch.crowd > 0.05 {
        f += 0.15 * t.posh * (ch.crowd_class - 0.5) * 2.0;
    }
    f -= 0.55 * t.polish * (1.0 - ch.polish);
    f -= 0.6 * t.safety * (1.0 - ch.safety);
    f -= 0.35 * t.clean * (1.0 - ch.clean);
    f += t.vibes[ch.vibe.index()];
    // Baseline: a place that is merely fine is a mild pleasure.
    (f + 0.25).clamp(-1.0, 1.0)
}

/// Something that happened on a visit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VisitEvent {
    Fight,
    RudeStaff,
    Overpriced,
    TooCrowded,
    GreatMusic,
    GreatFood,
    MetFriend,
    WithCompany,
}

impl VisitEvent {
    pub fn effect(self) -> f32 {
        match self {
            VisitEvent::Fight => -0.75,
            VisitEvent::RudeStaff => -0.35,
            VisitEvent::Overpriced => -0.3,
            VisitEvent::TooCrowded => -0.25,
            VisitEvent::GreatMusic => 0.45,
            VisitEvent::GreatFood => 0.35,
            VisitEvent::MetFriend => 0.3,
            VisitEvent::WithCompany => 0.2,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            VisitEvent::Fight => "fight",
            VisitEvent::RudeStaff => "rude staff",
            VisitEvent::Overpriced => "overpriced",
            VisitEvent::TooCrowded => "too crowded",
            VisitEvent::GreatMusic => "great music",
            VisitEvent::GreatFood => "great food",
            VisitEvent::MetFriend => "met a friend",
            VisitEvent::WithCompany => "good company",
        }
    }
}

/// One visit, rated: the fit, the events and a little luck.
#[derive(Clone, Debug, PartialEq)]
pub struct Rating {
    pub experience: f32,
    pub events: Vec<VisitEvent>,
}

/// Rate a visit. `friends` is how many people the citizen knows who are
/// there (household, coworkers, acquaintances); `came_with` whether they
/// came with someone.
pub fn rate(t: &Taste, ch: &PlaceCharacter, friends: usize, came_with: bool, rng: &mut impl Rng) -> Rating {
    let mut events = Vec::new();
    let night = ch.vibe.is_night_out();
    let park = ch.vibe == Vibe::Park;
    // Errands are lighter experiences than nights out.
    let weight = if night { 1.0 } else if park { 0.7 } else { 0.5 };
    let roll = |rng: &mut dyn rand::RngCore, p: f32| -> bool { rng.random::<f32>() < p.clamp(0.0, 1.0) };
    let rowdy = if matches!(ch.vibe, Vibe::DiveBar | Vibe::Club) { 0.02 } else { 0.0 };
    let fight = if park { 0.4 } else { 1.0 } * (0.008 + 0.12 * (1.0 - ch.safety) + 0.025 * (1.0 - ch.polish) + rowdy);
    if (night || park) && roll(rng, fight) {
        events.push(VisitEvent::Fight);
    }
    if !park && roll(rng, 0.02 + 0.08 * (1.0 - ch.polish)) {
        events.push(VisitEvent::RudeStaff);
    }
    if !park && t.cheap > 0.2 && roll(rng, 0.45 * (ch.price - 0.4).max(0.0) * t.cheap) {
        events.push(VisitEvent::Overpriced);
    }
    if ch.crowd > 0.85 && t.crowded < 0.0 && roll(rng, 0.6) {
        events.push(VisitEvent::TooCrowded);
    }
    if night && roll(rng, 0.06 + 0.1 * ch.polish + 0.05 * t.loud.max(0.0)) {
        events.push(VisitEvent::GreatMusic);
    }
    if matches!(ch.vibe, Vibe::Cafe | Vibe::NightMarket) && roll(rng, 0.1) {
        events.push(VisitEvent::GreatFood);
    }
    if friends > 0 && roll(rng, 0.5 + 0.4 * t.sociability) {
        events.push(VisitEvent::MetFriend);
    }
    if came_with {
        events.push(VisitEvent::WithCompany);
    }
    let luck = (rng.random::<f32>() + rng.random::<f32>() - 1.0) * 0.3;
    let base = fit(t, ch);
    let ev: f32 = events.iter().map(|e| e.effect()).sum();
    let experience = (base + weight * ev + luck).clamp(-1.0, 1.0);
    Rating { experience, events }
}

/// What a citizen remembers of one place.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaceMemory {
    pub place: BuildingId,
    pub land_use: LandUse,
    pub visits: u16,
    pub last_visit: u64,
    /// Running average of the experiences there, `-1..1`.
    pub score: f32,
    pub best: f32,
    pub worst: f32,
    pub regular: bool,
    /// Never going back.
    pub never: bool,
}

impl PlaceMemory {
    /// How much the memory is worth keeping: strong feelings and many
    /// visits stay, faint and old ones go. A grudge is kept.
    pub fn value(&self, now: u64) -> f32 {
        let days = now.saturating_sub(self.last_visit) as f32 / 1440.0;
        let grudge = if self.never { 0.6 } else { 0.0 };
        let habit = if self.regular { 0.5 } else { 0.0 };
        self.score.max(0.0) + self.score.abs() * 0.3 + 0.12 * (1.0 + self.visits as f32).ln() + grudge + habit - 0.03 * days
    }
}

/// Someone met at a regular haunt.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Acquaintance {
    pub who: CitizenId,
    pub place: BuildingId,
    pub times: u16,
    pub last: u64,
}

/// What changed in the book after a visit, for the memory stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Nothing notable.
    Ordinary,
    /// A great first visit.
    Found,
    /// Now a regular here.
    BecameRegular,
    /// Never going back.
    NeverAgain,
    /// Was a regular, has gone off it.
    Lapsed,
}

/// A citizen's places and the people met at them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaceBook {
    pub places: Vec<PlaceMemory>,
    pub acquaintances: Vec<Acquaintance>,
}

impl PlaceBook {
    pub fn get(&self, b: BuildingId) -> Option<&PlaceMemory> {
        self.places.iter().find(|p| p.place == b)
    }

    pub fn is_never(&self, b: BuildingId) -> bool {
        self.get(b).is_some_and(|p| p.never)
    }

    /// The best-loved regular spot of a land use, if any.
    pub fn regular_of(&self, lu: LandUse) -> Option<&PlaceMemory> {
        self.places.iter().filter(|p| p.land_use == lu && p.regular && !p.never).max_by(|a, b| a.score.total_cmp(&b.score))
    }

    /// Places of a land use known and not sworn off.
    pub fn known(&self, lu: LandUse) -> usize {
        self.places.iter().filter(|p| p.land_use == lu && !p.never).count()
    }

    /// Fold a rated visit in and say what it meant.
    pub fn record(&mut self, b: BuildingId, lu: LandUse, experience: f32, now: u64) -> Verdict {
        let i = match self.places.iter().position(|p| p.place == b) {
            Some(i) => i,
            None => {
                if self.places.len() >= MAX_PLACES {
                    let worst = (0..self.places.len()).min_by(|&x, &y| self.places[x].value(now).total_cmp(&self.places[y].value(now))).unwrap();
                    self.places.remove(worst);
                }
                self.places.push(PlaceMemory { place: b, land_use: lu, visits: 0, last_visit: now, score: 0.0, best: experience, worst: experience, regular: false, never: false });
                self.places.len() - 1
            }
        };
        let p = &mut self.places[i];
        let first = p.visits == 0;
        p.score = if first { experience } else { p.score + (experience - p.score) * SCORE_LEARN };
        p.visits = p.visits.saturating_add(1);
        p.last_visit = now;
        p.best = p.best.max(experience);
        p.worst = p.worst.min(experience);
        // A terrible night ends it, unless it is a habit; then it takes a
        // sour average.
        if (experience <= NEVER_AGAIN && !p.regular) || p.score <= NEVER_AGAIN * 0.6 {
            let was = p.never;
            p.never = true;
            p.regular = false;
            return if was { Verdict::Ordinary } else { Verdict::NeverAgain };
        }
        if p.regular && p.score < REGULAR_LAPSE {
            p.regular = false;
            return Verdict::Lapsed;
        }
        if !p.regular && p.visits >= REGULAR_VISITS && p.score >= REGULAR_SCORE {
            p.regular = true;
            return Verdict::BecameRegular;
        }
        if first && experience >= GREAT {
            return Verdict::Found;
        }
        Verdict::Ordinary
    }

    /// Note meeting `who` at `place`.
    pub fn meet(&mut self, who: CitizenId, place: BuildingId, now: u64) {
        if let Some(a) = self.acquaintances.iter_mut().find(|a| a.who == who) {
            a.times = a.times.saturating_add(1);
            a.last = now;
            a.place = place;
            return;
        }
        if self.acquaintances.len() >= MAX_ACQUAINTANCES {
            let k = (0..self.acquaintances.len()).min_by_key(|&k| (self.acquaintances[k].times, self.acquaintances[k].last)).unwrap();
            self.acquaintances.remove(k);
        }
        self.acquaintances.push(Acquaintance { who, place, times: 1, last: now });
    }

    pub fn knows(&self, who: CitizenId) -> bool {
        self.acquaintances.iter().any(|a| a.who == who)
    }

    /// Forget places that are gone.
    pub fn retain_places(&mut self, keep: impl Fn(BuildingId) -> bool) {
        self.places.retain(|p| keep(p.place));
        self.acquaintances.retain(|a| keep(a.place));
    }
}

/// A venue's (or shop's, or park's) name about town.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Reputation {
    /// Running average of everyone's experiences, `-1..1`.
    pub rating: f32,
    /// Visits rated, ever.
    pub rated: u32,
    pub visits_today: u32,
    pub regular_visits_today: u32,
    /// Revenue from regulars today (cover, rounds and tips).
    pub regular_revenue_today: f32,
    pub visits_yesterday: u32,
    pub regular_revenue_yesterday: f32,
    /// Counted from everyone's memories at midnight.
    pub regulars: u32,
    pub never_again: u32,
    /// Notable nights ever: great ones and fights.
    pub great_nights: u32,
    pub fights: u32,
}

impl Reputation {
    pub fn add(&mut self, experience: f32) {
        self.rated += 1;
        let k = (1.0 / self.rated as f32).max(0.08);
        self.rating += (experience - self.rating) * k;
    }

    /// The pull of word of mouth on someone who has never been: a good
    /// rating with enough behind it draws, sworn-off customers repel.
    pub fn pull(&self) -> f32 {
        let confidence = (self.rated as f32 / 20.0).min(1.0);
        let bad_name = (self.never_again as f32 / (self.regulars as f32 + 4.0)).min(1.5);
        0.35 * self.rating * confidence - 0.2 * bad_name
    }

    /// 1..5 stars for people.
    pub fn stars(&self) -> f32 {
        (3.0 + 2.0 * self.rating).clamp(1.0, 5.0)
    }
}

/// Every place's reputation, saved with the world.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Places {
    pub reputation: BTreeMap<BuildingId, Reputation>,
    /// Visits made together (household or coworkers) ever, and today.
    pub outings_together_total: u32,
    pub outings_together_today: u32,
    pub explorations_total: u32,
    pub never_again_total: u32,
    pub regulars_made_total: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zoning::LotId;

    fn venue(name: &str, tier: Tier, condition: f32) -> Building {
        let mut b = Building::new(LotId::default(), LandUse::Entertainment, 2, tier, 5, name.to_string(), 0, 0.3);
        b.business = Some(crate::market::Business::new(BusinessKind::Venue, 4));
        b.jobs = 4;
        b.condition = condition;
        b
    }

    #[test]
    fn vibes_come_from_the_name_and_tier() {
        assert_eq!(vibe_of(&venue("Rust Bucket Bar", Tier::Low, 0.5)), Vibe::DiveBar);
        assert_eq!(vibe_of(&venue("Velvet Lounge", Tier::High, 1.0)), Vibe::Lounge);
        assert_eq!(vibe_of(&venue("Karaoke Siren", Tier::Mid, 1.0)), Vibe::Karaoke);
        assert_eq!(vibe_of(&venue("Halo Bar", Tier::High, 1.0)), Vibe::Lounge, "no high-end dives");
    }

    #[test]
    fn a_rich_old_taste_hates_a_sketchy_dive_a_young_hedonist_does_not() {
        let dive = character(&venue("Rust Bucket Bar", Tier::Low, 0.45), Surroundings { visitors: 20, mean_class: Some(0.2), heat: 2.5, overflow: 0.6 });
        assert!(dive.sketchiness() > 0.5, "{dive:?}");
        // Find a seed of each kind.
        let wild = (0..2000u64).find(|&s| { let t = traits(s); t[2] > 0.85 && t[3] < 0.3 && t[0] > 0.5 }).unwrap();
        let staid = (0..2000u64).find(|&s| { let t = traits(s); t[2] < 0.3 && t[3] > 0.6 }).unwrap();
        let young = taste(wild, 22, Tier::Low, 60.0);
        let old = taste(staid, 72, Tier::High, 500.0);
        assert!(fit(&young, &dive) > 0.2, "young hedonist: {}", fit(&young, &dive));
        assert!(fit(&old, &dive) < -0.4, "rich and old: {}", fit(&old, &dive));
    }

    #[test]
    fn the_book_makes_regulars_swears_off_and_forgets() {
        let mut book = PlaceBook::default();
        let mut ids: slotmap::SlotMap<BuildingId, ()> = slotmap::SlotMap::with_key();
        let a = ids.insert(());
        assert_eq!(book.record(a, LandUse::Entertainment, 0.7, 0), Verdict::Found);
        assert_eq!(book.record(a, LandUse::Entertainment, 0.4, 10), Verdict::Ordinary);
        assert_eq!(book.record(a, LandUse::Entertainment, 0.5, 20), Verdict::BecameRegular);
        assert!(book.regular_of(LandUse::Entertainment).is_some());
        // One bad night does not end a habit.
        assert_eq!(book.record(a, LandUse::Entertainment, -0.6, 30), Verdict::Ordinary);
        assert!(!book.is_never(a));
        let b = ids.insert(());
        assert_eq!(book.record(b, LandUse::Entertainment, -0.8, 40), Verdict::NeverAgain);
        assert!(book.is_never(b));
        for _ in 0..20 {
            let x = ids.insert(());
            book.record(x, LandUse::Commercial, 0.0, 50);
        }
        assert_eq!(book.places.len(), MAX_PLACES);
        assert!(book.get(a).is_some() && book.get(b).is_some(), "the habit and the grudge are kept");
    }
}
