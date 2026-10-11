//! The city: new games and generated cities, the demo district, the ground
//! (clean-up, contamination, wind and sun) and the car-pound and parking
//! levers.

use nd_sim::Vec2 as SVec2;
use serde_json::{json, Value};

use super::ctx::Ctx;
use super::dispatch::Replaces;
use super::view::*;
use super::{identity, land};

pub(super) fn seed_demo(_: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    ctx.saves.title = false;
    ctx.sim.seed_demo_district();
    ctx.tools.status = "Demo district laid out around the centre".into();
    Ok(json!({ "buildings": ctx.sim.world.buildings.len(), "lots": ctx.sim.world.lots.len() }))
}

/// What `new_game` checks before it asks for `confirm`.
pub(super) fn new_game_replaces(cmd: &Value, _: &Ctx) -> Result<Replaces, String> {
    land::params_from(cmd)?;
    opt_str(cmd, "name")?;
    Ok(Replaces::IfUnsaved)
}

pub(super) fn new_game(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let params = land::params_from(cmd)?;
    let name = opt_str(cmd, "name")?;
    ctx.saves.title = false;
    ctx.sim.paused = false;
    ctx.sim.new_game(params);
    if let Some(name) = name {
        ctx.sim.set_city_name(name);
    }
    ctx.tools.status = format!("New city: {} {} landscape", ctx.sim.world.landscape.params.climate.label(), ctx.sim.world.landscape.params.preset.label());
    ctx.history.follow_city(ctx.sim.generation, &ctx.sim.world, "new game");
    let mut v = land::summary(&ctx.sim.world);
    v["identity"] = identity::identity_json(&ctx.sim.world);
    Ok(v)
}

/// What `generate_city` checks before it asks for `confirm`.
pub(super) fn generate_city_replaces(cmd: &Value, _: &Ctx) -> Result<Replaces, String> {
    land::city_params_from(cmd)?;
    opt_str(cmd, "name")?;
    Ok(Replaces::IfUnsaved)
}

pub(super) fn generate_city(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let (preset, params) = land::city_params_from(cmd)?;
    let name = opt_str(cmd, "name")?;
    ctx.saves.title = false;
    ctx.sim.paused = false;
    let start = std::time::Instant::now();
    let (plan, laid) = ctx.sim.generate_city(preset, params);
    if let Some(name) = name {
        ctx.sim.set_city_name(name);
    }
    ctx.tools.status = format!("New city: {} on a {} landscape", preset.label(), ctx.sim.world.landscape.params.preset.label());
    ctx.history.follow_city(ctx.sim.generation, &ctx.sim.world, "generated city");
    let mut v = land::summary(&ctx.sim.world);
    let class = |c: nd_procgen::citygen::RoadClass| json!({ "roads": plan.roads.iter().filter(|r| r.class == c).count(), "length_m": round(plan.road_length(c)) });
    v["city"] = json!({
        "preset": preset.key(),
        "seed": plan.seed,
        "arterial": class(nd_procgen::citygen::RoadClass::Arterial),
        "collector": class(nd_procgen::citygen::RoadClass::Collector),
        "local": class(nd_procgen::citygen::RoadClass::Local),
        "zones": laid.zones,
        "lots": laid.lots,
        "interchanges": laid.links,
        "way_in": laid.way_in,
        "centres": plan.cores.iter().map(|c| v2(*c)).collect::<Vec<_>>(),
        "furnished": laid.furnished,
        "took_ms": start.elapsed().as_millis() as u64,
    });
    v["identity"] = identity::identity_json(&ctx.sim.world);
    Ok(v)
}

/// [dirty-industry] Pay to clean up poisoned ground: {x, y, radius}.
pub(super) fn remediate(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let p = req_point(cmd)?;
    let r = opt_f32(cmd, "radius")?.unwrap_or(100.0);
    let (cost, removed) = ctx.sim.world.remediate(p, r)?;
    Ok(json!({ "ok": true, "cost": round(cost), "removed_cells": round(removed), "treasury": round(ctx.sim.world.economy.treasury) }))
}

