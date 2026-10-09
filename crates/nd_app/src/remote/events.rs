//! `events {since}`: numbered `SimEvent`s and status-line changes, and the
//! history rings' sampling.

use std::collections::VecDeque;

use bevy::prelude::*;
use nd_sim::SimEvent;
use serde_json::{json, Value};

use super::ctx::Ctx;
use super::history::History;
use super::view::*;
use crate::sim::Sim;
use crate::tools::ToolState;
use crate::ui::UiFlags;

/// Events kept for `events {since}`; at 256× a busy city makes hundreds a
/// second, so a poll every few seconds still sees everything.
pub(super) const EVENT_RING: usize = 2000;

/// Numbered events for `events {since}`: `SimEvent`s plus status-line
/// changes, each with the tick it happened at.
#[derive(Resource, Default)]
pub(super) struct EventLog {
    pub(super) seq: u64,
    ring: VecDeque<Value>,
    last_message: String,
    last_status: String,
}

impl EventLog {
    fn push(&mut self, tick: u64, mut v: Value) {
        self.seq += 1;
        v["seq"] = json!(self.seq);
        v["tick"] = json!(tick);
        self.ring.push_back(v);
        if self.ring.len() > EVENT_RING {
            self.ring.pop_front();
        }
    }
}

/// Record this frame's `SimEvent`s and status-line changes. Runs in `Sync`,
/// where `Sim::events` holds the frame's events for every reader.
pub(super) fn log_events(sim: Res<Sim>, flags: Res<UiFlags>, tools: Res<ToolState>, mut log: ResMut<EventLog>) {
    let w = &sim.world;
    let tick = w.clock.tick;
    for e in &sim.events {
        let v = match e {
            SimEvent::RoadsChanged => json!({ "kind": "RoadsChanged" }),
            SimEvent::LotsChanged => json!({ "kind": "LotsChanged" }),
            SimEvent::PlanChanged => json!({ "kind": "PlanChanged" }),
            SimEvent::NewDay(d) => json!({ "kind": "NewDay", "day": d + 1 }),
            SimEvent::BuildingBuilt(b) => json!({ "kind": "BuildingBuilt", "building": key_str(*b), "name": building_name(w, Some(*b)) }),
            SimEvent::BuildingChanged(b) => json!({ "kind": "BuildingChanged", "building": key_str(*b), "name": building_name(w, Some(*b)) }),
            SimEvent::BuildingRemoved(b, l) => json!({ "kind": "BuildingRemoved", "building": key_str(*b), "lot": key_str(*l) }),
            SimEvent::Expropriated(b, l) => json!({ "kind": "Expropriated", "building": key_str(*b), "lot": key_str(*l) }),
            SimEvent::BusinessClosed(b) => json!({ "kind": "BusinessClosed", "building": key_str(*b), "name": building_name(w, Some(*b)) }),
            SimEvent::CitizenArrived(c) => json!({ "kind": "CitizenArrived", "citizen": key_str(*c), "name": w.citizens.get(*c).map(|x| x.name.as_str()) }),
            SimEvent::CitizenLeft(c) => json!({ "kind": "CitizenLeft", "citizen": key_str(*c) }),
            SimEvent::IncidentStarted(i) | SimEvent::EmergencyCall(i) | SimEvent::UnitDispatched(i) | SimEvent::UnitOnScene(i) | SimEvent::IncidentResolved(i) => {
                let kind = match e {
                    SimEvent::IncidentStarted(_) => "IncidentStarted",
                    SimEvent::EmergencyCall(_) => "EmergencyCall",
                    SimEvent::UnitDispatched(_) => "UnitDispatched",
                    SimEvent::UnitOnScene(_) => "UnitOnScene",
                    _ => "IncidentResolved",
                };
                match w.services.incident(*i) {
                    Some(inc) => json!({ "kind": kind, "incident": i, "type": inc.kind.label(), "building": key_str(inc.building), "name": building_name(w, Some(inc.building)), "outcome": inc.outcome.map(|o| o.label()) }),
                    None => json!({ "kind": kind, "incident": i }),
                }
            }
            SimEvent::FireSpread(b) => json!({ "kind": "FireSpread", "building": key_str(*b), "name": building_name(w, Some(*b)) }),
            SimEvent::BurnedDown(b, l) => json!({ "kind": "BurnedDown", "building": key_str(*b), "lot": key_str(*l) }),
            SimEvent::Arrested(i, c) => json!({ "kind": "Arrested", "incident": i, "citizen": key_str(*c), "name": w.citizens.get(*c).map(|x| x.name.as_str()) }),
            SimEvent::Released(c) => json!({ "kind": "Released", "citizen": key_str(*c), "name": w.citizens.get(*c).map(|x| x.name.as_str()) }),
            SimEvent::Crash(id) => {
                let r = w.driving.recent.iter().find(|r| r.id == *id);
                json!({ "kind": "Crash", "crash": id, "pos": r.map(|r| [r.pos.x, r.pos.y]), "cause": r.map(|r| r.cause.label()), "severity": r.map(|r| r.severity), "injured": r.map(|r| r.injured.len()), "place": r.map(|r| r.place.clone()), "incident": r.and_then(|r| r.incident) })
            }
            SimEvent::Takeover(id) => json!({ "kind": "Takeover", "takeover": id, "place": w.takeover().map(|t| t.place.clone()) }),
            SimEvent::TakeoverOver(id) => json!({ "kind": "TakeoverOver", "takeover": id }),
            SimEvent::News(seq) => {
                let n = w.street_events.news_item(*seq);
                json!({ "kind": "News", "seq": seq, "event": n.map(|n| n.kind.label()), "headline": n.map(|n| n.headline.as_str()), "pos": n.map(|n| [n.pos.x, n.pos.y]) })
            }
            SimEvent::PathTrimmed { gesture, removed, grounds } => json!({ "kind": "PathTrimmed", "gesture_id": gesture.0, "removed": removed, "grounds": grounds.key() }),
            SimEvent::StadiumEvent(b) => json!({ "kind": "StadiumEvent", "building": key_str(*b), "name": building_name(w, Some(*b)) }),
            SimEvent::NewThought(c) => {
                let cit = w.citizens.get(*c);
                json!({ "kind": "NewThought", "citizen": key_str(*c), "name": cit.map(|x| x.name.as_str()), "thought": cit.map(|x| x.thought.as_str()) })
            }
        };
        log.push(tick, v);
    }
    if flags.message != log.last_message {
        log.last_message = flags.message.clone();
        let text = log.last_message.clone();
        log.push(tick, json!({ "kind": "Message", "text": text }));
    }
    if tools.status != log.last_status {
        log.last_status = tools.status.clone();
        let text = log.last_status.clone();
        log.push(tick, json!({ "kind": "ToolStatus", "text": text }));
    }
}

