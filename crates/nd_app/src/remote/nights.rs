//! [events] Street races, warehouse raves and road condition for the remote
//! API: `street_events` (what is on tonight, the record, the news, the
//! warehouses weighed for a rave), `race` and `rave` (start one now), and
//! `roads` (wear, the maintenance budget and who it serves; sets the
//! funding level). `segment_condition` is the piece's line in
//! `inspect {segment}`; `report_json` the summary in `report.traffic`.

use nd_sim::road_wear::{self, lane_metres, wears};
use nd_sim::street_events::{EventRecord, News, Race, Rave, RaveSpot};
use nd_sim::{BuildingId, SegmentId, World};
use serde_json::{json, Value};

use super::view::{field, key_from, key_str, opt_f32, opt_u64, opt_vec2, round, tick_time, v2};

fn news_row(n: &News) -> Value {
    json!({ "seq": n.seq, "tick": n.tick, "time": tick_time(n.tick), "kind": n.kind.label(), "headline": n.headline, "pos": v2(n.pos) })
}

fn record_row(r: &EventRecord) -> Value {
    json!({
        "id": r.id, "kind": r.kind.label(), "place": r.place, "pos": v2(r.pos), "finish": r.finish,
        "started": tick_time(r.started), "ended": tick_time(r.ended), "minutes": r.ended.saturating_sub(r.started),
        "people": r.people, "crowd": r.crowd, "winner": r.winner, "police": r.police, "arrests": r.arrests,
        "fines": round(r.fines), "complaints": r.complaints, "chase": r.chase, "culture": round(r.culture),
    })
}

fn race_json(w: &World, r: &Race) -> Value {
    let racers: Vec<Value> = r
        .cars
        .iter()
        .zip(r.racers.iter())
        .map(|(&car, &cid)| {
            let c = w.traffic.cars.get(car);
            json!({
                "car": key_str(car), "driver": key_str(cid), "name": w.citizens.get(cid).map(|c| c.name.as_str()),
                "pos": c.map(|c| v2(c.pos)), "speed_kmh": c.map(|c| round(c.v * 3.6)),
                "parked_in_lot": c.is_some_and(|c| c.in_stunt()), "place": r.place_of(car),
                "phase": c.and_then(|c| c.drive.gang).map(|g| format!("{:?}", g.phase).to_lowercase()),
            })
        })
        .collect();
    json!({
        "id": r.id, "stage": r.stage.label(),
        "meet": { "name": r.meet.name, "pos": v2(r.meet.centre), "lot": key_str(r.meet.lot) },
        "finish": { "name": r.finish.name, "pos": v2(r.finish.centre), "lot": key_str(r.finish.lot) },
        "started": tick_time(r.started), "go": r.go_at.map(tick_time), "ends": r.ends_at.map(tick_time),
        "route_m": round(r.route_m), "racers": racers, "finished": r.finished.len(),
        "crowd": r.crowd.len(), "incident": r.incident, "arrests": r.arrests, "fined": r.fined, "fines": round(r.fines), "chase_incident": r.chase,
    })
}

fn pull_json(p: &nd_sim::street_events::RavePull) -> Value {
    json!({ "youth": p.youth, "artists": p.artists, "art_scene": p.art_scene, "neighbours": p.neighbours, "culture": round(p.culture()), "has_scene": p.has_scene(), "night_chance": round(p.night_chance()), "why": p.why() })
}

fn rave_json(w: &World, r: &Rave) -> Value {
    json!({
        "id": r.id, "building": key_str(r.building), "place": r.place, "pos": v2(r.centre), "door": v2(r.door),
        "started": tick_time(r.started), "ends": tick_time(r.ends_at),
        "promoter": r.promoter.map(key_str), "promoter_name": r.promoter.and_then(|p| w.citizens.get(p)).map(|c| c.name.as_str()),
        "guests": r.guests.len(), "queue": r.queue.len(),
        "queue_at_door": r.queue.iter().filter(|&&c| w.citizens.get(c).is_some_and(|c| c.pos.distance(r.door) < 30.0)).count(),
        "inside": r.inside.len(), "peak": r.peak, "complaints": r.complaints, "incident": r.incident, "pull": pull_json(&r.pull),
    })
}

