//! [committee] The mayor's AI committee (docs/AI_COMMITTEE.md): a pool of
//! fictional politicians, six seats the mayor fills and empties at will,
//! alerts between meetings, a meeting every 30 sim days with a fixed agenda,
//! votes, and passed proposals turned into named projects for the mayor to
//! implement.
//!
//! This module is the deterministic half and holds no model: the pool (from
//! the game's name generator and a hash of the world seed), the compact city
//! report the members argue over (`report`), the alert rules (`rules`), the
//! proposal actions with their validation, the rule-based votes, the
//! candidate sites and the implementation (`proposals`). What the members
//! *say* comes from a text backend in `nd_minds::committee` (a template
//! stand-in, a local model or the Claude API).
//!
//! The sim never reads any of this. The committee's state is saved with the
//! world (`World::committee`, `serde(default)`, so old saves load with an
//! empty one), and its only way back into the city is a project the mayor
//! implements by hand (`World::committee_implement`), recorded in the
//! project's status.

pub mod proposals;
pub mod report;
pub mod rules;

#[cfg(test)]
mod tests;

use rand::seq::IndexedRandom;
use rand::{Rng, SeedableRng};
use serde::{Deserialize, Serialize};

pub use proposals::{Action, SiteOption};
pub use report::{Area, CityReport, Fact};

use crate::{SimRng, World};

/// Seats at the table (the mayor is not one of them: the mayor has a vote
/// of their own and, optionally, a veto).
pub const SEATS: usize = 6;
/// Candidates in the pool.
pub const POOL_SIZE: usize = 14;
/// Sim days between regular meetings.
pub const MEETING_INTERVAL_DAYS: u32 = 30;
/// Memory entries each politician keeps (oldest dropped).
pub const MEMORY_KEPT: usize = 12;
/// Alerts kept in the log.
pub const ALERTS_KEPT: usize = 80;
/// Past meetings' minutes kept in full.
pub const MINUTES_KEPT: usize = 6;
/// Proposals tabled per meeting at most (a turn limit: keeps cost and
/// length predictable).
pub const MAX_PROPOSALS: usize = 3;
/// Replies to the status reports per meeting at most.
pub const MAX_RESPONSES: usize = 2;

// ── scopes, tempers, leanings ───────────────────────────────────────────────

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

    const BACKGROUNDS: [[&'static str; 3]; 8] = [
        ["bus dispatcher", "rail signal engineer", "courier union organiser"],
        ["tenant organiser", "property developer", "housing co-op architect"],
        ["city auditor", "bond trader", "credit union manager"],
        ["retired police captain", "fire chief", "public defender"],
        ["soil chemist", "river warden", "solar co-op founder"],
        ["noodle-bar chain owner", "chamber of commerce president", "freight broker"],
        ["grid engineer", "road crew foreman", "utility regulator"],
        ["school principal", "emergency-room nurse", "clinic director"],
    ];

    fn index(self) -> usize {
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

// ── politicians ─────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Vote {
    Yes,
    No,
    Abstain,
}

impl Vote {
    pub fn key(self) -> &'static str {
        match self {
            Vote::Yes => "yes",
            Vote::No => "no",
            Vote::Abstain => "abstain",
        }
    }

    pub fn parse(s: &str) -> Option<Vote> {
        match s.trim().to_ascii_lowercase().as_str() {
            "yes" | "aye" | "for" | "y" => Some(Vote::Yes),
            "no" | "nay" | "against" | "n" => Some(Vote::No),
            "abstain" | "abstention" | "pass" => Some(Vote::Abstain),
            _ => None,
        }
    }
}

/// The mayor's vote: one vote like everyone's, or a veto that sinks it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MayorVote {
    Yes,
    No,
    Abstain,
    Veto,
}

