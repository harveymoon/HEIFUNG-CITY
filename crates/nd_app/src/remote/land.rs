//! Remote commands for the landscape: start a new game on one, read the
//! ground at a point, and summarise the water, highway, bridges and docks.

use nd_sim::landscape::{Climate, LandscapeParams, LandscapePreset, MIN_HYDRO_CURRENT};
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
    p.climate = climate_from(cmd)?.unwrap_or(p.climate);
    Ok(p)
}

/// [climate] The `climate` field, if given.
pub fn climate_from(cmd: &Value) -> Result<Option<Climate>, String> {
    match opt_str(cmd, "climate")? {
        Some(s) => Climate::from_label(s).map(Some).ok_or_else(|| format!("unknown climate {s:?}; one of temperate, desert, tropical, arctic, volcanic")),
        None => Ok(None),
    }
}

/// `generate_city`'s fields: the city preset and the landscape it is laid
/// on (the preset's own, or `landscape` with any levers given, as for
/// `new_game`).
pub fn city_params_from(cmd: &Value) -> Result<(nd_procgen::citygen::CityPreset, LandscapeParams), String> {
    use nd_procgen::citygen::CityPreset;
    let preset = match opt_str(cmd, "preset")? {
        Some(s) => CityPreset::parse(s).ok_or_else(|| format!("unknown city preset {s:?}; one of {}", CityPreset::ALL.map(|p| p.key()).join(", ")))?,
        None => CityPreset::GridPlaza,
    };
    let seed = opt_u64(cmd, "seed")?.unwrap_or(1);
    let mut p = match opt_str(cmd, "landscape")? {
        Some(s) => LandscapeParams::preset(LandscapePreset::from_label(s).ok_or_else(|| format!("unknown landscape {s:?}; one of river_valley, islands, coast, delta, plains, flat"))?, seed),
        None => preset.landscape(seed),
    };
    if let Some(v) = opt_f32(cmd, "water")? {
        p.water = v.clamp(0.0, 1.0);
    }
    if let Some(v) = opt_f32(cmd, "relief")?.or(opt_f32(cmd, "height")?) {
        p.relief = v.clamp(0.0, 1.0);
    }
    if let Some(v) = opt_f32(cmd, "branching")? {
        p.branching = v.clamp(0.0, 1.0);
    }
    p.climate = climate_from(cmd)?.unwrap_or(p.climate);
    Ok((preset, p))
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
        // [dirty-industry] The poisoned ground and the dirty plants whose reach covers the point.
        "contamination": round(w.contamination.at(p)),
        "dirty_nearby": dirty_near(w, p),
        // [grid-weather] Wind and sun for power plants.
        "resource": resource_at(w, p),
    })
}

/// [grid-weather] The weather now: the sky, the wind (on the `0..1` drift
/// scale and in m/s over open ground), the temperature, any spell and the
/// climate it all comes from.
pub fn weather_json(w: &World) -> Value {
    let x = &w.weather;
    json!({
        "label": x.label(), "cloud": round(x.cloud), "wind": round(x.wind), "wind_ms": round(x.wind_ms()), "temp_c": round(x.temp_c),
        "spell": x.spell.label(), "spell_hours_left": x.spell_hours, "climate": x.climate.label(),
        "turbine": { "cut_in_ms": nd_sim::weather::CUT_IN_MS, "rated_ms": nd_sim::weather::RATED_MS, "cut_out_ms": nd_sim::weather::CUT_OUT_MS },
    })
}

