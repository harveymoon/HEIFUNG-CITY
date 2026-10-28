//! [districts] `district {action, ...}`: paint, name and set the policies
//! of districts, and read their figures. [tourism] `tourism {action?}`:
//! visitors, attractions, hotels and what tourism brings in.

use nd_sim::districts::{CommercialSpec, ResidentialSpec};
use nd_sim::industry::IndustryKind;
use nd_sim::stories::{Policy, Story};
use nd_sim::tourism::{Arrival, TouristState};
use nd_sim::world::PolicyChange;
use nd_sim::World;
use serde_json::{json, Value};

use super::view::*;
use crate::render::Overlays;
use crate::tools::ToolState;

/// The district a command names: `id` (number), or `district`/`name` (an
/// id or a name).
fn district_id(cmd: &Value, w: &World) -> Result<Option<u16>, String> {
    if let Some(n) = opt_u64(cmd, "id")? {
        return u16::try_from(n).ok().and_then(|n| w.districts.get(n)).map(|d| Some(d.id)).ok_or_else(|| format!("no district {n}"));
    }
    for key in ["district", "name"] {
        match field(cmd, key) {
            Some(Value::Number(n)) => {
                let n = n.as_u64().unwrap_or(0);
                return u16::try_from(n).ok().and_then(|n| w.districts.get(n)).map(|d| Some(d.id)).ok_or_else(|| format!("no district {n}"));
            }
            Some(Value::String(s)) if key == "district" => return w.districts.find(s).map(Some).ok_or_else(|| format!("no district {s:?}")),
            _ => {}
        }
    }
    Ok(None)
}

/// `"none"` / `null` clears, a key sets, absent leaves alone.
fn spec<T>(cmd: &Value, name: &str, parse: impl Fn(&str) -> Option<T>, all: &str) -> Result<Option<Option<T>>, String> {
    match field(cmd, name) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(s)) if s.eq_ignore_ascii_case("none") || s.eq_ignore_ascii_case("any") || s.is_empty() => Ok(Some(None)),
        Some(Value::String(s)) => parse(s).map(|x| Some(Some(x))).ok_or_else(|| format!("unknown {name} {s:?}; one of {all}, none")),
        Some(v) => Err(format!("{name} must be a string, got {v}")),
    }
}

fn industry_of(s: &str) -> Option<IndustryKind> {
    let s = s.to_ascii_lowercase();
    IndustryKind::ALL.into_iter().find(|k| {
        let key = match k {
            IndustryKind::Farming => "farming",
            IndustryKind::OilExtraction => "oil",
            IndustryKind::Refinery => "refinery",
            IndustryKind::Manufacturing => "manufacturing",
            IndustryKind::Logistics => "logistics",
            IndustryKind::Fishing => "fishing",
            IndustryKind::Tech => "tech",
            IndustryKind::Mining => "mining",
            IndustryKind::Smelting => "smelting",
            IndustryKind::FactoryFarming => "factory_farming",
        };
        s == key || k.label().eq_ignore_ascii_case(&s)
    })
}

