//! [economy] Business pressure for the remote API: the `pressure` report
//! section and command (every pressure's strength and why, the signals
//! behind them, the trash companies, the art scene and the waterfront),
//! `pressure {open, at}` to open a business by hand, and the `story` block
//! of a building's `inspect` (why it is here, how it is doing).

use nd_sim::pressure::{Pressure, PressureKind};
use nd_sim::{BuildingId, BusinessKind, World};
use serde_json::{json, Value};

use super::view::{key_str, opt_bool, opt_str, req_point, round, v2};

fn reading(w: &World, p: &Pressure) -> Value {
    json!({
        "kind": p.kind.key(), "label": p.kind.label(), "business": p.kind.business().map(|b| b.key()),
        "strength": round(p.strength), "wanted": round(p.wanted), "have": p.have, "gap": round(p.gap),
        "struggling": p.struggling, "wants_more": p.wants_more(), "why": p.why, "held": p.held,
        "player_placed": p.kind.player_placed(),
        "needs": needs(w, p.kind),
    })
}

/// [r4-hunger] The lot a pressure's business needs (`World::lot_need`):
/// text, land uses, size, street, water, the anchors and the reach. Null
/// for the kinds the player builds or with nowhere to go yet.
fn needs(w: &World, kind: PressureKind) -> Value {
    let Some(n) = w.lot_need(kind) else { return Value::Null };
    json!({
        "text": n.text(), "uses": n.uses.iter().map(|u| u.label().to_lowercase()).collect::<Vec<_>>(), "min_area_m2": round(n.min_area),
        "street": n.needs_street, "waterfront": n.waterfront, "near": n.near, "anchors": n.anchors.iter().map(|a| v2(*a)).collect::<Vec<_>>(),
        "reach_m": n.reach.map(round),
    })
}

/// [r4-hunger] `pressure_where {kind}`: "show where" on the map for a
/// pressure (as the Economy pane's button): what it needs and the vacant
/// lots of its land uses with how each measures up; `{off: true}` hides
/// it; with neither, what is shown now.
pub(super) fn show_where(cmd: &Value, ctx: &mut super::ctx::Ctx) -> Result<Value, String> {
    if opt_bool(cmd, "off")? == Some(true) {
        ctx.flags.pressure_where = None;
    } else if let Some(k) = opt_str(cmd, "kind")? {
        let kind = PressureKind::from_key(k).ok_or_else(|| format!("unknown pressure kind {k:?}; one of {}", PressureKind::ALL.iter().map(|k| k.key()).collect::<Vec<_>>().join(", ")))?;
        ctx.flags.pressure_where = Some(kind);
    }
    let w = &ctx.sim.world;
    let Some(kind) = ctx.flags.pressure_where else { return Ok(json!({ "on": false })) };
    let lots = w.lot_need_lots(kind);
    let mut by: std::collections::BTreeMap<&str, u32> = Default::default();
    for (_, f) in &lots {
        *by.entry(f.key()).or_default() += 1;
    }
    Ok(json!({
        "on": true, "kind": kind.key(), "label": kind.label(), "held": w.pressure.reading(kind).and_then(|r| r.held.clone()),
        "needs": needs(w, kind), "lots_by_fit": by,
        "lots": lots.iter().filter_map(|(id, f)| w.lots.get(*id).map(|l| json!({ "lot": key_str(*id), "fit": f.key(), "center": v2(l.center), "area_m2": round(l.area()), "land_use": l.land_use.label().to_lowercase() }))).collect::<Vec<_>>(),
    }))
}

