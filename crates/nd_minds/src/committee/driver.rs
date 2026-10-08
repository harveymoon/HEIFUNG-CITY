//! [committee] The secretary's agenda, turn by turn.
//!
//! 1. Opening, then a status report from every member, then up to
//!    `MAX_RESPONSES` replies from members who see it differently.
//! 2. Topics of concern: up to `MAX_PROPOSALS` members (the most pressing
//!    first) table a structured proposal; each is validated before it is
//!    tabled, and an invalid one is ruled out of order.
//! 3. Votes: each member votes on every proposal in one turn; then the
//!    meeting waits for the mayor's vote (or veto) and tallies.
//! 4. Implementation: for each passed proposal the code finds the valid
//!    sites, the secretary picks one with a reason, and the result is filed
//!    as a named project for the mayor; with no valid site the secretary
//!    asks the mayor.
//!
//! Then the secretary closes. A meeting of six makes at most
//! 1 + 6 + 2 + 3 + 6 + 3 + 1 = 22 backend calls. The state lives in
//! `World::committee.meeting` (saved with the game); the driver only holds
//! the one request in flight, so a meeting loaded from a save picks up
//! where it stood.

use std::collections::HashMap;

use nd_sim::committee::proposals::{lean, site_options, suggest, urgency};
use nd_sim::committee::{mix, Ballot, CityReport, LineKind, MayorVote, Meeting, Outcome, Proposal, Speaker, Stage, MAX_PROPOSALS, MAX_RESPONSES};
use nd_sim::committee::{Action, SiteOption};
use nd_sim::World;

use super::mock::vote_in;
use super::{Courier, Delivered, PersonaCard, Task, TurnRequest, VoteItem};

/// The secretary's name in the transcript.
pub const SECRETARY: &str = "Secretary";

#[derive(Clone, Debug)]
enum Step {
    Opening,
    Status(u32),
    Response(u32),
    Propose(u32),
    Vote(u32),
    Pick(usize, Vec<SiteOption>),
    Closing,
}

/// Runs the meeting in session. Holds only the request in flight.
#[derive(Default)]
pub struct Driver {
    pending: Option<(u64, Step)>,
    /// Alert phrasings in flight: request id to alert id.
    alerts: HashMap<u64, u32>,
}

/// Open a meeting: the city report read, memories judged, the speakers for
/// the topics chosen. The caller pauses the sim.
pub fn start_meeting(w: &mut World, on_demand: bool) -> Result<u32, String> {
    w.committee_ensure();
    if w.committee.meeting.is_some() {
        return Err("a meeting is already in session".into());
    }
    let members = w.committee.seated_ids();
    if members.is_empty() {
        return Err("no members are seated: appoint some first".into());
    }
    let today = w.committee_today();
    let report = CityReport::build(w);
    w.committee.judge_memories(&report);
    let number = w.committee.meetings_held + 1;
    let seed = mix(mix(w.config.seed, w.landscape.params.seed), number as u64);
    // Who tables: the most pressing scopes first, then those the rules
    // have a valid proposal for; seat order breaks ties.
    let mut ranked: Vec<(u8, bool, usize, u32)> = Vec::new();
    for (i, &id) in members.iter().enumerate() {
        let p = w.committee.member(id).unwrap();
        let has = suggest(p, &report).is_some_and(|(a, _)| a.validate(w).is_ok());
        let u = urgency(p, &report, &w.committee, today);
        if has || u > 0 {
            ranked.push((u, has, i, id));
        }
    }
    ranked.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)).then(a.2.cmp(&b.2)));
    let proposers: Vec<u32> = ranked.into_iter().take(MAX_PROPOSALS).map(|x| x.3).collect();
    let c = &mut w.committee;
    c.meetings_held = number;
    c.next_meeting_day = today + nd_sim::committee::MEETING_INTERVAL_DAYS;
    for &id in &members {
        if let Some(p) = c.member_mut(id) {
            p.record.meetings += 1;
        }
    }
    c.meeting = Some(Meeting { number, day: today, seed, stage: Stage::Opening, cursor: 0, members, transcript: Vec::new(), proposals: Vec::new(), report, calls: 0, on_demand, proposers });
    Ok(number)
}

