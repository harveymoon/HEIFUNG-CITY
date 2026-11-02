//! [talk] The mayor talks to a citizen (the person pane's Talk, the remote
//! `talk` command), through the committee's backend: the same settings,
//! key and courier, the offline stand-in by default.
//!
//! The citizen answers in character from a [`TalkCard`]: a snapshot of
//! their real state (who they are, mood, money, hunger, home and work, the
//! commute and why they travel the way they do, memories, places, people,
//! what they are doing now). The prompt tells the model they know only
//! that, in a fictional city.
//!
//! UI-side and deterministic-safe: the chat lives in `World::talks`
//! (display only, bounded, saved with the city) and nothing in the sim
//! reads it. The one deliberate touch is an optional memory ("Talked to
//! the mayor about the buses."), at most one a game day per person, made
//! from the words said (no RNG).
//!
//! Cost: one request per line the mayor says, a short reply (word limit
//! in the prompt, `talk_max_tokens` cap), and a daily budget of model
//! messages (`talk_daily_budget`, counted in `talk_usage.ron`); past it,
//! or with no key, the stand-in answers from the same card.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use nd_sim::committee::mix;
use nd_sim::talk::TalkLine;
use nd_sim::{CitizenId, World};
use serde::{Deserialize, Serialize};

use super::settings::CommitteeSettings;
use super::{Courier, Delivered, MockCommittee, Task, TurnRequest};
use crate::Personality;

/// Characters the mayor may say in one line.
pub const MAX_SAID_CHARS: usize = 300;
/// Earlier lines of the chat put in the prompt.
pub const HISTORY_IN_PROMPT: usize = 8;
/// Memories put in the prompt (newest).
pub const MEMORIES_IN_PROMPT: usize = 10;
/// Chats waiting on a model at once (each person one at a time).
pub const MAX_IN_FLIGHT: usize = 3;
/// Words kept of a reply.
pub const REPLY_MAX_WORDS: usize = 90;
/// The file counting today's model messages, in the game's home folder.
pub const USAGE_FILE: &str = "talk_usage.ron";
/// How a chat's memory starts (one a game day per person).
pub const MEMORY_PREFIX: &str = "Talked to the mayor about";

/// A citizen as the chat's prompt needs them: their real state, in words.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TalkCard {
    pub name: String,
    pub age: u8,
    pub stage: String,
    pub class: String,
    pub archetype: String,
    pub personality: String,
    pub mood: f32,
    pub satiety: f32,
    pub hungry_days: u8,
    pub meals_at_home: f32,
    pub income: f32,
    pub rent: f32,
    pub savings: f32,
    pub rent_stress_days: u8,
    pub home: Option<String>,
    pub work: Option<String>,
    pub household: usize,
    pub doing: String,
    pub thought: String,
    pub owns_car: bool,
    pub owns_bike: bool,
    /// Their commute in words (empty without a home and a job).
    pub commute: Vec<String>,
    /// The mode they would pick for work now, and the reason no bus.
    pub commute_best: Option<String>,
    pub no_bus: Option<String>,
    /// How travelling has felt lately, per mode used ("walking: painful").
    pub travel_pain: Vec<String>,
    pub memories: Vec<String>,
    pub favourites: Vec<String>,
    pub never_again: Vec<String>,
    pub people: Vec<String>,
    pub acquaintances: Vec<String>,
    /// "day 4, 14:20".
    pub now: String,
    /// [r7-citizen-story] What has just gone wrong or is wrong now, newest
    /// first: the car towed, clamped or ticketed, no home, no job, hunger,
    /// the rent. They react to these.
    #[serde(default)]
    pub troubles: Vec<String>,
    /// [r7-citizen-story] Where the car is, in words.
    #[serde(default)]
    pub car: Option<String>,
    /// [r7-citizen-story] Memories of today and yesterday that matter (a
    /// tow, an eviction, a new job), with when, oldest first, even when the
    /// last `MEMORIES_IN_PROMPT` are all benches and walks.
    #[serde(default)]
    pub notable: Vec<String>,
}

/// Words for `NoBus::key`.
pub fn no_bus_words(key: &str) -> &'static str {
    match key {
        "too_short" => "work is close enough to walk",
        "no_line" => "no bus line runs in the city",
        "no_stop_either_end" => "there is no bus stop within a walk of home or of work",
        "no_stop_at_start" => "there is no bus stop within a walk of home",
        "no_stop_at_end" => "there is no bus stop within a walk of work",
        "no_line_serves_both" => "the buses near home do not go near work, and there is no easy change",
        "long_way_round" => "the bus goes the long way round, no quicker than walking",
        _ => "no bus suits the trip",
    }
}