impl MayorVote {
    pub fn key(self) -> &'static str {
        match self {
            MayorVote::Yes => "yes",
            MayorVote::No => "no",
            MayorVote::Abstain => "abstain",
            MayorVote::Veto => "veto",
        }
    }

    pub fn parse(s: &str) -> Option<MayorVote> {
        match s.trim().to_ascii_lowercase().as_str() {
            "veto" => Some(MayorVote::Veto),
            other => Vote::parse(other).map(|v| match v {
                Vote::Yes => MayorVote::Yes,
                Vote::No => MayorVote::No,
                Vote::Abstain => MayorVote::Abstain,
            }),
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

const PARTIES: &[&str] = &["Neon Civic League", "Grid Labour", "Free Market Bloc", "Green Circuit", "Order and Light", "Independent"];

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

    /// "Mira Okafor, Housing commissioner (Populist, Grid Labour)".
    pub fn title_line(&self) -> String {
        format!("{}, {} ({}, {})", self.name, self.scope.title(), self.temper.label(), self.party)
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

// ── alerts ──────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Alert {
    pub id: u32,
    pub day: u32,
    /// The member who raised it.
    pub member: u32,
    /// The rule's key (`rules::RULES`).
    pub rule: String,
    /// 1 notice, 2 warning, 3 urgent.
    pub severity: u8,
    /// The numbers behind it.
    pub facts: Vec<Fact>,
    /// The member's words (the backend phrases it; a template stands in
    /// until it answers).
    pub text: String,
    /// The backend's phrasing has replaced the template's.
    pub phrased: bool,
    pub seen: bool,
}

impl Default for Alert {
    fn default() -> Self {
        Self { id: 0, day: 0, member: 0, rule: String::new(), severity: 1, facts: Vec::new(), text: String::new(), phrased: false, seen: false }
    }
}

// ── meetings ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Speaker {
    Secretary,
    Member(u32),
    Mayor,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineKind {
    /// Someone speaking.
    Speech,
    /// An agenda heading from the secretary.
    Agenda,
    /// A vote cast.
    Vote,
    /// A tally or a result.
    Result,
    /// A note for the record (a rejected proposal, a backend fallback).
    Note,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub speaker: Speaker,
    pub name: String,
    pub kind: LineKind,
    pub text: String,
}

/// Where the agenda stands. Each stage is a run of turns; `Meeting::cursor`
/// counts the turns taken in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage {
    Opening,
    /// 1. A status report from every member.
    Status,
    /// ... with optional responses from others.
    Responses,
    /// 2. Topics of concern, tabled as structured proposals.
    Proposals,
    /// 3. The members vote...
    Votes,
    /// ... and the mayor votes (or vetoes). The meeting waits here.
    MayorVote,
    /// 4. The secretary turns what passed into projects.
    Implementation,
    Closing,
    Done,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::Opening => "opening",
            Stage::Status => "status reports",
            Stage::Responses => "responses",
            Stage::Proposals => "topics of concern",
            Stage::Votes => "votes",
            Stage::MayorVote => "the mayor's vote",
            Stage::Implementation => "implementation",
            Stage::Closing => "closing",
            Stage::Done => "adjourned",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Passed,
    Failed,
    Vetoed,
}

impl Outcome {
    pub fn key(self) -> &'static str {
        match self {
            Outcome::Passed => "passed",
            Outcome::Failed => "failed",
            Outcome::Vetoed => "vetoed",
        }
    }
}

/// A member's vote with their reason.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ballot {
    pub member: u32,
    pub vote: Vote,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    pub id: u32,
    /// The member who tabled it.
    pub by: u32,
    /// Validated before it was tabled.
    pub action: Action,
    /// Short name: "Northside clinic".
    pub title: String,
    /// Why, in the proposer's words.
    pub rationale: String,
    /// Estimated cost to build or the yearly effect, for the window.
    pub cost: f32,
    #[serde(default)]
    pub ballots: Vec<Ballot>,
    #[serde(default)]
    pub mayor: Option<MayorVote>,
    #[serde(default)]
    pub outcome: Option<Outcome>,
    /// The committee project it became.
    #[serde(default)]
    pub project: Option<u32>,
}

/// The tally of one proposal.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub yes: u32,
    pub no: u32,
    pub abstain: u32,
}