/// Sample the city into the history rings when an hour or a day turns
/// (cheap otherwise: a few integer compares and four counters copied).
pub(super) fn record_history(sim: Res<Sim>, mut history: ResMut<History>) {
    history.follow_city(sim.generation, &sim.world, "city replaced");
    history.observe(&sim.world);
}

/// `events {since?, limit?}`: the events after `since`, oldest first.
pub(super) fn events(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let since = opt_u64(cmd, "since")?.unwrap_or(0);
    let limit = opt_u64(cmd, "limit")?.unwrap_or(500) as usize;
    let log = &ctx.log;
    let events: Vec<Value> = log.ring.iter().filter(|e| e["seq"].as_u64().unwrap_or(0) > since).take(limit).cloned().collect();
    // [fix/review-app] From 0 too: events 1.. are gone once the ring
    // has turned over.
    let dropped = log.ring.front().and_then(|e| e["seq"].as_u64()).is_some_and(|first| since + 1 < first);
    Ok(json!({ "seq": log.seq, "events": events, "dropped": dropped, "message": ctx.flags.message, "tool_status": ctx.tools.status }))
}

/// `thoughts {limit?}`: the newest memories and the thoughts people have now.
pub(super) fn thoughts(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let limit = opt_u64(cmd, "limit")?.unwrap_or(50) as usize;
    let w = &ctx.sim.world;
    let mut rows: Vec<(u64, Value)> = w.citizens.iter().flat_map(|(id, c)| c.memories.iter().map(move |m| (m.tick, json!({ "citizen": key_str(id), "name": c.name, "tick": m.tick, "text": m.text })))).collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    let memories: Vec<Value> = rows.into_iter().take(limit).map(|(_, v)| v).collect();
    let thoughts: Vec<Value> = w.citizens.iter().filter(|(_, c)| !c.thought.is_empty()).take(limit).map(|(id, c)| json!({ "citizen": key_str(id), "name": c.name, "thought": c.thought })).collect();
    Ok(json!({ "memories": memories, "thoughts": thoughts }))
}
