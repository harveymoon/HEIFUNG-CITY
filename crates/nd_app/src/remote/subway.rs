//! [subway] The metro for the remote API: the `subway` report section and
//! the `subway` command, which places stations, lays and edits lines and
//! reads ridership per line and per station.

use nd_sim::subway::{self as metro, MetroLineId};
use nd_sim::transit::TransitLeg;
use nd_sim::{Activity, BuildingId, Vec2 as SVec2, World};
use serde_json::{json, Value};

use super::view::{building_name, field, key_from, key_str, opt_bool, opt_str, opt_u64, req_point, req_points, round, v2};

/// One line: its stations in order, timetable, trains and riders.
pub fn line_row(w: &World, id: MetroLineId) -> Value {
    let Some(l) = w.subway.lines.get(id) else { return Value::Null };
    let trains = w.subway.trains.values().filter(|t| t.line == id).count();
    let waiting = w.citizens.values().filter(|c| matches!(c.activity, Activity::Waiting { .. }) && matches!(c.transit.map(|t| t.leg), Some(TransitLeg::Subway { line, .. }) if line == id)).count();
    let riding = w.subway.trains.iter().filter(|(_, t)| t.line == id).map(|(_, t)| t.run.passengers.len()).sum::<usize>();
    json!({
        "id": key_str(id), "name": l.name, "colour": l.color, "headway_min": l.headway, "looped": l.looped,
        "length_m": round(l.length()), "stations": l.stations.iter().map(|&s| json!({ "id": key_str(s), "name": building_name(w, Some(s)) })).collect::<Vec<_>>(),
        "trains": trains, "waiting": waiting, "riding": riding,
        "riders_today": l.riders_today, "riders_yesterday": l.riders_yesterday, "runs_today": l.runs_today,
        // [r4-timetable] The headway now and the line's hours (`timetable` has the rest).
        "headway_now_min": l.headway_at(w.clock.minute_of_day() as u32), "hours": l.schedule().hours_text(),
        "timetable": if l.timetable.is_some() { l.schedule().preset(nd_sim::timetable::Kind::Metro).map_or("own", |p| p.key()) } else { "standard" },
    })
}

/// One station: where it is, the lines calling, boardings and the queue.
pub fn station_row(w: &World, id: BuildingId) -> Value {
    let Some(b) = w.buildings.get(id) else { return Value::Null };
    let (today, yesterday) = w.subway.boardings_at(id);
    let waiting = w.citizens.values().filter(|c| matches!(c.activity, Activity::Waiting { .. }) && matches!(c.transit.map(|t| t.leg), Some(TransitLeg::Subway { board, .. }) if board == id)).count();
    json!({
        "id": key_str(id), "name": b.name, "central": b.rail.is_some(),
        "entrance": w.building_entrance(id).map(v2), "platform": w.metro_platform(id).map(v2),
        "lines": w.subway.lines_at(id).map(|(lid, l)| json!({ "id": key_str(lid), "name": l.name })).collect::<Vec<_>>(),
        "boardings_today": today, "boardings_yesterday": yesterday, "waiting": waiting,
    })
}

/// The `subway` report section: totals, every line and every station.
pub fn report(w: &World) -> Value {
    let s = &w.subway.stats;
    json!({
        "stations": s.stations, "lines": s.lines, "trains": s.trains, "tunnel_m": round(s.tunnel_m),
        "waiting": s.waiting, "riding": s.riding,
        "riders_today": s.riders_today, "riders_yesterday": s.riders_yesterday,
        "fares_today": round(s.fares_today), "fares_yesterday": round(s.fares_yesterday), "upkeep_yesterday": round(s.upkeep_today),
        "runs_today": s.runs_today, "fare": metro::FARE,
        "in_service": metro::Subway::in_service(w.clock.minute_of_day() as u32),
        "line_list": w.subway.lines.keys().map(|id| line_row(w, id)).collect::<Vec<_>>(),
        "station_list": w.metro_stations().into_iter().map(|id| station_row(w, id)).collect::<Vec<_>>(),
    })
}

