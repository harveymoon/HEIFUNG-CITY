//! The command dispatcher: `remote_commands` drains the queue once a frame
//! and runs each command against the ECS.

use std::sync::atomic::Ordering;

use bevy::prelude::*;
use nd_sim::GestureId;
use nd_sim::Vec2 as SVec2;
use serde_json::{json, Value};

use super::ctx::Ctx;
use super::server::{Inbox, Pending, Reply, QUEUED, TAKEN};
use super::view::*;
use super::*;
use crate::render::Overlays;
use crate::sim::SPEEDS;
use crate::ui::load_now;
use nd_sim::survey::LandLayer;

/// Upper bound for `step`, so a typo cannot freeze the game for minutes.
pub(super) const MAX_STEP: u64 = 100_000;

/// [fix/review-app] `load`, `new_game` and `generate_city` throw the running
/// city away. When it has play time no save holds (`Sim::unsaved_play`)
/// they ask for `"confirm": true`, so an agent cannot wipe a player's game
/// by accident; a city just saved, just loaded, not yet run or untouched
/// (no building, project or new road) needs none.
pub(super) fn confirm_replace(cmd: &Value, ctx: &Ctx, what: &str) -> Result<(), String> {
    if !ctx.sim.unsaved_play() || opt_bool(cmd, "confirm")? == Some(true) {
        return Ok(());
    }
    let clock = &ctx.sim.world.clock;
    Err(format!(
        "{what} replaces the running city (day {}, {}), which has play time no save holds; save it first, or send confirm: true to replace it anyway",
        clock.day() + 1,
        clock.format_hhmm()
    ))
}

pub(super) fn remote_commands(inbox: Res<Inbox>, mut ctx: Ctx) {
    // Take the batch out first: the lock must not be held while commands run.
    let batch: Vec<Pending> = match inbox.0.lock() {
        Ok(rx) => rx.try_iter().collect(),
        Err(_) => return,
    };
    for p in batch {
        // Withdrawn after a 504: running it now would surprise a client
        // that was told it did not run.
        if p.state.compare_exchange(QUEUED, TAKEN, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            continue;
        }
        let reply = run_one(&p.cmd, &mut ctx);
        // The HTTP side may have timed out and gone; nothing to do then.
        let _ = p.reply.send(reply);
    }
}

/// Run one command and shape its reply.
pub(super) fn run_one(cmd: &Value, ctx: &mut Ctx) -> Reply {
    let result = execute(cmd, ctx);
    // [fix/review-app] A command that replaced the city: reset what named
    // the old one's ids now, before the next command in this batch reads
    // it (`city_reset` would only run later this frame).
    {
        let Ctx { sim, epoch, tools, selection, eplan, follow, fly, .. } = ctx;
        crate::city_reset::catch_up(sim.generation, epoch, tools, selection, eplan, follow, fly);
    }
    match result {
        Ok(mut body) => {
            if body.get("ok").is_none() {
                body["ok"] = json!(true);
            }
            Reply { status: 200, body }
        }
        Err(e) => Reply { status: 400, body: json!({ "ok": false, "error": e }) },
    }
}

