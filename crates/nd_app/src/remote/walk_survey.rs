//! [walk-crossings] `walk_crossings`: every crossing near a point (or in
//! town) and how it has been used since the walk graph was built or the
//! survey last reset: walkers at grade (and how many of them against the
//! light or mid-block), walkers and cyclists over its footbridge.

use super::ctx::Ctx;
use super::view::{opt_bool, opt_f32, opt_vec2, round, v2};
use nd_sim::walk::CrossingKind;
use serde_json::{json, Value};

pub(super) fn walk_crossings(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    if opt_bool(cmd, "reset")? == Some(true) {
        ctx.sim.world.peds.uses.clear();
    }
    let w = &ctx.sim.world;
    let at = opt_vec2(cmd, "at")?;
    let r = opt_f32(cmd, "radius")?.unwrap_or(80.0).max(1.0);
    let all = opt_bool(cmd, "all")? == Some(true);
    let (mut grade, mut against, mut unmarked, mut bridge, mut bikes) = (0u32, 0u32, 0u32, 0u32, 0u32);
    let mut rows: Vec<Value> = Vec::new();
    for (k, c) in w.walk.crossings.iter().enumerate() {
        if at.is_some_and(|p| p.distance(c.mid) > r) {
            continue;
        }
        let u = w.peds.uses.get(&(k as u32)).copied().unwrap_or_default();
        grade += u.at_grade;
        against += u.against;
        bridge += u.bridge;
        bikes += u.bridge_bikes;
        let unmarked_here = !c.marked() || c.kind == CrossingKind::Jay;
        if unmarked_here {
            unmarked += u.at_grade;
        }
        let used = u.at_grade + u.bridge + u.bridge_bikes + u.replans > 0;
        // Unused jaywalking edges are many: only listed when asked for.
        if c.kind == CrossingKind::Jay && !used && !all {
            continue;
        }
        let street = w.streets.of(c.segment).map(|s| s.name.clone());
        rows.push(json!({
            "index": k, "kind": format!("{:?}", c.kind).to_lowercase(), "mid": v2(c.mid), "segment": c.segment.0, "street": street,
            "marked": c.marked(), "closed": c.closed, "bridged": c.bridged, "button": c.button,
            "at_grade": u.at_grade, "against": u.against, "bridge": u.bridge, "bridge_bikes": u.bridge_bikes, "replans": u.replans,
        }));
    }
    Ok(json!({
        "at": at.map(v2), "radius": round(r), "walk_version": w.walk.version,
        "totals": { "at_grade": grade, "against": against, "unmarked_at_grade": unmarked, "bridge": bridge, "bridge_bikes": bikes },
        "replans_total": w.peds.replans_total, "jay_give_ups_total": w.peds.jay_give_ups_total, "jaywalks_total": w.peds.jaywalks_total,
        "collisions_total": w.peds.collisions_total, "near_misses_total": w.peds.near_misses_total,
        "crossings": rows,
    }))
}
