//! Citizen minds. `RuleBasedMind` is cheap and answers for everyone.
//! `LlmMind` routes spotlight citizens through a pluggable, asynchronous
//! backend and falls back to the rules while an answer is pending.

use nd_sim::mind::{Decision, Intent, MindContext, MindProvider};
use nd_sim::citizens::{EXHAUSTED, TIRED};
use nd_sim::{CitizenId, LandUse, LifeStage, Resource, SimRng};
use rand::seq::IndexedRandom;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// [committee] The committee's text backends and the meeting driver.
pub mod committee;

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

/// [carlife] What the commute has been like, when it has been bad: the
/// sim's `Commute` memory (and it is the sim that changes how they travel).
pub fn commute_thought(c: &nd_sim::Citizen) -> Option<&'static str> {
    use nd_sim::citizens::TripMode;
    let p = &c.commute;
    if c.car.is_some() && p.parking_pain > 1.0 {
        return Some("Another morning circling for a parking space. Maybe the bike.");
    }
    if p.pain_of(TripMode::Car) > 1.5 {
        return Some("Traffic again. There has to be a better way in.");
    }
    if p.pain_of(TripMode::Bus).max(p.pain_of(TripMode::Train)) > 1.5 {
        return Some("If the bus shows up at all. I should get a car.");
    }
    if p.pain_of(TripMode::Walk) > 1.5 && !c.owns_bike {
        return Some("It's a long walk in. A bike would change everything.");
    }
    None
}

// ── rule-based mind ──────────────────────────────────────────────────────────

#[derive(Clone)]
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
const HUNGRY_THOUGHTS: &[&str] = &[
    "Fridge is empty. Groceries run.",
    "Stomach's growling. Need food.",
    "Ration packs are gone. Shopping.",
];
const LUNCH_THOUGHTS: &[&str] = &["Grabbing a bite.", "Synth-bun and a coffee.", "Lunch break. Finally."];
const GOODS_THOUGHTS: &[&str] = &["Credits burning a hole. New optics?", "Treat yourself. The feed says so."];
const TIRED_THOUGHTS: &[&str] = &[
    "Dead on my feet. Bed.",
    "Eyes won't stay open. Home.",
    "Running on fumes. Need to crash.",
];
const DINNER_THOUGHTS: &[&str] = &["Dinner's waiting at home.", "Something in the pantry with my name on it.", "Home to eat."];
const PANTRY_THOUGHTS: &[&str] = &["Stocking up for the family.", "Pantry's getting thin. Big shop.", "Feeding a household is a full-time job."];
const KID_THOUGHTS: &[&str] = &["Nothing to do. Bored.", "Wish there was a school round here.", "Gonna go see the trees."];
const RETIREE_THOUGHTS: &[&str] = &["Slow morning. My right, at my age.", "A walk does the knees good.", "Seen this city change a lot."];
const EXPLORE_THOUGHTS: &[&str] = &[
    "Wandering, looking for a drink somewhere new.",
    "Same old bars. Let's find a new one.",
    "Heard there's a place round the corner. Let's see.",
];
const NO_HOME_THOUGHTS: &[&str] = &[
    "No place to crash yet. Keep moving.",
    "Someone's gotta have a free bunk.",
];

impl RuleBasedMind {
    fn pick(&mut self, list: &[&str]) -> String {
        list.choose(&mut self.rng).unwrap().to_string()
    }

    /// [places] A night out: the regular spot most of the time, somewhere
    /// new for the curious (and for anyone who knows nowhere yet), else
    /// wherever appeals.
    fn night_out(&mut self, ctx: &MindContext, p: &Personality) -> Option<Decision> {
        let decide = |intent: Intent, thought: String| Some(Decision { intent, thought, memory: None });
        if let Some(name) = ctx.regular_spot {
            if self.rng.random::<f32>() < 0.75 - 0.3 * p.curiosity {
                return decide(Intent::Visit(LandUse::Entertainment), format!("{name} tonight. My usual."));
            }
        }
        if ctx.places_known == 0 || self.rng.random::<f32>() < 0.35 * p.curiosity {
            let t = self.pick(EXPLORE_THOUGHTS);
            return decide(Intent::Explore(LandUse::Entertainment), t);
        }
        let t = self.pick(PARTY_THOUGHTS);
        decide(Intent::Visit(LandUse::Entertainment), t)
    }
}

