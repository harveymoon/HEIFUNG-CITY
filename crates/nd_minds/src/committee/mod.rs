//! [committee] What the committee says. The deterministic half (pool,
//! seats, rules, proposals, sites, votes by the rules) is `nd_sim::committee`;
//! this half turns each turn of a meeting into a request for a text
//! backend and folds the answer back in:
//!
//! - [`CommitteeBackend`]: submit a [`TurnRequest`], poll for the reply.
//!   [`mock::MockCommittee`] answers at once from templates and real
//!   numbers (deterministic in the request's seed); [`http::HttpBackend`]
//!   talks to a local OpenAI-compatible server (Ollama, LM Studio) or the
//!   Claude Messages API on a worker thread.
//! - [`Courier`]: wraps the chosen backend and falls back to the stand-in
//!   for a turn whose request fails or times out, so a meeting always ends.
//! - [`driver`]: the secretary's fixed agenda, turn by turn, with turn
//!   limits so a meeting costs a predictable number of calls.
//!
//! Nothing a model writes reaches the city by itself: proposals are
//! validated against what the game can do, sites come from code, and only
//! the mayor implements a project.

pub mod driver;
pub mod http;
pub mod mock;
pub mod questions;
pub mod settings;
pub mod talk;
pub mod voices;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod talk_tests;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use nd_sim::committee::{Action, Fact, Politician, Vote};
use serde::{Deserialize, Serialize};

pub use driver::{cast_mayor_vote, start_meeting, Driver};
pub use mock::MockCommittee;
pub use settings::{BackendChoice, CommitteeSettings};

/// Who speaks, as a prompt needs them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PersonaCard {
    pub id: u32,
    pub name: String,
    pub title: String,
    pub scope: String,
    pub temper: String,
    pub temper_key: String,
    pub temper_note: String,
    pub leanings: String,
    pub goals: Vec<String>,
    pub party: String,
    pub background: String,
    /// What they remember, newest last (a few lines).
    pub memory: Vec<String>,
}

impl PersonaCard {
    pub fn of(p: &Politician) -> Self {
        PersonaCard {
            id: p.id,
            name: p.name.clone(),
            title: p.scope.title().into(),
            scope: p.scope.label().into(),
            temper: p.temper.label().into(),
            temper_key: p.temper.key().into(),
            temper_note: p.temper.describe().into(),
            leanings: p.leanings.describe(),
            goals: p.goals(),
            party: p.party.clone(),
            background: p.background.clone(),
            memory: p.memory.iter().rev().take(4).rev().map(|m| m.line()).collect(),
        }
    }
}

/// One proposal as a voter sees it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VoteItem {
    pub id: u32,
    pub title: String,
    pub by: String,
    pub rationale: String,
    pub cost: f32,
    /// How the rules would have this member vote, and why.
    pub lean: Vote,
    pub lean_reason: String,
}

/// What a turn asks for.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Task {
    /// Phrase an alert between meetings (text).
    Alert { rule: String, when: String, severity: u8, facts: Vec<Fact> },
    /// The secretary opens the meeting (text).
    Opening { meeting: u32, members: Vec<String>, alerts: usize, on_demand: bool },
    /// A member's status report on their scope (text).
    /// `first`: no earlier meeting, so the figures compare with the week before.
    Status { facts: Vec<Fact>, alerts: Vec<String>, first: bool },
    /// A member answers another's report (text).
    Response { to: String, to_scope: String, said: String, agree: bool, facts: Vec<Fact>, first: bool },
    /// A member tables a topic (JSON `{proposal, rationale}`); the rules'
    /// suggestion is offered, the model may table its own.
    Propose { suggestion: Option<(Action, String)>, areas: Vec<String> },
    /// A member votes on every proposal (JSON `{votes: [...]}`).
    Vote { items: Vec<VoteItem> },
    /// The secretary picks a site among the valid ones (JSON `{choice, reason}`).
    Pick { title: String, options: Vec<String>, best: usize },
    /// The secretary closes (text).
    Closing { results: Vec<String>, projects: usize, next_day: u32 },
    /// "Test connection".
    Ping,
    /// [voices] One line `a` says to `b` in a conversation (text); the
    /// sim's own template line is the stand-in's answer.
    Dialogue { a: voices::CitizenCard, b: voices::CitizenCard, bond: String, topic: String, outcome: String, setting: String, place: Option<String>, template: String },
    /// [voices] A citizen's diary of the day, from their memories (text).
    /// [r7-citizen-story] `entries`: memories and conversations of today
    /// and yesterday, oldest first, with their times; `facts`: how things
    /// stand now (the car, home and work, today's trips).
    Diary { who: voices::CitizenCard, day: u32, entries: Vec<voices::DiaryEntry>, #[serde(default)] facts: Vec<String> },
    /// [ask-committee] The mayor's question between meetings (JSON
    /// `{respond, text}`): the member may choose not to answer. `earlier`:
    /// what others said before them in the thread (bounded). `last`: the
    /// last member asked, and nobody has answered yet (the stand-in then
    /// answers rather than leave the mayor in silence).
    Question { question: String, facts: Vec<Fact>, earlier: Vec<Heard>, last: bool },
    /// [talk] The mayor talks to a citizen: their in-character reply to
    /// `said` (text), from their real state and the chat so far.
    Chat { who: talk::TalkCard, history: Vec<talk::ChatTurn>, said: String },
}

