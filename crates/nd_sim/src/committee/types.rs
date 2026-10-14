//! What the committee keeps: votes, alerts, meetings and their lines,
//! proposals and ballots, projects and the daily samples.

use super::*;

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
    /// [ask-committee] The digest of the mayor's questions since the last
    /// meeting, as every turn of this meeting reads it (empty: none).
    #[serde(default)]
    pub questions: String,
    /// The question threads it covers (marked discussed by this meeting).
    #[serde(default)]
    pub questions_read: Vec<u32>,
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
