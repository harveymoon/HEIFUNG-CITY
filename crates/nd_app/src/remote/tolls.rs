//! [toll-inspect] A toll booth in full, for `inspect {toll}` and a selected
//! booth: the `tolls` row plus lorries, free passes, drivers who went
//! round, the queue, the road it charges and its hourly chart.

use super::view::round;
use nd_sim::World;
use serde_json::{json, Value};

pub fn toll_detail(w: &World, id: u32) -> Option<Value> {
    let b = w.tolls.get(id)?;
    let mut v = super::taxes::booth_row(w, b);
    let (road, ways) = crate::toll_select::road(w, b);
    let (queued, back, wait) = w.toll_queue(id).unwrap_or_default();
    v["kind"] = json!("toll");
    v["label"] = json!(b.size.label());
    v["road"] = json!(road);
    v["charges"] = json!(ways.iter().map(|(d, n)| json!({ "heading": d, "lanes": n })).collect::<Vec<_>>());
    v["trucks_today"] = json!(b.trucks_today);
    v["trucks_yesterday"] = json!(b.trucks_yesterday);
    v["trucks_total"] = json!(b.trucks_total);
    v["free_today"] = json!(b.free_today);
    v["free_yesterday"] = json!(b.free_yesterday);
    v["free_total"] = json!(b.free_total);
    v["avoided_today"] = json!(b.avoided_today);
    v["avoided_yesterday"] = json!(b.avoided_yesterday);
    v["avoided_total"] = json!(b.avoided_total);
    v["net_per_day"] = json!(round(b.net_per_day()));
    v["queue"] = json!({ "cars": queued, "back_m": round(back), "wait_minutes": round(wait) });
    v["this_hour"] = hour_json(&nd_sim::tolls::TollHour { day: w.clock.day() as u32 + 1, hour: w.clock.hour() as u8, ..b.hour });
    v["hourly"] = json!(b.hourly.iter().map(hour_json).collect::<Vec<_>>());
    v["note"] = json!("cars_* count every vehicle through (lorries and free ones too); lorries pay double; buses, emergency and city vehicles go free; avoided: drivers whose route went round the booth that would have gone through it with no toll; hourly: the last 72 hours, oldest first");
    Some(v)
}

fn hour_json(h: &nd_sim::tolls::TollHour) -> Value {
    json!({ "day": h.day, "hour": h.hour, "cars": h.cars, "trucks": h.trucks, "free": h.free, "avoided": h.avoided, "revenue": round(h.revenue) })
}

/// A booth named by its id (a number or a numeric string), or `"nearest"`
/// with `at: [x, y]` is left to the caller: here only the id, checked.
pub fn toll_id(w: &World, v: &Value) -> Result<u32, String> {
    let id = v.as_u64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok())).ok_or("toll: give the booth id (an integer, as `tolls` lists it)")? as u32;
    w.tolls.get(id).map(|b| b.id).ok_or_else(|| format!("no toll booth {id}"))
}