/// [ask-committee] A reply earlier in a question's thread, as the next
/// member reads it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Heard {
    pub name: String,
    pub scope: String,
    pub text: String,
}

impl Task {
    pub fn key(&self) -> &'static str {
        match self {
            Task::Alert { .. } => "alert",
            Task::Opening { .. } => "opening",
            Task::Status { .. } => "status",
            Task::Response { .. } => "response",
            Task::Propose { .. } => "propose",
            Task::Vote { .. } => "vote",
            Task::Pick { .. } => "pick",
            Task::Closing { .. } => "closing",
            Task::Ping => "ping",
            Task::Dialogue { .. } => "dialogue",
            Task::Diary { .. } => "diary",
            Task::Question { .. } => "question",
            Task::Chat { .. } => "talk",
        }
    }

    /// Does the reply have to be JSON?
    pub fn wants_json(&self) -> bool {
        matches!(self, Task::Propose { .. } | Task::Vote { .. } | Task::Pick { .. } | Task::Question { .. })
    }
}

/// One request to a backend.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TurnRequest {
    /// None: the secretary.
    pub speaker: Option<PersonaCard>,
    pub task: Task,
    pub city: String,
    pub day: u32,
    /// The compact city report (`CityReport::to_text`).
    pub report: String,
    /// Seeds the stand-in's wording.
    pub seed: u64,
    /// Reply length cap for a model.
    pub max_tokens: u32,
}

/// A backend's answer: the words, and the JSON object in them if the task
/// wanted one.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TurnReply {
    pub text: String,
    pub json: Option<serde_json::Value>,
}

impl TurnReply {
    /// Split a model's raw answer: the first balanced `{...}` is the JSON.
    pub fn from_raw(raw: &str, wants_json: bool) -> TurnReply {
        let text = raw.trim().to_string();
        if !wants_json {
            return TurnReply { text, json: None };
        }
        let json = extract_json(&text);
        TurnReply { text, json }
    }
}

