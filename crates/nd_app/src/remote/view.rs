//! JSON views of the simulation for the remote API: the id codec, field
//! parsers, and the row/detail serialisers shared by `find`, `inspect` and
//! `status`. Everything here is read-only; the command handlers in
//! `remote.rs` do the mutating.

use nd_minds::Personality;
use nd_sim::citizens::TravelMode;
use nd_sim::roads::RoadKind;
use nd_sim::zoning::polygon_area;
use nd_sim::{Activity, Building, BuildingId, Car, CarId, Citizen, CitizenId, Gesture, GestureId, LandUse, Lot, LotId, PowerPlantKind, RailFacility, RouteId, Tier, Train, TrainId, TrainKind, Vec2 as SVec2, World};
use serde_json::{json, Map, Value};
use slotmap::{Key, KeyData};

use crate::tools::{Selection, Tool, TransitMode};

// ── ids ─────────────────────────────────────────────────────────────────────
//
// Slotmap keys travel as `"idx:version"` strings. The two numbers are the
// key's own parts, so an id survives a save/load round trip of the same
// world and a stale id (the slot was reused) fails to resolve instead of
// pointing at a stranger.

pub fn key_str<K: Key>(k: K) -> String {
    let d = k.data().as_ffi();
    format!("{}:{}", d & 0xffff_ffff, d >> 32)
}

pub fn key_from<K: Key>(v: &Value) -> Result<K, String> {
    let s = v.as_str().ok_or_else(|| format!("id must be an \"idx:version\" string, got {v}"))?;
    let (idx, ver) = s.split_once(':').ok_or_else(|| format!("id {s:?} is not \"idx:version\""))?;
    let idx: u64 = idx.trim().parse().map_err(|_| format!("bad id {s:?}"))?;
    let ver: u64 = ver.trim().parse().map_err(|_| format!("bad id {s:?}"))?;
    Ok(K::from(KeyData::from_ffi((ver << 32) | idx)))
}

pub fn opt_key<K: Key>(k: Option<K>) -> Value {
    k.map_or(Value::Null, |k| Value::String(key_str(k)))
}

// ── field parsers ───────────────────────────────────────────────────────────

pub fn field<'a>(cmd: &'a Value, name: &str) -> Option<&'a Value> {
    cmd.get(name).filter(|v| !v.is_null())
}

pub fn req_f32(cmd: &Value, name: &str) -> Result<f32, String> {
    opt_f32(cmd, name)?.ok_or_else(|| format!("missing number field {name:?}"))
}

pub fn opt_f32(cmd: &Value, name: &str) -> Result<Option<f32>, String> {
    match field(cmd, name) {
        None => Ok(None),
        Some(v) => v.as_f64().map(|x| Some(x as f32)).ok_or_else(|| format!("field {name:?} must be a number")),
    }
}

pub fn opt_u64(cmd: &Value, name: &str) -> Result<Option<u64>, String> {
    match field(cmd, name) {
        None => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or_else(|| format!("field {name:?} must be a non-negative integer")),
    }
}

pub fn opt_bool(cmd: &Value, name: &str) -> Result<Option<bool>, String> {
    match field(cmd, name) {
        None => Ok(None),
        Some(v) => v.as_bool().map(Some).ok_or_else(|| format!("field {name:?} must be true or false")),
    }
}

pub fn opt_str<'a>(cmd: &'a Value, name: &str) -> Result<Option<&'a str>, String> {
    match field(cmd, name) {
        None => Ok(None),
        Some(v) => v.as_str().map(Some).ok_or_else(|| format!("field {name:?} must be a string")),
    }
}

pub fn req_str<'a>(cmd: &'a Value, name: &str) -> Result<&'a str, String> {
    opt_str(cmd, name)?.ok_or_else(|| format!("missing string field {name:?}"))
}

/// `[x, y]` in sim metres.
pub fn vec2_of(v: &Value) -> Result<SVec2, String> {
    let arr = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| format!("expected [x, y], got {v}"))?;
    let x = arr[0].as_f64().ok_or("x must be a number")? as f32;
    let y = arr[1].as_f64().ok_or("y must be a number")? as f32;
    Ok(SVec2::new(x, y))
}

/// Points `[[x, y], ...]` where each may carry a third number, its level
/// (metres above the ground; missing levels are None).
pub fn req_points_levels(cmd: &Value, name: &str) -> Result<(Vec<SVec2>, Vec<Option<f32>>), String> {
    let arr = field(cmd, name).and_then(Value::as_array).ok_or_else(|| format!("missing field {name:?}: [[x, y, level?], ...]"))?;
    let mut pts = Vec::new();
    let mut levels = Vec::new();
    for v in arr {
        let a = v.as_array().filter(|a| a.len() == 2 || a.len() == 3).ok_or_else(|| format!("expected [x, y] or [x, y, level], got {v}"))?;
        let n = |i: usize| a[i].as_f64().map(|x| x as f32).ok_or_else(|| format!("{v}: coordinates must be numbers"));
        pts.push(SVec2::new(n(0)?, n(1)?));
        levels.push(if a.len() == 3 { Some(n(2)?) } else { None });
    }
    Ok((pts, levels))
}

pub fn opt_vec2(cmd: &Value, name: &str) -> Result<Option<SVec2>, String> {
    field(cmd, name).map(vec2_of).transpose()
}

/// Either `{x, y}` fields or `pos: [x, y]`.
pub fn req_point(cmd: &Value) -> Result<SVec2, String> {
    if let Some(p) = opt_vec2(cmd, "pos")? {
        return Ok(p);
    }
    Ok(SVec2::new(req_f32(cmd, "x")?, req_f32(cmd, "y")?))
}