/// The mayor's vote on a tabled proposal (a veto only if the committee
/// allows it).
pub fn cast_mayor_vote(w: &mut World, proposal: u32, vote: MayorVote) -> Result<(), String> {
    let veto_ok = w.committee.veto_allowed;
    let m = w.committee.meeting.as_mut().ok_or("no meeting is in session")?;
    if !matches!(m.stage, Stage::Votes | Stage::MayorVote) {
        return Err(format!("the meeting is at {}, not the votes", m.stage.label()));
    }
    if vote == MayorVote::Veto && !veto_ok {
        return Err("the mayoral veto is switched off".into());
    }
    let p = m.proposals.iter_mut().find(|p| p.id == proposal).ok_or_else(|| format!("no proposal #{proposal}"))?;
    p.mayor = Some(vote);
    let line = match vote {
        MayorVote::Veto => format!("I veto #{}: {}.", p.id, p.title),
        v => format!("I vote {} on #{}: {}.", v.key(), p.id, p.title),
    };
    m.say(Speaker::Mayor, "The mayor", LineKind::Vote, line);
    Ok(())
}

/// The mayor abstains on every proposal not yet voted on.
pub fn abstain_rest(w: &mut World) -> Result<usize, String> {
    let ids: Vec<u32> = w.committee.meeting.as_ref().ok_or("no meeting is in session")?.proposals.iter().filter(|p| p.mayor.is_none()).map(|p| p.id).collect();
    for id in &ids {
        cast_mayor_vote(w, *id, MayorVote::Abstain)?;
    }
    Ok(ids.len())
}

fn card(w: &World, id: u32) -> Option<PersonaCard> {
    w.committee.member(id).map(PersonaCard::of)
}

fn request(w: &World, m: &Meeting, speaker: Option<u32>, task: Task, salt: u64, max_tokens: u32) -> TurnRequest {
    TurnRequest { speaker: speaker.and_then(|id| card(w, id)), task, city: m.report.city.clone(), day: m.day, report: m.report.to_text(), seed: mix(m.seed, salt), max_tokens }
}

/// Who answers whom: the pairs whose leanings differ most (or agree most,
/// when nobody differs), distinct responders, deterministic.
fn response_plan(w: &World, m: &Meeting) -> Vec<(u32, u32, bool)> {
    let ms: Vec<_> = m.members.iter().filter_map(|&id| w.committee.member(id)).collect();
    if ms.len() < 2 {
        return Vec::new();
    }
    let mut pairs = Vec::new();
    for a in &ms {
        for b in &ms {
            if a.id == b.id {
                continue;
            }
            let (x, y) = (&a.leanings, &b.leanings);
            let gap = (x.growth - y.growth).abs() + (x.spending - y.spending).abs() + (x.order - y.order).abs();
            let tie = (mix(m.seed, (a.id as u64) << 8 | b.id as u64) % 100) as f32 / 1000.0;
            pairs.push((gap + tie, a.id, b.id, gap < 0.9));
        }
    }
    pairs.sort_by(|p, q| q.0.total_cmp(&p.0));
    let mut out: Vec<(u32, u32, bool)> = Vec::new();
    for (_, a, b, agree) in pairs {
        if out.iter().any(|o| o.0 == a || o.1 == a) {
            continue;
        }
        out.push((a, b, agree));
        if out.len() == MAX_RESPONSES {
            break;
        }
    }
    out
}

impl Driver {
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    /// Forget the request in flight (the city was replaced).
    pub fn reset(&mut self) {
        self.pending = None;
        self.alerts.clear();
    }

    /// Ask the backend to phrase an alert; the rule's own words stand
    /// until it answers.
    pub fn phrase_alert(&mut self, w: &World, alert: u32, courier: &mut Courier) {
        let Some(a) = w.committee.alerts.iter().find(|a| a.id == alert) else { return };
        let when = nd_sim::committee::rules::rule(&a.rule).map_or("", |r| r.when).to_string();
        let req = TurnRequest {
            speaker: card(w, a.member),
            task: Task::Alert { rule: a.rule.clone(), when, severity: a.severity, facts: a.facts.clone() },
            city: if w.identity.name.is_empty() { "the city".into() } else { w.identity.name.clone() },
            day: a.day,
            report: String::new(),
            seed: mix(w.config.seed, a.id as u64),
            max_tokens: 120,
        };
        let id = courier.submit(req);
        self.alerts.insert(id, alert);
    }

