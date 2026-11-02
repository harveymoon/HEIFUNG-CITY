//! [cable] Cable cars for the remote API: the `cable` report section and
//! the `cable` command, which previews and builds lines (stations placed
//! where the points ask), sets cabins, hours and fares, and reads riders,
//! waits and capacity per line and per station.

use nd_sim::cable::{CableKind, CableLineId, CablePreview, CableStationId};
use nd_sim::{Activity, CableStop, SiteWant, Vec2 as SVec2, World};
use serde_json::{json, Value};

use super::view::{field, key_from, key_str, opt_f32, opt_str, opt_u64, round, v2};

/// One line: its stations, pylons, cabins, riders, waits and capacity.
pub fn line_row(w: &World, id: CableLineId) -> Value {
    let Some(l) = w.cable.lines.get(id) else { return Value::Null };
    let secs = w.config.traffic_seconds_per_tick;
    let waiting: u32 = l.stations.iter().map(|s| w.cable_station_waiting(*s)).sum();
    let (boarded, mean_wait, _, _) = l.timeline.totals(true);
    let (boarded_today, wait_today, _, _) = l.timeline.totals(false);
    let tallest = l.pylons().map(|p| p.height()).fold(0.0, f32::max);
    json!({
        "id": key_str(id), "name": l.name, "kind": l.kind.key(), "colour": l.color,
        "length_m": round(l.length()), "stations": l.stations.iter().map(|&s| json!({ "id": key_str(s), "name": w.cable.stations.get(s).map(|x| x.name.clone()) })).collect::<Vec<_>>(),
        "pylons": l.pylons().count(), "tallest_pylon_m": round(tallest),
        "cabins": l.cabins, "seats_per_cabin": l.spec().seats, "spacing_m": round(l.spacing()),
        "interval_min": round(l.interval(secs)), "expected_wait_min": round(l.expected_wait(secs)), "ride_min": round(l.ride_minutes(0, l.stations.len().saturating_sub(1), secs)),
        "capacity_per_hour_each_way": round(l.capacity_per_hour(secs)),
        "open": nd_sim::timetable::hm(l.open), "close": nd_sim::timetable::hm(l.close), "open_now": l.open_at(w.clock.minute_of_day() as u32), "running": l.running,
        "fare": round(l.fare), "waiting": waiting, "riding": l.aboard.len(),
        "riders_today": l.riders_today, "riders_yesterday": l.riders_yesterday,
        "boarded_yesterday": boarded, "mean_wait_min_yesterday": mean_wait.map(round), "boarded_today": boarded_today, "mean_wait_min_today": wait_today.map(round),
        "capacity_used": round(l.capacity_used()), "left_behind_today": l.left_behind_today, "left_behind_yesterday": l.left_behind_yesterday,
        "sightseers_today": l.sightseers_today, "sightseers_yesterday": l.sightseers_yesterday,
        "scenic": round(l.scenic), "built_for": round(l.cost), "upkeep_per_day": round(l.upkeep_per_day()),
    })
}

/// One station: where, on what, the lines calling, boardings and the queue.
pub fn station_row(w: &World, id: CableStationId) -> Value {
    let Some(s) = w.cable.stations.get(id) else { return Value::Null };
    json!({
        "id": key_str(id), "name": s.name, "pos": v2(s.pos), "entrance": v2(s.entrance),
        "site": if s.roof { "roof" } else if s.building.is_some() { "lot" } else { "open_ground" },
        "building": s.building.map(key_str), "building_name": s.building.and_then(|b| w.buildings.get(b)).map(|b| b.name.clone()),
        "platform_m": round(s.floor), "ground_m": round(s.ground), "above_street_m": round(s.floor - s.ground),
        "lines": w.cable.lines_at(id).map(|(lid, l)| json!({ "id": key_str(lid), "name": l.name, "kind": l.kind.key() })).collect::<Vec<_>>(),
        "boardings_today": s.boardings_today, "boardings_yesterday": s.boardings_yesterday, "waiting": w.cable_station_waiting(id),
    })
}

/// The `cable` report section.
pub fn report(w: &World) -> Value {
    let s = &w.cable.stats;
    json!({
        "lines": s.lines, "stations": s.stations, "cabins": s.cabins, "pylons": s.pylons, "rope_m": round(s.rope_m),
        "waiting": s.waiting, "riding": s.riding,
        "riders_today": s.riders_today, "riders_yesterday": s.riders_yesterday,
        "fares_today": round(s.fares_today), "fares_yesterday": round(s.fares_yesterday), "upkeep_yesterday": round(s.upkeep_yesterday),
        "line_list": w.cable.lines.keys().map(|id| line_row(w, id)).collect::<Vec<_>>(),
        "station_list": w.cable.stations.keys().map(|id| station_row(w, id)).collect::<Vec<_>>(),
    })
}