pub fn req_points(cmd: &Value, name: &str) -> Result<Vec<SVec2>, String> {
    let arr = field(cmd, name).and_then(Value::as_array).ok_or_else(|| format!("missing field {name:?}: [[x, y], ...]"))?;
    arr.iter().map(vec2_of).collect()
}

pub fn land_use_of(s: &str) -> Result<LandUse, String> {
    LandUse::ALL.iter().copied().find(|lu| lu.label().eq_ignore_ascii_case(s.trim())).ok_or_else(|| format!("unknown land_use {s:?}; one of residential, commercial, industrial, entertainment, civic, park"))
}

pub fn road_kind_of(s: &str) -> Result<RoadKind, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "street" | "road" => Ok(RoadKind::Street),
        "path" => Ok(RoadKind::Path),
        "rail" | "track" => Ok(RoadKind::Rail),
        _ => Err(format!("unknown road kind {s:?}; one of street, path, rail")),
    }
}

pub fn tool_of(s: &str) -> Result<Tool, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "inspect" => Ok(Tool::Inspect),
        "road" => Ok(Tool::Road),
        "zone" => Ok(Tool::Zone),
        "edit" => Ok(Tool::Edit),
        "bulldoze" => Ok(Tool::Bulldoze),
        "buildings" | "building" | "power" | "services" => Ok(Tool::Buildings),
        "transit" | "bus" | "buses" => Ok(Tool::Transit),
        "rail" => Ok(Tool::Rail),
        "district" | "districts" => Ok(Tool::District),
        _ => Err(format!("unknown tool {s:?}; one of inspect, road, zone, edit, bulldoze, buildings (alias power), transit, rail")),
    }
}

pub fn tool_name(t: Tool) -> &'static str {
    match t {
        Tool::Inspect => "inspect",
        Tool::Road => "road",
        Tool::Zone => "zone",
        Tool::Edit => "edit",
        Tool::Bulldoze => "bulldoze",
        Tool::Buildings => "buildings",
        Tool::Transit => "transit",
        Tool::Rail => "rail",
        Tool::District => "district",
    }
}

pub fn road_kind_name(k: RoadKind) -> &'static str {
    match k {
        RoadKind::Street => "street",
        RoadKind::Path => "path",
        RoadKind::Rail => "rail",
    }
}

pub fn plant_name(k: PowerPlantKind) -> &'static str {
    match k {
        PowerPlantKind::Coal => "coal",
        PowerPlantKind::Solar => "solar",
        PowerPlantKind::Wind => "wind",
        PowerPlantKind::Nuclear => "nuclear",
        PowerPlantKind::Hydro => "hydro",
        PowerPlantKind::Tidal => "tidal",
        PowerPlantKind::Geothermal => "geothermal",
    }
}

pub fn rail_name(k: RailFacility) -> &'static str {
    match k {
        RailFacility::Station => "station",
        RailFacility::FreightYard => "freight_yard",
        RailFacility::Central => "central_station",
    }
}

pub fn transit_mode_name(m: TransitMode) -> &'static str {
    match m {
        TransitMode::Route => "route",
        TransitMode::Depot => "depot",
        TransitMode::Metro => "metro",
    }
}

// ── small views ─────────────────────────────────────────────────────────────

pub fn v2(p: SVec2) -> Value {
    json!([round(p.x), round(p.y)])
}

pub fn points(ps: &[SVec2]) -> Value {
    Value::Array(ps.iter().map(|&p| v2(p)).collect())
}

/// Two decimals is plenty for metres and money, and keeps replies short.
pub fn round(x: f32) -> f64 {
    (x as f64 * 100.0).round() / 100.0
}

pub fn tick_time(tick: u64) -> Value {
    json!({ "tick": tick, "day": tick / 1440 + 1, "time": format!("{:02}:{:02}", (tick % 1440) / 60, tick % 60) })
}

pub fn building_name(w: &World, b: Option<BuildingId>) -> Value {
    b.and_then(|b| w.buildings.get(b)).map_or(Value::Null, |b| Value::String(b.name.clone()))
}

pub fn selection(w: &World, sel: Selection) -> Value {
    match sel {
        Selection::None => Value::Null,
        Selection::Citizen(id) => json!({ "kind": "citizen", "id": key_str(id), "name": w.citizens.get(id).map(|c| c.name.as_str()) }),
        Selection::Building(id) => json!({ "kind": "building", "id": key_str(id), "name": w.buildings.get(id).map(|b| b.name.as_str()) }),
        Selection::Car(id) => json!({ "kind": "car", "id": key_str(id) }),
        Selection::Parked(id) => json!({ "kind": "parked_car", "id": key_str(id) }),
    }
}

pub fn gesture(id: GestureId, g: &Gesture) -> Value {
    match g {
        Gesture::Road { points: ps, lanes_fwd, lanes_bwd, curved, kind, elevation } => json!({
            "id": id.0, "kind": "road", "road_kind": road_kind_name(*kind), "points": points(ps),
            "lanes_fwd": lanes_fwd, "lanes_bwd": lanes_bwd, "curved": curved,
            "elevation": (0..ps.len()).map(|i| elevation.get(i).copied().unwrap_or(0.0)).collect::<Vec<f32>>(),
        }),
        Gesture::Zone { polygon, land_use } => json!({ "id": id.0, "kind": "zone", "land_use": land_use.label(), "points": points(polygon), "area": round(polygon_area(polygon).abs()) }),
        Gesture::ParcelZone { parcels, land_use, industry } => json!({ "id": id.0, "kind": "parcel_zone", "land_use": land_use.map_or("unzoned", |l| l.label()), "parcels": parcels.len(), "industry": industry }),
        Gesture::Roundabout { center, radius, lanes } => json!({ "id": id.0, "kind": "roundabout", "center": [center.x, center.y], "radius": radius, "diameter": radius * 2.0, "lanes": lanes, "points": points(std::slice::from_ref(center)) }),
    }
}

