//! [committee] `committee` and `committee_meeting`: the committee's state,
//! seats, pool, alerts, projects and settings; calling a meeting, the
//! mayor's votes, fast-forward, the transcript, proposals and votes. See
//! docs/REMOTE.md and docs/AI_COMMITTEE.md.

use nd_minds::committee::driver::abstain_rest;
use nd_minds::committee::settings::BackendChoice;
use nd_minds::committee::cast_mayor_vote;
use nd_sim::committee::{rules, Alert, CityReport, CommitteeProject, Line, MayorVote, Meeting, Politician, Proposal, Question, Speaker};
use serde_json::{json, Value};

use super::view::*;
use crate::committee::CommitteeRt;
use crate::sim::Sim;

fn member(p: &Politician, seat: Option<usize>, full: bool) -> Value {
    let mut v = json!({
        "id": p.id, "name": p.name, "seat": seat.map(|s| s + 1), "scope": p.scope.key(), "title": p.scope.title(),
        "temper": p.temper.key(), "party": p.party, "age": p.age, "leanings": p.leanings.describe(),
        "spending": round(p.leanings.spending), "growth": round(p.leanings.growth), "order": round(p.leanings.order),
        "goals": p.goals(),
    });
    if full {
        v["background"] = json!(p.background);
        v["record"] = serde_json::to_value(&p.record).unwrap_or(Value::Null);
        v["memory"] = json!(p.memory.iter().map(|m| m.line()).collect::<Vec<_>>());
    }
    v
}

fn alert(a: &Alert, sim: &Sim) -> Value {
    json!({
        "id": a.id, "day": a.day + 1, "member": a.member, "name": sim.world.committee.name_of(a.member), "rule": a.rule,
        "when": rules::rule(&a.rule).map(|r| r.when), "severity": a.severity, "text": a.text, "phrased": a.phrased, "seen": a.seen,
        "facts": a.facts.iter().map(|f| f.line()).collect::<Vec<_>>(),
    })
}

fn project(p: &CommitteeProject) -> Value {
    json!({
        "id": p.id, "name": p.name, "meeting": p.meeting, "proposal": p.proposal, "by": p.by, "status": p.status.key(),
        "status_detail": serde_json::to_value(&p.status).unwrap_or(Value::Null),
        "action": serde_json::to_value(&p.action).unwrap_or(Value::Null),
        "pick": p.pick.map(|i| i + 1), "reason": p.reason, "world_project": p.world_project,
        "options": p.options.iter().enumerate().map(|(i, o)| json!({
            "option": i + 1, "label": o.label, "point": o.point.map(|q| [round(q.x), round(q.y)]), "parcels": o.parcels.len(),
            "stops": o.stops.iter().map(|q| [round(q.x), round(q.y)]).collect::<Vec<_>>(), "radius": round(o.radius),
            "area": o.area, "area_m2": round(o.area_m2), "cost": round(o.cost), "score": round(o.score), "notes": o.notes,
        })).collect::<Vec<_>>(),
        "log": p.log,
    })
}

fn line(l: &Line) -> Value {
    let who = match l.speaker {
        Speaker::Secretary => json!("secretary"),
        Speaker::Mayor => json!("mayor"),
        Speaker::Member(id) => json!(id),
    };
    json!({ "speaker": who, "name": l.name, "kind": format!("{:?}", l.kind).to_ascii_lowercase(), "text": l.text })
}

fn proposal(p: &Proposal, sim: &Sim) -> Value {
    let (t, outcome) = p.tally();
    json!({
        "id": p.id, "title": p.title, "by": p.by, "by_name": sim.world.committee.name_of(p.by),
        "action": serde_json::to_value(&p.action).unwrap_or(Value::Null), "rationale": p.rationale, "cost": round(p.cost),
        "votes": p.ballots.iter().map(|b| json!({ "member": b.member, "name": sim.world.committee.name_of(b.member), "vote": b.vote.key(), "reason": b.reason })).collect::<Vec<_>>(),
        "mayor": p.mayor.map(|v| v.key()), "tally": { "yes": t.yes, "no": t.no, "abstain": t.abstain },
        "outcome": p.outcome.map(|o| o.key()), "would_be": outcome.key(), "project": p.project,
    })
}

