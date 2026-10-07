//! [tax] [tolls] `taxes` and `tolls`: read and set the tax schedule and the
//! import duty, and list, price and remove toll booths. Placing a booth is
//! `place {kind: "toll_booth" | "toll_plaza"}` (`remote.rs`).

use nd_sim::tax::TaxCategory;
use nd_sim::tolls::TollBooth;
use nd_sim::{LandUse, World};
use serde_json::{json, Map, Value};

use super::view::*;

/// The whole schedule: every line with its rate, standard, pull, buildings
/// and take; totals; the duty.
pub fn schedule(w: &World) -> Value {
    let t = &w.economy.taxes;
    let rows = w.tax_breakdown();
    let lines: Vec<Value> = rows
        .iter()
        .map(|r| {
            let (cap, people) = if r.category.is_residential() { ("homes", "residents") } else { ("jobs", "workers") };
            let mut v = json!({
                "key": r.category.key(), "label": r.category.label(), "land_use": lu_name(r.category.land_use()),
                "rate": round(r.rate), "default": round(r.default_rate), "max": round(r.category.max_rate()), "pull": round(r.pull),
                "buildings": r.buildings,
                "today": round(r.today), "yesterday": round(r.yesterday), "recent_per_day": round(r.recent),
            });
            v[cap] = json!(r.capacity);
            v[people] = json!(r.people);
            v
        })
        .collect();
    let mut by_land_use = Map::new();
    for lu in [LandUse::Residential, LandUse::Commercial, LandUse::Entertainment, LandUse::Industrial] {
        let mine = rows.iter().filter(|r| r.category.land_use() == lu);
        let (b, today, yesterday): (u32, f32, f32) = mine.fold((0, 0.0, 0.0), |(b, x, y), r| (b + r.buildings, x + r.today, y + r.yesterday));
        by_land_use.insert(lu_name(lu).into(), json!({ "buildings": b, "today": round(today), "yesterday": round(yesterday), "pull": round(t.land_use_pull(lu)) }));
    }
    json!({
        "lines": lines,
        "by_land_use": by_land_use,
        "import_duty": round(t.import_duty), "import_duty_max": nd_sim::tax::MAX_IMPORT_DUTY,
        "duty_today": round(t.duty_today), "duty_yesterday": round(t.duty_yesterday),
        "tourist_tax": round(t.tourist_tax()), "tourist_tax_max": nd_sim::tax::MAX_TOURIST_TAX, "tourist_pull": round(t.tourist_pull()),
        "tourist_tax_today": round(w.tourism.stats.tax_today), "tourist_tax_yesterday": round(w.tourism.stats.tax_yesterday),
        "standard": t.is_standard(),
        "tax_today": round(w.economy.tax_today),
        "note": "rates are shares (0.12 = 12%); standard: 12% of incomes, 5% of business takings, no duty. pull < 1 means the line is taxed over the standard and grows less",
    })
}

fn lu_name(lu: LandUse) -> &'static str {
    match lu {
        LandUse::Residential => "residential",
        LandUse::Commercial => "commercial",
        LandUse::Industrial => "industrial",
        LandUse::Entertainment => "entertainment",
        LandUse::Civic => "civic",
        LandUse::Park => "park",
    }
}

/// A rate given as a share (0.2) or a percentage (20, or "20%").
fn rate_of(v: &Value) -> Result<f32, String> {
    let x = match v {
        Value::Number(n) => n.as_f64().ok_or("bad rate")? as f32,
        Value::String(s) => {
            let s = s.trim();
            let pct = s.ends_with('%');
            let x: f32 = s.trim_end_matches('%').trim().parse().map_err(|_| format!("bad rate {s:?}"))?;
            if pct {
                return Ok(x / 100.0);
            }
            x
        }
        _ => return Err("a rate is a number (0.2 or 20 for 20%)".into()),
    };
    Ok(if x > 1.0 { x / 100.0 } else { x })
}