// ── citizens ────────────────────────────────────────────────────────────────

/// What the citizen is up to, with the ids a caller can follow.
fn activity(w: &World, c: &Citizen) -> Value {
    let name_of = |b: Option<BuildingId>| building_name(w, b);
    let mut m = Map::new();
    m.insert("label".into(), json!(c.activity.label()));
    match &c.activity {
        Activity::Traveling { to, purpose, path, idx } => {
            m.insert("to".into(), opt_key(*to));
            m.insert("to_name".into(), name_of(*to));
            m.insert("purpose".into(), json!(format!("{purpose:?}")));
            m.insert("path_left".into(), json!(path.len().saturating_sub(*idx)));
        }
        Activity::Driving { car, to, purpose } => {
            m.insert("car".into(), json!(key_str(*car)));
            m.insert("to".into(), opt_key(*to));
            m.insert("to_name".into(), name_of(*to));
            m.insert("purpose".into(), json!(format!("{purpose:?}")));
        }
        Activity::Visiting(b) => {
            m.insert("building".into(), json!(key_str(*b)));
            m.insert("building_name".into(), name_of(Some(*b)));
        }
        Activity::Sitting { park, .. } => {
            m.insert("park".into(), json!(key_str(*park)));
        }
        Activity::Riding { bus } => {
            m.insert("bus".into(), json!(key_str(*bus)));
        }
        Activity::OnTrain { train } => {
            m.insert("train".into(), json!(key_str(*train)));
        }
        Activity::Flying { drone } => {
            m.insert("drone".into(), json!(key_str(*drone)));
        }
        Activity::OnMetro { train } => {
            m.insert("metro_train".into(), json!(key_str(*train)));
        }
        Activity::Arriving | Activity::AtHome | Activity::Working | Activity::Idle | Activity::Waiting { .. } => {}
    }
    if let Some(t) = &c.transit {
        m.insert("transit_target".into(), json!(key_str(t.target)));
        m.insert("transit_target_name".into(), name_of(Some(t.target)));
    }
    Value::Object(m)
}

pub fn citizen_row(w: &World, id: CitizenId, c: &Citizen) -> Value {
    json!({
        "id": key_str(id), "name": c.name, "activity": c.activity.label(), "pos": v2(c.pos),
        "home": opt_key(c.home), "home_name": building_name(w, c.home), "work": opt_key(c.work), "work_name": building_name(w, c.work),
        "class": c.class().label(), "mood": round(c.mood), "visible": c.is_visible(), "inside": opt_key(c.inside), "spotlight": c.spotlight,
    })
}

pub fn citizen_detail(w: &World, id: CitizenId, c: &Citizen, favourite: bool) -> Value {
    let rent = c.home.and_then(|h| w.buildings.get(h)).map_or(0.0, |b| b.rent);
    let mut v = json!({
        "id": key_str(id), "name": c.name, "personality": Personality::from_seed(c.seed).summary(),
        "activity": activity(w, c), "pos": v2(c.pos), "heading": round(c.heading), "visible": c.is_visible(),
        "mode": if c.mode == TravelMode::Bike { "bike" } else { "walk" },
        "home": opt_key(c.home), "home_name": building_name(w, c.home),
        "work": opt_key(c.work), "work_name": building_name(w, c.work),
        "inside": opt_key(c.inside), "parked_at": opt_key(c.parked_at),
        "class": c.class().label(), "mood": round(c.mood), "satiety": round(c.satiety),
        "arrived_day": c.arrived_tick / 1440 + 1,
        "income": round(c.income), "rent": round(rent), "savings": round(c.savings), "rent_stress_days": c.rent_stress_days,
        "groceries": round(c.groceries), "hungry_days": c.hungry_days, "owns_car": c.owns_car, "owns_bike": c.owns_bike,
        "age": c.age, "life_stage": c.stage().label(), "wakefulness": round(c.wakefulness), "asleep": c.asleep,
        "household": c.household.and_then(|h| w.households.get(h).map(|hh| json!({
            "id": key_str(h), "members": hh.members.iter().map(|m| key_str(*m)).collect::<Vec<_>>(), "money": round(hh.money),
            "groceries": round(hh.groceries), "income": round(w.household_income(h)), "rent_share": w.household_rent_share(h).filter(|s| s.is_finite()).map(round),
        }))),
        "vehicles": citizen_vehicles(w, c),
        "commute": {
            "pain": nd_sim::citizens::TripMode::ALL.iter().map(|m| (m.key().to_string(), json!({ "pain": round(c.commute.pain_of(*m)), "recent_trips": round(c.commute.trips[m.index()]) }))).collect::<serde_json::Map<String, Value>>(),
            "parking_pain": round(c.commute.parking_pain), "trip_mode_now": c.commute.mode.map(|m| m.key()),
            "last_trip": { "mode": c.commute.last_mode.map(|m| m.key()), "minutes": c.commute.last_minutes, "pain": round(c.commute.last_pain) },
        },
        "thought": c.thought, "spotlight": c.spotlight, "favourite": favourite,
        "memories": c.memories.iter().rev().map(|m| { let mut t = tick_time(m.tick); t["text"] = json!(m.text); t }).collect::<Vec<_>>(),
    });
    v["appearance"] = appearance(c);
    v
}

/// [looks] What the citizen looks like, as stored and as described.
pub fn appearance(c: &Citizen) -> Value {
    use nd_sim::looks::COLORS;
    let a = c.appearance();
    let colour = |i: u8| COLORS.get(i as usize).map(|s| json!({ "name": s.name, "srgb": s.srgb, "neon": s.neon }));
    json!({
        "description": a.describe(c.age, c.class()),
        "skin": { "tone": round(a.genes.skin), "label": a.skin_label(), "srgb": nd_sim::looks::skin_srgb(a.genes.skin) },
        "hair": { "style": format!("{:?}", a.style).to_lowercase(), "texture": a.genes.texture.label(), "natural": a.genes.hair.label(), "shown": a.shown_hair(c.age).label(), "srgb": a.shown_hair(c.age).srgb(), "greys_at": a.grey_at },
        "build": a.build.label(), "height": round(a.height), "stature_now": round(a.stature(c.age)),
        "favourite_colours": [colour(a.fav[0]), colour(a.fav[1])],
        "outfit": a.outfit.label(), "saved": c.look.is_some(),
    })
}