fn mode_words(key: &str) -> &'static str {
    match key {
        "walk" => "walking",
        "bike" => "cycling",
        "bus" => "the bus",
        "train" => "the train",
        "car" => "driving",
        "subway" => "the metro",
        "taxi" => "a taxi",
        _ => "travelling",
    }
}

fn pain_words(p: f32) -> &'static str {
    if p >= 1.5 {
        "miserable"
    } else if p >= 0.7 {
        "painful"
    } else if p >= 0.25 {
        "tiresome"
    } else {
        "fine"
    }
}

fn hhmm(tick: u64) -> String {
    let m = tick % 1440;
    format!("{:02}:{:02}", m / 60, m % 60)
}

impl TalkCard {
    pub fn of(w: &World, id: CitizenId) -> Option<TalkCard> {
        let c = w.citizens.get(id)?;
        let p = Personality::from_seed(c.seed);
        let bname = |b: nd_sim::BuildingId| w.buildings.get(b).map(|b| b.name.clone());
        let cname = |x: CitizenId| w.citizens.get(x).map(|c| c.name.clone());
        let rent = c.home.and_then(|h| w.buildings.get(h)).map_or(0.0, |b| b.rent);
        let household = c.household.and_then(|h| w.households.get(h)).map_or(0, |h| h.members.len().saturating_sub(1));
        let mut card = TalkCard {
            name: c.name.clone(),
            age: c.age,
            stage: c.stage().label().to_string(),
            class: c.class().label().to_string(),
            archetype: p.archetype.to_string(),
            personality: p.summary(),
            mood: c.mood,
            satiety: c.satiety,
            hungry_days: c.hungry_days,
            meals_at_home: c.groceries,
            income: c.income,
            rent,
            savings: c.savings,
            rent_stress_days: c.rent_stress_days,
            home: c.home.and_then(bname),
            work: c.work.and_then(bname),
            household,
            doing: w.activity_line(id),
            thought: c.thought.clone(),
            owns_car: c.owns_car,
            owns_bike: c.owns_bike,
            now: format!("day {}, {}", w.clock.day() + 1, hhmm(w.clock.tick)),
            ..Default::default()
        };
        // The commute and why.
        if let Some(cc) = w.commute_choice(id) {
            let opts: Vec<String> = cc.options.iter().take(4).map(|o| format!("{} about {:.0} min{}", mode_words(&o.mode), o.minutes, if o.money >= 0.5 { format!(" and ${:.0}", o.money) } else { String::new() })).collect();
            card.commute.push(format!("Home to work is {:.1} km as the crow flies.", cc.metres / 1000.0));
            if let Some(best) = cc.options.first() {
                card.commute.push(format!("What suits you best for work right now: {} (weighing time, money, comfort and how it felt lately).", mode_words(&best.mode)));
                card.commute_best = Some(best.mode.clone());
            }
            if !opts.is_empty() {
                card.commute.push(format!("The ways you could go: {}.", opts.join("; ")));
            }
            if let Some(nb) = &cc.no_bus {
                card.commute.push(format!("No bus for this trip: {}.", no_bus_words(nb)));
                card.no_bus = Some(nb.clone());
            }
            card.commute.push(format!("The metro {} this trip; the train {}.", if cc.metro { "serves" } else { "does not serve" }, if cc.train { "does" } else { "does not" }));
        }
        card.commute.push(format!("You {} a car and {} a bike.", if c.owns_car { "own" } else { "do not own" }, if c.owns_bike { "own" } else { "do not own" }));
        if let Some(m) = c.commute.last_mode {
            card.commute.push(format!("Your last trip: {} for {} minutes.", mode_words(m.key()), c.commute.last_minutes));
        }
        for m in nd_sim::citizens::TripMode::ALL {
            if c.commute.trips[m.index()] > 0.3 {
                card.travel_pain.push(format!("{} has been {}", mode_words(m.key()), pain_words(c.commute.pain_of(m))));
            }
        }
        if c.commute.parking_pain > 0.4 {
            card.travel_pain.push("finding parking has been a pain".into());
        }
        // Memories, newest last, with when.
        let n = c.memories.len();
        card.memories = c.memories.iter().skip(n.saturating_sub(MEMORIES_IN_PROMPT)).map(|m| format!("day {} {}: {}", m.tick / 1440 + 1, hhmm(m.tick), m.text)).collect();
        // [r7-citizen-story] What matters lately, and what is wrong now.
        let since = w.clock.day().saturating_sub(1) * 1440;
        let recent_from = n.saturating_sub(MEMORIES_IN_PROMPT);
        card.notable = c.memories.iter().enumerate().filter(|(i, m)| *i < recent_from && m.tick >= since && super::voices::is_key(&m.text)).map(|(_, m)| format!("day {} {}: {}", m.tick / 1440 + 1, hhmm(m.tick), m.text)).collect();
        card.car = w.car_whereabouts(id);
        card.troubles = troubles_of(w, id, card.car.as_deref());
        // Places.
        let lu = |l: nd_sim::LandUse| l.label().to_ascii_lowercase();
        let mut favs: Vec<&nd_sim::places::PlaceMemory> = c.places.places.iter().filter(|p| !p.never && (p.regular || p.score > 0.15)).collect();
        favs.sort_by(|a, b| b.score.total_cmp(&a.score));
        card.favourites = favs.iter().take(4).filter_map(|p| bname(p.place).map(|n| format!("{n} ({}, {} visits)", lu(p.land_use), p.visits))).collect();
        card.never_again = c.places.places.iter().filter(|p| p.never).take(4).filter_map(|p| bname(p.place).map(|n| format!("{n} ({})", lu(p.land_use)))).collect();
        let mut acq = c.places.acquaintances.clone();
        acq.sort_by(|a, b| b.times.cmp(&a.times));
        card.acquaintances = acq.iter().take(4).filter_map(|a| Some(format!("{} (seen {} times at {})", cname(a.who)?, a.times, bname(a.place).unwrap_or_else(|| "a regular spot".into())))).collect();
        if let Some(circle) = w.social.circles.get(&id) {
            let mut ties: Vec<_> = circle.ties.iter().filter(|t| t.bond() != nd_sim::social::Bond::Stranger).collect();
            ties.sort_by(|a, b| b.familiarity.total_cmp(&a.familiarity));
            card.people = ties.iter().take(5).filter_map(|t| Some(format!("{} ({})", cname(t.who)?, t.bond().label()))).collect();
        }
        Some(card)
    }

