//! JSON views of the simulation for the remote API: the id codec, field
//! parsers, and the row/detail serialisers shared by `find`, `inspect` and
//! `status`. Everything here is read-only; the command handlers (the
//! registry in `dispatch.rs`) do the mutating.

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
    let idx: u32 = idx.trim().parse().map_err(|_| format!("bad id {s:?}"))?;
    let ver: u32 = ver.trim().parse().map_err(|_| format!("bad id {s:?}"))?;
    // [fix/review-app] A live key's version is odd; slotmap would quietly
    // make an even one odd, so "42:2" named 42:3.
    if ver % 2 == 0 {
        return Err(format!("bad id {s:?}: no live id has an even version"));
    }
    Ok(K::from(KeyData::from_ffi((u64::from(ver) << 32) | u64::from(idx))))
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
        // [fix/review-app] Finite as an f32 too: 1e39 became infinity (an
        // infinite `grant` went into the save).
        Some(v) => match v.as_f64().map(|x| x as f32) {
            Some(x) if x.is_finite() => Ok(Some(x)),
            Some(_) => Err(format!("field {name:?} is out of range")),
            None => Err(format!("field {name:?} must be a number")),
        },
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
    if !(x.is_finite() && y.is_finite()) {
        return Err(format!("{v}: coordinates out of range"));
    }
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
        let n = |i: usize| a[i].as_f64().map(|x| x as f32).filter(|x| x.is_finite()).ok_or_else(|| format!("{v}: coordinates must be numbers in range"));
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
        Selection::Toll(id) => json!({ "kind": "toll", "id": id, "name": w.tolls.get(id).map(|b| b.name.as_str()) }),
        Selection::Road(sid) => json!({ "kind": "road", "id": sid.0, "name": crate::road_select::name(w, sid), "road_kind": crate::road_select::kind(w, sid) }),
        Selection::Citizen(id) => json!({ "kind": "citizen", "id": key_str(id), "name": w.citizens.get(id).map(|c| c.name.as_str()) }),
        Selection::Building(id) => json!({ "kind": "building", "id": key_str(id), "name": w.buildings.get(id).map(|b| b.name.as_str()) }),
        Selection::Car(id) => json!({ "kind": "car", "id": key_str(id), "label": w.car_label(id), "why": w.traffic.cars.contains_key(id).then(|| w.stop_reason_text(id)) }),
        Selection::Parked(id) => json!({ "kind": "parked_car", "id": key_str(id) }),
        Selection::Bot(id) => json!({ "kind": "bot", "id": key_str(id), "name": nd_sim::bots::bot_name(w, id) }),
        Selection::Train(id) => json!({ "kind": "train", "id": key_str(id), "headcode": w.rail.trains.get(id).map(|t| crate::train_select::headcode(id, t.kind)) }),
        Selection::Unit(id) => json!({ "kind": "unit", "id": key_str(id), "label": crate::fire_scene::unit_title(w, id), "doing": crate::fire_scene::unit_doing(w, id) }),
        Selection::Ship(id) => json!({ "kind": "ship", "id": key_str(id), "label": crate::ferry_select::ship_title(w, id), "doing": crate::ferry_select::ship_doing(w, id) }),
        Selection::Drone(id) => json!({ "kind": "drone", "id": key_str(id), "label": crate::air_select::title(w, id), "doing": crate::air_select::doing(w, id) }),
        Selection::Rv(seed) => json!({ "kind": "rv", "id": seed, "label": crate::rv_select::title(w, seed) }),
        Selection::Metro(id) => json!({ "kind": "metro_train", "id": key_str(id), "headcode": crate::train_select::metro_code(id), "line": w.subway.trains.get(id).and_then(|t| w.subway.lines.get(t.line)).map(|l| l.name.clone()) }),
    }
}

/// [feat/pt-ux] A drone taxi or helicopter: what it is, what it does,
/// where it is going and who is aboard.
pub fn drone_json(w: &World, id: nd_sim::air::DroneId) -> Value {
    let Some(d) = w.air.drones.get(id) else { return Value::Null };
    let (rider, crew) = crate::air_select::aboard(w, id);
    json!({
        "kind": "drone", "id": key_str(id), "craft": d.craft.label(),
        "label": crate::air_select::title(w, id), "doing": crate::air_select::doing(w, id),
        "phase": d.phase.label(), "job": d.job.label(),
        "base": building_name(w, Some(d.base)),
        "destination": crate::air_select::destination(w, id).map(|(n, p)| json!({ "name": n, "at": [p.x, p.y] })),
        "altitude": d.alt, "battery": d.battery,
        "aboard": rider.map(|c| json!({ "id": key_str(c), "name": w.citizens.get(c).map(|c| c.name.as_str()) })),
        "crew": crew.iter().map(|&c| json!({ "id": key_str(c), "name": w.citizens.get(c).map(|c| c.name.as_str()) })).collect::<Vec<_>>(),
    })
}

