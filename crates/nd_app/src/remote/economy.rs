//! [economy] Business pressure for the remote API: the `pressure` report
//! section and command (every pressure's strength and why, the signals
//! behind them, the trash companies, the art scene and the waterfront),
//! `pressure {open, at}` to open a business by hand, and the `story` block
//! of a building's `inspect` (why it is here, how it is doing).

use nd_sim::pressure::{Pressure, PressureKind};
use nd_sim::{BuildingId, BusinessKind, World};
use serde_json::{json, Value};

use super::view::{key_str, opt_bool, opt_str, req_point, round, v2};

fn reading(p: &Pressure) -> Value {
    json!({
        "kind": p.kind.key(), "label": p.kind.label(), "business": p.kind.business().map(|b| b.key()),
        "strength": round(p.strength), "wanted": round(p.wanted), "have": p.have, "gap": round(p.gap),
        "struggling": p.struggling, "wants_more": p.wants_more(), "why": p.why, "held": p.held,
        "player_placed": p.kind.player_placed(),
    })
}

/// The `pressure` report section.
pub fn report(w: &World) -> Value {
    let s = &w.pressure.signals;
    let mut readings: Vec<&Pressure> = w.pressure.readings.iter().collect();
    readings.sort_by(|a, b| b.strength.total_cmp(&a.strength).then(a.kind.cmp(&b.kind)));
    let t = &w.society.trash.stats;
    let sc = &w.scene;
    json!({
        "pressures": readings.iter().map(|p| reading(p)).collect::<Vec<_>>(),
        "opened_total": w.pressure.opened_total,
        "signals": {
            "outside_tows_per_day": round(s.outside_tows), "local_tows_per_day": round(s.local_tows), "crashes_per_day": round(s.crashes), "cars": s.cars,
            "rich_households": s.rich_households, "cars_bought_per_day": round(s.cars_bought), "cars_bought_outside_per_day": round(s.cars_bought_outside),
            "trash_uncovered_buildings": s.trash_uncovered, "trash_overflowing": s.trash_overflowing, "trash_depots": s.depots,
            "bike_owners": s.bike_owners, "bikes_bought_per_day": round(s.bikes_bought), "long_walks_per_day": round(s.long_walks),
            "livestock_per_day": round(s.livestock), "fish_per_day": round(s.fish), "fillets_per_day": round(s.fillets),
            "fishing_ports": [s.fishing_small, s.fishing_large], "marinas": [s.marinas_small, s.marinas_large],
            "comfortable_adults": s.comfortable_adults, "artists": s.artists, "artists_mid": s.artists_mid, "rich_adults": s.rich_adults,
            "rich_near_water": s.rich_near_water, "has_water": s.has_water, "art_scene": s.art_scene(),
        },
        "trash": { "local_companies": w.trash_companies().len(), "local_clients": t.local_clients, "local_fee": round(t.local_fee), "out_of_town_fee": nd_sim::trash::PRIVATE_FEE_HOUSEHOLD },
        "art": {
            "openings_total": sc.openings_total, "donations_total": round(sc.donations_total), "art_sold_total": sc.art_sold_total,
            "openings": sc.openings.iter().rev().take(6).map(|o| json!({
                "gallery": key_str(o.gallery), "name": w.buildings.get(o.gallery).map(|b| b.name.clone()), "from_tick": o.from, "until_tick": o.until,
                "on_now": o.on_at(w.clock.tick), "invited": o.invited.len(), "visitors": o.visitors, "donations": round(o.donations), "sales": o.sales,
            })).collect::<Vec<_>>(),
            "galleries_wanting_to_expand": w.buildings.iter().filter(|(_, b)| b.business.as_ref().is_some_and(|z| z.wants_expand)).map(|(id, _)| key_str(id)).collect::<Vec<_>>(),
        },
        "waterfront": {
            "boats_out": sc.boats_out, "berth_fees_last_night": round(sc.berth_fees_today),
            "docks": w.ports.ports.values().filter(|p| p.kind.business().is_some()).map(|p| json!({
                "kind": format!("{:?}", p.kind), "label": p.kind.label(), "building": p.building.map(key_str), "pos": v2(p.center), "works": p.works(),
                "berths": p.kind.berths(), "boats_out": p.building.map_or(0, |b| w.ports.ships.values().filter(|s| s.port == b).count()),
                "open": p.building.and_then(|b| w.buildings.get(b)).and_then(|b| b.business.as_ref()).is_some_and(|z| !z.closed),
            })).collect::<Vec<_>>(),
        },
        "repairs_today": round(sc.repairs_today), "bike_repairs_today": round(sc.bike_repairs_today),
    })
}

/// A building's `story`: why its business is there and how it is doing.
pub fn story(w: &World, id: BuildingId) -> Value {
    let Some(s) = w.business_story(id) else { return Value::Null };
    let z = w.buildings.get(id).and_then(|b| b.business.as_ref());
    json!({
        "why": s.why, "doing": s.doing, "struggling": s.struggling, "wants_expand": s.wants_expand,
        "opened_by_pressure": z.and_then(|z| PressureKind::of_business(z.kind)).map(|p| p.key()),
        "donations": z.map(|z| round(z.donations)),
        "opening_now": w.scene.opening_at(id, w.clock.tick).map(|o| json!({ "invited": o.invited.len(), "visitors": o.visitors, "donations": round(o.donations) })),
    })
}

/// `pressure`: the report section; with `open` (a business kind key such as
/// mechanic, car_dealer, golf_range) and `at` [x, y], opens one on the
/// vacant lot there (`finish`: built at once).
pub fn command(cmd: &Value, w: &mut World) -> Result<Value, String> {
    if let Some(kind) = opt_str(cmd, "open")? {
        let kind = BusinessKind::from_key(kind).ok_or_else(|| format!("unknown business kind {kind:?}; one of {}", BusinessKind::ALL.iter().map(|k| k.key()).collect::<Vec<_>>().join(", ")))?;
        let p = req_point(cmd)?;
        let lot = w
            .lots
            .iter()
            .filter(|(_, l)| l.building.is_none() && l.usable())
            .map(|(id, l)| (id, if nd_sim::zoning::point_in_polygon(p, &l.polygon) { 0.0 } else { l.center.distance(p) }))
            .filter(|(_, d)| *d < 150.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
            .ok_or("no vacant lot within 150 m")?;
        let finish = opt_bool(cmd, "finish")?.unwrap_or(false);
        let id = w.open_business(lot, kind, finish)?;
        return Ok(json!({ "building": key_str(id), "kind": kind.key(), "lot": key_str(lot), "finished": finish, "story": story(w, id) }));
    }
    Ok(report(w))
}