pub(super) fn execute(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let name = cmd.get("cmd").and_then(Value::as_str).ok_or("missing \"cmd\" field; try {\"cmd\": \"help\"}")?;
    match name {
        "help" => Ok(help::help()),
        "status" => Ok(status::status(ctx)),
        // [memory] The process's memory and what the renderer and the world hold.
        "memory" => Ok(status::memory_json(ctx)),
        "pause" => {
            let paused = opt_bool(cmd, "paused")?.unwrap_or(!ctx.sim.paused);
            ctx.sim.paused = paused;
            Ok(json!({ "paused": paused }))
        }
        "speed" => {
            let sim = &mut ctx.sim;
            if let Some(i) = opt_u64(cmd, "index")? {
                if i as usize >= SPEEDS.len() {
                    return Err(format!("speed index must be 0..{}", SPEEDS.len() - 1));
                }
                sim.speed_index = i as usize;
            } else if let Some(v) = opt_f32(cmd, "value")? {
                // The nearest step on a log scale: 10 picks 16×, 1.5 picks 1×.
                let v = v.max(0.01).ln();
                sim.speed_index = SPEEDS.iter().enumerate().min_by(|a, b| ((a.1).1.ln() - v).abs().total_cmp(&((b.1).1.ln() - v).abs())).map_or(1, |(i, _)| i);
            } else {
                return Err("speed needs {index: 0..5} or {value: multiple of real time}".into());
            }
            Ok(json!({ "speed_index": sim.speed_index, "speed": SPEEDS[sim.speed_index].1, "ticks_per_second": sim.speed(), "label": SPEEDS[sim.speed_index].0 }))
        }
        "step" => {
            let n = opt_u64(cmd, "ticks")?.unwrap_or(1);
            if n > MAX_STEP {
                return Err(format!("ticks must be at most {MAX_STEP}"));
            }
            let start = std::time::Instant::now();
            // Hour by hour, each chunk ending at minute 59, so history sees
            // every hour and reads the day's counters before midnight
            // resets them, however many days one request covers.
            // [fix/review-app] The committee's round runs for every new day
            // (not once per request), and a meeting it calls ends the step
            // there: the city waits for the meeting, as in game.
            let mut left = n;
            let mut meeting = false;
            while left > 0 {
                let r = ctx.sim.world.clock.tick % 60;
                let chunk = left.min(if r < 59 { 59 - r } else { 60 });
                ctx.sim.step_ticks(chunk as u32);
                ctx.history.observe(&ctx.sim.world);
                left -= chunk;
                if crate::committee::daily(&mut ctx.sim, &mut ctx.committee, &mut ctx.flags) {
                    meeting = true;
                    break;
                }
            }
            let mut v = json!({ "ticks": n - left, "clock": status::clock(&ctx.sim), "took_ms": start.elapsed().as_millis() as u64 });
            if meeting {
                v["stopped"] = json!("the committee meets: the city is paused for the meeting (committee_meeting to run it)");
            }
            Ok(v)
        }
        "seed_demo" => {
            ctx.saves.title = false;
            ctx.sim.seed_demo_district();
            ctx.tools.status = "Demo district laid out around the centre".into();
            Ok(json!({ "buildings": ctx.sim.world.buildings.len(), "lots": ctx.sim.world.lots.len() }))
        }
        "save" => files::save(cmd, ctx),
        "city_name" | "city_identity" => identity::city_name(cmd, ctx),
        "game_history" => identity::game_history(cmd, ctx),
        "load" => {
            let path = files::allowed_path(req_str(cmd, "path")?, "ron", false)?;
            confirm_replace(cmd, ctx, "load")?;
            let message = load_now(&path, &mut ctx.sim, &mut ctx.flags)?;
            ctx.saves.mark_dirty();
            ctx.saves.title = false;
            ctx.history.follow_city(ctx.sim.generation, &ctx.sim.world, "city loaded");
            Ok(json!({ "message": message, "clock": status::clock(&ctx.sim), "paused": ctx.sim.paused }))
        }
        "new_game" => {
            let params = land::params_from(cmd)?;
            let name = opt_str(cmd, "name")?;
            confirm_replace(cmd, ctx, "new_game")?;
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
        "generate_city" => {
            let (preset, params) = land::city_params_from(cmd)?;
            let name = opt_str(cmd, "name")?;
            confirm_replace(cmd, ctx, "generate_city")?;
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
                "took_ms": start.elapsed().as_millis() as u64,
            });
            v["identity"] = identity::identity_json(&ctx.sim.world);
            Ok(v)
        }
        "terrain_at" => Ok(land::terrain_at(&ctx.sim.world, req_point(cmd)?)),
        // [dirty-industry] Pay to clean up poisoned ground: {x, y, radius}.
        "remediate" => {
            let p = req_point(cmd)?;
            let r = opt_f32(cmd, "radius")?.unwrap_or(100.0);
            let (cost, removed) = ctx.sim.world.remediate(p, r)?;
            Ok(json!({ "ok": true, "cost": round(cost), "removed_cells": round(removed), "treasury": round(ctx.sim.world.economy.treasury) }))
        }
        // [dirty-industry] The Bulldoze tool's clean-up: a crew digs it out
        // over a few days (`quote: true` only prices it).
        "cleanup" | "clean_up" => {
            let p = req_point(cmd)?;
            let r = opt_f32(cmd, "radius")?.unwrap_or(100.0);
            let w = &mut ctx.sim.world;
            let q = if opt_bool(cmd, "quote")?.unwrap_or(false) { w.cleanup_quote(p, r)? } else { w.start_cleanup(p, r)? };
            let crews: Vec<_> = w.cleanups.iter().map(|j| json!({ "id": j.id, "at": [round(j.center.x), round(j.center.y)], "radius": round(j.radius), "progress": round(j.progress()), "days": j.hours / 24, "cost": round(j.cost) })).collect();
            Ok(json!({ "ok": true, "radius": round(q.radius), "cells": q.cells, "amount": round(q.amount), "cost": round(q.cost), "days": q.days, "treasury": round(w.economy.treasury), "crews": crews }))
        }
        // [dirty-industry] Testing: pour contamination onto the ground.
        "contaminate" => {
            let p = req_point(cmd)?;
            let r = opt_f32(cmd, "radius")?.unwrap_or(60.0).clamp(0.0, 400.0);
            let a = opt_f32(cmd, "amount")?.unwrap_or(0.6).clamp(0.0, 1.0);
            ctx.sim.world.contamination.add(p, r, a);
            Ok(json!({ "ok": true, "at": [p.x, p.y], "contamination": round(ctx.sim.world.contamination.at(p)) }))
        }
        // [social] Testing: a street-art crew's mural on a building now.
        "paint_mural" => {
            let bid: nd_sim::BuildingId = key_from(field(cmd, "building").ok_or("missing building")?)?;
            let w = &mut ctx.sim.world;
            let b = w.buildings.get(bid).ok_or("no such building")?;
            let lot = b.lot;
            let l = w.lots.get(lot).ok_or("no lot")?;
            let along = (l.front_edge[1] - l.front_edge[0]).normalize_or_zero();
            let mid = (l.front_edge[0] + l.front_edge[1]) * 0.5;
            let inward = (l.center - mid).normalize_or_zero();
            let group = match opt_u64(cmd, "group")? {
                Some(g) => g as u32,
                None => w.social.groups.iter().find(|g| g.kind == nd_sim::social::GroupKind::StreetArt).map_or(0, |g| g.id),
            };
            let quality = opt_f32(cmd, "quality")?.unwrap_or(0.6).clamp(-1.0, 1.0);
            let seed = opt_u64(cmd, "seed")?.unwrap_or((l.center.x * 31.0 + l.center.y * 17.0).abs() as u64 ^ 0x5EED);
            w.social.murals.retain(|m| m.lot != lot);
            w.social.murals.push(nd_sim::social::Mural { group, lot, building: Some(bid), seed, day: w.clock.day(), quality, pos: mid + inward * 0.9, along, facing: -inward, width: 6.0, height: 3.4 });
            Ok(json!({ "ok": true, "group": group, "at": [mid.x, mid.y], "facing": [-inward.x, -inward.y] }))
        }
        // [dirty-industry] Dirty plants and contamination near a point.
        "dirt_at" | "contamination_at" => {
            let p = req_point(cmd)?;
            let w = &ctx.sim.world;
            Ok(json!({ "at": [p.x, p.y], "contamination": round(w.contamination.at(p)), "dirty_nearby": land::dirty_near(w, p) }))
        }
        // [grid-weather] Wind and sun at a point; y may be given as z (the
        // renderer's name for the sim's y).
        "resource" => {
            let p = match (opt_f32(cmd, "x")?, opt_f32(cmd, "y")?.or(opt_f32(cmd, "z")?)) {
                (Some(x), Some(y)) => SVec2::new(x, y),
                _ => req_point(cmd)?,
            };
            Ok(land::resource_at(&ctx.sim.world, p))
        }
        "grid" => Ok(land::grid_json(&ctx.sim.world)),
        "weather" => Ok(land::weather_json(&ctx.sim.world)),
        "landscape" => Ok(land::summary(&ctx.sim.world)),
        "ships" => Ok(economy::ships(&ctx.sim.world)),
        // [climate] Dev: set the sky, to see rain, snow or sun on demand.
        "set_weather" => {
            let w = &mut ctx.sim.world;
            if let Some(c) = opt_f32(cmd, "cloud")? {
                w.weather.cloud = c.clamp(0.0, 1.0);
            }
            if let Some(v) = opt_f32(cmd, "wind")? {
                w.weather.wind = v.clamp(0.05, 1.0);
            }
            if let Some(t) = opt_f32(cmd, "temp_c")? {
                w.weather.temp_c = t.clamp(-50.0, 55.0);
            }
            Ok(json!({ "cloud": round(w.weather.cloud), "wind": round(w.weather.wind), "temp_c": round(w.weather.temp_c), "spell": format!("{:?}", w.weather.spell).to_lowercase(), "label": w.weather.label(), "climate": w.landscape.params.climate.key(), "weather_climate": w.weather.climate.key(), "precipitation": crate::render::climate::precip_now(w).map(|(k, i)| json!({ "kind": k, "intensity": round(i) })) }))
        }
        "grant" => {
            // Dev: money for trying things out.
            let amount = opt_f32(cmd, "amount")?.unwrap_or(100_000.0);
            ctx.sim.world.economy.treasury += amount;
            Ok(json!({ "treasury": round(ctx.sim.world.economy.treasury) }))
        }
        "walk_to" => {
            // Dev: send a citizen out on foot (to try crossings and footbridges).
            let id = key_from(field(cmd, "citizen").ok_or("missing field \"citizen\": id")?)?;
            let to = opt_vec2(cmd, "to")?.ok_or("missing field \"to\": [x, y]")?;
            let from = opt_vec2(cmd, "from")?;
            let n = ctx.sim.world.dev_walk(id, from, to)?;
            Ok(json!({ "citizen": key_str(id), "route_points": n }))
        }
        "list_saves" => Ok(files::list_saves()),
        "plan" => Ok(planning::plan(ctx)),
        "add_road" => planning::add_road(cmd, ctx),
        "grounds_junctions" | "park_junctions" => {
            let lot: Option<nd_sim::LotId> = field(cmd, "lot").map(key_from).transpose()?;
            let near = opt_vec2(cmd, "near")?;
            let radius = opt_f32(cmd, "radius")?.unwrap_or(150.0);
            Ok(grounds::list(&ctx.sim.world, lot, opt_str(cmd, "kind")?, near, radius))
        }
        "add_roundabout" => planning::add_roundabout(cmd, ctx),
        // [interchange] A diamond interchange across the highway.
        "add_interchange" | "interchange" => planning::add_interchange(cmd, ctx),
        "add_zone" => planning::add_zone(cmd, ctx),
        "paint" => planning::paint(cmd, ctx),
        "bulldoze" => {
            // [free-placement] A city building goes at once, its ground back
            // to the zoning plan.
            if let Some(v) = field(cmd, "building") {
                let bid: nd_sim::BuildingId = key_from(v)?;
                let msg = ctx.sim.world.bulldoze_building(bid)?;
                ctx.tools.status = format!("{msg}; its ground goes back to the zoning plan.");
                return Ok(json!({ "message": ctx.tools.status, "treasury": round(ctx.sim.world.economy.treasury) }));
            }
            let id = GestureId(opt_u64(cmd, "gesture_id")?.ok_or("missing gesture_id (or building)")?);
            if ctx.sim.world.gesture(id).is_none() {
                return Err(format!("no gesture {}", id.0));
            }
            ctx.sim.world.remove_gesture(id);
            if opt_bool(cmd, "implement")?.unwrap_or(false) {
                ctx.sim.world.implement();
            }
            ctx.tools.status = "Marked for removal. Ctrl+Enter implements.".into();
            Ok(planning::project_state(ctx))
        }
        "project" => planning::project(cmd, ctx),
        "place" => place::place(cmd, ctx),
        "plant_preview" => place::plant_preview(cmd, ctx),
        // [tax] [tolls]
        "taxes" | "tax" => taxes::command(cmd, &mut ctx.sim.world),
        "tolls" | "toll" => taxes::tolls(cmd, &mut ctx.sim.world),
        "incident" => incidents::incident(cmd, ctx),
        "incidents" => incidents::incidents(cmd, ctx),
        "units" => Ok(units::units_list(&ctx.sim.world, opt_str(cmd, "kind")?, opt_bool(cmd, "out")?.unwrap_or(false), ctx.sim.frac())),
        "society" => society::command(cmd, &mut ctx.sim.world),
        // [social]
        "social" => social::command(cmd, &ctx.sim.world),
        // [events]
        "street_events" | "nights" => nights::command(cmd, &ctx.sim.world),
        "race" => nights::race(cmd, &mut ctx.sim.world),
        "rave" => nights::rave(cmd, &mut ctx.sim.world),
        "roads" | "road_condition" | "road_budget" => nights::roads(cmd, &mut ctx.sim.world),
        "repair_road" => road::repair_road(cmd, ctx),
        // [interchange] dev: lay the roads again from the plan (as an
        // implement does) with the current rules, nothing in the plan changed.
        "rebuild_roads" => {
            let w = &mut ctx.sim.world;
            let before = w.roads.alive_segments().count();
            w.rebuild_from_plan();
            Ok(json!({ "segments_before": before, "segments": w.roads.alive_segments().count(), "roads_version": w.roads.version }))
        }
        "district" | "districts" => districts::district(cmd, &mut ctx.sim.world, &mut ctx.overlays, &mut ctx.tools),
        "tourism" | "tourists" => districts::tourism(cmd, &mut ctx.sim.world),
        "concerns" => concerns::command(cmd, &ctx.sim.world),
        // [committee] The mayor's committee and its meetings.
        "committee" => committee::command(cmd, &mut ctx.sim, &mut ctx.committee),
        "committee_meeting" | "meeting" => committee::meeting_command(cmd, &mut ctx.sim, &mut ctx.committee),
        // [voices] Diaries on demand; model-written conversation lines.
        "diary" => committee::diary(cmd, &mut ctx.sim, &mut ctx.committee),
        "voices" => committee::voices(cmd, &ctx.sim, &ctx.committee),
        // [land] the land market; ownership highlight data
        "estate" | "land_market" => property::command(cmd, &mut ctx.sim.world),
        "ownership" => property::ownership(cmd, &ctx.sim.world),
        // [megacorp] mega-corps and universities
        "corps" | "corp" | "megacorp" => corp::command(cmd, &mut ctx.sim.world),
        "campus" | "university" => corp::campus(cmd, &mut ctx.sim.world),
        // [supertall] the skyline, height limits, the record book; dev tower starts
        "skyline" | "towers" | "height_limit" => skyline::command(cmd, &mut ctx.sim.world),
        "pressure" | "pressures" => economy::command(cmd, &mut ctx.sim.world),
        // [waterfront] waterfront lots, beaches and private jetties
        "shore" | "waterfront" | "beaches" | "jetties" | "waterfront_parcels" => shore::command(name, cmd, &mut ctx.sim.world),
        "bus" => bus::bus(cmd, ctx),
        "subway" | "metro" => subway::command(cmd, &mut ctx.sim.world),
        // [metro-ux] The metro line tool, click by click.
        "metro_line" => subway::metro_line(cmd, &mut ctx.sim.world, &mut ctx.tools),
        "find" => inspect::find(cmd, ctx),
        "inspect" => inspect::inspect(cmd, ctx),
        "lane_use" => junction::lane_use(cmd, ctx),
        "intersection" => junction::command(cmd, &mut ctx.sim.world, &mut ctx.tools),
        // [rail-crossing] Level crossings: cycle, trains coming, waits.
        "level_crossings" | "level_crossing" | "crossings" | "rail_crossings" => crossings::command(cmd, &ctx.sim.world),
        // [boot] Clamp a private car and have it towed: `car` (on the road:
        // it pulls over first) or `vehicle` (its record, parked or driving).
        "tow_car" | "tow" | "boot" => {
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
        "parking_price" => {
            // [parking] The fee at a lot the city runs.
            let id = key_from(field(cmd, "building").ok_or("missing building id")?)?;
            let price = opt_f32(cmd, "price")?.ok_or("missing price")?;
            let set = ctx.sim.world.set_parking_price(id, price)?;
            Ok(json!({ "building": key_str(id), "price": set }))
        }
        "select" => inspect::select(cmd, ctx),
        "selected" => Ok(inspect::selected_detail(&ctx.sim, *ctx.selection, ctx.tools.route_sel, &ctx.assets, &ctx.tools.favorites)),
        "flags" => flags::flags_cmd(cmd, ctx),
        "flag" => {
            // Flag the current selection from the API, as F8 does in game.
            let note = opt_str(cmd, "note")?.unwrap_or_default().to_string();
            let w = &ctx.sim.world;
            let sel = *ctx.selection;
            let look = ctx.cams.iter().next().map_or([0.0, 0.0], |c| [c.target.x, c.target.z]);
            let snapshot = inspect::selected_detail(&ctx.sim, sel, ctx.tools.route_sel, &ctx.assets, &ctx.tools.favorites);
            let brief = inspect::selection_json_brief(w, sel);
            let f = ctx.claude.add(w.clock.tick, sel, brief, note, look, snapshot).clone();
            Ok(json!({ "flag": crate::flags::flag_json(&f, None) }))
        }
        "favourite" | "favorite" => {
            let id = key_from(field(cmd, "citizen").ok_or("missing citizen id")?)?;
            if !ctx.sim.world.citizens.contains_key(id) {
                return Err("no such citizen".into());
            }
            let on = opt_bool(cmd, "on")?.unwrap_or(!ctx.tools.favorites.contains(&id));
            ctx.tools.favorites.retain(|f| *f != id);
            if on {
                ctx.tools.favorites.push(id);
            }
            Ok(json!({ "citizen": key_str(id), "favourite": on, "favourites": ctx.tools.favorites.iter().map(|&f| key_str(f)).collect::<Vec<_>>() }))
        }
        "tool" => tool::tool(cmd, ctx),
        "camera" => camera::camera(cmd, ctx),
        // [camera] The inspector's Follow, Zoom to and gift buttons.
        "follow" => follow::follow(cmd, ctx),
        "zoom_to" | "zoom" => follow::zoom_to(cmd, ctx),
        "gift" => follow::gift(cmd, ctx),
        "overlay" => {
            // Map layers are exclusive (`crate::map_layers`): turning one on
            // turns every other off; turning the one showing off leaves none.
            use crate::map_layers::{self as ml, LayerId};
            let o = &mut ctx.overlays;
            let unknown = |k: &str| format!("unknown layer {k:?}; one of none, {}", ml::keys().join(", "));
            let parse = |k: &str| -> Result<Option<LayerId>, String> {
                if k.is_empty() || k.eq_ignore_ascii_case("none") || k.eq_ignore_ascii_case("off") {
                    Ok(None)
                } else {
                    LayerId::parse(k).map(Some).ok_or_else(|| unknown(k))
                }
            };
            let switch = |o: &mut Overlays, l: LayerId, on: bool| {
                if on {
                    ml::set_active(o, Some(l));
                } else if ml::active(o) == Some(l) {
                    ml::set_active(o, None);
                }
            };
            // `{active: "traffic" | "none"}`: the layer to show.
            match cmd.get("active") {
                None => {}
                Some(Value::Null) => ml::set_active(o, None),
                Some(_) => {
                    let k = opt_str(cmd, "active")?.unwrap_or("none");
                    ml::set_active(o, parse(k)?);
                }
            }
            // `{layer, on}` (on omitted toggles).
            if let Some(layer) = opt_str(cmd, "layer")? {
                let on = opt_bool(cmd, "on")?;
                if layer == "projects" {
                    o.projects = on.unwrap_or(!o.projects);
                } else {
                    match parse(layer)? {
                        Some(l) => {
                            let now = ml::active(o) == Some(l);
                            switch(o, l, on.unwrap_or(!now));
                        }
                        None => ml::set_active(o, None),
                    }
                }
            }
            // `{land: "oil"}` shows a land layer, `"none"` (or null) hides
            // whichever land layer shows.
            match cmd.get("land") {
                None => {}
                Some(Value::Null) => {
                    if o.land.is_some() {
                        o.land = None;
                    }
                }
                Some(_) => {
                    let k = opt_str(cmd, "land")?.unwrap_or("none");
                    match parse(k)? {
                        Some(l @ LayerId::Land(_)) => ml::set_active(o, Some(l)),
                        Some(_) => return Err(format!("{k:?} is not a land layer; one of none, {}", LandLayer::ALL.iter().map(|l| l.key()).collect::<Vec<_>>().join(", "))),
                        None => o.land = None,
                    }
                }
            }
            // The older form, a bool per layer name: true shows it (alone).
            for l in ml::MAP_LAYERS {
                if let Some(b) = opt_bool(cmd, l.key)? {
                    switch(o, LayerId::parse(l.key).expect("registry key"), b);
                }
            }
            if let Some(b) = opt_bool(cmd, "projects")? {
                o.projects = b;
            }
            Ok(look::overlays_json(o))
        }
        "lighting" => {
            let l = &mut ctx.lighting;
            if let Some(h) = opt_f32(cmd, "hour")? {
                l.hour = h.clamp(0.0, 24.0);
                // Asking for an hour means wanting to see it.
                l.follow_clock = false;
            }
            if let Some(b) = opt_bool(cmd, "follow_clock")? {
                l.follow_clock = b;
            }
            Ok(json!({ "follow_clock": l.follow_clock, "hour": l.hour, "heat_vision": l.heat }))
        }
        "heat_vision" | "heat" => {
            let l = &mut ctx.lighting;
            l.heat = opt_bool(cmd, "on")?.unwrap_or(!l.heat);
            Ok(json!({ "heat_vision": l.heat }))
        }
        "graphics" | "quality" => {
            // [look] `{quality: auto|low|medium|high}` pins the tier (for
            // screenshots that must compare) or hands it back to Auto.
            if let Some(q) = opt_str(cmd, "quality")?.or(opt_str(cmd, "tier")?) {
                ctx.lighting.quality = crate::render::QualitySetting::parse(q).ok_or_else(|| format!("unknown quality {q:?}; auto, low, medium or high"))?;
            }
            Ok(look::graphics_json(ctx))
        }
        "screenshot" => files::screenshot(cmd, ctx),
        "events" => {
            let since = opt_u64(cmd, "since")?.unwrap_or(0);
            let limit = opt_u64(cmd, "limit")?.unwrap_or(500) as usize;
            let log = &ctx.log;
            let events: Vec<Value> = log.ring.iter().filter(|e| e["seq"].as_u64().unwrap_or(0) > since).take(limit).cloned().collect();
            // [fix/review-app] From 0 too: events 1.. are gone once the ring
            // has turned over.
            let dropped = log.ring.front().and_then(|e| e["seq"].as_u64()).is_some_and(|first| since + 1 < first);
            Ok(json!({ "seq": log.seq, "events": events, "dropped": dropped, "message": ctx.flags.message, "tool_status": ctx.tools.status }))
        }
        "thoughts" => {
            let limit = opt_u64(cmd, "limit")?.unwrap_or(50) as usize;
            let w = &ctx.sim.world;
            let mut rows: Vec<(u64, Value)> = w.citizens.iter().flat_map(|(id, c)| c.memories.iter().map(move |m| (m.tick, json!({ "citizen": key_str(id), "name": c.name, "tick": m.tick, "text": m.text })))).collect();
            rows.sort_by(|a, b| b.0.cmp(&a.0));
            let memories: Vec<Value> = rows.into_iter().take(limit).map(|(_, v)| v).collect();
            let thoughts: Vec<Value> = w.citizens.iter().filter(|(_, c)| !c.thought.is_empty()).take(limit).map(|(id, c)| json!({ "citizen": key_str(id), "name": c.name, "thought": c.thought })).collect();
            Ok(json!({ "memories": memories, "thoughts": thoughts }))
        }
        "report" => {
            let start = std::time::Instant::now();
            let sections = opt_strings(cmd, "sections")?.unwrap_or_default();
            let mut v = report::report(&ctx.sim.world, &sections)?;
            // The sim's own history rings: asked for by name only, as they
            // are long.
            if sections.iter().any(|s| s == "history") {
                let days = opt_u64(cmd, "history_days")?.unwrap_or(30).clamp(1, 365) as usize;
                v["history"] = report::sim_history(&ctx.sim.world, days);
            }
            v["clock"] = status::clock(&ctx.sim);
            v["took_ms"] = json!(ms(start));
            Ok(v)
        }
        "industry" => map::industry(cmd, &mut ctx.sim.world),
        "map" => {
            let start = std::time::Instant::now();
            let layer = opt_str(cmd, "layer")?.unwrap_or("summary");
            let mut v = map::map(&ctx.sim.world, opt_f32(cmd, "cell_m")?, layer)?;
            v["took_ms"] = json!(ms(start));
            Ok(v)
        }
        "history" => {
            let keys = opt_strings(cmd, "keys")?;
            let days = opt_u64(cmd, "days")?.map(|d| d as usize);
            ctx.history.to_json(keys.as_deref(), days)
        }
        "list" => {
            let start = std::time::Instant::now();
            let args = list::ListArgs {
                what: req_str(cmd, "what")?,
                sort: opt_str(cmd, "sort")?,
                desc: opt_bool(cmd, "desc")?.unwrap_or(false),
                filter: field(cmd, "filter"),
                fields: opt_strings(cmd, "fields")?,
                offset: opt_u64(cmd, "offset")?.unwrap_or(0) as usize,
                limit: (opt_u64(cmd, "limit")?.unwrap_or(list::DEFAULT_LIMIT as u64) as usize).min(list::MAX_LIMIT),
            };
            let mut v = list::list(&ctx.sim.world, &args)?;
            v["took_ms"] = json!(ms(start));
            Ok(v)
        }
        "export" => files::export(cmd, ctx),
        "directory" => directory::directory(&ctx.sim.world, &mut ctx.windows, cmd),
        "build_highlight" => window::build_highlight(cmd, ctx),
        "window" if req_str(cmd, "name").is_ok_and(|n| n.trim().eq_ignore_ascii_case("new_game") || n.trim().eq_ignore_ascii_case("new game")) => {
            let open = opt_bool(cmd, "open")?.unwrap_or(true);
            ctx.windows.new_game = Some(open);
            Ok(json!({ "window": "new game", "open": open }))
        }
        "window" if req_str(cmd, "name").is_ok_and(|n| n.trim().eq_ignore_ascii_case("build")) => {
            // The Build window, optionally at a section.
            let section = match opt_str(cmd, "section")? {
                Some(k) => Some(crate::ui::build::Cat::parse(k).ok_or_else(|| format!("unknown section {k:?}; one of {}", crate::ui::build::Cat::ALL.iter().map(|c| c.key()).collect::<Vec<_>>().join(", ")))?),
                None => None,
            };
            let open = opt_bool(cmd, "open")?.unwrap_or(if section.is_some() { true } else { !ctx.windows.build_open });
            ctx.windows.build = Some((open, section));
            Ok(json!({ "window": "build", "open": open, "section": section.map(|c| c.key()).or(ctx.windows.build_section) }))
        }
        "window" if req_str(cmd, "name").is_ok_and(|n| n.trim().eq_ignore_ascii_case("committee")) => {
            // [committee] The Committee window, optionally at a tab.
            let open = opt_bool(cmd, "open")?.unwrap_or(true);
            let tab = match opt_str(cmd, "tab")? {
                Some(t) => Some(crate::ui::committee::Tab::parse(t).ok_or_else(|| format!("unknown tab {t:?}; meeting, members, alerts, projects, settings"))?),
                None => None,
            };
            ctx.committee.open_request = Some(open);
            ctx.committee.tab_request = tab;
            Ok(json!({ "window": "committee", "open": open, "tab": opt_str(cmd, "tab")? }))
        }
        "window" if req_str(cmd, "name").is_ok_and(|n| matches!(n.trim().to_ascii_lowercase().as_str(), "history" | "game_history" | "game history")) => {
            // [identity] The Game history window.
            let open = opt_bool(cmd, "open")?.unwrap_or(true);
            ctx.saves.history_requested = Some(open);
            Ok(json!({ "window": "game history", "open": open }))
        }
        "window" if req_str(cmd, "name").is_ok_and(|n| matches!(n.trim().to_ascii_lowercase().as_str(), "title" | "title_screen" | "title screen")) => {
            let open = opt_bool(cmd, "open")?.unwrap_or(!ctx.saves.title);
            ctx.saves.title = open;
            if !open {
                // As "Play this map": the city behind runs again.
                ctx.sim.paused = false;
            }
            Ok(json!({ "window": "title", "open": open }))
        }
        "window" if req_str(cmd, "name").is_ok_and(|n| crate::ui::city_data::Page::from_window(n).is_some()) => {
            // [city-data] The report windows are City Data pages now: the old
            // name opens its tab (at its last sub-tab) or its page; People's
            // old `tab` and a `sub` pick a sub-tab. `expanded` is accepted and
            // means open: every page is full size.
            use crate::ui::city_data::{Page, Target};
            let name = req_str(cmd, "name")?;
            let mut target = Page::from_window(name).ok_or("unreachable")?;
            if let Some(sub) = opt_str(cmd, "sub")?.or(opt_str(cmd, "tab")?) {
                let tab = match target {
                    Target::Tab(t) => t,
                    Target::Page(p) => p.tab(),
                };
                let sub = if sub.trim().eq_ignore_ascii_case("overview") && tab == crate::ui::city_data::Tab::People { "population" } else { sub };
                target = Target::Page(Page::parse(None, tab.key(), Some(sub))?);
            }
            let open = if opt_bool(cmd, "expanded")? == Some(true) { Some(true) } else { opt_bool(cmd, "open")? };
            let mut v = ctx.windows.request_data(Some(target), open);
            v["window"] = json!("city data");
            v["note"] = json!("the report windows are tabs of the City Data view now; see city_data");
            Ok(v)
        }
        "window" => {
            let info = window::info_of(req_str(cmd, "name")?)?;
            let (open, list) = ctx.windows.request(info, opt_bool(cmd, "open")?);
            Ok(json!({ "window": info.title().to_ascii_lowercase(), "open": open, "windows_open": list }))
        }
        "city_data" => {
            // [city-data] The full-screen City Data view.
            use crate::ui::city_data::{Page, Target};
            let tab = opt_str(cmd, "tab")?;
            let sub = opt_str(cmd, "sub")?;
            let open = opt_bool(cmd, "open")?;
            if let Some(y) = opt_f32(cmd, "scroll")? {
                ctx.windows.data_scroll = Some(y.max(0.0));
            }
            if tab.is_none() && sub.is_some() {
                return Err("sub needs a tab".into());
            }
            if tab.is_none() && open.is_none() {
                return Ok(ctx.windows.data_json());
            }
            let target = match (tab, sub) {
                (Some(t), Some(s)) => Some(Target::Page(Page::parse(None, t, Some(s))?)),
                (Some(t), None) => Some(Target::Tab(crate::ui::city_data::Tab::parse(t).ok_or_else(|| format!("unknown tab {t:?}; one of city, economy, market, people, services, directory"))?)),
                _ => None,
            };
            // Naming a page opens it unless told otherwise.
            let open = open.or(target.map(|_| true));
            Ok(ctx.windows.request_data(target, open))
        }
        other => Err(format!("unknown command {other:?}; try {{\"cmd\": \"help\"}}")),
    }
}

