//! Citizen minds. `RuleBasedMind` is cheap and answers for everyone.
//! `LlmMind` routes spotlight citizens through a pluggable, asynchronous
//! backend and falls back to the rules while an answer is pending.

use nd_sim::mind::{Decision, Intent, MindContext, MindProvider};
use nd_sim::{CitizenId, LandUse, SimRng};
use rand::seq::IndexedRandom;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── personality ──────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Personality {
    pub sociability: f32,
    pub ambition: f32,
    pub hedonism: f32,
    pub caution: f32,
    pub curiosity: f32,
    pub archetype: &'static str,
}

impl Personality {
    pub fn from_seed(seed: u64) -> Self {
        let mut rng = SimRng::seed_from_u64(seed);
        let t: [f32; 5] = [rng.random(), rng.random(), rng.random(), rng.random(), rng.random()];
        let names = ["Socialite", "Corpo", "Nightcrawler", "Survivor", "Tinkerer"];
        let dominant = (0..5).max_by(|&a, &b| t[a].total_cmp(&t[b])).unwrap();
        Self {
            sociability: t[0],
            ambition: t[1],
            hedonism: t[2],
            caution: t[3],
            curiosity: t[4],
            archetype: names[dominant],
        }
    }

    pub fn summary(&self) -> String {
        format!(
            "{} · social {:.0}% · ambition {:.0}% · hedonism {:.0}% · caution {:.0}% · curiosity {:.0}%",
            self.archetype,
            self.sociability * 100.0,
            self.ambition * 100.0,
            self.hedonism * 100.0,
            self.caution * 100.0,
            self.curiosity * 100.0
        )
    }
}

// ── rule-based mind ──────────────────────────────────────────────────────────

pub struct RuleBasedMind {
    rng: SimRng,
}

impl Default for RuleBasedMind {
    fn default() -> Self {
        Self { rng: SimRng::seed_from_u64(0xC0FFEE) }
    }
}

const NIGHT_THOUGHTS: &[&str] = &[
    "Time to jack out for the night.",
    "Rain on the window. Sleep.",
    "The grid never sleeps, but I do.",
    "Home. Lock the door twice.",
];
const WORK_THOUGHTS: &[&str] = &[
    "Shift starts. Credits don't earn themselves.",
    "Another day feeding the machine.",
    "Badge in, brain off.",
    "If I'm late again they'll dock me.",
];
const PARTY_THOUGHTS: &[&str] = &[
    "The night is young and so is my credit line.",
    "Neon Alley is calling.",
    "Bass. Lights. Forget the week.",
    "One drink. Maybe three.",
];
const SHOP_THOUGHTS: &[&str] = &[
    "Out of synth-noodles again.",
    "Need a new optic. The left one flickers.",
    "Grab supplies before the stores close.",
];
const PARK_THOUGHTS: &[&str] = &[
    "Need air that isn't recycled.",
    "The park. Real trees, mostly.",
];
const HOME_THOUGHTS: &[&str] = &[
    "Nothing out there tonight. Staying in.",
    "Feed's on. Door's locked.",
    "Quiet evening. Rare.",
];
const WANDER_THOUGHTS: &[&str] = &[
    "Walking the grid, seeing what changed.",
    "New towers going up every week.",
    "Where does this street even go?",
];
const NO_HOME_THOUGHTS: &[&str] = &[
    "No place to crash yet. Keep moving.",
    "Someone's gotta have a free bunk.",
];

impl RuleBasedMind {
    fn pick(&mut self, list: &[&str]) -> String {
        list.choose(&mut self.rng).unwrap().to_string()
    }
}