// ── buildings ───────────────────────────────────────────────────────────────

pub fn building_row(w: &World, id: BuildingId, b: &Building) -> Value {
    json!({
        "id": key_str(id), "name": b.name, "land_use": b.land_use.label(), "tier": b.tier.label(), "floors": b.floors,
        "pos": w.lots.get(b.lot).map_or(Value::Null, |l| v2(l.center)), "lot": key_str(b.lot),
        "homes": b.homes, "residents": b.residents.len(), "jobs": b.jobs, "workers": b.workers.len(),
        "under_construction": b.construction.is_some() && !b.being_demolished(), "being_demolished": b.demolition.is_some(),
        "business": b.business.as_ref().map(|z| z.kind.label()), "plant": b.plant.map(plant_name),
        "depot": b.depot, "rail": b.rail.as_ref().map(|r| rail_name(r.kind)), "derelict": b.is_derelict(),
        "service": b.service.map(|s| format!("{}:{}", s.kind.key(), tier_key(s.tier))), "burning": w.services.is_burning(id),
    })
}

pub fn building_detail(w: &World, id: BuildingId, b: &Building, style: &str) -> Value {
    let lot = w.lots.get(b.lot);
    let demolition = b.demolition.as_ref().map(|d| {
        json!({
            "progress": round(d.progress), "days_left": round(d.days_left()), "days": round(d.days),
            "loads_hauled": d.loads, "loads_total": nd_sim::Demolition::loads_for(d.floors),
            "floors": d.floors, "tier": d.tier.label(), "land_use": d.land_use.label(),
            "crew": b.workers.len(), "crew_wanted": b.jobs,
            "then": if d.clear_lot { "vacant lot" } else if b.construction.is_some() { "construction of the new building" } else { "the structure that took the lot over" },
        })
    });
    let construction = b.construction.as_ref().filter(|_| !b.being_demolished()).map(|c| {
        let (homes, jobs) = b.planned_capacity();
        json!({
            "progress": round(c.progress), "days_left": round(c.days_left()), "days": round(c.days),
            "deliveries": c.deliveries, "deliveries_needed": nd_sim::Construction::DELIVERIES,
            "waiting_for_materials": c.waiting_for_materials(), "ordered": c.ordered,
            "crew": b.workers.len(), "crew_wanted": b.jobs, "planned_homes": homes, "planned_jobs": jobs,
        })
    });
    let rail = b.rail.as_ref().map(|l| {
        json!({
            "kind": rail_name(l.kind), "rail_connected": l.rail_connected, "road_connected": l.road_connected, "works": l.works(),
            "platform": points(&l.platform), "inbound_orders": b.rail_inbound.len(),
            "inbound_units": round(b.rail_inbound.iter().map(|c| c.amount).sum::<f32>()), "outbound_units": round(b.rail_outbound),
        })
    });
    let business = b.business.as_ref().map(|z| {
        let recipe = z.kind.recipe();
        json!({
            "kind": z.kind.label(), "closed": z.closed, "money": round(z.money), "revenue_today": round(z.revenue_today),
            "spend_today": round(z.spend_today), "deals": z.deals, "staffing": round(b.staffing()), "broke_days": z.broke_days,
            "needs": recipe.inputs.iter().map(|(r, _)| json!({ "resource": r.label(), "stock": round(z.stock_of(*r)), "target": round(z.target_input(*r, b.jobs)) })).collect::<Vec<_>>(),
            "sells": recipe.outputs.iter().map(|(r, _)| json!({ "resource": r.label(), "stock": round(z.stock_of(*r)), "price": round(z.sell_price(*r, b.jobs)) })).collect::<Vec<_>>(),
        })
    });
    let plant = b.plant.map(|k| {
        json!({ "kind": plant_name(k), "label": k.label(), "capacity_mw": round(k.capacity_mw()), "availability": round(k.availability(&w.weather, w.clock.day_fraction())), "upkeep_per_day": round(k.upkeep_per_day()) })
    });
    let park = lot.and_then(|l| l.park.as_ref()).map(|p| {
        json!({ "benches": p.benches.len(), "paths": p.paths.len(), "entrances": p.entrances.len(), "features": p.features.iter().map(|f| f.kind.label()).collect::<Vec<_>>(), "cluster_lots": p.cluster_lots, "cluster_area": round(p.cluster_area) })
    });
    let people = |ids: &[CitizenId]| ids.iter().filter_map(|&c| w.citizens.get(c).map(|x| json!({ "id": key_str(c), "name": x.name }))).collect::<Vec<_>>();
    json!({
        "id": key_str(id), "name": b.name, "land_use": b.land_use.label(), "tier": b.tier.label(), "style": style, "floors": b.floors,
        "lot": key_str(b.lot), "pos": lot.map_or(Value::Null, |l| v2(l.center)), "built_day": b.built_tick / 1440 + 1,
        "under_construction": construction,
        "demolition": demolition,
        "look": b.look.map(|l| l.label()), "district": w.district_of_lot(b.lot).map(|d| d.name.clone()),
        "land_value": round(b.land_value), "rent": round(b.rent), "wage": round(b.wage()), "condition": round(b.condition), "derelict": b.is_derelict(),
        "homes": b.homes, "jobs": b.jobs, "visitors": b.visitors,
        "residents": people(&b.residents), "workers": people(&b.workers),
        "footfall_today": lot.map(|l| l.footfall_today), "footfall_hour": lot.map(|l| l.footfall_hour), "car_access": lot.map(|l| l.car_access()),
        "powered": round(b.powered), "demand_mw": round(nd_sim::power::demand_mw(b)), "unpowered_hours_today": b.unpowered_hours_today,
        "parking": if b.parking_capacity > 0 { json!({ "capacity": b.parking_capacity, "used": b.parking_used, "occupancy": round(b.parking_used as f32 / b.parking_capacity.max(1) as f32), "price": round(b.parking_price), "public": b.is_parking(), "city_run": w.vehicles.city_lots.get(&id).map(|l| l.size.key()) }) } else { Value::Null },
        "parking_office": b.service.filter(|s| s.kind == nd_sim::ServiceKind::ParkingEnforcement).map(|s| json!({
            "officers": b.workers.len(), "patrol_cars_now": w.enforcement.patrols.iter().filter(|p| p.office == id).count(), "patrol_cars_wanted": w.enforcement_cars(id),
            "radius_m": s.kind.radius(s.tier), "patrol_cars": w.enforcement.patrols.iter().filter(|p| p.office == id).map(|p| key_str(p.car)).collect::<Vec<_>>(),
        })),
        "tow_company": b.business.as_ref().filter(|z| z.kind == nd_sim::BusinessKind::TowYard).map(|_| json!({
            "trucks": w.tow_trucks(id), "jobs_open": w.enforcement.jobs.iter().filter(|j| j.company == Some(id)).count(),
            "pound": w.vehicles.cars.iter().filter(|(_, c)| c.spot().is_some_and(|s| s.kind == nd_sim::ParkKind::Pound && s.building == Some(id))).map(|(v, c)| json!({ "vehicle": key_str(v), "owner": key_str(c.owner), "release_fee": round(c.spot().map_or(0.0, |s| w.release_fee(s))) })).collect::<Vec<_>>(),
        })),
        "plant": plant, "depot": b.depot, "rail": rail, "business": business, "park": park,
    })
}

