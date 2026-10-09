//! Dev and testing levers: money, contamination, murals, a walk, the sky,
//! and laying the roads again.

use serde_json::{json, Value};

use super::ctx::Ctx;
use super::view::*;

/// Dev: money for trying things out.
pub(super) fn grant(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let amount = opt_f32(cmd, "amount")?.unwrap_or(100_000.0);
    ctx.sim.world.economy.treasury += amount;
    Ok(json!({ "treasury": round(ctx.sim.world.economy.treasury) }))
}

/// [dirty-industry] Testing: pour contamination onto the ground.
pub(super) fn contaminate(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let p = req_point(cmd)?;
    let r = opt_f32(cmd, "radius")?.unwrap_or(60.0).clamp(0.0, 400.0);
    let a = opt_f32(cmd, "amount")?.unwrap_or(0.6).clamp(0.0, 1.0);
    ctx.sim.world.contamination.add(p, r, a);
    Ok(json!({ "ok": true, "at": [p.x, p.y], "contamination": round(ctx.sim.world.contamination.at(p)) }))
}

/// [social] Testing: a street-art crew's mural on a building now.
pub(super) fn paint_mural(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let bid: nd_sim::BuildingId = key_from(field(cmd, "building").ok_or("missing building")?)?;
    let w = &mut ctx.sim.world;
    let b = w.buildings.get(bid).ok_or("no such building")?;
    let lot = b.lot;
    let l = w.lots.get(lot).ok_or("no lot")?;
    let along = (l.front_edge[1] - l.front_edge[0]).normalize_or_zero();
    let mid = (l.front_edge[0] + l.front_edge[1]) * 0.5;
    let inward = (l.center - mid).normalize_or_zero();
    let group = match opt_u64(cmd, "group")? {
        Some(g) => g as u32,
        None => w.social.groups.iter().find(|g| g.kind == nd_sim::social::GroupKind::StreetArt).map_or(0, |g| g.id),
    };
    let quality = opt_f32(cmd, "quality")?.unwrap_or(0.6).clamp(-1.0, 1.0);
    let seed = opt_u64(cmd, "seed")?.unwrap_or((l.center.x * 31.0 + l.center.y * 17.0).abs() as u64 ^ 0x5EED);
    w.social.murals.retain(|m| m.lot != lot);
    w.social.murals.push(nd_sim::social::Mural { group, lot, building: Some(bid), seed, day: w.clock.day(), quality, pos: mid + inward * 0.9, along, facing: -inward, width: 6.0, height: 3.4 });
    Ok(json!({ "ok": true, "group": group, "at": [mid.x, mid.y], "facing": [-inward.x, -inward.y] }))
}

/// Dev: send a citizen out on foot (to try crossings and footbridges).
pub(super) fn walk_to(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let id = key_from(field(cmd, "citizen").ok_or("missing field \"citizen\": id")?)?;
    let to = opt_vec2(cmd, "to")?.ok_or("missing field \"to\": [x, y]")?;
    let from = opt_vec2(cmd, "from")?;
    let n = ctx.sim.world.dev_walk(id, from, to)?;
    Ok(json!({ "citizen": key_str(id), "route_points": n }))
}

/// [climate] Dev: set the sky, to see rain, snow or sun on demand.
pub(super) fn set_weather(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let w = &mut ctx.sim.world;
    if let Some(c) = opt_f32(cmd, "cloud")? {
        w.weather.cloud = c.clamp(0.0, 1.0);
    }
    if let Some(v) = opt_f32(cmd, "wind")? {
        w.weather.wind = v.clamp(0.05, 1.0);
    }
    if let Some(t) = opt_f32(cmd, "temp_c")? {
        w.weather.temp_c = t.clamp(-50.0, 55.0);
    }
    Ok(json!({ "cloud": round(w.weather.cloud), "wind": round(w.weather.wind), "temp_c": round(w.weather.temp_c), "spell": format!("{:?}", w.weather.spell).to_lowercase(), "label": w.weather.label(), "climate": w.landscape.params.climate.key(), "weather_climate": w.weather.climate.key(), "precipitation": crate::render::climate::precip_now(w).map(|(k, i)| json!({ "kind": k, "intensity": round(i) })) }))
}

/// [interchange] Dev: lay the roads again from the plan (as an implement
/// does) with the current rules, nothing in the plan changed.
pub(super) fn rebuild_roads(_: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let w = &mut ctx.sim.world;
    let before = w.roads.alive_segments().count();
    w.rebuild_from_plan();
    Ok(json!({ "segments_before": before, "segments": w.roads.alive_segments().count(), "roads_version": w.roads.version }))
}