/// A station by id, or the one whose entrance or platform is nearest a point.
pub(super) fn station_arg(w: &World, v: &Value) -> Result<BuildingId, String> {
    if v.is_string() {
        let id: BuildingId = key_from(v)?;
        return w.metro_platform(id).map(|_| id).ok_or_else(|| format!("{} is not a metro station", key_str(id)));
    }
    let p = match v {
        Value::Array(a) if a.len() == 2 => SVec2::new(a[0].as_f64().unwrap_or(0.0) as f32, a[1].as_f64().unwrap_or(0.0) as f32),
        _ => return Err("a station is an id string or an [x, y] point".into()),
    };
    w.metro_stations()
        .into_iter()
        .filter_map(|s| w.metro_platform(s).map(|q| (s, q.distance(p))))
        .filter(|(_, d)| *d < 120.0)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(s, _)| s)
        .ok_or_else(|| format!("no metro station within 120 m of ({:.0}, {:.0})", p.x, p.y))
}

fn line_arg(w: &World, cmd: &Value) -> Result<MetroLineId, String> {
    let id: MetroLineId = key_from(field(cmd, "line").ok_or("missing line id")?)?;
    w.subway.lines.contains_key(id).then_some(id).ok_or_else(|| "no such line".to_string())
}

/// `subway {action}`: `status` (default), `station {x, y}`, `line
/// {stations: [id | [x, y], ...]}` or `line {points: [[x, y], ...]}`
/// (a station is placed at each point that has none within 120 m),
/// `headway {line, minutes}`, `remove_line {line}`, `inspect_line {line}`,
/// `inspect_station {station}`.
pub fn command(cmd: &Value, w: &mut World) -> Result<Value, String> {
    let action = opt_str(cmd, "action")?.unwrap_or("status").to_ascii_lowercase();
    match action.as_str() {
        "status" | "list" => Ok(report(w)),
        "station" => {
            let p = req_point(cmd)?;
            let lot = w.lots.iter().find(|(_, l)| nd_sim::zoning::point_in_polygon(p, &l.polygon)).map(|(id, _)| id).or_else(|| w.lots.iter().filter(|(_, l)| l.usable()).map(|(id, l)| (id, l.center.distance(p))).filter(|(_, d)| *d < 80.0).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(id, _)| id)).ok_or("no lot there: aim at a lot")?;
            let id = w.build_metro_station(lot)?;
            Ok(json!({ "station": station_row(w, id), "treasury": round(w.economy.treasury) }))
        }
        "line" | "add_line" => {
            let mut stations: Vec<BuildingId> = Vec::new();
            let mut placed: Vec<String> = Vec::new();
            if let Some(Value::Array(list)) = field(cmd, "stations") {
                for v in list {
                    stations.push(station_arg(w, v)?);
                }
            } else {
                for p in req_points(cmd, "points")? {
                    let found = station_arg(w, &json!([p.x, p.y]));
                    let id = match found {
                        Ok(id) => id,
                        Err(_) => {
                            let lot = w.lots.iter().find(|(_, l)| nd_sim::zoning::point_in_polygon(p, &l.polygon)).map(|(id, _)| id).ok_or_else(|| format!("no station or lot at ({:.0}, {:.0})", p.x, p.y))?;
                            let id = w.build_metro_station(lot)?;
                            placed.push(key_str(id));
                            id
                        }
                    };
                    stations.push(id);
                }
            }
            // [metro-ux] `loop: true` closes the line back on its first station.
            if opt_bool(cmd, "loop")?.unwrap_or(false) && stations.len() >= 3 && stations.first() != stations.last() {
                stations.push(stations[0]);
            }
            let id = w.add_metro_line(&stations)?;
            if let Some(m) = opt_u64(cmd, "headway")? {
                w.set_metro_headway(id, m as u32)?;
            }
            Ok(json!({ "line": line_row(w, id), "stations_placed": placed, "treasury": round(w.economy.treasury) }))
        }
        // [metro-ux] What a line would be, without laying it.
        "preview" => {
            let list = match field(cmd, "stations") {
                Some(Value::Array(list)) => list.clone(),
                _ => return Err("missing stations: [id or [x, y], ...]".into()),
            };
            let mut stations = list.iter().map(|v| station_arg(w, v)).collect::<Result<Vec<_>, _>>()?;
            if opt_bool(cmd, "loop")?.unwrap_or(false) && stations.len() >= 3 && stations.first() != stations.last() {
                stations.push(stations[0]);
            }
            let p = w.metro_line_preview(&stations)?;
            Ok(json!({ "preview": preview_json(&p), "affordable": p.tunnel_cost <= w.economy.treasury, "treasury": round(w.economy.treasury) }))
        }
        // [w1] Stop by stop, as bus lines: only new tunnel is paid for.
        "insert_stop" | "remove_stop" | "move_stop" | "set_loop" | "relay" => {
            let id = line_arg(w, cmd)?;
            let before = w.economy.treasury;
            let index = opt_u64(cmd, "index")?.map(|i| i as usize);
            let mut at = Value::Null;
            match action.as_str() {
                "insert_stop" => {
                    let s = station_arg(w, field(cmd, "station").ok_or("missing station: id or [x, y]")?)?;
                    at = json!(w.insert_metro_stop(id, index, s)?);
                }
                "remove_stop" => w.remove_metro_stop(id, index.ok_or("missing index")?)?,
                "move_stop" => {
                    let s = station_arg(w, field(cmd, "station").ok_or("missing station: id or [x, y]")?)?;
                    w.move_metro_stop(id, index.ok_or("missing index")?, s)?;
                }
                "set_loop" => w.set_metro_looped(id, opt_bool(cmd, "on")?.ok_or("missing on: bool")?)?,
                _ => {
                    let list = match field(cmd, "stations") {
                        Some(Value::Array(list)) => list.clone(),
                        _ => return Err("missing stations: [id or [x, y], ...]".into()),
                    };
                    let stations = list.iter().map(|v| station_arg(w, v)).collect::<Result<Vec<_>, _>>()?;
                    let looped = opt_bool(cmd, "loop")?.unwrap_or(w.subway.lines[id].looped);
                    w.relay_metro_line(id, &stations, looped)?;
                }
            }
            Ok(json!({ "line": line_row(w, id), "index": at, "paid": round(before - w.economy.treasury), "treasury": round(w.economy.treasury) }))
        }
        "headway" => {
            let id = line_arg(w, cmd)?;
            let m = opt_u64(cmd, "minutes")?.ok_or("missing minutes")?;
            let set = w.set_metro_headway(id, m as u32)?;
            Ok(json!({ "line": line_row(w, id), "headway_min": set }))
        }
        "remove_line" => {
            let id = line_arg(w, cmd)?;
            w.remove_metro_line(id);
            Ok(json!({ "removed": key_str(id), "lines": w.subway.lines.len() }))
        }
        "inspect_line" => {
            let id = line_arg(w, cmd)?;
            let l = &w.subway.lines[id];
            let mut row = line_row(w, id);
            row["path"] = json!(l.path.iter().step_by(4).map(|p| v2(*p)).collect::<Vec<_>>());
            row["trains_detail"] = json!(w.subway.trains.iter().filter(|(_, t)| t.line == id).map(|(tid, t)| json!({ "id": key_str(tid), "forward": t.forward, "at": v2(t.head()), "passengers": t.run.passengers.len(), "dwelling": t.run.dwell > 0.0, "next_stop": t.run.stops.get(t.run.next_stop).map(|s| building_name(w, Some(s.0))) })).collect::<Vec<_>>());
            Ok(row)
        }
        "inspect_station" => {
            let id = station_arg(w, field(cmd, "station").ok_or("missing station")?)?;
            Ok(station_row(w, id))
        }
        other => Err(format!("unknown subway action {other:?}; one of status, station, line, preview, headway, remove_line, inspect_line, inspect_station")),
    }
}