/// [grid-weather] The grid's hour: the time-of-use prices now, the demand
/// now against the full (flat) draw, the next 24 hours if the weather
/// holds, and the last 24 recorded hours of demand, price and temperature.
pub fn grid_json(w: &World) -> Value {
    let p = &w.economy.power;
    let base = nd_sim::power::DemandBase::of(w.buildings.values());
    let ahead: Vec<Value> = w
        .grid_day_ahead()
        .into_iter()
        .map(|g| json!({ "hour": g.hour, "temp_c": round(g.temp_c), "demand_mw": round(g.demand_mw), "import_price": round(g.import_price), "export_price": round(g.export_price) }))
        .collect();
    let n = w.economy.history.hours.len();
    let past: Vec<Value> = w
        .economy
        .history
        .hours
        .iter()
        .skip(n.saturating_sub(24))
        .map(|h| json!({ "day": h.day, "hour": h.hour, "demand_mw": round(h.demand_mw), "supply_mw": round(h.supply_mw), "imported_mw": round(h.imported_mw), "exported_mw": round(h.exported_mw), "import_price": round(h.import_price), "temp_c": round(h.temp_c) }))
        .collect();
    json!({
        "hour": round(w.grid_hour()),
        "import_price": round(p.import_price), "export_price": round(p.export_price), "mean_import_price": nd_sim::power::GRID_IMPORT_PRICE,
        "demand_mw": round(p.demand_mw), "full_draw_mw": round(base.full()),
        "demand_factor_now": round(if base.full() > 0.0 { p.demand_mw / base.full() } else { 0.0 }),
        "weather": weather_json(w),
        "day_ahead": ahead,
        "last_24h": past,
    })
}