/// [feat/pt-ux] A camper or van at the kerb: who lives in it, or that it
/// was left behind, and its tow.
pub fn rv_json(w: &World, seed: u64) -> Value {
    let Some(r) = w.society.homeless.rv(seed) else { return Value::Null };
    json!({
        "kind": "rv", "id": seed, "label": crate::rv_select::title(w, seed),
        "vehicle": if r.camper { "camper" } else { "van" }, "at": [r.pos.x, r.pos.y],
        "abandoned": r.abandoned.is_some(), "abandoned_since": r.abandoned.map(tick_time),
        "owner": crate::rv_select::owner_name(w, r),
        "residents": crate::rv_select::residents(w, r).iter().map(|&c| json!({ "id": key_str(c), "name": w.citizens.get(c).map(|c| c.name.as_str()) })).collect::<Vec<_>>(),
        "tow_at": r.tow_at.map(tick_time),
    })
}

pub fn gesture(id: GestureId, g: &Gesture) -> Value {
    match g {
        Gesture::Road { points: ps, lanes_fwd, lanes_bwd, curved, kind, elevation, car_only } => json!({
            "id": id.0, "kind": "road", "road_kind": road_kind_name(*kind), "points": points(ps),
            "lanes_fwd": lanes_fwd, "lanes_bwd": lanes_bwd, "curved": curved, "use": nd_sim::roads::RoadUse::of(*kind, *lanes_fwd, *lanes_bwd, *car_only).key(), "use_set": car_only.is_some(),
            "elevation": (0..ps.len()).map(|i| elevation.get(i).copied().unwrap_or(0.0)).collect::<Vec<f32>>(),
        }),
        Gesture::Zone { polygon, land_use } => json!({ "id": id.0, "kind": "zone", "land_use": land_use.label(), "points": points(polygon), "area": round(polygon_area(polygon).abs()) }),
        Gesture::ParcelZone { parcels, land_use, industry } => json!({ "id": id.0, "kind": "parcel_zone", "land_use": land_use.map_or("unzoned", |l| l.label()), "parcels": parcels.len(), "industry": industry }),
        Gesture::Roundabout { center, radius, lanes, level } => json!({ "id": id.0, "kind": "roundabout", "center": [center.x, center.y], "radius": radius, "diameter": radius * 2.0, "lanes": lanes, "level": level, "points": points(std::slice::from_ref(center)) }),
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
        // [passengers] Riding along: whose car, where to.
        Activity::Passenger { driver, to, purpose, seated } => {
            m.insert("driver".into(), json!(key_str(*driver)));
            m.insert("driver_name".into(), json!(w.citizens.get(*driver).map(|d| d.name.as_str())));
            m.insert("in_car".into(), json!(seated));
            if let Some(Activity::Driving { car, .. }) = w.citizens.get(*driver).map(|d| &d.activity) {
                m.insert("car".into(), json!(key_str(*car)));
            }
            m.insert("to".into(), opt_key(*to));
            m.insert("to_name".into(), name_of(*to));
            m.insert("purpose".into(), json!(format!("{purpose:?}")));
        }
        // [ferries] Away for the day in a town off the map.
        Activity::Abroad { terminal, town, until } => {
            m.insert("terminal".into(), json!(key_str(*terminal)));
            m.insert("town".into(), json!(w.ferry_town_name(*terminal, *town)));
            m.insert("back_after_tick".into(), json!(until));
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
        // [economy] an artist's calling (the art scene)
        "artist": nd_sim::world::is_artist(c),
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
    // [tools] On a raised walkway (a footbridge), how high (outside the
    // literal above: it is at the json! macro's recursion limit).
    v["raised_level"] = json!(round(w.walker_level(c, c.pos)));
    // [footbridge] On an elevated path, viaduct or bridge: the deck's height
    // they are drawn at (null at grade).
    v["way_height"] = json!(w.walker_way_height(c, c.pos).map(round));
    v["appearance"] = appearance(c);
    // [clarity] What they are doing in words, and their pose outdoors.
    v["doing"] = json!(w.activity_line(id));
    v["pose"] = json!(format!("{:?}", w.pose_of(id)).to_lowercase());
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
        "id": key_str(id), "name": b.name, "land_use": w.shown_use(id).label(), "use": b.land_use.label(), "tier": b.tier.label(), "floors": b.floors,
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
            "reason": d.reason.key(), "why": d.reason.label(), "clear_tick": w.clock.after_work_hours(d.work_hours_left()),
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
            "blocked": c.blocked.map(|k| k.key()), "blocked_text": c.blocked.map(|k| k.text()),
            "crew": b.workers.len(), "crew_wanted": b.jobs, "planned_homes": homes, "planned_jobs": jobs,
        })
    });
    let rail = b.rail.as_ref().map(|l| {
        json!({
            "kind": rail_name(l.kind), "rail_connected": l.rail_connected, "road_connected": l.road_connected, "walk_connected": l.walk_connected, "works": l.works(),
            "platform": points(&l.platform), "inbound_orders": w.rail.orders_for(id).0,
            "inbound_units": round(w.rail.orders_for(id).1), "aboard_units": round(w.rail.aboard_for(id)), "outbound_units": round(b.rail_outbound),
        })
    });
    let business = b.business.as_ref().map(|z| {
        let recipe = z.kind.recipe();
        json!({
            "kind": z.kind.label(), "industry": z.kind.industry().map(|k| k.label()), "closed": z.closed, "money": round(z.money), "revenue_today": round(z.revenue_today),
            "books": z.books.iter().map(|d| json!({ "revenue": round(d.revenue), "spend": round(d.spend) })).collect::<Vec<_>>(),
            "spend_today": round(z.spend_today), "deals": z.deals, "staffing": round(b.staffing()), "broke_days": z.broke_days,
            "needs": recipe.inputs.iter().map(|(r, _)| json!({ "resource": r.label(), "stock": round(z.stock_of(*r)), "target": round(z.target_input(*r, b.jobs)) })).collect::<Vec<_>>(),
            "sells": recipe.outputs.iter().map(|(r, _)| json!({ "resource": r.label(), "stock": round(z.stock_of(*r)), "price": round(z.sell_price(*r, b.jobs)) })).collect::<Vec<_>>(),
        })
    });
    let plant = b.plant.map(|k| {
        // [plant-groups] This lot's own figures (sized to it), and its park.
        let group = w.plant_group_of(id).map(|g| {
            let members: Vec<&nd_sim::Building> = g.members.iter().filter_map(|m| w.buildings.get(*m)).collect();
            json!({
                "name": g.name, "anchor": key_str(g.anchor()), "members": g.members.iter().map(|m| key_str(*m)).collect::<Vec<_>>(),
                "member_names": members.iter().map(|m| m.name.clone()).collect::<Vec<_>>(), "lots": g.lots.iter().map(|l| key_str(*l)).collect::<Vec<_>>(),
                "area_m2": round(g.area), "capacity_mw": round(members.iter().map(|m| w.plant_capacity_mw(m)).sum::<f32>()),
                "output_mw": round(members.iter().map(|m| w.plant_dispatched_mw(m)).sum::<f32>()), "upkeep_per_day": round(members.iter().map(|m| w.plant_upkeep(m)).sum::<f32>()),
                "bonus": round(nd_sim::world::plant_groups::group_bonus(g.members.len())),
            })
        });
        let s = w.plant_resource(b);
        json!({
            "kind": plant_name(k), "label": k.label(), "capacity_mw": round(w.plant_capacity_mw(b)), "availability": round(w.plant_availability(b)),
            "upkeep_per_day": round(w.plant_upkeep(b)), "output_mw": round(w.plant_dispatched_mw(b)), "area_m2": round(lot.map_or(0.0, |l| l.area())), "size_scale": round(w.plant_scale(b)),
            "group": group, "site": { "wind_factor": round(s.wind), "sun_factor": round(s.sun) },
        })
    });
    let park = lot.and_then(|l| l.park.as_ref()).map(|p| {
        json!({ "benches": p.benches.len(), "paths": p.paths.len(), "entrances": p.entrances.len(), "features": p.features.iter().map(|f| f.kind.label()).collect::<Vec<_>>(), "cluster_lots": p.cluster_lots, "cluster_area": round(p.cluster_area) })
    });
    let people = |ids: &[CitizenId]| ids.iter().filter_map(|&c| w.citizens.get(c).map(|x| json!({ "id": key_str(c), "name": x.name }))).collect::<Vec<_>>();
    let mut v = json!({
        "id": key_str(id), "name": b.name, "land_use": w.shown_use(id).label(), "tier": b.tier.label(), "style": style, "floors": b.floors,
        "lot": key_str(b.lot), "pos": lot.map_or(Value::Null, |l| v2(l.center)), "built_day": b.built_tick / 1440 + 1,
        "under_construction": construction,
        "demolition": demolition,
        "look": b.look.map(|l| l.label()), "district": w.district_of_lot(b.lot).map(|d| d.name.clone()),
        "land_value": round(b.land_value), "rent": round(b.rent), "wage": round(b.wage()), "condition": round(b.condition), "derelict": b.is_derelict(),
        "homes": b.homes, "jobs": b.jobs, "visitors": b.visitors,
        "residents": people(&b.residents), "workers": people(&b.workers),
        "footfall_today": lot.map(|l| l.footfall_today), "footfall_hour": lot.map(|l| l.footfall_hour), "car_access": lot.map(|l| l.car_access()),
        "powered": round(b.powered), "demand_mw": round(nd_sim::power::demand_mw(b)), "demand_now_mw": round(nd_sim::power::demand_at(b, w.grid_hour(), w.weather.temp_c)), "unpowered_hours_today": b.unpowered_hours_today,
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
        "taxis_and_freight": super::taxis::building_extra(w, id),
        // [economy] why the business is here and how it is doing
        "story": super::economy::story(w, id),
    });
    // [free-placement] The building's own use (power, wages, venues); `land_use` is Civic for a city building.
    v["use"] = json!(b.land_use.label());
    // [w1] A site: which way it is going and what will stand there.
    v["site"] = w.site_plan(id).map_or(Value::Null, |p| {
        json!({
            "kind": p.kind.key(), "progress": round(p.progress), "coming_down_floors": p.coming_down_floors,
            "next": p.next.map(|n| json!({ "building": key_str(n.building), "name": n.name, "land_use": n.land_use.label(), "tier": n.tier.label(), "floors": n.floors, "height_m": round(n.height_m), "joins": n.joins })),
        })
    });
    // [attractions] A landmark: its hours, visitors, takings, pull and site.
    if let Some(k) = b.service.map(|s| s.kind).filter(|k| k.is_landmark()) {
        let book = w.attractions.book(id);
        let hours = k.hours();
        let (r, wt) = k.land_value(nd_sim::Tier::Mid);
        v["attraction"] = json!({
            "kind": k.key(), "label": k.label(), "about": nd_sim::attractions::about(k),
            "open_hours": if hours.always() { "always".to_string() } else { format!("{:02}-{:02}", hours.open, hours.close) },
            "open_now": b.operating() && hours.contains(w.clock.hour() as u8) && b.workers.len() * 4 >= b.jobs as usize,
            "capacity": k.capacity(nd_sim::Tier::Mid), "admission": round(k.admission()),
            "visits_today": book.visits_today, "visits_yesterday": book.visits_yesterday,
            "tourists_today": book.tourists_today, "tourists_yesterday": book.tourists_yesterday,
            "takings_today": round(book.takings_today), "takings_yesterday": round(book.takings_yesterday), "visits_total": book.visits_total,
            "tourist_pull": round(k.tourist_pull() * (0.5 + 0.5 * b.staffing())),
            "land_value_lift": { "radius_m": r, "raw_weight": round(wt * (0.55 + 0.45 * b.staffing())) },
            "pier": w.attractions.piers.get(&id).map(|d| json!({ "root": v2(d.root), "tip": v2(d.tip), "length_m": round(d.length()), "width_m": d.width, "head_m": d.head })),
            "rise_above_town_m": lot.and_then(|l| w.rise_above_town(l.center)).map(round),
        });
    }
    // [dock-access] A dock, station or yard: on the road network or not, and
    // where to draw a road to join it.
    if let Some(connected) = w.road_access(id) {
        v["road_access"] = access_json(w, id, connected);
    }
    if let Some(p) = w.ports.port_of(id) {
        v["dock"] = json!({ "kind": p.kind.label(), "road_connected": p.road_connected, "walk_connected": p.walk_connected, "reaches_elsewhere": p.reaches_elsewhere, "works": p.works(), "ferries_sail": p.sails(), "berth": v2(p.berth), "stub": points(&p.road_line()) });
    }
    v["dirt"] = dirt_json(w, id);
    // [waterfront] its beach, its jetty, its edge on the water
    super::shore::building_extra(w, id, &mut v);
    // [fishing] A fishing port's boats and grounds.
    v["fishing"] = super::economy::fishing(w, id);
    v
}