// ── lots ────────────────────────────────────────────────────────────────────

pub fn lot_row(w: &World, id: LotId, l: &Lot) -> Value {
    json!({
        "id": key_str(id), "land_use": l.land_use.label(), "pos": v2(l.center), "area": round(l.area()),
        "building": opt_key(l.building), "vacant": l.building.is_none() && l.usable(), "landlocked": l.landlocked,
        "park": l.park.is_some(), "land_value": round(w.land_value.at(l.center)), "car_access": l.car_access(),
    })
}

pub fn lot_detail(w: &World, id: LotId, l: &Lot) -> Value {
    let value = w.land_value.at(l.center);
    let zone = w.zones.get(l.zone).map(|z| json!({ "id": key_str(l.zone), "land_use": z.land_use.label(), "area": round(polygon_area(&z.polygon).abs()) }));
    json!({
        "id": key_str(id), "land_use": l.land_use.label(), "pos": v2(l.center), "polygon": points(&l.polygon), "area": round(l.area()),
        "width": round(l.width()), "depth": round(l.depth()), "entrance": v2(l.entrance()), "front_dir": v2(l.front_dir),
        "front_segment": l.front_segment.0, "car_segment": l.car_segment.map(|s| s.0),
        "building": opt_key(l.building), "building_name": building_name(w, l.building),
        "vacant": l.building.is_none() && l.usable(), "usable": l.usable(), "landlocked": l.landlocked, "car_access": l.car_access(),
        "park": l.park.is_some(), "land_value": round(value), "land_tier": Tier::for_land_value(value).label(),
        "footfall_today": l.footfall_today, "footfall_hour": l.footfall_hour, "zone": zone,
    })
}

// ── vehicles ────────────────────────────────────────────────────────────────

pub fn car_kind(w: &World, c: &Car) -> &'static str {
    use nd_sim::vehicle::Errand;
    if c.bus.is_some() {
        "bus"
    } else if c.siren {
        "emergency"
    } else if c.cargo.is_some() {
        "truck"
    } else if w.citizens.contains_key(c.citizen) {
        "car"
    } else {
        match c.errand {
            // [carlife] A work van out on (or back from) a job, empty.
            Errand::Haul { .. } | Errand::Return(_) => "van",
            // A private car moving without its owner aboard: to or from a
            // dealer, parking itself, leaving town.
            _ if c.owned.is_some() => "car",
            // Through traffic crossing the map.
            _ => "through",
        }
    }
}

pub fn car_row(w: &World, id: CarId, c: &Car) -> Value {
    let kind = car_kind(w, c);
    let mut m = json!({
        "id": key_str(id), "kind": kind, "pos": v2(c.pos), "heading": round(c.heading), "speed_kmh": round(c.v * 3.6), "at_stop": c.hold > 0.0,
        "model": { "body": c.model.body.label(), "class": format!("{:?}", c.model.class).to_lowercase(), "wear": c.model.wear, "tuned": c.model.tuned, "livery": format!("{:?}", c.model.livery).to_lowercase() },
    });
    if kind == "car" {
        m["citizen"] = json!(key_str(c.citizen));
        m["citizen_name"] = json!(w.citizens.get(c.citizen).map(|x| x.name.as_str()));
    }
    if let Some(bus) = &c.bus {
        m["route"] = json!(key_str(bus.route));
        m["route_name"] = json!(w.transit.routes.get(bus.route).map(|r| r.name.as_str()));
        m["passengers"] = json!(bus.passengers.len());
        m["next_stop"] = if bus.to_depot || bus.next_stop == usize::MAX { Value::Null } else { json!(bus.next_stop) };
        m["to_depot"] = json!(bus.to_depot);
        m["depot"] = json!(key_str(bus.depot));
    }
    if let Some(cargo) = &c.cargo {
        m["cargo"] = json!({ "resource": cargo.resource.label(), "amount": round(cargo.amount), "price": round(cargo.price), "from": key_str(cargo.from), "from_name": building_name(w, Some(cargo.from)), "to": key_str(cargo.to), "to_name": building_name(w, Some(cargo.to)) });
    }
    m
}