fn spot_json(s: &RaveSpot) -> Value {
    json!({ "building": key_str(s.building), "name": s.name, "pos": v2(s.pos), "pull": pull_json(&s.pull), "last_rave_day": s.last_rave_day.map(|d| d + 1) })
}

/// `street_events`: tonight's race and rave, the plans, the warehouses
/// weighed for a rave (`fresh: true` weighs them now), the record and the
/// news.
pub fn command(cmd: &Value, w: &World) -> Result<Value, String> {
    let e = &w.street_events;
    let limit = opt_u64(cmd, "limit")?.unwrap_or(10) as usize;
    let spots: Vec<Value> = if field(cmd, "fresh").and_then(|v| v.as_bool()).unwrap_or(false) { w.rave_spots().iter().take(12).map(spot_json).collect() } else { e.rave_spots.iter().map(spot_json).collect() };
    Ok(json!({
        "race": e.race.as_ref().map(|r| race_json(w, r)),
        "race_planned": e.race_planned.map(tick_time),
        "race_crew": w.race_crew().len(),
        "rave": e.rave.as_ref().map(|r| rave_json(w, r)),
        "rave_planned": e.rave_planned.map(|(b, t)| json!({ "building": key_str(b), "name": w.buildings.get(b).map(|b| b.name.as_str()), "doors": tick_time(t) })),
        "rave_spots": spots,
        "history": e.history.iter().rev().take(limit).map(record_row).collect::<Vec<_>>(),
        "news": e.news.iter().rev().take(limit).map(news_row).collect::<Vec<_>>(),
        "totals": { "races": e.races_total, "raves": e.raves_total, "busts": e.busts_total, "fines": round(e.fines_total) },
        "note": "races roll at 23:00 (35% of nights with 3+ free locals owning fast cars), raves at 22:00 for 23:00 at the warehouse with the strongest scene (culture >= 1: young night owls / 30 + artists / 6 within 900 m)",
    }))
}

/// `race {at?}`: start a street race now, meeting at the lot nearest `at`.
pub fn race(cmd: &Value, w: &mut World) -> Result<Value, String> {
    let at = opt_vec2(cmd, "at")?;
    let id = w.start_race(at)?;
    Ok(json!({ "race": id, "detail": w.race().map(|r| race_json(w, r)) }))
}

/// `rave {building? | at?}`: open a rave now at that warehouse (or the one
/// nearest `at`, or the one with the strongest scene).
pub fn rave(cmd: &Value, w: &mut World) -> Result<Value, String> {
    let b: BuildingId = match (field(cmd, "building"), opt_vec2(cmd, "at")?) {
        (Some(v), _) => key_from(v)?,
        (None, Some(p)) => w.rave_spots().into_iter().min_by(|a, b| a.pos.distance(p).total_cmp(&b.pos.distance(p))).map(|s| s.building).ok_or("no old warehouse (cheap industrial building) in town")?,
        (None, None) => w.rave_spots().first().map(|s| s.building).ok_or("no old warehouse (cheap industrial building) in town")?,
    };
    let id = w.start_rave(b)?;
    Ok(json!({ "rave": id, "detail": w.rave().map(|r| rave_json(w, r)) }))
}

fn mid(w: &World, i: usize) -> nd_sim::Vec2 {
    let (a, b) = w.roads.segment_points(SegmentId(i as u32));
    (a + b) * 0.5
}

/// One piece's condition, for `inspect {segment}` and `roads`.
pub fn segment_condition(w: &World, sid: SegmentId) -> Value {
    let i = sid.0 as usize;
    if !wears(&w.roads, i) {
        return Value::Null;
    }
    let wear = w.road_wear.wear_of(i);
    let lv = w.land_value.at(mid(w, i));
    let queue = w.repair_queue();
    let place = queue.iter().position(|j| j.segment == sid);
    let rough = road_wear::roughness(wear);
    json!({
        "wear": round(wear), "condition": round(1.0 - wear), "label": road_wear::label(wear),
        "speed_factor": round(road_wear::slow_factor(rough)), "crash_factor": round(road_wear::crash_factor(rough)),
        "land_value": round(lv), "repair_cost": round(road_wear::repair_cost(lane_metres(&w.roads, i), wear)),
        "queue_place": place.map(|p| p + 1), "queue_length": queue.len(),
        "street": w.streets.of(sid).map(|s| s.name.as_str()),
    })
}