fn meeting(m: &Meeting, sim: &Sim, from: usize) -> Value {
    json!({
        "number": m.number, "day": m.day + 1, "stage": m.stage.label(), "on_demand": m.on_demand, "calls": m.calls,
        "waiting_for_mayor": m.waiting_for_mayor(), "done": m.is_done(),
        "members": m.members, "proposers": m.proposers,
        "questions_digest": m.questions, "questions_read": m.questions_read,
        "transcript_from": from, "transcript_len": m.transcript.len(),
        "transcript": m.transcript.iter().skip(from).map(line).collect::<Vec<_>>(),
        "proposals": m.proposals.iter().map(|p| proposal(p, sim)).collect::<Vec<_>>(),
    })
}

/// [ask-committee] One question thread.
fn question(q: &Question, sim: &Sim, rt: &CommitteeRt) -> Value {
    let c = &sim.world.committee;
    let typing: Vec<u32> = rt.asker.typing().into_iter().filter(|(qid, _)| *qid == q.id).map(|(_, m)| m).collect();
    json!({
        "id": q.id, "day": q.day + 1, "text": q.text, "discussed": q.discussed, "complete": q.is_complete(),
        "asked": q.asked.iter().map(|&id| json!({ "id": id, "name": c.name_of(id) })).collect::<Vec<_>>(),
        "waiting": q.waiting(), "typing": typing,
        "replies": q.replies.iter().map(|r| json!({ "member": r.member, "name": r.name, "scope": r.scope, "respond": r.respond, "text": r.text, "stand_in": r.stand_in })).collect::<Vec<_>>(),
    })
}

fn backend(rt: &CommitteeRt) -> Value {
    let s = &rt.settings;
    json!({
        "choice": s.backend.key(), "speaking_through": rt.courier.backend_name(), "error": rt.backend_error,
        "local_url": s.local_url, "local_model": s.local_model, "claude_model": s.claude_model, "claude_effort": s.claude_effort,
        "claude_fallbacks": s.claude_fallbacks, "claude_key": s.key_source().label(), "timeout_s": s.timeout_s, "max_tokens": s.max_tokens,
        "calls": rt.courier.calls, "fallbacks": rt.courier.fallbacks, "errors": rt.courier.errors,
        "test": match &rt.test_result { Some(Ok(t)) => json!({ "ok": true, "text": t }), Some(Err(e)) => json!({ "ok": false, "error": e }), None => if rt.test.is_some() { json!("running") } else { Value::Null } },
    })
}

fn state(sim: &Sim, rt: &CommitteeRt) -> Value {
    let c = &sim.world.committee;
    let seats: Vec<Value> = c.seats.iter().enumerate().map(|(i, s)| s.and_then(|id| c.member(id)).map_or_else(|| json!({ "seat": i + 1, "empty": true }), |p| member(p, Some(i), true))).collect();
    json!({
        "day": sim.world.committee_today() + 1,
        "next_meeting_day": c.next_meeting_day + 1, "meetings_held": c.meetings_held, "auto_meetings": c.auto_meetings, "veto_allowed": c.veto_allowed,
        "in_session": c.meeting.as_ref().map(|m| json!({ "number": m.number, "stage": m.stage.label(), "waiting_for_mayor": m.waiting_for_mayor() })),
        "seats": seats,
        "candidates": c.candidates().iter().map(|p| member(p, None, false)).collect::<Vec<_>>(),
        "alerts": c.alerts.iter().rev().take(10).map(|a| alert(a, sim)).collect::<Vec<_>>(),
        "unseen_alerts": c.unseen_alerts(),
        "projects": c.projects.iter().rev().take(10).map(project).collect::<Vec<_>>(),
        "backend": backend(rt),
    })
}

