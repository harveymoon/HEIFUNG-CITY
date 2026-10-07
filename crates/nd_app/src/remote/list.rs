//! `list`: flat tables for bulk review (every business, building, citizen,
//! lot, route, plant or rail facility) with one consistent set of columns
//! per kind, sorting, filtering, column selection and paging.
//!
//! Why next to `find`: `find` is a search (near a point, a few filters, the
//! same rows `inspect` links from); `list` is a spreadsheet: numeric columns
//! that sort meaningfully (staffing, money, rent share), so a reader can ask
//! "the 20 least staffed food shops" in one call. Ids are the usual
//! `"idx:version"` strings, so rows go straight into `inspect`, `select` and
//! `camera {follow}`.

use nd_sim::transit::TransitLeg;
use nd_sim::{Activity, World};
use serde_json::{json, Map, Value};

use super::report::{building_pos, lu_key, route_length, UNPOWERED_BELOW};
use super::view::{key_str, opt_key, plant_name, rail_name, round, v2};

pub const WHATS: &[&str] = &["businesses", "buildings", "citizens", "lots", "routes", "plants", "facilities"];
pub const DEFAULT_LIMIT: usize = 50;
pub const MAX_LIMIT: usize = 1000;

fn r3(x: f32) -> f64 {
    (x as f64 * 1000.0).round() / 1000.0
}

