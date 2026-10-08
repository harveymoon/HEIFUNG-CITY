//! [social] People's lives together for the remote API: the `social`
//! command (the day's conversations, friendships and feuds, the city's
//! bonds, every group with its members and works, petitions, murals,
//! gardens, gigs, news and the latest conversations) and the `social`
//! block of `inspect {citizen}` (ties, groups, recent conversations).

use nd_sim::social::{Bond, Group, SocialDay, Talk, Work};
use nd_sim::{CitizenId, World};
use serde_json::{json, Map, Value};

use super::view::{building_name, field, key_str, opt_u64, round, tick_time, v2};

fn day_json(d: &SocialDay) -> Value {
    json!({
        "day": d.day + 1, "talks": d.talks, "good_talks": d.good_talks, "bad_talks": d.bad_talks,
        "friendships": d.friendships, "close_friendships": d.close_friendships, "couples": d.couples, "feuds": d.feuds, "fights": d.fights,
        "meetups": d.meetups, "home_visits": d.home_visits, "favours": d.favours, "loans": d.loans, "jobs_found": d.jobs_found, "homes_found": d.homes_found,
        "moved_in": d.moved_in, "enemy_avoidance": d.avoided, "gossip": d.gossip, "groups_formed": d.groups_formed, "groups_folded": d.groups_folded, "splits": d.splits, "works": d.works,
    })
}

fn name(w: &World, c: CitizenId) -> Value {
    w.citizens.get(c).map_or(Value::Null, |x| Value::String(x.name.clone()))
}

fn work_json(w: &World, x: &Work) -> Value {
    let mut v = json!({ "kind": x.key(), "day": x.day() + 1 });
    match x {
        Work::Gig { venue, .. } => v["venue"] = building_name(w, Some(*venue)),
        Work::Match { park, .. } => v["park"] = building_name(w, Some(*park)),
        Work::Reading { place, .. } => v["place"] = building_name(w, Some(*place)),
        Work::Mural { lot, .. } | Work::Garden { lot, .. } => {
            v["lot"] = json!(key_str(*lot));
            v["at"] = w.lots.get(*lot).map_or(Value::Null, |l| v2(l.center));
        }
        Work::Business { building, .. } => {
            v["building"] = json!(key_str(*building));
            v["name"] = building_name(w, Some(*building));
        }
        Work::Petition { id, .. } | Work::Protest { petition: id, .. } => v["petition"] = json!(id),
    }
    v
}

fn group_json(w: &World, g: &Group, detail: bool) -> Value {
    let mut v = json!({
        "id": g.id, "name": g.name, "kind": g.kind.key(), "members": g.members.len(), "founder": name(w, g.founder), "founded_day": g.founded + 1,
        "momentum": round(g.momentum), "threshold": g.kind.threshold(), "reputation": round(g.reputation), "meetings": g.meetings,
        "base": building_name(w, g.base), "base_id": g.base.map(key_str), "works": g.works.iter().map(|x| work_json(w, x)).collect::<Vec<_>>(),
        "split_from": g.split_from, "folded_day": g.folded.map(|d| d + 1),
    });
    if detail {
        v["member_list"] = Value::Array(g.members.iter().map(|m| json!({ "id": key_str(*m), "name": name(w, *m), "founder": *m == g.founder })).collect());
    }
    v
}

fn talk_json(w: &World, t: &Talk) -> Value {
    json!({
        "when": tick_time(t.tick), "a": name(w, t.a), "b": name(w, t.b), "a_id": key_str(t.a), "b_id": key_str(t.b),
        "topic": t.topic.key(), "topic_label": t.topic.label(), "outcome": t.outcome.key(), "where": format!("{:?}", t.setting).to_lowercase(),
        "place": building_name(w, t.place), "at": v2(t.pos), "line": t.line,
    })
}