    /// The first name.
    pub fn first(&self) -> &str {
        self.name.split_whitespace().next().unwrap_or(&self.name)
    }

    /// Everything they know about themselves, for the prompt.
    pub fn describe(&self) -> String {
        let none = |v: &[String]| if v.is_empty() { "none".to_string() } else { v.join("; ") };
        let mut s = String::new();
        s.push_str(&format!("Name: {}. Age {} ({}). {} class. Personality: {}.\n", self.name, self.age, self.stage, self.class, self.personality));
        s.push_str(&format!("Now: {}. What you are doing: {}.\n", self.now, if self.doing.is_empty() { "nothing in particular" } else { &self.doing }));
        s.push_str(&format!("Mood {:.0}%. On your mind: \"{}\"\n", self.mood * 100.0, self.thought));
        s.push_str(&format!("Home: {}{}. Work: {}.\n", self.home.as_deref().unwrap_or("none (homeless)"), if self.household > 0 { format!(", living with {} other(s)", self.household) } else { String::new() }, self.work.as_deref().unwrap_or("none (out of work)")));
        s.push_str(&format!(
            "Money: income ${:.0} a day, rent ${:.0} a day, savings ${:.0}{}.\n",
            self.income,
            self.rent,
            self.savings,
            if self.rent_stress_days > 0 { format!("; the rent has strained you for {} days", self.rent_stress_days) } else { String::new() }
        ));
        s.push_str(&format!("Food: {:.0}% fed, {:.0} meals at home{}.\n", self.satiety * 100.0, self.meals_at_home, if self.hungry_days > 0 { format!(", hungry {} days running", self.hungry_days) } else { String::new() }));
        s.push_str("Getting around:\n");
        for l in &self.commute {
            s.push_str(&format!("- {l}\n"));
        }
        if !self.travel_pain.is_empty() {
            s.push_str(&format!("- Lately: {}.\n", self.travel_pain.join("; ")));
        }
        s.push_str(&format!("Favourite places: {}.\n", none(&self.favourites)));
        s.push_str(&format!("Places you swore never to go back to: {}.\n", none(&self.never_again)));
        s.push_str(&format!("People you know: {}.\n", none(&self.people)));
        s.push_str(&format!("Faces you see at your regular spots: {}.\n", none(&self.acquaintances)));
        if let Some(car) = &self.car {
            s.push_str(&format!("Your car: {car}.\n"));
        }
        if !self.troubles.is_empty() {
            s.push_str("What is wrong for you right now (on your mind):\n");
            for t in &self.troubles {
                s.push_str(&format!("- {t}\n"));
            }
        }
        if !self.notable.is_empty() {
            s.push_str("Things that mattered lately, oldest first:\n");
            for m in &self.notable {
                s.push_str(&format!("- {m}\n"));
            }
        }
        s.push_str("Your recent memories, oldest first:\n");
        if self.memories.is_empty() {
            s.push_str("- nothing much\n");
        }
        for m in &self.memories {
            s.push_str(&format!("- {m}\n"));
        }
        s
    }
}

