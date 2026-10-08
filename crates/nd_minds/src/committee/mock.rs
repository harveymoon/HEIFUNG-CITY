//! [committee] The stand-in: what the committee says with no model at all.
//! Templates flavoured by each member's temper, filled with the real
//! figures of the request, and the rules' own proposals and votes; every
//! choice comes from the request's seed, so a meeting replays word for word.

use nd_sim::committee::report::Fact;
use nd_sim::committee::{capitalise, mix, Vote};
use serde_json::json;

use super::{CommitteeBackend, PersonaCard, Task, TurnReply, TurnRequest};

#[derive(Default)]
pub struct MockCommittee {
    ready: Vec<(u64, TurnReply)>,
}

impl CommitteeBackend for MockCommittee {
    fn submit(&mut self, id: u64, req: TurnRequest) {
        let r = self.answer(&req);
        self.ready.push((id, r));
    }

    fn poll(&mut self) -> Vec<(u64, Result<TurnReply, String>)> {
        self.ready.drain(..).map(|(id, r)| (id, Ok(r))).collect()
    }

    fn name(&self) -> String {
        "stand-in (templates)".into()
    }

    fn is_mock(&self) -> bool {
        true
    }
}

/// A choice from `list` by seed and salt.
fn pick<'a>(seed: u64, salt: u64, list: &[&'a str]) -> &'a str {
    list[(mix(seed, salt) % list.len() as u64) as usize]
}

fn chance(seed: u64, salt: u64, p: f32) -> bool {
    (mix(seed, salt) % 1000) as f32 / 1000.0 < p
}

/// Lower is better for these figures.
fn bad_when_up(label: &str) -> bool {
    ["unhoused", "burden", "crime", "unpowered", "wear", "contamin", "unemploy", "unanswered", "brownout", "waiting", "spending", "worst"].iter().any(|k| label.contains(k))
}

/// Is the fact bad news? (a worsening trend, or a level past a plain line)
fn is_bad(f: &Fact) -> bool {
    if let Some(ch) = f.change() {
        if ch.abs() >= 0.05 {
            return (ch > 0.0) == bad_when_up(&f.label);
        }
    }
    let l = f.label.as_str();
    match f.unit.as_str() {
        "%" if l.contains("cover") || l.contains("near a") => f.value < 0.5,
        "%" if bad_when_up(l) => f.value > 0.3,
        "credits" if l.contains("net") => f.value < 0.0,
        "credits" if l.contains("treasury") => f.value < 10_000.0,
        _ if bad_when_up(l) => f.value > 0.0 && !l.contains("demand"),
        _ => false,
    }
}

fn trend(f: &Fact, since: &str) -> Option<String> {
    let ch = f.change()?;
    if ch.abs() < 0.03 {
        return Some(format!("flat {since}"));
    }
    Some(format!("{} {:.0}% {since}", if ch > 0.0 { "up" } else { "down" }, ch.abs() * 100.0))
}

fn opener(p: &PersonaCard, seed: u64) -> &'static str {
    let list: &[&str] = match p.temper_key.as_str() {
        "firebrand" => &["Let me be blunt.", "I'll say it plainly, because nobody else at this table will.", "This should alarm every one of us."],
        "technocrat" => &["The figures, then.", "Three numbers matter this month.", "Briefly, and by the data."],
        "populist" => &["I spent the week on the street, and people are talking.", "Ask anyone in a corner shop.", "Ordinary people feel this long before we do."],
        "pragmatist" => &["Here is where we stand.", "No drama: the state of things.", "Short version."],
        "idealist" => &["This committee exists for one reason.", "We owe this city better than drift.", "Principle first, then the numbers."],
        "old_guard" => &["I have seen this pattern before.", "In my experience, and it is long,", "We have been here before."],
        "contrarian" => &["Before everyone nods along,", "I'll take the unpopular view.", "Not what you want to hear, but"],
        _ => &["I'll keep it short, and I'll remember who listened.", "My report, and my door is open after.", "Let's be practical and talk trade-offs."],
    };
    pick(seed, 1, list)
}