/// [dirty-industry] The Bulldoze tool's clean-up: a crew digs it out over a
/// few days (`quote: true` only prices it).
pub(super) fn cleanup(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let p = req_point(cmd)?;
    let r = opt_f32(cmd, "radius")?.unwrap_or(100.0);
    let w = &mut ctx.sim.world;
    let q = if opt_bool(cmd, "quote")?.unwrap_or(false) { w.cleanup_quote(p, r)? } else { w.start_cleanup(p, r)? };
    let crews: Vec<_> = w.cleanups.iter().map(|j| json!({ "id": j.id, "at": [round(j.center.x), round(j.center.y)], "radius": round(j.radius), "progress": round(j.progress()), "days": j.hours / 24, "cost": round(j.cost) })).collect();
    Ok(json!({ "ok": true, "radius": round(q.radius), "cells": q.cells, "amount": round(q.amount), "cost": round(q.cost), "days": q.days, "treasury": round(w.economy.treasury), "crews": crews }))
}

/// [dirty-industry] Dirty plants and contamination near a point.
pub(super) fn dirt_at(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let p = req_point(cmd)?;
    let w = &ctx.sim.world;
    Ok(json!({ "at": [p.x, p.y], "contamination": round(w.contamination.at(p)), "dirty_nearby": land::dirty_near(w, p) }))
}

/// [grid-weather] Wind and sun at a point; y may be given as z (the
/// renderer's name for the sim's y).
pub(super) fn resource(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let p = match (opt_f32(cmd, "x")?, opt_f32(cmd, "y")?.or(opt_f32(cmd, "z")?)) {
        (Some(x), Some(y)) => SVec2::new(x, y),
        _ => req_point(cmd)?,
    };
    Ok(land::resource_at(&ctx.sim.world, p))
}

/// [boot] Clamp a private car and have it towed: `car` (on the road: it
/// pulls over first) or `vehicle` (its record, parked or driving).
pub(super) fn tow_car(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let w = &mut ctx.sim.world;
    let job = if let Some(v) = field(cmd, "car") {
        w.order_tow_car(key_from(v)?)?
    } else {
        let v = field(cmd, "vehicle").ok_or("car or vehicle is required")?;
        w.order_tow(key_from(v)?)?
    };
    let j = w.enforcement.job(job).copied();
    Ok(json!({ "tow_job": job, "vehicle": j.map(|j| key_str(j.car)), "company": j.and_then(|j| j.company).map(key_str), "out_of_town": j.is_some_and(|j| j.company.is_none()) }))
}

/// [games] Put a game on at stadium `building` today (dev), and say where
/// it is in its day.
pub(super) fn stadium_game(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let w = &mut ctx.sim.world;
    let id = key_from(field(cmd, "building").ok_or("building is required")?)?;
    let b = w.buildings.get(id).ok_or("no such building")?;
    if !b.service.is_some_and(|s| s.kind == nd_sim::ServiceKind::Stadium) {
        return Err(format!("{} is not a stadium", b.name));
    }
    if !w.services.events_today.contains(&id) {
        w.services.events_today.push(id);
    }
    use nd_sim::world::games::*;
    Ok(json!({ "building": key_str(id), "game_today": true, "gates_open": GATES_OPEN, "kickoff": KICKOFF, "final_whistle": FINAL_WHISTLE, "phase": w.game_phase(id).map(|p| p.label()), "away_cars_parked": w.game_cars_parked(id), "away_cars_driving_in": w.games.driving_in.iter().filter(|f| f.stadium == id).count() }))
}

/// [airlift] Lift a vehicle on the road away by helicopter: `car`.
pub(super) fn airlift(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let w = &mut ctx.sim.world;
    let car = key_from(field(cmd, "car").ok_or("car is required")?)?;
    w.airlift(car)?;
    let a = w.airlifts().last().unwrap();
    Ok(json!({ "airlift": true, "at": [a.at.x, a.at.y], "from": [a.from.x, a.from.y], "to": [a.to.x, a.to.y], "pound": a.pound.map(key_str), "hook_tick": a.hook_tick(), "end_tick": a.end_tick() }))
}

/// [parking] The fee at a lot the city runs.
pub(super) fn parking_price(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let id = key_from(field(cmd, "building").ok_or("missing building id")?)?;
    let price = opt_f32(cmd, "price")?.ok_or("missing price")?;
    let set = ctx.sim.world.set_parking_price(id, price)?;
    Ok(json!({ "building": key_str(id), "price": set }))
}
