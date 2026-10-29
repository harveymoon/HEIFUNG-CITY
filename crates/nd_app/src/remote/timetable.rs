//! [r4-timetable] Timetables and the day's timeline for the remote API:
//! `timetable {action}` lists every line's timetable, gets one line's with
//! its timeline (hour by hour, today and yesterday, and by part of the
//! day), sets one (hours, rushes, night service, each part's headway or
//! buses), applies a preset, and sets the city's default for new lines.
//! A line is `{"bus": id}`, `{"metro": id}`, `"rail"` or `{"ferry": id}`
//! (a terminal's building id), or `kind` and `id` side by side.

use nd_sim::timetable::{hm, parse_hm, Kind, LineRef, Part, Preset, Slot, Timeline, Timetable};
use nd_sim::World;
use serde_json::{json, Value};

use super::view::{field, key_from, key_str, opt_bool, opt_str, round};

fn line_key(l: LineRef) -> Value {
    match l {
        LineRef::Bus(r) => json!({ "bus": key_str(r) }),
        LineRef::Metro(m) => json!({ "metro": key_str(m) }),
        LineRef::Rail => json!("rail"),
        LineRef::Ferry(b) => json!({ "ferry": key_str(b) }),
    }
}

/// The line a command names.
pub fn line_arg(w: &World, cmd: &Value) -> Result<LineRef, String> {
    let line = match (field(cmd, "line"), opt_str(cmd, "kind")?) {
        (Some(Value::String(s)), _) if s == "rail" || s == "trains" => LineRef::Rail,
        (Some(Value::Object(o)), _) => {
            let (k, v) = o.iter().next().ok_or("line: {\"bus\": id} | {\"metro\": id} | \"rail\" | {\"ferry\": id}")?;
            match k.as_str() {
                "bus" | "route" => LineRef::Bus(key_from(v)?),
                "metro" | "subway" => LineRef::Metro(key_from(v)?),
                "ferry" | "terminal" => LineRef::Ferry(key_from(v)?),
                "rail" | "trains" => LineRef::Rail,
                other => return Err(format!("line kind {other:?}: bus | metro | rail | ferry")),
            }
        }
        (_, Some("rail" | "trains")) => LineRef::Rail,
        (_, Some(kind)) => {
            let id = field(cmd, "id").or_else(|| field(cmd, "route")).or_else(|| field(cmd, "line")).ok_or("missing id")?;
            match kind {
                "bus" => LineRef::Bus(key_from(id)?),
                "metro" | "subway" => LineRef::Metro(key_from(id)?),
                "ferry" => LineRef::Ferry(key_from(id)?),
                other => return Err(format!("kind {other:?}: bus | metro | rail | ferry")),
            }
        }
        _ => return Err("name a line: line {\"bus\": id} | {\"metro\": id} | \"rail\" | {\"ferry\": id}, or kind + id".into()),
    };
    w.line_schedule(line).map(|_| line).ok_or_else(|| "no such line".to_string())
}

fn timetable_json(kind: Kind, t: &Timetable, custom: bool) -> Value {
    let parts: Vec<Value> = Part::ALL
        .iter()
        .map(|&p| {
            let spans: Vec<Value> = t.segments().into_iter().filter(|s| s.2 == Some(p)).map(|(a, b, _)| json!([hm(a), hm(b)])).collect();
            json!({ "part": p.key(), "value": t.value(p), "text": kind.value_text(t.value(p)), "runs": !spans.is_empty(), "spans": spans })
        })
        .collect();
    json!({
        "kind": kind.key(), "custom": custom, "preset": if custom { t.preset(kind).map(|p| p.key()) } else { Some("standard") },
        "hours": t.hours_text(), "start": hm(t.start), "end": hm(t.end), "am_rush": [hm(t.am.0), hm(t.am.1)], "pm_rush": [hm(t.pm.0), hm(t.pm.1)], "night": t.night,
        "unit": if kind.per_bus() { "buses out" } else { "minutes between departures" },
        "parts": parts,
        if kind.per_bus() { "bus_minutes_per_day" } else { "departures_per_day" }: round(t.daily_work(kind)),
    })
}

fn slot_json(h: usize, s: &Slot) -> Value {
    json!({ "hour": h, "boarded": s.boarded, "mean_wait_min": s.mean_wait().map(round), "waiting": round(s.mean_waiting()), "riding": round(s.mean_riding()), "runs": s.runs, "missed": s.missed })
}

fn timeline_json(t: &Timetable, tl: &Timeline) -> Value {
    let day = |yesterday: bool| {
        let (b, w, missed, runs) = tl.totals(yesterday);
        let by = tl.by_part(t, yesterday);
        json!({
            "boarded": b, "mean_wait_min": w.map(round), "missed": missed, "runs": runs,
            "by_part": Part::ALL.iter().map(|p| json!({ "part": p.key(), "boarded": by[p.index()].0, "mean_wait_min": by[p.index()].1.map(round) })).collect::<Vec<_>>(),
            "hours": tl.day(yesterday).iter().enumerate().map(|(h, s)| slot_json(h, s)).collect::<Vec<_>>(),
        })
    };
    json!({ "today": day(false), "yesterday": day(true), "note": "per hour: boarded, the mean wait of those boarding (from reaching the stop; ferries untimed), riders waiting and on board (averaged over quarter-hour samples), runs (departures; for a bus line buses out summed over the samples) and riders left waiting after the last run" })
}

