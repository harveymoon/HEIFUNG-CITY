//! [megacorp] Mega-corps and universities over the remote API: `corps`
//! (summary, rules, dev fast-forwards) and `campus` (summary, dev growth),
//! plus the brand of a building for `inspect` (`building_brand`). Logos are
//! data: shape, monogram, colour and the pixel rows the renderer lights.

use super::view::{field, key_from, key_str, round, v2};
use nd_sim::corp::{Brand, Campus, MegaCorp};
use nd_sim::estate::OwnerId;
use nd_sim::{BuildingId, World};
use serde_json::{json, Value};

/// A brand as data.
pub fn brand_json(b: &Brand) -> Value {
    json!({ "name": b.name, "colour": b.hex(), "colour_index": b.colour, "shape": b.shape.key(), "monogram": b.monogram.to_string(), "logo": b.logo_rows() })
}

fn corp_json(w: &World, c: &MegaCorp) -> Value {
    let o = w.estate.owners.get(c.owner);
    let props: Vec<BuildingId> = o.map(|o| o.properties.clone()).unwrap_or_default();
    let mut by_use = serde_json::Map::new();
    let mut kinds = serde_json::Map::new();
    for p in &props {
        let Some(b) = w.buildings.get(*p) else { continue };
        *by_use.entry(b.land_use.label().to_string()).or_insert(json!(0)) = json!(by_use.get(b.land_use.label()).and_then(|v| v.as_u64()).unwrap_or(0) + 1);
        if let Some(z) = &b.business {
            let k = z.kind.label().to_string();
            let n = kinds.get(&k).and_then(|v| v.as_u64()).unwrap_or(0) + 1;
            kinds.insert(k, json!(n));
        }
    }
    let (homes, blocks) = w.corp_homes(c.owner);
    let staff = w.corp_staff(c.owner);
    let housed = staff.iter().filter(|s| w.citizens.get(**s).and_then(|x| x.home).and_then(|h| w.buildings.get(h)).is_some_and(|h| h.plot.owner == Some(c.owner))).count();
    json!({
        "id": key_str(c.owner),
        "brand": brand_json(&c.brand),
        "origin": c.origin_name, "origin_kind": c.origin.label(),
        "founded_day": c.founded_day + 1,
        "hq": c.hq.map(|h| json!({
            "id": key_str(h), "name": w.buildings.get(h).map(|b| b.name.clone()),
            "floors": w.buildings.get(h).map(|b| b.floors), "building": w.buildings.get(h).is_some_and(|b| b.construction.is_some()),
            "pos": w.building_lot(h).map_or(Value::Null, |l| v2(l.center)),
        })),
        "cash": c.cash.round(),
        "holdings": props.len(),
        "holdings_by_use": by_use,
        "industries": kinds,
        "staff": staff.len(),
        "housing": { "blocks": blocks.len(), "homes": homes, "staff_housed": housed },
        "revenue_yesterday": round(c.revenue_yesterday),
        "market_share": round(w.corp_market_share(c.owner)),
        "bought_total": c.bought_total,
        "levy_total": c.levy_total.round(),
        "properties": props.iter().filter_map(|p| w.buildings.get(*p).map(|b| json!({ "id": key_str(*p), "name": b.name, "land_use": b.land_use.label(), "business": b.business.as_ref().map(|z| z.kind.label()), "homes": b.homes }))).collect::<Vec<_>>(),
    })
}

fn campus_json(w: &World, k: &Campus) -> Value {
    let b = w.buildings.get(k.building);
    let ki = w.corps.campuses.iter().position(|x| x.building == k.building).unwrap_or(0);
    json!({
        "id": key_str(k.building),
        "owner": key_str(k.owner),
        "brand": brand_json(&k.brand),
        "name": b.map(|b| b.name.clone()),
        "tier": b.and_then(|b| b.service).map(|s| s.tier.label()),
        "pos": w.building_lot(k.building).map_or(Value::Null, |l| v2(l.center)),
        "places": w.campus_places(ki),
        "students": k.students.len(),
        "dorm_homes": w.campus_dorm_homes(ki),
        "funds": k.funds.round(),
        "annexes": k.annexes.iter().map(|(a, kind)| json!({ "id": key_str(*a), "kind": kind.key(), "name": w.buildings.get(*a).map(|b| b.name.clone()), "building": w.buildings.get(*a).is_some_and(|b| b.construction.is_some()) })).collect::<Vec<_>>(),
        "pending": k.pending.iter().map(|(l, kind)| json!({ "lot": key_str(*l), "kind": kind.key() })).collect::<Vec<_>>(),
        "expansions_total": k.expansions_total,
    })
}

/// `inspect.building.brand`: the corp or university that owns it, its
/// logo, and what the building is to it.
pub fn building_brand(w: &World, id: BuildingId) -> Value {
    if let Some(c) = w.corp_of(id) {
        let role = if c.hq == Some(id) { "headquarters" } else if w.buildings.get(id).is_some_and(|b| b.business.is_none() && b.homes > 0) { "staff housing" } else { "holding" };
        return json!({ "kind": "mega_corp", "corp": key_str(c.owner), "role": role, "brand": brand_json(&c.brand), "rent_discount": if role == "staff housing" { nd_sim::corp::STAFF_RENT_DISCOUNT } else { 0.0 } });
    }
    if let Some(k) = w.campus_of(id) {
        let role = if k.building == id { "main building".to_string() } else { k.annex_of(id).map_or("campus".to_string(), |a| a.label().to_string()) };
        return json!({ "kind": "university", "campus": key_str(k.building), "role": role, "brand": brand_json(&k.brand) });
    }
    Value::Null
}