/// [grid-weather] The wind and sun at a point: the wind field's site factor,
/// the mean and current wind there, what a wind farm and a solar field
/// there would make now and on an ordinary day.
pub fn resource_at(w: &World, p: SVec2) -> Value {
    let land = &w.landscape;
    let site = w.site_resource_at(p);
    let wind_now = w.weather.wind_ms() * site.wind;
    let mean = nd_sim::weather::mean_wind_ms_at(land, p);
    json!({
        "at": v2(p),
        "climate": land.params.climate.label(),
        "water": land.is_water(p),
        "wind": {
            "site_factor": round(site.wind), "mean_ms": round(mean), "now_ms": round(wind_now),
            "typical_yield": round(nd_sim::weather::wind_yield_at(land, p)), "yield_now": round(w.kind_availability_at(PowerPlantKind::Wind, p)),
            "cut_out_now": wind_now >= nd_sim::weather::CUT_OUT_MS,
        },
        "solar": {
            "site_factor": round(site.sun), "climate_sun": round(nd_sim::weather::profile(land.params.climate).sun), "sun": round(nd_sim::weather::sun_at(land, p)),
            "yield_now": round(w.kind_availability_at(PowerPlantKind::Solar, p)),
        },
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
        .map(|p| json!({ "kind": p.kind.label(), "center": v2(p.center), "berth": v2(p.berth), "water": p.water.label(), "road_connected": p.road_connected, "reaches_elsewhere": p.reaches_elsewhere, "works": p.works(), "building": p.building.map(super::view::key_str),
            "ferries_sail": p.sails(), "stub_ends": p.building.map(|b| w.stub_ends_of(b).into_iter().map(v2).collect::<Vec<_>>()).unwrap_or_default(),
            "remote_towns": p.remote.iter().map(|t| json!({ "name": t.name, "edge": t.edge.label(), "km": round(t.km) })).collect::<Vec<_>>(), "sailings": p.sailings,
            "waiting_to_board": p.building.and_then(|b| w.ports.boarding.get(&b)).copied().unwrap_or(0) }))
        .collect();
    let s = &w.ports.stats;
    json!({
        "preset": land.params.preset.label(),
        "climate": land.params.climate.key(),
        "params": { "seed": land.params.seed, "water": round(land.params.water), "relief": round(land.params.relief), "branching": round(land.params.branching), "islands": land.params.islands, "highway": land.params.highway },
        "climate": land.params.climate.label(),
        "rivers": rivers,
        "lakes": land.lakes.iter().map(|l| v2(l.center)).collect::<Vec<_>>(),
        "sea": land.sea.as_ref().map(|s| json!({ "islands": s.islands.len(), "coast": !s.coast.is_empty() })),
        "hot_spots": land.hot_spots.iter().map(|h| json!({ "center": v2(h.pos), "radius": round(h.radius), "heat": round(h.heat) })).collect::<Vec<_>>(),
        "highway": { "entries": entries.len(), "exits": exits.len(), "through_cars": w.through_cars() },
        "bridges": { "count": bridges.len(), "metres": round(bridges.iter().map(|b| b.0).sum::<f32>()), "highway": bridges.iter().filter(|b| b.1).count() },
        "ports": ports,
        "ships": w.ports.ships.values().map(|sh| json!({ "kind": format!("{:?}", sh.kind).to_lowercase(), "at": v2(sh.pose().0), "docked": sh.docked, "passengers": sh.passengers, "cargo": sh.cargo.len(),
            "remote_town": sh.remote.map(|k| w.ferry_town_name(sh.port, k)), "back_home": sh.home })).collect::<Vec<_>>(),
        "port_stats": { "freight_landed_today": s.freight_landed_today, "waiting_cargo": s.waiting_cargo, "ferry_riders_today": s.ferry_riders_today, "ferry_fares_today": round(s.ferry_fares_today),
            "remote_sailings_today": s.remote_sailings_today, "visitors_in_today": s.visitors_in_today, "visitors_out_today": s.visitors_out_today, "commuters_in_today": s.commuters_in_today, "commuters_out_today": s.commuters_out_today,
            "day_trips_today": s.day_trips_today, "day_trippers_back_today": s.day_trippers_back_today,
            "abroad_now": w.citizens.values().filter(|c| matches!(c.activity, nd_sim::Activity::Abroad { .. })).count(), "commuters_in_town": w.tourism.commuters() },
    })
}

/// A dock kind from a `place` kind string.
pub fn dock_of(s: &str) -> Option<PortKind> {
    match s {
        "port" | "shipping_port" | "harbour" | "harbor" => Some(PortKind::ShippingPort),
        "ferry" | "ferry_terminal" => Some(PortKind::FerryTerminal),
        // [economy]
        "marina" | "marina_small" | "small_marina" => Some(PortKind::MarinaSmall),
        "marina_large" | "large_marina" | "yacht_club" => Some(PortKind::MarinaLarge),
        "fishing_port" | "fishing_port_small" | "small_fishing_port" | "fish_quay" => Some(PortKind::FishingPortSmall),
        "fishing_port_large" | "large_fishing_port" | "fishing_harbour" | "fishing_harbor" => Some(PortKind::FishingPortLarge),
        _ => None,
    }
}

/// [dirty-industry] The dirty plants whose reach covers `p`, nearest first,
/// and what a home and a farm there would suffer.
pub fn dirty_near(w: &World, p: nd_sim::Vec2) -> Value {
    use nd_sim::industry::{IndustryKind, Neighbour};
    let mut v: Vec<(f32, Value)> = w
        .dirty
        .iter()
        .filter(|s| s.pos.distance(p) < s.reach)
        .map(|s| {
            let d = s.pos.distance(p);
            let name = w.buildings.get(s.building).map(|b| b.name.clone()).unwrap_or_default();
            let what = w.buildings.get(s.building).and_then(|b| b.business.as_ref().map(|z| z.kind.label()).or(b.plant.map(|k| k.label()))).unwrap_or("plant");
            (d, json!({ "building": super::view::key_str(s.building), "name": name, "what": what, "distance_m": round(d), "level": round(s.level), "reach_m": round(s.reach) }))
        })
        .collect();
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    json!({
        "plants": v.into_iter().map(|x| x.1).collect::<Vec<_>>(),
        "home_exposure": round(w.dirt_exposure_at(p, Neighbour::Land(nd_sim::LandUse::Residential), None)),
        "farm_exposure": round(w.dirt_exposure_at(p, Neighbour::Industry(IndustryKind::Farming), None)),
    })
}
