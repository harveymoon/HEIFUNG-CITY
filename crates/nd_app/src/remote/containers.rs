//! [K34] Shipping containers over the API: one box's life story, a
//! building's boxes (a depot's stock, a Stacks home's walls, a lorry's
//! load), and the town's counts.

use super::ctx::Ctx;
use super::view::*;
use nd_sim::containers::{BoxId, Where};
use nd_sim::World;
use serde_json::{json, Value};

/// A box by its `"idx:version"` id or its number (`"KOLU 482913 7"`).
pub fn box_id(w: &World, v: &Value) -> Result<BoxId, String> {
    if let Ok(id) = key_from::<BoxId>(v) {
        return w.containers.boxes.contains_key(id).then_some(id).ok_or_else(|| "no such box".into());
    }
    let s = v.as_str().ok_or("box: an id or a box number")?;
    w.containers.by_number(s).ok_or_else(|| format!("no box numbered {s:?}"))
}

fn where_json(w: &World, at: Where) -> Value {
    match at {
        Where::Train => json!({ "kind": "train" }),
        Where::Truck(c) => json!({ "kind": "lorry", "car": key_str(c), "to": w.traffic.cars.get(c).and_then(|x| x.cargo).map(|g| key_str(g.to)) }),
        Where::Stored(b) => json!({ "kind": "stored", "building": key_str(b), "name": w.buildings.get(b).map(|x| x.name.clone()) }),
        Where::Site(b) => json!({ "kind": "home_site", "building": key_str(b), "name": w.buildings.get(b).map(|x| x.name.clone()) }),
        Where::Home(b) => json!({ "kind": "home", "building": key_str(b), "name": w.buildings.get(b).map(|x| x.name.clone()) }),
        Where::Scrap(b) => json!({ "kind": "scrap", "building": key_str(b), "name": w.buildings.get(b).map(|x| x.name.clone()) }),
    }
}

/// One box: number, line, origin, age, condition and marks, owner, where
/// it is, and its history.
pub fn box_json(w: &World, id: BoxId) -> Value {
    let Some(s) = w.box_story(id) else { return Value::Null };
    let r = &w.containers.boxes[id];
    json!({
        "id": key_str(id), "number": s.number, "line": s.line, "size": s.size, "kind": s.kind,
        "origin": s.origin, "age_years": round(s.built_year_ago), "condition": s.condition, "wear": round(s.wear),
        "marks": s.marks, "owner": s.owner, "owner_building": s.owner_id.map(key_str),
        "now": s.now, "where": where_json(w, r.at), "goods": s.goods, "visits": s.visits,
        "history": s.history.iter().map(|(t, what, place, b)| json!({ "time": super::inspect::tick_time_json(*t), "what": what, "where": place, "building": b.map(key_str) })).collect::<Vec<_>>(),
    })
}

/// The boxes a building holds, if any: a depot's stock (with its
/// capacity), the boxes a Stacks home is built of, a site's so far.
pub fn building_boxes(w: &World, b: nd_sim::BuildingId) -> Value {
    let day = w.clock.day() as i64;
    let row = |id: BoxId| w.containers.boxes.get(id).map(|r| json!({ "id": key_str(id), "number": r.number(), "line": nd_sim::containers::LINES[r.line as usize % 10].name, "size_ft": if r.forty { 40 } else { 20 }, "condition": r.condition_label(day), "full": r.goods.is_some() }));
    if let Some(h) = w.containers.homes.get(&b) {
        return json!({ "home": { "built_of": h.boxes.len(), "wants": h.want, "finished": h.done, "boxes": h.boxes.iter().filter_map(|id| row(*id)).collect::<Vec<_>>() } });
    }
    let Some(kind) = w.box_depot(b) else { return Value::Null };
    let ids = w.containers.stored_at(b);
    json!({ "depot": { "kind": kind.label(), "stored": ids.len(), "capacity": w.box_capacity(b, kind), "full": ids.iter().filter(|id| w.containers.boxes[**id].goods.is_some()).count(), "boxes": ids.iter().take(60).filter_map(|id| row(*id)).collect::<Vec<_>>() } })
}

/// The boxes on lorry `car`.
pub fn truck_boxes(w: &World, car: nd_sim::CarId) -> Value {
    let ids = w.boxes_on_truck(car);
    if ids.is_empty() {
        return Value::Null;
    }
    json!(ids.iter().filter_map(|id| w.containers.boxes.get(*id).map(|r| json!({ "id": key_str(*id), "number": r.number(), "full": r.goods.is_some() }))).collect::<Vec<_>>())
}

/// The town's boxes: where they are, the counts since records began, the
/// depots and the homes.
pub fn containers_json(w: &World) -> Value {
    let c = w.box_counts();
    let s = &w.containers.stats;
    let depots: Vec<Value> = w.box_depots().into_iter().map(|(b, k, _)| json!({ "building": key_str(b), "name": w.buildings.get(b).map(|x| x.name.clone()), "kind": k.label(), "stored": w.containers.stored_at(b).len(), "capacity": w.box_capacity(b, k) })).collect();
    let homes: Vec<Value> = w.containers.homes.iter().map(|(b, h)| json!({ "building": key_str(*b), "name": w.buildings.get(*b).map(|x| x.name.clone()), "boxes": h.boxes.len(), "wants": h.want, "finished": h.done })).collect();
    json!({
        "live": w.containers.live,
        "boxes": { "total": c.total, "on_trains": c.on_trains, "on_lorries": c.on_trucks, "stored": c.stored, "stored_full": c.stored_full, "on_home_sites": c.on_sites, "in_homes": c.in_homes, "at_scrap_yards": c.at_scrap },
        "since_records_began": {
            "arrived_by_rail": s.arrived_total[0], "arrived_by_sea": s.arrived_total[1], "arrived_by_road": s.arrived_total[2],
            "delivered": s.delivered_total, "sold_to_stack_developers": s.sold_to_stacks_total, "bought_off_the_lorry": s.sold_off_truck_total,
            "bought_out_of_town_for_homes": s.bought_out_of_town_for_homes, "stacks_homes_built": s.homes_built_total,
            "bought_by_yards": s.bought_by_yards_total, "sent_back_out": s.sent_out_total, "sold_out_of_town": s.sold_out_of_town_total,
            "scrapped": s.scrapped_total, "steel_made": round(s.steel_total),
        },
        "today": { "arrived": s.arrived_today, "delivered": s.delivered_today, "scrapped": s.scrapped_today },
        "awaiting_scrap": s.awaiting_scrap,
        "depots": depots,
        "homes": homes,
    })
}

/// `containers {box?, building?}`: one box, a building's boxes, or the town's.
pub(super) fn containers(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let w = &ctx.sim.world;
    if let Some(v) = field(cmd, "box") {
        return Ok(json!({ "box": box_json(w, box_id(w, v)?) }));
    }
    if let Some(v) = field(cmd, "building") {
        let b = key_from(v)?;
        return Ok(json!({ "building": key_str(b), "containers": building_boxes(w, b) }));
    }
    Ok(containers_json(w))
}
