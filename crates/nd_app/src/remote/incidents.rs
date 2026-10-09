//! `incident` and `incidents`: emergencies started by hand, and the record.

use serde_json::{json, Value};

use super::ctx::Ctx;
use super::view::*;

/// Start an emergency by hand, for testing and demos: `kind` fire / crime
/// / medical at a `building` (medical picks someone inside it, or `citizen`).
pub(super) fn incident(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let kind = req_str(cmd, "kind")?.trim().to_ascii_lowercase();
    let w = &mut ctx.sim.world;
    let id = match kind.as_str() {
        "fire" | "crime" => {
            let b = key_from(field(cmd, "building").ok_or("building is required")?)?;
            if !w.buildings.contains_key(b) {
                return Err("no such building".into());
            }
            let id = if kind == "fire" { w.start_fire(b) } else { w.start_crime(b) }.ok_or("cannot start that here (already burning, a site, a park or a station)")?;
            // [fire-scene] Dev: start it already this big (0..1), for a fire
            // that takes the engine a while to knock down.
            if let Some(k) = opt_f32(cmd, "intensity")?.filter(|_| kind == "fire") {
                if let Some(i) = w.services.incident_mut(id) {
                    i.intensity = k.clamp(0.01, 1.0);
                }
            }
            id
        }
        "medical" | "sick" => {
            let c = match field(cmd, "citizen") {
                Some(v) => key_from(v)?,
                None => {
                    let b: nd_sim::BuildingId = key_from(field(cmd, "building").ok_or("citizen or building is required")?)?;
                    w.citizens.iter().find(|(_, c)| c.inside == Some(b)).map(|(id, _)| id).ok_or("nobody is inside that building")?
                }
            };
            w.fall_ill(c).ok_or("that citizen is not indoors or is already a patient")?
        }
        // [emergency] A car chase: a free police car comes for a suspect
        // whose car is parked near home, and they run for it.
        "chase" => w.stage_chase()?,
        // [driving] A crash: the car given (or the nearest to `at`) crashes
        // at `speed` m/s (25: written off). Processed on the next tick.
        "crash" => {
            let car: nd_sim::CarId = match field(cmd, "car") {
                Some(v) => key_from(v)?,
                None => {
                    let at = cmd.get("at").and_then(|v| v.as_array()).and_then(|a| Some(nd_sim::Vec2::new(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32))).unwrap_or(nd_sim::Vec2::ZERO);
                    w.traffic.cars.iter().filter(|(_, c)| !c.siren && c.bus.is_none() && !c.in_stunt() && !c.crashed()).min_by(|a, b| a.1.pos.distance(at).total_cmp(&b.1.pos.distance(at))).map(|(id, _)| id).ok_or("no car on the road")?
                }
            };
            if !w.traffic.cars.contains_key(car) {
                return Err("no such car".into());
            }
            let speed = cmd.get("speed").and_then(|v| v.as_f64()).unwrap_or(25.0) as f32;
            w.traffic.crash(vec![car], nd_sim::traffic::CrashCause::OffRoad, speed);
            return Ok(json!({ "car": key_str(car), "crash": w.traffic.next_crash - 1 }));
        }
        // [driving] A street takeover at the junction nearest `at`.
        "takeover" => {
            let at = cmd.get("at").and_then(|v| v.as_array()).and_then(|a| Some(nd_sim::Vec2::new(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32)));
            let id = w.start_takeover(at)?;
            return Ok(json!({ "takeover": id }));
        }
        // [moto] A moto pack meeting at the road end nearest `at`.
        "moto_pack" | "moto" => {
            let at = cmd.get("at").and_then(|v| v.as_array()).and_then(|a| Some(nd_sim::Vec2::new(a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32)));
            let id = w.start_moto_pack(at)?;
            return Ok(json!({ "moto_pack": id }));
        }
        _ => return Err(format!("unknown incident kind {kind:?}; fire, crime, medical, crash, takeover or moto_pack")),
    };
    Ok(json!({ "incident": id }))
}