fn corp_owner(cmd: &Value, w: &World) -> Result<OwnerId, String> {
    if let Some(v) = field(cmd, "corp") {
        return key_from(v);
    }
    if let Some(v) = field(cmd, "building") {
        let b: BuildingId = key_from(v)?;
        return w.corp_of(b).map(|c| c.owner).ok_or_else(|| "that building is no corp's".to_string());
    }
    w.corps.corps.first().map(|c| c.owner).ok_or_else(|| "no mega-corps yet".to_string())
}

/// `corps {action}`.
pub fn command(cmd: &Value, w: &mut World) -> Result<Value, String> {
    let action = cmd.get("action").and_then(|v| v.as_str()).unwrap_or("summary");
    match action {
        "summary" | "list" => {
            let candidates: Vec<Value> = w.companies().into_iter().take(8).map(|c| json!({ "flagship": key_str(c.flagship), "name": c.name, "kind": c.kind.label(), "holdings": c.holdings, "staff": c.staff, "revenue": round(c.revenue), "qualifies": c.qualifies() })).collect();
            Ok(json!({
                "corps": w.corps.corps.iter().map(|c| corp_json(w, c)).collect::<Vec<_>>(),
                "campuses": w.corps.campuses.iter().map(|k| campus_json(w, k)).collect::<Vec<_>>(),
                "rules": { "allowed": w.corps.rules.allowed, "max_share": w.corps.rules.max_share, "levy": w.corps.rules.levy, "cap_per_corp": w.corp_cap(), "room": w.corp_room() },
                "thresholds": { "revenue": nd_sim::corp::PROMOTE_REVENUE, "staff": nd_sim::corp::PROMOTE_STAFF, "holdings": nd_sim::corp::PROMOTE_HOLDINGS, "min_pop": nd_sim::corp::CORP_MIN_POP },
                "biggest_companies": candidates,
                "stats": serde_json::to_value(&w.corps.stats).unwrap_or(Value::Null),
            }))
        }
        "rules" => {
            let allowed = cmd.get("allowed").and_then(|v| v.as_bool());
            let share = cmd.get("max_share").and_then(|v| v.as_f64()).map(|v| v as f32);
            let levy = cmd.get("levy").and_then(|v| v.as_f64()).map(|v| v as f32);
            w.set_corp_rules(allowed, share, levy);
            Ok(json!({ "allowed": w.corps.rules.allowed, "max_share": w.corps.rules.max_share, "levy": w.corps.rules.levy, "cap_per_corp": w.corp_cap() }))
        }
        // Dev: the company owning `building` becomes a mega-corp now.
        "promote" => {
            let b: BuildingId = key_from(field(cmd, "building").ok_or("promote needs a building (a business)")?)?;
            let ci = w.promote_to_corp(b)?;
            let c = w.corps.corps[ci].clone();
            Ok(json!({ "corp": corp_json(w, &c) }))
        }
        // Dev: buy the next thing now (`n` times).
        "buy" => {
            let owner = corp_owner(cmd, w)?;
            let n = cmd.get("n").and_then(|v| v.as_u64()).unwrap_or(1).clamp(1, 50);
            let mut got = Vec::new();
            for _ in 0..n {
                match w.corp_buy_now(owner)? {
                    Some(b) => got.push(json!({ "id": key_str(b), "name": w.buildings.get(b).map(|x| x.name.clone()) })),
                    None => break,
                }
            }
            Ok(json!({ "bought": got }))
        }
        // Dev: HQ now if none, building sites finished.
        "finish" => {
            let owner = corp_owner(cmd, w)?;
            let n = w.corp_finish_now(owner)?;
            Ok(json!({ "finished": n }))
        }
        "drive" => {
            let owner = corp_owner(cmd, w)?;
            let lot = key_from(field(cmd, "lot").ok_or("drive needs a lot")?)?;
            Ok(json!({ "drive_m": w.corp_drive_m(owner, lot) }))
        }
        other => Err(format!("unknown corps action {other:?}: summary | rules {{allowed, max_share, levy}} | promote {{building}} | buy {{corp|building, n}} | finish {{corp}} | drive {{corp, lot}}")),
    }
}

/// `campus {action}`.
pub fn campus(cmd: &Value, w: &mut World) -> Result<Value, String> {
    let action = cmd.get("action").and_then(|v| v.as_str()).unwrap_or("summary");
    match action {
        "summary" | "list" => Ok(json!({ "campuses": w.corps.campuses.iter().map(|k| campus_json(w, k)).collect::<Vec<_>>() })),
        "grow" => {
            let b: BuildingId = match field(cmd, "building") {
                Some(v) => key_from(v)?,
                None => w.corps.campuses.first().map(|k| k.building).ok_or("no university yet")?,
            };
            let days = cmd.get("days").and_then(|v| v.as_u64()).unwrap_or(1) as u32;
            let n = w.campus_grow_now(b, days)?;
            let k = w.corps.campuses.iter().find(|k| k.building == b).cloned().unwrap();
            Ok(json!({ "annexes": n, "campus": campus_json(w, &k) }))
        }
        other => Err(format!("unknown campus action {other:?}: summary | grow {{building, days}}")),
    }
}