/// [dirty-industry] A building and dirty industry: how dirty it is itself,
/// its unhappy neighbours (worst first) and what they cost it, and the
/// ground under it.
pub fn dirt_json(w: &World, id: BuildingId) -> Value {
    let Some(b) = w.buildings.get(id) else { return Value::Null };
    let me = w.dirty.iter().find(|s| s.building == id).map(|s| {
        json!({ "level": round(s.level), "reach_m": round(s.reach), "reads_as": s.as_kind.label(), "spill": b.business.as_ref().and_then(|z| z.kind.spill()).or_else(|| b.plant.and_then(|k| k.ground_spill())).map(|(r, a)| json!({ "radius_m": r, "per_hour": a })) })
    });
    let neighbours: Vec<Value> = w.unhappy_neighbours(id).into_iter().map(|n| json!({ "building": key_str(n.source), "name": n.name, "what": n.what, "distance_m": round(n.distance), "hit": round(n.hit) })).collect();
    let contamination = w.lots.get(b.lot).map_or(0.0, |l| w.contamination.over(l.center, &l.polygon));
    json!({
        "dirty": me, "dirtiness": round(b.business.as_ref().map_or(0.0, |z| z.kind.dirtiness())),
        "as_neighbour": w.neighbour_use(id).map(|n| n.label()), "exposure": round(w.building_exposure(id)),
        "unhappy_neighbours": neighbours, "costs": w.adjacency_cost(id), "ground_contamination": round(contamination),
    })
}

