//! [concerns] What people complain about, for the remote API: the
//! `report.people.concerns` block, the `concerns {days?, concern?}` command
//! and the "top concern" alert. All read from `World::concerns`.

use nd_sim::concerns::{Concern, MoodSample, Ranked, WEEK};
use nd_sim::{Tier, World};
use serde_json::{json, Value};

use super::report::Alert;
use super::view::{building_name, key_str, opt_str, opt_u64, round, v2};

/// What to do about each concern, for the alert's hint.
fn hint(c: Concern) -> &'static str {
    match c {
        Concern::Housing => "rent above half of income: zone residential on cheap land (low-tier homes) and add jobs that pay",
        Concern::Food => "people go hungry: food sellers need stock and staff near homes (economy.resources groceries, map layer=hunger)",
        Concern::Jobs => "no work or too skint: zone commercial and industry for jobs near homes",
        Concern::Safety => "crime and fires: police and fire coverage (Buildings > Services), jobs and housing stop people turning to crime",
        Concern::Trash => "overflowing bins: a trash depot with crews in range (society.trash)",
        Concern::Noise => "drones and street takeovers: keep pads and flight paths off homes, police coverage stops takeovers",
        Concern::Commute => "long trips and failed transit: more routes, buses, stops or roads where trips are slow",
        Concern::Parking => "drivers circle for spaces: parking lots near busy places, or transit so fewer drive",
        Concern::Power => "brownouts: build power plants (report services.power)",
        Concern::Services => "slow or no help: more police, fire and health sites, or closer ones (report services)",
        Concern::Nightlife => "places shut, packed or rough: more venues and shops, better ones (economy.venues)",
        Concern::Displacement => "homes demolished, expropriated or evicted: careful with road works through housing",
        Concern::Other => "see the quotes",
    }
}

fn mood_json(s: &MoodSample) -> Value {
    let class = |t: Tier| s.class(t).map(round);
    json!({
        "day": s.day() + 1, "tick": s.tick, "people": s.people, "mean": round(s.mean), "unhappy_share": round(s.unhappy), "happy_share": round(s.happy),
        "by_class": { "low": class(Tier::Low), "middle": class(Tier::Mid), "high": class(Tier::High) },
    })
}

fn ranked_json(w: &World, r: &Ranked, quotes: usize) -> Value {
    let today = w.clock.day();
    let k = &w.concerns;
    let spot = k.hotspot(r.concern, today, WEEK).map(|(at, n)| {
        let near = w.building_nearest(at);
        json!({ "at": v2(at), "complaints": n, "near": building_name(w, near), "building": near.map(key_str) })
    });
    json!({
        "concern": r.concern.key(), "label": r.concern.label(),
        "this_week": r.this_week, "last_week": r.last_week, "today": r.today, "all_time": r.total,
        "trend": round(r.trend()), "direction": match r.direction() { 1 => "rising", -1 => "falling", _ => "steady" },
        "mean_severity": round(r.severity), "where": spot,
        "quotes": k.quotes(r.concern).take(quotes).map(|q| json!({
            "citizen": key_str(q.citizen), "name": q.name, "text": q.text, "day": q.tick / 1440 + 1, "severity": round(q.severity),
            "building": q.building.map(key_str), "at": v2(q.pos),
        })).collect::<Vec<_>>(),
    })
}

/// The `report.people.concerns` block: top concerns this week with trend,
/// happiness now and over the last fortnight.
pub fn summary(w: &World) -> Value {
    let k = &w.concerns;
    let ranked = k.ranked(w.clock.day());
    let now = w.mood_now();
    json!({
        "note": "people complaining per concern (each counts once a day); trend is this week against last week",
        "top": ranked.iter().take(6).map(|r| ranked_json(w, r, 2)).collect::<Vec<_>>(),
        "happiness_now": mood_json(&now),
        "happiness_daily": k.mood_days.iter().rev().take(14).rev().map(mood_json).collect::<Vec<_>>(),
    })
}

/// `concerns {days?, concern?, quotes?}`: every ranked concern with its
/// daily counts over `days` (default 30), quotes, hot spot, and the
/// happiness series (daily over `days`, hourly over the last 48 h).
pub fn command(cmd: &Value, w: &World) -> Result<Value, String> {
    let days = opt_u64(cmd, "days")?.unwrap_or(30).max(1);
    let quotes = opt_u64(cmd, "quotes")?.unwrap_or(5) as usize;
    let only = match opt_str(cmd, "concern")? {
        Some(s) => Some(Concern::from_key(s).ok_or_else(|| format!("unknown concern {s:?}; one of {}", Concern::ALL.iter().map(|c| c.key()).collect::<Vec<_>>().join(", ")))?),
        None => None,
    };
    let k = &w.concerns;
    let today = w.clock.day();
    let from = today.saturating_sub(days - 1);
    let ranked: Vec<Value> = k
        .ranked(today)
        .iter()
        .filter(|r| only.is_none_or(|c| c == r.concern))
        .map(|r| {
            let mut v = ranked_json(w, r, quotes);
            v["daily"] = json!((from..=today).map(|d| k.count(r.concern, d, d)).collect::<Vec<_>>());
            v
        })
        .collect();
    Ok(json!({
        "today": today + 1, "days": days, "from_day": from + 1,
        "concerns": ranked,
        "categories": Concern::ALL.iter().map(|c| json!({ "key": c.key(), "label": c.label(), "about": c.about() })).collect::<Vec<_>>(),
        "happiness_now": mood_json(&w.mood_now()),
        "happiness_daily": k.mood_days.iter().filter(|s| s.day() >= from).map(mood_json).collect::<Vec<_>>(),
        "happiness_hourly": k.mood_hours.iter().map(mood_json).collect::<Vec<_>>(),
    }))
}

/// The city's top concern this week, if enough people share it: severity 2
/// when it is rising and a quarter of the city is unhappy.
pub fn alerts(w: &World) -> Vec<Alert> {
    let k = &w.concerns;
    let Some(top) = k.ranked(w.clock.day()).into_iter().find(|r| r.this_week >= 5 && r.concern != Concern::Other) else { return Vec::new() };
    let mood = w.mood_now();
    let severity = if top.direction() > 0 && mood.unhappy >= 0.25 { 2 } else { 1 };
    let mut a = Alert::new("top_concern", severity, hint(top.concern));
    for q in k.quotes(top.concern) {
        a.add(key_str(q.citizen), Some(q.pos));
    }
    a.count = top.this_week as usize;
    let near = k.hotspot(top.concern, w.clock.day(), WEEK).and_then(|(at, _)| w.building_nearest(at)).and_then(|b| w.buildings.get(b)).map(|b| format!(", most around {}", b.name)).unwrap_or_default();
    let trend = match top.direction() {
        1 => format!("up from {} last week", top.last_week),
        -1 => format!("down from {} last week", top.last_week),
        _ => "steady on last week".to_string(),
    };
    let quote = k.quotes(top.concern).next().map(|q| format!(" \"{}\" ({})", q.text, q.name)).unwrap_or_default();
    a.message = format!("top concern: {} ({} complaints this week, {trend}{near}).{quote}", top.concern.label().to_lowercase(), top.this_week);
    vec![a]
}
