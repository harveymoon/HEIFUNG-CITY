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

/// [r7-citizen-story] One thing that happened, as the diary is told it:
/// when (today or yesterday, at a time), what, and whether it matters
/// (a tow, an eviction, a new job: `is_key`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DiaryEntry {
    pub today: bool,
    /// "18:52".
    pub time: String,
    pub text: String,
    pub key: bool,
}

impl DiaryEntry {
    /// "today 09:10: Paid 95 to get the car out of the pound."
    pub fn line(&self) -> String {
        format!("{} {}: {}", if self.today { "today" } else { "yesterday" }, self.time, self.text)
    }
}

/// Memories that matter to a day's story, by their words: the car and the
/// parking rules, home and work, money, food, the mayor.
pub fn is_key(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    ["pound", "tow", "clamp", "ticket", "had to go", "evict", "hired", "lost my job", "fired", "laid off", "moved into", "found a place", "bought", "sold", "hungry", "fed here", "rent", "leaving", "rolled into", "crash", "robbed", "arrested", "temper", "auction", "mayor", "never came", "sleeping in", "no home", "nowhere to"]
        .iter()
        .any(|k| t.contains(k))
}

fn hhmm(tick: u64) -> String {
    format!("{:02}:{:02}", (tick % 1440) / 60, tick % 60)
}

/// [r7-citizen-story] What the diary is told about how things stand now:
/// where they are, home and work, the car, money and food troubles, and
/// today's trips. The model writes from these and the entries only.
pub fn diary_facts(w: &World, id: CitizenId) -> Vec<String> {
    let Some(c) = w.citizens.get(id) else { return Vec::new() };
    let name = |b: Option<nd_sim::BuildingId>| b.and_then(|b| w.buildings.get(b)).map(|b| b.name.clone());
    let mut out = Vec::new();
    let now = w.activity_line(id);
    out.push(format!("Now ({}): {}", hhmm(w.clock.tick), if now.is_empty() { "nothing in particular".to_string() } else { now }));
    out.push(format!("Home: {}. Work: {}.", name(c.home).unwrap_or_else(|| "none (no home)".into()), name(c.work).unwrap_or_else(|| "none (out of work)".into())));
    match w.car_whereabouts(id) {
        Some(car) => out.push(format!("Car: {car}.")),
        None => out.push("Car: none.".into()),
    }
    if c.hungry_days > 0 {
        out.push(format!("Hungry {} days running.", c.hungry_days));
    }
    if c.rent_stress_days > 0 {
        out.push(format!("The rent has strained them for {} days.", c.rent_stress_days));
    }
    let trips = w.travel_history(id, 1);
    if !trips.is_empty() {
        let n = trips.len();
        let shown: Vec<String> = trips.iter().skip(n.saturating_sub(6)).map(|t| t.line()).collect();
        out.push(format!("Trips today, in order: {}.", shown.join("; ")));
    }
    out
}