#[cfg(test)]
mod dispatch_tests {
    use super::*;
    use crate::assets::GameAssets;
    use crate::camera::{CameraMode, WalkState};
    use crate::render::{Lighting, RenderStats};
    use crate::sim::Sim;
    use crate::tools::{EffectivePlan, Selection, ToolState};
    use crate::ui::{Saves, UiFlags};
    use bevy::diagnostic::DiagnosticsStore;
    use bevy::ecs::system::RunSystemOnce;

    /// A headless app holding everything `Ctx` borrows (no window, no
    /// renderer, no files read).
    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins);
        let mut committee = crate::committee::CommitteeRt::default();
        committee.settings = Default::default();
        committee.courier = nd_minds::committee::Courier::mock();
        app.insert_resource(Sim::new())
            .insert_resource(GameAssets::for_tests())
            .insert_resource(committee)
            .insert_resource(Saves::default())
            .init_resource::<ToolState>()
            .init_resource::<Selection>()
            .init_resource::<EffectivePlan>()
            .init_resource::<UiFlags>()
            .init_resource::<Overlays>()
            .init_resource::<Lighting>()
            .init_resource::<CameraMode>()
            .init_resource::<WalkState>()
            .init_resource::<crate::camera::CameraFollow>()
            .init_resource::<crate::camera::CameraFly>()
            .init_resource::<EventLog>()
            .init_resource::<History>()
            .init_resource::<WindowRequests>()
            .init_resource::<RenderStats>()
            .init_resource::<DiagnosticsStore>()
            .init_resource::<crate::flags::ClaudeFlags>()
            .init_resource::<crate::build_highlight::BuildHighlight>()
            .init_resource::<Assets<Mesh>>()
            .init_resource::<Assets<StandardMaterial>>()
            .init_resource::<Assets<Image>>()
            .init_resource::<crate::city_reset::CityEpoch>();
        app
    }

    /// Run one command as the API does; (HTTP status, body).
    fn send(app: &mut App, cmd: Value) -> (u16, Value) {
        let r = app.world_mut().run_system_once(move |mut ctx: Ctx| run_one(&cmd, &mut ctx)).expect("the command runs");
        (r.status, r.body)
    }

    fn tick(app: &App) -> u64 {
        app.world().resource::<Sim>().world.clock.tick
    }

    fn start(app: &App) -> u64 {
        app.world().resource::<Sim>().saved_tick.expect("a city starts clean")
    }

    #[test]
    fn read_only_commands_leave_the_city_alone() {
        let mut app = app();
        let generation = app.world().resource::<Sim>().generation;
        for cmd in ["status", "help", "report", "list_saves"] {
            let (status, body) = send(&mut app, json!({ "cmd": cmd }));
            assert_eq!(status, 200, "{cmd}: {body}");
            assert_eq!(body["ok"], true);
        }
        assert_eq!(tick(&app), start(&app));
        assert_eq!(app.world().resource::<Sim>().generation, generation);
        let (status, body) = send(&mut app, json!({ "cmd": "no_such_command" }));
        assert_eq!(status, 400);
        assert_eq!(body["ok"], false);
    }

    #[test]
    fn mutating_commands_change_the_city() {
        let mut app = app();
        let t0 = tick(&app);
        let (status, body) = send(&mut app, json!({ "cmd": "step", "ticks": 5 }));
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["ticks"], 5);
        assert_eq!(tick(&app), t0 + 5);
        let before = app.world().resource::<Sim>().world.economy.treasury;
        let (status, _) = send(&mut app, json!({ "cmd": "grant", "amount": 1000 }));
        assert_eq!(status, 200);
        assert_eq!(app.world().resource::<Sim>().world.economy.treasury, before + 1000.0);
        // An amount that is infinite as an f32 is refused.
        let (status, _) = send(&mut app, json!({ "cmd": "grant", "amount": 1e39 }));
        assert_eq!(status, 400);
        assert!(app.world().resource::<Sim>().world.economy.treasury.is_finite());
    }

    /// [fix/review-app] `new_game` and `generate_city` need `confirm` when
    /// the city has play time no save holds, and a city that was replaced
    /// leaves no selection or tool ids behind.
    #[test]
    fn replacing_a_played_city_needs_confirm() {
        let mut app = app();
        // A city just started has nothing to lose.
        let (status, body) = send(&mut app, json!({ "cmd": "new_game", "preset": "flat" }));
        assert_eq!(status, 200, "{body}");
        // Left running empty, it still has nothing to lose.
        send(&mut app, json!({ "cmd": "step", "ticks": 30 }));
        let (status, body) = send(&mut app, json!({ "cmd": "new_game", "preset": "flat" }));
        assert_eq!(status, 200, "an untouched city: {body}");
        // Something built and run: now it does.
        send(&mut app, json!({ "cmd": "seed_demo" }));
        let t0 = tick(&app);
        send(&mut app, json!({ "cmd": "step", "ticks": 30 }));
        let generation = app.world().resource::<Sim>().generation;
        for cmd in ["new_game", "generate_city"] {
            let (status, body) = send(&mut app, json!({ "cmd": cmd }));
            assert_eq!(status, 400, "{cmd}: {body}");
            assert!(body["error"].as_str().unwrap().contains("confirm: true"), "{body}");
        }
        assert_eq!(app.world().resource::<Sim>().generation, generation, "nothing replaced");
        assert_eq!(tick(&app), t0 + 30);
        // With confirm it goes ahead; the old ids go with the old city.
        app.world_mut().resource_mut::<ToolState>().selected_gesture = Some(GestureId(3));
        let (status, body) = send(&mut app, json!({ "cmd": "new_game", "preset": "flat", "confirm": true }));
        assert_eq!(status, 200, "{body}");
        assert!(app.world().resource::<Sim>().generation > generation);
        assert_eq!(app.world().resource::<ToolState>().selected_gesture, None);
        assert_eq!(app.world().resource::<crate::city_reset::CityEpoch>().0, app.world().resource::<Sim>().generation);
    }

    /// [fix/review-app] The committee's backend is the player's to change.
    #[test]
    fn the_committee_backend_needs_the_players_leave() {
        let mut app = app();
        let (status, body) = send(&mut app, json!({ "cmd": "committee", "action": "settings", "backend": "claude" }));
        assert_eq!(status, 400, "{body}");
        let (status, body) = send(&mut app, json!({ "cmd": "committee", "action": "settings", "local_url": "http://example.invalid" }));
        assert_eq!(status, 400, "{body}");
        let (status, body) = send(&mut app, json!({ "cmd": "committee", "action": "settings", "veto_allowed": false }));
        assert_eq!(status, 200, "{body}");
        assert_eq!(body["remote_may_switch"], false);
        app.world_mut().resource_mut::<crate::committee::CommitteeRt>().remote_may_switch = true;
        let (status, body) = send(&mut app, json!({ "cmd": "committee", "action": "settings", "backend": "mock" }));
        assert_eq!(status, 200, "{body}");
    }

    /// Every name `execute` matches, read from its source: the arms
    /// `"name" | "other" =>` at the match's own indent.
    fn dispatched() -> std::collections::BTreeSet<String> {
        let src = include_str!("dispatch.rs").replace("\r\n", "\n");
        let body = &src[src.find("fn execute(").expect("execute")..];
        let body = &body[..body.find("\n}\n").expect("end of execute")];
        let mut out = std::collections::BTreeSet::new();
        for line in body.lines() {
            // An arm: names, then a guard (` if ...`) or ` =>`.
            let Some(rest) = line.strip_prefix("        \"") else { continue };
            let names = rest.split(" if ").next().unwrap_or("").split(" =>").next().unwrap_or("");
            for name in format!("\"{names}").split(" | ") {
                let name = name.trim().trim_matches('"');
                if !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase() || b == b'_') {
                    out.insert(name.to_string());
                }
            }
        }
        out
    }

    /// [fix/review-app] `help` documents every command the dispatcher
    /// knows (or names it as an alias), and nothing it does not.
    #[test]
    fn help_matches_the_dispatcher() {
        let mut app = app();
        let (_, body) = send(&mut app, json!({ "cmd": "help" }));
        let listed: std::collections::BTreeSet<String> = body["commands"].as_object().expect("commands").keys().cloned().collect();
        let aliases: std::collections::BTreeSet<String> = help::ALIASES.iter().map(|(a, _)| a.to_string()).collect();
        let known = dispatched();
        assert!(known.len() > 100, "the parse found the arms: {known:?}");
        let missing: Vec<_> = known.iter().filter(|n| !listed.contains(*n) && !aliases.contains(*n)).collect();
        let extra: Vec<_> = listed.iter().chain(&aliases).filter(|n| !known.contains(*n)).collect();
        assert!(missing.is_empty(), "dispatched but not in help: {missing:?}");
        assert!(extra.is_empty(), "in help but not dispatched: {extra:?}");
        for (a, c) in help::ALIASES {
            assert!(listed.contains(*c), "alias {a} points at {c}, which help does not list");
            assert!(!listed.contains(*a), "{a} is both a command and an alias");
        }
    }
}