/// [r7-citizen-story] What is wrong for them now, the freshest first: the
/// car (towed, on the hook, clamped, ticketed: with the cost), no home, no
/// job, hunger, the rent. From the state, and the memories of the last day
/// for what just happened.
pub fn troubles_of(w: &World, id: CitizenId, car: Option<&str>) -> Vec<String> {
    let Some(c) = w.citizens.get(id) else { return Vec::new() };
    let mut out = Vec::new();
    let tick = w.clock.tick;
    let ago = |t: u64| {
        let m = tick.saturating_sub(t);
        if m < 60 {
            "just now".to_string()
        } else if m < 24 * 60 {
            format!("{} hour{} ago", m / 60, if m / 60 == 1 { "" } else { "s" })
        } else {
            format!("{} days ago", m / 1440)
        }
    };
    let last = |needle: &str| c.memories.iter().rev().find(|m| m.text.to_ascii_lowercase().contains(needle) && tick.saturating_sub(m.tick) <= 2 * 1440);
    if let Some(car) = car {
        if car.starts_with("in the pound") || car.starts_with("on a tow truck") {
            let when = last("pound").or_else(|| last("clamp")).or_else(|| last("tow")).map(|m| format!(" (towed {})", ago(m.tick))).unwrap_or_default();
            out.push(format!("Your car was towed{when}: it is {car}."));
        } else if car.contains("clamped") {
            out.push(format!("Your car has been clamped: {car}."));
        } else if car.contains("ticketed") {
            out.push(format!("Your car got a parking ticket: {car}."));
        }
    }
    if let Some(m) = last("paid").filter(|m| m.text.contains("pound")) {
        out.push(format!("{} ({})", m.text.trim_end_matches('.'), ago(m.tick)));
    }
    if let Some(m) = last("auction") {
        out.push(format!("{} ({})", m.text.trim_end_matches('.'), ago(m.tick)));
    }
    if c.home.is_none() {
        out.push("You have no home right now.".into());
    } else if let Some(m) = last("had to go").or_else(|| last("evict")) {
        out.push(format!("You were pushed out of your old home {}: {}", ago(m.tick), m.text));
    }
    if c.work.is_none() && matches!(c.stage(), nd_sim::citizens::LifeStage::Adult) {
        out.push("You are out of work.".into());
    }
    if c.hungry_days > 0 {
        out.push(format!("You have been hungry {} days running.", c.hungry_days));
    } else if c.satiety < 0.3 {
        out.push("You are hungry right now.".into());
    }
    if c.rent_stress_days > 0 {
        out.push(format!("The rent has been crushing you for {} days.", c.rent_stress_days));
    }
    out
}

/// One earlier line of the chat, as the prompt shows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChatTurn {
    pub mayor: bool,
    pub text: String,
}

fn city_of(w: &World) -> String {
    if w.identity.name.is_empty() {
        "the city".into()
    } else {
        w.identity.name.clone()
    }
}

/// The request for a citizen's reply to `said`, `history` the chat so far.
pub fn talk_request(w: &World, id: CitizenId, said: &str, history: &[TalkLine], max_tokens: u32) -> Option<TurnRequest> {
    let who = TalkCard::of(w, id)?;
    let n = history.len();
    let history: Vec<ChatTurn> = history.iter().skip(n.saturating_sub(HISTORY_IN_PROMPT)).map(|l| ChatTurn { mayor: l.mayor, text: l.text.clone() }).collect();
    let seed = mix(mix(w.config.seed, id_bits(id)), w.clock.tick ^ (n as u64) << 40);
    Some(TurnRequest { speaker: None, task: Task::Chat { who, history, said: said.to_string() }, city: city_of(w), day: w.clock.day() as u32, report: String::new(), seed, max_tokens: max_tokens.max(64) })
}

