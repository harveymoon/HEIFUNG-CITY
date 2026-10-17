//! [insight] The city's memory through the API: the history of a car,
//! citizen or building (`inspect {.., history: true}`), a trip replayed as
//! points (`replay`), and a read-only dump of anything in the game: one
//! entity of any kind with all its serialised fields (`get {kind, id}`), a
//! field of the world itself (`get {field, path}`), and a listing of a kind
//! with filters on any field (`query`).

use nd_sim::insight::{mode_key, unffi, wait_key, Ev, Rec, Trip, ACT_NAMES, ERRAND_NAMES};
use nd_sim::{BuildingId, CarId, CitizenId, SegmentId, Vec2 as SVec2, World};
use serde::ser::{self, Impossible, Serialize};
use serde_json::{json, Map, Value};

use super::ctx::Ctx;
use super::view::*;

/// A key's ffi bits as the API's `"idx:version"` (null for none).
fn ffi_str(v: u64) -> Value {
    if v == 0 { Value::Null } else { Value::String(format!("{}:{}", v & 0xffff_ffff, v >> 32)) }
}

fn building_of(w: &World, v: u64) -> Option<BuildingId> {
    unffi::<BuildingId>(v).filter(|b| w.buildings.contains_key(*b))
}

fn name_of_building(w: &World, v: u64) -> Value {
    building_of(w, v).map_or(Value::Null, |b| Value::String(w.buildings[b].name.clone()))
}

fn name_of_citizen(w: &World, v: u64) -> Value {
    unffi::<CitizenId>(v).and_then(|c| w.citizens.get(c)).map_or(Value::Null, |c| Value::String(c.name.clone()))
}

