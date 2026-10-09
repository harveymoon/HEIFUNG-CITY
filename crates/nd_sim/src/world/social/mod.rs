//! [social] People talking: who meets whom, how it goes, and what comes of
//! it (`crate::social` holds the data and the pure rules).
//!
//! - **Encounters** (`step_social`, every `ENCOUNTER_EVERY` ticks): everyone
//!   out and about is put in a spot: a bar or club, a stadium, a bench, a
//!   park, a workplace, a friend's home, a bus or train, a bus stop, a few
//!   metres of pavement. Within each spot a few pairs are drawn (a hash of
//!   the tick and the spot) and may get talking, more readily where people
//!   talk (bars, benches, a friend's sofa) and the more sociable they are;
//!   at most `TALK_BUDGET` conversations a round, so the cost is bounded
//!   whatever the city's size and never O(n²).
//! - **Conversations** (`converse`): a topic from their lives (work, rent,
//!   the city's concern of the week, the news, the game, a place they both
//!   know, a third person, a shared passion), and an outcome from their
//!   compatibility, how they already feel about each other, their moods and
//!   how touchy the subject is: agreement, laughter, flirting, a favour, an
//!   argument, insults, now and then (enemies, drink) a fight that the
//!   police are called to. Both ties move; notable turns are remembered.
//! - **What it changes** (hourly at :25, daily at midnight): friends meet
//!   up at a place they both like (one calls round and they walk there
//!   together), visit each other at home, go to a friend's band's gig;
//!   enemies steer clear of each other's places (`social_pull`, read by
//!   `choose_place`); friends find each other jobs and flats, lend money,
//!   partners move in together; gossip sours people on a third person and
//!   on places.
//! - **Groups**: friends with a shared passion form a band, a sports club,
//!   a book club, a street-art crew, a startup, a garden group or a
//!   neighbourhood association. Members, meetings and momentum; at a
//!   threshold they make something: a gig, a match, a reading, a mural, a
//!   garden on a vacant lot, a business, a petition to the player (and a
//!   protest when it is ignored). Rivalries inside split them.
//!
//! Every roll is a hash of who, when and a salt: the world's random stream
//! is never drawn here, so the same seed makes the same friends.

use super::*;
use crate::services::ServiceKind;
use crate::social::*;

mod encounters;
mod talk;
mod outings;
mod ties;
mod groups;
mod works;
mod petitions;

/// Ticks between encounter rounds.
pub const ENCOUNTER_EVERY: u64 = 3;
/// Conversations a round at most.
pub const TALK_BUDGET: usize = 48;
/// Pairs looked at per spot per round.
const PAIRS_PER_SPOT: usize = 3;
/// Pavement cell for passing in the street, metres.
const STREET_CELL: f32 = 4.0;
/// Friends call round for each other within this, metres.
const PICKUP_REACH: f32 = 700.0;
/// A planned outing is given up after this.
const OUTING_TTL: u64 = 150;
/// Walking together lasts at most this.
const WALK_TOGETHER: u64 = 150;

fn key64<K: slotmap::Key>(k: K) -> u64 {
    k.data().as_ffi()
}

fn cell(p: Vec2, size: f32) -> u64 {
    let x = (p.x / size).floor() as i32 as u32 as u64;
    let y = (p.y / size).floor() as i32 as u32 as u64;
    (x << 32) | y
}

fn setting_code(s: Setting) -> u8 {
    match s {
        Setting::Venue => 0,
        Setting::Stadium => 1,
        Setting::Bench => 2,
        Setting::Park => 3,
        Setting::Work => 4,
        Setting::Home => 5,
        Setting::Transit => 6,
        Setting::Stop => 7,
        Setting::Street => 8,
    }
}

fn pair_key(a: u64, b: u64) -> u64 {
    let (lo, hi) = if a < b { (a, b) } else { (b, a) };
    lo ^ hi.rotate_left(29) ^ 0x5_0C1A
}

/// Two people of an age to talk: adults with anyone grown, teenagers with
/// those near their own age; no small children.
fn may_talk(a: &Citizen, b: &Citizen) -> bool {
    if a.age < 12 || b.age < 12 {
        return false;
    }
    if a.age < 18 || b.age < 18 {
        return (a.age as i32 - b.age as i32).abs() <= 4;
    }
    true
}

