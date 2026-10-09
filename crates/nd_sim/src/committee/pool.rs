//! The pool of politicians: scopes, tempers and leanings, their memories,
//! and the pool generated (fictional names) for a new game.

use super::*;

/// What a member answers for: the rules that make them raise alerts, the
/// figures in their status report, the kind of proposal they table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Scope {
    Transit,
    Housing,
    Finance,
    Safety,
    Environment,
    Business,
    /// Power and roads.
    Utilities,
    /// Health and schools.
    Welfare,
}

impl Scope {
    pub const ALL: [Scope; 8] = [Scope::Transit, Scope::Housing, Scope::Finance, Scope::Safety, Scope::Environment, Scope::Business, Scope::Utilities, Scope::Welfare];

    pub fn key(self) -> &'static str {
        match self {
            Scope::Transit => "transit",
            Scope::Housing => "housing",
            Scope::Finance => "finance",
            Scope::Safety => "safety",
            Scope::Environment => "environment",
            Scope::Business => "business",
            Scope::Utilities => "utilities",
            Scope::Welfare => "welfare",
        }
    }

    pub fn from_key(s: &str) -> Option<Scope> {
        Scope::ALL.into_iter().find(|x| x.key().eq_ignore_ascii_case(s.trim()))
    }

    pub fn label(self) -> &'static str {
        match self {
            Scope::Transit => "Transit",
            Scope::Housing => "Housing",
            Scope::Finance => "Finance",
            Scope::Safety => "Public safety",
            Scope::Environment => "Environment",
            Scope::Business => "Business",
            Scope::Utilities => "Power and roads",
            Scope::Welfare => "Health and schools",
        }
    }

    /// The seat's title.
    pub fn title(self) -> &'static str {
        match self {
            Scope::Transit => "Transit commissioner",
            Scope::Housing => "Housing commissioner",
            Scope::Finance => "City treasurer",
            Scope::Safety => "Public safety commissioner",
            Scope::Environment => "Environment commissioner",
            Scope::Business => "Commerce commissioner",
            Scope::Utilities => "Public works commissioner",
            Scope::Welfare => "Health and education commissioner",
        }
    }

    /// The goal that comes with the job.
    pub fn goal(self) -> &'static str {
        match self {
            Scope::Transit => "more people on buses and trains, fewer cars stuck in traffic",
            Scope::Housing => "homes people can afford, nobody sleeping rough",
            Scope::Finance => "a treasury in the black",
            Scope::Safety => "less crime and every call answered",
            Scope::Environment => "clean ground and green space",
            Scope::Business => "jobs, shops and busy streets",
            Scope::Utilities => "lights on and roads in good repair",
            Scope::Welfare => "a clinic and a school within reach of every home",
        }
    }

    pub(super) const BACKGROUNDS: [[&'static str; 3]; 8] = [
        ["bus dispatcher", "rail signal engineer", "courier union organiser"],
        ["tenant organiser", "property developer", "housing co-op architect"],
        ["city auditor", "bond trader", "credit union manager"],
        ["retired police captain", "fire chief", "public defender"],
        ["soil chemist", "river warden", "solar co-op founder"],
        ["noodle-bar chain owner", "chamber of commerce president", "freight broker"],
        ["grid engineer", "road crew foreman", "utility regulator"],
        ["school principal", "emergency-room nurse", "clinic director"],
    ];

    pub(super) fn index(self) -> usize {
        self as usize
    }
}

/// How a member talks and how stubborn they are.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Temper {
    Firebrand,
    Technocrat,
    Populist,
    Pragmatist,
    Idealist,
    OldGuard,
    Contrarian,
    Dealmaker,
}

impl Temper {
    pub const ALL: [Temper; 8] = [Temper::Firebrand, Temper::Technocrat, Temper::Populist, Temper::Pragmatist, Temper::Idealist, Temper::OldGuard, Temper::Contrarian, Temper::Dealmaker];