pub fn car_detail(w: &World, id: CarId, c: &Car) -> Value {
    let mut m = car_row(w, id, c);
    if let Some(bus) = &c.bus {
        m["passenger_list"] = json!(bus.passengers.iter().filter_map(|&p| w.citizens.get(p).map(|x| json!({ "id": key_str(p), "name": x.name }))).collect::<Vec<_>>());
    }
    m["route_lanes_left"] = json!(c.route.len().saturating_sub(c.idx));
    m["errand"] = json!(format!("{:?}", c.errand));
    // [parking] A tow truck: the job, and the car it is fetching or carrying.
    if let nd_sim::Errand::Tow(job) = c.errand {
        if let Some(j) = w.enforcement.job(job) {
            m["tow"] = json!({ "job": j.id, "state": format!("{:?}", j.state), "reason": j.reason.label(), "vehicle": key_str(j.car), "company": j.company.map(key_str), "company_name": building_name(w, j.company), "out_of_town": j.company.is_none() });
        }
    }
    if let nd_sim::Errand::Patrol(office) = c.errand {
        m["patrol"] = json!({ "office": key_str(office), "office_name": building_name(w, Some(office)) });
    }
    m["traffic"] = car_traffic(w, c);
    m
}

/// Why a vehicle drives as it does: what holds it, its lane's arrows, its
/// next turn and how long it has waited for a left turn.
pub fn car_traffic(w: &World, c: &Car) -> Value {
    use nd_sim::traffic::arrows_label;
    let net = &w.traffic.net;
    let lane = &net.lanes[c.route[c.idx] as usize];
    let next = c.route.get(c.idx + 1).map(|&n| &net.lanes[n as usize]);
    let next_turn = match next {
        Some(n) if n.is_connector() && !lane.is_connector() => match n.turn {
            t if t < 0 => "left",
            0 => "straight",
            _ => "right",
        },
        Some(n) if lane.is_beside(c.route[c.idx + 1]) && n.length > 0.0 => "lane change",
        _ => "none",
    };
    json!({
        "wait": format!("{:?}", c.wait).to_lowercase(), "stalled_s": round(c.stalled), "in_junction": lane.is_connector(),
        "lane_arrows": if lane.arrows != 0 { Value::String(arrows_label(lane.arrows)) } else { Value::Null },
        "next_turn": next_turn, "left_turn_wait_s": round(c.turn_wait), "yielding_s": round(c.yielded),
        "passing_on_shoulder": c.passing, "lateral_offset_m": round(c.lateral), "entering": c.entering,
        // [driving]
        "driver": ({
            let p = &c.drive.profile;
            json!({ "style": p.label(), "speed_factor": round(p.speed), "headway_s": round(p.headway), "accel": round(p.accel), "reaction_s": round(p.reaction),
                "signal_compliance": round(p.compliance), "aggression": round(p.aggression), "skill": round(p.skill), "drunkenness": round(p.drunk), "ignores_signals": p.ignores_signals })
        }),
        "driver_drunkenness_now": w.citizens.get(c.citizen).map(|x| round(x.drunk)),
        "weave_m": round(c.drive.weave), "running_red": c.drive.running.is_some(),
        "crash": c.drive.crash.map(|s| json!({ "crash": s.id, "wrecked": s.wrecked, "hold_s": if s.wrecked { Value::Null } else { json!(round(s.hold)) } })),
        "takeover": c.drive.gang.map(|g| json!({ "takeover": g.takeover, "phase": format!("{:?}", g.phase).to_lowercase() })),
        "stunt": c.drive.stunt.map(|s| if s.is_donut() { "donut" } else { "ring" }),
    })
}

/// A junction: its signal, every arriving street's lane arrows and the
/// waits of the left turns made there.
pub fn junction_detail(w: &World, k: usize) -> Value {
    use nd_sim::traffic::arrows_label;
    let net = &w.traffic.net;
    let inter = &net.intersections[k];
    let node = w.roads.node(inter.node);
    let approaches: Vec<Value> = net
        .approaches
        .iter()
        .filter(|a| a.node == inter.node)
        .map(|a| {
            let (p, q) = w.roads.segment_points(a.segment);
            let far = if p.distance(node.pos) > q.distance(node.pos) { p } else { q };
            let total: f32 = a.counts.iter().sum();
            json!({
                "segment": a.segment.0, "from": v2(node.pos + (far - node.pos).normalize_or_zero() * 30.0),
                "lanes": a.arrows.iter().map(|&x| arrows_label(x)).collect::<Vec<_>>(), "offered": arrows_label(a.exits), "set_by_player": a.fixed,
                "recent_turn_share": if total > 0.0 { json!({ "left": round(a.counts[0] / total), "straight": round(a.counts[1] / total), "right": round(a.counts[2] / total) }) } else { Value::Null },
                "queued": a.lanes.iter().map(|&l| net.lanes[l as usize].cars.len()).sum::<usize>(),
            })
        })
        .collect();
    let group = inter.phases.get(inter.phase);
    json!({
        "node": inter.node.0, "pos": v2(node.pos), "streets": inter.approaches, "signalized": inter.signalized,
        "signal": if inter.signalized { json!({
            "phases": inter.phases, "green_approaches": group, "timer_s": round(inter.timer),
            "protected_left_arrow_s": round(inter.arrow), "arrow_now": inter.timer < inter.arrow,
        }) } else { Value::Null },
        "approaches": approaches,
        "left_turns": inter.left_turns,
        "left_turn_wait_mean_s": round(if inter.left_turns > 0 { inter.left_wait_sum / inter.left_turns as f32 } else { 0.0 }),
        "left_turn_wait_max_s": round(inter.left_wait_max),
    })
}