pub fn district_json(w: &World) -> Value {
    let stats = w.district_stats();
    let rows: Vec<Value> = stats
        .iter()
        .map(|s| {
            let d = w.districts.get(s.id);
            json!({
                "id": s.id, "name": s.name, "policy": s.policy,
                "residential": d.and_then(|d| d.residential).map(|x| x.key()),
                "commercial": d.and_then(|d| d.commercial).map(|x| x.key()),
                "industry": d.and_then(|d| d.industry).map(|x| x.label()),
                "historic": d.is_some_and(|d| d.historic),
                "height_limit": d.and_then(|d| d.height_limit),
                "landmark": d.is_some_and(|d| d.is_landmark()),
                "story": d.and_then(|d| d.story).map(|s| s.key()),
                "story_label": d.map(|d| d.story_label()),
                "closest_story": d.filter(|d| d.is_custom()).and_then(Story::closest).map(|s| s.key()),
                "policies": d.map(|d| Policy::ALL.iter().map(|p| (p.key().to_string(), json!(round(d.policies.get(*p))))).collect::<serde_json::Map<_, _>>()),
                "policy_money_yesterday": d.map(|d| Policy::ALL.iter().filter(|p| d.books.yesterday[p.index()] != 0.0).map(|p| (p.key().to_string(), json!(round(d.books.yesterday[p.index()])))).collect::<serde_json::Map<_, _>>()),
                "policy_money_today": d.map(|d| Policy::ALL.iter().filter(|p| d.books.today[p.index()] != 0.0).map(|p| (p.key().to_string(), json!(round(d.books.today[p.index()])))).collect::<serde_json::Map<_, _>>()),
                "levied_yesterday": d.map(|d| round(d.books.levied_yesterday())),
                "spent_yesterday": d.map(|d| round(d.books.spent_yesterday())),
                "growth_pull": d.map(|d| nd_sim::LandUse::ALL.iter().filter(|lu| d.policies.pull(**lu) != 1.0).map(|lu| (lu.label().to_string(), json!(round(d.policies.pull(*lu))))).collect::<serde_json::Map<_, _>>()),
                "color": d.map(|d| d.color),
                "area_m2": round(s.area_m2), "centroid": s.centroid.map(round),
                "lots": s.lots, "vacant_lots": s.vacant_lots, "buildings": s.buildings, "under_construction": s.under_construction,
                "by_land_use": nd_sim::LandUse::ALL.iter().map(|lu| (lu.label().to_string(), json!(s.by_land_use[lu.index()]))).collect::<serde_json::Map<_, _>>(),
                "by_tier": { "low": s.by_tier[0], "mid": s.by_tier[1], "high": s.by_tier[2] },
                "in_look": s.in_look,
                "homes": s.homes, "residents": s.residents, "jobs": s.jobs, "workers": s.workers, "businesses_open": s.businesses_open,
                "hotels": s.hotels, "beds": s.beds, "tourists_staying": s.tourists_staying,
                "land_value_avg": round(s.land_value_avg), "rent_avg": round(s.rent_avg), "attraction": round(s.attraction),
            })
        })
        .collect();
    json!({ "districts": rows, "count": rows.len() })
}