    /// Hand a reply in; true if it was this driver's.
    pub fn take(&mut self, w: &mut World, d: &Delivered) -> bool {
        if let Some(alert) = self.alerts.remove(&d.id) {
            if let Some(a) = w.committee.alert_mut(alert) {
                if !d.reply.text.trim().is_empty() {
                    a.text = d.reply.text.trim().to_string();
                    a.phrased = d.fell_back.is_none();
                }
            }
            return true;
        }
        let Some((id, step)) = self.pending.clone() else { return false };
        if id != d.id {
            return false;
        }
        self.pending = None;
        if w.committee.meeting.is_none() {
            return true;
        }
        if let Some(why) = &d.fell_back {
            let m = w.committee.meeting.as_mut().unwrap();
            if !m.transcript.iter().any(|l| l.kind == LineKind::Note && l.text.starts_with("(The model")) {
                m.say(Speaker::Secretary, SECRETARY, LineKind::Note, format!("(The model did not answer a turn: {why}. The stand-in speaks for it; this note appears once per meeting.)"));
            }
        }
        self.apply(w, step, d);
        true
    }

    fn apply(&mut self, w: &mut World, step: Step, d: &Delivered) {
        let text = d.reply.text.trim().to_string();
        match step {
            Step::Opening => {
                let m = w.committee.meeting.as_mut().unwrap();
                m.say(Speaker::Secretary, SECRETARY, LineKind::Speech, text);
                m.say(Speaker::Secretary, SECRETARY, LineKind::Agenda, "1. Status reports");
                m.advance(Stage::Status);
            }
            Step::Status(id) => {
                let name = w.committee.name_of(id);
                let m = w.committee.meeting.as_mut().unwrap();
                m.say(Speaker::Member(id), &name, LineKind::Speech, text);
                m.cursor += 1;
            }
            Step::Response(id) => {
                let name = w.committee.name_of(id);
                let m = w.committee.meeting.as_mut().unwrap();
                m.say(Speaker::Member(id), &name, LineKind::Speech, text);
                m.cursor += 1;
            }
            Step::Propose(id) => self.apply_proposal(w, id, d),
            Step::Vote(id) => self.apply_votes(w, id, d),
            Step::Pick(idx, options) => {
                let json = d.reply.json.as_ref();
                let choice = json.and_then(|j| j.get("choice")).and_then(|c| c.as_u64().or_else(|| c.as_str().and_then(|s| s.trim().parse().ok()))).map(|c| c as usize).filter(|&c| c >= 1 && c <= options.len()).map(|c| c - 1);
                let (pick, reason) = match choice {
                    Some(c) => (c, json.and_then(|j| j.get("reason")).and_then(|r| r.as_str()).map(str::to_string).unwrap_or(text)),
                    None => (0, format!("Option 1: {} (the best-scoring valid site).", options[0].label)),
                };
                self.file(w, idx, options, Some(pick), reason);
            }
            Step::Closing => {
                let m = w.committee.meeting.as_mut().unwrap();
                m.say(Speaker::Secretary, SECRETARY, LineKind::Speech, text);
                m.advance(Stage::Done);
                w.committee.file_meeting();
            }
        }
    }

    fn apply_proposal(&mut self, w: &mut World, id: u32, d: &Delivered) {
        let name = w.committee.name_of(id);
        let json = d.reply.json.clone();
        let rationale = json.as_ref().and_then(|j| j.get("rationale")).and_then(|r| r.as_str()).map(str::to_string).filter(|s| !s.trim().is_empty()).unwrap_or_else(|| d.reply.text.trim().to_string());
        let raw = json.as_ref().and_then(|j| j.get("proposal").cloned().or_else(|| j.get("action").map(|_| j.clone())));
        let parsed: Option<Result<Action, String>> = match raw {
            None | Some(serde_json::Value::Null) => None,
            Some(v) => Some(serde_json::from_value::<Action>(v).map_err(|e| format!("not an action the game knows ({e})")).and_then(|a| a.validate(w))),
        };
        let cost = match &parsed {
            Some(Ok(a)) => a.cost_estimate(w),
            _ => 0.0,
        };
        let m = w.committee.meeting.as_mut().unwrap();
        m.say(Speaker::Member(id), &name, LineKind::Speech, rationale.clone());
        match parsed {
            None => {}
            Some(Err(why)) => m.say(Speaker::Secretary, SECRETARY, LineKind::Note, format!("Out of order: {why}. Not tabled.")),
            Some(Ok(a)) => {
                let title = a.title();
                if m.proposals.iter().any(|p| p.title == title) {
                    m.say(Speaker::Secretary, SECRETARY, LineKind::Note, format!("\"{title}\" is already tabled."));
                } else {
                    let pid = m.proposals.len() as u32 + 1;
                    let cost_note = if cost > 0.0 { format!(", about {} credits", nd_sim::committee::report::thousands(cost)) } else { String::new() };
                    m.say(Speaker::Secretary, SECRETARY, LineKind::Note, format!("Tabled as #{pid}: {title}{cost_note}."));
                    m.proposals.push(Proposal { id: pid, by: id, action: a, title, rationale, cost, ballots: Vec::new(), mayor: None, outcome: None, project: None });
                }
            }
        }
        m.cursor += 1;
    }