impl Proposal {
    /// Members' and the mayor's votes counted together; a veto sinks it,
    /// otherwise a simple majority of the votes cast (ties fail).
    pub fn tally(&self) -> (Tally, Outcome) {
        let mut t = Tally::default();
        for b in &self.ballots {
            match b.vote {
                Vote::Yes => t.yes += 1,
                Vote::No => t.no += 1,
                Vote::Abstain => t.abstain += 1,
            }
        }
        match self.mayor {
            Some(MayorVote::Veto) => return (t, Outcome::Vetoed),
            Some(MayorVote::Yes) => t.yes += 1,
            Some(MayorVote::No) => t.no += 1,
            Some(MayorVote::Abstain) | None => t.abstain += 1,
        }
        (t, if t.yes > t.no { Outcome::Passed } else { Outcome::Failed })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Meeting {
    pub number: u32,
    pub day: u32,
    /// Seeds the stand-in's choices, so a meeting replays exactly.
    pub seed: u64,
    pub stage: Stage,
    /// Turns taken in the current stage.
    pub cursor: usize,
    /// Who sat at the table, in seat order.
    pub members: Vec<u32>,
    pub transcript: Vec<Line>,
    pub proposals: Vec<Proposal>,
    /// The city as the meeting read it.
    pub report: CityReport,
    /// Backend calls made (the cost meter).
    #[serde(default)]
    pub calls: u32,
    /// Called by the mayor rather than the calendar.
    #[serde(default)]
    pub on_demand: bool,
    /// Members (in order) asked to table a topic.
    #[serde(default)]
    pub proposers: Vec<u32>,
}

impl Meeting {
    pub fn say(&mut self, speaker: Speaker, name: &str, kind: LineKind, text: impl Into<String>) {
        self.transcript.push(Line { speaker, name: name.to_string(), kind, text: text.into() });
    }

    pub fn is_done(&self) -> bool {
        self.stage == Stage::Done
    }

    /// Waiting for the mayor's votes.
    pub fn waiting_for_mayor(&self) -> bool {
        self.stage == Stage::MayorVote && self.proposals.iter().any(|p| p.mayor.is_none())
    }

    pub fn advance(&mut self, stage: Stage) {
        self.stage = stage;
        self.cursor = 0;
    }
}

// ── projects ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ProjectStatus {
    /// Waiting for the mayor to implement or dismiss it.
    Pending,
    /// No valid site: the secretary asks the mayor (retry later, or dismiss).
    NeedsMayor,
    Implemented { day: u32 },
    Dismissed { day: u32 },
    /// The mayor tried; the game refused (the site was taken meanwhile, the
    /// treasury ran dry). Stays open for a retry.
    Failed { why: String },
}

impl ProjectStatus {
    pub fn key(&self) -> &'static str {
        match self {
            ProjectStatus::Pending => "pending",
            ProjectStatus::NeedsMayor => "needs_mayor",
            ProjectStatus::Implemented { .. } => "implemented",
            ProjectStatus::Dismissed { .. } => "dismissed",
            ProjectStatus::Failed { .. } => "failed",
        }
    }

    pub fn is_open(&self) -> bool {
        matches!(self, ProjectStatus::Pending | ProjectStatus::NeedsMayor | ProjectStatus::Failed { .. })
    }
}

/// A passed proposal waiting for the mayor: what, where (picked by the
/// secretary from the sites the code found), and why.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CommitteeProject {
    pub id: u32,
    /// "Committee: Northside clinic".
    pub name: String,
    pub meeting: u32,
    pub proposal: u32,
    pub by: u32,
    pub action: Action,
    /// Every valid option the code found, best first.
    pub options: Vec<SiteOption>,
    /// The one the secretary picked (index into `options`).
    pub pick: Option<usize>,
    /// The secretary's reason.
    pub reason: String,
    pub status: ProjectStatus,
    /// For zoning: the world project (`World::projects`) holding the
    /// painted parcels, previewed dimmed on the map like any other.
    #[serde(default)]
    pub world_project: Option<u64>,
    /// What implementing did, or why it could not.
    #[serde(default)]
    pub log: Vec<String>,
}

impl CommitteeProject {
    pub fn site(&self) -> Option<&SiteOption> {
        self.pick.and_then(|i| self.options.get(i))
    }
}

// ── the committee ───────────────────────────────────────────────────────────

/// A day's figures the rules compare against a week later.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sample {
    pub day: u32,
    pub rent_burden: f32,
    pub contaminated_cells: u32,
    pub riders: f32,
    pub crime: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Committee {
    /// Filled once, from the world seed (`ensure`).
    pub pool: Vec<Politician>,
    /// Politician ids by seat.
    pub seats: Vec<Option<u32>>,
    pub alerts: Vec<Alert>,
    /// Day each alert rule last fired, by rule key.
    pub rule_last: std::collections::BTreeMap<String, u32>,
    /// Daily samples for the week-on-week rules (newest last).
    pub samples: Vec<Sample>,
    /// The day the next regular meeting is due.
    pub next_meeting_day: u32,
    pub meetings_held: u32,
    /// The meeting in session, if any.
    pub meeting: Option<Meeting>,
    /// The last meetings' minutes (newest last).
    pub minutes: Vec<Meeting>,
    pub projects: Vec<CommitteeProject>,
    pub next_id: u32,
    /// Meetings come by the calendar (off: only when the mayor calls one).
    pub auto_meetings: bool,
    /// The mayor may veto.
    pub veto_allowed: bool,
    /// The report the last meeting read, for "since the last meeting".
    pub baseline: Option<CityReport>,
}