/// The street a recorded piece was, if the roads still have it there.
fn street_at(w: &World, segment: u32, at: SVec2) -> Value {
    let sid = SegmentId(segment);
    let Some(s) = w.roads.try_segment(sid).filter(|s| s.alive) else { return Value::Null };
    let (a, b) = w.roads.segment_points(sid);
    let ab = b - a;
    let t = ((at - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    if (a + ab * t).distance(at) > 20.0 {
        return Value::Null;
    }
    let _ = s;
    w.streets.of(sid).map_or(Value::Null, |st| Value::String(st.name.clone()))
}

/// One record as JSON: when, where, what.
pub fn record_json(w: &World, r: &Rec) -> Value {
    let mut v = tick_time(r.tick);
    v["at"] = json!([round(r.at.x), round(r.at.y)]);
    let mut put = |k: &str, x: Value| {
        v[k] = x;
    };
    match &r.ev {
        Ev::Start => put("event", json!("took_to_road")),
        Ev::Enter { segment, forward } => {
            put("event", json!("street"));
            put("segment", json!(segment));
            put("forward", json!(forward));
            put("street", street_at(w, *segment, r.at));
        }
        Ev::Replan { why, avoided, before_m, after_m } => {
            put("event", json!("replan"));
            put("why", json!(why.key()));
            put("reason", json!(why.text()));
            put("avoided", avoided.map_or(Value::Null, |a| json!([round(a.x), round(a.y)])));
            put("remaining_before_m", json!(before_m));
            put("remaining_after_m", after_m.map_or(Value::Null, |m| json!(m)));
            put("found_route", json!(after_m.is_some()));
        }
        Ev::UTurn { dead_end } => {
            put("event", json!("u_turn"));
            put("dead_end", json!(dead_end));
        }
        Ev::RageUTurn => put("event", json!("rage_u_turn")),
        Ev::Stall { seconds, cause, blocker } => {
            put("event", json!("stall"));
            put("seconds", json!(seconds));
            put("cause", json!(wait_key(*cause)));
            put("behind", opt_key(*blocker));
        }
        Ev::Anger { pct } => {
            put("event", json!("anger"));
            put("pct", json!(pct));
        }
        Ev::Crash => put("event", json!("crash")),
        Ev::Errand { what, tries, building } => {
            put("event", json!("errand"));
            put("errand", json!(ERRAND_NAMES.get(*what as usize).copied().unwrap_or("other")));
            if *tries > 0 {
                put("tries", json!(tries));
            }
            put("building", opt_key(*building));
            put("building_name", building_name(w, *building));
        }
        Ev::End(_) => put("event", json!("left_road")),
        Ev::Trip { mode, purpose, to, minutes, alt, failed, leg } => {
            put("event", json!(if *leg { "next_leg" } else { "set_off" }));
            put("mode", json!(mode_key(*mode)));
            put("purpose", json!(nd_sim::world::journeys::purpose_phrase(purpose)));
            put("to", opt_key(*to));
            put("to_name", building_name(w, *to));
            put("expected_min", json!(minutes));
            put("runner_up", alt.map_or(Value::Null, |(m, min)| json!({ "mode": mode_key(m), "expected_min": min })));
            let could_not: Vec<&str> = (0..8u8).filter(|m| failed & (1 << m) != 0).map(mode_key).collect();
            put("could_not", json!(could_not));
        }
        Ev::Act(a) => {
            put("event", json!("activity"));
            put("activity", json!(ACT_NAMES.get(a.tag as usize).copied().unwrap_or("unknown")));
            if a.tag == 5 {
                put("by", json!(if a.mode == 1 { "bike" } else { "foot" }));
            }
            match a.tag {
                1 | 2 | 3 | 5 | 7 | 14 => {
                    put("building", ffi_str(a.with));
                    put("building_name", name_of_building(w, a.with));
                }
                6 | 9 => put("car", ffi_str(a.with)),
                10 | 12 => put("train", ffi_str(a.with)),
                11 => put("drone", ffi_str(a.with)),
                13 => {
                    put("driver", ffi_str(a.with));
                    put("driver_name", name_of_citizen(w, a.with));
                }
                _ => {}
            }
        }
        Ev::NoRoute { to, why } => {
            put("event", json!("no_route"));
            put("to", opt_key(*to));
            put("to_name", building_name(w, *to));
            put("why", json!(why));
        }
        Ev::Life { what, other, text } => {
            put("event", json!(what.key()));
            put("other", ffi_str(*other));
            match what {
                nd_sim::insight::Life::Home | nd_sim::insight::Life::Job | nd_sim::insight::Life::Arrived => put("building_name", name_of_building(w, *other)),
                nd_sim::insight::Life::CrimeVictim | nd_sim::insight::Life::Suspect | nd_sim::insight::Life::Patient => put("other", json!(other)),
                _ => {}
            }
            if !text.is_empty() {
                put("text", json!(text));
            }
        }
        Ev::Happened { what, other, text } => {
            use nd_sim::insight::Happened as H;
            put("event", json!(what.key()));
            match what {
                H::MovedIn | H::MovedOut => put("households", json!(other)),
                H::Incident | H::Resolved => put("incident", json!(other)),
                H::Owner => {
                    put("owner", ffi_str(*other));
                    let owner = unffi::<nd_sim::estate::OwnerId>(*other).and_then(|o| w.estate.owners.get(o)).map(|o| o.name.clone());
                    put("owner_name", owner.map_or(json!("independent"), Value::String));
                }
                _ => {}
            }
            if !text.is_empty() {
                put("text", json!(text));
            }
        }
    }
    v
}

/// A vehicle's trip: who, what, a summary and every record.
pub fn trip_json(w: &World, t: &Trip, records: bool) -> Value {
    let recs = t.log.records();
    let mut replans: Map<String, Value> = Map::new();
    let (mut u_turns, mut dead_ends, mut rage, mut stalls, mut stall_s, mut max_anger, mut crashes, mut streets) = (0, 0, 0, 0, 0u32, 0u8, 0, 0);
    let mut causes: Map<String, Value> = Map::new();
    for r in &recs {
        match &r.ev {
            Ev::Replan { why, .. } => {
                let n = replans.get(why.key()).and_then(Value::as_u64).unwrap_or(0);
                replans.insert(why.key().into(), json!(n + 1));
            }
            Ev::UTurn { dead_end } => {
                u_turns += 1;
                dead_ends += *dead_end as u32;
            }
            Ev::RageUTurn => rage += 1,
            Ev::Stall { seconds, cause, .. } => {
                stalls += 1;
                stall_s += seconds;
                let k = wait_key(*cause);
                let n = causes.get(k).and_then(Value::as_u64).unwrap_or(0);
                causes.insert(k.into(), json!(n + *seconds as u64));
            }
            Ev::Anger { pct } => max_anger = max_anger.max(*pct),
            Ev::Crash => crashes += 1,
            Ev::Enter { .. } => streets += 1,
            _ => {}
        }
    }
    let end = t.end.unwrap_or(w.clock.tick);
    let mut v = json!({
        "car": key_str(t.car), "kind": t.kind.key(), "on_road": t.end.is_none(),
        "vehicle": opt_key(t.vehicle), "driver": opt_key(t.driver), "driver_name": t.driver.and_then(|d| w.citizens.get(d)).map(|c| c.name.clone()),
        "to": opt_key(t.to), "to_name": building_name(w, t.to),
        "set_off": tick_time(t.start), "ended": t.end.map_or(Value::Null, tick_time), "minutes_on_road": end.saturating_sub(t.start),
        "summary": {
            "street_pieces": streets, "replans": replans, "u_turns": u_turns, "dead_end_u_turns": dead_ends, "rage_u_turns": rage,
            "stalls": stalls, "stalled_min": 0.0, "stalled_by_cause_s": causes,
            "max_anger_pct": max_anger, "crashes": crashes, "records_dropped": t.log.dropped,
        },
    });
    // Traffic seconds to clock minutes: a tick is a minute of `traffic_seconds_per_tick` seconds.
    v["summary"]["stalled_min"] = json!(round(stall_s as f32 / w.config.traffic_seconds_per_tick.max(0.1)));
    if let Some(c) = w.traffic.cars.get(t.car).filter(|_| t.end.is_none()) {
        v["now"] = json!({ "at": [round(c.pos.x), round(c.pos.y)], "wait": wait_key(c.wait), "stalled_s": round(c.stalled), "anger_pct": (c.drive.anger * 100.0).round(), "route_left_m": round(w.traffic.net.route_length(&c.route[c.idx..], c.s)) });
    }
    if records {
        v["records"] = Value::Array(recs.iter().map(|r| record_json(w, r)).collect());
    }
    v
}

/// The activities that end a trip: being somewhere.
fn arrived(tag: u8) -> bool {
    matches!(tag, 1 | 2 | 3 | 4 | 7 | 14)
}

/// At home and at work name the citizen's own home and workplace (not kept
/// in each record).
fn own_places(w: &World, id: CitizenId, mut v: Value) -> Value {
    let c = w.citizens.get(id);
    let b = match v["activity"].as_str() {
        Some("at_home") => c.and_then(|c| c.home),
        Some("working") => c.and_then(|c| c.work),
        _ => return v,
    };
    if v["building"].is_null() {
        v["building"] = opt_key(b);
        v["building_name"] = building_name(w, b);
    }
    v
}

/// A citizen's history: trips grouped from their records (each set-off
/// with its legs, waits and where it ended), routes that failed, life
/// events and the last drive.
pub fn citizen_history(w: &World, id: CitizenId) -> Value {
    let Some(p) = w.insight.people.get(id) else { return json!({ "trips": [], "life": [], "note": "nothing recorded yet" }) };
    let recs = p.trips.records();
    let rec = |r: &Rec| own_places(w, id, record_json(w, r));
    let mut trips: Vec<Value> = Vec::new();
    let mut cur: Option<(u64, Value, Vec<Value>)> = None;
    fn close(cur: &mut Option<(u64, Value, Vec<Value>)>, trips: &mut Vec<Value>, end: Option<(u64, &str)>) {
        if let Some((start, mut t, legs)) = cur.take() {
            t["legs"] = Value::Array(legs);
            if let Some((tick, what)) = end {
                t["ended"] = json!(what);
                t["took_min"] = json!(tick.saturating_sub(start));
            } else {
                t["ended"] = Value::Null;
            }
            trips.push(t);
        }
    }
    for (k, r) in recs.iter().enumerate() {
        let next_tick = recs.get(k + 1).map_or(w.clock.tick, |n| n.tick);
        match &r.ev {
            Ev::Trip { leg: false, .. } => {
                close(&mut cur, &mut trips, None);
                cur = Some((r.tick, record_json(w, r), Vec::new()));
            }
            Ev::Act(a) => {
                let name = ACT_NAMES.get(a.tag as usize).copied().unwrap_or("unknown");
                if cur.is_some() && arrived(a.tag) {
                    close(&mut cur, &mut trips, Some((r.tick, name)));
                    continue;
                }
                match &mut cur {
                    Some((_, _, legs)) => {
                        let mut l = rec(r);
                        l["minutes"] = json!(next_tick.saturating_sub(r.tick));
                        if a.tag == 8 {
                            l["waited_min"] = json!(next_tick.saturating_sub(r.tick));
                        }
                        legs.push(l);
                    }
                    None => {}
                }
            }
            Ev::Trip { leg: true, .. } | Ev::NoRoute { .. } => {
                if let Some((_, _, legs)) = &mut cur {
                    legs.push(record_json(w, r));
                }
            }
            _ => {}
        }
    }
    close(&mut cur, &mut trips, None);
    let failed: Vec<Value> = recs.iter().filter(|r| matches!(r.ev, Ev::NoRoute { .. })).map(|r| record_json(w, r)).collect();
    let today = w.clock.tick / 1440;
    let trips_today = trips.iter().filter(|t| t["tick"].as_u64().is_some_and(|x| x / 1440 == today)).count();
    json!({
        "trips": trips, "trips_today": trips_today, "failed_routes": failed,
        "life": p.life.records().iter().map(|r| record_json(w, r)).collect::<Vec<_>>(),
        "last_drive": p.last_drive.as_deref().map_or(Value::Null, |t| trip_json(w, t, true)),
        "current_drive": match w.citizens.get(id).map(|c| &c.activity) {
            Some(nd_sim::Activity::Driving { car, .. }) => w.insight.live.get(*car).map_or(Value::Null, |t| trip_json(w, t, true)),
            _ => Value::Null,
        },
        "records": recs.iter().map(rec).collect::<Vec<_>>(),
        "records_dropped": p.trips.dropped + p.life.dropped,
    })
}

/// A building's history: every recorded event.
pub fn building_history(w: &World, id: BuildingId) -> Value {
    let Some(p) = w.insight.buildings.get(id) else { return json!({ "events": [] }) };
    json!({ "events": p.log.records().iter().map(|r| record_json(w, r)).collect::<Vec<_>>(), "records_dropped": p.log.dropped })
}

/// A car's history: its trip on the road (or its last).
pub fn car_history(w: &World, id: CarId) -> Value {
    w.insight.trip_of(id).map_or(json!({ "note": "nothing recorded for this car" }), |t| trip_json(w, t, true))
}

/// `replay {car | citizen, step_m}`: the trip's way as points with times.
pub fn replay(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let w = &ctx.sim.world;
    let trip = if let Some(v) = field(cmd, "car") {
        let id: CarId = key_from(v)?;
        w.insight.trip_of(id).ok_or("nothing recorded for this car")?
    } else if let Some(v) = field(cmd, "citizen") {
        let id: CitizenId = key_from(v)?;
        let c = w.citizens.get(id).ok_or("no such citizen")?;
        let live = match c.activity {
            nd_sim::Activity::Driving { car, .. } => w.insight.live.get(car),
            _ => None,
        };
        live.or_else(|| w.insight.people.get(id).and_then(|p| p.last_drive.as_deref())).ok_or("no drive recorded for this citizen")?
    } else {
        return Err("replay needs car or citizen".into());
    };
    let step = opt_f32(cmd, "step_m")?.unwrap_or(0.0);
    let mut pts = w.trip_replay(trip);
    if step > 0.0 {
        let mut kept: Vec<nd_sim::world::ReplayPoint> = Vec::new();
        let n = pts.len();
        for (k, p) in pts.drain(..).enumerate() {
            if k == 0 || k + 1 == n || kept.last().is_some_and(|q| q.at.distance(p.at) >= step) {
                kept.push(p);
            }
        }
        pts = kept;
    }
    let length: f32 = pts.windows(2).map(|p| p[0].at.distance(p[1].at)).sum();
    let events: Vec<Value> = trip.log.records().iter().filter(|r| !matches!(r.ev, Ev::Enter { .. })).map(|r| record_json(w, r)).collect();
    Ok(json!({
        "car": key_str(trip.car), "kind": trip.kind.key(), "on_road": trip.end.is_none(),
        "set_off": tick_time(trip.start), "length_m": round(length),
        "points": pts.iter().map(|p| json!([round(p.at.x), round(p.at.y), (p.tick * 100.0).round() / 100.0])).collect::<Vec<_>>(),
        "points_format": "[x, y, tick] (tick: game minutes since the start, fractional between street pieces)",
        "events": events,
    }))
}

// ── the generic dump ──

/// The kinds `get` and `query` know.
pub const KINDS: &[&str] = &[
    "citizen", "building", "lot", "zone", "household", "car", "vehicle", "train", "facility", "route", "unit", "incident", "drone", "bot", "metro", "metro_line", "owner", "cab", "ship", "tourist", "segment", "node",
];

/// Slotmap keys inside a serialised value (`{idx, version}`) as the API's
/// `"idx:version"` strings.
fn tidy(v: Value) -> Value {
    match v {
        Value::Object(m) => {
            if m.len() == 2 {
                if let (Some(i), Some(ver)) = (m.get("idx").and_then(Value::as_u64), m.get("version").and_then(Value::as_u64)) {
                    return Value::String(format!("{i}:{ver}"));
                }
            }
            Value::Object(m.into_iter().map(|(k, v)| (k, tidy(v))).collect())
        }
        Value::Array(a) => Value::Array(a.into_iter().map(tidy).collect()),
        x => x,
    }
}

fn to_json<T: Serialize>(x: &T) -> Value {
    tidy(serde_json::to_value(x).unwrap_or_else(|e| json!({ "error": e.to_string() })))
}

/// One entity's id and a way to serialise it.
type Entity<'a> = (String, Box<dyn Fn() -> Value + 'a>);

fn slots<'a, K: slotmap::Key, V: Serialize>(m: &'a slotmap::SlotMap<K, V>) -> Vec<Entity<'a>> {
    m.iter().map(|(k, v)| -> Entity<'a> { (key_str(k), Box::new(move || to_json(v))) }).collect()
}

