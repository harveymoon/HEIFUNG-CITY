//! [subway] The metro for the remote API: the `subway` report section and
//! the `subway` command, which places stations, lays and edits lines and
//! reads ridership per line and per station.

use nd_sim::subway::{self as metro, MetroLineId};
use nd_sim::transit::TransitLeg;
use nd_sim::{Activity, BuildingId, Vec2 as SVec2, World};
use serde_json::{json, Value};

use super::view::{building_name, field, key_from, key_str, opt_str, opt_u64, req_point, req_points, round, v2};

/// One line: its stations in order, timetable, trains and riders.
pub fn line_row(w: &World, id: MetroLineId) -> Value {
    let Some(l) = w.subway.lines.get(id) else { return Value::Null };
    let trains = w.subway.trains.values().filter(|t| t.line == id).count();
    let waiting = w.citizens.values().filter(|c| matches!(c.activity, Activity::Waiting { .. }) && matches!(c.transit.map(|t| t.leg), Some(TransitLeg::Subway { line, .. }) if line == id)).count();
    let riding = w.subway.trains.iter().filter(|(_, t)| t.line == id).map(|(_, t)| t.run.passengers.len()).sum::<usize>();
    json!({
        "id": key_str(id), "name": l.name, "colour": l.color, "headway_min": l.headway,
        "length_m": round(l.length()), "stations": l.stations.iter().map(|&s| json!({ "id": key_str(s), "name": building_name(w, Some(s)) })).collect::<Vec<_>>(),
        "trains": trains, "waiting": waiting, "riding": riding,
        "riders_today": l.riders_today, "riders_yesterday": l.riders_yesterday, "runs_today": l.runs_today,
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
fn station_arg(w: &World, v: &Value) -> Result<BuildingId, String> {
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
            let id = w.add_metro_line(&stations)?;
            if let Some(m) = opt_u64(cmd, "headway")? {
                w.set_metro_headway(id, m as u32)?;
            }
            Ok(json!({ "line": line_row(w, id), "stations_placed": placed, "treasury": round(w.economy.treasury) }))
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
        other => Err(format!("unknown subway action {other:?}; one of status, station, line, headway, remove_line, inspect_line, inspect_station")),
    }
}
