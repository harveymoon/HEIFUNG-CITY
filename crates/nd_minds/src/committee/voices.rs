//! [voices] Words for the people the player cares about, through the
//! committee's backend: the lines spotlit and favourite citizens say in
//! their conversations, and diaries written on demand from what the sim
//! already remembers for them.
//!
//! Display only. The sim writes its own template line into every
//! `Talk` and never reads anything here; a model's line or diary lives in
//! [`Voices`] (app memory, not the save) and only replaces what is shown.
//! With the stand-in, conversations keep the sim's template line (no
//! request at all) and diaries are composed from templates and the
//! memories; with a model both are asked asynchronously, the template
//! standing until the answer comes, and a failure falls back to it.

use std::collections::{BTreeMap, HashMap};

use nd_sim::committee::mix;
use nd_sim::social::Talk;
use nd_sim::{CitizenId, World};
use serde::{Deserialize, Serialize};

use super::{Courier, Delivered, Task, TurnRequest};
use crate::Personality;

/// Conversation lines asked at once, at most (a busy plaza does not flood
/// a local model).
pub const MAX_LINES_IN_FLIGHT: usize = 3;
/// Diaries kept (oldest dropped).
pub const DIARIES_KEPT: usize = 200;
/// Lines kept.
pub const LINES_KEPT: usize = 400;

/// A citizen as a prompt needs them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CitizenCard {
    pub name: String,
    pub age: u8,
    pub personality: String,
    pub archetype: String,
    pub class: String,
    pub mood: f32,
    pub home: Option<String>,
    pub work: Option<String>,
    pub thought: String,
}

impl CitizenCard {
    pub fn of(w: &World, id: CitizenId) -> Option<CitizenCard> {
        let c = w.citizens.get(id)?;
        let p = Personality::from_seed(c.seed);
        let name_of = |b: Option<nd_sim::BuildingId>| b.and_then(|b| w.buildings.get(b)).map(|b| b.name.clone());
        Some(CitizenCard { name: c.name.clone(), age: c.age, personality: p.summary(), archetype: p.archetype.to_string(), class: c.class().label().to_string(), mood: c.mood, home: name_of(c.home), work: name_of(c.work), thought: c.thought.clone() })
    }

    pub fn describe(&self) -> String {
        format!(
            "{}, {}, {} class; {}; mood {:.0}%; home {}; work {}",
            self.name,
            self.age,
            self.class,
            self.personality,
            self.mood * 100.0,
            self.home.as_deref().unwrap_or("none"),
            self.work.as_deref().unwrap_or("none")
        )
    }
}

fn city_of(w: &World) -> String {
    if w.identity.name.is_empty() {
        "the city".into()
    } else {
        w.identity.name.clone()
    }
}

/// The request for one conversation line (`a` speaks to `b`).
pub fn dialogue_request(w: &World, t: &Talk) -> Option<TurnRequest> {
    let a = CitizenCard::of(w, t.a)?;
    let b = CitizenCard::of(w, t.b)?;
    let place = t.place.and_then(|p| w.buildings.get(p)).map(|b| b.name.clone());
    let bond = w.social.circles.get(&t.a).map(|c| c.bond(t.b).label().to_string()).unwrap_or_else(|| "strangers".into());
    Some(TurnRequest {
        speaker: None,
        task: Task::Dialogue { a, b, bond, topic: t.topic.label().to_string(), outcome: t.outcome.label().to_string(), setting: format!("{:?}", t.setting).to_ascii_lowercase(), place, template: t.line.clone() },
        city: city_of(w),
        day: (t.tick / 1440) as u32,
        report: String::new(),
        seed: mix(w.config.seed, t.serial),
        max_tokens: 60,
    })
}

/// The request for a citizen's diary of today, from their memories.
pub fn diary_request(w: &World, id: CitizenId) -> Option<TurnRequest> {
    let c = w.citizens.get(id)?;
    let who = CitizenCard::of(w, id)?;
    let day = w.clock.day();
    let since = day.saturating_sub(1) * 1440;
    let entries: Vec<String> = c.memories.iter().filter(|m| m.tick >= since).map(|m| m.text.clone()).collect();
    let entries = if entries.is_empty() { c.memories.iter().rev().take(3).rev().map(|m| m.text.clone()).collect() } else { entries };
    let name = |x: CitizenId| w.citizens.get(x).map_or_else(|| "someone".to_string(), |c| c.name.clone());
    let talks: Vec<String> = w.social.circles.get(&id).map_or_else(Vec::new, |circle| {
        circle.talks.iter().filter(|n| n.tick >= since).rev().take(4).map(|n| format!("talked {} with {} ({})", n.topic.label(), name(n.with), n.outcome.label())).collect()
    });
    Some(TurnRequest { speaker: None, task: Task::Diary { who, day: day as u32, entries, talks }, city: city_of(w), day: day as u32, report: String::new(), seed: mix(mix(w.config.seed, id_bits(id)), day), max_tokens: 220 })
}