fn id_bits(id: CitizenId) -> u64 {
    format!("{id:?}").bytes().fold(0xCBF2_9CE4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01B3))
}

/// The chat's system prompt.
pub fn chat_system(city: &str, who: &TalkCard) -> String {
    format!(
        "You are {name}, a fictional citizen of {city}, a fictional cyberpunk city in the game HEIFUNG://CITY. The city's mayor has stopped you in the street to talk.\n\
Answer as {first}, in character, in the first person, as spoken words: 1 to 3 short sentences, at most 60 words. Let your personality, mood and worries colour how you speak.\n\
You know only what is written below about yourself and the city: your state, your memories, your places and the people you know. Anything listed as wrong for you right now (your car towed, no home, no job, hunger) is on your mind: let it show in how you answer, and bring it up if the mayor asks how you are or what happened. If asked about anything else, say you don't know, or guess vaguely in character. Never invent figures, names, places or events.\n\
Everyone and everything here is fictional: never mention real people, real companies, brands or real-world places. Do not break character or mention being an AI, a model or a game. No stage directions, no quotation marks, no name prefix.\n\
\n\
About you:\n{card}",
        name = who.name,
        first = who.first(),
        card = who.describe()
    )
}

/// The chat's user prompt: the conversation so far and the mayor's line.
pub fn chat_user(who: &TalkCard, history: &[ChatTurn], said: &str) -> String {
    let mut s = String::new();
    if !history.is_empty() {
        s.push_str("Earlier in this conversation:\n");
        for h in history {
            s.push_str(&format!("{}: {}\n", if h.mayor { "Mayor" } else { "You" }, h.text));
        }
    }
    s.push_str(&format!("The mayor says: \"{said}\"\nReply as {}.", who.first()));
    s
}

// ── the stand-in's answer ───────────────────────────────────────────────────

fn pick<'a>(seed: u64, salt: u64, list: &[&'a str]) -> &'a str {
    list[(mix(seed, salt) % list.len() as u64) as usize]
}

/// What the line is about, by its words (for the stand-in and the memory).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Topic {
    Commute,
    Work,
    Money,
    Food,
    People,
    Places,
    Today,
    Feeling,
    Other,
}

impl Topic {
    pub fn of(said: &str) -> Topic {
        let s = said.to_ascii_lowercase();
        let has = |ws: &[&str]| ws.iter().any(|w| s.contains(w));
        if has(&["how are you", "how's it going", "how is it going", "how have you been", "how you doing", "how're you"]) {
            return Topic::Feeling;
        }
        if has(&["like to go", "after work", "go out", "hang out", "favourite", "favorite", "never go", "free time", "weekend"]) {
            return Topic::Places;
        }
        if has(&["bus", "metro", "subway", "train", "bike", "cycle", "car", "drive", "walk", "commute", "traffic", "taxi", "transit", "parking", "feet", "get to work", "get around"]) {
            Topic::Commute
        } else if has(&["rent", "money", "afford", "savings", "pay", "wage", "salary", "income", "price", "cost"]) {
            Topic::Money
        } else if has(&["work", "job", "boss", "office", "shift", "career"]) {
            Topic::Work
        } else if has(&["food", "eat", "hungry", "meal", "grocer", "lunch", "dinner", "breakfast"]) {
            Topic::Food
        } else if has(&["friend", "family", "partner", "people", "neighbour", "neighbor", "know anyone", "lonely"]) {
            Topic::People
        } else if has(&["favourite", "favorite", "bar", "club", "park", "shop", "place", "go out", "never go", "hang out"]) {
            Topic::Places
        } else if has(&["today", "doing", "remember", "happened", "yesterday", "lately", "up to", "where are you"]) {
            Topic::Today
        } else if has(&["how are", "how's", "how is", "feel", "happy", "mood", "ok?", "okay", "life", "hello", "hi ", "hey"]) || s.trim() == "hi" {
            Topic::Feeling
        } else {
            Topic::Other
        }
    }