impl MindProvider for RuleBasedMind {
    fn decide(&mut self, ctx: &MindContext) -> Option<Decision> {
        let p = Personality::from_seed(ctx.citizen.seed);
        let clock = ctx.clock;
        let decide = |intent: Intent, thought: String| Some(Decision { intent, thought, memory: None });

        if !ctx.has_home {
            let t = self.pick(NO_HOME_THOUGHTS);
            return decide(Intent::Wander, t);
        }

        if clock.is_night() {
            if p.hedonism > 0.75 && ctx.available[LandUse::Entertainment.index()] && self.rng.random_bool(0.25) {
                let t = self.pick(PARTY_THOUGHTS);
                return decide(Intent::Visit(LandUse::Entertainment), t);
            }
            let t = self.pick(NIGHT_THOUGHTS);
            return decide(Intent::GoHome, t);
        }

        if clock.is_work_hours() {
            if ctx.has_work {
                let t = self.pick(WORK_THOUGHTS);
                return decide(Intent::GoWork, t);
            }
            if self.rng.random_bool((0.3 + p.curiosity * 0.4) as f64) {
                let t = self.pick(WANDER_THOUGHTS);
                return decide(Intent::Wander, t);
            }
            let t = self.pick(HOME_THOUGHTS);
            return decide(Intent::GoHome, t);
        }

        if clock.is_evening() {
            let roll: f32 = self.rng.random();
            let p_party = (p.hedonism * 0.5 + p.sociability * 0.3) * 0.8;
            let p_shop = 0.25;
            let p_park = p.caution * 0.2;
            if roll < p_party && ctx.available[LandUse::Entertainment.index()] {
                let t = self.pick(PARTY_THOUGHTS);
                return decide(Intent::Visit(LandUse::Entertainment), t);
            }
            if roll < p_party + p_shop && ctx.available[LandUse::Commercial.index()] {
                let t = self.pick(SHOP_THOUGHTS);
                return decide(Intent::Visit(LandUse::Commercial), t);
            }
            if roll < p_party + p_shop + p_park && ctx.available[LandUse::Park.index()] {
                let t = self.pick(PARK_THOUGHTS);
                return decide(Intent::Visit(LandUse::Park), t);
            }
            let t = self.pick(HOME_THOUGHTS);
            return decide(Intent::GoHome, t);
        }

        // Early morning.
        decide(Intent::Stay, String::new())
    }

    fn name(&self) -> &'static str {
        "rules"
    }
}

// ── LLM mind: backend trait, mock backend, hybrid provider ──────────────────

/// A prompt for a spotlight citizen. Backends turn this into whatever their
/// model wants; the structure keeps the game in control of what gets asked.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MindPrompt {
    pub citizen_name: String,
    pub personality: String,
    pub time: String,
    pub day: u64,
    pub home: Option<String>,
    pub work: Option<String>,
    pub activity: String,
    pub recent_memories: Vec<String>,
    pub population: u32,
    pub options: Vec<String>,
}

/// Structured answer we accept from a model.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MindAnswer {
    pub intent: String,
    pub thought: String,
    pub memory: Option<String>,
}

pub type RequestId = u64;

/// Anything that can answer prompts asynchronously: a mock, a local model, an
/// HTTP API. The sim thread only ever calls `submit` and `poll`.
pub trait MindBackend: Send + Sync {
    fn submit(&mut self, id: RequestId, prompt: MindPrompt);
    fn poll(&mut self) -> Vec<(RequestId, MindAnswer)>;
    fn name(&self) -> &'static str;
}

/// Answers after a few polls with a canned, personality-flavoured line. Lets
/// the whole spotlight pipeline be exercised without a model.
pub struct MockBackend {
    queue: Vec<(RequestId, MindPrompt, u8)>,
}

impl Default for MockBackend {
    fn default() -> Self {
        Self { queue: Vec::new() }
    }
}

impl MindBackend for MockBackend {
    fn submit(&mut self, id: RequestId, prompt: MindPrompt) {
        self.queue.push((id, prompt, 3));
    }