/// The junction (three or more streets) nearest a point.
pub fn junction_near(w: &World, p: SVec2) -> Option<usize> {
    w.traffic
        .net
        .intersections
        .iter()
        .enumerate()
        .filter(|(_, i)| i.approaches >= 3)
        .min_by(|a, b| w.roads.node(a.1.node).pos.distance(p).total_cmp(&w.roads.node(b.1.node).pos.distance(p)))
        .map(|(k, _)| k)
}

pub fn train_row(w: &World, id: TrainId, t: &Train) -> Value {
    let head = nd_sim::rail::point_along(&t.poly, t.s).0;
    let next = t.stops.get(t.next_stop).map(|(b, _)| *b);
    json!({
        "id": key_str(id), "kind": match t.kind { TrainKind::Passenger => "passenger", TrainKind::Freight => "freight" },
        "cars": t.cars, "pos": v2(head), "speed": round(t.speed), "held": t.held, "dwell": round(t.dwell),
        "passengers": t.passengers.len(), "cargo_units": round(t.cargo.iter().map(|c| c.amount).sum::<f32>()),
        "next_stop": opt_key(next), "next_stop_name": building_name(w, next), "stops": t.stops.len(),
    })
}

pub fn route_row(id: RouteId, r: &nd_sim::BusRoute) -> Value {
    json!({
        "id": key_str(id), "name": r.name, "color": r.color, "buses": r.buses, "fare": round(r.fare), "riders_today": r.riders_today,
        "stops": r.stops.iter().map(|s| json!({ "pos": v2(s.pos), "segment": s.segment.0 })).collect::<Vec<_>>(),
        "broken_legs": r.broken, "legs": r.legs.len(),
    })
}

/// The remote-API name of something the Buildings tool places: a plant
/// name, `depot`, `station`, `freight_yard`, or `<service>:<tier>`.
pub fn placeable_key(p: nd_sim::Placeable) -> String {
    match p {
        nd_sim::Placeable::Plant(k) => plant_name(k).to_string(),
        nd_sim::Placeable::Service(k, t) => format!("{}:{}", k.key(), tier_key(t)),
        nd_sim::Placeable::BusDepot => "depot".to_string(),
        nd_sim::Placeable::Rail(f) => rail_name(f).to_string(),
        nd_sim::Placeable::Dock(nd_sim::PortKind::ShippingPort) => "shipping_port".to_string(),
        nd_sim::Placeable::Dock(nd_sim::PortKind::FerryTerminal) => "ferry_terminal".to_string(),
        nd_sim::Placeable::ParkingLot(s) => s.key().to_string(),
        nd_sim::Placeable::MetroStation => "metro_station".to_string(),
        nd_sim::Placeable::Toll(s) => s.key().to_string(),
    }
}

pub fn tier_key(t: nd_sim::Tier) -> &'static str {
    match t {
        nd_sim::Tier::Low => "low",
        nd_sim::Tier::Mid => "mid",
        nd_sim::Tier::High => "high",
    }
}

/// [carlife] A citizen's wheels: the bike, and the car with where it is.
pub fn citizen_vehicles(w: &World, c: &Citizen) -> Value {
    let car = c.car.and_then(|vid| w.vehicles.cars.get(vid).map(|car| (vid, car))).map(|(vid, car)| {
        let place = match car.place {
            nd_sim::CarPlace::Parked(s) => vehicle_spot(w, vid, &s),
            nd_sim::CarPlace::Driving(cid) => json!({ "driving": key_str(cid), "errand": format!("{:?}", w.traffic.cars.get(cid).map(|t| t.errand)) }),
        };
        json!({ "id": key_str(vid), "body": car.model.body.label(), "class": format!("{:?}", car.model.class), "price": round(car.model.price()), "place": place })
    });
    json!({ "bike": c.owns_bike, "car": car })
}

// ── air ─────────────────────────────────────────────────────────────────────

/// One drone or helicopter, for `inspect {drone}` and `find {what: "drones"}`.
pub fn drone_detail(w: &World, id: nd_sim::air::DroneId) -> Option<Value> {
    let d = w.air.drones.get(id)?;
    let job = match d.job {
        nd_sim::air::Job::Taxi { rider, from, to, aboard } => json!({ "kind": "taxi", "rider": key_str(rider), "rider_name": w.citizens.get(rider).map(|c| c.name.clone()), "from": key_str(from), "from_name": building_name(w, Some(from)), "to": key_str(to), "to_name": building_name(w, Some(to)), "aboard": aboard }),
        nd_sim::air::Job::Medic { incident, patient, hospital, aboard, .. } => json!({ "kind": "medic", "incident": incident, "patient": key_str(patient), "hospital": opt_key(hospital), "hospital_name": building_name(w, hospital), "aboard": aboard }),
        nd_sim::air::Job::Patrol { incident } => json!({ "kind": "patrol", "incident": incident }),
        nd_sim::air::Job::Home => json!({ "kind": "home" }),
        nd_sim::air::Job::Idle => json!({ "kind": "idle" }),
    };
    Some(json!({
        "id": key_str(id), "craft": d.craft.label(), "call_sign": format!("{} {}", d.craft.call_sign(), d.number),
        "base": key_str(d.base), "base_name": building_name(w, Some(d.base)),
        "pos": v2(d.pos), "altitude_m": round(d.alt), "heading": round(d.heading),
        "phase": d.phase.label(), "doing": d.job.label(), "job": job,
        "dest": opt_key(d.dest), "dest_name": building_name(w, d.dest), "goal": v2(d.goal),
        "on_pad": opt_key(d.at), "slot": d.slot, "battery": round(d.battery.max(0.0)),
        "flights_today": d.flights_today, "km_today": round(d.km_today), "flights_total": d.flights_total,
    }))
}

