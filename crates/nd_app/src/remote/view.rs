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
    }
}

pub fn rail_name(k: RailFacility) -> &'static str {
    match k {
        RailFacility::Station => "station",
        RailFacility::FreightYard => "freight_yard",
    }
}

pub fn transit_mode_name(m: TransitMode) -> &'static str {
    match m {
        TransitMode::Route => "route",
        TransitMode::Depot => "depot",
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
    }
}

pub fn gesture(id: GestureId, g: &Gesture) -> Value {
    match g {
        Gesture::Road { points: ps, lanes_fwd, lanes_bwd, curved, kind } => json!({
            "id": id.0, "kind": "road", "road_kind": road_kind_name(*kind), "points": points(ps),
            "lanes_fwd": lanes_fwd, "lanes_bwd": lanes_bwd, "curved": curved,
        }),
        Gesture::Zone { polygon, land_use } => json!({ "id": id.0, "kind": "zone", "land_use": land_use.label(), "points": points(polygon), "area": round(polygon_area(polygon).abs()) }),
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
    json!({
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
        "thought": c.thought, "spotlight": c.spotlight, "favourite": favourite,
        "memories": c.memories.iter().rev().map(|m| { let mut t = tick_time(m.tick); t["text"] = json!(m.text); t }).collect::<Vec<_>>(),
    })
}

// ── buildings ───────────────────────────────────────────────────────────────

pub fn building_row(w: &World, id: BuildingId, b: &Building) -> Value {
    json!({
        "id": key_str(id), "name": b.name, "land_use": b.land_use.label(), "tier": b.tier.label(), "floors": b.floors,
        "pos": w.lots.get(b.lot).map_or(Value::Null, |l| v2(l.center)), "lot": key_str(b.lot),
        "homes": b.homes, "residents": b.residents.len(), "jobs": b.jobs, "workers": b.workers.len(),
        "under_construction": b.construction.is_some(),
        "business": b.business.as_ref().map(|z| z.kind.label()), "plant": b.plant.map(plant_name),
        "depot": b.depot, "rail": b.rail.as_ref().map(|r| rail_name(r.kind)), "derelict": b.is_derelict(),
        "service": b.service.map(|s| format!("{}:{}", s.kind.key(), tier_key(s.tier))), "burning": w.services.is_burning(id),
    })
}

pub fn building_detail(w: &World, id: BuildingId, b: &Building, style: &str) -> Value {
    let lot = w.lots.get(b.lot);
    let construction = b.construction.as_ref().map(|c| {
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
        "land_value": round(b.land_value), "rent": round(b.rent), "wage": round(b.wage()), "condition": round(b.condition), "derelict": b.is_derelict(),
        "homes": b.homes, "jobs": b.jobs, "visitors": b.visitors,
        "residents": people(&b.residents), "workers": people(&b.workers),
        "footfall_today": lot.map(|l| l.footfall_today), "footfall_hour": lot.map(|l| l.footfall_hour), "car_access": lot.map(|l| l.car_access()),
        "powered": round(b.powered), "demand_mw": round(nd_sim::power::demand_mw(b)), "unpowered_hours_today": b.unpowered_hours_today,
        "parking": if b.parking_capacity > 0 { json!({ "capacity": b.parking_capacity, "used": b.parking_used, "price": round(b.parking_price), "public": b.is_parking() }) } else { Value::Null },
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
    if c.bus.is_some() {
        "bus"
    } else if c.cargo.is_some() {
        "truck"
    } else if w.citizens.contains_key(c.citizen) {
        "car"
    } else {
        "unknown"
    }
}

pub fn car_row(w: &World, id: CarId, c: &Car) -> Value {
    let kind = car_kind(w, c);
    let mut m = json!({
        "id": key_str(id), "kind": kind, "pos": v2(c.pos), "heading": round(c.heading), "speed_kmh": round(c.v * 3.6), "at_stop": c.hold > 0.0,
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
    m
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
    }
}

pub fn tier_key(t: nd_sim::Tier) -> &'static str {
    match t {
        nd_sim::Tier::Low => "low",
        nd_sim::Tier::Mid => "mid",
        nd_sim::Tier::High => "high",
    }
}