impl Default for Committee {
    fn default() -> Self {
        Self {
            pool: Vec::new(),
            seats: vec![None; SEATS],
            alerts: Vec::new(),
            rule_last: Default::default(),
            samples: Vec::new(),
            next_meeting_day: 0,
            meetings_held: 0,
            meeting: None,
            minutes: Vec::new(),
            projects: Vec::new(),
            next_id: 1,
            auto_meetings: true,
            veto_allowed: true,
            baseline: None,
        }
    }
}

impl Committee {
    pub fn is_set_up(&self) -> bool {
        !self.pool.is_empty()
    }

    /// Fill the pool from `seed` (once) and seat the outgoing mayor's
    /// committee, one member for each of the six main scopes; schedule the
    /// first meeting. Deterministic in the seed; draws nothing from the
    /// world's RNG.
    pub fn ensure(&mut self, seed: u64, today: u32) {
        if self.seats.len() != SEATS {
            self.seats.resize(SEATS, None);
        }
        if self.next_id == 0 {
            self.next_id = 1;
        }
        if !self.pool.is_empty() {
            return;
        }
        self.pool = generate_pool(seed);
        self.next_id = self.next_id.max(self.pool.iter().map(|p| p.id + 1).max().unwrap_or(1));
        let first = [Scope::Transit, Scope::Housing, Scope::Finance, Scope::Safety, Scope::Environment, Scope::Business];
        for (seat, scope) in first.into_iter().enumerate() {
            if let Some(id) = self.pool.iter().find(|p| p.scope == scope).map(|p| p.id) {
                self.seat(seat, id, today);
            }
        }
        self.next_meeting_day = today + MEETING_INTERVAL_DAYS;
    }

    pub fn fresh_id(&mut self) -> u32 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    pub fn member(&self, id: u32) -> Option<&Politician> {
        self.pool.iter().find(|p| p.id == id)
    }

    pub fn member_mut(&mut self, id: u32) -> Option<&mut Politician> {
        self.pool.iter_mut().find(|p| p.id == id)
    }

    pub fn name_of(&self, id: u32) -> String {
        self.member(id).map_or_else(|| format!("member {id}"), |p| p.name.clone())
    }

    /// Seated members in seat order.
    pub fn seated(&self) -> Vec<&Politician> {
        self.seats.iter().flatten().filter_map(|&id| self.member(id)).collect()
    }

    pub fn seated_ids(&self) -> Vec<u32> {
        self.seats.iter().flatten().copied().filter(|&id| self.member(id).is_some()).collect()
    }

    pub fn seat_of(&self, id: u32) -> Option<usize> {
        self.seats.iter().position(|s| *s == Some(id))
    }

    /// Candidates not seated.
    pub fn candidates(&self) -> Vec<&Politician> {
        self.pool.iter().filter(|p| self.seat_of(p.id).is_none()).collect()
    }

    /// The first seated member who answers for `scope`.
    pub fn watcher(&self, scope: Scope) -> Option<u32> {
        self.seated().into_iter().find(|p| p.scope == scope).map(|p| p.id)
    }

    fn seat(&mut self, seat: usize, id: u32, today: u32) {
        self.seats[seat] = Some(id);
        if let Some(p) = self.member_mut(id) {
            p.record.seated_since = Some(today);
            p.record.times_appointed += 1;
        }
    }

    /// Appoint a candidate: to `seat` if given (whoever sat there is
    /// dismissed), else to the first empty seat.
    pub fn appoint(&mut self, id: u32, seat: Option<usize>, today: u32) -> Result<usize, String> {
        if self.member(id).is_none() {
            return Err(format!("no politician {id} in the pool"));
        }
        if let Some(s) = self.seat_of(id) {
            return Err(format!("{} already holds seat {}", self.name_of(id), s + 1));
        }
        let seat = match seat {
            Some(s) if s < SEATS => s,
            Some(s) => return Err(format!("there are {SEATS} seats; seat {} does not exist", s + 1)),
            None => self.seats.iter().position(Option::is_none).ok_or("every seat is taken: dismiss someone first, or name the seat to replace them")?,
        };
        if let Some(old) = self.seats[seat] {
            self.dismiss(old, today)?;
        }
        self.seat(seat, id, today);
        Ok(seat)
    }