/// `report.services.air`: pads, the fleet, today's flights, riders, fares, noise.
pub fn air_report(w: &World) -> Value {
    use nd_sim::air::{Craft, PadKind, Phase};
    let a = &w.air;
    let pads_of = |k: PadKind| a.pads.iter().filter(|p| p.kind == k).count();
    let vertiports: Vec<Value> = a
        .pads
        .iter()
        .filter(|p| p.kind == PadKind::Vertiport)
        .map(|p| json!({ "id": key_str(p.building), "name": building_name(w, Some(p.building)), "slots": p.slots, "slots_used": a.slots_used(p.building), "drones_based": a.drones.values().filter(|d| d.base == p.building).count(), "pos": v2(p.pos) }))
        .collect();
    let craft = |c: Craft| a.drones.values().filter(|d| d.craft == c).count();
    let airborne = a.drones.values().filter(|d| d.phase.airborne()).count();
    let holding = a.drones.values().filter(|d| d.phase == Phase::Holding).count();
    let waiting = w.citizens.values().filter(|c| matches!(c.activity, Activity::Waiting { .. }) && matches!(c.transit.map(|t| t.leg), Some(nd_sim::transit::TransitLeg::Air { .. }))).count();
    let flying = w.citizens.values().filter(|c| matches!(c.activity, Activity::Flying { .. })).count();
    let mean_battery = {
        let t: Vec<f32> = a.drones.values().filter(|d| d.craft == Craft::Taxi).map(|d| d.battery).collect();
        if t.is_empty() { Value::Null } else { json!(round(t.iter().sum::<f32>() / t.len() as f32)) }
    };
    let (noise_max, noise_cells) = (a.noise.values().fold(0.0f32, |m, n| m.max(*n)), a.noise.len());
    let s = &a.stats;
    json!({
        "vertiports": vertiports,
        "pads": { "vertiports": pads_of(PadKind::Vertiport), "rooftop": pads_of(PadKind::Rooftop), "hospital": pads_of(PadKind::Hospital), "fire": pads_of(PadKind::Fire), "police": pads_of(PadKind::Police) },
        "rooftop_pads": a.rooftops.iter().map(|b| json!({ "id": key_str(*b), "name": building_name(w, Some(*b)) })).collect::<Vec<_>>(),
        "drones": { "taxis": craft(Craft::Taxi), "air_ambulances": craft(Craft::AirAmbulance), "police_helicopters": craft(Craft::PoliceHeli), "airborne": airborne, "holding": holding, "mean_taxi_battery": mean_battery },
        "flights_today": s.flights_today, "riders_today": s.riders_today, "fares_today": round(s.fares_today),
        "flights_yesterday": s.flights_yesterday, "riders_yesterday": s.riders_yesterday, "fares_yesterday": round(s.fares_yesterday), "riders_total": s.riders_total,
        "waiting_for_drone": waiting, "in_the_air": flying, "gave_up_today": s.gave_up_today, "holds_today": s.holds_today,
        "medic_flights_today": s.medic_flights_today, "medic_flights_total": s.medic_flights_total, "police_flights_today": s.police_flights_today,
        "company": { "name": a.company.name, "money": round(a.company.money), "revenue_today": round(a.company.revenue_today), "revenue_yesterday": round(a.company.revenue_yesterday), "costs_per_day": round(a.company.costs_today) },
        "noise": { "cells": noise_cells, "max": round(noise_max), "max_land_value_penalty": round(nd_sim::air::noise_penalty(noise_max)), "grumbles_today": s.grumbles_today },
    })
}

/// [parking] Where a parked car is, with its ticket, limit and the pound.
pub fn vehicle_spot(w: &World, vid: nd_sim::VehicleId, s: &nd_sim::ParkSpot) -> Value {
    let mut v = json!({ "parked": s.kind.label(), "building": s.building.map(key_str), "building_name": building_name(w, s.building), "pos": v2(s.pos), "since_tick": s.since, "minutes_here": w.clock.tick.saturating_sub(s.since) });
    if s.kind.on_street() {
        v["limit_min"] = json!(w.kerb_limit(vid));
        v["illegal"] = json!(w.parked_illegally(s));
        v["ticketed_tick"] = if s.ticketed > 0 { json!(s.ticketed) } else { Value::Null };
    }
    if s.kind == nd_sim::ParkKind::Pound {
        v["impounded"] = json!({ "pound": s.building.map(key_str), "out_of_town": s.building.is_none(), "release_fee": round(w.release_fee(s)), "auction_in_min": (s.since + nd_sim::enforcement::AUCTION_AFTER).saturating_sub(w.clock.tick) });
    }
    if let Some(j) = w.enforcement.job_for(vid) {
        v["tow_job"] = json!({ "job": j.id, "state": format!("{:?}", j.state), "reason": j.reason.label(), "truck": j.truck.map(key_str) });
    }
    v
}

/// [parking] A private car by its record id: owner, model and place.
pub fn vehicle_detail(w: &World, vid: nd_sim::VehicleId) -> Option<Value> {
    let car = w.vehicles.cars.get(vid)?;
    let place = match car.place {
        nd_sim::CarPlace::Parked(s) => vehicle_spot(w, vid, &s),
        nd_sim::CarPlace::Driving(cid) => json!({ "driving": key_str(cid), "errand": format!("{:?}", w.traffic.cars.get(cid).map(|t| t.errand)) }),
    };
    Some(json!({ "id": key_str(vid), "owner": key_str(car.owner), "owner_name": w.citizens.get(car.owner).map(|c| c.name.clone()), "body": car.model.body.label(), "class": format!("{:?}", car.model.class), "price": round(car.model.price()), "place": place }))
}
