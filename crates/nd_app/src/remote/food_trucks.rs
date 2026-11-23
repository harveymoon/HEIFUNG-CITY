//! [r14-foodtrucks] The `food_trucks` command: every food truck, van and
//! cart (`nd_sim::food_trucks`) with its type, menu, owner household, where
//! it is, today's plan and the spots it traded at, meals and takings; the
//! town's crowds the trucks plan from; the day's numbers. Dev levers:
//! `found` (start one now), `plan` (plan the rest of the day again),
//! `enabled` (switch the trucks off to compare a town without them).

use nd_sim::food_trucks::{SpotKind, TruckId, TruckJob, TruckKind, TruckPlace};
use nd_sim::World;
use serde_json::{json, Value};

use super::view::{field, key_from, key_str, round, v2};

fn hhmm(tick: u64) -> String {
    let m = tick % nd_sim::clock::MINUTES_PER_DAY;
    format!("{:02}:{:02}", m / 60, m % 60)
}

/// One truck in full.
pub fn truck_json(w: &World, id: TruckId) -> Value {
    let Some(t) = w.food_trucks.trucks.get(id) else { return Value::Null };
    let row = t.kind.row();
    let hh = w.households.get(t.household);
    let crew = t.crew.and_then(|c| w.citizens.get(c).map(|x| json!({ "id": key_str(c), "name": x.name })));
    let (place, pos) = match t.place {
        TruckPlace::Base => ("home", w.building_lot(t.base).map(|l| l.center).unwrap_or(t.last_pos)),
        TruckPlace::Pitch { pitch, open } => (if open { "open" } else { "parked" }, pitch.pos),
        TruckPlace::Road(_) => ("road", t.last_pos),
    };
    let pitch = t.place.pitch().map(|p| json!({ "segment": p.segment.0, "side": p.side, "slot": p.slot, "legal_kerb": p.legal, "heading_deg": round(p.heading.to_degrees()) }));
    let car = match t.place {
        TruckPlace::Road(c) => Some(key_str(c)),
        _ => None,
    };
    let plan: Vec<Value> = t.plan.iter().map(|s| json!({ "day": s.day, "from": s.from, "to": s.to, "spot": s.name, "kind": s.kind.label(), "at": v2(s.at), "crowd": round(s.crowd), "state": s.state.label() })).collect();
    let visit = |v: &nd_sim::food_trucks::Visit| json!({ "spot": v.name, "kind": v.kind.label(), "at": v2(v.at), "opened": hhmm(v.opened), "closed": v.closed.map(hhmm), "meals": v.meals, "takings": round(v.takings) });
    json!({
        "id": key_str(id), "name": t.name, "type": t.kind.key(), "label": row.label, "menu": row.menu, "price": row.price,
        "household": key_str(t.household), "household_money": hh.map(|h| round(h.money)), "base": key_str(t.base),
        "base_name": w.buildings.get(t.base).map(|b| b.name.clone()),
        "crew": crew, "place": place, "doing": match t.job { TruckJob::Idle => t.place.label(), j => j.label() }, "pos": v2(pos), "pitch": pitch, "car": car,
        "queue": w.food_trucks.queue_len(id), "stock": round(t.stock),
        "meals_today": t.meals_today, "meals_yesterday": t.meals_yesterday, "meals_total": t.meals_total,
        "takings_today": round(t.takings_today), "takings_yesterday": round(t.takings_yesterday), "takings_total": t.takings_total.round(),
        "costs_today": round(t.costs_today), "costs_yesterday": round(t.costs_yesterday),
        "bought_local_today": round(t.bought_local_today), "bought_wholesale_today": round(t.bought_wholesale_today),
        "fed_hungry_today": t.fed_hungry_today, "losing_days": t.losing_days,
        "plan": plan,
        "visits_today": t.visits.iter().map(visit).collect::<Vec<_>>(),
        "visits_yesterday": t.visits_yesterday.iter().map(visit).collect::<Vec<_>>(),
        "model": { "body": t.model.body.label(), "paint": t.model.paint, "cart": row.cart },
    })
}