impl MindProvider for RuleBasedMind {
    fn decide(&mut self, ctx: &MindContext) -> Option<Decision> {
        let p = Personality::from_seed(ctx.citizen.seed);
        let clock = ctx.clock;
        let decide = |intent: Intent, thought: String| Some(Decision { intent, thought, memory: None });

        if !ctx.has_home {
            // [r4-hunger] No roof, but food first: a grown-up with nothing
            // left in the bag buys groceries (eaten wherever they are). They
            // only ever wandered, looking for a bunk, and starved.
            if ctx.satiety < 0.45 && ctx.groceries < 1.0 && ctx.stage != LifeStage::Child && ctx.groceries_for_sale && !clock.is_night() {
                let t = self.pick(HUNGRY_THOUGHTS);
                return decide(Intent::Shop(Resource::Groceries), t);
            }
            let t = self.pick(NO_HOME_THOUGHTS);
            return decide(Intent::Wander, t);
        }

        // Asleep: nothing to decide until morning.
        if ctx.asleep {
            return decide(Intent::Stay, String::new());
        }
        // Sleep comes first: the tired go home, unless mid-shift and not
        // yet exhausted (they finish the shift, badly).
        if ctx.wakefulness < TIRED && !(ctx.on_shift && ctx.wakefulness >= EXHAUSTED * 0.5) {
            if ctx.at_home {
                return decide(Intent::Stay, String::new());
            }
            let t = self.pick(TIRED_THOUGHTS);
            return decide(Intent::GoHome, t);
        }

        // Food next: an empty pantry means a shop; a hungry stomach with
        // food at home means dinner at home.
        let grown = ctx.stage != LifeStage::Child;
        let hungry = ctx.satiety < 0.45 && ctx.groceries < 1.0;
        if hungry && grown && ctx.groceries_for_sale && !clock.is_night() && self.rng.random_bool(0.8) {
            let t = self.pick(HUNGRY_THOUGHTS);
            return decide(Intent::Shop(Resource::Groceries), t);
        }
        if ctx.satiety < 0.4 && ctx.groceries >= 1.0 && !ctx.at_home && !ctx.on_shift {
            // The sociable eat out: venues feed their guests.
            let p_out = (p.hedonism * 0.5 + p.sociability * 0.3) * 0.8;
            if grown && clock.is_evening() && clock.hour() >= 18 && ctx.available[LandUse::Entertainment.index()] && self.rng.random::<f32>() < p_out {
                return decide(Intent::Visit(LandUse::Entertainment), "Dinner out tonight.".to_string());
            }
            let t = self.pick(DINNER_THOUGHTS);
            return decide(Intent::GoHome, t);
        }
        if !ctx.at_home && (12..14).contains(&clock.hour()) && ctx.satiety < 0.6 && ctx.meals_for_sale && self.rng.random_bool((0.3 + p.hedonism * 0.4) as f64) {
            let t = self.pick(LUNCH_THOUGHTS);
            return decide(Intent::Shop(Resource::BakedGoods), t);
        }
        // Someone has to keep the pantry full for the whole household.
        if grown && ctx.pantry_low && !ctx.on_shift && ctx.groceries_for_sale && (7..21).contains(&clock.hour()) && self.rng.random_bool(0.5) {
            let t = self.pick(PANTRY_THOUGHTS);
            return decide(Intent::Shop(Resource::Groceries), t);
        }

        // [r4-leisure-places] A gym-goer's morning: the gym before work.
        if ctx.workout && grown && ctx.at_home && !ctx.on_shift && ctx.available[LandUse::Entertainment.index()] {
            return decide(Intent::Visit(LandUse::Entertainment), "Gym before work.".to_string());
        }
        // Shifts follow the workplace's opening hours.
        if ctx.on_shift {
            let t = commute_thought(ctx.citizen).map(str::to_string).unwrap_or_else(|| self.pick(WORK_THOUGHTS));
            return decide(Intent::GoWork, t);
        }

        // Children and retirees have their days to themselves. (Schools
        // would go here: `Citizen::goes_to_school` marks who would attend.)
        if ctx.stage != LifeStage::Adult && !clock.is_night() && !clock.is_evening() && clock.hour() >= 8 {
            let roll: f32 = self.rng.random();
            if roll < 0.25 && ctx.available[LandUse::Park.index()] {
                let t = self.pick(if ctx.stage == LifeStage::Child { KID_THOUGHTS } else { RETIREE_THOUGHTS });
                return decide(Intent::Visit(LandUse::Park), t);
            }
            if roll < 0.35 && ctx.stage == LifeStage::Retiree {
                let t = self.pick(RETIREE_THOUGHTS);
                return decide(Intent::Wander, t);
            }
            // [r4-leisure-places] A matinee, a diner lunch, a game of bowls.
            if roll < 0.42 && ctx.stage == LifeStage::Retiree && clock.hour() >= 10 && ctx.available[LandUse::Entertainment.index()] {
                return decide(Intent::Visit(LandUse::Entertainment), "A matinee, then a bite.".to_string());
            }
            let t = self.pick(if ctx.stage == LifeStage::Child { KID_THOUGHTS } else { HOME_THOUGHTS });
            return decide(Intent::GoHome, t);
        }

        if clock.is_night() {
            if grown && p.hedonism > 0.75 && ctx.wakefulness > 0.25 && ctx.available[LandUse::Entertainment.index()] && self.rng.random_bool(0.25) {
                return self.night_out(ctx, &p);
            }
            let t = self.pick(NIGHT_THOUGHTS);
            return decide(Intent::GoHome, t);
        }

        if clock.is_work_hours() {
            if self.rng.random_bool((0.3 + p.curiosity * 0.4) as f64) {
                let t = self.pick(WANDER_THOUGHTS);
                return decide(Intent::Wander, t);
            }
            let t = self.pick(HOME_THOUGHTS);
            return decide(Intent::GoHome, t);
        }

        if clock.is_evening() {
            let roll: f32 = self.rng.random();
            // [places] Having a regular spot is a reason to go out.
            let habit = if ctx.regular_spot.is_some() { 0.06 } else { 0.0 };
            let p_party = (p.hedonism * 0.5 + p.sociability * 0.3) * 0.8 + habit;
            let p_shop = 0.25;
            let p_park = p.caution * 0.2;
            if roll < p_party && grown && ctx.available[LandUse::Entertainment.index()] {
                return self.night_out(ctx, &p);
            }
            if roll < p_party + p_shop && ctx.groceries_for_sale {
                let t = self.pick(SHOP_THOUGHTS);
                return decide(Intent::Shop(Resource::Groceries), t);
            }
            if ctx.citizen.savings > 250.0 && p.hedonism > 0.5 && self.rng.random_bool(0.08) {
                let t = self.pick(GOODS_THOUGHTS);
                return decide(Intent::Shop(Resource::Goods), t);
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
    /// [r7-sim-thread] A copy with the same requests in hand, when the
    /// backend can make one (a model behind a connection cannot): the game
    /// steps a copy of the city ahead on its sim thread and throws the copy
    /// away when the player edits the city first, so the mind that copy
    /// asked must be thrown away with it.
    fn try_clone(&self) -> Option<Box<dyn MindBackend>> {
        None
    }
}

/// Answers after a few polls with a canned, personality-flavoured line. Lets
/// the whole spotlight pipeline be exercised without a model.
#[derive(Clone)]
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

    fn try_clone(&self) -> Option<Box<dyn MindBackend>> {
        Some(Box::new(self.clone()))
    }
}

/// Rules for everyone, backend for spotlight citizens. Spotlight citizens
/// act on the rule-based decision immediately and get their thought (and
/// optionally intent) replaced when the model answers.
pub struct HybridMind {
    rules: RuleBasedMind,
    backend: Box<dyn MindBackend>,
    pending: HashMap<RequestId, CitizenId>,
    /// Each spotlight citizen's open request and the tick it was asked.
    in_flight: HashMap<CitizenId, (RequestId, u64)>,
    next_request: RequestId,
}

/// Ticks after which an unanswered request is given up and the citizen may
/// be asked about again (a backend that drops a request would otherwise
/// silence that citizen for good). Its answer, if it still comes, is
/// dropped.
pub const REQUEST_TTL: u64 = 240;

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

    /// [r7-sim-thread] An exact copy (see [`MindBackend::try_clone`]);
    /// None when the backend cannot be copied.
    pub fn try_clone(&self) -> Option<HybridMind> {
        Some(HybridMind { rules: self.rules.clone(), backend: self.backend.try_clone()?, pending: self.pending.clone(), in_flight: self.in_flight.clone(), next_request: self.next_request })
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
            options: ["Stay", "GoHome", "GoWork", "Shop(Groceries)", "Shop(BakedGoods)", "Shop(Goods)", "Visit(Entertainment)", "Visit(Park)", "Explore(Entertainment)", "Wander"]
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
            "Explore(Entertainment)" => Intent::Explore(LandUse::Entertainment),
            "Explore(Park)" => Intent::Explore(LandUse::Park),
            "Explore(Commercial)" => Intent::Explore(LandUse::Commercial),
            "Shop(Groceries)" => Intent::Shop(Resource::Groceries),
            "Shop(BakedGoods)" => Intent::Shop(Resource::BakedGoods),
            "Shop(Goods)" => Intent::Shop(Resource::Goods),
            _ => return None,
        })
    }
}