/// The first JSON object in a text (models like to wrap it in prose or
/// code fences).
pub fn extract_json(s: &str) -> Option<serde_json::Value> {
    let bytes = s.as_bytes();
    let mut start = 0;
    while let Some(off) = s[start..].find('{') {
        let open = start + off;
        let (mut depth, mut in_str, mut esc) = (0i32, false, false);
        for (i, &c) in bytes.iter().enumerate().skip(open) {
            if in_str {
                match c {
                    _ if esc => esc = false,
                    b'\\' => esc = true,
                    b'"' => in_str = false,
                    _ => {}
                }
                continue;
            }
            match c {
                b'"' => in_str = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s[open..=i]) {
                            return Some(v);
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
        start = open + 1;
    }
    None
}

/// Anything that can answer turns asynchronously. The game thread only
/// ever calls `submit` and `poll`; neither blocks.
pub trait CommitteeBackend: Send + Sync {
    fn submit(&mut self, id: u64, req: TurnRequest);
    fn poll(&mut self) -> Vec<(u64, Result<TurnReply, String>)>;
    fn name(&self) -> String;
    /// The courier gave up on turn `id` (it timed out and the stand-in
    /// answered): a backend that has not sent it yet should not, since a
    /// late answer is thrown away and a paid call would be wasted.
    fn cancel(&mut self, _id: u64) {}
    /// True for the stand-in (no model, no network).
    fn is_mock(&self) -> bool {
        false
    }
}

// ── prompts ─────────────────────────────────────────────────────────────────

/// The system prompt: who is speaking and the rules of the room.
pub fn system_prompt(req: &TurnRequest) -> String {
    match &req.task {
        Task::Dialogue { a, .. } => {
            return format!("You write one line of dialogue for {}, a fictional citizen of {}, a cyberpunk city in the game HEIFUNG://CITY. Who they are: {}. Reply with the line only: one short spoken sentence (at most 20 words), no quotes, no name, no stage directions.", a.name, req.city, a.describe());
        }
        Task::Chat { who, .. } => return talk::chat_system(&req.city, who),
        Task::Diary { who, .. } => {
            return format!("You write the private diary of {}, a fictional citizen of {}, a cyberpunk city in the game HEIFUNG://CITY. Who they are: {}. Write tonight's entry about today in the first person, in their voice, 60 to 120 words, plain prose. The events are listed oldest first with their times: tell them in that order, never moving one before or after another, and never count a trip as part of a later event. Yesterday's are background only. Use only the events and facts you are given; never invent events, outcomes, places, people or numbers. What the facts say is still so (a car still in the pound, no job, no home) has not been put right: do not write that it was.", who.name, req.city, who.describe());
        }
        _ => {}
    }
    let mut s = String::new();
    let question = matches!(req.task, Task::Question { .. });
    if question {
        s.push_str(&format!("This is question time in a city-builder game, HEIFUNG://CITY: between meetings the mayor of {}, a fictional cyberpunk city, has put a question to the committee in its chat room. Everyone in the room is fictional.\n", req.city));
    } else {
        s.push_str(&format!("This is a turn in a city-builder game, HEIFUNG://CITY: a meeting of the mayor's committee of {}, a fictional cyberpunk city. Everyone in the room is fictional.\n", req.city));
    }
    match &req.speaker {
        Some(p) => {
            s.push_str(&format!("You are {}, the {} (scope: {}). Former {}. Party: {}.\n", p.name, p.title, p.scope, p.background, p.party));
            s.push_str(&format!("Temper: {} ({}). Leanings: {}.\n", p.temper, p.temper_note, p.leanings));
            s.push_str(&format!("Your goals: {}.\n", p.goals.join("; ")));
            if !p.memory.is_empty() {
                s.push_str("What you remember of past votes:\n");
                for m in &p.memory {
                    s.push_str(&format!("- {m}\n"));
                }
            }
            if question {
                s.push_str("Stay in character and consistent with your past votes. You need not answer: answer when the question touches your scope, your goals or your convictions, or when you have something worth adding to what the others said; otherwise decline. If you answer, speak in plain words, 1 to 4 sentences, to the mayor or in reply to a colleague. Quote only numbers that are in the city report; never invent figures. No stage directions.\n");
            } else {
                s.push_str("Stay in character and consistent with your past votes. Speak in plain words, 2 to 4 sentences. Quote only numbers that are in the city report; never invent figures. No stage directions.\n");
            }
        }
        None => s.push_str("You are the committee secretary: neutral, brief and orderly. You keep to the agenda and speak in 1 to 3 sentences. Quote only numbers in the city report.\n"),
    }
    s
}

/// The user prompt: the report and the task.
pub fn user_prompt(req: &TurnRequest) -> String {
    if let Task::Chat { who, history, said } = &req.task {
        return talk::chat_user(who, history, said);
    }
    let mut s = format!("{}\n", req.report);
    let facts = |f: &[Fact]| f.iter().map(Fact::line).collect::<Vec<_>>().join("; ");
    match &req.task {
        Task::Alert { when, facts: f, severity, .. } => s.push_str(&format!("Between meetings, a rule in your scope has tripped: {when}. The figures: {}. Severity {severity} of 3. Write the one- or two-sentence alert you send the mayor.", facts(f))),
        Task::Opening { meeting, members, alerts, on_demand } => s.push_str(&format!(
            "Open meeting {meeting}{}. Present: {}. {alerts} alerts were raised since the last meeting. Announce the agenda: 1) status reports, 2) topics of concern, 3) votes, 4) implementation.",
            if *on_demand { ", called by the mayor" } else { "" },
            members.join(", ")
        )),
        Task::Status { facts: f, alerts, first } => {
            s.push_str(&format!("Agenda item 1: give your status report on your scope. Your figures ({}): {}.", if *first { "\"was\" is the week before" } else { "\"was\" is at the last meeting" }, facts(f)));
            if !alerts.is_empty() {
                s.push_str(&format!(" Alerts you raised since the last meeting: {}.", alerts.join("; ")));
            }
        }
        Task::Response { to, to_scope, said, agree, facts: f, .. } => s.push_str(&format!(
            "{to} ({to_scope}) just said: \"{said}\". Respond briefly from your own scope and leanings; you lean towards {}. Your figures: {}.",
            if *agree { "agreeing" } else { "disagreeing" },
            facts(f)
        )),
        Task::Propose { suggestion, areas } => {
            s.push_str("Agenda item 2: table one topic of concern as a structured proposal the game can carry out. Allowed actions and fields:\n");
            s.push_str(nd_sim::committee::proposals::ACTION_HELP);
            s.push_str(&format!("\nAreas: {}.\n", areas.join(", ")));
            if let Some((a, why)) = suggestion {
                s.push_str(&format!("The figures suggest: {} (because {why}). You may table that or something better within your scope.\n", serde_json::to_string(a).unwrap_or_default()));
            }
            s.push_str("Reply with only a JSON object: {\"proposal\": {\"action\": \"...\", ...fields}, \"rationale\": \"your 1-2 sentence case\"}, or {\"proposal\": null, \"rationale\": \"why nothing\"}. Never give coordinates: the secretary finds the site.");
        }
        Task::Vote { items } => {
            s.push_str("Agenda item 3: vote on each proposal: yes, no or abstain, with a one-sentence reason.\n");
            for it in items {
                s.push_str(&format!("- #{}: {} (tabled by {}; cost about {:.0}): {}\n", it.id, it.title, it.by, it.cost, it.rationale));
            }
            s.push_str("Reply with only a JSON object: {\"votes\": [{\"proposal\": <id>, \"vote\": \"yes|no|abstain\", \"reason\": \"...\"}]}.");
        }
        Task::Pick { title, options, .. } => {
            s.push_str(&format!("Agenda item 4: \"{title}\" passed. The game found these valid sites:\n"));
            for (i, o) in options.iter().enumerate() {
                s.push_str(&format!("{}. {o}\n", i + 1));
            }
            s.push_str("Pick one for the mayor's review. Reply with only a JSON object: {\"choice\": <number>, \"reason\": \"one sentence\"}.");
        }
        Task::Closing { results, projects, next_day } => s.push_str(&format!("Close the meeting. Results: {}. {projects} project(s) go to the mayor for review. The next meeting is on day {next_day}.", if results.is_empty() { "nothing was voted".into() } else { results.join("; ") })),
        Task::Ping => s.push_str("Reply with the single word: ready."),
        Task::Question { question, facts: f, earlier, .. } => {
            if !f.is_empty() {
                s.push_str(&format!("Your figures: {}.\n", facts(f)));
            }
            s.push_str(&format!("The mayor asks the committee: \"{question}\"\n"));
            if earlier.is_empty() {
                s.push_str("Nobody has answered yet.\n");
            } else {
                s.push_str("Already said in this thread:\n");
                for h in earlier {
                    s.push_str(&format!("- {} ({}): \"{}\"\n", h.name, h.scope, h.text));
                }
            }
            s.push_str("Decide whether to answer. Reply with only a JSON object: {\"respond\": true, \"text\": \"your answer\"} or {\"respond\": false, \"text\": \"\"}.");
        }
        Task::Dialogue { a, b, bond, topic, outcome, setting, place, template } => {
            s = format!(
                "{} talks to {} ({}; {bond}) about {topic}, {}. How it goes: {outcome}. The game's own line was: \"{template}\". Write {}'s line instead, in their voice.",
                a.name,
                b.name,
                b.describe(),
                match place {
                    Some(p) => format!("at {p}"),
                    None => format!("in the {setting}"),
                },
                a.name
            );
        }
        Task::Diary { day, entries, facts, .. } => {
            s = format!("Day {}. What happened, oldest first (yesterday's for background, then today's), with the times:\n", day + 1);
            for e in entries {
                s.push_str(&format!("- {}\n", e.line()));
            }
            if entries.is_empty() {
                s.push_str("- nothing much\n");
            }
            if !facts.is_empty() {
                s.push_str("How things stand now:\n");
                for f in facts {
                    s.push_str(&format!("- {f}\n"));
                }
            }
            s.push_str("Write tonight's diary entry about today, keeping the events in the order above.");
        }
        // [talk] Answered above.
        Task::Chat { .. } => {}
    }
    s
}

// ── the courier: a backend with the stand-in behind it ──────────────────────

/// The chosen backend plus the stand-in: a request that fails, comes back
/// unusable or takes longer than the timeout is answered by the stand-in
/// instead, and the error is kept for the window. Ids are the courier's.
pub struct Courier {
    backend: Box<dyn CommitteeBackend>,
    mock: MockCommittee,
    open: HashMap<u64, (TurnRequest, Instant)>,
    next: u64,
    pub timeout: Duration,
    /// Recent errors, newest last (never holds a key: backends redact).
    pub errors: Vec<String>,
    /// Turns the stand-in answered for a failed backend.
    pub fallbacks: u32,
    /// Turns asked of the backend.
    pub calls: u32,
}

/// A reply as the courier hands it on.
#[derive(Clone, Debug)]
pub struct Delivered {
    pub id: u64,
    pub req: TurnRequest,
    pub reply: TurnReply,
    /// The stand-in answered because the backend failed.
    pub fell_back: Option<String>,
}

impl Courier {
    pub fn new(backend: Box<dyn CommitteeBackend>, timeout: Duration) -> Self {
        Courier { backend, mock: MockCommittee::default(), open: HashMap::new(), next: 1, timeout, errors: Vec::new(), fallbacks: 0, calls: 0 }
    }

    pub fn mock() -> Self {
        Self::new(Box::new(MockCommittee::default()), Duration::from_secs(60))
    }

    pub fn backend_name(&self) -> String {
        self.backend.name()
    }

    pub fn is_mock(&self) -> bool {
        self.backend.is_mock()
    }

    /// Swap the backend; requests still out are answered by the stand-in.
    pub fn set_backend(&mut self, backend: Box<dyn CommitteeBackend>) {
        self.backend = backend;
        for (_, (_, at)) in self.open.iter_mut() {
            *at = Instant::now() - self.timeout - Duration::from_secs(1);
        }
    }

    pub fn submit(&mut self, req: TurnRequest) -> u64 {
        let id = self.next;
        self.next += 1;
        self.calls += 1;
        self.backend.submit(id, req.clone());
        self.open.insert(id, (req, Instant::now()));
        id
    }

    pub fn pending(&self) -> usize {
        self.open.len()
    }

    fn note_error(&mut self, e: String) {
        self.errors.push(e);
        if self.errors.len() > 8 {
            self.errors.remove(0);
        }
    }

    /// Replies ready now (the stand-in's for failures and timeouts).
    pub fn poll(&mut self) -> Vec<Delivered> {
        let mut out = Vec::new();
        for (id, res) in self.backend.poll() {
            let Some((req, _)) = self.open.remove(&id) else { continue };
            match res {
                Ok(reply) if !req.task.wants_json() || reply.json.is_some() => out.push(Delivered { id, req, reply, fell_back: None }),
                // [ask-committee] A question answered in prose (or broken
                // JSON) is still the member's: `questions::parse_answer`
                // reads what it can.
                Ok(reply) if matches!(req.task, Task::Question { .. }) && !reply.text.trim().is_empty() => out.push(Delivered { id, req, reply, fell_back: None }),
                Ok(reply) if matches!(req.task, Task::Propose { .. } | Task::Chat { .. }) && reply.text.trim().is_empty() => {
                    let why = "the model gave an empty answer".to_string();
                    self.note_error(why.clone());
                    out.push(self.stand_in(id, req, why));
                }
                Ok(reply) => {
                    // JSON wanted and none found: the stand-in's choice, but
                    // the model's words kept for the transcript.
                    let why = "the model's answer had no JSON".to_string();
                    self.note_error(format!("{why}: {}", short(&reply.text)));
                    let mut d = self.stand_in(id, req, why);
                    if !reply.text.trim().is_empty() && !matches!(d.req.task, Task::Vote { .. } | Task::Pick { .. }) {
                        d.reply.text = reply.text;
                    }
                    out.push(d);
                }
                Err(e) => {
                    self.note_error(e.clone());
                    out.push(self.stand_in(id, req, e));
                }
            }
        }
        let late: Vec<u64> = self.open.iter().filter(|(_, (_, at))| at.elapsed() > self.timeout).map(|(id, _)| *id).collect();
        for id in late {
            let Some((req, _)) = self.open.remove(&id) else { continue };
            // [fix/review-app] Not sent later by a backend still queueing it.
            self.backend.cancel(id);
            let why = format!("no answer within {} s", self.timeout.as_secs());
            self.note_error(why.clone());
            out.push(self.stand_in(id, req, why));
        }
        out.sort_by_key(|d| d.id);
        out
    }

    fn stand_in(&mut self, id: u64, req: TurnRequest, why: String) -> Delivered {
        self.fallbacks += 1;
        let reply = self.mock.answer(&req);
        Delivered { id, req, reply, fell_back: Some(why) }
    }
}

fn short(s: &str) -> String {
    let t: String = s.chars().take(80).collect();
    if s.chars().count() > 80 {
        format!("{t}...")
    } else {
        t
    }
}