fn may_court(a: &Citizen, b: &Citizen) -> bool {
    a.age >= 18 && b.age >= 18 && (a.age as i32 - b.age as i32).abs() <= 12
}

fn surname(name: &str) -> &str {
    name.split_whitespace().last().unwrap_or(name)
}

fn pick<'a>(list: &'a [&'a str], h: u64) -> &'a str {
    list[(h % list.len() as u64) as usize]
}

fn mix(a: u64, b: u64) -> u64 {
    let mut z = a ^ b.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// What a spot is: the setting and the building (if any).
#[derive(Clone, Copy)]
pub(super) struct Spot {
    pub(super) setting: Setting,
    pub(super) place: Option<BuildingId>,
}

impl World {
    // ── the tick ───────────────────────────────────────────────────────────

    /// Every tick: outings under way; every few, a round of encounters.
    pub(super) fn step_social(&mut self) {
        if !self.config.social {
            return;
        }
        let tick = self.clock.tick;
        if !self.social.outings.is_empty() {
            self.step_outings();
        }
        if !self.social.walking_with.is_empty() && tick % 10 == 0 {
            let citizens = &self.citizens;
            self.social.walking_with.retain(|id, (_, until)| tick < *until && citizens.get(*id).is_some_and(|c| c.activity.is_traveling()));
        }
        if tick % ENCOUNTER_EVERY == 1 {
            crate::prof!("social_encounters", self.encounters());
        }
    }

    // ── what ties change ───────────────────────────────────────────────────

    /// Hourly at :25: friends plan outings and call on each other, groups
    /// meet, bands play, neighbourhoods protest.
    pub(super) fn hourly_social(&mut self) {
        if !self.config.social {
            return;
        }
        crate::prof_scope!("hourly_social");
        self.social_concern = self.concern_of_the_week();
        let hour = self.clock.hour();
        if (11..=22).contains(&hour) {
            self.plan_outings();
        }
        if hour == 18 {
            self.group_meetings();
        }
        if hour == 22 {
            self.tally_meetings();
        }
        if hour == 19 {
            self.start_gigs();
        }
        if hour == 11 {
            self.start_protests();
        }
        if (20..=23).contains(&hour) {
            self.count_gig_crowds();
        }
        let tick = self.clock.tick;
        self.social.gigs.retain(|g| g.until + 1440 > tick);
    }

    // ── midnight ───────────────────────────────────────────────────────────

    pub(super) fn daily_social(&mut self) {
        if !self.config.social {
            return;
        }
        crate::prof_scope!("daily_social");
        self.prune_social();
        self.fade_ties();
        self.friends_help();
        self.loans();
        self.move_in_together();
        self.daily_groups();
        self.check_petitions();
        self.social.today.avoided = self.social.avoid_hits.take();
        let day = self.clock.day();
        self.social.roll_day(day);
    }

    /// Citizens gone: their circles and ties to them go too.
    fn prune_social(&mut self) {
        let citizens = &self.citizens;
        self.social.circles.retain(|id, _| citizens.contains_key(*id));
        for c in self.social.circles.values_mut() {
            c.ties.retain(|t| citizens.contains_key(t.who));
            c.talks.retain(|t| citizens.contains_key(t.with));
        }
        self.social.walking_with.retain(|a, (b, _)| citizens.contains_key(*a) && citizens.contains_key(*b));
        self.social.outings.retain(|o| citizens.contains_key(o.caller) && citizens.contains_key(o.friend));
        let day = self.clock.day();
        for g in self.social.groups.iter_mut() {
            g.members.retain(|m| citizens.contains_key(*m));
        }
        self.social.groups.retain(|g| g.folded.is_none_or(|d| day < d + 20));
        let groups: std::collections::BTreeSet<u32> = self.social.groups.iter().filter(|g| g.active()).map(|g| g.id).collect();
        for c in self.social.circles.values_mut() {
            c.groups.retain(|g| groups.contains(g));
        }
        self.social.circles.retain(|_, c| !c.is_empty());
    }

    // ── groups ─────────────────────────────────────────────────────────────

    /// For tests: a tie between two people as `a` sees it.
    #[cfg(test)]
    pub(crate) fn tie_between(&self, a: CitizenId, b: CitizenId) -> Option<&Tie> {
        self.social.circles.get(&a)?.tie(b)
    }
}

/// A few words of what was said.
fn talk_line(topic: Topic, outcome: Outcome, place: Option<&str>, about: Option<&str>, h: u64) -> String {
    let place = place.unwrap_or("that place");
    let about = about.unwrap_or("them");
    let good = outcome.is_good();
    let pickn = |xs: &[&str]| xs[(mix(h, 0x11E) % xs.len() as u64) as usize].to_string();
    match outcome {
        Outcome::Flirted => return pickn(&["Can I buy you a drink?", "You look nice tonight.", "Same time tomorrow?"]),
        Outcome::Favour => return pickn(&["I'll put a word in for you.", "Let me help with that.", "I know someone who's hiring."]),
        Outcome::Fight => return pickn(&["Say that again!", "Outside. Now.", "You want some?"]),
        Outcome::Insulted => return pickn(&["Get lost.", "Who asked you?", "You're a joke."]),
        _ => {}
    }
    match (topic, good) {
        (Topic::Work, true) => pickn(&["Work's been all right.", "Long shift, but fine.", "The boss finally said thanks."]),
        (Topic::Work, false) => pickn(&["You call that a job?", "Your lot never work.", "Easy for you to say."]),
        (Topic::Rent, true) => pickn(&["Rent's a joke round here.", "They put it up again.", "Can't afford this place."]),
        (Topic::Rent, false) => pickn(&["Move somewhere cheaper, then.", "Rents are fair.", "Stop moaning."]),
        (Topic::City(c), true) => format!("Somebody should fix {}.", Topic::City(c).label()),
        (Topic::City(c), false) => format!("{}? That's nonsense.", capital(Topic::City(c).label())),
        (Topic::News, true) => pickn(&["Did you hear what happened?", "Crazy night, wasn't it?", "Saw it on the news."]),
        (Topic::News, false) => pickn(&["You believe everything.", "That's not what happened.", "Fake news."]),
        (Topic::Sports, true) => pickn(&["What a game!", "We'll win the next one.", "Did you see that goal?"]),
        (Topic::Sports, false) => pickn(&["Your team is rubbish.", "The ref was bought.", "Worst game ever."]),
        (Topic::Place, true) => format!("{place} is the best."),
        (Topic::Place, false) => format!("{place}? Never again."),
        (Topic::Gossip, true) => format!("Heard about {about}?"),
        (Topic::Gossip, false) => format!("Leave {about} out of it."),
        (Topic::Music, true) => pickn(&["Heard the new stuff?", "We should start a band.", "That track's on repeat."]),
        (Topic::Music, false) => pickn(&["That's not music.", "You've got no taste."]),
        (Topic::Art, true) => pickn(&["Seen the new mural?", "That gallery's brilliant."]),
        (Topic::Art, false) => pickn(&["It's just graffiti.", "Art's a scam."]),
        (Topic::Books, true) => pickn(&["Finished that book yet?", "You have to read this."]),
        (Topic::Books, false) => pickn(&["Books are boring.", "Never read it, never will."]),
        (Topic::Weather, true) => pickn(&["Nice out, isn't it?", "Bit muggy today.", "Rain later, they say."]),
        (Topic::Weather, false) => pickn(&["Wrong, as usual.", "Who cares about the weather?"]),
        (Topic::Family, true) => pickn(&["How are the kids?", "Family's well, thanks."]),
        (Topic::Family, false) => pickn(&["Mind your own family.", "None of your business."]),
    }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().chain(c).collect())
}

/// [murder] Does this fight between enemies (pair `salt`, at `tick`) end in
/// a death? `FEUD_KILL_CHANCE` of them, by a hash: no draws.
pub(crate) fn feud_turns_deadly(salt: u64, tick: u64) -> bool {
    hash01(salt, tick, 0x4D_0D3E) < crate::crime::FEUD_KILL_CHANCE
}