impl MindProvider for HybridMind {
    fn decide(&mut self, ctx: &MindContext) -> Option<Decision> {
        let base = self.rules.decide(ctx);
        let now = ctx.clock.tick;
        if let Some(&(old, asked)) = self.in_flight.get(&ctx.id) {
            if now.saturating_sub(asked) > REQUEST_TTL {
                self.pending.remove(&old);
                self.in_flight.remove(&ctx.id);
            }
        }
        if ctx.citizen.spotlight && !self.in_flight.contains_key(&ctx.id) {
            let id = self.next_request;
            self.next_request += 1;
            self.pending.insert(id, ctx.id);
            self.in_flight.insert(ctx.id, (id, now));
            self.backend.submit(id, Self::build_prompt(ctx));
        }
        base
    }

    fn poll(&mut self) -> Vec<(CitizenId, Decision)> {
        let mut out = Vec::new();
        for (req, answer) in self.backend.poll() {
            let Some(citizen) = self.pending.remove(&req) else { continue };
            if self.in_flight.get(&citizen).is_some_and(|f| f.0 == req) {
                self.in_flight.remove(&citizen);
            }
            let intent = Self::parse_intent(&answer.intent).unwrap_or(Intent::Stay);
            out.push((citizen, Decision { intent, thought: answer.thought, memory: answer.memory }));
        }
        out
    }