fn id_bits(id: CitizenId) -> u64 {
    format!("{id:?}").bytes().fold(0xCBF2_9CE4_8422_2325u64, |h, b| (h ^ b as u64).wrapping_mul(0x0100_0000_01B3))
}

// ── the stand-in's words ────────────────────────────────────────────────────

fn pick<'a>(seed: u64, salt: u64, list: &[&'a str]) -> &'a str {
    list[(mix(seed, salt) % list.len() as u64) as usize]
}

/// The stand-in's line: the sim's own template.
pub fn mock_dialogue(template: &str) -> String {
    template.to_string()
}

/// The stand-in's diary: the day's memories in the first person, framed by
/// the personality and the mood.
pub fn mock_diary(who: &CitizenCard, day: u32, entries: &[String], talks: &[String], seed: u64) -> String {
    let opening = match who.archetype.as_str() {
        "Socialite" => pick(seed, 1, &["Busy day, people everywhere.", "Saw half the city today, or felt like it.", "Never a quiet moment."]),
        "Corpo" => pick(seed, 1, &["Another day closer to the corner office.", "Logged the hours. Logged them well.", "Ambition doesn't sleep, and neither do I."]),
        "Nightcrawler" => pick(seed, 1, &["Writing this late, as usual.", "The neon was loud tonight.", "Day was a blur; the night was better."]),
        "Survivor" => pick(seed, 1, &["Made it through another one.", "Kept my head down today.", "Careful day. That's how you last here."]),
        _ => pick(seed, 1, &["Noticed something new today.", "Took the long way round again.", "The city keeps changing under my feet."]),
    };
    let mut parts = vec![format!("Day {}. {opening}", day + 1)];
    for (i, e) in entries.iter().take(4).enumerate() {
        let e = e.trim().trim_end_matches('.');
        let lead = pick(seed, 10 + i as u64, &["", "Also: ", "Then ", "And ", "Later, "]);
        let e = if lead.is_empty() { e.to_string() } else { lower_first(e) };
        parts.push(format!("{lead}{e}."));
    }
    if let Some(t) = talks.first() {
        parts.push(format!("I {t}.", t = t.trim_end_matches('.')));
    }
    if entries.is_empty() && talks.is_empty() {
        parts.push(pick(seed, 3, &["Nothing much happened, and that's fine.", "Quiet. I'll take it.", "Same streets, same faces."]).to_string());
    }
    parts.push(
        if who.mood > 0.65 {
            pick(seed, 4, &["Good day, all told.", "I could get used to this.", "Going to sleep smiling."])
        } else if who.mood < 0.35 {
            pick(seed, 4, &["Tomorrow has to be better.", "Some days this city wins.", "Bed. Forget it."])
        } else {
            pick(seed, 4, &["Could be worse.", "Tomorrow, then.", "That's the day."])
        }
        .to_string(),
    );
    parts.join(" ")
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) if !s.starts_with("I ") && !s.starts_with("I'") => f.to_lowercase().collect::<String>() + c.as_str(),
        Some(f) => f.to_string() + c.as_str(),
        None => String::new(),
    }
}

// ── the store ───────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub struct Diary {
    pub day: u32,
    pub text: String,
    /// A model wrote it (false: the stand-in, or a fallback).
    pub by_model: bool,
    /// Asked, not answered yet.
    pub pending: bool,
}

enum Ask {
    Line(u64),
    Diary(CitizenId),
}

/// Lines and diaries for display, kept in the app (not the save).
#[derive(Default)]
pub struct Voices {
    lines: BTreeMap<u64, String>,
    diaries: HashMap<CitizenId, Diary>,
    asks: HashMap<u64, Ask>,
    /// The newest conversation looked at.
    seen: u64,
    pub lines_written: u32,
    pub diaries_written: u32,
}

