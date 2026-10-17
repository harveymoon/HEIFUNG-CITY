//! [road-inspect] A street piece in full, for `inspect {road}`, a selected
//! road and `repair_road`: the road card's figures (`road_select::Card`).

use super::ctx::Ctx;
use super::view::{field, round, v2, vec2_of};
use crate::tools::Selection;
use crate::road_select::Card;
use crate::tools::Depth;
use nd_sim::{SegmentId, World};
use serde_json::{json, Value};

/// The piece a `road` (or `segment`) field names: an id, or `[x, y]` for
/// the piece a click there would pick (else the nearest road).
pub fn road_id(w: &World, v: &Value, depth: Depth) -> Result<SegmentId, String> {
    let sid = if let Some(n) = v.as_u64() {
        SegmentId(n as u32)
    } else if let Some(n) = v.as_str().and_then(|s| s.parse::<u32>().ok()) {
        SegmentId(n)
    } else {
        let p = vec2_of(v)?;
        crate::road_select::pick(w, p, depth).or_else(|| w.roads.nearest_on_network(p).map(|n| n.0)).ok_or("no roads")?
    };
    w.roads.try_segment(sid).filter(|s| s.alive).ok_or("no such road piece")?;
    Ok(sid)
}

pub fn road_detail(w: &World, sid: SegmentId) -> Option<Value> {
    let c = Card::of(w, sid)?;
    let (a, b) = w.roads.segment_points(sid);
    let n = &c.now;
    let (this_hour, last_hour, today, yesterday) = c.flows();
    let first = c.hour_now as i64 + 1 - c.hours.len() as i64;
    let decay = c.decay.as_ref().map(|d| {
        json!({
            "wear": round(d.wear), "condition": d.label, "speed_factor": round(d.speed_factor), "crash_factor": round(d.crash_factor),
            "wear_per_day": d.rate, "wear_per_day_measured": d.rate_measured,
            "next_condition": d.next.map(|(l, days)| json!({ "label": l, "days": round(days) })),
            "repair_cost": round(d.repair_cost), "land_value": round(d.land_value),
            "queued_for_repair": d.queue_place.is_some(), "queue_place": d.queue_place, "queue_length": d.queue_len, "priority": round(d.priority),
            "last_repaired_day": d.repaired_day.map(|x| x + 1), "last_repair_whole": d.repaired_day.map(|_| d.repaired_whole), "last_repair_ordered": d.repaired_day.map(|_| d.repaired_by_order),
            "funding": round(d.funding),
        })
    });
    Some(json!({
        "kind": "road", "id": sid.0, "segment": sid.0, "name": c.name, "road_kind": c.kind, "level": c.level,
        "lanes_fwd": c.lanes_fwd, "lanes_bwd": c.lanes_bwd, "one_way": c.one_way,
        "speed_limit_kmh": round(c.limit * 3.6), "length_m": round(c.length), "from": v2(a), "to": v2(b),
        "run": { "pieces": c.run.iter().map(|s| s.0).collect::<Vec<_>>(), "index": c.run_index, "length_m": round(c.run_length) },
        "now": {
            "vehicles": n.vehicles(), "cars": n.cars, "lorries": n.lorries, "buses": n.buses, "emergency_or_city": n.sirens,
            "bikes": n.bikes, "walkers": n.walkers,
            "mean_speed_kmh": round(n.mean_speed * 3.6), "allowed_kmh": round(n.limit * 3.6), "congestion": n.congestion(),
            "queued": n.queued, "queue_m": round(n.queue_m), "per_lane_km": round(n.density()),
        },
        "carries_vehicles": c.vehicles,
        "flow": { "counts": "vehicles", "this_hour": this_hour, "last_hour": last_hour, "today": today, "yesterday": yesterday, "peak": c.peak().map(|(h, k)| json!({ "day": h / 24 + 1, "hour": h % 24, "vehicles": k })) },
        "hourly": c.hours,
        "hourly_from": json!({ "day": first.max(0) / 24 + 1, "hour": first.rem_euclid(24), "before_start": first < 0 }),
        "decay": decay,
        "tolls": c.tolls.iter().map(|(id, name, price)| json!({ "id": id, "name": name, "price": round(*price) })).collect::<Vec<_>>(),
        "crossings": { "junction": c.crossings.0, "zebra": c.crossings.1, "push_button": c.crossings.2, "footbridge": c.crossings.3 },
        "bus_stops": c.bus_stops, "kerb_parking": c.kerb_parking, "kerb_parked": c.kerb_parked,
        "note": "figures are for this piece (a run between junctions may be several pieces: `run`); flow counts vehicles driving onto it, both ways; hourly: the last 72 hours, oldest first, the last the hour under way; decay is null for ways the city's crews do not mend",
    }))
}

/// [road-inspect] `repair_road {road?}`: "Repair now" on a piece (by id or
/// [x, y]; else the selected one): resurfaced at once, paid from the
/// treasury as road maintenance.
pub(super) fn repair_road(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let sid = match field(cmd, "road").or_else(|| field(cmd, "segment")) {
        Some(v) => road_id(&ctx.sim.world, v, ctx.tools.depth)?,
        None => match *ctx.selection {
            Selection::Road(sid) => sid,
            _ => return Err("missing field \"road\": an id or [x, y] (or select a road first)".into()),
        },
    };
    let cost = ctx.sim.world.repair_road_now(sid)?;
    Ok(json!({ "repaired": sid.0, "cost": round(cost), "treasury": round(ctx.sim.world.economy.treasury), "road": road_detail(&ctx.sim.world, sid) }))
}