fn id_of(cmd: &Value, name: &str) -> Result<u32, String> {
    opt_u64(cmd, name)?.map(|v| v as u32).ok_or_else(|| format!("missing {name}"))
}

/// `committee {action?}`.
pub fn command(cmd: &Value, sim: &mut Sim, rt: &mut CommitteeRt) -> Result<Value, String> {
    sim.world.committee_ensure();
    let today = sim.world.committee_today();
    let action = opt_str(cmd, "action")?.unwrap_or("state");
    match action {
        "state" | "status" => Ok(state(sim, rt)),
        "members" | "pool" => {
            let c = &sim.world.committee;
            Ok(json!({ "pool": c.pool.iter().map(|p| member(p, c.seat_of(p.id), true)).collect::<Vec<_>>() }))
        }
        "appoint" => {
            let id = id_of(cmd, "id")?;
            let seat = opt_u64(cmd, "seat")?.map(|s| (s as usize).saturating_sub(1));
            let s = sim.world.committee.appoint(id, seat, today)?;
            Ok(json!({ "appointed": id, "name": sim.world.committee.name_of(id), "seat": s + 1 }))
        }
        "dismiss" => {
            let id = id_of(cmd, "id")?;
            let s = sim.world.committee.dismiss(id, today)?;
            Ok(json!({ "dismissed": id, "name": sim.world.committee.name_of(id), "seat": s + 1 }))
        }
        "alerts" => {
            let limit = opt_u64(cmd, "limit")?.unwrap_or(50) as usize;
            let v: Vec<Value> = sim.world.committee.alerts.iter().rev().take(limit).map(|a| alert(a, sim)).collect();
            if opt_bool(cmd, "mark_seen")?.unwrap_or(false) {
                for a in sim.world.committee.alerts.iter_mut() {
                    a.seen = true;
                }
            }
            Ok(json!({ "alerts": v, "rules": rules::RULES.iter().map(|r| json!({ "key": r.key, "scope": r.scope.key(), "when": r.when, "cooldown_days": r.cooldown })).collect::<Vec<_>>() }))
        }
        "check_alerts" => {
            // Run today's rules now (normally once a sim day).
            let ids = sim.world.committee_daily();
            for id in &ids {
                rt.driver.phrase_alert(&sim.world, *id, &mut rt.courier);
            }
            Ok(json!({ "raised": ids }))
        }
        "projects" => Ok(json!({ "projects": sim.world.committee.projects.iter().map(project).collect::<Vec<_>>() })),
        "implement" => {
            let id = id_of(cmd, "project")?;
            let done = sim.world.committee_implement(id)?;
            Ok(json!({ "implemented": id, "done": done, "project": sim.world.committee.project(id).map(project) }))
        }
        "dismiss_project" => {
            let id = id_of(cmd, "project")?;
            sim.world.committee_dismiss_project(id)?;
            Ok(json!({ "dismissed": id }))
        }
        "pick_site" => {
            let id = id_of(cmd, "project")?;
            let opt = opt_u64(cmd, "option")?.ok_or("missing option (1-based)")? as usize;
            sim.world.committee_pick_site(id, opt.saturating_sub(1))?;
            Ok(json!({ "project": sim.world.committee.project(id).map(project) }))
        }
        "refresh_sites" => {
            let id = id_of(cmd, "project")?;
            let n = sim.world.committee_refresh_sites(id)?;
            Ok(json!({ "found": n, "project": sim.world.committee.project(id).map(project) }))
        }
        "report" => {
            let r = CityReport::build(&sim.world);
            Ok(json!({ "text": r.to_text(), "report": serde_json::to_value(&r).unwrap_or(Value::Null) }))
        }
        "ask" => {
            // [ask-committee] Put a question to the seated members.
            let text = opt_str(cmd, "text")?.or(opt_str(cmd, "question")?).ok_or("missing text (the question)")?.to_string();
            let id = rt.ask(sim, &text)?;
            // The stand-in answers at once: collect the thread now. A model
            // answers on later frames: read `questions` again.
            if rt.courier.is_mock() {
                let others = rt.asker.run(&mut sim.world, &mut rt.courier, 64);
                for d in others {
                    rt.hand_on(&mut sim.world, &d);
                }
            }
            let q = sim.world.committee.question(id).ok_or("the question was not kept")?;
            Ok(json!({ "asked": id, "backend": rt.courier.backend_name(), "question": question(q, sim, rt) }))
        }
        "questions" => {
            let c = &sim.world.committee;
            if let Some(id) = opt_u64(cmd, "id")? {
                let q = c.question(id as u32).ok_or_else(|| format!("no question {id}"))?;
                return Ok(json!({ "question": question(q, sim, rt) }));
            }
            let limit = opt_u64(cmd, "limit")?.unwrap_or(20) as usize;
            let undiscussed = c.undiscussed().len();
            Ok(json!({
                "questions": c.questions.iter().rev().take(limit).map(|q| question(q, sim, rt)).collect::<Vec<_>>(),
                "kept": c.questions.len(), "undiscussed": undiscussed, "answered": rt.asker.answered,
                "next_meeting_digest": c.questions_digest(),
            }))
        }
        "minutes" => {
            let c = &sim.world.committee;
            let m = match opt_u64(cmd, "meeting")? {
                Some(n) => c.minutes.iter().find(|m| m.number == n as u32),
                None => c.minutes.last(),
            };
            Ok(json!({ "kept": c.minutes.iter().map(|m| m.number).collect::<Vec<_>>(), "minutes": m.map(|m| meeting(m, sim, 0)) }))
        }
        "settings" => {
            // Never takes or returns an API key: set ANTHROPIC_API_KEY or use
            // the window. [fix/review-app] The backend and its connection
            // change only when the player allowed it in the window (a model
            // may cost money; a URL points the committee at a server).
            let wants_switch = ["backend", "local_url", "local_model", "claude_model", "claude_effort", "claude_fallbacks", "timeout_s"].iter().any(|k| field(cmd, k).is_some());
            if wants_switch && !rt.remote_may_switch {
                return Err("the backend settings are the player's: tick \"Let the remote API change the backend\" in the Committee window's settings first".into());
            }
            let mut changed = false;
            if let Some(b) = opt_str(cmd, "backend")? {
                rt.settings.backend = BackendChoice::parse(b).ok_or_else(|| format!("unknown backend {b:?}; mock, local or claude"))?;
                changed = true;
            }
            for (k, slot) in [("local_url", 0), ("local_model", 1), ("claude_model", 2), ("claude_effort", 3)] {
                if let Some(v) = opt_str(cmd, k)? {
                    let s = &mut rt.settings;
                    *[&mut s.local_url, &mut s.local_model, &mut s.claude_model, &mut s.claude_effort][slot] = v.trim().to_string();
                    changed = true;
                }
            }
            if let Some(v) = opt_bool(cmd, "claude_fallbacks")? {
                rt.settings.claude_fallbacks = v;
                changed = true;
            }
            if let Some(v) = opt_u64(cmd, "timeout_s")? {
                rt.settings.timeout_s = v.clamp(5, 600);
                changed = true;
            }
            if let Some(v) = opt_bool(cmd, "auto_meetings")? {
                sim.world.committee.auto_meetings = v;
            }
            if let Some(v) = opt_bool(cmd, "veto_allowed")? {
                sim.world.committee.veto_allowed = v;
            }
            if changed {
                rt.apply_settings();
                if opt_bool(cmd, "save")?.unwrap_or(false) {
                    rt.save_settings()?;
                }
            }
            // One test at a time (each holds a thread for up to a minute).
            if opt_bool(cmd, "test")?.unwrap_or(false) && rt.test.is_none() {
                rt.test_connection();
            }
            Ok(json!({ "remote_may_switch": rt.remote_may_switch, "backend": backend(rt), "auto_meetings": sim.world.committee.auto_meetings, "veto_allowed": sim.world.committee.veto_allowed }))
        }
        other => Err(format!("unknown committee action {other:?}; state, members, appoint, dismiss, alerts, check_alerts, projects, implement, dismiss_project, pick_site, refresh_sites, report, ask, questions, minutes, settings")),
    }
}