// ── [metro-ux] the line tool, as the player uses it ─────────────────────────

/// A line preview's figures.
pub fn preview_json(p: &metro::LinePreview) -> Value {
    json!({
        "stops": p.stops, "looped": p.looped, "length_m": round(p.length_m),
        "legs_m": p.legs_m.iter().map(|x| round(*x)).collect::<Vec<_>>(),
        "run_min": round(p.run_min), "round_trip_min": round(p.round_trip_min),
        "trains": p.trains, "headway_min": p.headway,
        "tunnel_cost": round(p.tunnel_cost), "upkeep_per_day": round(p.upkeep_per_day),
    })
}

/// The line being drawn in the game, as the panel shows it.
pub fn draft_json(w: &World, t: &crate::tools::ToolState) -> Value {
    let d = &t.metro_draft;
    json!({
        "stations": d.stations.iter().map(|&s| json!({ "id": key_str(s), "name": building_name(w, Some(s)) })).collect::<Vec<_>>(),
        "looped": d.looped, "ready": d.ready(),
        "preview": preview_json(&w.metro_draft_preview(d)),
        "hover": t.metro_hover.map(key_str),
        "tool_active": crate::metro_ux::tool_active(t),
        "stations_in_city": w.metro_stations().len(),
    })
}

/// `metro_line {stations?, click?, loop?, undo?, finish?, cancel?, headway?, tool?}`:
/// drive the metro line tool as the player does, so the game shows it (the
/// tool is picked, which switches to the Metro layer, unless `tool: false`).
/// In order: `cancel` drops the draft; each of `stations` (ids, or `[x, y]`
/// points that pick a station as a click there would) is clicked in;
/// `click` is one more; `loop` clicks the first station again; `undo` (true
/// or a count) takes stops back; `finish` lays the line (`headway` minutes).
/// A refused click does not fail the command (its row in `clicks` says
/// why); a failed `finish` does.
pub fn metro_line(cmd: &Value, w: &mut World, t: &mut crate::tools::ToolState) -> Result<Value, String> {
    use crate::metro_ux::{add_stop, finish_line, missed_hint, undo_stop, PICK_RADIUS};
    if opt_bool(cmd, "tool")?.unwrap_or(true) && !crate::metro_ux::tool_active(t) {
        crate::ui::build::activate(crate::ui::build::Item::MetroLines, t);
    }
    if opt_bool(cmd, "cancel")?.unwrap_or(false) {
        t.metro_draft.clear();
        t.status = "Line cancelled: nothing was built.".into();
    }
    let mut clicks: Vec<Value> = Vec::new();
    let mut targets: Vec<Value> = match field(cmd, "stations") {
        Some(Value::Array(list)) => list.clone(),
        Some(_) => return Err("stations is a list of ids or [x, y] points".into()),
        None => Vec::new(),
    };
    if let Some(c) = field(cmd, "click").or_else(|| field(cmd, "add")) {
        targets.push(c.clone());
    }
    for v in &targets {
        let station = match v {
            Value::String(_) => Some(station_arg(w, v)?),
            Value::Array(a) if a.len() == 2 => {
                let p = SVec2::new(a[0].as_f64().unwrap_or(0.0) as f32, a[1].as_f64().unwrap_or(0.0) as f32);
                w.metro_station_at(p, PICK_RADIUS)
            }
            _ => return Err("a station is an id string or an [x, y] point".into()),
        };
        let before = t.metro_draft.clone();
        let msg = match station {
            Some(s) => add_stop(w, &mut t.metro_draft, s),
            None => missed_hint(w),
        };
        clicks.push(json!({ "at": v, "station": station.map(key_str), "taken": t.metro_draft != before, "message": msg }));
        t.status = msg;
    }
    if opt_bool(cmd, "loop")?.unwrap_or(false) {
        let first = t.metro_draft.stations.first().copied().ok_or("no line to close: add stations first")?;
        let before = t.metro_draft.clone();
        let msg = add_stop(w, &mut t.metro_draft, first);
        clicks.push(json!({ "at": "loop", "station": key_str(first), "taken": t.metro_draft != before, "message": msg }));
        t.status = msg;
    }
    let undo = match field(cmd, "undo") {
        Some(Value::Bool(b)) => *b as u64,
        Some(v) => v.as_u64().ok_or("undo is true or a count")?,
        None => 0,
    };
    for _ in 0..undo {
        t.status = undo_stop(w, &mut t.metro_draft);
    }
    let mut line = Value::Null;
    if opt_bool(cmd, "finish")?.unwrap_or(false) {
        let before: Vec<MetroLineId> = w.subway.lines.keys().collect();
        let msg = finish_line(w, &mut t.metro_draft);
        t.status = msg.clone();
        let id = w.subway.lines.keys().find(|id| !before.contains(id)).ok_or(msg)?;
        if let Some(m) = opt_u64(cmd, "headway")? {
            w.set_metro_headway(id, m as u32)?;
        }
        line = line_row(w, id);
    }
    Ok(json!({ "draft": draft_json(w, t), "clicks": clicks, "status": t.status, "line": line, "treasury": round(w.economy.treasury) }))
}