    /// Request ids keep counting up, so answers to anything asked before
    /// the reset find no pending entry and are dropped.
    fn reset(&mut self) {
        self.pending.clear();
        self.in_flight.clear();
    }

    fn name(&self) -> &'static str {
        "hybrid"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nd_sim::citizens::{Activity, Citizen, TravelMode};
    use nd_sim::{SimClock, Vec2};

    fn person(age: u8) -> Citizen {
        Citizen {
            name: "Test".into(),
            seed: 3,
            home: None,
            work: None,
            pos: Vec2::ZERO,
            heading: 0.0,
            prev_pos: Vec2::ZERO,
            prev_heading: 0.0,
            mode: TravelMode::Walk,
            owns_car: false,
            owns_bike: false,
            vehicle: None,
            parked_at: None,
            transit: None,
            activity: Activity::Idle,
            memories: Vec::new(),
            places: Default::default(),
            thought: String::new(),
            spotlight: false,
            next_decision_tick: 0,
            arrived_tick: 0,
            inside: None,
            mood: 0.5,
            income: 80.0,
            savings: 20.0,
            rent_stress_days: 0,
            satiety: 0.9,
            groceries: 5.0,
            hungry_days: 0,
            trail: Vec::new(),
            trail_stops: Default::default(),
            pending_footfall: Vec::new(),
            household: None,
            age,
            wakefulness: 0.8,
            asleep: false,
            packed_meal: 0.0,
            drunk: 0.0,
            car: None,
            to_car: None,
            commute: Default::default(),
            look: None,
        }
    }