/// Every row of one kind, unsorted and unfiltered.
pub fn rows(w: &World, what: &str) -> Result<Vec<Value>, String> {
    let pos = |b: &nd_sim::Building| building_pos(w, b).map_or(Value::Null, v2);
    Ok(match what {
        "businesses" => w
            .buildings
            .iter()
            .filter_map(|(id, b)| b.business.as_ref().map(|z| (id, b, z)))
            .map(|(id, b, z)| {
                let recipe = z.kind.recipe();
                let stock: Map<String, Value> = z.stock.iter().filter(|(_, v)| **v > 0.0).map(|(r, v)| (r.label().to_string(), json!(round(*v)))).collect();
                // Inputs under a quarter of their target hold production back.
                let short: Vec<&str> = recipe.inputs.iter().filter(|(r, _)| z.stock_of(*r) < 0.25 * z.target_input(*r, b.jobs)).map(|(r, _)| r.label()).collect();
                json!({
                    "id": key_str(id), "name": b.name, "kind": z.kind.label(), "land_use": lu_key(b.land_use), "tier": b.tier.label(),
                    "staffing": r3(b.staffing()), "workers": b.workers.len(), "jobs": b.jobs, "money": round(z.money),
                    "revenue_today": round(z.revenue_today), "spend_today": round(z.spend_today), "profit_today": round(z.revenue_today - z.spend_today),
                    "deals": z.deals, "broke_days": z.broke_days, "closed": z.closed, "under_construction": b.construction.is_some(),
                    "powered": round(b.powered), "derelict": b.is_derelict(), "stock": stock, "inputs_short": short, "pos": pos(b),
                })
            })
            .collect(),
        "buildings" => w
            .buildings
            .iter()
            .map(|(id, b)| {
                json!({
                    "id": key_str(id), "name": b.name, "land_use": lu_key(w.shown_use(id)), "tier": b.tier.label(), "floors": b.floors,
                    "homes": b.homes, "residents": b.residents.len(), "jobs": b.jobs, "workers": b.workers.len(), "staffing": r3(b.staffing()),
                    "rent": round(b.rent), "land_value": round(b.land_value), "condition": round(b.condition), "powered": round(b.powered),
                    "unpowered": b.powered < UNPOWERED_BELOW && nd_sim::power::demand_mw(b) > 0.0, "under_construction": b.construction.is_some(),
                    "business": b.business.as_ref().map(|z| z.kind.label()), "closed": b.business.as_ref().is_some_and(|z| z.closed),
                    "plant": b.plant.map(plant_name), "depot": b.depot, "rail": b.rail.as_ref().map(|r| rail_name(r.kind)),
                    "derelict": b.is_derelict(), "built_day": b.built_tick / 1440 + 1, "lot": key_str(b.lot), "pos": pos(b),
                })
            })
            .collect(),
        "citizens" => w
            .citizens
            .iter()
            .map(|(id, c)| {
                let rent = c.home.and_then(|h| w.buildings.get(h)).map_or(0.0, |b| b.rent);
                json!({
                    "id": key_str(id), "name": c.name, "class": c.class().label(), "activity": c.activity.label(),
                    "income": round(c.income), "savings": round(c.savings), "rent": round(rent),
                    "rent_share": r3(w.rent_share_of(id).filter(|s| s.is_finite()).unwrap_or(0.0)), "rent_stress_days": c.rent_stress_days,
                    "age": c.age, "household": c.household.map(key_str),
                    "mood": r3(c.mood), "satiety": r3(c.satiety), "hungry_days": c.hungry_days, "groceries": round(c.groceries),
                    "home": opt_key(c.home), "work": opt_key(c.work), "housed": c.home.is_some(), "employed": c.work.is_some(),
                    "owns_car": c.owns_car, "owns_bike": c.owns_bike, "arrived_day": c.arrived_tick / 1440 + 1, "pos": v2(c.pos),
                })
            })
            .collect(),
        "lots" => w
            .lots
            .iter()
            .map(|(id, l)| {
                json!({
                    "id": key_str(id), "land_use": lu_key(l.land_use), "area": round(l.area()), "land_value": round(w.land_value.at(l.center)),
                    "building": opt_key(l.building), "vacant": l.building.is_none() && l.usable(), "landlocked": l.landlocked,
                    "car_access": l.car_access(), "park": l.park.is_some(), "footfall_today": l.footfall_today, "pos": v2(l.center),
                })
            })
            .collect(),
        "routes" => {
            let mut waiting = std::collections::HashMap::new();
            for c in w.citizens.values() {
                if let (Activity::Waiting { .. }, Some(TransitLeg::Bus { route, .. })) = (&c.activity, c.transit.map(|t| t.leg)) {
                    *waiting.entry(route).or_insert(0u32) += 1;
                }
            }
            w.transit
                .routes
                .iter()
                .map(|(id, r)| {
                    json!({
                        "id": key_str(id), "name": r.name, "stops": r.stops.len(), "buses": r.buses, "fare": round(r.fare),
                        "riders_today": r.riders_today, "waiting": waiting.get(&id).copied().unwrap_or(0), "length_m": route_length(r).round(),
                        "broken_legs": r.broken.len(), "runnable": r.runnable(), "first_stop": r.stops.first().map(|s| v2(s.pos)),
                    })
                })
                .collect()
        }
        "plants" => w
            .buildings
            .iter()
            .filter_map(|(id, b)| b.plant.map(|k| (id, b, k)))
            .map(|(id, b, k)| {
                let avail = if b.is_derelict() { 0.0 } else { w.plant_capacity_mw(b) * w.plant_availability(b) };
                json!({
                    "id": key_str(id), "name": b.name, "kind": plant_name(k), "capacity_mw": round(w.plant_capacity_mw(b)), "available_mw": round(avail),
                    "upkeep_per_day": round(w.plant_upkeep(b)), "workers": b.workers.len(), "jobs": b.jobs, "derelict": b.is_derelict(), "pos": pos(b),
                })
            })
            .collect(),
        "facilities" => w
            .buildings
            .iter()
            .filter_map(|(id, b)| b.rail.as_ref().map(|l| (id, b, l)))
            .map(|(id, b, l)| {
                json!({
                    "id": key_str(id), "name": b.name, "kind": rail_name(l.kind), "rail_connected": l.rail_connected, "road_connected": l.road_connected,
                    "works": l.works() && !b.is_derelict(), "inbound_units": round(b.rail_inbound.iter().map(|c| c.amount).sum()),
                    "outbound_units": round(b.rail_outbound), "workers": b.workers.len(), "jobs": b.jobs, "derelict": b.is_derelict(), "pos": pos(b),
                })
            })
            .collect(),
        other => return Err(format!("unknown list {other:?}; one of {}", WHATS.join(", "))),
    })
}