    fn apply_votes(&mut self, w: &mut World, id: u32, d: &Delivered) {
        let name = w.committee.name_of(id);
        let Some(me) = w.committee.member(id).cloned() else { return };
        let m = w.committee.meeting.as_ref().unwrap();
        let mut ballots = Vec::new();
        for p in &m.proposals {
            let proposer = w.committee.member(p.by);
            let (lv, lr) = lean(&me, p, proposer, &m.report, m.seed);
            let (vote, reason) = d.reply.json.as_ref().and_then(|j| vote_in(j, p.id)).map(|(v, r)| (v, if r.trim().is_empty() { lr.clone() } else { r })).unwrap_or((lv, nd_sim::committee::capitalise(&lr)));
            ballots.push((p.id, vote, reason));
        }
        let m = w.committee.meeting.as_mut().unwrap();
        let mut parts = Vec::new();
        for (pid, vote, reason) in ballots {
            if let Some(p) = m.proposals.iter_mut().find(|p| p.id == pid) {
                p.ballots.push(Ballot { member: id, vote, reason: reason.clone() });
            }
            parts.push(format!("{} on #{pid} ({})", vote.key(), reason.trim_end_matches('.')));
        }
        m.say(Speaker::Member(id), &name, LineKind::Vote, format!("I vote {}.", parts.join("; ")));
        m.cursor += 1;
    }

    /// File passed proposal `idx` as a project.
    fn file(&mut self, w: &mut World, idx: usize, options: Vec<SiteOption>, pick: Option<usize>, reason: String) {
        let (number, prop) = {
            let m = w.committee.meeting.as_ref().unwrap();
            (m.number, m.proposals[idx].clone())
        };
        let label = pick.and_then(|i| options.get(i)).map(|o| o.label.clone());
        let pid = w.committee_file_project(number, &prop, options, pick, reason.clone());
        let pname = w.committee.project(pid).map(|p| p.name.clone()).unwrap_or_default();
        let m = w.committee.meeting.as_mut().unwrap();
        m.proposals[idx].project = Some(pid);
        let line = match label {
            Some(l) if prop.action.needs_site() => format!("For #{} {}: {l}. {} Filed as \"{pname}\" for the mayor's review.", prop.id, prop.title, reason.trim()),
            Some(_) => format!("#{} {}: filed as \"{pname}\" for the mayor to put into effect.", prop.id, prop.title),
            None => format!("For #{} {} the survey found no valid site. Mayor, I need your help: point me to ground for it, or let the city change and I will look again. Filed as \"{pname}\", waiting for you.", prop.id, prop.title),
        };
        m.say(Speaker::Secretary, SECRETARY, LineKind::Speech, line);
        m.cursor += 1;
    }