    struct Case {
        hour: u64,
        stage_age: u8,
        wakefulness: f32,
        asleep: bool,
        at_home: bool,
        on_shift: bool,
        satiety: f32,
        groceries: f32,
        pantry_low: bool,
        regular_spot: Option<&'static str>,
        places_known: usize,
        /// [r4-hunger] No home.
        homeless: bool,
        /// [r4-leisure-places] A gym-goer's morning.
        workout: bool,
    }

    impl Default for Case {
        fn default() -> Self {
            Self { hour: 10, stage_age: 35, wakefulness: 0.8, asleep: false, at_home: false, on_shift: false, satiety: 0.9, groceries: 5.0, pantry_low: false, regular_spot: None, places_known: 0, homeless: false, workout: false }
        }
    }

    /// The intents the mind picks for a case over many rolls.
    fn intents(case: Case) -> Vec<Intent> {
        let mut c = person(case.stage_age);
        c.wakefulness = case.wakefulness;
        c.asleep = case.asleep;
        c.satiety = case.satiety;
        c.groceries = case.groceries;
        let clock = SimClock::starting_at(2, case.hour, 0);
        let mut mind = RuleBasedMind::default();
        (0..60)
            .filter_map(|_| {
                let ctx = MindContext {
                    id: CitizenId::default(),
                    citizen: &c,
                    clock: &clock,
                    has_home: !case.homeless,
                    has_work: true,
                    home_name: Some("Home"),
                    work_name: Some("Work"),
                    available: [true; 6],
                    population: 100,
                    satiety: case.satiety,
                    groceries: case.groceries,
                    groceries_for_sale: true,
                    meals_for_sale: true,
                    stage: c.stage(),
                    wakefulness: case.wakefulness,
                    asleep: case.asleep,
                    at_home: case.at_home,
                    on_shift: case.on_shift,
                    household_size: 3,
                    pantry_low: case.pantry_low,
                    regular_spot: case.regular_spot,
                    places_known: case.places_known,
                    workout: case.workout,
                };
                mind.decide(&ctx).map(|d| d.intent)
            })
            .collect()
    }

    #[test]
    fn the_asleep_stay_asleep_and_the_tired_go_home() {
        assert!(intents(Case { hour: 2, asleep: true, at_home: true, ..Default::default() }).iter().all(|i| *i == Intent::Stay));
        assert!(intents(Case { hour: 19, wakefulness: 0.1, ..Default::default() }).iter().all(|i| *i == Intent::GoHome), "tired out: home to bed");
        assert!(intents(Case { hour: 15, wakefulness: 0.1, on_shift: true, ..Default::default() }).iter().all(|i| *i == Intent::GoWork), "tired mid-shift: finish it");
        assert!(intents(Case { hour: 15, wakefulness: 0.01, on_shift: true, ..Default::default() }).iter().all(|i| *i == Intent::GoHome), "exhausted: even the shift waits");
    }

    /// [r4-hunger] With no home they look for a bunk, but a hungry grown-up
    /// with nothing left in the bag buys groceries first (they only ever
    /// wandered, and starved).
    #[test]
    fn the_homeless_buy_food_when_the_bag_is_empty() {
        let fed = intents(Case { homeless: true, ..Default::default() });
        assert!(fed.iter().all(|i| *i == Intent::Wander), "fed: looking for a bunk");
        let bag = intents(Case { homeless: true, satiety: 0.3, groceries: 3.0, ..Default::default() });
        assert!(bag.iter().all(|i| *i == Intent::Wander), "food in the bag: eaten where they are");
        let empty = intents(Case { homeless: true, satiety: 0.3, groceries: 0.0, ..Default::default() });
        assert!(empty.iter().all(|i| *i == Intent::Shop(Resource::Groceries)), "hungry, nothing left: shopping");
        let night = intents(Case { homeless: true, hour: 2, satiety: 0.3, groceries: 0.0, ..Default::default() });
        assert!(night.iter().all(|i| *i == Intent::Wander), "not at night");
        let kid = intents(Case { homeless: true, stage_age: 9, satiety: 0.3, groceries: 0.0, ..Default::default() });
        assert!(kid.iter().all(|i| *i == Intent::Wander), "children do not do the shopping");
    }

