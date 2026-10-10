//! `find`, `inspect`, `select` and `selected`: what is where, and
//! everything about one thing.

use nd_sim::{GestureId, Vec2 as SVec2};
use serde_json::{json, Value};

use super::ctx::Ctx;
use super::view::*;
use super::{corp, ferry, follow, nights, places, property, road, skyline, social, tolls, trains, units};
use crate::assets::GameAssets;
use crate::tools::Selection;

pub(super) fn find(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let what = req_str(cmd, "what")?;
    let near = opt_vec2(cmd, "near")?;
    let radius = opt_f32(cmd, "radius")?.unwrap_or(if near.is_some() { 100.0 } else { f32::INFINITY });
    let limit = opt_u64(cmd, "limit")?.unwrap_or(50) as usize;
    let filter = field(cmd, "filter");
    let w = &ctx.sim.world;
    // (distance, row) so results can be sorted nearest-first when asked;
    // a row is built only if it is returned (`limit` keeps the first).
    let mut rows: Vec<(f32, Box<dyn FnOnce() -> Value + '_>)> = Vec::new();
    let within = |p: SVec2| -> Option<f32> {
        let d = near.map_or(0.0, |n| n.distance(p));
        (d <= radius).then_some(d)
    };
    let contains = |hay: &str, needle: &Option<String>| needle.as_ref().is_none_or(|n| hay.to_ascii_lowercase().contains(n));
    match what {
        "citizens" | "citizen" | "people" => {
            let f_activity = str_filter(filter, "activity");
            let f_name = str_filter(filter, "name");
            let f_class = str_filter(filter, "class").or(str_filter(filter, "tier"));
            let f_visible = bool_filter(filter, "visible");
            let f_spot = bool_filter(filter, "spotlight");
            for (id, c) in w.citizens.iter() {
                if !contains(c.activity.label(), &f_activity) || !contains(&c.name, &f_name) || !contains(c.class().label(), &f_class) {
                    continue;
                }
                if f_visible.is_some_and(|v| v != c.is_visible()) || f_spot.is_some_and(|v| v != c.spotlight) {
                    continue;
                }
                if let Some(d) = within(c.pos) {
                    rows.push((d, Box::new(move || citizen_row(w, id, c))));
                }
            }
        }
        "buildings" | "building" => {
            let f_lu = str_filter(filter, "land_use");
            let f_tier = str_filter(filter, "tier");
            let f_biz = str_filter(filter, "business_kind").or(str_filter(filter, "business"));
            let f_name = str_filter(filter, "name");
            let f_site = bool_filter(filter, "under_construction");
            let f_plant = bool_filter(filter, "plant");
            let f_rail = bool_filter(filter, "rail");
            let f_depot = bool_filter(filter, "depot");
            let f_derelict = bool_filter(filter, "derelict");
            for (id, b) in w.buildings.iter() {
                if !contains(b.land_use.label(), &f_lu) || !contains(b.tier.label(), &f_tier) || !contains(&b.name, &f_name) {
                    continue;
                }
                if f_biz.is_some() && !b.business.as_ref().is_some_and(|z| contains(z.kind.label(), &f_biz)) {
                    continue;
                }
                if f_site.is_some_and(|v| v != b.construction.is_some()) || f_plant.is_some_and(|v| v != b.plant.is_some()) || f_rail.is_some_and(|v| v != b.rail.is_some()) || f_depot.is_some_and(|v| v != b.depot) || f_derelict.is_some_and(|v| v != b.is_derelict()) {
                    continue;
                }
                let Some(lot) = w.lots.get(b.lot) else { continue };
                if let Some(d) = within(lot.center) {
                    rows.push((d, Box::new(move || building_row(w, id, b))));
                }
            }
        }
        "lots" | "lot" => {
            let f_lu = str_filter(filter, "land_use");
            let f_vacant = bool_filter(filter, "vacant");
            let f_park = bool_filter(filter, "park");
            let min_area = filter.and_then(|f| f.get("min_area")).and_then(Value::as_f64).unwrap_or(0.0) as f32;
            for (id, l) in w.lots.iter() {
                if !contains(l.land_use.label(), &f_lu) || l.area() < min_area {
                    continue;
                }
                if f_vacant.is_some_and(|v| v != (l.building.is_none() && l.usable())) || f_park.is_some_and(|v| v != l.park.is_some()) {
                    continue;
                }
                if let Some(d) = within(l.center) {
                    rows.push((d, Box::new(move || lot_row(w, id, l))));
                }
            }
        }
        "cars" | "car" | "vehicles" => {
            let f_kind = str_filter(filter, "kind");
            for (id, c) in w.traffic.cars.iter() {
                if !contains(car_kind(w, c), &f_kind) {
                    continue;
                }
                if let Some(d) = within(c.pos) {
                    rows.push((d, Box::new(move || car_row(w, id, c))));
                }
            }
        }
        "trains" | "train" => {
            let f_kind = str_filter(filter, "kind");
            for (id, t) in w.rail.trains.iter() {
                let row = train_row(w, id, t);
                if !contains(row["kind"].as_str().unwrap_or(""), &f_kind) {
                    continue;
                }
                let head = nd_sim::rail::point_along(&t.poly, t.s).0;
                if let Some(d) = within(head) {
                    rows.push((d, Box::new(move || row)));
                }
            }
        }
        // [train-inspect] Metro trains in their tunnels.
        "metro_trains" | "metro_train" | "metros" => {
            for (id, t) in w.subway.trains.iter() {
                if let Some(d) = within(t.head()) {
                    rows.push((d, Box::new(move || trains::metro_row(w, id))));
                }
            }
        }
        // [camera] Pavement delivery robots; `filter.shop` matches the shop's name.
        "bots" | "bot" | "delivery_bots" => {
            let f_shop = str_filter(filter, "shop");
            for (id, b) in w.bots.bots.iter() {
                let shop = w.buildings.get(b.shop).map_or("", |s| s.name.as_str());
                if !contains(shop, &f_shop) {
                    continue;
                }
                if let Some(d) = within(b.pos) {
                    rows.push((d, Box::new(move || follow::bot_row(w, id))));
                }
            }
        }
        "drones" | "drone" | "aircraft" => {
            let f_kind = str_filter(filter, "kind");
            for (id, d) in w.air.drones.iter() {
                if !contains(d.craft.label(), &f_kind) {
                    continue;
                }
                if let (Some(dist), Some(row)) = (within(d.pos), drone_detail(w, id)) {
                    rows.push((dist, Box::new(move || row)));
                }
            }
        }
        "routes" | "route" | "lines" => {
            for (id, r) in w.transit.routes.iter() {
                let d = near.map_or(0.0, |n| r.stops.iter().map(|s| s.pos.distance(n)).fold(f32::INFINITY, f32::min));
                if d <= radius {
                    rows.push((d, Box::new(move || route_row(id, r))));
                }
            }
        }
        "parcels" | "parcel" => {
            let f_lu = str_filter(filter, "land_use");
            let f_zoned = bool_filter(filter, "zoned");
            let f_landlocked = bool_filter(filter, "landlocked");
            let planned = w.planned_zoning();
            for (i, p) in w.parcels.parcels.iter().enumerate() {
                let built = p.land_use.map_or("unzoned", |l| l.label());
                if !contains(built, &f_lu) || f_zoned.is_some_and(|z| z != p.land_use.is_some()) || f_landlocked.is_some_and(|l| l != p.landlocked()) {
                    continue;
                }
                if let Some(d) = within(p.center) {
                    let plan = planned.get(i).copied().flatten().map_or("unzoned", |l| l.label());
                    rows.push((d, Box::new(move || {
                        let accepts: Vec<&str> = nd_sim::LandUse::ALL.iter().filter(|l| p.accepts(**l)).map(|l| l.label()).collect();
                        json!({
                            "index": i, "center": v2(p.center), "area": round(p.area), "land_use": built, "planned": plan,
                            "landlocked": p.landlocked(), "street": p.street_frontage, "rough": p.rough, "rough_why": p.rough_why, "accepts": accepts, "water_frontage_m": round(p.water_frontage),
                            "lot": p.lot.map(key_str), "key": [p.key.center.0, p.key.center.1],
                        })
                    })));
                }
            }
        }
        "gestures" => {
            for (id, g) in &w.effective_plan().gestures {
                let d = near.map_or(0.0, |n| g.points().iter().map(|p| p.distance(n)).fold(f32::INFINITY, f32::min));
                if d <= radius {
                    rows.push((d, Box::new(move || gesture(*id, g))));
                }
            }
        }
        other => return Err(format!("unknown what {other:?}; one of citizens, buildings, lots, parcels, cars, trains, metro_trains, routes, gestures")),
    }
    let total = rows.len();
    if near.is_some() {
        rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    }
    let rows: Vec<Value> = rows
        .into_iter()
        .take(limit)
        .map(|(d, row)| {
            let mut r = row();
            if near.is_some() {
                r["distance"] = json!(round(d));
            }
            r
        })
        .collect();
    Ok(json!({ "what": what, "total": total, "returned": rows.len(), "rows": rows }))
}

pub(super) fn inspect(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let w = &ctx.sim.world;
    // [toll-inspect] A toll booth by its id.
    if let Some(v) = field(cmd, "toll") {
        let id = tolls::toll_id(w, v)?;
        return Ok(json!({ "toll": tolls::toll_detail(w, id) }));
    }
    if let Some(v) = field(cmd, "citizen") {
        let id = key_from(v)?;
        let c = w.citizens.get(id).ok_or("no such citizen")?;
        let mut v = citizen_detail(w, id, c, ctx.tools.favorites.contains(&id));
        v["places"] = places::citizen_places(w, id);
        v["social"] = social::citizen_social(w, id);
        v["holdings"] = follow::holdings_json(w, id);
        return Ok(json!({ "citizen": v }));
    }
    if let Some(v) = field(cmd, "building") {
        let id = key_from(v)?;
        let b = w.buildings.get(id).ok_or("no such building")?;
        let mut v = building_detail(w, id, b, building_style(&ctx.sim, id, &ctx.assets));
        v["place"] = places::building_place(w, id);
        // [land] owner, sale price, plots, form, pressure
        v["land"] = property::building_land(w, id);
        // [megacorp] the owning corp or university and its logo
        v["brand"] = corp::building_brand(w, id);
        // [supertall] height, rank in the skyline, and a league tower's record
        v["tower"] = skyline::building_tower(w, id);
        // [ferry-routes] a terminal's routes, a shipping port's lane
        if crate::ferry_select::is_route_dock(w, id) {
            v["routes"] = ferry::routes_json(w, id);
        }
        return Ok(json!({ "building": v }));
    }
    if let Some(v) = field(cmd, "lot") {
        let id = key_from(v)?;
        let l = w.lots.get(id).ok_or("no such lot")?;
        return Ok(json!({ "lot": lot_detail(w, id, l) }));
    }
    if let Some(v) = field(cmd, "car") {
        let id = key_from(v)?;
        let c = w.traffic.cars.get(id).ok_or("no such car")?;
        let mut v = json!({ "car": car_detail(w, id, c) });
        // [fire-scene] An emergency unit's car: the unit card as well.
        if let Some(uid) = w.services.unit_of_car(id) {
            v["unit"] = units::unit_detail(&ctx.sim, uid, &ctx.assets.rules).unwrap_or(Value::Null);
        }
        return Ok(v);
    }
    if let Some(v) = field(cmd, "unit") {
        let id = key_from(v)?;
        return Ok(json!({ "unit": units::unit_detail(&ctx.sim, id, &ctx.assets.rules).ok_or("no such unit")? }));
    }
    if let Some(v) = field(cmd, "vehicle") {
        let id = key_from(v)?;
        return Ok(json!({ "vehicle": vehicle_detail(w, id).ok_or("no such vehicle")? }));
    }
    if let Some(v) = field(cmd, "bot") {
        let id = key_from(v)?;
        return Ok(json!({ "bot": follow::bot_detail(w, id).ok_or("no such delivery bot")? }));
    }
    if let Some(v) = field(cmd, "junction") {
        let k = junction_near(w, vec2_of(v)?).ok_or("no junctions")?;
        return Ok(json!({ "junction": junction_detail(w, k) }));
    }
    if let Some(v) = field(cmd, "drone") {
        let id: nd_sim::air::DroneId = key_from(v)?;
        return Ok(json!({ "drone": drone_detail(w, id).ok_or("no such drone")? }));
    }
    if let Some(v) = field(cmd, "train") {
        let id = key_from(v)?;
        return Ok(json!({ "train": trains::train_detail(w, id, &ctx.assets.palette.car_colors).ok_or("no such train")? }));
    }
    if let Some(v) = field(cmd, "metro") {
        let id = key_from(v)?;
        return Ok(json!({ "metro": trains::metro_detail(w, id).ok_or("no such metro train")? }));
    }
    if let Some(v) = field(cmd, "ship") {
        let id = key_from(v)?;
        return Ok(json!({ "ship": ferry::ship_json(w, id).ok_or("no such ship")? }));
    }
    if let Some(v) = field(cmd, "route") {
        let id = key_from(v)?;
        let r = w.transit.routes.get(id).ok_or("no such route")?;
        return Ok(json!({ "route": route_row(id, r) }));
    }
    if let Some(v) = field(cmd, "gesture") {
        let id = GestureId(v.as_u64().ok_or("gesture id is a number")?);
        let g = w.gesture(id).ok_or("no such gesture")?;
        return Ok(json!({ "gesture": gesture(id, &g) }));
    }
    // A road piece: by id, or the one nearest a point.
    // [road-inspect] `road`: an id, or [x, y] picked as a click would be.
    let seg = match (field(cmd, "segment"), field(cmd, "road")) {
        (Some(v), _) => Some(nd_sim::SegmentId(v.as_u64().ok_or("segment id is a number")? as u32)),
        (None, Some(v)) => Some(road::road_id(w, v, ctx.tools.depth)?),
        _ => None,
    };
    if let Some(sid) = seg {
        let s = w.roads.try_segment(sid).ok_or("no such segment")?;
        let (a, b) = w.roads.segment_points(sid);
        let z = |n: nd_sim::NodeId| w.road_z.get(n.0 as usize).copied().unwrap_or(0.0);
        let level = if s.tunnel { "tunnel" } else if s.viaduct { "viaduct" } else if s.bridge { "bridge" } else if w.roads.node(s.a).level < 0.0 || w.roads.node(s.b).level < 0.0 { "cutting" } else { "ground" };
        let crossings: Vec<Value> = w
            .roads
            .grade_crossings()
            .into_iter()
            .filter(|c| c.upper.0 == sid || c.lower.0 == sid)
            .map(|c| {
                let other = if c.upper.0 == sid { c.lower.0 } else { c.upper.0 };
                json!({ "at": [round(c.at.x), round(c.at.y)], "passes": if c.upper.0 == sid { "over" } else { "under" }, "other_segment": other.0, "clearance": round((w.road_height(c.upper.0, c.upper.1) - w.road_height(c.lower.0, c.lower.1)).abs()) })
            })
            .collect();
        return Ok(json!({ "segment": {
            "id": sid.0, "from": [round(a.x), round(a.y)], "to": [round(b.x), round(b.y)], "kind": road_kind_name(s.kind),
            "lanes_fwd": s.lanes_fwd, "lanes_bwd": s.lanes_bwd, "highway": s.is_highway(),
            "level": level, "levels": [w.roads.node(s.a).level, w.roads.node(s.b).level], "heights": [round(z(s.a)), round(z(s.b))], "inner_heights": w.road_inner.get(sid.0 as usize).map(|v| v.iter().map(|(t, h)| [round(*t), round(*h)]).collect::<Vec<_>>()).unwrap_or_default(),
            "bridge": s.bridge, "viaduct": s.viaduct, "tunnel": s.tunnel,
            "nodes": [s.a.0, s.b.0], "junction_degrees": [w.roads.neighbors(s.a).count(), w.roads.neighbors(s.b).count()],
            "grade_crossings": crossings,
            "condition": nights::segment_condition(w, sid),
        }, "road": road::road_detail(w, sid) }));
    }
    // No id: whatever is selected.
    match *ctx.selection {
        Selection::Toll(id) => inspect(&json!({ "toll": id }), ctx),
        Selection::Road(sid) => inspect(&json!({ "segment": sid.0 }), ctx),
        Selection::Citizen(id) => inspect(&json!({ "citizen": key_str(id) }), ctx),
        Selection::Building(id) => inspect(&json!({ "building": key_str(id) }), ctx),
        Selection::Car(id) => inspect(&json!({ "car": key_str(id) }), ctx),
        Selection::Parked(id) => inspect(&json!({ "vehicle": key_str(id) }), ctx),
        Selection::Bot(id) => inspect(&json!({ "bot": key_str(id) }), ctx),
        Selection::Train(id) => inspect(&json!({ "train": key_str(id) }), ctx),
        Selection::Metro(id) => inspect(&json!({ "metro": key_str(id) }), ctx),
        Selection::Unit(id) => inspect(&json!({ "unit": key_str(id) }), ctx),
        Selection::Ship(id) => inspect(&json!({ "ship": key_str(id) }), ctx),
        Selection::Drone(id) => Ok(drone_json(&ctx.sim.world, id)),
        Selection::Rv(seed) => Ok(rv_json(&ctx.sim.world, seed)),
        Selection::None => Err("inspect needs one of citizen, building, lot, car, bot, train, metro, toll, road, segment, route, gesture (an id), or a selection".into()),
    }
}

/// `tick_time` for other modules (the flags).
pub fn tick_time_json(tick: u64) -> Value {
    tick_time(tick)
}

/// Kind, id and label of a selection (`null` for none).
pub fn selection_json_brief(w: &nd_sim::World, sel: Selection) -> Value {
    selection(w, sel)
}

/// The building style label, as the inspector shows it: the style the
/// renderer draws (`nd_procgen::building_style`, one decision for both).
pub(super) fn building_style(sim: &crate::sim::Sim, id: nd_sim::BuildingId, assets: &GameAssets) -> &'static str {
    nd_procgen::building_style(&sim.world, id, &assets.rules, Some(&sim.styles)).map_or("unknown", |s| s.label())
}

