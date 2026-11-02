//! [airport] `airports` (every airport's card), the airport branch of
//! `place` (fitted to the ground, `heading` and `check`), and the
//! `airport` section of `inspect {building}`.

use nd_sim::airport::{AirportSize, Plane};
use nd_sim::{BuildingId, Vec2 as SVec2, World};
use serde_json::{json, Value};

use super::ctx::Ctx;
use super::view::*;
use crate::tools::Selection;

/// `airports`: every airport with its card, the planes on and over it
/// (where each one is now), and the sizes on offer.
pub(super) fn airports(_cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let w = &ctx.sim.world;
    let list: Vec<Value> = w.airports.airports.values().filter_map(|a| a.building).map(|b| building_airport(w, b)).collect();
    let sizes: Vec<Value> = AirportSize::ALL
        .iter()
        .map(|s| {
            let (len, wid) = s.size();
            json!({ "kind": s.key(), "label": s.label(), "cost": s.cost(), "upkeep_per_day": s.upkeep_per_day(), "jobs": s.jobs(), "site_m": [len, wid], "runways": s.runways().len(), "rotations_per_day": s.rotations() })
        })
        .collect();
    Ok(json!({ "airports": list, "sizes": sizes, "rule": format!(
        "placed with place {{kind: airfield|airport, x, y, heading?}}: fitted to the ground (every heading tried without one), on dry ground levelled by at most {:.0} m, off roads and civic buildings, with nothing rising through the approach surfaces (1:{:.0} from {:.0} m past each runway end, {:.0} m out); they are height limit zones afterwards",
        nd_sim::airport::MAX_EARTHWORK, 1.0 / nd_sim::airport::APPROACH_SLOPE, nd_sim::airport::APPROACH_START, nd_sim::airport::APPROACH_LENGTH
    ) }))
}

/// One airport's card for `inspect` and `airports`.
pub fn building_airport(w: &World, b: BuildingId) -> Value {
    let Some(r) = w.airport_report(b) else { return Value::Null };
    let Some((_, a)) = w.airports.by_building(b) else { return Value::Null };
    let t = w.clock.tick as f32;
    let planes: Vec<Value> = a.planes.iter().map(|p| plane_json(a, p, t)).collect();
    let mut v = serde_json::to_value(&r).unwrap_or(Value::Null);
    v["building"] = json!(key_str(b));
    v["centre"] = v2(a.center);
    v["footprint"] = points(&a.footprint());
    v["forecourt_road"] = points(&a.forecourt());
    v["level_m"] = json!(round(a.level));
    v["planes_now"] = json!(planes);
    v
}

fn plane_json(a: &nd_sim::airport::Airport, p: &Plane, t: f32) -> Value {
    let pose = nd_sim::airport::plane_pose(a, p, t);
    json!({ "flight": p.call_sign(), "kind": p.kind.label(), "phase": p.phase.label(), "from": p.from_city(), "to": p.to_city(), "pos": v2(pose.pos), "alt_m": round(pose.alt), "heading_deg": round(pose.heading.to_degrees().rem_euclid(360.0)), "stand": p.stand })
}

/// `place {kind: airfield|airport, x, y, heading?, check?}`.
pub(super) fn place(cmd: &Value, ctx: &mut Ctx, size: AirportSize, p: SVec2) -> Result<Value, String> {
    let heading = opt_f32(cmd, "heading")?;
    let clear = opt_bool(cmd, "clear")?.unwrap_or(false);
    let w = &mut ctx.sim.world;
    if opt_bool(cmd, "check")?.unwrap_or(false) {
        return Ok(match w.plan_airport_with(size, p, heading, clear) {
            Ok(a) => json!({ "ok": true, "heading_deg": round(a.along.y.atan2(a.along.x).to_degrees().rem_euclid(180.0)), "level_m": round(a.level), "footprint": points(&a.footprint()), "cost": round(size.cost() + a.earthwork_cost), "earthworks_and_clearing": round(a.earthwork_cost), "streets_cleared_m": round(a.cleared_m) }),
            Err(why) => json!({ "ok": false, "reason": why }),
        });
    }
    let before = w.economy.treasury;
    let id = w.place_airport_with(size, p, heading, clear)?;
    let paid = before - w.economy.treasury;
    let connected = w.airports.by_building(id).is_some_and(|(_, a)| a.road_connected);
    let note = if connected {
        format!("{} built for {:.0}; {:.0}/day to run; flights from the next timetabled arrival", size.label(), paid, size.upkeep_per_day())
    } else {
        format!("{} built for {:.0}; not connected to the road network yet: draw a road to its forecourt road (forecourt_road)", size.label(), paid)
    };
    *ctx.selection = Selection::Building(id);
    ctx.tools.status = note.clone();
    Ok(json!({ "building": key_str(id), "message": note, "airport": building_airport(&ctx.sim.world, id), "treasury": round(ctx.sim.world.economy.treasury) }))
}