    /// Take the meeting forward until it needs an answer from the backend
    /// or the mayor, or it is over. Returns whether anything changed.
    pub fn advance(&mut self, w: &mut World, courier: &mut Courier) -> bool {
        let mut changed = false;
        for _ in 0..64 {
            if self.pending.is_some() {
                return changed;
            }
            let Some(m) = w.committee.meeting.as_ref() else { return changed };
            let (stage, cursor) = (m.stage, m.cursor);
            let submit = |w: &World, speaker: Option<u32>, task: Task, salt: u64, max: u32| -> TurnRequest { request(w, w.committee.meeting.as_ref().unwrap(), speaker, task, salt, max) };
            let next: Option<(TurnRequest, Step)> = match stage {
                Stage::Opening => {
                    let m = w.committee.meeting.as_ref().unwrap();
                    let members = m.members.iter().map(|&id| w.committee.member(id).map_or_else(String::new, |p| format!("{} ({})", p.name, p.scope.label()))).collect();
                    let since = w.committee.baseline.as_ref().map_or(0, |b| b.day);
                    let alerts = w.committee.alerts.iter().filter(|a| a.day >= since).count();
                    Some((submit(w, None, Task::Opening { meeting: m.number, members, alerts, on_demand: m.on_demand }, 1, 200), Step::Opening))
                }
                Stage::Status => {
                    let m = w.committee.meeting.as_ref().unwrap();
                    match m.members.get(cursor).copied() {
                        Some(id) => {
                            let p = w.committee.member(id).unwrap();
                            let facts = m.report.scope_facts(p.scope, w.committee.baseline.as_ref());
                            let since = w.committee.baseline.as_ref().map_or(0, |b| b.day);
                            let alerts = w.committee.alerts.iter().filter(|a| a.member == id && a.day >= since).map(|a| a.text.clone()).take(3).collect();
                            Some((submit(w, Some(id), Task::Status { facts, alerts, first: w.committee.baseline.is_none() }, 100 + cursor as u64, 250), Step::Status(id)))
                        }
                        None => {
                            w.committee.meeting.as_mut().unwrap().advance(Stage::Responses);
                            None
                        }
                    }
                }
                Stage::Responses => {
                    let m = w.committee.meeting.as_ref().unwrap();
                    let plan = response_plan(w, m);
                    match plan.get(cursor).copied() {
                        Some((a, b, agree)) => {
                            let pb = w.committee.member(b).unwrap();
                            let said = m.transcript.iter().find(|l| l.speaker == Speaker::Member(b) && l.kind == LineKind::Speech).map(|l| l.text.clone()).unwrap_or_default();
                            let facts = m.report.scope_facts(w.committee.member(a).unwrap().scope, w.committee.baseline.as_ref());
                            let task = Task::Response { to: pb.name.clone(), to_scope: pb.scope.label().into(), said, agree, facts, first: w.committee.baseline.is_none() };
                            Some((submit(w, Some(a), task, 200 + cursor as u64, 200), Step::Response(a)))
                        }
                        None => {
                            let m = w.committee.meeting.as_mut().unwrap();
                            m.say(Speaker::Secretary, SECRETARY, LineKind::Agenda, "2. Topics of concern");
                            m.advance(Stage::Proposals);
                            None
                        }
                    }
                }
                Stage::Proposals => {
                    let m = w.committee.meeting.as_ref().unwrap();
                    match m.proposers.get(cursor).copied() {
                        Some(id) => {
                            let p = w.committee.member(id).unwrap();
                            let suggestion = suggest(p, &m.report).and_then(|(a, why)| a.validate(w).ok().map(|a| (a, why)));
                            let areas = nd_sim::committee::report::areas(w).into_iter().map(|a| a.name).collect();
                            Some((submit(w, Some(id), Task::Propose { suggestion, areas }, 300 + cursor as u64, 300), Step::Propose(id)))
                        }
                        None => {
                            let m = w.committee.meeting.as_mut().unwrap();
                            if m.proposals.is_empty() {
                                m.say(Speaker::Secretary, SECRETARY, LineKind::Note, "No topics were tabled, so there is nothing to vote on.");
                                m.advance(Stage::Closing);
                            } else {
                                m.say(Speaker::Secretary, SECRETARY, LineKind::Agenda, "3. Votes");
                                m.advance(Stage::Votes);
                            }
                            None
                        }
                    }
                }
                Stage::Votes => {
                    let m = w.committee.meeting.as_ref().unwrap();
                    match m.members.get(cursor).copied() {
                        Some(id) => {
                            let me = w.committee.member(id).unwrap();
                            let items = m
                                .proposals
                                .iter()
                                .map(|p| {
                                    let (lv, lr) = lean(me, p, w.committee.member(p.by), &m.report, m.seed);
                                    VoteItem { id: p.id, title: p.title.clone(), by: w.committee.name_of(p.by), rationale: p.rationale.clone(), cost: p.cost, lean: lv, lean_reason: lr }
                                })
                                .collect();
                            Some((submit(w, Some(id), Task::Vote { items }, 400 + cursor as u64, 300), Step::Vote(id)))
                        }
                        None => {
                            let m = w.committee.meeting.as_mut().unwrap();
                            m.say(Speaker::Secretary, SECRETARY, LineKind::Agenda, "The mayor's vote");
                            m.advance(Stage::MayorVote);
                            None
                        }
                    }
                }
                Stage::MayorVote => {
                    if w.committee.meeting.as_ref().unwrap().waiting_for_mayor() {
                        return changed;
                    }
                    self.tally(w);
                    None
                }
                Stage::Implementation => {
                    let m = w.committee.meeting.as_ref().unwrap();
                    let passed: Vec<usize> = m.proposals.iter().enumerate().filter(|(_, p)| p.outcome == Some(Outcome::Passed)).map(|(i, _)| i).collect();
                    match passed.get(cursor).copied() {
                        Some(idx) => {
                            let action = m.proposals[idx].action.clone();
                            let title = m.proposals[idx].title.clone();
                            let options = site_options(w, &action);
                            if options.is_empty() {
                                self.file(w, idx, options, None, "no valid site".into());
                                None
                            } else if !action.needs_site() || options.len() == 1 {
                                let reason = if action.needs_site() { "It is the only valid site the survey found.".to_string() } else { "A policy: nothing to place.".to_string() };
                                self.file(w, idx, options, Some(0), reason);
                                None
                            } else {
                                let labels = options.iter().map(|o| format!("{} ({})", o.label, o.notes.join("; "))).collect();
                                Some((submit(w, None, Task::Pick { title, options: labels, best: 0 }, 500 + cursor as u64, 150), Step::Pick(idx, options)))
                            }
                        }
                        None => {
                            w.committee.meeting.as_mut().unwrap().advance(Stage::Closing);
                            None
                        }
                    }
                }
                Stage::Closing => {
                    if cursor > 0 {
                        return changed;
                    }
                    let m = w.committee.meeting.as_ref().unwrap();
                    let results = m.proposals.iter().map(|p| format!("#{} {} {}", p.id, p.title, p.outcome.map_or("not voted", |o| o.key()))).collect();
                    let projects = m.proposals.iter().filter(|p| p.project.is_some()).count();
                    let task = Task::Closing { results, projects, next_day: w.committee.next_meeting_day + 1 };
                    w.committee.meeting.as_mut().unwrap().cursor = 1;
                    Some((submit(w, None, task, 900, 150), Step::Closing))
                }
                Stage::Done => {
                    w.committee.file_meeting();
                    return true;
                }
            };
            changed = true;
            if let Some((req, step)) = next {
                let id = courier.submit(req);
                if let Some(m) = w.committee.meeting.as_mut() {
                    m.calls += 1;
                }
                self.pending = Some((id, step));
            }
        }
        changed
    }