    pub fn key(self) -> &'static str {
        match self {
            Temper::Firebrand => "firebrand",
            Temper::Technocrat => "technocrat",
            Temper::Populist => "populist",
            Temper::Pragmatist => "pragmatist",
            Temper::Idealist => "idealist",
            Temper::OldGuard => "old_guard",
            Temper::Contrarian => "contrarian",
            Temper::Dealmaker => "dealmaker",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Temper::Firebrand => "Firebrand",
            Temper::Technocrat => "Technocrat",
            Temper::Populist => "Populist",
            Temper::Pragmatist => "Pragmatist",
            Temper::Idealist => "Idealist",
            Temper::OldGuard => "Old guard",
            Temper::Contrarian => "Contrarian",
            Temper::Dealmaker => "Dealmaker",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            Temper::Firebrand => "loud, impatient, quick to blame",
            Temper::Technocrat => "dry, numbers first, distrusts slogans",
            Temper::Populist => "speaks for the street, suspicious of experts",
            Temper::Pragmatist => "calm, looks for what works this month",
            Temper::Idealist => "argues from principle, hates half-measures",
            Temper::OldGuard => "has seen it all before, wary of anything new",
            Temper::Contrarian => "takes the other side on purpose",
            Temper::Dealmaker => "trades votes, wants something back",
        }
    }
}

/// Where a member stands, each -1..1. They conflict on purpose: a growth
/// hawk on the Environment seat and a frugal Transit commissioner make for
/// a real argument.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Leanings {
    /// -1 frugal .. +1 spend freely.
    pub spending: f32,
    /// -1 green, slow growth .. +1 growth and jobs first.
    pub growth: f32,
    /// -1 liberties, lighter policing .. +1 law and order.
    pub order: f32,
    /// 0..1 how readily they follow the mayor.
    pub loyalty: f32,
}

impl Leanings {
    /// One line for the window and the prompts.
    pub fn describe(&self) -> String {
        let side = |v: f32, neg: &str, pos: &str| -> String {
            if v > 0.5 {
                format!("strongly {pos}")
            } else if v > 0.15 {
                pos.to_string()
            } else if v < -0.5 {
                format!("strongly {neg}")
            } else if v < -0.15 {
                neg.to_string()
            } else {
                String::new()
            }
        };
        let parts: Vec<String> = [side(self.spending, "frugal", "a big spender"), side(self.growth, "green", "pro-growth"), side(self.order, "civil-libertarian", "law-and-order")].into_iter().filter(|s| !s.is_empty()).collect();
        if parts.is_empty() {
            "a centrist".into()
        } else {
            parts.join(", ")
        }
    }
}

/// One thing a member remembers: how they voted on what, and how it went.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryEntry {
    pub day: u32,
    pub meeting: u32,
    /// The proposal's title ("Northside clinic").
    pub proposal: String,
    /// `Action::kind_key` of it.
    pub kind: String,
    pub vote: Option<Vote>,
    pub passed: bool,
    /// Whether they tabled it.
    pub proposed: bool,
    /// Their scope's headline figure when they voted, to judge the outcome
    /// by at the next meeting: (`CityReport::metric` key, value).
    pub metric: Option<(String, f32)>,
    /// What came of it: "implemented on day 64", "crime down 18% since".
    pub outcome: String,
    /// +1 it went well by their figure, -1 badly (or the mayor threw it
    /// out), 0 not known yet. A bad one weakens their resolve next time.
    pub verdict: i8,
}

impl Default for MemoryEntry {
    fn default() -> Self {
        Self { day: 0, meeting: 0, proposal: String::new(), kind: String::new(), vote: None, passed: false, proposed: false, metric: None, outcome: String::new(), verdict: 0 }
    }
}

impl MemoryEntry {
    /// One line for the window and the prompts.
    pub fn line(&self) -> String {
        let how = match self.vote {
            Some(v) => format!("voted {}", v.key()),
            None => "did not vote".into(),
        };
        let mut s = format!("Day {}: {} on \"{}\"{} ({})", self.day + 1, how, self.proposal, if self.proposed { ", which I tabled" } else { "" }, if self.passed { "passed" } else { "failed" });
        if !self.outcome.is_empty() {
            s.push_str(&format!("; {}", self.outcome));
        }
        s
    }
}

/// A member's record: what the window shows beside the memory.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Record {
    /// Day of the current appointment (None: not seated).
    pub seated_since: Option<u32>,
    pub times_appointed: u32,
    pub times_dismissed: u32,
    pub meetings: u32,
    pub alerts: u32,
    pub proposed: u32,
    pub proposals_passed: u32,
    pub votes_yes: u32,
    pub votes_no: u32,
    pub abstained: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Politician {
    pub id: u32,
    /// From the game's name generator: fictional.
    pub name: String,
    pub age: u8,
    pub scope: Scope,
    pub temper: Temper,
    pub leanings: Leanings,
    /// A fictional party.
    pub party: String,
    /// What they did before politics.
    pub background: String,
    #[serde(default)]
    pub memory: Vec<MemoryEntry>,
    #[serde(default)]
    pub record: Record,
}

pub(super) const PARTIES: &[&str] = &["Neon Civic League", "Grid Labour", "Free Market Bloc", "Green Circuit", "Order and Light", "Independent"];