/// `{"cmd": "social"}`: the city's social life.
pub fn command(cmd: &Value, w: &World) -> Result<Value, String> {
    let s = &w.social;
    if let Some(gv) = field(cmd, "group") {
        let id = gv.as_u64().ok_or("group is a group id (number)")? as u32;
        let g = s.group(id).ok_or("no such group")?;
        return Ok(json!({ "group": group_json(w, g, true) }));
    }
    let days = opt_u64(cmd, "days")?.unwrap_or(7) as usize;
    let talks = opt_u64(cmd, "talks")?.unwrap_or(10) as usize;
    let bonds = s.bond_counts();
    let mut pairs = Map::new();
    for b in [Bond::Acquaintance, Bond::Friend, Bond::CloseFriend, Bond::Partner, Bond::Rival, Bond::Enemy] {
        pairs.insert(b.label().replace(' ', "_"), json!(bonds.get(&b).copied().unwrap_or(0) / 2));
    }
    let people = s.circles.values().filter(|c| !c.ties.is_empty()).count();
    let with_friend = s.circles.values().filter(|c| c.friends().next().is_some()).count();
    Ok(json!({
        "today": day_json(&s.today),
        "yesterday": s.yesterday().map(day_json),
        "history": s.history.iter().rev().take(days).rev().map(day_json).collect::<Vec<_>>(),
        "pairs": pairs,
        "people_with_ties": people, "people_with_a_friend": with_friend, "population": w.citizens.len(),
        "totals": { "talks": s.talks_total, "friendships": s.friendships_total, "feuds": s.feuds_total, "fights": s.fights_total, "couples": s.couples_total },
        "groups": s.active_groups().map(|g| group_json(w, g, false)).collect::<Vec<_>>(),
        "folded_groups": s.groups.iter().filter(|g| !g.active()).count(),
        "petitions": s.petitions.iter().rev().take(10).map(|p| json!({
            "id": p.id, "group": s.group(p.group).map(|g| g.name.clone()), "day": p.day + 1, "ask": p.ask.key(), "concern": p.ask.concern().key(),
            "text": p.text, "at": v2(p.at), "signatures": p.signatures, "state": p.state.label(), "rally": building_name(w, p.rally),
        })).collect::<Vec<_>>(),
        "murals": s.murals.iter().map(|m| json!({ "group": m.group, "building": building_name(w, m.building), "at": v2(m.pos), "quality": round(m.quality), "day": m.day + 1 })).collect::<Vec<_>>(),
        "gardens": s.gardens.iter().map(|g| json!({ "group": g.group, "lot": key_str(g.lot), "at": v2(g.pos), "day": g.day + 1 })).collect::<Vec<_>>(),
        "gigs": s.gigs.iter().map(|g| json!({ "group": s.group(g.group).map(|x| x.name.clone()), "venue": building_name(w, Some(g.venue)), "from": tick_time(g.from), "until": tick_time(g.until), "on_now": g.on_at(w.clock.tick), "crowd": g.crowd })).collect::<Vec<_>>(),
        "outings_planned": s.outings.len(), "walking_together": s.walking_with.len() / 2,
        "news": s.news.iter().rev().take(15).map(|n| json!({ "when": tick_time(n.tick), "text": n.text, "at": v2(n.pos), "group": n.group })).collect::<Vec<_>>(),
        "recent_talks": s.recent.iter().rev().take(talks).map(|t| talk_json(w, t)).collect::<Vec<_>>(),
    }))
}

/// `inspect {citizen}` → `social`.
pub fn citizen_social(w: &World, id: CitizenId) -> Value {
    let Some(c) = w.circle_of(id) else { return json!({ "ties": [], "groups": [], "talks": [] }) };
    let mut ties: Vec<_> = c.ties.iter().collect();
    ties.sort_by(|a, b| a.bond().rank().cmp(&b.bond().rank()).then(b.affinity.abs().total_cmp(&a.affinity.abs())));
    json!({
        "ties": ties.iter().map(|t| json!({
            "id": key_str(t.who), "name": name(w, t.who), "bond": t.bond().label(), "affinity": round(t.affinity), "familiarity": round(t.familiarity),
            "trust": round(t.trust), "talks": t.talks, "met": t.met.label(), "met_at": building_name(w, t.met_at), "since": tick_time(t.since), "last": tick_time(t.last),
            "partner": t.partner, "lent": round(t.lent),
        })).collect::<Vec<_>>(),
        "groups": c.groups.iter().filter_map(|g| w.social.group(*g)).map(|g| json!({ "id": g.id, "name": g.name, "kind": g.kind.key(), "founder": g.founder == id })).collect::<Vec<_>>(),
        "talks": c.talks.iter().rev().map(|n| json!({
            "when": tick_time(n.tick), "with": name(w, n.with), "with_id": key_str(n.with), "topic": n.topic.label(), "outcome": n.outcome.key(),
            "place": building_name(w, n.place), "where": format!("{:?}", n.setting).to_lowercase(),
        })).collect::<Vec<_>>(),
        "walking_with": w.walking_with(id).map(|f| name(w, f)),
    })
}