/// The request for a citizen's diary of today, from their memories and
/// conversations of today and yesterday in time order, with their times,
/// and how things stand now (`diary_facts`).
pub fn diary_request(w: &World, id: CitizenId) -> Option<TurnRequest> {
    let c = w.citizens.get(id)?;
    let who = CitizenCard::of(w, id)?;
    let day = w.clock.day();
    let since = day.saturating_sub(1) * 1440;
    let today_from = day * 1440;
    let name = |x: CitizenId| w.citizens.get(x).map_or_else(|| "someone".to_string(), |c| c.name.clone());
    let entry = |tick: u64, text: String, key: bool| (tick, DiaryEntry { today: tick >= today_from, time: hhmm(tick), text, key });
    let mut dated: Vec<(u64, DiaryEntry)> = c.memories.iter().filter(|m| m.tick >= since).map(|m| entry(m.tick, m.text.clone(), is_key(&m.text))).collect();
    if dated.is_empty() {
        // Nothing lately: the last few things they remember.
        dated = c.memories.iter().rev().take(3).rev().map(|m| entry(m.tick, m.text.clone(), is_key(&m.text))).collect();
    }
    if let Some(circle) = w.social.circles.get(&id) {
        for n in circle.talks.iter().filter(|n| n.tick >= since).rev().take(4) {
            dated.push(entry(n.tick, format!("Talked {} with {} ({}).", n.topic.label(), name(n.with), n.outcome.label()), false));
        }
    }
    // Oldest first, as it happened (stable: same-minute memories keep their order).
    dated.sort_by_key(|(t, _)| *t);
    let entries = dated.into_iter().map(|(_, e)| e).collect();
    let facts = diary_facts(w, id);
    Some(TurnRequest { speaker: None, task: Task::Diary { who, day: day as u32, entries, facts }, city: city_of(w), day: day as u32, report: String::new(), seed: mix(mix(w.config.seed, id_bits(id)), day), max_tokens: 260 })
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

/// Entries a stand-in diary tells, at most.
pub const MOCK_DIARY_ENTRIES: usize = 6;

/// [r7-citizen-story] What the stand-in's diary tells, in time order:
/// today's entries, the ones that matter first when there are too many
/// (a tow, an eviction, a job), a bench sat on twice told once; with
/// little today, yesterday's that matter come first ("Yesterday, ...").
pub fn diary_pick(entries: &[DiaryEntry]) -> Vec<&DiaryEntry> {
    let norm = |e: &DiaryEntry| e.text.chars().filter(|c| !c.is_ascii_digit()).collect::<String>();
    let mut seen = std::collections::HashSet::new();
    let today: Vec<(usize, &DiaryEntry)> = entries.iter().enumerate().filter(|(_, e)| e.today).filter(|(_, e)| seen.insert(norm(e))).collect();
    let mut keep: Vec<usize> = today.iter().filter(|(_, e)| e.key).map(|(i, _)| *i).collect();
    // Too many that matter: the latest ones.
    if keep.len() > MOCK_DIARY_ENTRIES {
        keep.drain(..keep.len() - MOCK_DIARY_ENTRIES);
    }
    // Room left: the rest of the day, latest first (the day's end is what
    // is on their mind tonight), then all back in time order.
    for (i, _) in today.iter().rev().filter(|(_, e)| !e.key) {
        if keep.len() >= MOCK_DIARY_ENTRIES {
            break;
        }
        keep.push(*i);
    }
    if keep.len() < 2 {
        let ys: Vec<usize> = entries.iter().enumerate().filter(|(_, e)| !e.today && e.key).map(|(i, _)| i).collect();
        keep.extend(ys.iter().skip(ys.len().saturating_sub(2)));
    }
    keep.sort_unstable();
    keep.into_iter().map(|i| &entries[i]).collect()
}

/// The stand-in's diary: the day's memories in the first person, in the
/// order they happened (`diary_pick`), framed by the personality and the
/// mood, and how the car stands if it is in trouble.
pub fn mock_diary(who: &CitizenCard, day: u32, entries: &[DiaryEntry], facts: &[String], seed: u64) -> String {
    let opening = match who.archetype.as_str() {
        "Socialite" => pick(seed, 1, &["Busy day, people everywhere.", "Saw half the city today, or felt like it.", "Never a quiet moment."]),
        "Corpo" => pick(seed, 1, &["Another day closer to the corner office.", "Logged the hours. Logged them well.", "Ambition doesn't sleep, and neither do I."]),
        "Nightcrawler" => pick(seed, 1, &["Writing this late, as usual.", "The neon was loud tonight.", "Day was a blur; the night was better."]),
        "Survivor" => pick(seed, 1, &["Made it through another one.", "Kept my head down today.", "Careful day. That's how you last here."]),
        _ => pick(seed, 1, &["Noticed something new today.", "Took the long way round again.", "The city keeps changing under my feet."]),
    };
    let mut parts = vec![format!("Day {}. {opening}", day + 1)];
    let picked = diary_pick(entries);
    let mut first_today = true;
    for (i, e) in picked.iter().enumerate() {
        let text = e.text.trim().trim_end_matches('.');
        let lead = if !e.today {
            "Yesterday, "
        } else if first_today {
            first_today = false;
            if e.time.as_str() < "12:00" { pick(seed, 10 + i as u64, &["", "This morning: ", "First, "]) } else { pick(seed, 10 + i as u64, &["", "First, "]) }
        } else {
            pick(seed, 10 + i as u64, &["Then ", "After that, ", "Later, ", "And "])
        };
        let text = if lead.is_empty() { text.to_string() } else { lower_first(text) };
        parts.push(format!("{lead}{text}."));
    }
    // The car still in trouble: said as it stands now, not as done.
    if let Some(car) = facts.iter().find_map(|f| f.strip_prefix("Car: ")).filter(|c| c.starts_with("in the pound") || c.starts_with("on a tow truck")) {
        parts.push(format!("The car is still {}.", car.trim_end_matches('.').replacen(": ", ". It's ", 1)));
    }
    if picked.is_empty() {
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