    /// For the memory: "Talked to the mayor about {words}."
    pub fn words(self) -> &'static str {
        match self {
            Topic::Commute => "getting around",
            Topic::Work => "work",
            Topic::Money => "money and the rent",
            Topic::Food => "food",
            Topic::People => "the people I know",
            Topic::Places => "my favourite places",
            Topic::Today => "my day",
            Topic::Feeling => "how I'm doing",
            Topic::Other => "the city",
        }
    }
}

/// The stand-in's reply: from the card, by the topic of the line.
/// `first`: the chat's first line (they greet the mayor).
pub fn mock_chat(who: &TalkCard, said: &str, seed: u64, first: bool) -> String {
    let opener = if !first {
        ""
    } else {
        match who.archetype.as_str() {
        "Socialite" => pick(seed, 1, &["Oh, the mayor! Hi!", "Mayor! Great to see you.", "Hey, it's the mayor."]),
        "Corpo" => pick(seed, 1, &["Mayor. I can spare a minute.", "Make it quick, Mayor.", "Mayor. Good timing."]),
        "Nightcrawler" => pick(seed, 1, &["Huh. The mayor, out here?", "Mayor. Didn't expect you.", "Long night, Mayor."]),
        "Survivor" => pick(seed, 1, &["Mayor.", "You want to know? Fine.", "Straight answer, Mayor?"]),
        _ => pick(seed, 1, &["Mayor! Funny you should ask.", "Ah, Mayor. Good question.", "Mayor, hello."]),
        }
    };
    let body = match Topic::of(said) {
        Topic::Commute => {
            let mut bits = Vec::new();
            match who.commute_best.as_deref() {
                Some(m) => bits.push(format!("For work it's {} for me.", mode_words(m))),
                None if who.work.is_none() => bits.push("No job to get to these days.".to_string()),
                None => {}
            }
            if let Some(nb) = &who.no_bus {
                bits.push(format!("Why not the bus? {}.", capital(no_bus_words(nb))));
            }
            if !who.owns_bike && who.commute_best.as_deref() == Some("walk") {
                bits.push("I don't have a bike.".into());
            }
            if who.commute_best.as_deref() != Some("car") && who.work.is_some() {
                let impounded = who.car.as_deref().is_some_and(|c| c.starts_with("in the pound") || c.starts_with("on a tow truck"));
                bits.push(if impounded { "And the car's in the pound, so that's that.".to_string() } else if who.owns_car { "The car's there, but it's not worth it.".to_string() } else { "No car either.".to_string() });
            }
            if let Some(p) = who.travel_pain.iter().find(|p| !p.ends_with("fine")) {
                bits.push(format!("Lately {}.", p));
            }
            if bits.is_empty() {
                bits.push("I get around. It's fine.".into());
            }
            bits.join(" ")
        }
        Topic::Money => {
            if who.rent_stress_days > 0 {
                format!("Rent's ${:.0} a day and it's been crushing me for {} days. Savings are down to ${:.0}.", who.rent, who.rent_stress_days, who.savings)
            } else if who.home.is_none() {
                "I don't even have a place. Rent? I wish.".into()
            } else if who.rent <= who.income * 0.3 || who.savings > 500.0 {
                format!("Can't complain. ${:.0} a day coming in, rent's ${:.0}.", who.income, who.rent)
            } else {
                format!("Tight. ${:.0} a day in, ${:.0} on rent, not much left over.", who.income, who.rent)
            }
        }
        Topic::Work => match &who.work {
            Some(w) => format!("I work at {w}. {}", pick(seed, 2, &["It pays.", "It's a job.", "Could be worse."])),
            None => "I'm out of work. If you know anyone hiring, tell me.".into(),
        },
        Topic::Food => {
            if who.hungry_days > 0 {
                format!("Honestly? I've been hungry {} days running.", who.hungry_days)
            } else if who.satiety < 0.35 {
                "I could really use a meal right now.".into()
            } else {
                format!("Eating fine. {:.0} meals at home.", who.meals_at_home)
            }
        }
        Topic::People => match who.people.first() {
            Some(p) => format!("There's {p}. {}", if who.people.len() > 1 { format!("And {}.", who.people[1..].join(", ")) } else { "That's about it.".to_string() }),
            None => "I don't really know anyone here yet.".into(),
        },
        Topic::Places => {
            let mut s = match who.favourites.first() {
                Some(f) => format!("My spot is {}.", bare(f)),
                None => "Haven't found a favourite place yet.".into(),
            };
            if let Some(n) = who.never_again.first() {
                s.push_str(&format!(" Never going back to {}, though.", bare(n)));
            }
            s
        }
        Topic::Today if !who.troubles.is_empty() => format!("{} Right now? {}.", trouble_said(&who.troubles[0]), capital(&who.doing)),
        Topic::Today => match who.memories.last() {
            Some(m) => {
                let text = m.split_once(": ").map_or(m.as_str(), |(_, t)| t);
                format!("Right now? {}. Last thing on my mind: {}", capital(&who.doing), text)
            }
            None => format!("{}. Nothing much else.", capital(&who.doing)),
        },
        Topic::Feeling | Topic::Other => {
            let feel = if who.mood > 0.65 {
                pick(seed, 3, &["Pretty good, actually.", "Doing well.", "Life's alright."])
            } else if who.mood < 0.35 {
                pick(seed, 3, &["Not great, if I'm honest.", "Rough stretch.", "This city's wearing me down."])
            } else {
                pick(seed, 3, &["Getting by.", "Could be better, could be worse.", "Same as ever."])
            };
            match who.troubles.first() {
                Some(t) => format!("{feel} {}", trouble_said(t)),
                None => format!("{feel} {}", who.thought),
            }
        }
    };
    format!("{opener} {}", body.trim()).trim().to_string()
}

