//! [parks] (D5) The parks by remote.
//!
//! - `parks {}`: every park (one per park of lots side by side): name,
//!   lots, area, kept, run_down, litter, dealers, campers, homeless,
//!   visits, preserve, clean-up cost.
//! - `park {building, clean_up: true}`: pay for a clean-up;
//!   `park {building, preserve: bool}`: leave it wild or keep it again;
//!   `park {building, set: {trash, dealers}}` (dev) sets its state.

use nd_sim::parks::ParkState;
use nd_sim::{BuildingId, World};
use serde_json::{json, Value};

use super::ctx::Ctx;
use super::view::{field, key_from, key_str, opt_bool, round, v2};

pub fn park_json(w: &World, a: BuildingId, s: &ParkState) -> Value {
    json!({
        "building": key_str(a), "name": s.name, "pos": w.buildings.get(a).and_then(|b| w.lots.get(b.lot)).map(|l| v2(l.center)),
        "lots": s.lots.len().max(1), "members": s.members.iter().map(|m| key_str(*m)).collect::<Vec<_>>(), "area_m2": round(s.area),
        "kept": round(s.kept), "run_down": s.run_down(), "litter": round(s.trash), "dealers": s.dealers, "campers": s.campers, "homeless": s.homeless,
        "visits_today": s.visits_today, "visits_yesterday": s.visits_yesterday, "preserve": s.preserve, "cleanups": s.cleanups,
        "cleanup_cost": round(s.cleanup_cost()), "troubles": s.troubles(),
    })
}

pub(super) fn parks(_cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let w = &ctx.sim.world;
    let list: Vec<Value> = w.parks.parks.iter().map(|(a, s)| park_json(w, *a, s)).collect();
    Ok(json!({ "parks": list, "run_down": w.parks.parks.values().filter(|s| s.run_down()).count(), "preserves": w.parks.parks.values().filter(|s| s.preserve).count() }))
}

pub(super) fn park(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let id: BuildingId = key_from(field(cmd, "building").ok_or("park needs building: a park's building")?)?;
    let w = &mut ctx.sim.world;
    if w.parks.anchor(id).is_none() {
        // A park just made is listed on the hour; look now.
        w.refresh_parks_now();
    }
    let a = w.parks.anchor(id).ok_or("not one of the city's parks")?;
    let mut out = json!({});
    if let Some(set) = field(cmd, "set") {
        let s = w.parks.parks.get_mut(&a).unwrap();
        if let Some(t) = set.get("trash").or_else(|| set.get("litter")).and_then(Value::as_f64) {
            s.trash = (t as f32).clamp(0.0, 1.0);
        }
        if let Some(d) = set.get("dealers").and_then(Value::as_u64) {
            s.dealers = d.min(nd_sim::parks::MAX_DEALERS as u64) as u8;
        }
        s.kept = ParkState::kept_for(s.trash, s.dealers, s.campers, s.homeless);
        w.parks.version += 1;
        w.events.push(nd_sim::SimEvent::BuildingChanged(a));
        let members = w.parks.parks[&a].members.clone();
        for m in members {
            w.events.push(nd_sim::SimEvent::BuildingChanged(m));
        }
    }
    if opt_bool(cmd, "clean_up")?.unwrap_or(false) {
        out["clean_up"] = match w.clean_up_park(a) {
            Ok((cost, msg)) => json!({ "ok": true, "cost": round(cost), "message": msg }),
            Err(e) => json!({ "ok": false, "reason": e }),
        };
    }
    if let Some(on) = opt_bool(cmd, "preserve")? {
        out["preserve"] = match w.set_park_preserve(a, on) {
            Ok(msg) => json!({ "ok": true, "message": msg }),
            Err(e) => json!({ "ok": false, "reason": e }),
        };
    }
    let w = &ctx.sim.world;
    out["park"] = park_json(w, a, &w.parks.parks[&a]);
    out["treasury"] = json!(round(w.economy.treasury));
    Ok(out)
}