/// A preview's figures (and the supports, when asked).
pub fn preview_json(p: &CablePreview, supports: bool) -> Value {
    let mut v = json!({
        "kind": p.kind.key(), "stations": p.stations, "new_stations": p.new_stations,
        "length_m": round(p.length_m), "pylons": p.pylons, "tallest_pylon_m": round(p.tallest_pylon_m),
        "highest_over_ground_m": round(p.max_above_ground_m), "over_water": round(p.over_water),
        "ride_min": round(p.ride_min), "cabins": p.cabins, "interval_min": round(p.interval_min), "capacity_per_hour_each_way": round(p.capacity_per_hour),
        "build_cost": round(p.build_cost), "upkeep_per_day": round(p.upkeep_per_day), "scenic": round(p.scenic),
        "ok": p.refusal.is_none(), "refusal": p.refusal,
    });
    if supports {
        v["supports"] = json!(p.supports.iter().map(|s| json!({ "pos": v2(s.pos), "s": round(s.s), "base": round(s.base), "top": round(s.top), "height": round(s.height()), "station": s.station })).collect::<Vec<_>>());
    }
    v
}

fn kind_arg(cmd: &Value) -> Result<CableKind, String> {
    match opt_str(cmd, "kind")? {
        None => Ok(CableKind::Gondola),
        Some(k) => CableKind::parse(k).ok_or_else(|| format!("unknown kind {k:?}: gondola or tramway")),
    }
}

fn want_of(v: Option<&Value>) -> Result<SiteWant, String> {
    match v.and_then(Value::as_str) {
        None => Ok(SiteWant::Auto),
        Some(s) => SiteWant::parse(s).ok_or_else(|| format!("unknown site {s:?}: auto, ground or roof")),
    }
}

/// The stops: `stations: [id | [x, y] | {at: [x, y], site}]` or
/// `points: [[x, y], ...]` (with `sites: ["auto"|"ground"|"roof", ...]`).
fn stops_arg(w: &World, cmd: &Value) -> Result<Vec<CableStop>, String> {
    let list = field(cmd, "stations").or_else(|| field(cmd, "points")).and_then(Value::as_array).ok_or("missing points: [[x, y], ...] (or stations: [id | [x, y], ...])")?;
    let sites = field(cmd, "sites").and_then(Value::as_array);
    let mut out = Vec::with_capacity(list.len());
    for (k, v) in list.iter().enumerate() {
        let want = want_of(sites.and_then(|s| s.get(k)))?;
        let stop = match v {
            Value::String(_) => {
                let id: CableStationId = key_from(v)?;
                if !w.cable.stations.contains_key(id) {
                    return Err(format!("no cable station {}", key_str(id)));
                }
                CableStop::Station(id)
            }
            Value::Array(a) if a.len() == 2 => CableStop::At(SVec2::new(a[0].as_f64().unwrap_or(0.0) as f32, a[1].as_f64().unwrap_or(0.0) as f32), want),
            Value::Object(_) => {
                let at = v.get("at").and_then(Value::as_array).filter(|a| a.len() == 2).ok_or("a stop object needs at: [x, y]")?;
                CableStop::At(SVec2::new(at[0].as_f64().unwrap_or(0.0) as f32, at[1].as_f64().unwrap_or(0.0) as f32), want_of(v.get("site"))?)
            }
            _ => return Err("a stop is a station id, [x, y] or {at: [x, y], site}".into()),
        };
        out.push(stop);
    }
    Ok(out)
}

fn line_arg(w: &World, cmd: &Value) -> Result<CableLineId, String> {
    let id: CableLineId = key_from(field(cmd, "line").ok_or("missing line id")?)?;
    w.cable.lines.contains_key(id).then_some(id).ok_or_else(|| "no such line".to_string())
}

/// A time "HH:MM" or minutes of the day.
fn time_arg(cmd: &Value, name: &str) -> Result<Option<u32>, String> {
    match field(cmd, name) {
        None => Ok(None),
        Some(Value::String(s)) => nd_sim::timetable::parse_hm(s).map(Some).ok_or_else(|| format!("{name}: \"HH:MM\"")),
        Some(v) => v.as_u64().map(|m| Some(m as u32)).ok_or_else(|| format!("{name}: \"HH:MM\" or minutes")),
    }
}