// ── lots ────────────────────────────────────────────────────────────────────

pub fn lot_row(w: &World, id: LotId, l: &Lot) -> Value {
    json!({
        "id": key_str(id), "land_use": l.land_use.label(), "pos": v2(l.center), "area": round(l.area()),
        "building": opt_key(l.building), "vacant": l.building.is_none() && l.usable(), "landlocked": l.landlocked,
        "park": l.park.is_some(), "land_value": round(w.land_value.at(l.center)), "car_access": l.car_access(),
        "city_ground": w.is_city_ground(l),
        "waterfront": l.shore.is_waterfront(), "water_frontage_m": round(l.shore.frontage), "beach": l.shore.beach,
    })
}

pub fn lot_detail(w: &World, id: LotId, l: &Lot) -> Value {
    let value = w.land_value.at(l.center);
    let zone = w.zones.get(l.zone).map(|z| json!({ "id": key_str(l.zone), "land_use": z.land_use.label(), "area": round(polygon_area(&z.polygon).abs()) }));
    json!({
        "id": key_str(id), "land_use": l.land_use.label(), "city_ground": w.is_city_ground(l), "pos": v2(l.center), "polygon": points(&l.polygon), "area": round(l.area()),
        "width": round(l.width()), "depth": round(l.depth()), "entrance": v2(l.entrance()), "front_dir": v2(l.front_dir),
        "front_segment": l.front_segment.0, "car_segment": l.car_segment.map(|s| s.0),
        "building": opt_key(l.building), "building_name": building_name(w, l.building),
        "vacant": l.building.is_none() && l.usable(), "usable": l.usable(), "landlocked": l.landlocked, "car_access": l.car_access(),
        "park": l.park.is_some(), "land_value": round(value), "land_tier": Tier::for_land_value(value).label(),
        "land_value_for_use": round(w.lot_value(id)), "access": w.lot_walk_access(id).map(|a| json!({ "park_min": a.park_min.map(round), "nightlife_min": a.nightlife_min.map(round), "value_for_homes": round(a.value), "text": w.lot_access_text(id).unwrap_or_else(|| a.describe()) })),
        "links": w.lot_link_warnings(id),
        "footfall_today": l.footfall_today, "footfall_hour": l.footfall_hour, "zone": zone,
        // [waterfront] its edge on the water, and a beach's state
        "shore": super::shore::lot_shore(w, id),
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
        // [passengers] Everyone riding along besides the driver.
        m["occupants"] = json!(c.occupants.iter().filter_map(|&p| w.citizens.get(p).map(|x| json!({ "id": key_str(p), "name": x.name }))).collect::<Vec<_>>());
        m["vehicle"] = json!(c.owned.map(key_str));
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
    // [services] A recovery truck going to a wreck, and a wreck waiting.
    if let nd_sim::Errand::Recover(wreck) = c.errand {
        let rec = w.driving.wrecks.iter().find(|x| x.car == wreck);
        m["recovery"] = json!({ "wreck": key_str(wreck), "metres_to_go": w.traffic.cars.get(wreck).map(|x| round(x.pos.distance(c.pos))), "amber": c.drive.amber, "off_law_allowed": c.drive.recovering, "move": format!("{:?}", c.drive.offlaw.mode), "company": rec.and_then(|r| r.company).map(key_str), "out_of_town": rec.is_some_and(|r| r.company.is_none()), "sent_tick": rec.map(|r| r.sent) });
    }
    if let Some(r) = w.driving.wrecks.iter().find(|x| x.car == id) {
        m["wreck"] = json!({ "since_tick": r.since, "incident": r.incident, "tow_due_tick": (r.due > 0).then_some(r.due), "truck": r.truck.map(key_str), "truck_sent_tick": r.truck.map(|_| r.sent), "pushed_aside_m": round(c.lateral), "flipped": r.flipped, "righted_at_tick": (r.righted_at > 0).then_some(r.righted_at) });
    }
    // [driving] Easing past a wreck on the shoulder.
    if c.passing_hazard() {
        m["passing_hazard"] = json!(c.drive.offlaw.hazard.map(key_str));
    }
    m["traffic"] = car_traffic(w, c);
    m["label"] = json!(w.car_label(id));
    m["why"] = stop_reason_json(w, id);
    m["jam"] = jam_json(w, id);
    m["trip"] = trip_json(w, id, c);
    m["lanes"] = lanes_json(w, c);
    m["driver"] = driver_json(w, c);
    // [inspector] Everyone aboard, as the inspector lists them.
    m["occupants"] = json!(crate::ui::occupants(w, id).iter().filter_map(|&p| w.citizens.get(p).map(|x| json!({ "id": key_str(p), "name": x.name }))).collect::<Vec<_>>());
    m["journey"] = journey_json(w, id);
    m
}

/// [clarity] A vehicle's journey as the inspector shows it: `purpose`,
/// `from` and `to` ({label, building, car, pos}; `to` null when it has no
/// destination, like a police car patrolling), and `at_wheel`: a citizen,
/// a crew of citizens, or `unknown` (a stranger from out of town).
pub fn journey_json(w: &World, id: CarId) -> Value {
    let Some(j) = w.car_journey(id) else { return Value::Null };
    let place = |p: &nd_sim::world::Place| json!({ "label": p.label, "building": p.building.map(key_str), "car": p.car.map(key_str), "pos": p.pos.map(v2) });
    let wheel = match &j.driver {
        nd_sim::world::Driver::Citizen(c) => json!({ "citizen": key_str(*c), "name": w.citizens.get(*c).map(|x| x.name.clone()) }),
        nd_sim::world::Driver::Crew(crew) => json!({ "crew": crew.iter().filter_map(|&c| w.citizens.get(c).map(|x| json!({ "id": key_str(c), "name": x.name, "work": x.work.map(key_str) }))).collect::<Vec<_>>() }),
        nd_sim::world::Driver::Unknown(who) => json!({ "unknown": who }),
    };
    json!({ "purpose": j.purpose, "from": place(&j.from), "to": j.to.as_ref().map(place), "at_wheel": wheel })
}

/// [inspector] The driver card: who is at the wheel of a private car, why
/// and where they drive, and their driving style, with `boldness` the
/// inspector's slow (-1) .. normal (0) .. reckless (1) slider, now and calm.
pub fn driver_json(w: &World, c: &Car) -> Value {
    let p = &c.drive.profile;
    let style = json!({
        "label": p.label(), "boldness": round(p.boldness()), "calm_boldness": c.drive.calm.map(|x| round(x.boldness())),
        "skill": round(p.skill), "speed": round(p.speed), "headway_s": round(p.headway), "aggression": round(p.aggression), "compliance": round(p.compliance), "drunk": round(p.drunk),
    });
    let Some(x) = w.citizens.get(c.citizen).filter(|_| c.bus.is_none() && c.cargo.is_none()) else { return json!({ "citizen": Value::Null, "style": style }) };
    let (to, purpose) = match &x.activity {
        Activity::Driving { to, purpose, .. } => (*to, Some(format!("{purpose:?}"))),
        _ => (None, None),
    };
    json!({
        "citizen": key_str(c.citizen), "name": x.name, "age": x.age, "mood": round(x.mood), "class": x.class().label(),
        "purpose": purpose, "destination": to.map(key_str), "destination_name": to.and_then(|b| w.buildings.get(b)).map(|b| b.name.clone()),
        "anger": round(c.drive.anger), "style": style,
    })
}

/// Why a vehicle is held up: `reason` (a key), `text` (plain language,
/// naming the vehicle responsible), and that vehicle's id and label.
pub fn stop_reason_json(w: &World, id: CarId) -> Value {
    let r = w.traffic.stop_reason(id);
    let by = r.by();
    // [bridge-riders] Stopping for people: `by` names the person (the one
    // nearest the vehicle) and `by_kind` says which it is.
    let person = w.stopped_for(id);
    let (by, by_label, by_kind) = match (by, person) {
        (Some(b), _) => (Some(key_str(b)), Some(w.car_label(b)), Some("vehicle")),
        (None, Some((p, _))) => (Some(key_str(p)), Some(w.person_label(p)), Some("citizen")),
        (None, None) => (None, None, None),
    };
    json!({
        "reason": r.key(), "text": w.stop_reason_text(id), "good_reason": r.is_good(),
        "by": by, "by_label": by_label, "by_kind": by_kind,
        "people_on_crossing": person.map(|(_, n)| n),
        "stalled_s": w.traffic.cars.get(id).map(|c| round(c.stalled)),
    })
}

/// The queue in front of a vehicle back to its head (`Traffic::jam_chain`):
/// each car's id, label, reason and speed, the head's reason, and whether
/// it is a gridlock ring.
pub fn jam_json(w: &World, id: CarId) -> Value {
    let chain = w.traffic.jam_chain(id);
    let head = chain.head();
    let rows: Vec<Value> = chain.cars.iter().take(60).map(|&c| {
        let car = &w.traffic.cars[c];
        json!({ "id": key_str(c), "label": w.car_label(c), "reason": w.traffic.stop_reason(c).key(), "speed_kmh": round(car.v * 3.6), "pos": v2(car.pos), "stalled_s": round(car.stalled) })
    }).collect();
    json!({
        "length": chain.cars.len(), "cycle": chain.cycle,
        "head": head.map(key_str), "head_label": head.map(|h| w.car_label(h)),
        "head_reason": head.map(|h| stop_reason_json(w, h)),
        "chain": rows, "chain_truncated": chain.cars.len() > 60,
    })
}

/// [rage] The drive so far against what it should take, and the driver's
/// temper.
pub fn trip_json(w: &World, id: CarId, c: &Car) -> Value {
    let d = &c.drive;
    let (elapsed, expected) = w.car_trip_minutes(id).unwrap_or((0.0, 0.0));
    json!({
        "timed": expected > 0.0, "started_tick": d.trip_start, "elapsed_min": round(elapsed), "expected_min": round(expected),
        "over_ratio": if expected > 0.0 { json!(round(elapsed / expected)) } else { Value::Null },
        "can_rage": d.calm.is_some(), "anger": round(d.anger),
        "anger_cause": if d.anger <= 0.02 { Value::Null } else if d.sore_loser { json!("lost_race") } else { json!("late") }, "anger_peak": round(d.anger_peak), "patience": round(d.patience), "temper": round(d.temper),
        "angry_profile": d.anger > 0.0,
    })
}

/// Where on the lane graph a vehicle is: its lane, how far along, the
/// lanes ahead on its route and where the route ends.
pub fn lanes_json(w: &World, c: &Car) -> Value {
    let net = &w.traffic.net;
    let lane_kind = |l: u32| net.lanes.get(l as usize).map(|x| match x.kind {
        nd_sim::traffic::LaneKind::Road { segment, forward, index } => json!({ "lane": l, "road": segment.0, "forward": forward, "index": index, "length_m": round(x.length), "cars": x.cars.len() }),
        nd_sim::traffic::LaneKind::Connector { node, .. } => json!({ "lane": l, "junction_node": node.0, "turn": x.turn, "green": x.green, "length_m": round(x.length), "cars": x.cars.len() }),
    });
    let here = c.route.get(c.idx).copied();
    json!({
        "lane": here.and_then(lane_kind), "s_m": round(c.s), "s_end_m": round(c.s_end), "route_idx": c.idx, "route_len": c.route.len(),
        "ahead": c.route.iter().skip(c.idx + 1).take(12).filter_map(|&l| lane_kind(l)).collect::<Vec<_>>(),
        "route_lanes": c.route.iter().skip(c.idx).take(200).collect::<Vec<_>>(),
        "ends_at": c.route.last().and_then(|&l| net.lanes.get(l as usize)).map(|l| v2(l.point_at(c.s_end.min(l.length)).0)),
        "entering_from_kerb": c.entering, "lateral_m": round(c.lateral), "blocked_by": c.blocked_by.map(key_str),
    })
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
        "takeover": c.drive.gang.map(|g| json!({ "takeover": g.takeover, "phase": format!("{:?}", g.phase).to_lowercase(), "race": g.race, "moto_pack": g.pack })),
        "stunt": c.drive.stunt.map(|s| if s.is_donut() { "donut" } else if matches!(s.kind, nd_sim::traffic::StuntKind::Manoeuvre { .. }) { "manoeuvre" } else { "ring" }),
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
        "id": key_str(id), "headcode": crate::train_select::headcode(id, t.kind), "kind": match t.kind { TrainKind::Passenger => "passenger", TrainKind::Freight => "freight" },
        "cars": t.cars, "pos": v2(head), "speed": round(t.speed), "held": t.held, "dwell": round(t.dwell),
        "passengers": t.passengers.len(), "cargo_units": round(t.cargo.iter().map(|c| c.amount).sum::<f32>()), "exports_units": round(t.exports),
        "capacity_units": if t.kind == TrainKind::Freight { round(t.freight_capacity()) } else { 0.0 },
        "next_stop": opt_key(next), "next_stop_name": building_name(w, next), "stops": t.stops.len(),
    })
}

