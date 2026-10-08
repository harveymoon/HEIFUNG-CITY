//! [rail-crossing] `level_crossings` (alias `crossings`): every level
//! crossing, or the one nearest `at`, with its guard (barriers, lights,
//! footpath gates), where it is in its cycle, the trains coming, and the
//! counts (trains an hour, closures, vehicles held and their average wait).

use nd_sim::level_crossing::LevelCrossing;
use nd_sim::World;
use serde_json::{json, Value};

use super::view::*;

/// One crossing as JSON.
pub fn crossing_json(w: &World, k: usize, c: &LevelCrossing) -> Value {
    let now = w.clock.tick;
    let coming: Vec<Value> = w.trains_coming(c).iter().take(4).map(|(id, s)| json!({ "train": key_str(*id), "seconds": round(*s) })).collect();
    let arms: Vec<Value> = c
        .arms
        .iter()
        .map(|a| {
            json!({
                "segment": a.segment, "away": v2(a.away), "lanes_in": a.lanes_in, "lanes_out": a.lanes_out, "path": a.path,
                "stop_line_m": round(a.stop), "barrier": c.guard.has_arms() && (a.path || a.lanes_in > 0),
                "barrier_length_m": if c.guard.has_arms() { round(a.arm_length()) } else { 0.0 },
            })
        })
        .collect();
    json!({
        "index": k, "node": c.node.0, "at": v2(c.pos), "rail_dir": v2(c.rail_dir),
        "guard": c.guard.label(), "barriers": c.guard.has_arms(), "lanes": c.lanes, "reach_m": round(c.reach),
        "phase": c.phase.label(), "lights_flashing": c.flashing(), "closed_to_traffic": c.shut(),
        "barrier_down": round(c.arm), "train_over": c.occupied, "eta_s": c.eta.map(round),
        "trains_coming": coming,
        "vehicles_waiting": c.vehicles_waiting, "people_waiting": c.people_waiting,
        "trains_last_hour": c.trains_last_hour(now), "trains_per_hour": round(c.trains_per_hour(now)), "trains_total": c.trains_total,
        "closures": c.closures, "closure_s": round(c.closure_seconds), "average_closure_s": round(c.average_closure()),
        "vehicles_held": c.vehicles_held, "average_wait_s": round(c.average_wait()), "trains_held": c.trains_held,
        "arms": arms,
    })
}

/// The crossing nearest `p` within `radius`, as JSON.
pub fn near_json(w: &World, p: nd_sim::Vec2, radius: f32) -> Option<Value> {
    let c = w.level_crossing_near(p, radius)?;
    let k = w.level_crossings().iter().position(|x| x.node == c.node)?;
    Some(crossing_json(w, k, c))
}

/// `{"cmd":"level_crossings"}` lists them; with `at: [x,y]` (and `radius`,
/// default 60 m) the nearest one; with `index` that one.
pub fn command(cmd: &Value, w: &World) -> Result<Value, String> {
    if let Some(p) = opt_vec2(cmd, "at")? {
        let r = opt_f32(cmd, "radius")?.unwrap_or(60.0);
        return near_json(w, p, r).map(|c| json!({ "crossing": c })).ok_or_else(|| format!("no level crossing within {r} m"));
    }
    if let Some(k) = field(cmd, "index").and_then(|v| v.as_u64()) {
        let c = w.level_crossings().get(k as usize).ok_or_else(|| format!("no level crossing {k}"))?;
        return Ok(json!({ "crossing": crossing_json(w, k as usize, c) }));
    }
    let list: Vec<Value> = w.level_crossings().iter().enumerate().map(|(k, c)| crossing_json(w, k, c)).collect();
    let shut = w.level_crossings().iter().filter(|c| c.shut()).count();
    Ok(json!({ "count": list.len(), "shut_now": shut, "crossings": list }))
}