/// `committee_meeting {start | vote | abstain_rest | fast_forward}`: then
/// the meeting as it stands (or the last minutes).
pub fn meeting_command(cmd: &Value, sim: &mut Sim, rt: &mut CommitteeRt) -> Result<Value, String> {
    sim.world.committee_ensure();
    let mut started = None;
    if opt_bool(cmd, "start")?.unwrap_or(false) {
        started = Some(rt.call_meeting(sim)?);
    }
    if let Some(v) = field(cmd, "vote") {
        // {vote: {proposal, vote}} or {vote: "yes", proposal: n}
        let (pid, vote) = match v {
            Value::Object(_) => (v.get("proposal").and_then(Value::as_u64).ok_or("vote.proposal missing")?, v.get("vote").and_then(Value::as_str).ok_or("vote.vote missing")?),
            Value::String(s) => (opt_u64(cmd, "proposal")?.ok_or("missing proposal")?, s.as_str()),
            _ => return Err("vote: {proposal, vote} or a string with proposal".into()),
        };
        let mv = MayorVote::parse(vote).ok_or_else(|| format!("unknown vote {vote:?}; yes, no, abstain or veto"))?;
        cast_mayor_vote(&mut sim.world, pid as u32, mv)?;
    }
    if opt_bool(cmd, "abstain_rest")?.unwrap_or(false) {
        abstain_rest(&mut sim.world)?;
    }
    if opt_bool(cmd, "fast_forward")?.unwrap_or(false) || opt_bool(cmd, "skip")?.unwrap_or(false) {
        rt.fast_forward = true;
        // The stand-in answers at once: run to the mayor's vote or the end
        // now. A model keeps its own pace (the frames pump it).
        if rt.courier.is_mock() {
            // [fix/review-app] Voice lines, diaries (and [ask-committee]
            // answers) polled on the way are theirs, not dropped.
            let others = rt.driver.run(&mut sim.world, &mut rt.courier, 400);
            for d in others {
                rt.hand_on(&mut sim.world, &d);
            }
        }
    }
    let from = opt_u64(cmd, "transcript_from")?.unwrap_or(0) as usize;
    let c = &sim.world.committee;
    Ok(match &c.meeting {
        Some(m) => json!({ "started": started, "in_session": true, "meeting": meeting(m, sim, from) }),
        None => json!({ "started": started, "in_session": false, "next_meeting_day": c.next_meeting_day + 1, "last": c.minutes.last().map(|m| meeting(m, sim, from)) }),
    })
}