/// Network figures and the budget, for `roads` and `report.traffic`.
pub fn report_json(w: &World) -> Value {
    let rw = &w.road_wear;
    let (mut pieces, mut worn, mut crumbling, mut lane_m) = (0u32, 0u32, 0u32, 0.0f32);
    for i in 0..w.roads.segments.len() {
        if !wears(&w.roads, i) {
            continue;
        }
        pieces += 1;
        lane_m += lane_metres(&w.roads, i);
        let x = rw.wear_of(i);
        worn += u32::from(x >= road_wear::WORN);
        crumbling += u32::from(x >= road_wear::CRUMBLING);
    }
    let by = w.road_wear_by_land_value();
    let last = rw.days.last();
    let need = rw.need_per_day(&w.roads);
    json!({
        "pieces": pieces, "lane_km": round(lane_m / 1000.0), "mean_wear": round(rw.mean_wear(&w.roads)),
        "worn": worn, "crumbling": crumbling, "waiting_for_repair": w.repair_queue().len(),
        "mean_wear_by_land_value": { "cheap": round(by[0].0), "middle": round(by[1].0), "dear": round(by[2].0) },
        "budget": {
            "funding": round(rw.funding), "max_funding": road_wear::MAX_FUNDING, "need_per_day": round(need), "allowance_per_day": round(need * rw.funding),
            "spent_yesterday": last.map(|d| round(d.spent)), "repaired_yesterday": last.map(|d| d.repaired),
            "repaired_land_value_yesterday": last.map(|d| round(d.repaired_land_value)),
            "ledger_today": round(w.economy.ledger.today_of(nd_sim::economy::Flow::RoadMaintenance)),
            "ledger_yesterday": round(w.economy.ledger.yesterday_of(nd_sim::economy::Flow::RoadMaintenance)),
            "repaired_total": rw.repaired_total,
        },
        "days": rw.days.iter().rev().take(7).map(|d| json!({ "day": d.day + 1, "allowance": round(d.allowance), "spent": round(d.spent), "repaired": d.repaired, "repaired_lane_m": round(d.repaired_lane_m), "repaired_land_value": round(d.repaired_land_value), "waiting": d.waiting })).collect::<Vec<_>>(),
        "note": "crews resurface pieces worn past 0.3, dearest land first (priority = wear x (0.35 + 1.3 x land value)); worn past 0.35 cars drive up to 35% slower and misjudge up to 2.5x as often",
    })
}

/// `roads {funding?, near?, limit?}`: set the maintenance funding level (1 =
/// what the network needs, 0..1.5), then the network's condition, the
/// budget, and the worst pieces (or those nearest `near`).
pub fn roads(cmd: &Value, w: &mut World) -> Result<Value, String> {
    if let Some(f) = opt_f32(cmd, "funding")? {
        w.set_road_funding(f);
    }
    let limit = opt_u64(cmd, "limit")?.unwrap_or(10) as usize;
    let near = opt_vec2(cmd, "near")?;
    let mut ids: Vec<usize> = (0..w.roads.segments.len()).filter(|&i| wears(&w.roads, i)).collect();
    match near {
        Some(p) => ids.sort_by(|&a, &b| mid(w, a).distance(p).total_cmp(&mid(w, b).distance(p)).then(a.cmp(&b))),
        None => ids.sort_by(|&a, &b| w.road_wear.wear_of(b).total_cmp(&w.road_wear.wear_of(a)).then(a.cmp(&b))),
    }
    let rows: Vec<Value> = ids
        .into_iter()
        .take(limit)
        .map(|i| {
            let sid = SegmentId(i as u32);
            let mut v = segment_condition(w, sid);
            v["segment"] = json!(i);
            v["pos"] = v2(mid(w, i));
            v
        })
        .collect();
    let mut out = report_json(w);
    out["pieces_listed"] = json!(rows);
    Ok(out)
}