/// One line's timetable, with its timeline when `timeline` is set.
pub fn line_json(w: &World, l: LineRef, timeline: bool) -> Value {
    let Some((t, custom)) = w.line_schedule(l) else { return Value::Null };
    let mut v = json!({ "line": line_key(l), "name": w.line_name(l), "timetable": timetable_json(l.kind(), &t, custom), "now": t.part_at(w.clock.minute_of_day() as u32).map(|p| p.key()) });
    if timeline {
        if let Some(tl) = w.line_timeline(l) {
            v["timeline"] = timeline_json(&t, tl);
        }
    }
    v
}

/// The fields of `set`, onto the line's timetable now.
fn edited(cmd: &Value, kind: Kind, mut t: Timetable) -> Result<Timetable, String> {
    let time = |name: &str| -> Result<Option<u32>, String> {
        match field(cmd, name) {
            None => Ok(None),
            Some(Value::Number(n)) => n.as_u64().map(|m| Some(m as u32)).ok_or_else(|| format!("{name}: minutes of the day")),
            Some(Value::String(s)) => parse_hm(s).map(Some).ok_or_else(|| format!("{name}: \"HH:MM\" (24:00 for midnight)")),
            Some(_) => Err(format!("{name}: \"HH:MM\" or minutes of the day")),
        }
    };
    let pair = |name: &str| -> Result<Option<(u32, u32)>, String> {
        match field(cmd, name) {
            None => Ok(None),
            Some(Value::Array(a)) if a.len() == 2 => {
                let one = |v: &Value| match v {
                    Value::Number(n) => n.as_u64().map(|m| m as u32),
                    Value::String(s) => parse_hm(s),
                    _ => None,
                };
                Ok(Some((one(&a[0]).ok_or(format!("{name}: [from, to]"))?, one(&a[1]).ok_or(format!("{name}: [from, to]"))?)))
            }
            Some(_) => Err(format!("{name}: [\"HH:MM\", \"HH:MM\"]")),
        }
    };
    if let Some(m) = time("start")? {
        t.start = m;
    }
    if let Some(m) = time("end")? {
        t.end = m;
    }
    if let Some(p) = pair("am_rush")? {
        t.am = p;
    }
    if let Some(p) = pair("pm_rush")? {
        t.pm = p;
    }
    if let Some(n) = opt_bool(cmd, "night")? {
        t.night = n;
    }
    if let Some(v) = field(cmd, "values") {
        match v {
            Value::Object(o) => {
                for (k, x) in o {
                    let p = Part::from_key(k).ok_or_else(|| format!("values: unknown part {k:?} (morning_rush | day | evening_rush | evening | night)"))?;
                    t.values[p.index()] = x.as_u64().ok_or("values: whole numbers")? as u32;
                }
            }
            Value::Array(a) if a.len() == 5 => {
                for (i, x) in a.iter().enumerate() {
                    t.values[i] = x.as_u64().ok_or("values: whole numbers")? as u32;
                }
            }
            _ => return Err("values: {part: n} or [morning_rush, day, evening_rush, evening, night]".into()),
        }
    }
    if let (Some(p), Some(v)) = (opt_str(cmd, "part")?, field(cmd, "value")) {
        let p = Part::from_key(p).ok_or_else(|| format!("unknown part {p:?}"))?;
        t.values[p.index()] = v.as_u64().ok_or("value: a whole number")? as u32;
    }
    Ok(t.clamped(kind))
}

/// `timetable {action}`.
pub fn command(cmd: &Value, w: &mut World) -> Result<Value, String> {
    let action = opt_str(cmd, "action")?.unwrap_or("list");
    match action {
        "list" => Ok(json!({
            "default": w.transit.default_preset.map_or("standard", |p| p.key()),
            "presets": Preset::ALL.iter().map(|p| json!({ "key": p.key(), "label": p.label(), "what": p.blurb() })).collect::<Vec<_>>(),
            "lines": w.timetabled_lines().into_iter().map(|l| line_json(w, l, false)).collect::<Vec<_>>(),
        })),
        "get" | "timeline" => {
            let l = line_arg(w, cmd)?;
            Ok(line_json(w, l, true))
        }
        "set" => {
            let l = line_arg(w, cmd)?;
            if opt_bool(cmd, "standard")? == Some(true) {
                w.set_line_timetable(l, None)?;
            } else {
                let (now, _) = w.line_schedule(l).ok_or("no such line")?;
                let t = edited(cmd, l.kind(), now)?;
                w.set_line_timetable(l, Some(t))?;
            }
            Ok(line_json(w, l, false))
        }
        "preset" => {
            let l = line_arg(w, cmd)?;
            let name = opt_str(cmd, "preset")?.or(opt_str(cmd, "name")?).ok_or("preset: commuter | all_day | 24h | night_owl | standard")?;
            if name == "standard" {
                w.set_line_timetable(l, None)?;
            } else {
                let p = Preset::from_key(name).ok_or_else(|| format!("unknown preset {name:?}: commuter | all_day | 24h | night_owl | standard"))?;
                w.set_line_preset(l, p)?;
            }
            Ok(line_json(w, l, false))
        }
        "default" => {
            let name = opt_str(cmd, "preset")?.or(opt_str(cmd, "name")?).ok_or("preset: commuter | all_day | 24h | night_owl | standard")?;
            let p = if name == "standard" { None } else { Some(Preset::from_key(name).ok_or_else(|| format!("unknown preset {name:?}"))?) };
            let apply = opt_bool(cmd, "apply")?.unwrap_or(false);
            w.set_default_preset(p, apply)?;
            Ok(json!({ "default": p.map_or("standard", |p| p.key()), "applied_to_every_line": apply }))
        }
        other => Err(format!("timetable: unknown action {other:?} (list | get | timeline | set | preset | default)")),
    }
}
