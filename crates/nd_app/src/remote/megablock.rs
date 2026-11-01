//! [megablocks] Mega-blocks over the remote API: `megablock` (summary of
//! every block with what is inside and its trip tallies, why each corp may
//! or may not build one, dev actions to start or finish one now) and the
//! `megablock` block of `inspect` on a building.

use super::view::{field, key_from, key_str, round, v2};
use nd_sim::estate::OwnerId;
use nd_sim::{BuildingId, World};
use serde_json::{json, Value};

/// One block as data.
pub fn block_json(w: &World, id: BuildingId) -> Value {
    let Some(r) = w.megablock_report(id) else { return Value::Null };
    let m = w.buildings.get(id).and_then(|b| b.plot.megablock);
    json!({
        "id": key_str(id),
        "name": r.name,
        "corp": r.corp,
        "corp_id": m.map(|m| key_str(m.corp)),
        "open": r.open,
        "progress": round(r.progress),
        "stage": r.stage,
        "stage_key": (!r.open).then(|| nd_sim::megablock::Stage::of_progress(r.progress).key()),
        "days_left": round(r.days_left),
        "started_day": m.map(|m| m.started_day + 1),
        "opened_day": m.and_then(|m| m.opened_day).map(|d| d + 1),
        "pos": w.footprint(id).map_or(Value::Null, |l| v2(l.center)),
        "plots": r.plots,
        "area_m2": round(r.area_m2),
        "towers": r.towers,
        "tower_floors": m.map(|m| (0..m.towers).map(|i| m.floors_of(i)).collect::<Vec<_>>()),
        "storeys": r.floors,
        "height_m": round(r.height_m),
        "homes": r.homes,
        "households": r.households,
        "residents": r.residents,
        "shops": { "units": r.shops, "let": r.shops_let },
        "market_counters": { "units": r.market_counters, "let": r.counters_let },
        "offices": { "units": r.offices, "let": r.offices_let },
        "sky_gardens": r.gardens,
        "bridges": r.bridges,
        "parking": r.parking,
        "jobs": { "total": r.jobs, "filled": r.jobs_filled },
        "residents_working": r.workers,
        "residents_working_inside": r.work_inside,
        "trips": {
            "inside_today": r.inside_today, "out_today": r.out_today, "gardens_today": r.gardens_today,
            "inside_yesterday": r.inside_yesterday, "out_yesterday": r.out_yesterday, "gardens_yesterday": r.gardens_yesterday,
            "inside_share_yesterday": m.map(|m| round(m.inside_share())),
            "inside_total": r.inside_total,
        },
        "jobs_moved_inside_total": m.map(|m| m.moved_in_jobs_total),
        "tenants": r.tenants.iter().map(|(t, n, k)| json!({ "id": key_str(*t), "name": n, "kind": k })).collect::<Vec<_>>(),
    })
}

/// `inspect.building.megablock`: the block's data, or null.
pub fn building_megablock(w: &World, id: BuildingId) -> Value {
    let host = w.host_of(id);
    if w.buildings.get(host).is_some_and(|b| b.plot.megablock.is_some()) {
        let mut v = block_json(w, host);
        if host != id {
            v["tenant_in"] = json!(key_str(host));
        }
        return v;
    }
    Value::Null
}

fn corp_index(cmd: &Value, w: &World) -> Result<Option<usize>, String> {
    match field(cmd, "corp") {
        Some(v) => {
            let o: OwnerId = key_from(v)?;
            Ok(Some(w.corps.corps.iter().position(|c| c.owner == o).ok_or("no such corp")?))
        }
        None => Ok(None),
    }
}

/// `megablock {action}`.
pub fn command(cmd: &Value, w: &mut World) -> Result<Value, String> {
    let action = cmd.get("action").and_then(|v| v.as_str()).unwrap_or("summary");
    match action {
        "summary" | "list" => {
            let blocks: Vec<Value> = w.megablock_list().into_iter().map(|b| block_json(w, b)).collect();
            let corps: Vec<Value> = (0..w.corps.corps.len())
                .map(|ci| {
                    let c = &w.corps.corps[ci];
                    json!({ "corp": key_str(c.owner), "name": c.brand.name, "megablock": c.megablock.map(key_str), "may_build": w.megablock_refusal(ci).is_none(), "why_not": w.megablock_refusal(ci) })
                })
                .collect();
            Ok(json!({
                "blocks": blocks,
                "corps": corps,
                "stats": serde_json::to_value(&w.megablocks).unwrap_or(Value::Null),
                "rules": {
                    "min_pop": nd_sim::megablock::MEGABLOCK_MIN_POP, "corp_min_days": nd_sim::megablock::MEGABLOCK_CORP_MIN_DAYS,
                    "min_holdings": nd_sim::megablock::MEGABLOCK_MIN_HOLDINGS, "daily_chance": nd_sim::megablock::MEGABLOCK_DAILY_CHANCE,
                    "min_area_m2": nd_sim::megablock::MEGABLOCK_MIN_AREA, "build_days": nd_sim::megablock::MEGABLOCK_BUILD_DAYS,
                },
            }))
        }
        // Start one now: `corp` (else the first that can), `force` (dev,
        // default false: skip the population, age and holdings gates).
        "start" => {
            let force = cmd.get("force").and_then(|v| v.as_bool()).unwrap_or(false);
            let order: Vec<usize> = match corp_index(cmd, w)? {
                Some(ci) => vec![ci],
                None => (0..w.corps.corps.len()).collect(),
            };
            let mut why = "no mega-corps yet".to_string();
            for ci in order {
                match w.start_megablock(ci, force) {
                    Ok(b) => return Ok(json!({ "started": block_json(w, b) })),
                    Err(e) => why = e,
                }
            }
            Err(why)
        }
        // Dev: start one if needed and bring it to `progress` (0..1; 1 or
        // absent: finished and open), the bought plots cleared.
        "force" | "finish" => {
            let corp = corp_index(cmd, w)?.map(|ci| w.corps.corps[ci].owner);
            let progress = cmd.get("progress").and_then(|v| v.as_f64()).map(|p| p as f32);
            let b = w.force_megablock(corp, progress)?;
            Ok(json!({ "block": block_json(w, b) }))
        }
        // Why no site: each try's failure counted over the corp's first
        // `n` places to start from.
        "why" => {
            let ci = corp_index(cmd, w)?.unwrap_or(0);
            let c = w.corps.corps.get(ci).ok_or("no mega-corps yet")?;
            let owner = c.owner;
            let n = cmd.get("n").and_then(|v| v.as_u64()).unwrap_or(60) as usize;
            let mut why = [0u32; 6];
            let mut found = Vec::new();
            for p in w.megablock_primaries(ci).into_iter().take(n) {
                if let Some(s) = w.megablock_site_why(p, owner, &mut why) {
                    found.push(json!({ "primary": key_str(p), "plots": s.len() }));
                }
            }
            Ok(json!({ "primary_not_ok": why[0], "hole": why[1], "too_many_plots": why[2], "too_small": why[3], "not_compact": why[4], "shape": why[5], "sites": found }))
        }
        _ => Err(format!("unknown megablock action {action} (summary | start | force)")),
    }
}