/// `taxes {}` reads; `{set: {line: rate, ...}}` sets lines (a key, an
/// industry kind, or a land use name for its whole group: residential,
/// commercial, entertainment, industrial); `{category, rate}` sets one;
/// `{import_duty}` sets the duty; `{tourist_tax}` the tax on a hotel
/// night; `{reset: true}` goes back to standard.
pub fn command(cmd: &Value, w: &mut World) -> Result<Value, String> {
    let mut changed: Vec<Value> = Vec::new();
    if opt_bool(cmd, "reset")?.unwrap_or(false) {
        w.economy.taxes.reset();
        w.economy.recompute();
        changed.push(json!("reset to the standard rates"));
    }
    let mut set_one = |w: &mut World, key: &str, v: &Value| -> Result<(), String> {
        let rate = rate_of(v)?;
        let cats: Vec<TaxCategory> = match TaxCategory::from_key(key) {
            Some(c) => vec![c],
            None => match land_use_of(key) {
                Ok(lu) if !TaxCategory::group(lu).is_empty() => TaxCategory::group(lu),
                _ => return Err(format!("unknown tax line {key:?}; one of {} or a land use (residential, commercial, entertainment, industrial)", TaxCategory::ALL.iter().map(|c| c.key()).collect::<Vec<_>>().join(", "))),
            },
        };
        for c in cats {
            let set = w.set_tax_rate(c, rate);
            changed.push(json!({ "key": c.key(), "rate": round(set) }));
        }
        Ok(())
    };
    if let Some(key) = opt_str(cmd, "category")?.or(opt_str(cmd, "line")?) {
        let v = field(cmd, "rate").ok_or("missing rate")?;
        set_one(w, key, v)?;
    }
    if let Some(set) = field(cmd, "set") {
        let obj = set.as_object().ok_or("set is an object {line: rate}")?;
        for (k, v) in obj {
            set_one(w, k, v)?;
        }
    }
    if let Some(v) = field(cmd, "import_duty") {
        let set = w.set_import_duty(rate_of(v)?);
        changed.push(json!({ "import_duty": round(set) }));
    }
    // [clarity] The tourist tax on a hotel night.
    if let Some(v) = field(cmd, "tourist_tax") {
        let set = w.set_tourist_tax(rate_of(v)?);
        changed.push(json!({ "tourist_tax": round(set) }));
    }
    let mut out = schedule(w);
    out["changed"] = json!(changed);
    Ok(out)
}

pub fn booth_row(w: &World, b: &TollBooth) -> Value {
    let seg = b.segment.and_then(|s| w.roads.try_segment(s));
    json!({
        "id": b.id, "name": b.name, "size": b.size.key(), "pos": v2(b.pos), "price": round(b.price),
        "truck_price": round(b.price * nd_sim::tolls::TRUCK_FACTOR),
        "lanes": seg.map(|s| s.lanes()), "highway": seg.map(|s| s.is_highway()), "segment": b.segment.map(|s| s.0),
        "cars_today": b.cars_today, "revenue_today": round(b.revenue_today),
        "cars_yesterday": b.cars_yesterday, "revenue_yesterday": round(b.revenue_yesterday),
        "cars_total": b.cars_total, "revenue_total": round(b.revenue_total),
        "upkeep_per_day": b.size.upkeep_per_day(),
    })
}

pub fn tolls_summary(w: &World) -> Value {
    let t = &w.tolls;
    json!({
        "booths": t.booths.iter().map(|b| booth_row(w, b)).collect::<Vec<_>>(),
        "paid_today": t.paid_today, "free_today": t.free_today, "trucks_today": t.trucks_today,
        "revenue_today": round(t.revenue_today()), "upkeep_per_day": round(t.upkeep_per_day()),
        "ledger_today": round(w.economy.ledger.today_of(nd_sim::economy::Flow::Tolls)),
        "ledger_yesterday": round(w.economy.ledger.yesterday_of(nd_sim::economy::Flow::Tolls)),
        "note": "a toll counts as a detour of 120 m per unit of money when drivers plan routes; lorries pay double; buses and emergency vehicles go free",
    })
}

/// `tolls {}` lists; `{action: "price", id, price}`; `{action: "remove", id}`
/// (or `at: [x, y]` for the booth nearest the point within 30 m).
pub fn tolls(cmd: &Value, w: &mut World) -> Result<Value, String> {
    let action = opt_str(cmd, "action")?.unwrap_or("list").to_ascii_lowercase();
    let id = match opt_u64(cmd, "id")? {
        Some(id) => Some(id as u32),
        None => match opt_vec2(cmd, "at")? {
            Some(p) => Some(w.tolls.nearest(p, 30.0).map(|b| b.id).ok_or("no toll booth within 30 m")?),
            None => None,
        },
    };
    match action.as_str() {
        "list" | "" => {}
        "price" | "set_price" => {
            let id = id.ok_or("missing id (or at)")?;
            let price = opt_f32(cmd, "price")?.ok_or("missing price")?;
            w.set_toll_price(id, price)?;
        }
        "remove" | "delete" | "bulldoze" => {
            let id = id.ok_or("missing id (or at)")?;
            w.remove_toll(id)?;
        }
        "inspect" => {
            let id = id.ok_or("missing id (or at)")?;
            let b = w.tolls.get(id).ok_or("no such toll booth")?;
            return Ok(booth_row(w, b));
        }
        other => return Err(format!("unknown action {other:?}; list, price, remove or inspect")),
    }
    Ok(tolls_summary(w))
}