/// The town's numbers.
pub fn report(w: &World) -> Value {
    let f = &w.food_trucks;
    let s = &f.stats;
    let mut by_type = serde_json::Map::new();
    for k in TruckKind::ALL {
        by_type.insert(k.key().into(), json!(f.trucks.values().filter(|t| t.kind == k).count()));
    }
    let spots_by = |v: &Vec<u32>| -> Value {
        let mut m = serde_json::Map::new();
        for k in SpotKind::ALL {
            m.insert(k.label().into(), json!(v.get(k.index()).copied().unwrap_or(0)));
        }
        Value::Object(m)
    };
    json!({
        "trucks": f.trucks.len(), "by_type": by_type, "enabled": !f.disabled,
        "open_now": f.trucks.values().filter(|t| t.place.open()).count(),
        "on_road": f.trucks.values().filter(|t| matches!(t.place, TruckPlace::Road(_))).count(),
        "at_home": f.trucks.values().filter(|t| t.place == TruckPlace::Base).count(),
        "customers_queuing": f.trips.len(),
        "meals_today": s.meals_today, "meals_yesterday": s.meals_yesterday, "meals_total": s.meals_total,
        "takings_today": round(s.takings_today), "takings_yesterday": round(s.takings_yesterday),
        "fed_hungry_today": s.fed_hungry_today, "fed_hungry_yesterday": s.fed_hungry_yesterday,
        "open_by_hour": s.open_by_hour, "open_by_hour_yesterday": s.open_by_hour_yesterday,
        "meals_by_spot": spots_by(&s.meals_by_spot), "meals_by_spot_yesterday": spots_by(&s.meals_by_spot_yesterday),
        "ice_cream_too_cold_today": s.too_cold_today, "ice_cream_too_cold_yesterday": s.too_cold_yesterday,
        "founded_total": s.founded_total, "sold_total": s.sold_total,
        "hungry": w.food_need.hungry, "hungry_share": round(w.food_need.share),
    })
}

pub fn command(cmd: &Value, w: &mut World) -> Result<Value, String> {
    let mut note: Option<Value> = None;
    if let Some(on) = field(cmd, "enabled").and_then(|v| v.as_bool()) {
        w.food_trucks.disabled = !on;
    }
    if let Some(f) = field(cmd, "found") {
        let kind = match f.as_str() {
            Some(k) => TruckKind::ALL.into_iter().find(|x| x.key() == k).ok_or(format!("no truck type '{k}': ice_cream|taco|noodle|kebab|coffee|hot_dog|fruit"))?,
            None => {
                let n = f.as_u64().unwrap_or(1).clamp(1, 30) as usize;
                let mut made = Vec::new();
                for i in 0..n {
                    let k = TruckKind::from_index(i + w.food_trucks.trucks.len());
                    made.push(key_str(w.found_food_truck(k, None)?));
                }
                note = Some(json!({ "founded": made }));
                TruckKind::IceCream
            }
        };
        if note.is_none() {
            let hh = field(cmd, "household").map(key_from).transpose()?;
            let id = w.found_food_truck(kind, hh)?;
            note = Some(json!({ "founded": [key_str(id)] }));
        }
    }
    if field(cmd, "plan").and_then(|v| v.as_bool()) == Some(true) {
        let h = (w.clock.hour() as u8 + 1).max(nd_sim::food_trucks::PLAN_HOUR + 1);
        w.plan_food_trucks(h);
    }
    let mut ids: Vec<TruckId> = w.food_trucks.trucks.keys().collect();
    ids.sort_by_key(|id| w.food_trucks.trucks[*id].number);
    let list: Vec<Value> = ids.into_iter().map(|id| truck_json(w, id)).collect();
    let spots: Vec<Value> = if field(cmd, "spots").and_then(|v| v.as_bool()) == Some(false) {
        Vec::new()
    } else {
        w.food_trucks.spots.iter().map(|s| json!({ "kind": s.kind.label(), "name": s.name, "at": v2(s.at), "crowd": round(s.crowd), "from": s.from, "to": s.to })).collect()
    };
    Ok(json!({ "summary": report(w), "list": list, "crowd_spots": spots, "done": note }))
}