/// Every entity of a kind.
fn entities<'a>(w: &'a World, kind: &str) -> Result<Vec<Entity<'a>>, String> {
    Ok(match kind {
        "citizen" | "citizens" => slots(&w.citizens),
        "building" | "buildings" => slots(&*w.buildings),
        "lot" | "lots" => slots(&*w.lots),
        "zone" | "zones" => slots(&w.zones),
        "household" | "households" => slots(&w.households),
        "car" | "cars" => slots(&w.traffic.cars),
        "vehicle" | "vehicles" => slots(&w.vehicles.cars),
        "train" | "trains" => slots(&w.rail.trains),
        "facility" | "facilities" => slots(&w.facilities),
        "route" | "routes" => slots(&w.transit.routes),
        "unit" | "units" => slots(&w.services.units),
        "drone" | "drones" => slots(&w.air.drones),
        "bot" | "bots" => slots(&w.bots.bots),
        // Metro trains are not saved (rebuilt from the lines): their Debug form.
        "metro" | "metros" => w.subway.trains.iter().map(|(k, v)| -> Entity<'a> { (key_str(k), Box::new(move || json!({ "debug": format!("{v:?}") }))) }).collect(),
        "metro_line" | "metro_lines" => slots(&w.subway.lines),
        "owner" | "owners" => slots(&w.estate.owners),
        "cab" | "cabs" => slots(&w.taxis.cabs),
        "ship" | "ships" => slots(&w.ports.ships),
        "tourist" | "tourists" => slots(&w.tourism.tourists),
        "incident" | "incidents" => w.services.incidents.iter().map(|i| -> Entity<'a> { (i.id.to_string(), Box::new(move || to_json(i))) }).collect(),
        "segment" | "segments" => w.roads.segments.iter().enumerate().filter(|(_, s)| s.alive).map(|(k, s)| -> Entity<'a> { (k.to_string(), Box::new(move || to_json(s))) }).collect(),
        "node" | "nodes" => w.roads.nodes.iter().enumerate().map(|(k, n)| -> Entity<'a> { (k.to_string(), Box::new(move || to_json(n))) }).collect(),
        _ => return Err(format!("unknown kind {kind:?}; kinds: {}", KINDS.join(", "))),
    })
}

/// The id a request gives: `"idx:version"` or a number.
fn id_text(v: &Value) -> Result<String, String> {
    match v {
        Value::String(s) => Ok(s.trim().to_string()),
        Value::Number(n) => Ok(n.to_string()),
        _ => Err("id must be an \"idx:version\" string or a number".into()),
    }
}

/// A dotted path into a value (`"business.stock.Groceries"`, `"route.3"`).
fn at_path<'v>(v: &'v Value, path: &str) -> Option<&'v Value> {
    path.split('.').filter(|s| !s.is_empty()).try_fold(v, |v, k| match v {
        Value::Object(m) => m.get(k),
        Value::Array(a) => k.parse::<usize>().ok().and_then(|i| a.get(i)),
        _ => None,
    })
}

/// Cap a reply's size: big answers ask for a narrower path.
fn sized(v: Value, cmd: &Value) -> Result<Value, String> {
    let max = opt_u64(cmd, "max_bytes")?.unwrap_or(4_000_000) as usize;
    let n = serde_json::to_string(&v).map_or(0, |s| s.len());
    if n > max {
        let keys: Vec<String> = match &v {
            Value::Object(m) => m.keys().take(80).cloned().collect(),
            Value::Array(a) => vec![format!("an array of {}", a.len())],
            _ => Vec::new(),
        };
        return Err(format!("{n} bytes is over max_bytes {max}: ask for a narrower path (here: {})", keys.join(", ")));
    }
    Ok(v)
}

/// `get {kind, id, path}`: one entity's fields; `get {field, path}`: a field
/// of the world (`field: "?"` lists them).
pub fn get(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let w = &ctx.sim.world;
    let path = opt_str(cmd, "path")?.unwrap_or("");
    if let Some(f) = opt_str(cmd, "field")? {
        let mut grab = FieldGrab { name: f, out: None, names: Vec::new() };
        let _ = w.serialize(&mut grab);
        if f == "?" || f.is_empty() {
            return Ok(json!({ "fields": grab.names }));
        }
        let v = tidy(grab.out.ok_or_else(|| format!("the world has no saved field {f:?}; fields: {}", grab.names.join(", ")))?);
        let v = at_path(&v, path).cloned().ok_or_else(|| format!("nothing at path {path:?}"))?;
        return Ok(json!({ "field": f, "path": path, "value": sized(v, cmd)? }));
    }
    let kind = req_str(cmd, "kind")?;
    let id = id_text(field(cmd, "id").ok_or("get needs id (with kind) or field")?)?;
    let row = entities(w, kind)?.into_iter().find(|(k, _)| *k == id).ok_or_else(|| format!("no {kind} {id}"))?;
    let v = (row.1)();
    let v = at_path(&v, path).cloned().ok_or_else(|| format!("nothing at path {path:?}"))?;
    Ok(json!({ "kind": kind, "id": id, "path": path, "value": sized(v, cmd)? }))
}

/// Does one filter hold: a number range `{min, max}`, a substring (strings),
/// or equal.
fn passes(v: Option<&Value>, want: &Value) -> bool {
    let Some(v) = v else { return want.is_null() };
    match want {
        Value::Object(r) if r.contains_key("min") || r.contains_key("max") => {
            let Some(x) = v.as_f64().or_else(|| v.as_array().map(|a| a.len() as f64)) else { return false };
            r.get("min").and_then(Value::as_f64).is_none_or(|m| x >= m) && r.get("max").and_then(Value::as_f64).is_none_or(|m| x <= m)
        }
        Value::String(s) => match v {
            Value::String(h) => h.to_ascii_lowercase().contains(&s.to_ascii_lowercase()),
            // An enum's variant name, say `{"Visiting": ...}`.
            Value::Object(m) => m.keys().any(|k| k.eq_ignore_ascii_case(s)),
            x => x.to_string().eq_ignore_ascii_case(s),
        },
        x => v == x,
    }
}

/// `query {kind, filter: {path: value | {min, max} | "substring"}, fields: [paths], offset, limit}`.
pub fn query(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let w = &ctx.sim.world;
    let kind = req_str(cmd, "kind")?;
    let filter: Vec<(String, Value)> = match field(cmd, "filter") {
        None => Vec::new(),
        Some(Value::Object(m)) => m.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        Some(_) => return Err("filter is an object of path: value".into()),
    };
    let fields: Option<Vec<String>> = match field(cmd, "fields") {
        None => None,
        Some(Value::Array(a)) => Some(a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()),
        Some(_) => return Err("fields is an array of paths".into()),
    };
    let offset = opt_u64(cmd, "offset")?.unwrap_or(0) as usize;
    let limit = (opt_u64(cmd, "limit")?.unwrap_or(50) as usize).min(2000);
    let all = entities(w, kind)?;
    let total = all.len();
    let mut matched = 0usize;
    let mut rows = Vec::new();
    for (id, make) in all {
        // With no filter only the page is serialised.
        if filter.is_empty() {
            matched += 1;
            if matched <= offset || rows.len() >= limit {
                continue;
            }
        }
        let v = make();
        if !filter.is_empty() {
            if !filter.iter().all(|(p, want)| passes(at_path(&v, p), want)) {
                continue;
            }
            matched += 1;
            if matched <= offset || rows.len() >= limit {
                continue;
            }
        }
        let row = match &fields {
            Some(fs) => {
                let mut m = Map::new();
                m.insert("id".into(), json!(id));
                for f in fs {
                    m.insert(f.clone(), at_path(&v, f).cloned().unwrap_or(Value::Null));
                }
                Value::Object(m)
            }
            None => json!({ "id": id, "value": v }),
        };
        rows.push(row);
    }
    sized(json!({ "kind": kind, "total": total, "matched": matched, "offset": offset, "rows": rows }), cmd)
}

// ── a serializer that keeps one field of the world ──

/// Serialises the `World` and keeps the field called `name` as JSON (and
/// the names of all of them): every saved field is reachable this way,
/// private ones too, without serialising the rest to JSON.
struct FieldGrab<'n> {
    name: &'n str,
    out: Option<Value>,
    names: Vec<&'static str>,
}