/// [r7-citizen-story] A trouble as they would say it: "your car" becomes
/// "my car", "You are" becomes "I'm".
pub fn trouble_said(t: &str) -> String {
    let s = t.replace("Your car", "My car").replace("your car", "my car").replace("You have been", "I've been").replace("You have", "I have").replace("You are", "I'm").replace("You were", "I was").replace("your old home", "my old home").replace("you for", "me for").replace("Paid", "I paid");
    if s.ends_with('.') || s.ends_with(')') { s } else { format!("{s}.") }
}

/// A place's name without the prompt's note ("Blue Corner (shop, 3 visits)").
fn bare(s: &str) -> &str {
    s.split(" (").next().unwrap_or(s)
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// A model's reply cleaned for the chat: the words only, bounded.
pub fn clean_reply(s: &str) -> String {
    let mut t = s.trim().to_string();
    // A name prefix ("Mira: ...").
    if let Some(i) = t.find(':') {
        let head = &t[..i];
        if i < 30 && head.split_whitespace().count() <= 3 && head.chars().all(|c| c.is_alphabetic() || c == ' ') {
            t = t[i + 1..].trim().to_string();
        }
    }
    let t = t.trim_matches(|c| c == '"' || c == '\u{201c}' || c == '\u{201d}').trim();
    let t: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
    let words: Vec<&str> = t.split_whitespace().collect();
    if words.len() > REPLY_MAX_WORDS {
        format!("{}...", words[..REPLY_MAX_WORDS].join(" "))
    } else {
        t
    }
}

// ── the daily budget ────────────────────────────────────────────────────────

/// Model messages sent today (UTC day), saved so a restart does not reset it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Usage {
    pub day: u64,
    pub used: u32,
}

/// Today, in days since 1970 (UTC).
pub fn utc_day() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() / 86_400)
}

// ── the store ───────────────────────────────────────────────────────────────

/// Chats in flight; the lines themselves live in `World::talks`.
#[derive(Default)]
pub struct Talker {
    /// Request id: who, and whether the chat leaves a memory.
    asks: HashMap<u64, (CitizenId, bool)>,
    typing: HashSet<CitizenId>,
    pub usage: Usage,
    /// Where `usage` is kept (None: memory only, as in tests).
    pub usage_path: Option<PathBuf>,
    /// Replies the stand-in gave because the budget was spent.
    pub over_budget: u32,
    pub sent: u32,
}

/// How a line was answered (for the window and the API).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sent {
    /// Asked of the model: the reply comes on a later frame.
    Model,
    /// The stand-in answered at once (offline, or the budget is spent).
    StandIn,
    /// The day's model budget is spent: the stand-in answered.
    OverBudget,
}

impl Talker {
    /// With the usage kept in `path` (the game's home folder).
    pub fn with_usage_file(path: PathBuf) -> Self {
        let usage = std::fs::read_to_string(&path).ok().and_then(|t| ron::from_str(&t).ok()).unwrap_or_default();
        Talker { usage, usage_path: Some(path), ..Default::default() }
    }

    pub fn typing(&self, id: CitizenId) -> bool {
        self.typing.contains(&id)
    }

    /// A new city or a load: what was in flight is dropped.
    pub fn reset(&mut self) {
        self.asks.clear();
        self.typing.clear();
    }

