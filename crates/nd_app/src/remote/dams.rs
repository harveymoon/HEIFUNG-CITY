//! [dams] (H1) Hydro dam walls by remote.
//!
//! - `dams {}`: every hydro plant with its dam (drawn wall, awaited, or the
//!   old automatic arch).
//! - `dam {building, a, b}`: what a wall from bank `a` to bank `b` would be
//!   (length, span, cost, expected MW) or why not; `build: true` builds it.
//! - `dam {building, step: true}`: the Buildings tool's wall step for that
//!   plant, as after placing it; then `cursor: [x, y]` reads the wall under
//!   the pointer out as the panel does, and `click: [x, y]` clicks (first
//!   bank, then the far bank, which builds). `step: false` leaves it.

use nd_sim::dams::DamPlan;
use nd_sim::{BuildingId, PowerPlantKind, World};
use serde_json::{json, Value};

use super::ctx::Ctx;
use super::view::{field, key_from, key_str, opt_bool, opt_vec2, round, v2};

fn plan_json(d: &DamPlan) -> Value {
    json!({ "ok": true, "a": v2(d.a), "b": v2(d.b), "length_m": round(d.length), "span_m": round(d.span), "cost": round(d.cost), "expected_mw": round(d.expected_mw), "readout": crate::tools::dam_readout(d) })
}

fn plant_json(w: &World, id: BuildingId) -> Value {
    let b = &w.buildings[id];
    let dam = match w.dams.wall_of(id) {
        Some(wall) => json!({ "state": "wall", "a": v2(wall.a), "b": v2(wall.b), "length_m": round(wall.length()), "footpath": wall.path.map(|g| g.0), "crest": wall.crest(6).iter().map(|p| v2(*p)).collect::<Vec<_>>() }),
        None if w.dams.is_pending(id) => json!({ "state": "awaiting_wall" }),
        None => json!({ "state": "automatic", "at": w.auto_dam(id).map(|(c, ..)| v2(c)) }),
    };
    json!({ "building": key_str(id), "name": b.name, "pos": w.lots.get(b.lot).map(|l| v2(l.center)), "output_mw": round(w.plant_output_mw(b)), "nameplate_mw": round(w.plant_capacity_mw(b)), "dam": dam })
}

pub(super) fn dams(_cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let w = &ctx.sim.world;
    let plants: Vec<Value> = w.buildings.iter().filter(|(_, b)| b.plant == Some(PowerPlantKind::Hydro)).map(|(id, _)| plant_json(w, id)).collect();
    Ok(json!({ "plants": plants, "rules": { "cost_per_m": nd_sim::dams::DAM_COST_PER_M, "max_length_m": nd_sim::dams::DAM_MAX_LENGTH, "near_plant_m": nd_sim::dams::DAM_NEAR_PLANT, "bank_reach_m": nd_sim::dams::DAM_BANK_REACH } }))
}

pub(super) fn dam(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let id: BuildingId = key_from(field(cmd, "building").ok_or("dam needs building: the hydro plant's id")?)?;
    if ctx.sim.world.buildings.get(id).is_none_or(|b| b.plant != Some(PowerPlantKind::Hydro)) {
        return Err("not a hydro plant".into());
    }
    // The tool's step.
    if let Some(on) = opt_bool(cmd, "step")? {
        ctx.tools.dam = on.then_some(crate::tools::DamStep { plant: id, a: None });
        ctx.tools.dam_note = None;
        if on {
            ctx.tools.tool = crate::tools::Tool::Buildings;
            ctx.sim.world.await_dam_wall(id);
        }
    }
    if let Some(c) = opt_vec2(cmd, "cursor")? {
        ctx.tools.cursor_pin = Some(c);
    }
    let mut out = json!({});
    if let Some(p) = opt_vec2(cmd, "click")? {
        let step = ctx.tools.dam.filter(|s| s.plant == id).ok_or("no wall step for this plant: send step: true first")?;
        ctx.tools.cursor_pin = Some(p);
        out["click"] = match step.a {
            None => match ctx.sim.world.dam_end_problem(p) {
                Some(why) => json!({ "ok": false, "reason": why }),
                None => {
                    ctx.tools.dam = Some(crate::tools::DamStep { a: Some(p), ..step });
                    json!({ "ok": true, "first_bank": v2(p) })
                }
            },
            Some(a) => {
                let status = crate::tools::dam_click(&mut ctx.sim, &mut ctx.tools, &mut ctx.selection, id, a, p);
                ctx.tools.status = status.clone();
                json!({ "ok": ctx.tools.dam.is_none(), "status": status })
            }
        };
    }
    // The read-out under the pointer, as the panel shows it.
    if let (Some(step), Some(p)) = (ctx.tools.dam, ctx.tools.cursor_pin) {
        let w = &ctx.sim.world;
        out["readout"] = match step.a {
            None => match w.dam_end_problem(p) {
                Some(why) => json!({ "ok": false, "reason": why }),
                None => json!({ "ok": true, "note": "first bank here" }),
            },
            Some(a) => match w.plan_dam_wall(id, a, p) {
                Ok(d) => plan_json(&d),
                Err(why) => json!({ "ok": false, "reason": why }),
            },
        };
    }
    // A wall straight from a to b.
    if let (Some(a), Some(b)) = (opt_vec2(cmd, "a")?, opt_vec2(cmd, "b")?) {
        let build = opt_bool(cmd, "build")?.unwrap_or(false);
        let w = &mut ctx.sim.world;
        out["wall"] = match if build { w.build_dam_wall(id, a, b) } else { w.plan_dam_wall(id, a, b) } {
            Ok(d) => {
                let mut v = plan_json(&d);
                v["built"] = json!(build);
                v
            }
            Err(why) => json!({ "ok": false, "reason": why }),
        };
    }
    out["step"] = match ctx.tools.dam {
        Some(s) if s.plant == id => json!({ "first_bank": s.a.map(v2) }),
        _ => Value::Null,
    };
    out["plant"] = plant_json(&ctx.sim.world, id);
    out["treasury"] = json!(round(ctx.sim.world.economy.treasury));
    Ok(out)
}