fn verdict(p: &PersonaCard, bad: bool, seed: u64) -> &'static str {
    let list: &[&str] = match (p.temper_key.as_str(), bad) {
        ("firebrand", true) => &["That is a failure, and it has names attached.", "Unacceptable. I want action today, not a working group.", "If we sit on this, we deserve what comes."],
        ("firebrand", false) => &["Good. Now don't let it slip.", "Finally some progress, no thanks to the doubters.", "We hold this line or I'll be back shouting."],
        ("technocrat", true) => &["The trend is negative; it needs a decision, not a debate.", "On these numbers, waiting costs more than acting.", "The data is unambiguous."],
        ("technocrat", false) => &["Within tolerance. No action needed this cycle.", "Stable. I recommend we leave it alone.", "The numbers are fine; let's not fiddle."],
        ("populist", true) => &["Real people are paying for this, not us.", "Families can't wait for our next meeting.", "Folks are angry, and frankly they're right."],
        ("populist", false) => &["People notice, and they're grateful.", "For once the street and the spreadsheet agree.", "Good news for ordinary folks."],
        ("pragmatist", true) => &["It's fixable, but only if we fix it.", "Not a crisis yet. It will be.", "One sensible decision would turn this."],
        ("pragmatist", false) => &["Steady. I'd spend our energy elsewhere.", "It's working; keep it boring.", "Nothing to add. It holds."],
        ("idealist", true) => &["Half-measures got us here.", "This is a choice we keep making by not choosing.", "The city deserves a real answer."],
        ("idealist", false) => &["Good, but good is not the goal.", "Progress. Not enough, but progress.", "Let's aim higher while we can."],
        ("old_guard", true) => &["Last time this happened we waited, and it cost us.", "We knew this was coming.", "It always starts like this."],
        ("old_guard", false) => &["Steady as she goes.", "No need to reinvent anything.", "Fine. Let's not get clever."],
        ("contrarian", true) => &["Everyone will blame the obvious cause. I doubt it's the right one.", "Bad, yes. But not for the reasons you'll hear today.", "The fashionable fix won't work."],
        ("contrarian", false) => &["Good numbers make committees lazy.", "Fine figures. That's when mistakes get made.", "Don't congratulate yourselves yet."],
        (_, true) => &["I'll back whoever has a fix, and I'll expect the favour returned.", "There's a deal to be made here.", "We need votes on this, and I can find some."],
        (_, false) => &["Fine, and I'll trade my quiet for support elsewhere.", "No complaints; remember that when I need a vote.", "Good. I'll spend my capital on other things."],
    };
    pick(seed, 2, list)
}

fn fact_sentence(f: &Fact, seed: u64, salt: u64, since: &str) -> String {
    let base = f.line();
    match trend(f, since) {
        Some(t) if f.before.is_some() => {
            let base = format!("{} {}", f.label, f.show(f.value));
            match mix(seed, salt) % 3 {
                0 => format!("{} ({t})", capitalise(&base)),
                1 => format!("{}, {t}", capitalise(&base)),
                _ => format!("We are at {base}, {t}"),
            }
        }
        _ => capitalise(&base),
    }
}

