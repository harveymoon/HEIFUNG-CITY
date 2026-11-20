//! [r11-lot-memory] The lot history (`nd_sim::lot_memory`) for the remote
//! API: `lot_memory` reports it (the city's totals, the newest traces, the
//! history of the lot at a point) and, for testing, pulls a building down,
//! changes a trade or ages the history to see the ghosts fade.

use nd_sim::lot_memory::TraceKind;
use nd_sim::World;
use serde_json::{json, Value};

use super::view::{field, key_from, lot_memory_json, opt_str, opt_u64, opt_vec2, v2};

fn trace_row(t: &nd_sim::lot_memory::Trace, tick: u64) -> Value {
    json!({ "id": t.id, "kind": if t.kind == TraceKind::Demolished { "demolished" } else { "changed" }, "at": v2(t.at), "weeks_ago": t.weeks(tick), "line": t.line() })
}

pub(super) fn command(cmd: &Value, sim: &mut crate::sim::Sim) -> Result<Value, String> {
    let ghosts: Vec<(nd_sim::BuildingId, u32)> = {
        let mut v: Vec<(nd_sim::BuildingId, u32)> = sim.ghost_walls.iter().filter(|(id, _)| sim.world.buildings.contains_key(**id)).map(|(k, n)| (*k, *n)).collect();
        v.sort();
        v
    };
    let w = &mut sim.world;
    let mut done = Vec::new();
    if let Some(v) = field(cmd, "demolish") {
        done.push(w.dev_demolish(key_from(v)?)?);
    }
    if let Some(v) = field(cmd, "change") {
        let to = match opt_str(cmd, "to")? {
            None | Some("shut") | Some("none") => None,
            Some(k) => Some(nd_sim::BusinessKind::from_key(k).ok_or_else(|| format!("no trade {k}"))?),
        };
        done.push(w.dev_change_trade(key_from(v)?, to)?);
    }
    if let Some(days) = opt_u64(cmd, "age_days")? {
        w.lot_memory.age(days.min(400) * nd_sim::clock::MINUTES_PER_DAY);
        done.push(format!("aged {days} days"));
    }
    let tick = w.clock.tick;
    let m = &w.lot_memory;
    let n = opt_u64(cmd, "top")?.unwrap_or(10).min(200) as usize;
    let mut v = json!({
        "traces": m.len(), "demolished": m.traces().iter().filter(|t| t.kind == TraceKind::Demolished).count(),
        "changed": m.traces().iter().filter(|t| t.kind == TraceKind::Changed).count(), "version": m.version,
        "per_lot": nd_sim::lot_memory::PER_LOT, "max": nd_sim::lot_memory::MAX_TRACES,
        "newest": m.traces().iter().rev().take(n).map(|t| trace_row(t, tick)).collect::<Vec<_>>(),
        "done": done,
        // What the renderer drew last: the walls carrying ghosts.
        "ghost_walls": { "buildings": ghosts.len(), "triangles": ghosts.iter().map(|g| g.1).sum::<u32>(), "on": ghosts.iter().take(n).map(|(id, t)| json!({ "building": super::view::key_str(*id), "pos": w.buildings.get(*id).and_then(|b| w.lots.get(b.lot)).map(|l| v2(l.center)), "triangles": t })).collect::<Vec<_>>() },
    });
    if let Some(p) = opt_vec2(cmd, "at")? {
        let lot = w.lots.values().find(|l| l.contains(p));
        v["at"] = json!({ "lot": lot.is_some(), "vacant": lot.is_some_and(|l| l.building.is_none()), "what_stood_here": lot_memory_json(w, lot) });
    }
    Ok(v)
}