/// Does `v` (a row's field) pass the filter `f`? Strings match as a
/// case-insensitive substring, `{min, max}` as a numeric range, anything
/// else by equality.
fn passes(v: &Value, f: &Value) -> bool {
    match f {
        Value::String(needle) => match v {
            Value::String(s) => s.to_ascii_lowercase().contains(&needle.to_ascii_lowercase()),
            Value::Null => needle.is_empty() || needle == "null",
            other => other.to_string().eq_ignore_ascii_case(needle),
        },
        Value::Object(range) => {
            let Some(x) = v.as_f64() else { return false };
            range.get("min").and_then(Value::as_f64).is_none_or(|m| x >= m) && range.get("max").and_then(Value::as_f64).is_none_or(|m| x <= m)
        }
        Value::Number(n) => v.as_f64().is_some_and(|x| Some(x) == n.as_f64()),
        other => v == other,
    }
}

fn compare(a: &Value, b: &Value) -> std::cmp::Ordering {
    use std::cmp::Ordering::*;
    match (a, b) {
        (Value::Null, Value::Null) => Equal,
        (Value::Null, _) => Greater,
        (_, Value::Null) => Less,
        _ => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => x.total_cmp(&y),
            _ => match (a.as_bool(), b.as_bool()) {
                (Some(x), Some(y)) => x.cmp(&y),
                _ => a.as_str().unwrap_or("").cmp(b.as_str().unwrap_or("")),
            },
        },
    }
}

pub struct ListArgs<'a> {
    pub what: &'a str,
    pub sort: Option<&'a str>,
    pub desc: bool,
    pub filter: Option<&'a Value>,
    pub fields: Option<Vec<String>>,
    pub offset: usize,
    pub limit: usize,
}

pub fn list(w: &World, a: &ListArgs) -> Result<Value, String> {
    let mut rows = rows(w, a.what)?;
    let columns: Vec<String> = rows.first().and_then(Value::as_object).map(|m| m.keys().cloned().collect()).unwrap_or_default();
    let check = |name: &str| -> Result<(), String> {
        if !columns.is_empty() && !columns.iter().any(|c| c == name) {
            return Err(format!("{} rows have no column {name:?}; columns: {}", a.what, columns.join(", ")));
        }
        Ok(())
    };
    if let Some(f) = a.filter {
        let f = f.as_object().ok_or("filter must be an object {column: value | {min, max}}")?;
        for k in f.keys() {
            check(k)?;
        }
        rows.retain(|r| f.iter().all(|(k, v)| passes(&r[k.as_str()], v)));
    }
    if let Some(s) = a.sort {
        check(s)?;
        // Nulls stay last either way.
        rows.sort_by(|x, y| {
            let (p, q) = (&x[s], &y[s]);
            if a.desc && !p.is_null() && !q.is_null() { compare(q, p) } else { compare(p, q) }
        });
    }
    if let Some(fields) = &a.fields {
        for f in fields {
            check(f)?;
        }
    }
    let total = rows.len();
    let page: Vec<Value> = rows
        .into_iter()
        .skip(a.offset)
        .take(a.limit)
        .map(|r| match &a.fields {
            // `id` is always kept so a row can be followed up.
            Some(fields) => Value::Object(r.as_object().map(|m| m.iter().filter(|(k, _)| *k == "id" || fields.iter().any(|f| f == *k)).map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default()),
            None => r,
        })
        .collect();
    Ok(json!({ "what": a.what, "total": total, "offset": a.offset, "returned": page.len(), "columns": columns, "rows": page }))
}