/// `cable {action}`: `status` (default), `site {pos, site}`, `preview
/// {kind, points|stations, sites}`, `build {kind, points|stations, sites,
/// cabins, fare, open, close}`, `cabins {line, n}`, `hours {line, open,
/// close}`, `fare {line, fare}`, `remove_line {line}`, `inspect_line
/// {line}`, `inspect_station {station}`.
pub fn command(cmd: &Value, w: &mut World) -> Result<Value, String> {
    let action = opt_str(cmd, "action")?.unwrap_or("status").to_ascii_lowercase();
    match action.as_str() {
        "status" | "list" => Ok(report(w)),
        "site" => {
            let p = super::view::req_point(cmd)?;
            let want = want_of(field(cmd, "site"))?;
            match w.cable_station_site(p, want) {
                Ok(s) => Ok(json!({ "ok": true, "pos": v2(s.pos), "roof": s.roof, "platform_m": round(s.floor), "ground_m": round(s.ground), "entrance": v2(s.entrance), "what": s.what })),
                Err(e) => Ok(json!({ "ok": false, "refusal": e })),
            }
        }
        "preview" => {
            let kind = kind_arg(cmd)?;
            let stops = stops_arg(w, cmd)?;
            let p = w.cable_line_preview(kind, &stops);
            Ok(json!({ "preview": preview_json(&p, true), "treasury": round(w.economy.treasury) }))
        }
        "build" | "line" | "add_line" => {
            let kind = kind_arg(cmd)?;
            let stops = stops_arg(w, cmd)?;
            let before = w.economy.treasury;
            let id = w.add_cable_line(kind, &stops)?;
            if let Some(n) = opt_u64(cmd, "cabins")? {
                w.set_cable_cabins(id, n as u32)?;
            }
            if let Some(f) = opt_f32(cmd, "fare")? {
                w.set_cable_fare(id, f)?;
            }
            let (o, c) = (time_arg(cmd, "open")?, time_arg(cmd, "close")?);
            if o.is_some() || c.is_some() {
                let l = &w.cable.lines[id];
                let (o, c) = (o.unwrap_or(l.open), c.unwrap_or(l.close));
                w.set_cable_hours(id, o, c)?;
            }
            Ok(json!({ "line": line_row(w, id), "paid": round(before - w.economy.treasury), "treasury": round(w.economy.treasury) }))
        }
        "cabins" => {
            let id = line_arg(w, cmd)?;
            let n = opt_u64(cmd, "n")?.or(opt_u64(cmd, "cabins")?).ok_or("missing n")?;
            let set = w.set_cable_cabins(id, n as u32)?;
            Ok(json!({ "cabins": set, "line": line_row(w, id) }))
        }
        "hours" => {
            let id = line_arg(w, cmd)?;
            let l = &w.cable.lines[id];
            let (o, c) = (time_arg(cmd, "open")?.unwrap_or(l.open), time_arg(cmd, "close")?.unwrap_or(l.close));
            w.set_cable_hours(id, o, c)?;
            Ok(json!({ "line": line_row(w, id) }))
        }
        "fare" => {
            let id = line_arg(w, cmd)?;
            let f = opt_f32(cmd, "fare")?.ok_or("missing fare")?;
            w.set_cable_fare(id, f)?;
            Ok(json!({ "line": line_row(w, id) }))
        }
        "remove_line" => {
            let id = line_arg(w, cmd)?;
            w.remove_cable_line(id);
            Ok(json!({ "removed": key_str(id), "lines": w.cable.lines.len(), "stations": w.cable.stations.len() }))
        }
        "inspect_line" => {
            let id = line_arg(w, cmd)?;
            let l = &w.cable.lines[id];
            let secs = w.config.traffic_seconds_per_tick;
            let mut row = line_row(w, id);
            row["supports"] = json!(l.supports.iter().map(|s| json!({ "pos": v2(s.pos), "height_m": round(s.height()), "rope_m": round(s.top), "station": s.station })).collect::<Vec<_>>());
            let hours: Vec<Value> = l.timeline.day(true).iter().enumerate().map(|(h, s)| json!({ "hour": h, "boarded": s.boarded, "mean_wait_min": if s.timed > 0 { Some(round(s.wait_min as f32 / s.timed as f32)) } else { None } })).filter(|v| v["boarded"].as_u64().unwrap_or(0) > 0).collect();
            row["yesterday_by_hour"] = json!(hours);
            row["cabins_detail"] = json!((0..l.cabins).step_by((l.cabins as usize / 24).max(1)).map(|k| { let p = l.cabin_pose(k, secs); json!({ "cabin": k, "at": v2(p.pos), "floor_m": round(p.z), "in_station": p.station, "riders": l.in_cabin(k) }) }).collect::<Vec<_>>());
            Ok(row)
        }
        "inspect_station" => {
            let v = field(cmd, "station").ok_or("missing station: id or [x, y]")?;
            let id = match v {
                Value::String(_) => key_from(v)?,
                Value::Array(a) if a.len() == 2 => w.cable_station_at(SVec2::new(a[0].as_f64().unwrap_or(0.0) as f32, a[1].as_f64().unwrap_or(0.0) as f32), 120.0).ok_or("no cable station within 120 m")?,
                _ => return Err("station: id or [x, y]".into()),
            };
            let mut row = station_row(w, id);
            row["riders_waiting_here"] = json!(w.citizens.values().filter(|c| matches!(c.activity, Activity::Waiting { .. }) && matches!(c.transit.map(|t| t.leg), Some(nd_sim::transit::TransitLeg::Cable { board, .. }) if board == id)).count());
            Ok(row)
        }
        other => Err(format!("unknown cable action {other:?}; one of status, site, preview, build, cabins, hours, fare, remove_line, inspect_line, inspect_station")),
    }
}
