//! [r5-layered] The `under_decks` command: the bays of ground under the
//! raised roads (`nd_sim::undercroft`), what each is used for, who runs it
//! and how busy it is; `action: set` gives a bay a use (a dev lever for
//! trying the looks out).

use nd_sim::undercroft::UnderUse;
use nd_sim::World;
use serde_json::{json, Value};

use super::view::{field, round, v2};

pub fn command(cmd: &Value, w: &mut World) -> Result<Value, String> {
    if field(cmd, "action").and_then(|v| v.as_str()) == Some("set") {
        let id = field(cmd, "bay").and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).ok_or("set needs bay (id from the list)")?;
        let want = field(cmd, "use").and_then(|v| v.as_str()).ok_or("set needs use: bare|market|hawkers|workshop|court")?;
        let u = UnderUse::ALL.into_iter().find(|u| u.key() == want && *u != UnderUse::Shelter).ok_or(format!("no use '{want}'"))?;
        w.set_bay_use(id, u)?;
    }
    let near = field(cmd, "near").and_then(|v| v.as_array()).and_then(|a| Some(nd_sim::Vec2::new(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32)));
    let r = field(cmd, "r").and_then(|v| v.as_f64()).unwrap_or(300.0) as f32;
    let u = &w.undercroft;
    let counts = u.counts();
    let mut by_use = serde_json::Map::new();
    for x in UnderUse::ALL {
        by_use.insert(x.key().into(), json!(counts[x as usize]));
    }
    let hour = w.clock.hour() as u8;
    let bays: Vec<Value> = u
        .bays
        .iter()
        .filter(|b| near.is_none_or(|p| b.pos.distance(p) <= r))
        .map(|b| {
            let (here, coming) = w.bay_people(b.id);
            json!({
                "id": b.id, "use": b.use_.key(), "label": b.use_.label(), "pos": v2(b.pos), "dir": v2(b.dir), "len_m": round(b.len), "width_m": round(b.width),
                "headroom_m": round(b.clearance), "under": b.under, "walkway": b.walkway, "park": b.park, "open": b.open_at(hour),
                "since_day": b.since_day, "holders": b.holders.len(), "people_here": here, "on_the_way": coming,
                "visits_today": b.visits_today, "visits_yesterday": b.visits_yesterday, "takings_today": round(b.takings_today), "takings_yesterday": round(b.takings_yesterday), "visits_total": b.visits_total,
            })
        })
        .collect();
    Ok(json!({
        "bays": u.bays.len(), "by_use": by_use, "visits_total": u.visits_total, "takings_total": u.takings_total.round(),
        "visits_yesterday": u.bays.iter().map(|b| b.visits_yesterday).sum::<u32>(), "takings_yesterday": round(u.bays.iter().map(|b| b.takings_yesterday).sum::<f32>()),
        "camps_under_decks": w.society.homeless.camps.iter().filter(|c| c.under).count(),
        "list": bays,
    }))
}