impl Politician {
    /// Their goals: the job's, then the one their leanings add, which often
    /// pulls the other way.
    pub fn goals(&self) -> Vec<String> {
        let mut out = vec![self.scope.goal().to_string()];
        let l = &self.leanings;
        let strongest = [(l.spending.abs(), 0), (l.growth.abs(), 1), (l.order.abs(), 2)].into_iter().max_by(|a, b| a.0.total_cmp(&b.0)).map_or(0, |x| x.1);
        out.push(
            match (strongest, [l.spending, l.growth, l.order][strongest] >= 0.0) {
                (0, true) => "spend boldly on what the city needs",
                (0, false) => "no new spending the treasury cannot carry",
                (1, true) => "growth and jobs before everything",
                (1, false) => "no growth that dirties the ground",
                (2, true) => "visible policing on every corner",
                _ => "keep the city free and loosely policed",
            }
            .to_string(),
        );
        out
    }

    pub fn bio(&self) -> String {
        format!("{}, {}. Former {}. {}. Stands for: {}.", self.name, self.age, self.background, capitalise(self.temper.describe()), self.goals().join("; "))
    }

    pub fn remember(&mut self, m: MemoryEntry) {
        self.memory.push(m);
        if self.memory.len() > MEMORY_KEPT {
            let extra = self.memory.len() - MEMORY_KEPT;
            self.memory.drain(..extra);
        }
    }

    /// Their last vote on a kind of action, if they remember one.
    pub fn past_vote_on(&self, kind: &str) -> Option<&MemoryEntry> {
        self.memory.iter().rev().find(|m| m.kind == kind && m.vote.is_some())
    }
}

pub fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// The pool: every scope at least once, fictional names from the game's
/// name generator, distinct.
pub fn generate_pool(seed: u64) -> Vec<Politician> {
    let mut rng = SimRng::seed_from_u64(mix(seed, 0xC0_3317_7EE5));
    let mut scopes: Vec<Scope> = Scope::ALL.to_vec();
    while scopes.len() < POOL_SIZE {
        scopes.push(*Scope::ALL.choose(&mut rng).unwrap());
    }
    let mut names: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for (i, scope) in scopes.into_iter().enumerate() {
        let mut name = crate::names::citizen_name(&mut rng);
        let mut tries = 0;
        while names.contains(&name) && tries < 20 {
            name = crate::names::citizen_name(&mut rng);
            tries += 1;
        }
        names.push(name.clone());
        let temper = *Temper::ALL.choose(&mut rng).unwrap();
        let mut leanings = Leanings { spending: rng.random_range(-1.0..1.0), growth: rng.random_range(-1.0..1.0), order: rng.random_range(-1.0..1.0), loyalty: rng.random_range(0.1..0.9) };
        // The job pulls a little: an environment commissioner leans green
        // more often than not, a treasurer frugal; never always.
        match scope {
            Scope::Environment => leanings.growth -= 0.35,
            Scope::Finance => leanings.spending -= 0.35,
            Scope::Business => leanings.growth += 0.35,
            Scope::Safety => leanings.order += 0.3,
            Scope::Welfare | Scope::Transit => leanings.spending += 0.25,
            _ => {}
        }
        for v in [&mut leanings.spending, &mut leanings.growth, &mut leanings.order] {
            *v = v.clamp(-1.0, 1.0);
        }
        if temper == Temper::Contrarian {
            leanings.loyalty *= 0.4;
        }
        let background = Scope::BACKGROUNDS[scope.index()].choose(&mut rng).unwrap().to_string();
        out.push(Politician {
            id: i as u32 + 1,
            name,
            age: rng.random_range(29..72),
            scope,
            temper,
            leanings,
            party: PARTIES.choose(&mut rng).unwrap().to_string(),
            background,
            memory: Vec::new(),
            record: Record::default(),
        });
    }
    out
}

#[cfg(test)]
mod pool_tests {
    use super::*;

    #[test]
    fn the_pool_is_fictional_varied_and_stable() {
        let a = generate_pool(7);
        let b = generate_pool(7);
        assert_eq!(a, b, "same seed, same pool");
        assert_eq!(a.len(), POOL_SIZE);
        for s in Scope::ALL {
            assert!(a.iter().any(|p| p.scope == s), "{s:?} missing");
        }
        let mut names: Vec<_> = a.iter().map(|p| &p.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), a.len(), "names are distinct");
        assert_ne!(generate_pool(8), a);
        let _ = test_seed();
    }
}