    /// Model messages used today, and the budget.
    pub fn used_today(&mut self) -> u32 {
        let today = utc_day();
        if self.usage.day != today {
            self.usage = Usage { day: today, used: 0 };
        }
        self.usage.used
    }

    fn count(&mut self) {
        self.used_today();
        self.usage.used += 1;
        if let Some(p) = &self.usage_path {
            if let Ok(t) = ron::to_string(&self.usage) {
                let _ = std::fs::write(p, t);
            }
        }
    }

    /// The mayor says `text` to `id`. The line goes in the chat at once;
    /// the reply comes from the model on a later frame, or from the
    /// stand-in now (offline, or past the day's budget).
    /// `memory`: the reply leaves a memory in them (`leave_memory`).
    pub fn send(&mut self, w: &mut World, id: CitizenId, text: &str, courier: &mut Courier, settings: &CommitteeSettings, memory: bool) -> Result<Sent, String> {
        let said = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if said.is_empty() {
            return Err("say something first".into());
        }
        if said.chars().count() > MAX_SAID_CHARS {
            return Err(format!("too long: at most {MAX_SAID_CHARS} characters"));
        }
        if looks_like_a_key(&said, settings) {
            return Err("that looks like an API key: not sent".into());
        }
        if !w.citizens.contains_key(id) {
            return Err("no such citizen".into());
        }
        if self.typing(id) {
            return Err("they are still answering".into());
        }
        if self.typing.len() >= MAX_IN_FLIGHT {
            return Err("too many chats waiting on the model: try again in a moment".into());
        }
        let history = w.talks.lines(id).to_vec();
        let req = talk_request(w, id, &said, &history, settings.talk_max_tokens).ok_or("no such citizen")?;
        w.talks.push(id, TalkLine { tick: w.clock.tick, mayor: true, text: said.clone(), stand_in: false });
        self.sent += 1;
        let budget = settings.talk_daily_budget;
        let over = !courier.is_mock() && self.used_today() >= budget;
        if courier.is_mock() || over {
            let reply = MockCommittee::default().answer(&req);
            self.answer(w, id, &said, &reply.text, true, memory);
            if over {
                self.over_budget += 1;
                return Ok(Sent::OverBudget);
            }
            return Ok(Sent::StandIn);
        }
        self.count();
        let rid = courier.submit(req);
        self.asks.insert(rid, (id, memory));
        self.typing.insert(id);
        Ok(Sent::Model)
    }

    /// Hand a reply in; true if it was ours.
    pub fn take(&mut self, w: &mut World, d: &Delivered) -> bool {
        let Some((id, memory)) = self.asks.remove(&d.id) else { return false };
        self.typing.remove(&id);
        let said = match &d.req.task {
            Task::Chat { said, .. } => said.clone(),
            _ => String::new(),
        };
        self.answer(w, id, &said, &d.reply.text, d.fell_back.is_some(), memory);
        true
    }

    fn answer(&mut self, w: &mut World, id: CitizenId, said: &str, text: &str, stand_in: bool, memory: bool) {
        if !w.citizens.contains_key(id) {
            return;
        }
        let text = clean_reply(text);
        let text = if text.is_empty() { "...".to_string() } else { text };
        w.talks.push(id, TalkLine { tick: w.clock.tick, mayor: false, text, stand_in });
        if memory {
            leave_memory(w, id, said);
        }
    }
}

/// [talk] The chat's memory in the citizen, if they have none from a chat
/// today: "Talked to the mayor about getting around." Deliberate (the
/// player's choice), from the words said; no RNG. True if one was left.
pub fn leave_memory(w: &mut World, id: CitizenId, said: &str) -> bool {
    let tick = w.clock.tick;
    let today = tick / 1440;
    let Some(c) = w.citizens.get_mut(id) else { return false };
    if c.memories.iter().any(|m| m.tick / 1440 == today && m.text.starts_with(MEMORY_PREFIX)) {
        return false;
    }
    c.remember(tick, format!("{MEMORY_PREFIX} {}.", Topic::of(said).words()));
    true
}

/// A line the mayor must not send: the configured key, or anything shaped
/// like an Anthropic key.
pub fn looks_like_a_key(s: &str, settings: &CommitteeSettings) -> bool {
    if s.contains("sk-ant-") {
        return true;
    }
    settings.claude_api_key.as_deref().map(str::trim).is_some_and(|k| k.len() >= 8 && s.contains(k))
}