    fn poll(&mut self) -> Vec<(RequestId, MindAnswer)> {
        let mut ready = Vec::new();
        self.queue.retain_mut(|(id, prompt, countdown)| {
            if *countdown > 0 {
                *countdown -= 1;
                return true;
            }
            let last = prompt.recent_memories.last().cloned().unwrap_or_default();
            ready.push((
                *id,
                MindAnswer {
                    intent: "Stay".to_string(),
                    thought: format!(
                        "[mock llm] {} thinks about {} at {} on day {}.",
                        prompt.citizen_name,
                        if last.is_empty() { "nothing much".to_string() } else { last },
                        prompt.time,
                        prompt.day + 1
                    ),
                    memory: None,
                },
            ));
            false
        });
        ready
    }

    fn name(&self) -> &'static str {
        "mock"
    }
}

/// Rules for everyone, backend for spotlight citizens. Spotlight citizens
/// act on the rule-based decision immediately and get their thought (and
/// optionally intent) replaced when the model answers.
pub struct HybridMind {
    rules: RuleBasedMind,
    backend: Box<dyn MindBackend>,
    pending: HashMap<RequestId, CitizenId>,
    in_flight: HashMap<CitizenId, RequestId>,
    next_request: RequestId,
}

impl HybridMind {
    pub fn new(backend: Box<dyn MindBackend>) -> Self {
        Self {
            rules: RuleBasedMind::default(),
            backend,
            pending: HashMap::new(),
            in_flight: HashMap::new(),
            next_request: 1,
        }
    }

    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }

    fn build_prompt(ctx: &MindContext) -> MindPrompt {
        let p = Personality::from_seed(ctx.citizen.seed);
        MindPrompt {
            citizen_name: ctx.citizen.name.clone(),
            personality: p.summary(),
            time: ctx.clock.format_hhmm(),
            day: ctx.clock.day(),
            home: ctx.home_name.map(str::to_string),
            work: ctx.work_name.map(str::to_string),
            activity: ctx.citizen.activity.label().to_string(),
            recent_memories: ctx.citizen.memories.iter().rev().take(6).map(|m| m.text.clone()).collect(),
            population: ctx.population,
            options: ["Stay", "GoHome", "GoWork", "Visit(Commercial)", "Visit(Entertainment)", "Visit(Park)", "Wander"]
                .iter()
                .map(|s| s.to_string())
                .collect(),
        }
    }

    fn parse_intent(s: &str) -> Option<Intent> {
        Some(match s.trim() {
            "Stay" => Intent::Stay,
            "GoHome" => Intent::GoHome,
            "GoWork" => Intent::GoWork,
            "Wander" => Intent::Wander,
            "Visit(Commercial)" => Intent::Visit(LandUse::Commercial),
            "Visit(Entertainment)" => Intent::Visit(LandUse::Entertainment),
            "Visit(Park)" => Intent::Visit(LandUse::Park),
            "Visit(Civic)" => Intent::Visit(LandUse::Civic),
            _ => return None,
        })
    }
}

impl MindProvider for HybridMind {
    fn decide(&mut self, ctx: &MindContext) -> Option<Decision> {
        let base = self.rules.decide(ctx);
        if ctx.citizen.spotlight && !self.in_flight.contains_key(&ctx.id) {
            let id = self.next_request;
            self.next_request += 1;
            self.pending.insert(id, ctx.id);
            self.in_flight.insert(ctx.id, id);
            self.backend.submit(id, Self::build_prompt(ctx));
        }
        base
    }

    fn poll(&mut self) -> Vec<(CitizenId, Decision)> {
        let mut out = Vec::new();
        for (req, answer) in self.backend.poll() {
            let Some(citizen) = self.pending.remove(&req) else { continue };
            self.in_flight.remove(&citizen);
            let intent = Self::parse_intent(&answer.intent).unwrap_or(Intent::Stay);
            out.push((citizen, Decision { intent, thought: answer.thought, memory: answer.memory }));
        }
        out
    }

    fn name(&self) -> &'static str {
        "hybrid"
    }
}