#[derive(Debug)]
struct GrabError(String);

impl std::fmt::Display for GrabError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GrabError {}

impl ser::Error for GrabError {
    fn custom<T: std::fmt::Display>(msg: T) -> Self {
        GrabError(msg.to_string())
    }
}

macro_rules! not_a_struct {
    ($($f:ident($($t:ty),*)),* $(,)?) => {
        $(fn $f(self, $(_: $t),*) -> Result<(), GrabError> { Err(GrabError("not a struct".into())) })*
    };
}

impl<'a, 'n> ser::Serializer for &'a mut FieldGrab<'n> {
    type Ok = ();
    type Error = GrabError;
    type SerializeSeq = Impossible<(), GrabError>;
    type SerializeTuple = Impossible<(), GrabError>;
    type SerializeTupleStruct = Impossible<(), GrabError>;
    type SerializeTupleVariant = Impossible<(), GrabError>;
    type SerializeMap = Impossible<(), GrabError>;
    type SerializeStruct = Self;
    type SerializeStructVariant = Impossible<(), GrabError>;

    not_a_struct!(serialize_bool(bool), serialize_i8(i8), serialize_i16(i16), serialize_i32(i32), serialize_i64(i64), serialize_u8(u8), serialize_u16(u16), serialize_u32(u32), serialize_u64(u64), serialize_f32(f32), serialize_f64(f64), serialize_char(char), serialize_str(&str), serialize_bytes(&[u8]), serialize_none(), serialize_unit(), serialize_unit_struct(&'static str), serialize_unit_variant(&'static str, u32, &'static str));

    fn serialize_some<T: ?Sized + Serialize>(self, _: &T) -> Result<(), GrabError> {
        Err(GrabError("not a struct".into()))
    }
    fn serialize_newtype_struct<T: ?Sized + Serialize>(self, _: &'static str, _: &T) -> Result<(), GrabError> {
        Err(GrabError("not a struct".into()))
    }
    fn serialize_newtype_variant<T: ?Sized + Serialize>(self, _: &'static str, _: u32, _: &'static str, _: &T) -> Result<(), GrabError> {
        Err(GrabError("not a struct".into()))
    }
    fn serialize_seq(self, _: Option<usize>) -> Result<Self::SerializeSeq, GrabError> {
        Err(GrabError("not a struct".into()))
    }
    fn serialize_tuple(self, _: usize) -> Result<Self::SerializeTuple, GrabError> {
        Err(GrabError("not a struct".into()))
    }
    fn serialize_tuple_struct(self, _: &'static str, _: usize) -> Result<Self::SerializeTupleStruct, GrabError> {
        Err(GrabError("not a struct".into()))
    }
    fn serialize_tuple_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> Result<Self::SerializeTupleVariant, GrabError> {
        Err(GrabError("not a struct".into()))
    }
    fn serialize_map(self, _: Option<usize>) -> Result<Self::SerializeMap, GrabError> {
        Err(GrabError("not a struct".into()))
    }
    fn serialize_struct(self, _: &'static str, _: usize) -> Result<Self, GrabError> {
        Ok(self)
    }
    fn serialize_struct_variant(self, _: &'static str, _: u32, _: &'static str, _: usize) -> Result<Self::SerializeStructVariant, GrabError> {
        Err(GrabError("not a struct".into()))
    }
}

impl<'a, 'n> ser::SerializeStruct for &'a mut FieldGrab<'n> {
    type Ok = ();
    type Error = GrabError;

    fn serialize_field<T: ?Sized + Serialize>(&mut self, key: &'static str, value: &T) -> Result<(), GrabError> {
        self.names.push(key);
        if key == self.name {
            self.out = Some(serde_json::to_value(value).unwrap_or_else(|e| json!({ "error": e.to_string() })));
        }
        Ok(())
    }

    fn end(self) -> Result<(), GrabError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_read_as_ids_and_paths_and_filters_find_things() {
        let v = tidy(json!({ "home": { "idx": 4, "version": 3 }, "list": [{ "idx": 1, "version": 1 }], "n": 5, "act": { "Visiting": { "idx": 2, "version": 1 } } }));
        assert_eq!(v["home"], json!("4:3"));
        assert_eq!(v["list"][0], json!("1:1"));
        assert_eq!(at_path(&v, "list.0"), Some(&json!("1:1")));
        assert!(passes(at_path(&v, "n"), &json!({ "min": 4, "max": 6 })));
        assert!(!passes(at_path(&v, "n"), &json!({ "min": 6 })));
        assert!(passes(at_path(&v, "act"), &json!("visiting")));
        assert!(passes(at_path(&v, "home"), &json!("4:3")));
    }

    #[test]
    fn the_field_grabber_keeps_one_field_of_the_world() {
        let w = World::new(nd_sim::WorldConfig::default());
        let mut grab = FieldGrab { name: "economy", out: None, names: Vec::new() };
        w.serialize(&mut grab).unwrap();
        assert!(grab.names.contains(&"economy") && grab.names.contains(&"insight") && grab.names.contains(&"citizens"));
        assert!(grab.out.unwrap().get("treasury").is_some());
        // Every kind lists.
        for k in KINDS {
            entities(&w, k).unwrap();
        }
    }
}