    #[test]
    fn shifts_follow_the_workplace_not_the_clock() {
        assert!(intents(Case { hour: 20, on_shift: true, ..Default::default() }).iter().all(|i| *i == Intent::GoWork), "an evening shift at a venue");
        assert!(intents(Case { hour: 10, on_shift: false, ..Default::default() }).iter().all(|i| *i != Intent::GoWork), "off shift in office hours");
    }

    #[test]
    fn food_comes_home_and_someone_fills_the_pantry() {
        let dinner = intents(Case { hour: 19, satiety: 0.3, groceries: 4.0, ..Default::default() });
        assert!(dinner.iter().all(|i| matches!(i, Intent::GoHome | Intent::Visit(LandUse::Entertainment))), "hungry with food at home: dinner, at home or out");
        assert!(dinner.contains(&Intent::GoHome));
        let empty = intents(Case { hour: 11, satiety: 0.3, groceries: 0.0, ..Default::default() });
        assert!(empty.iter().filter(|i| **i == Intent::Shop(Resource::Groceries)).count() > 30);
        let pantry = intents(Case { hour: 16, pantry_low: true, ..Default::default() });
        assert!(pantry.iter().any(|i| *i == Intent::Shop(Resource::Groceries)), "a low pantry sends a grown-up shopping");
        let kid = intents(Case { hour: 16, stage_age: 9, pantry_low: true, satiety: 0.3, groceries: 0.0, ..Default::default() });
        assert!(kid.iter().all(|i| *i != Intent::Shop(Resource::Groceries)), "children do not do the shopping");
    }

    #[test]
    fn strangers_go_looking_and_regulars_go_back() {
        let out = |i: &Intent| matches!(i, Intent::Visit(LandUse::Entertainment) | Intent::Explore(LandUse::Entertainment));
        let stranger = intents(Case { hour: 20, ..Default::default() });
        assert!(stranger.iter().filter(|i| out(i)).all(|i| *i == Intent::Explore(LandUse::Entertainment)), "knowing nowhere, a night out is a search");
        let regular = intents(Case { hour: 20, regular_spot: Some("Velvet Den"), places_known: 3, ..Default::default() });
        let n = |v: &[Intent], i: Intent| v.iter().filter(|x| **x == i).count();
        assert!(regular.iter().filter(|i| out(i)).count() >= stranger.iter().filter(|i| out(i)).count(), "a regular spot is a reason to go out");
        assert!(n(&regular, Intent::Visit(LandUse::Entertainment)) > n(&regular, Intent::Explore(LandUse::Entertainment)), "mostly back to the usual");
    }

    #[test]
    fn children_and_retirees_never_go_to_work() {
        for age in [8, 75] {
            let days = intents(Case { hour: 10, stage_age: age, ..Default::default() });
            assert!(days.iter().all(|i| *i != Intent::GoWork));
            assert!(days.iter().all(|i| *i != Intent::Visit(LandUse::Entertainment)) || age > 17);
        }
    }

    /// [r4-leisure-places] Gym-goers head out from home before work; the
    /// rest (and anyone on shift) do not.
    #[test]
    fn gym_goers_work_out_before_work() {
        let gym = intents(Case { hour: 6, at_home: true, workout: true, ..Default::default() });
        assert!(gym.iter().all(|i| *i == Intent::Visit(LandUse::Entertainment)));
        let rest = intents(Case { hour: 6, at_home: true, ..Default::default() });
        assert!(rest.iter().all(|i| *i != Intent::Visit(LandUse::Entertainment)));
        let shift = intents(Case { hour: 6, at_home: true, workout: true, on_shift: true, ..Default::default() });
        assert!(shift.iter().all(|i| *i == Intent::GoWork));
    }

    /// [r4-leisure-places] Retirees have their days: some go out to a
    /// matinee or a diner lunch.
    #[test]
    fn retirees_sometimes_go_out_by_day() {
        let days = intents(Case { hour: 14, stage_age: 72, ..Default::default() });
        assert!(days.iter().any(|i| *i == Intent::Visit(LandUse::Entertainment)));
    }
}