impl Voices {
    /// A model's line for a conversation, if one came.
    pub fn line(&self, serial: u64) -> Option<&str> {
        self.lines.get(&serial).map(String::as_str)
    }

    pub fn diary(&self, id: CitizenId) -> Option<&Diary> {
        self.diaries.get(&id)
    }

    pub fn reset(&mut self) {
        *self = Voices::default();
    }

    /// Look at new conversations: one where a watched citizen speaks gets
    /// a model's line (the stand-in keeps the template: no request).
    pub fn pump(&mut self, w: &World, courier: &mut Courier, watched: &dyn Fn(CitizenId) -> bool) {
        let newest = w.social.recent.back().map_or(0, |t| t.serial);
        if newest < self.seen {
            self.seen = 0;
        }
        if courier.is_mock() {
            self.seen = newest;
            return;
        }
        let in_flight = self.asks.values().filter(|a| matches!(a, Ask::Line(_))).count();
        let mut room = MAX_LINES_IN_FLIGHT.saturating_sub(in_flight);
        for t in w.social.recent.iter().filter(|t| t.serial > self.seen) {
            if room > 0 && (watched(t.a) || watched(t.b)) {
                if let Some(req) = dialogue_request(w, t) {
                    let id = courier.submit(req);
                    self.asks.insert(id, Ask::Line(t.serial));
                    room -= 1;
                }
            }
        }
        self.seen = newest;
    }

    /// Ask for a citizen's diary of today (again, if `refresh`). The
    /// stand-in's arrives on the next poll.
    pub fn ask_diary(&mut self, w: &World, id: CitizenId, courier: &mut Courier, refresh: bool) {
        let day = w.clock.day() as u32;
        if let Some(d) = self.diaries.get(&id) {
            if d.pending || (d.day == day && !refresh) {
                return;
            }
        }
        let Some(req) = diary_request(w, id) else { return };
        let text = self.diaries.get(&id).map(|d| d.text.clone()).unwrap_or_default();
        self.diaries.insert(id, Diary { day, text, by_model: false, pending: true });
        let rid = courier.submit(req);
        self.asks.insert(rid, Ask::Diary(id));
    }

    /// Hand a reply in; true if it was ours.
    pub fn take(&mut self, d: &Delivered) -> bool {
        let Some(ask) = self.asks.remove(&d.id) else { return false };
        let text = clean_line(&d.reply.text);
        match ask {
            Ask::Line(serial) => {
                if d.fell_back.is_none() && !text.is_empty() {
                    self.lines.insert(serial, text);
                    self.lines_written += 1;
                    while self.lines.len() > LINES_KEPT {
                        let first = *self.lines.keys().next().unwrap();
                        self.lines.remove(&first);
                    }
                }
            }
            Ask::Diary(id) => {
                let day = match &d.req.task {
                    Task::Diary { day, .. } => *day,
                    _ => 0,
                };
                self.diaries.insert(id, Diary { day, text: d.reply.text.trim().to_string(), by_model: d.fell_back.is_none() && !courier_mock(d), pending: false });
                self.diaries_written += 1;
                if self.diaries.len() > DIARIES_KEPT {
                    if let Some(old) = self.diaries.iter().filter(|(_, x)| !x.pending).min_by_key(|(_, x)| x.day).map(|(k, _)| *k) {
                        self.diaries.remove(&old);
                    }
                }
            }
        }
        true
    }
}

/// The stand-in marks its diaries so (see `MockCommittee::answer`).
fn courier_mock(d: &Delivered) -> bool {
    d.reply.json.as_ref().is_some_and(|j| j.get("stand_in").is_some())
}

/// One line, unquoted, no speaker prefix, at most about 25 words.
pub fn clean_line(s: &str) -> String {
    let mut t = s.trim().lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
    // "Mira: ..." or "Mira Sato says: ..." -> the words only.
    if let Some(i) = t.find(':') {
        let head = &t[..i];
        if i < 30 && head.split_whitespace().count() <= 3 && head.chars().all(|c| c.is_alphabetic() || c == ' ' || c == '"' || c == '\'') {
            t = t[i + 1..].trim().to_string();
        }
    }
    let t = t.trim_matches(|c| c == '"' || c == '\u{201c}' || c == '\u{201d}').trim().to_string();
    let words: Vec<&str> = t.split_whitespace().collect();
    if words.len() > 28 {
        format!("{}...", words[..28].join(" "))
    } else {
        t
    }
}