pub fn district(cmd: &Value, w: &mut World, overlays: &mut Overlays, tools: &mut ToolState) -> Result<Value, String> {
    let action = opt_str(cmd, "action")?.unwrap_or("list");
    let mut out = match action {
        "list" | "stats" => district_json(w),
        "create" | "new" => {
            let id = w.create_district(opt_str(cmd, "name")?.map(str::to_string))?;
            json!({ "id": id, "name": w.districts.get(id).map(|d| d.name.clone()) })
        }
        "paint" | "erase" => {
            let erase = action == "erase" || opt_bool(cmd, "erase")?.unwrap_or(false);
            let id = if erase {
                0
            } else {
                match district_id(cmd, w)? {
                    Some(id) => id,
                    // Painting with no district starts one, as the tool does.
                    None => w.create_district(opt_str(cmd, "new_name")?.map(str::to_string))?,
                }
            };
            let cells = if let Some(poly) = field(cmd, "polygon").map(|_| req_points(cmd, "polygon")).transpose()? {
                w.paint_district_polygon(&poly, id)?
            } else if let Some(rect) = field(cmd, "rect").map(|_| req_points(cmd, "rect")).transpose()? {
                let [a, b] = rect[..] else { return Err("rect is [[x0, y0], [x1, y1]]".into()) };
                let poly = [nd_sim::Vec2::new(a.x, a.y), nd_sim::Vec2::new(b.x, a.y), nd_sim::Vec2::new(b.x, b.y), nd_sim::Vec2::new(a.x, b.y)];
                w.paint_district_polygon(&poly, id)?
            } else {
                let at = opt_vec2(cmd, "at")?.map_or_else(|| req_point(cmd), Ok)?;
                let r = opt_f32(cmd, "radius")?.unwrap_or(40.0);
                w.paint_district_circle(at, r, id)?
            };
            overlays.districts = true;
            json!({ "id": if id == 0 { Value::Null } else { json!(id) }, "cells_changed": cells, "erased": erase })
        }
        "stories" => {
            let rows: Vec<Value> = Story::ALL.iter().map(|s| json!({ "key": s.key(), "label": s.label(), "backstory": s.backstory(), "sets": s.sets(), "planned_look": s.planned_look() })).collect();
            json!({ "stories": rows, "policies": Policy::ALL.iter().map(|p| json!({ "key": p.key(), "label": p.label(), "range": p.range(), "help": p.help() })).collect::<Vec<_>>() })
        }
        "set" | "policy" => {
            let id = district_id(cmd, w)?.ok_or("missing id (or district name)")?;
            // [stories] `story`: a key or label, "none" for no story.
            let story = match field(cmd, "story") {
                None => None,
                Some(Value::Null) => Some(None),
                Some(Value::String(s)) if s.eq_ignore_ascii_case("none") || s.is_empty() => Some(None),
                Some(Value::String(s)) => Some(Some(Story::parse(s).ok_or_else(|| format!("unknown story {s:?}; one of {}, none", Story::ALL.iter().map(|s| s.key()).collect::<Vec<_>>().join(", ")))?)),
                Some(v) => return Err(format!("story must be a string, got {v}")),
            };
            let mut policies = Vec::new();
            for p in Policy::ALL {
                match field(cmd, p.key()) {
                    None => {}
                    Some(Value::Bool(b)) => policies.push((p, *b as u8 as f32)),
                    Some(v) => policies.push((p, v.as_f64().ok_or_else(|| format!("{} must be a number", p.key()))? as f32)),
                }
            }
            let change = PolicyChange {
                story,
                policies,
                residential: spec(cmd, "residential", ResidentialSpec::parse, "kowloon, suburban, eco, capsule, brick_steel, lantern_alley, garden, harbour_pastel, solarpunk_green")?,
                commercial: spec(cmd, "commercial", CommercialSpec::parse, "neon_strip, tech_campus, night_market, tourist, capsule, brick_steel, lantern_alley, garden, harbour_pastel, solarpunk_green")?,
                industry: spec(cmd, "industry", industry_of, "farming, oil, refinery, manufacturing, logistics, fishing, tech")?,
                historic: opt_bool(cmd, "historic")?,
                // [supertall] metres; null or 0 lifts it
                height_limit: match field(cmd, "height_limit") {
                    None => None,
                    Some(v) if v.is_null() => Some(None),
                    Some(v) => Some(v.as_f64().filter(|m| *m > 0.0).map(|m| m as f32)),
                },
                name: opt_str(cmd, "rename")?.or(if field(cmd, "id").is_some() { opt_str(cmd, "name")? } else { None }).map(str::to_string),
            };
            w.set_district_policy(id, change)?;
            let d = w.districts.get(id).unwrap();
            json!({ "id": id, "name": d.name, "policy": d.policy_label(), "story": d.story.map(|s| s.key()), "policies": d.policies.summary() })
        }
        "remove" | "delete" => {
            let id = district_id(cmd, w)?.ok_or("missing id (or district name)")?;
            json!({ "removed": w.remove_district(id) })
        }
        "at" => {
            let p = opt_vec2(cmd, "at")?.map_or_else(|| req_point(cmd), Ok)?;
            let d = w.districts.at(p);
            json!({ "district": d.map(|d| json!({ "id": d.id, "name": d.name, "policy": d.policy_label() })) })
        }
        "select" => {
            let id = district_id(cmd, w)?;
            tools.district.selected = id;
            tools.district.rename = id.and_then(|id| w.districts.get(id)).map(|d| d.name.clone()).unwrap_or_default();
            json!({ "selected": id })
        }
        "layer" => {
            overlays.districts = opt_bool(cmd, "on")?.unwrap_or(!overlays.districts);
            json!({})
        }
        other => return Err(format!("unknown district action {other:?}; one of list, stories, create, paint, erase, set, remove, at, select, layer")),
    };
    out["layer_on"] = json!(overlays.districts);
    Ok(out)
}

