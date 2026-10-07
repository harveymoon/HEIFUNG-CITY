//! Remote commands for the landscape: start a new game on one, read the
//! ground at a point, and summarise the water, highway, bridges and docks.

use nd_sim::landscape::{LandscapeParams, LandscapePreset, MIN_HYDRO_CURRENT};
use nd_sim::{PortKind, PowerPlantKind, Vec2 as SVec2, World};
use serde_json::{json, Value};

use super::view::{opt_bool, opt_f32, opt_str, opt_u64, round, v2};

/// The levers of a `new_game` command over a preset's defaults.
pub fn params_from(cmd: &Value) -> Result<LandscapeParams, String> {
    let preset = match opt_str(cmd, "preset")? {
        Some(s) => LandscapePreset::from_label(s).ok_or_else(|| format!("unknown preset {s:?}; one of river_valley, islands, coast, delta, plains, flat"))?,
        None => LandscapePreset::RiverValley,
    };
    let seed = opt_u64(cmd, "seed")?.unwrap_or(7);
    let mut p = LandscapeParams::preset(preset, seed);
    if let Some(v) = opt_f32(cmd, "water")? {
        p.water = v.clamp(0.0, 1.0);
    }
    if let Some(v) = opt_f32(cmd, "relief")?.or(opt_f32(cmd, "height")?) {
        p.relief = v.clamp(0.0, 1.0);
    }
    if let Some(v) = opt_f32(cmd, "branching")? {
        p.branching = v.clamp(0.0, 1.0);
    }
    if let Some(v) = opt_u64(cmd, "islands")? {
        p.islands = v.min(8) as u8;
    }
    if let Some(v) = opt_bool(cmd, "highway")? {
        p.highway = v;
    }
    Ok(p)
}

/// Everything about the ground at a point.
pub fn terrain_at(w: &World, p: SVec2) -> Value {
    let land = &w.landscape;
    let water = land.water_at(p).map(|h| json!({ "kind": h.kind.label(), "current_mps": round(h.current), "width": if h.width.is_finite() { json!(round(h.width)) } else { Value::Null }, "flow": v2(h.flow) }));
    let shore = land.nearest_shore(p, 200.0).map(|s| json!({ "point": v2(s.point), "distance": round(s.distance), "water": s.water.kind.label(), "current_mps": round(s.water.current) }));
    let ring: Vec<SVec2> = (0..8).map(|k| p + SVec2::from_angle(k as f32 * 0.785) * 10.0).collect();
    let lot = w.lots.iter().find(|(_, l)| nd_sim::zoning::point_in_polygon(p, &l.polygon)).map(|(id, _)| id);
    let sites: Vec<Value> = match lot {
        Some(lot) => [PowerPlantKind::Hydro, PowerPlantKind::Tidal, PowerPlantKind::Geothermal]
            .into_iter()
            .map(|k| match w.power_site(lot, k) {
                Ok(f) => json!({ "kind": k.label(), "ok": true, "site_factor": round(f) }),
                Err(e) => json!({ "kind": k.label(), "ok": false, "why": e }),
            })
            .collect(),
        None => Vec::new(),
    };
    let docks: Vec<Value> = PortKind::ALL
        .into_iter()
        .map(|k| match w.plan_port(k, p) {
            Ok(port) => json!({ "kind": k.label(), "ok": true, "center": v2(port.center), "pier": [v2(port.pier[0]), v2(port.pier[1])] }),
            Err(e) => json!({ "kind": k.label(), "ok": false, "why": e }),
        })
        .collect();
    json!({
        "at": v2(p),
        "natural_height": round(land.height(p)),
        "ground_height": round(w.ground_height(p)),
        "surface_height": round(w.surface_height(p)),
        "slope": round(land.slope_over(&ring)),
        "water": water,
        "nearest_shore": shore,
        "hot_spot": land.hot_spot_at(p).map(|h| json!({ "center": v2(h.pos), "radius": round(h.radius), "heat": round(h.heat) })),
        "lot": lot.map(|l| super::view::key_str(l)),
        "power_sites": sites,
        "docks": docks,
        // The land survey layers at this point (what the K layers show).
        // [access] Minutes on foot to the nearest park and night out, along
        // the walk network, and what that adds to (or takes off) a home's land value.
        "access": w.land_value.access_at(p).map(|a| json!({ "park_min": a.park_min.map(round), "nightlife_min": a.nightlife_min.map(round), "value_for_homes": round(a.value), "text": a.describe() })),
        "survey": w.survey_at(p).into_iter().map(|(l, v)| (l.key().to_string(), json!(round(v)))).collect::<serde_json::Map<String, Value>>(),
    })
}