/// [voices] `diary {citizen, refresh?}`: a citizen's diary of today, written
/// on demand from their memories (the stand-in at once; a model on a later
/// frame: call again until `pending` is false).
pub fn diary(cmd: &Value, sim: &mut Sim, rt: &mut CommitteeRt) -> Result<Value, String> {
    let id: nd_sim::CitizenId = key_from(field(cmd, "citizen").ok_or("missing citizen id")?)?;
    if !sim.world.citizens.contains_key(id) {
        return Err("no such citizen".into());
    }
    let refresh = opt_bool(cmd, "refresh")?.unwrap_or(false);
    rt.voices.ask_diary(&sim.world, id, &mut rt.courier, refresh);
    if rt.courier.is_mock() {
        for d in rt.courier.poll() {
            rt.hand_on(&mut sim.world, &d);
        }
    }
    let (voices, courier) = (&rt.voices, &rt.courier);
    let d = voices.diary(id);
    Ok(json!({
        "citizen": key_str(id), "name": sim.world.citizens[id].name,
        "day": d.map(|d| d.day + 1), "text": d.map(|d| d.text.clone()), "pending": d.is_some_and(|d| d.pending),
        "by_model": d.is_some_and(|d| d.by_model), "backend": courier.backend_name(),
    }))
}