impl MockCommittee {
    /// The stand-in's answer to any turn (also used by the courier when a
    /// model fails).
    pub fn answer(&self, req: &TurnRequest) -> TurnReply {
        let seed = req.seed;
        let text = |t: String| TurnReply { text: t, json: None };
        match &req.task {
            Task::Ping => text("ready".into()),
            Task::Dialogue { template, .. } => text(super::voices::mock_dialogue(template)),
            Task::Diary { who, day, entries, talks } => TurnReply { text: super::voices::mock_diary(who, *day, entries, talks, seed), json: Some(json!({ "stand_in": true })) },
            Task::Opening { meeting, members, alerts, on_demand } => {
                let open = pick(seed, 3, &["The committee is in session.", "Order, please. We are in session.", "Good evening. The committee is in session."]);
                let called = if *on_demand { " This meeting was called by the mayor." } else { "" };
                text(format!(
                    "{open} Meeting {meeting} of the {} committee, day {}.{called} Present: {}. {} since we last met. The agenda: status reports, topics of concern, votes, and implementation.",
                    if req.city.starts_with("the ") { req.city.trim_start_matches("the ").to_string() } else { req.city.clone() },
                    req.day + 1,
                    members.join(", "),
                    match alerts {
                        0 => "No alerts were raised".to_string(),
                        1 => "One alert was raised".to_string(),
                        n => format!("{n} alerts were raised"),
                    }
                ))
            }
            Task::Alert { rule, severity, facts, .. } => {
                let p = req.speaker.as_ref();
                let hook = match rule.as_str() {
                    "transit_riders_down" => "Riders are leaving the buses.",
                    "transit_no_buses" => "This town has outgrown walking.",
                    "housing_rent_burden" => "Rents are eating people's wages.",
                    "housing_unhoused" => "People are sleeping on the street.",
                    "housing_shortage" => "There is nowhere left to rent.",
                    "finance_deficit" => "We are spending money we do not have.",
                    "finance_low_treasury" => "The treasury is nearly dry.",
                    "safety_crime_up" => "Crime is climbing.",
                    "safety_police_coverage" | "safety_fire_coverage" => "Whole streets are out of reach of a station.",
                    "safety_unanswered" => "Calls are going unanswered.",
                    "environment_contamination_spreading" | "environment_hotspot" => "The ground is being poisoned.",
                    "business_unemployment" => "People cannot find work.",
                    "business_demand_unmet" => "Shops want to open and there is nowhere to put them.",
                    "utilities_unpowered" | "utilities_brownouts" => "The lights are going out.",
                    "utilities_road_wear" => "The roads are falling apart.",
                    "welfare_health_coverage" => "Too many homes are far from a clinic.",
                    "welfare_school_coverage" => "Children are too far from a school.",
                    _ => "Something in my portfolio needs your eye.",
                };
                let figures = facts.iter().enumerate().map(|(i, f)| fact_sentence(f, seed, 10 + i as u64, "on the week before")).collect::<Vec<_>>().join("; ");
                let ask = match severity {
                    3 => pick(seed, 4, &["This cannot wait for the meeting.", "Mayor, I need a decision now.", "Act on this before it spreads."]),
                    2 => pick(seed, 4, &["I want it on the next agenda.", "We should talk about this soon.", "Please keep an eye on it."]),
                    _ => pick(seed, 4, &["Worth watching.", "For your information.", "Just so you know."]),
                };
                let lead = match p.map(|p| p.temper_key.as_str()) {
                    Some("firebrand") => "Mayor! ",
                    Some("technocrat") => "Note: ",
                    Some("populist") => "Mayor, people are hurting. ",
                    Some("old_guard") => "A word of warning. ",
                    _ => "",
                };
                text(format!("{lead}{hook} {figures}. {ask}"))
            }
            Task::Status { facts, alerts, first } => {
                let since = if *first { "on the week before" } else { "since we last met" };
                let Some(p) = &req.speaker else { return text(String::new()) };
                let mut parts = vec![opener(p, seed).to_string()];
                let shown: Vec<&Fact> = facts.iter().take(3).collect();
                for (i, f) in shown.iter().enumerate() {
                    parts.push(format!("{}.", fact_sentence(f, seed, 20 + i as u64, since)));
                }
                let bad = facts.iter().take(3).any(is_bad);
                parts.push(verdict(p, bad, seed).to_string());
                if !alerts.is_empty() {
                    parts.push(format!("I raised {} alert{} since we last met, and I stand by {}.", alerts.len(), if alerts.len() == 1 { "" } else { "s" }, if alerts.len() == 1 { "it" } else { "them" }));
                }
                if let Some(m) = p.memory.last().filter(|_| chance(seed, 5, 0.45)) {
                    parts.push(format!("For the record: {}.", m.trim_end_matches('.')));
                } else if chance(seed, 6, 0.4) {
                    parts.push(format!("My aim has not changed: {}.", p.goals.first().cloned().unwrap_or_default()));
                }
                text(parts.join(" "))
            }
            Task::Response { to, to_scope, agree, facts, first, .. } => {
                let since = if *first { "on the week before" } else { "since we last met" };
                let Some(p) = &req.speaker else { return text(String::new()) };
                let mine = facts.first().map(|f| fact_sentence(f, seed, 30, since)).unwrap_or_default();
                let first = to.split_whitespace().next().unwrap_or(to);
                let t = if *agree {
                    pick(seed, 7, &["{first} is right, and from {mine_scope} it looks the same: {mine}.", "I'll back {first} on this. {mine}, and the two are connected.", "Agreed. {mine}; {to_scope} and {mine_scope} rise or fall together."])
                } else {
                    match p.temper_key.as_str() {
                        "firebrand" => pick(seed, 7, &["With respect, {first} has it backwards. {mine}, and nobody asked about that.", "No. Fixing {to_scope} at any price leaves {mine_low}."]),
                        "technocrat" => pick(seed, 7, &["The numbers say otherwise, {first}. {mine}.", "Correlation is not cause. {mine}, which matters more."]),
                        "contrarian" => pick(seed, 7, &["I don't buy it, {first}. {mine}.", "Everyone will agree with {first}. I won't: {mine_low}."]),
                        _ => pick(seed, 7, &["I hear {first}, but {mine_low} is the bigger problem.", "Before we spend on {to_scope}: {mine_low}.", "Fair, {first}, but remember {mine_low}."]),
                    }
                };
                let mine_low = {
                    let mut c = mine.chars();
                    match c.next() {
                        Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
                        None => String::new(),
                    }
                };
                text(t.replace("{first}", first).replace("{to_scope}", &to_scope.to_lowercase()).replace("{mine_scope}", &p.scope.to_lowercase()).replace("{mine_low}", &mine_low).replace("{mine}", &mine))
            }
            Task::Propose { suggestion, .. } => {
                let Some(p) = &req.speaker else { return text(String::new()) };
                match suggestion {
                    Some((a, why)) => {
                        let lead = match p.temper_key.as_str() {
                            "firebrand" => "I move, and I want a vote today:",
                            "technocrat" => "Proposal, on the evidence:",
                            "populist" => "For the people out there, I propose:",
                            "idealist" => "I propose we do the right thing:",
                            "old_guard" => "A tried and tested measure:",
                            "contrarian" => "Here is what nobody else will propose:",
                            "dealmaker" => "A proposal, and I'll remember who backs it:",
                            _ => "I propose:",
                        };
                        let rationale = format!("{lead} {}. Because {why}.", a.title());
                        TurnReply { text: rationale.clone(), json: Some(json!({ "proposal": a, "rationale": rationale })) }
                    }
                    None => {
                        let r = pick(seed, 8, &["Nothing in my portfolio needs a vote this month.", "I have no motion. Let's not legislate for the sake of it.", "I pass. My figures can wait."]).to_string();
                        TurnReply { text: r.clone(), json: Some(json!({ "proposal": null, "rationale": r })) }
                    }
                }
            }
            Task::Vote { items } => {
                let votes: Vec<serde_json::Value> = items.iter().map(|it| json!({ "proposal": it.id, "vote": it.lean.key(), "reason": capitalise(&it.lean_reason) })).collect();
                let summary = items.iter().map(|it| format!("{} on #{}", it.lean.key(), it.id)).collect::<Vec<_>>().join(", ");
                TurnReply { text: format!("I vote {summary}."), json: Some(json!({ "votes": votes })) }
            }
            Task::Pick { options, best, .. } => {
                let i = (*best).min(options.len().saturating_sub(1));
                let reason = pick(seed, 9, &["it is the best of the valid sites the survey found", "it does the most good for the least disruption", "it reaches the most people who need it"]);
                let r = format!("Option {} of {}, because {reason}.", i + 1, options.len());
                TurnReply { text: r.clone(), json: Some(json!({ "choice": i + 1, "reason": r })) }
            }
            Task::Closing { results, projects, next_day } => {
                let res = if results.is_empty() { "Nothing came to a vote".to_string() } else { results.join("; ") };
                let close = pick(seed, 11, &["That concludes the business of this meeting.", "The meeting is adjourned.", "Thank you, members. We are adjourned."]);
                text(format!("{res}. {} {close} We meet again on day {next_day}.", match projects {
                    0 => "Nothing goes to the mayor's desk.".to_string(),
                    1 => "One project goes to the mayor for review.".to_string(),
                    n => format!("{n} projects go to the mayor for review."),
                }))
            }
        }
    }
}

/// The vote a reply gives for a proposal, if it gives one.
pub fn vote_in(json: &serde_json::Value, proposal: u32) -> Option<(Vote, String)> {
    let votes = json.get("votes")?.as_array()?;
    for v in votes {
        let id = v.get("proposal").and_then(|x| x.as_u64().or_else(|| x.as_str().and_then(|s| s.trim_start_matches('#').parse().ok())))?;
        if id as u32 == proposal {
            let vote = Vote::parse(v.get("vote")?.as_str()?)?;
            let reason = v.get("reason").and_then(|r| r.as_str()).unwrap_or("").to_string();
            return Some((vote, reason));
        }
    }
    None
}
