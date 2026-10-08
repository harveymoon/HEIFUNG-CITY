//! [supertall] The skyscraper league over the remote API: `skyline` for
//! the tallest buildings, the league towers, the record book, the news, the
//! developers' money and the height limits; actions to set the city's limit
//! and the vertiport clearance, and dev actions to start a tower now and
//! finish the sites. `inspect {building}` carries a `tower` block.

use super::view::{field, key_from, key_str, opt_f32, round, v2, vec2_of};
use nd_sim::towers::{League, TowerDesign, MEGATALL_M, SKYSCRAPER_M, SUPERTALL_M};
use nd_sim::{BuildingId, TowerOrder, World};
use serde_json::{json, Value};

/// `inspect.building.tower`: how tall it stands and its place in the
/// skyline; for a league tower its class, form, developer, plots and dates.
pub fn building_tower(w: &World, id: BuildingId) -> Value {
    let Some(b) = w.buildings.get(id) else { return Value::Null };
    let rank = w.skyline_rank(id);
    let mut v = json!({
        "height_m": round(w.building_height(id)),
        "rank": rank,
        "rank_label": rank.map(|r| if r == 1 { "the city's tallest".to_string() } else { format!("{} tallest", nd_sim::towers::ordinal(r)) }),
        "height_limit_m": w.height_limit_at(b.lot),
    });
    if let Some(t) = &b.plot.tower {
        v["league"] = json!(t.league().label());
        v["design"] = json!(t.design.key());
        v["design_label"] = json!(t.design.label());
        v["developer"] = json!(w.owner_of(id).map(|(_, o)| o.name.clone()));
        v["lots"] = json!(b.plot.lots(b.lot).iter().map(|l| key_str(*l)).collect::<Vec<_>>());
        v["pending_lots"] = json!(b.plot.pending_lots.iter().map(|l| key_str(*l)).collect::<Vec<_>>());
        v["site_m2"] = json!(round(w.footprint(id).map_or(0.0, |l| l.area())));
        v["floors"] = json!(b.floors);
        v["started_day"] = json!(t.started_day);
        v["topped_out_day"] = json!(t.topped_out_day);
        v["record_bid"] = json!(t.record_bid);
        v["progress"] = json!(b.construction.as_ref().map(|c| round(c.progress)));
        v["days_left"] = json!(b.construction.as_ref().map(|c| round(c.days_left())));
    }
    v
}

fn tower_row(w: &World, id: BuildingId) -> Value {
    let b = &w.buildings[id];
    let t = b.plot.tower.unwrap();
    json!({
        "id": key_str(id), "name": b.name, "height_m": round(t.height), "league": t.league().label(), "design": t.design.key(),
        "developer": w.owner_of(id).map(|(_, o)| o.name.clone()), "lots": 1 + b.plot.extra_lots.len() + b.plot.pending_lots.len(),
        "pos": w.footprint(id).map(|l| v2(l.center)),
        "under_construction": b.construction.is_some(), "progress": b.construction.as_ref().map(|c| round(c.progress)),
        "rank": w.skyline_rank(id), "land_use": b.land_use.label(), "homes": b.planned_capacity().0, "jobs": b.planned_capacity().1,
    })
}

fn summary(w: &World, limit: usize) -> Value {
    let s = &w.skyline;
    let ranking: Vec<Value> = w
        .skyline_ranking()
        .into_iter()
        .take(limit)
        .enumerate()
        .map(|(i, (id, h))| {
            let b = &w.buildings[id];
            json!({ "rank": i + 1, "id": key_str(id), "name": b.name, "height_m": round(h), "league": b.plot.tower.map(|t| t.league().label()), "pos": w.footprint(id).map(|l| v2(l.center)) })
        })
        .collect();
    let classes = [League::Skyscraper, League::Supertall, League::Megatall].map(|l| json!({ "class": l.label(), "standing": w.buildings.values().filter(|b| b.construction.is_none() && b.plot.tower.is_some_and(|t| t.league() == l)).count(), "rising": w.buildings.values().filter(|b| b.construction.is_some() && b.plot.tower.is_some_and(|t| t.league() == l)).count() }));
    json!({
        "limit_m": s.limit,
        "vertiport_clearance": s.vertiport_clearance,
        "classes_m": { "skyscraper": SKYSCRAPER_M, "supertall": SUPERTALL_M, "megatall": MEGATALL_M },
        "record_m": round(s.record()),
        "tallest_planned_m": round(w.tallest_planned()),
        "population_cap_m": round(nd_sim::towers::height_for_population(w.citizens.len())),
        "demand": round(w.tower_demand()),
        "classes": classes,
        "towers": w.league_towers().into_iter().map(|id| tower_row(w, id)).collect::<Vec<_>>(),
        "ranking": ranking,
        "records": s.records.iter().rev().map(|r| json!({ "day": r.day, "name": r.name, "developer": r.developer, "height_m": round(r.height), "building": r.building.map(key_str) })).collect::<Vec<_>>(),
        "news": s.news.iter().rev().take(12).map(|n| json!({ "seq": n.seq, "tick": n.tick, "headline": n.headline, "pos": v2(n.pos), "building": n.building.map(key_str) })).collect::<Vec<_>>(),
        "developers": s.developers.iter().map(|d| json!({ "name": d.name, "capital": d.capital.round(), "pull": round(d.pull), "owner": d.owner.map(key_str) })).collect::<Vec<_>>(),
        "totals": { "projects": s.projects_total, "topped_out": s.topped_total, "plots_bought": s.plots_bought_total },
    })
}