pub fn tourism_json(w: &World, limit: usize) -> Value {
    let t = &w.tourism;
    let s = &t.stats;
    let hotels: Vec<Value> = w
        .hotels()
        .into_iter()
        .map(|(id, beds, guests)| {
            let b = &w.buildings[id];
            json!({ "id": key_str(id), "name": b.name, "tier": b.tier.label(), "beds": beds, "guests": guests, "under_construction": b.construction.is_some(),
                "money": b.business.as_ref().map(|z| round(z.money)), "revenue_today": b.business.as_ref().map(|z| round(z.revenue_today)),
                "district": w.district_of_lot(b.lot).map(|d| d.name.clone()), "pos": w.building_lot(id).map(|l| points(&[l.center])[0].clone()) })
        })
        .collect();
    let mut attractions: Vec<&nd_sim::tourism::Attraction> = t.attractions.iter().collect();
    attractions.sort_by(|a, b| b.score.total_cmp(&a.score));
    let tourists: Vec<Value> = t
        .tourists
        .iter()
        .take(limit)
        .map(|(id, x)| {
            let target = match &x.state {
                TouristState::Walking { path, .. } => path.last().map(|p| json!([round(p.x), round(p.y)])),
                TouristState::Riding { to, .. } => Some(json!([round(to.x), round(to.y)])),
                _ => None,
            };
            json!({ "id": key_str(id), "pos": [round(x.pos.x), round(x.pos.y)], "activity": x.activity_label(), "visible": x.is_visible(), "class": x.class.label(), "age": x.age,
                "via": x.via.label(), "hotel": key_str(x.hotel), "money": round(x.money), "spent": round(x.spent), "sights": x.sights,
                "arrived_day": x.arrived_tick / 1440 + 1, "leaves_day": x.leave_day + 1, "going_to": target, "commuter_at": x.commuter.map(key_str) })
        })
        .collect();
    json!({
        "attractiveness": round(t.attractiveness),
        "demand_per_day": round(t.demand_per_day),
        "ways_in": Arrival::ALL.iter().map(|a| (a.label().to_string(), json!(t.ways_in[a.index()]))).collect::<serde_json::Map<_, _>>(),
        "tourists": t.count(),
        "ferry_commuters": t.commuters(),
        "visible": t.tourists.values().filter(|x| x.is_visible()).count(),
        "hotel_demand": round(t.hotel_demand),
        "today": {
            "arrivals": s.arrivals_today,
            "arrivals_by_mode": Arrival::ALL.iter().map(|a| (a.label().to_string(), json!(s.arrivals_by_mode_today[a.index()]))).collect::<serde_json::Map<_, _>>(),
            "departures": s.departures_today, "turned_away": s.turned_away_today, "nights": s.nights_today, "sights_visited": s.sights_today,
            "spend": round(s.spend_today()), "spend_hotels": round(s.spend_hotels_today), "spend_shops": round(s.spend_shops_today), "spend_venues": round(s.spend_venues_today),
            "tourist_tax": round(s.tax_today),
        },
        "yesterday": { "arrivals": s.arrivals_yesterday, "turned_away": s.turned_away_yesterday, "nights": s.nights_yesterday, "spend": round(s.spend_yesterday), "tourist_tax": round(s.tax_yesterday) },
        "totals": { "arrivals": s.arrivals_total, "spend": s.spend_total.round() },
        "ledger_tourist_tax": { "today": round(w.economy.ledger.today_of(nd_sim::economy::Flow::TouristTax)), "yesterday": round(w.economy.ledger.yesterday_of(nd_sim::economy::Flow::TouristTax)) },
        "hotels": hotels,
        "attractions": attractions.iter().take(30).map(|a| json!({ "kind": a.kind.label(), "name": a.name, "score": round(a.score), "pos": [round(a.pos.x), round(a.pos.y)], "building": a.building.map(key_str) })).collect::<Vec<_>>(),
        "tourist_list": tourists,
    })
}

pub fn tourism(cmd: &Value, w: &mut World) -> Result<Value, String> {
    if opt_str(cmd, "action")? == Some("refresh") {
        w.refresh_attractions();
    }
    let limit = opt_u64(cmd, "limit")?.unwrap_or(20) as usize;
    Ok(tourism_json(w, limit))
}