pub fn route_row(id: RouteId, r: &nd_sim::BusRoute) -> Value {
    json!({
        "id": key_str(id), "name": r.name, "color": r.color, "buses": r.buses, "fare": round(r.fare), "riders_today": r.riders_today,
        "riders_yesterday": r.boardings_yesterday(),
        "stops": r.stops.iter().map(|s| json!({ "pos": v2(s.pos), "segment": s.segment.0, "on_today": s.stats.on_today, "off_today": s.stats.off_today, "back": s.back })).collect::<Vec<_>>(),
        "broken_legs": r.broken, "legs": r.legs.len(), "two_way": r.two_way, "outbound_stops": r.outbound_len(),
    })
}

/// [lines] One bus line as the Lines window shows it: per stop its
/// boardings and alightings (today, yesterday), the leg to the next stop
/// (metres, minutes and whether the buses timed it or it is an estimate),
/// and every bus on the line with where it is along the loop (progress in
/// stops: 2.4 is 40% of the way from the third stop to the fourth).
pub fn line_detail(w: &World, id: RouteId) -> Value {
    let Some(r) = w.transit.routes.get(id) else { return Value::Null };
    let tsp = w.config.traffic_seconds_per_tick;
    let n = r.stops.len();
    let stops: Vec<Value> = r
        .stops
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let (minutes, timed) = r.leg_minutes(i, tsp);
            json!({
                "index": i, "pos": v2(s.pos),
                "on_today": s.stats.on_today, "off_today": s.stats.off_today, "on_yesterday": s.stats.on_yesterday, "off_yesterday": s.stats.off_yesterday,
                "leg_to": (i + 1) % n.max(1), "leg_m": round(r.leg_length(i)), "leg_min": round(minutes), "leg_timed": timed, "leg_runs": s.stats.leg_runs,
                "leg_broken": r.broken.contains(&i),
            })
        })
        .collect();
    let buses: Vec<Value> = w
        .traffic
        .cars
        .iter()
        .filter_map(|(cid, c)| c.bus.as_ref().filter(|b| b.route == id).map(|b| (cid, c, b)))
        .map(|(cid, c, b)| {
            let state = if b.next_stop == usize::MAX { "to_depot" } else if !w.transit.departures.contains_key(&cid) { "from_depot" } else if b.to_depot { "last_lap" } else { "on_line" };
            json!({
                "id": key_str(cid), "state": state, "next_stop": (b.next_stop != usize::MAX).then_some(b.next_stop),
                "progress": if state == "on_line" || state == "last_lap" { r.bus_progress(b.next_stop, c.pos).map(round) } else { None },
                "passengers": b.passengers.len(), "pos": v2(c.pos), "speed_kmh": round(c.v * 3.6),
            })
        })
        .collect();
    let mut v = route_row(id, r);
    v["length_m"] = json!(round((0..n).map(|i| r.leg_length(i)).sum::<f32>()));
    v["lap_min"] = json!(round((0..n).map(|i| r.leg_minutes(i, tsp).0).sum::<f32>()));
    v["stops"] = json!(stops);
    v["buses_out"] = json!(buses);
    v["headway_min"] = json!(r.headway(tsp));
    v["problems"] = line_problems_json(&w.line_problems(id));
    v["books"] = json!({
        "riders_today": r.riders_today, "riders_yesterday": r.riders_yesterday,
        "fares_today": round(r.fares_today), "fares_yesterday": round(r.fares_yesterday),
        "upkeep_yesterday": round(r.upkeep_yesterday), "net_yesterday": round(r.fares_yesterday - r.upkeep_yesterday),
        "bus_minutes_today": r.bus_minutes_today,
    });
    v
}