/// `skyline`: the summary, or an action.
pub fn command(cmd: &Value, w: &mut World) -> Result<Value, String> {
    let default = if field(cmd, "cmd").and_then(|v| v.as_str()) == Some("height_limit") { "limit" } else { "summary" };
    let action = field(cmd, "action").and_then(|v| v.as_str()).unwrap_or(default);
    let rows = field(cmd, "limit_rows").and_then(|v| v.as_u64()).unwrap_or(15) as usize;
    match action {
        "summary" | "list" => Ok(summary(w, rows)),
        "limit" | "set_limit" => {
            match field(cmd, "limit").or(field(cmd, "m")) {
                Some(v) if v.is_null() => w.set_height_limit(None),
                Some(v) => {
                    let m = v.as_f64().ok_or("limit: metres, or null for none")? as f32;
                    w.set_height_limit((m > 0.0).then_some(m));
                }
                None => {}
            }
            if let Some(b) = field(cmd, "vertiport_clearance").and_then(|v| v.as_bool()) {
                w.skyline.vertiport_clearance = b;
            }
            Ok(json!({ "limit_m": w.skyline.limit, "vertiport_clearance": w.skyline.vertiport_clearance }))
        }
        "sites" => {
            let force = field(cmd, "force").and_then(|v| v.as_bool()).unwrap_or(false);
            let sites: Vec<Value> = w.tower_sites(force).into_iter().take(rows).map(|s| json!({ "lot": key_str(s.lot), "pos": v2(w.lots[s.lot].center), "score": round(s.score), "value": round(s.value), "neighbour_floors": s.neighbour_floors, "limit_m": s.limit, "land_use": w.lots[s.lot].land_use.label() })).collect();
            Ok(json!({ "sites": sites }))
        }
        "start" | "build" => {
            let order = TowerOrder {
                height: opt_f32(cmd, "height")?,
                design: match field(cmd, "design").and_then(|v| v.as_str()) {
                    Some(s) => Some(TowerDesign::parse(s).ok_or("design: deco_spire|bundled|twist|needle")?),
                    None => None,
                },
                near: field(cmd, "at").map(vec2_of).transpose()?,
                force: field(cmd, "force").and_then(|v| v.as_bool()).unwrap_or(true),
            };
            if field(cmd, "rich").and_then(|v| v.as_bool()).unwrap_or(false) {
                for d in &mut w.skyline.developers {
                    d.capital = d.capital.max(2e9);
                }
            }
            let id = w.start_tower(order)?;
            Ok(json!({ "tower": tower_row(w, id) }))
        }
        "finish" => {
            if let Some(v) = field(cmd, "building") {
                let id: BuildingId = key_from(v)?;
                if w.buildings.get(id).is_none_or(|b| b.plot.tower.is_none()) {
                    return Err("not a league tower".into());
                }
            }
            let n = w.finish_towers();
            Ok(json!({ "finished": n, "record_m": round(w.skyline.record()) }))
        }
        "fund" => {
            let amount = opt_f32(cmd, "amount")?.unwrap_or(1e9) as f64;
            for d in &mut w.skyline.developers {
                d.capital += amount;
            }
            Ok(json!({ "developers": w.skyline.developers.len() }))
        }
        other => Err(format!("unknown skyline action '{other}' (summary, limit, sites, start, finish, fund)")),
    }
}