    /// Dismiss a seated member; their seat stays empty until filled.
    pub fn dismiss(&mut self, id: u32, _today: u32) -> Result<usize, String> {
        let seat = self.seat_of(id).ok_or_else(|| format!("{} holds no seat", self.name_of(id)))?;
        self.seats[seat] = None;
        if let Some(p) = self.member_mut(id) {
            p.record.seated_since = None;
            p.record.times_dismissed += 1;
        }
        Ok(seat)
    }

    pub fn push_alert(&mut self, mut a: Alert) -> u32 {
        a.id = self.fresh_id();
        let id = a.id;
        if let Some(p) = self.member_mut(a.member) {
            p.record.alerts += 1;
        }
        self.alerts.push(a);
        if self.alerts.len() > ALERTS_KEPT {
            let extra = self.alerts.len() - ALERTS_KEPT;
            self.alerts.drain(..extra);
        }
        id
    }

    pub fn alert_mut(&mut self, id: u32) -> Option<&mut Alert> {
        self.alerts.iter_mut().find(|a| a.id == id)
    }

    pub fn unseen_alerts(&self) -> usize {
        self.alerts.iter().filter(|a| !a.seen).count()
    }

    pub fn project(&self, id: u32) -> Option<&CommitteeProject> {
        self.projects.iter().find(|p| p.id == id)
    }

    pub fn project_mut(&mut self, id: u32) -> Option<&mut CommitteeProject> {
        self.projects.iter_mut().find(|p| p.id == id)
    }

    /// Is a regular meeting due today?
    pub fn meeting_due(&self, today: u32) -> bool {
        self.auto_meetings && self.meeting.is_none() && self.is_set_up() && today >= self.next_meeting_day && !self.seated_ids().is_empty()
    }

    /// File a finished meeting: minutes kept, the next one scheduled.
    pub fn file_meeting(&mut self) {
        if let Some(m) = self.meeting.take() {
            self.baseline = Some(m.report.clone());
            self.minutes.push(m);
            if self.minutes.len() > MINUTES_KEPT {
                let extra = self.minutes.len() - MINUTES_KEPT;
                self.minutes.drain(..extra);
            }
        }
    }

    /// A daily sample for the week-on-week rules.
    pub fn push_sample(&mut self, s: Sample) {
        if self.samples.last().is_some_and(|l| l.day == s.day) {
            self.samples.pop();
        }
        self.samples.push(s);
        if self.samples.len() > 30 {
            let extra = self.samples.len() - 30;
            self.samples.drain(..extra);
        }
    }

    /// The sample `days` before `today`, or the oldest kept if fewer.
    pub fn sample_before(&self, today: u32, days: u32) -> Option<&Sample> {
        self.samples.iter().rev().find(|s| s.day + days <= today).or_else(|| self.samples.first().filter(|s| s.day < today))
    }
}

/// A stable 64-bit mix (splitmix64), for seeds from seeds.
pub use crate::hash::mix;

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

// ── the world's side ────────────────────────────────────────────────────────

impl World {
    /// Today, as the committee counts days (0-based like the clock).
    pub fn committee_today(&self) -> u32 {
        self.clock.day() as u32
    }

    /// Set the committee up if it is not (a new game, or a save from
    /// before the committee).
    pub fn committee_ensure(&mut self) {
        let today = self.committee_today();
        // The game id makes each new game's pool its own; a hash, not the
        // world's RNG, so the sim is untouched.
        let id = crate::hash::fnv_bytes(self.identity.game_id.bytes());
        let seed = mix(mix(self.config.seed, self.landscape.params.seed), id);
        self.committee.ensure(seed, today);
    }

    /// The day's committee work outside meetings: a sample for the rules
    /// and the alerts they raise. Returns the new alerts' ids. Reads the
    /// world, writes only the committee.
    pub fn committee_daily(&mut self) -> Vec<u32> {
        self.committee_ensure();
        let report = CityReport::build(self);
        let today = self.committee_today();
        self.committee.push_sample(Sample { day: today, rent_burden: report.rent_burden, contaminated_cells: report.contaminated_cells, riders: report.riders_per_day, crime: report.crime_per_day });
        rules::raise_alerts(&mut self.committee, &report, today)
    }
}

#[cfg(test)]
pub(crate) fn test_seed() -> u64 {
    42
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