/// Everything about what the player has selected, as `inspect` gives it,
/// in one reply: a car with why it stands, the jam in front of it, its trip
/// and lanes; a citizen (and the car they are driving); a building; a
/// parked car; plus the bus line picked in City Data's Services, Transit, if any.
pub fn selected_detail(sim: &crate::sim::Sim, sel: Selection, route_sel: Option<nd_sim::RouteId>, assets: &GameAssets, favourites: &[nd_sim::CitizenId]) -> Value {
    let w = &sim.world;
    let detail = match sel {
        Selection::None => Value::Null,
        Selection::Toll(id) => tolls::toll_detail(w, id).unwrap_or(Value::Null),
        Selection::Road(sid) => road::road_detail(w, sid).unwrap_or(Value::Null),
        Selection::Car(id) => w.traffic.cars.get(id).map_or(Value::Null, |c| car_detail(w, id, c)),
        Selection::Citizen(id) => match w.citizens.get(id) {
            Some(c) => {
                let mut v = citizen_detail(w, id, c, favourites.contains(&id));
                v["places"] = places::citizen_places(w, id);
                v["social"] = social::citizen_social(w, id);
                v["holdings"] = follow::holdings_json(w, id);
                if let nd_sim::Activity::Driving { car, .. } = c.activity {
                    if let Some(x) = w.traffic.cars.get(car) {
                        v["driving_car"] = car_detail(w, car, x);
                    }
                }
                v
            }
            None => Value::Null,
        },
        Selection::Building(id) => match w.buildings.get(id) {
            Some(b) => {
                let mut v = building_detail(w, id, b, building_style(sim, id, assets));
                v["place"] = places::building_place(w, id);
                v
            }
            None => Value::Null,
        },
        Selection::Parked(id) => vehicle_detail(w, id).unwrap_or(Value::Null),
        Selection::Bot(id) => follow::bot_detail(w, id).unwrap_or(Value::Null),
        Selection::Train(id) => trains::train_detail(w, id, &assets.palette.car_colors).unwrap_or(Value::Null),
        Selection::Metro(id) => trains::metro_detail(w, id).unwrap_or(Value::Null),
        Selection::Unit(id) => units::unit_detail(sim, id, &assets.rules).unwrap_or(Value::Null),
        Selection::Ship(id) => ferry::ship_json(w, id).unwrap_or(Value::Null),
        Selection::Drone(id) => drone_json(w, id),
        Selection::Rv(seed) => rv_json(w, seed),
    };
    let bus_line = route_sel.and_then(|r| w.transit.routes.get(r).map(|route| {
        let mut v = route_row(r, route);
        v["buses"] = json!(w.traffic.cars.iter().filter(|(_, c)| c.bus.as_ref().is_some_and(|b| b.route == r)).map(|(id, c)| car_row(w, id, c)).collect::<Vec<_>>());
        v
    }));
    let brief = selection(w, sel);
    json!({
        "kind": brief.get("kind").cloned().unwrap_or(Value::Null), "id": brief.get("id").cloned().unwrap_or(Value::Null),
        "selection": brief, "detail": detail, "bus_line": bus_line, "clock": tick_time(w.clock.tick),
    })
}