/// [pt-transit] A line's problems (`World::line_problems`,
/// `Transit::check_stops`): kind, the leg it is on (stop index it leaves
/// from), whether it stops the line running, and the text.
pub fn line_problems_json(problems: &[nd_sim::LineProblem]) -> Value {
    json!(problems.iter().map(|p| json!({ "kind": p.kind(), "leg": p.leg(), "blocks": p.blocks(), "text": p.to_string() })).collect::<Vec<_>>())
}

/// The remote-API name of something the Buildings tool places: a plant
/// name, `depot`, `station`, `freight_yard`, or `<service>:<tier>`.
pub fn placeable_key(p: nd_sim::Placeable) -> String {
    match p {
        nd_sim::Placeable::Plant(k) => plant_name(k).to_string(),
        nd_sim::Placeable::Battery(k) => k.key().to_string(),
        nd_sim::Placeable::Service(k, t) => format!("{}:{}", k.key(), tier_key(t)),
        nd_sim::Placeable::BusDepot => "depot".to_string(),
        nd_sim::Placeable::Rail(f) => rail_name(f).to_string(),
        nd_sim::Placeable::Dock(nd_sim::PortKind::ShippingPort) => "shipping_port".to_string(),
        nd_sim::Placeable::Dock(nd_sim::PortKind::FerryTerminal) => "ferry_terminal".to_string(),
        nd_sim::Placeable::Dock(nd_sim::PortKind::FerryTerminalLarge) => "ferry_terminal_large".to_string(),
        nd_sim::Placeable::Dock(nd_sim::PortKind::MarinaSmall) => "marina_small".to_string(),
        nd_sim::Placeable::Dock(nd_sim::PortKind::MarinaLarge) => "marina_large".to_string(),
        nd_sim::Placeable::Dock(nd_sim::PortKind::FishingPortSmall) => "fishing_port_small".to_string(),
        nd_sim::Placeable::Dock(nd_sim::PortKind::FishingPortLarge) => "fishing_port_large".to_string(),
        nd_sim::Placeable::ParkingLot(s) => s.key().to_string(),
        nd_sim::Placeable::MetroStation => "metro_station".to_string(),
        nd_sim::Placeable::Toll(s) => s.key().to_string(),
        nd_sim::Placeable::CargoHub => "cargo_hub".to_string(),
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
        // [boot] Clamped on the city's orders, waiting for the tow truck.
        v["booted"] = json!(s.booted);
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

/// [dock-access] A dock's or station's road connection for the API: whether
/// it is on the network, the free ends of its street stub to draw a road
/// onto, and what is held up meanwhile.
pub fn access_json(w: &World, id: nd_sim::BuildingId, connected: bool) -> Value {
    json!({
        "connected": connected,
        // [sweep-sim] Reached on foot (a path) and whether that is enough.
        "serves": w.access_serves(id),
        "footpath_only": w.footpath_only(id),
        "note": w.access_note(id),
        "stub_ends": w.stub_ends_of(id).into_iter().map(v2).collect::<Vec<_>>(),
        "problem": w.access_problem(id),
    })
}

// ── find / inspect / select ─────────────────────────────────────────────────

pub fn str_filter(filter: Option<&Value>, name: &str) -> Option<String> {
    filter.and_then(|f| f.get(name)).and_then(Value::as_str).map(|s| s.to_ascii_lowercase())
}

pub fn bool_filter(filter: Option<&Value>, name: &str) -> Option<bool> {
    filter.and_then(|f| f.get(name)).and_then(Value::as_bool)
}

/// Milliseconds since `start`, to two decimals.
pub fn ms(start: std::time::Instant) -> f64 {
    (start.elapsed().as_secs_f64() * 100_000.0).round() / 100.0
}

/// An optional array of strings, e.g. `sections`, `keys`, `fields`.
pub fn opt_strings(cmd: &Value, name: &str) -> Result<Option<Vec<String>>, String> {
    match field(cmd, name) {
        None => Ok(None),
        Some(Value::Array(a)) => a.iter().map(|v| v.as_str().map(str::to_string).ok_or_else(|| format!("{name:?} must be an array of strings"))).collect::<Result<Vec<_>, _>>().map(Some),
        Some(Value::String(s)) => Ok(Some(vec![s.clone()])),
        Some(_) => Err(format!("{name:?} must be an array of strings")),
    }
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    /// [fix/review-app] Numbers that do not fit an f32 and ids that do not
    /// name a live key are refused, not wrapped or rounded.
    #[test]
    fn out_of_range_numbers_and_bad_ids_are_refused() {
        assert_eq!(opt_f32(&json!({ "amount": 5.5 }), "amount"), Ok(Some(5.5)));
        assert!(opt_f32(&json!({ "amount": 1e39 }), "amount").is_err());
        assert!(opt_f32(&json!({ "amount": -1e39 }), "amount").is_err());
        assert!(vec2_of(&json!([1e39, 0])).is_err());
        assert!(req_points_levels(&json!({ "p": [[0, 0], [1, 1e40]] }), "p").is_err());
        let ok: Result<CarId, String> = key_from(&json!("42:3"));
        assert_eq!(ok.map(key_str), Ok("42:3".to_string()));
        assert!(key_from::<CarId>(&json!("42:2")).is_err(), "even versions are never live");
        assert!(key_from::<CarId>(&json!("4294967296:1")).is_err(), "index past u32");
        assert!(key_from::<CarId>(&json!("1:4294967297")).is_err(), "version past u32");
    }
}