/// [voices] `voices {limit?}`: which conversations got a model's line, and
/// the counts.
pub fn voices(cmd: &Value, sim: &Sim, rt: &CommitteeRt) -> Result<Value, String> {
    let limit = opt_u64(cmd, "limit")?.unwrap_or(20) as usize;
    let w = &sim.world;
    let name = |c: nd_sim::CitizenId| w.citizens.get(c).map_or("", |x| x.name.as_str());
    let lines: Vec<Value> = w
        .social
        .recent
        .iter()
        .rev()
        .filter_map(|t| rt.voices.line(t.serial).map(|l| json!({ "serial": t.serial, "a": key_str(t.a), "a_name": name(t.a), "b": key_str(t.b), "b_name": name(t.b), "topic": t.topic.label(), "template": t.line, "line": l })))
        .take(limit)
        .collect();
    Ok(json!({ "backend": rt.courier.backend_name(), "lines_written": rt.voices.lines_written, "diaries_written": rt.voices.diaries_written, "lines": lines }))
}

/// [talk] `talk {citizen, text?, memory?, clear?}`: the mayor says `text`
/// to a citizen and reads the chat back. Offline (the stand-in) the reply
/// is there at once; through a model it comes later (`typing`: call again
/// without `text` to read it). Never takes or returns a key.
pub fn talk(cmd: &Value, sim: &mut Sim, rt: &mut CommitteeRt) -> Result<Value, String> {
    use nd_minds::committee::talk::Sent;
    let id: nd_sim::CitizenId = key_from(field(cmd, "citizen").ok_or("missing citizen id")?)?;
    if !sim.world.citizens.contains_key(id) {
        return Err("no such citizen".into());
    }
    if opt_bool(cmd, "clear")?.unwrap_or(false) {
        sim.world.talks.clear(id);
    }
    if let Some(open) = opt_bool(cmd, "open")? {
        rt.talk_open_request = Some(open);
    }
    let mut sent = None;
    if let Some(text) = opt_str(cmd, "text")? {
        let memory = opt_bool(cmd, "memory")?.unwrap_or(rt.settings.talk_memories);
        sent = Some(rt.talk(&mut sim.world, id, text, memory)?);
    }
    let typing = rt.talker.typing(id);
    let lines = sim.world.talks.lines(id);
    let reply = sent.filter(|_| !typing).and_then(|_| lines.last().filter(|l| !l.mayor)).map(|l| l.text.clone());
    let used = rt.talker.used_today();
    Ok(json!({
        "citizen": key_str(id), "name": sim.world.citizens[id].name,
        "sent": sent.map(|s| match s { Sent::Model => "model", Sent::StandIn => "stand_in", Sent::OverBudget => "over_budget" }),
        "reply": reply, "typing": typing,
        "lines": lines.iter().map(|l| json!({ "day": l.tick / 1440 + 1, "tick": l.tick, "who": if l.mayor { "mayor" } else { "citizen" }, "text": l.text, "stand_in": l.stand_in })).collect::<Vec<_>>(),
        "status": rt.talk_status(), "backend": rt.courier.backend_name(),
        "model_messages_today": used, "daily_budget": rt.settings.talk_daily_budget, "memories": rt.settings.talk_memories,
    }))
}