/// [feat/pt-ux] `aim: [x, y, h]` (h metres above the ground), or `at`
/// with `click: true`: the point a click from the camera is aimed at.
fn aim_point(cmd: &Value, w: &nd_sim::World) -> Result<Option<bevy::math::Vec3>, String> {
    if let Some(a) = cmd.get("aim").and_then(|v| v.as_array()) {
        let n = |i: usize| a.get(i).and_then(|v| v.as_f64()).map(|v| v as f32);
        let (Some(x), Some(y)) = (n(0), n(1)) else { return Err("aim: [x, y, height]".into()) };
        let p = nd_sim::Vec2::new(x, y);
        return Ok(Some(bevy::math::Vec3::new(x, w.surface_height(p) + n(2).unwrap_or(0.0), y)));
    }
    if cmd.get("click").and_then(|v| v.as_bool()) == Some(true) {
        if let Some(p) = opt_vec2(cmd, "at")? {
            return Ok(Some(bevy::math::Vec3::new(p.x, w.surface_height(p), p.y)));
        }
    }
    Ok(None)
}

pub(super) fn select(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let w = &ctx.sim.world;
    let sel = if let Some(v) = field(cmd, "toll") {
        // [toll-inspect]
        Selection::Toll(tolls::toll_id(w, v)?)
    } else if let Some(v) = field(cmd, "road").or_else(|| field(cmd, "segment")) {
        // [road-inspect] A street piece by id, or the one under [x, y].
        Selection::Road(road::road_id(w, v, ctx.tools.depth)?)
    } else if let Some(v) = field(cmd, "citizen") {
        let id = key_from(v)?;
        if !w.citizens.contains_key(id) {
            return Err("no such citizen".into());
        }
        Selection::Citizen(id)
    } else if let Some(v) = field(cmd, "building") {
        let id = key_from(v)?;
        if !w.buildings.contains_key(id) {
            return Err("no such building".into());
        }
        Selection::Building(id)
    } else if let Some(v) = field(cmd, "car") {
        let id = key_from(v)?;
        if !w.traffic.cars.contains_key(id) {
            return Err("no such car".into());
        }
        Selection::Car(id)
    } else if let Some(v) = field(cmd, "vehicle") {
        let id = key_from(v)?;
        if !w.vehicles.cars.get(id).is_some_and(|c| c.spot().is_some()) {
            return Err("no such parked vehicle".into());
        }
        Selection::Parked(id)
    } else if let Some(v) = field(cmd, "bot") {
        let id = key_from(v)?;
        if !w.bots.bots.contains_key(id) {
            return Err("no such delivery bot".into());
        }
        Selection::Bot(id)
    } else if let Some(v) = field(cmd, "train") {
        let id = key_from(v)?;
        if !w.rail.trains.contains_key(id) {
            return Err("no such train".into());
        }
        Selection::Train(id)
    } else if let Some(v) = field(cmd, "metro") {
        let id = key_from(v)?;
        if !w.subway.trains.contains_key(id) {
            return Err("no such metro train".into());
        }
        Selection::Metro(id)
    } else if let Some(v) = field(cmd, "unit") {
        // [fire-scene] An emergency unit (`units` lists them), driving or on scene.
        let id = key_from(v)?;
        if crate::fire_scene::unit_pose(w, id, ctx.sim.frac()).is_none() {
            return Err(if w.services.units.contains_key(id) { "that unit is in its station".into() } else { "no such unit".into() });
        }
        Selection::Unit(id)
    } else if let Some(v) = field(cmd, "ship") {
        // [ferry-routes] A ferry or freighter (`landscape` lists the ships).
        let id = key_from(v)?;
        if !w.ports.ships.contains_key(id) {
            return Err("no such ship".into());
        }
        Selection::Ship(id)
    } else if let Some(v) = field(cmd, "drone") {
        // [feat/pt-ux] A drone taxi or helicopter (`air` lists them).
        let id = key_from(v)?;
        if !w.air.drones.contains_key(id) {
            return Err("no such drone".into());
        }
        Selection::Drone(id)
    } else if let Some(v) = field(cmd, "rv") {
        // [feat/pt-ux] A camper or van at the kerb, by its id (`society`'s `rv_list`).
        let seed = v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())).ok_or("rv: an id (a number)")?;
        if w.society.homeless.rv(seed).is_none() {
            return Err("no such camper or van".into());
        }
        Selection::Rv(seed)
    } else if let Some(aim) = aim_point(cmd, w)? {
        // [feat/pt-ux] A click from the camera as it stands at the point
        // `aim` [x, y, height above the ground] (or the ground at `at` with
        // `click: true`): by the ray, as the Inspect tool picks, a tower by
        // its walls and from far out not the people.
        let eye = ctx.cams.iter().next().map(|c| c.eye()).ok_or("no camera")?;
        let dir = (aim - eye).normalize_or_zero();
        // Above the horizon the ray meets no ground: only what is in the air
        // (or a tower) can be hit.
        let ground = crate::camera::ray_ground(w, eye, dir);
        let view_dist = ground.map_or(f32::INFINITY, |g| eye.distance(bevy::math::Vec3::new(g.x, w.surface_height(g), g.y)));
        crate::tools::pick_click(&ctx.sim, &crate::tools::Click { ground, depth: ctx.tools.depth, ray: Some((eye, dir)), view_dist })
    } else if let Some(p) = opt_vec2(cmd, "at")? {
        // Pick at the spot as a click there from close by does.
        crate::tools::pick(&*ctx.sim, p, ctx.tools.depth)
    } else {
        Selection::None
    };
    // [fix/review-app] As a click does: a unit's car selects the unit, a
    // bus its line.
    crate::tools::apply_pick(&ctx.sim, &mut ctx.tools, &mut ctx.selection, sel);
    let sel = *ctx.selection;
    Ok(json!({ "selection": selection(&ctx.sim.world, sel) }))
}
