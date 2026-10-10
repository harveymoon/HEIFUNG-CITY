//! [pt-walk] `connectivity`: the stranded areas (parts of the city cut off
//! from the main pedestrian grid or road network), the trips that found no
//! route, the alerts raised for them, and (with `segments: true`) every
//! street and path with whether it is on the main networks: the hook for a
//! connectivity map layer.

use super::ctx::Ctx;
use super::view::{key_str, round, v2};
use nd_sim::world::connectivity::{AlertKind, Net};
use serde_json::{json, Value};

fn net_key(n: Net) -> &'static str {
    match n {
        Net::Walk => "walk",
        Net::Road => "road",
    }
}

pub(super) fn connectivity(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let w = &ctx.sim.world;
    let k = &w.connectivity;
    let areas: Vec<Value> = w
        .stranded_areas()
        .iter()
        .map(|a| {
            json!({
                "net": net_key(a.net), "center": v2(a.center), "lo": v2(a.lo), "hi": v2(a.hi),
                "lots": a.lots.iter().map(|l| key_str(*l)).collect::<Vec<_>>(),
                "buildings": a.buildings.iter().map(|b| json!({ "id": key_str(*b), "name": w.buildings.get(*b).map(|b| b.name.clone()) })).collect::<Vec<_>>(),
                "residents": a.residents, "workers": a.workers, "here_now": a.here_now, "text": a.describe(w),
            })
        })
        .collect();
    let alerts: Vec<Value> = k
        .alerts
        .iter()
        .map(|a| {
            let kind = match a.kind {
                AlertKind::NoRoute => "no_route".to_string(),
                AlertKind::Stranded(n) => format!("stranded_{}", net_key(n)),
            };
            json!({ "seq": a.seq, "tick": a.tick, "kind": kind, "pos": v2(a.pos), "building": a.building.map(key_str), "count": a.count, "text": a.text })
        })
        .collect();
    let failed: Vec<Value> = k.failed.iter().rev().map(|f| json!({ "tick": f.tick, "citizen": key_str(f.citizen), "from": v2(f.from), "target": key_str(f.target), "target_name": w.buildings.get(f.target).map(|b| b.name.clone()), "why": f.why })).collect();
    let mut out = json!({
        "walk_main": k.walk_main, "drive_main": k.drive_main,
        "walk_components": w.walk.comp.iter().collect::<std::collections::BTreeSet<_>>().len(),
        "stranded": areas, "alerts": alerts, "failed_trips": failed,
        "failed_today": k.failed_today.iter().map(|(b, n)| json!({ "building": key_str(*b), "name": w.buildings.get(*b).map(|b| b.name.clone()), "count": n })).collect::<Vec<_>>(),
    });
    if cmd.get("segments").and_then(Value::as_bool).unwrap_or(false) {
        let segs: Vec<Value> = w
            .connectivity_segments()
            .into_iter()
            .filter(|(_, l)| l.walk == Some(false) || l.road == Some(false) || cmd.get("all").and_then(Value::as_bool).unwrap_or(false))
            .map(|(id, l)| {
                let (a, b) = w.roads.segment_points(id);
                json!({ "segment": id.0, "a": [round(a.x), round(a.y)], "b": [round(b.x), round(b.y)], "walk": l.walk, "road": l.road })
            })
            .collect();
        out["segments"] = json!(segs);
    }
    Ok(out)
}