/// The incident record: `active` (only those under way), `limit` (newest
/// first, default 50), `kind` fire / crime / medical / crash.
pub(super) fn incidents(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let w = &ctx.sim.world;
    let only_active = opt_bool(cmd, "active")?.unwrap_or(false);
    let limit = opt_u64(cmd, "limit")?.unwrap_or(50) as usize;
    let kind = opt_str(cmd, "kind")?.map(|k| k.to_ascii_lowercase());
    let rows: Vec<Value> = w
        .services
        .incidents
        .iter()
        .rev()
        .filter(|i| !only_active || i.active())
        .filter(|i| kind.as_deref().is_none_or(|k| i.kind.label() == k))
        .take(limit)
        .map(|i| incident_row(w, i))
        .collect();
    let s = &w.services.stats;
    Ok(json!({
        "returned": rows.len(), "total_on_record": w.services.incidents.len(), "active": w.services.active().count(),
        "jail_occupancy": w.services.jail_occupancy().iter().map(|(b, n)| json!({ "jail": key_str(*b), "name": building_name(w, Some(*b)), "inmates": n })).collect::<Vec<_>>(),
        "held_at_stations": s.held_at_stations, "released_early_total": s.released_early_total,
        "out_of_town_calls_today": s.out_of_town_calls_today, "out_of_town_fees_today": round(s.out_of_town_fees_today),
        "chases_now": w.services.chases.len(), "chases_today": s.chases_today, "chases_tagged_today": s.chases_tagged_today, "chases_lost_today": s.chases_lost_today, "chases_total": s.chases_total, "chases_tagged_total": s.chases_tagged_total,
        "second_alarms_today": s.second_alarms_today, "second_alarms_total": s.second_alarms_total,
        "incidents": rows,
    }))
}

pub(super) fn incident_row(w: &nd_sim::World, i: &nd_sim::Incident) -> Value {
    json!({
        "id": i.id, "kind": i.kind.label(), "title": i.title(), "stage": i.stage.label(), "outcome": i.outcome.map(|o| o.label()),
        "building": key_str(i.building), "place": i.place, "pos": v2(i.pos),
        "started": i.started, "called": i.reported, "dispatched": i.dispatched, "arrived": i.arrived, "resolved": i.resolved,
        "call_to_dispatch_min": i.dispatched.zip(i.reported).map(|(a, b)| a.saturating_sub(b)), "dispatch_to_scene_min": i.travel_ticks(),
        "on_scene_min": i.work_ticks(), "call_to_arrival_min": i.response_ticks(),
        "unit": i.unit_name, "out_of_town": i.outside, "fee": round(i.fee), "intensity": round(i.intensity),
        "second_alarm": i.second_alarm, "second_unit": i.second.and_then(|u| w.services.units.get(u)).map(|u| format!("{} {}", u.kind.call_sign(), u.number)), "engines_on_scene": (i.kind == nd_sim::IncidentKind::Fire).then(|| w.engines_on_scene(i.id)),
        "people": i.people.iter().map(|p| json!({ "citizen": key_str(p.citizen), "name": p.name, "role": p.role, "taken_to": p.taken_to })).collect::<Vec<_>>(),
        "damage": i.damage.iter().map(|d| json!({ "building": key_str(d.building), "name": d.name, "condition_lost": round(d.lost), "destroyed": d.destroyed })).collect::<Vec<_>>(),
        "still_burning": i.active() && i.kind == nd_sim::IncidentKind::Fire && w.services.is_burning(i.building),
        "chase": i.chase.as_ref().map(|c| json!({ "vehicle": c.vehicle, "started": c.started, "ended": c.ended, "result": c.result.map(|r| r.label()), "metres": round(c.metres), "closest_m": round(c.closest) })),
    })
}