    /// Count the votes, announce the results, let every member remember.
    fn tally(&mut self, w: &mut World) {
        let m = w.committee.meeting.as_mut().unwrap();
        let mut lines = Vec::new();
        for p in m.proposals.iter_mut() {
            let (t, outcome) = p.tally();
            p.outcome = Some(outcome);
            lines.push(match outcome {
                Outcome::Vetoed => format!("#{} {}: vetoed by the mayor ({} for, {} against).", p.id, p.title, t.yes, t.no),
                o => format!("#{} {}: {} {}-{}{}.", p.id, p.title, o.key(), t.yes, t.no, if t.abstain > 0 { format!(", {} abstaining", t.abstain) } else { String::new() }),
            });
        }
        for l in lines {
            m.say(Speaker::Secretary, SECRETARY, LineKind::Result, l);
        }
        let any = m.proposals.iter().any(|p| p.outcome == Some(Outcome::Passed));
        let (number, day, props, report) = (m.number, m.day, m.proposals.clone(), m.report.clone());
        if any {
            m.say(Speaker::Secretary, SECRETARY, LineKind::Agenda, "4. Implementation");
            m.advance(Stage::Implementation);
        } else {
            m.advance(Stage::Closing);
        }
        w.committee.remember_votes(number, day, &props, &report);
    }

    /// Run until the meeting needs the mayor or is over, polling the
    /// courier (for the stand-in, which answers at once; tests and the
    /// remote API's fast-forward). Gives up after `max_rounds` polls.
    ///
    /// Returns the deliveries that were not the meeting's (alerts, voice
    /// lines, diaries polled on the way): the caller hands them on, or they
    /// would stay pending for ever.
    #[must_use = "deliveries that are not the meeting's must be handed on"]
    pub fn run(&mut self, w: &mut World, courier: &mut Courier, max_rounds: usize) -> Vec<Delivered> {
        let mut others = Vec::new();
        for _ in 0..max_rounds {
            self.advance(w, courier);
            for d in courier.poll() {
                if !self.take(w, &d) {
                    others.push(d);
                }
            }
            let Some(m) = w.committee.meeting.as_ref() else { return others };
            if self.pending.is_none() && (m.waiting_for_mayor() || m.is_done()) {
                self.advance(w, courier);
                return others;
            }
            if self.pending.is_some() && !courier.is_mock() {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        others
    }
}