/// The landscape in numbers.
pub fn summary(w: &World) -> Value {
    let land = &w.landscape;
    let rivers: Vec<Value> = land
        .rivers
        .iter()
        .map(|r| json!({ "kind": r.kind().label(), "width": round(r.width), "current_mps": round(r.current), "hydro_ok": r.current >= MIN_HYDRO_CURRENT, "source": v2(r.points[0]), "mouth": v2(*r.points.last().unwrap()), "length": round(nd_sim::geom::path_length(&r.points)) }))
        .collect();
    let bridges: Vec<(f32, bool)> = w.roads.alive_segments().filter(|(_, s)| s.bridge).map(|(id, s)| (w.roads.segment_points(id).0.distance(w.roads.segment_points(id).1), s.is_highway())).collect();
    let (entries, exits) = w.highway_ends();
    let ports: Vec<Value> = w
        .ports
        .ports
        .values()
        .map(|p| json!({ "kind": p.kind.label(), "center": v2(p.center), "berth": v2(p.berth), "water": p.water.label(), "road_connected": p.road_connected, "reaches_elsewhere": p.reaches_elsewhere, "works": p.works(), "building": p.building.map(super::view::key_str) }))
        .collect();
    let s = &w.ports.stats;
    json!({
        "preset": land.params.preset.label(),
        "params": { "seed": land.params.seed, "water": round(land.params.water), "relief": round(land.params.relief), "branching": round(land.params.branching), "islands": land.params.islands, "highway": land.params.highway },
        "rivers": rivers,
        "lakes": land.lakes.iter().map(|l| v2(l.center)).collect::<Vec<_>>(),
        "sea": land.sea.as_ref().map(|s| json!({ "islands": s.islands.len(), "coast": !s.coast.is_empty() })),
        "hot_spots": land.hot_spots.iter().map(|h| json!({ "center": v2(h.pos), "radius": round(h.radius), "heat": round(h.heat) })).collect::<Vec<_>>(),
        "highway": { "entries": entries.len(), "exits": exits.len(), "through_cars": w.through_cars() },
        "bridges": { "count": bridges.len(), "metres": round(bridges.iter().map(|b| b.0).sum::<f32>()), "highway": bridges.iter().filter(|b| b.1).count() },
        "ports": ports,
        "ships": w.ports.ships.values().map(|sh| json!({ "kind": format!("{:?}", sh.kind).to_lowercase(), "at": v2(sh.pose().0), "docked": sh.docked, "passengers": sh.passengers, "cargo": sh.cargo.len() })).collect::<Vec<_>>(),
        "port_stats": { "freight_landed_today": s.freight_landed_today, "waiting_cargo": s.waiting_cargo, "ferry_riders_today": s.ferry_riders_today, "ferry_fares_today": round(s.ferry_fares_today) },
    })
}

/// A dock kind from a `place` kind string.
pub fn dock_of(s: &str) -> Option<PortKind> {
    match s {
        "port" | "shipping_port" | "harbour" | "harbor" => Some(PortKind::ShippingPort),
        "ferry" | "ferry_terminal" => Some(PortKind::FerryTerminal),
        _ => None,
    }
}