/// The `pressure` report section.
pub fn report(w: &World) -> Value {
    let s = &w.pressure.signals;
    let mut readings: Vec<&Pressure> = w.pressure.readings.iter().collect();
    readings.sort_by(|a, b| b.strength.total_cmp(&a.strength).then(a.kind.cmp(&b.kind)));
    let t = &w.society.trash.stats;
    let sc = &w.scene;
    json!({
        "pressures": readings.iter().map(|p| reading(w, p)).collect::<Vec<_>>(),
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
                "berths": p.kind.berths(), "boats_out": p.building.map_or(0, |b| w.ports.ships.values().filter(|s| s.port == b).count() + w.fleet(b).map_or(0, |f| f.at_sea())),
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

/// [fishing] A fishing port's fleet for `inspect {building}`: its boats
/// (what each is doing, where, the catch), its grounds and the day's
/// landings. Null for anything else.
pub fn fishing(w: &World, id: BuildingId) -> Value {
    let Some(f) = w.fleet(id) else { return Value::Null };
    let berth = w.ports.port_of(id).map(|p| p.berth);
    json!({
        "boats_total": f.boats.len(), "at_sea": f.at_sea(), "in_port": f.boats.len() - f.at_sea(),
        "landed_today": round(f.landed_today), "landed_yesterday": round(f.landed_yesterday), "trips_today": f.trips_today,
        "kept_in": f.kept_in, "fishes_grounds": !f.grounds.is_empty(), "offing": v2(f.offing),
        "boats": w.fishing_boats(id).into_iter().map(|r| json!({
            "name": r.name, "state": r.phase.key(), "doing": r.phase.label(), "at": v2(r.pos), "at_sea": r.phase.at_sea(), "nets_out": r.phase.nets_out(),
            "ground": r.ground.map(|g| g + 1), "fish_aboard": round(r.hold), "minutes_fished": r.fished_min, "catch_today": round(r.catch_today),
            "home_by": r.home_by.map(|h| format!("{:02}:{:02}", h / 60, h % 60)),
        })).collect::<Vec<_>>(),
        "berths": f.moorings.iter().enumerate().map(|(k, (p, _))| json!({ "at": v2(*p), "boat": f.boats.iter().find(|b| b.slot as usize == k).map(|b| b.name.clone()), "empty": f.boats.iter().find(|b| b.slot as usize == k).is_none_or(|b| b.phase.at_sea()) })).collect::<Vec<_>>(),
        "grounds": f.grounds.iter().enumerate().map(|(i, g)| json!({
            "ground": i + 1, "center": v2(g.center), "radius_m": round(g.radius), "richness": round(g.richness), "route_m": round(g.run()),
            "km_out": berth.map(|b| round(g.center.distance(b) / 1000.0)), "fouled": round(w.water_fouling(g.center)),
        })).collect::<Vec<_>>(),
    })
}

/// [fishing] Every ship and boat on the water: freighters, ferries, marina
/// boats and trawlers (at sea or at their berths).
pub fn ships(w: &World) -> Value {
    let mut out: Vec<Value> = w
        .ports
        .ships
        .values()
        .map(|sh| json!({ "kind": format!("{:?}", sh.kind).to_lowercase(), "port": key_str(sh.port), "at": v2(sh.pose().0), "heading_deg": round(sh.pose().1.y.atan2(sh.pose().1.x).to_degrees()), "docked": sh.docked, "dwell": sh.dwell, "boxes_in": sh.boxes_in, "boxes_out": sh.boxes_out, "broadside": sh.quay.is_some(),
            "crane": w.ports.ports.values().find(|p| p.building == Some(sh.port)).and_then(|p| nd_sim::ports::crane_work(p, sh, 0.0)).map(|c| json!({ "move": c.n + 1, "of": c.of, "landing": c.landing, "carrying": c.carrying })) }))
        .collect();
    for (b, f) in &w.fishing.fleets {
        for (k, t) in f.boats.iter().enumerate() {
            let (p, d) = f.boat_pose(k, 1.0);
            out.push(json!({ "kind": "fishingboat", "name": t.name, "port": key_str(*b), "port_name": w.buildings.get(*b).map(|x| x.name.clone()), "at": v2(p), "heading_deg": round(d.y.atan2(d.x).to_degrees()),
                "state": t.phase.key(), "at_sea": t.phase.at_sea(), "nets_out": t.phase.nets_out(), "fish_aboard": round(t.hold), "catch_today": round(t.catch_today) }));
        }
    }
    json!({ "count": out.len(), "trawlers_at_sea": w.fishing.fleets.values().map(|f| f.at_sea()).sum::<usize>(), "ships": out })
}
