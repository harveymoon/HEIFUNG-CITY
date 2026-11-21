//! [r12-yards] Yards and fronts for the remote API: `yards` reports what
//! is drawn round the camera, what one home's households put out (and
//! whose it is), and finds homes of a kind to look at (children, the old,
//! a hobby, the washing out, a block).

use nd_sim::yards::{self as sy, PropKind};
use nd_sim::{BuildingId, World};
use serde_json::{json, Value};

use super::view::{field, key_from, key_str, opt_str, opt_u64, opt_vec2, v2};

/// What a home's households put out now: `[{kind, label, owner, owner_name,
/// household, place}]`.
pub fn home_json(w: &World, id: BuildingId) -> Value {
    let laundry = sy::laundry_out(&w.weather, w.clock.hour());
    let ctx = sy::home_ctx(w, id, &sy::beaches(w));
    let rows: Vec<Value> = sy::home_wishes(w, id, laundry, ctx)
        .iter()
        .map(|x| {
            json!({ "kind": x.kind.key(), "label": sy::line(w, x), "owner": key_str(x.owner), "owner_name": w.citizens.get(x.owner).map(|c| c.name.clone()), "household": key_str(x.household), "place": format!("{:?}", x.place).to_lowercase() })
        })
        .collect();
    Value::Array(rows)
}

/// Does a home's wish list have what `find` asks for?
fn matches(find: &str, w: &World, id: BuildingId, wishes: &[sy::Wish]) -> bool {
    let has = |k: PropKind| wishes.iter().any(|x| x.kind == k);
    let b = &w.buildings[id];
    let block = sy::is_block(b.homes, b.floors);
    match find {
        "kids" | "family" => !block && (has(PropKind::KidBike) || has(PropKind::Trike) || has(PropKind::Toys)),
        "elderly" | "old" => !block && has(PropKind::Chair),
        "hobby" => !block && (has(PropKind::ProjectBike) || has(PropKind::Tools) || has(PropKind::Grill) || has(PropKind::Surfboard)),
        "laundry" => !block && has(PropKind::Laundry),
        "block" | "flats" => block && (has(PropKind::Plant) || has(PropKind::Laundry)),
        k => PropKind::ALL.iter().any(|p| p.key() == k && has(*p)),
    }
}

pub(super) fn command(cmd: &Value, sim: &crate::sim::Sim) -> Result<Value, String> {
    let w = &sim.world;
    let mut by_kind = serde_json::Map::new();
    for k in PropKind::ALL {
        let n = sim.yard_props.iter().filter(|p| p.kind == k).count();
        if n > 0 {
            by_kind.insert(k.key().into(), json!(n));
        }
    }
    let n = opt_u64(cmd, "top")?.unwrap_or(10).min(100) as usize;
    let mut homes: Vec<BuildingId> = sim.yard_props.iter().map(|p| p.building).collect();
    homes.dedup();
    let mut v = json!({
        "on": crate::render::yards::on(),
        "laundry_out": sy::laundry_out(&w.weather, w.clock.hour()),
        "wet": sy::wet(&w.weather),
        "homes_with_spots": sim.yard_spots.len(),
        "drawn": { "props": sim.yard_props.len(), "homes": homes.len(), "by_kind": by_kind, "reach_m": crate::render::yards::REACH, "fade_m": [crate::render::yards::FADE.start, crate::render::yards::FADE.end], "far_eye_m": crate::render::yards::FAR_EYE },
    });
    // One home: by id, or the home under a point.
    let home = match field(cmd, "building") {
        Some(b) => Some(key_from::<BuildingId>(b)?),
        None => opt_vec2(cmd, "at")?.and_then(|p| w.lots.values().find(|l| l.contains(p)).and_then(|l| l.building)),
    };
    if let Some(id) = home {
        let b = w.buildings.get(id).ok_or("no such building")?;
        let drawn: Vec<Value> = sim
            .yard_props
            .iter()
            .filter(|p| p.building == id)
            .map(|p| json!({ "kind": p.kind.key(), "owner": key_str(p.owner), "pos": [p.at.x, p.at.y, p.at.z], "yaw": p.yaw }))
            .collect();
        let spots = sim.yard_spots.get(&id).map(|(_, s)| json!({ "step": s.step.len(), "pavement": s.step.iter().filter(|x| x.pavement).count(), "front": s.front.len(), "yard": s.yard.len(), "windows": s.windows.len() }));
        v["home"] = json!({ "building": key_str(id), "name": b.name, "households": b.households.len(), "block": sy::is_block(b.homes, b.floors), "pos": w.lots.get(b.lot).map(|l| v2(l.center)), "out_front": home_json(w, id), "drawn": drawn, "spots": spots });
    }
    // Homes of a kind, nearest `near` (or the camera's look-at) first.
    if let Some(find) = opt_str(cmd, "find")? {
        let near = opt_vec2(cmd, "near")?.unwrap_or_default();
        let laundry = sy::laundry_out(&w.weather, w.clock.hour());
        let beaches = sy::beaches(w);
        let mut found: Vec<(f32, BuildingId)> = Vec::new();
        for (id, b) in w.buildings.iter() {
            if b.homes == 0 || b.households.is_empty() || !sim.yard_spots.contains_key(&id) && field(cmd, "drawn_only").is_some() {
                continue;
            }
            let Some(l) = w.lots.get(b.lot) else { continue };
            let d = l.center.distance(near);
            if found.len() >= n && found.last().is_some_and(|f| f.0 < d) {
                continue;
            }
            let wishes = sy::home_wishes(w, id, laundry, sy::home_ctx(w, id, &beaches));
            if matches(find, w, id, &wishes) {
                found.push((d, id));
                found.sort_by(|a, b| a.0.total_cmp(&b.0));
                found.truncate(n);
            }
        }
        v["found"] = Value::Array(
            found
                .iter()
                .map(|(d, id)| {
                    let b = &w.buildings[*id];
                    json!({ "building": key_str(*id), "name": b.name, "pos": w.lots.get(b.lot).map(|l| v2(l.center)), "front_dir": w.lots.get(b.lot).map(|l| v2(l.front_dir)), "distance": d.round(), "out_front": home_json(w, *id) })
                })
                .collect(),
        );
    }
    Ok(v)
}
