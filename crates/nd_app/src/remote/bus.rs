//! `bus`: bus lines, stops and buses.

use serde_json::{json, Value};

use super::ctx::Ctx;
use super::view::*;

pub(super) fn bus(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let action = req_str(cmd, "action")?;
    let w = &mut ctx.sim.world;
    let route_arg = |w: &nd_sim::World| -> Result<nd_sim::RouteId, String> {
        let id = key_from(field(cmd, "route").ok_or("missing route id")?)?;
        if !w.transit.routes.contains_key(id) {
            return Err("no such route".into());
        }
        Ok(id)
    };
    match action {
        // Highlight a line on the map (its path, stops and buses), as
        // clicking it in City Data's Services, Transit does; no route clears it.
        "select" => {
            let id = match field(cmd, "route") {
                Some(_) => Some(route_arg(w)?),
                None => None,
            };
            ctx.tools.route_sel = id;
            Ok(json!({ "route": opt_key(id) }))
        }
        "new_route" => {
            let id = w.add_bus_route();
            ctx.tools.route_sel = Some(id);
            Ok(json!({ "route": key_str(id), "row": route_row(id, &w.transit.routes[id]) }))
        }
        "add_stop" => {
            let p = req_point(cmd)?;
            // No route given: extend the one being drawn, or start a new one.
            let id = match field(cmd, "route") {
                Some(_) => route_arg(w)?,
                None => match ctx.tools.route_sel.filter(|r| w.transit.routes.contains_key(*r)) {
                    Some(r) => r,
                    None => {
                        let r = w.add_bus_route();
                        ctx.tools.route_sel = Some(r);
                        r
                    }
                },
            };
            let idx = w.add_bus_stop(id, p)?;
            Ok(json!({ "route": key_str(id), "stop_index": idx, "row": route_row(id, &w.transit.routes[id]) }))
        }
        "move_stop" => {
            let id = route_arg(w)?;
            let idx = opt_u64(cmd, "index")?.ok_or("missing stop index")? as usize;
            let p = req_point(cmd)?;
            if !w.move_bus_stop(id, idx, p) {
                return Err("stop could not be moved there (no street within reach?)".into());
            }
            Ok(json!({ "route": key_str(id), "row": route_row(id, &w.transit.routes[id]) }))
        }
        "remove_stop" => {
            let id = route_arg(w)?;
            let idx = opt_u64(cmd, "index")?.ok_or("missing stop index")? as usize;
            if w.transit.routes[id].stops.len() <= idx {
                return Err("no such stop".into());
            }
            w.remove_bus_stop(id, idx);
            ctx.tools.stop_sel = None;
            let alive = w.transit.routes.contains_key(id);
            if !alive && ctx.tools.route_sel == Some(id) {
                ctx.tools.route_sel = None;
            }
            Ok(json!({ "route": key_str(id), "route_alive": alive, "row": w.transit.routes.get(id).map(|r| route_row(id, r)) }))
        }
        "set_buses" => {
            let id = route_arg(w)?;
            let n = opt_u64(cmd, "n")?.ok_or("missing n")?.min(20) as u8;
            w.set_route_buses(id, n);
            Ok(json!({ "route": key_str(id), "row": route_row(id, &w.transit.routes[id]) }))
        }
        "remove_route" => {
            let id = route_arg(w)?;
            w.remove_bus_route(id);
            if ctx.tools.route_sel == Some(id) {
                ctx.tools.route_sel = None;
            }
            Ok(json!({ "removed": key_str(id), "routes": w.transit.routes.len() }))
        }
        "list" => {
            let (wanted, capacity) = w.fleet_balance();
            let rows: Vec<Value> = w.transit.routes.iter().map(|(id, r)| {
                let mut row = route_row(id, r);
                row["problems"] = line_problems_json(&w.line_problems(id));
                row
            }).collect();
            Ok(json!({ "routes": rows, "fleet": { "wanted": wanted, "depot_capacity": capacity } }))
        }
        // [pt-transit] What keeps a line from running, or is worth a warning.
        "problems" => {
            let id = route_arg(w)?;
            Ok(json!({ "route": key_str(id), "problems": line_problems_json(&w.line_problems(id)) }))
        }
        // [pt-transit] Check a line before committing it: `points` [[x,y],..]
        // snapped to stops as clicks would be; the problems such a loop has.
        "check" => {
            let pts = cmd.get("points").and_then(Value::as_array).ok_or("missing points [[x,y],...]")?;
            let mut stops = Vec::new();
            for p in pts {
                let xy = p.as_array().filter(|a| a.len() == 2).ok_or("points are [x,y] pairs")?;
                let v = nd_sim::Vec2::new(xy[0].as_f64().unwrap_or(0.0) as f32, xy[1].as_f64().unwrap_or(0.0) as f32);
                stops.push(w.transit.stop_for_click(&w.roads, v).ok_or_else(|| format!("no street within reach of ({:.0}, {:.0})", v.x, v.y))?);
            }
            let problems = nd_sim::Transit::check_stops(&w.traffic.net, &stops);
            Ok(json!({ "stops": stops.len(), "problems": line_problems_json(&problems) }))
        }
        "set_fare" => {
            let id = route_arg(w)?;
            let fare = cmd.get("fare").and_then(Value::as_f64).ok_or("missing fare")? as f32;
            w.set_route_fare(id, fare)?;
            Ok(json!({ "route": key_str(id), "row": route_row(id, &w.transit.routes[id]) }))
        }
        // [lines] The Lines window's figures for one line, and its problems.
        "line" => {
            let id = route_arg(w)?;
            let v = line_detail(w, id);
            Ok(v)
        }
        // [lines] Edit mode, as the Lines window's "Edit line" and "Finish
        // editing line" and the player's clicks on the map in it.
        "edit" => {
            let id = route_arg(w)?;
            crate::tools::line_edit::start(&mut ctx.tools, id);
            Ok(json!({ "editing": key_str(id), "status": ctx.tools.status }))
        }
        // [fix/buildings-ui] Draw a new line, as the Lines window's "New
        // line": clicks add stops in order, `shift` clicks insert.
        "new_line" => {
            let id = w.add_bus_route();
            crate::tools::line_edit::start_new(&mut ctx.tools, id);
            Ok(json!({ "editing": key_str(id), "status": ctx.tools.status }))
        }
        "finish_edit" => {
            let msg = crate::tools::line_edit::finish(w, &mut ctx.tools);
            Ok(json!({ "editing": Value::Null, "status": msg }))
        }
        // A click on the map while editing: left (add, insert, share, grab)
        // or right (remove the stop there). A grab is let go at once.
        "click" => {
            if ctx.tools.line_edit.is_none() {
                return Err("not editing a line (bus {action: edit, route})".into());
            }
            let p = req_point(cmd)?;
            let right = cmd.get("right").and_then(Value::as_bool).unwrap_or(false);
            let shift = cmd.get("shift").and_then(Value::as_bool).unwrap_or(false);
            crate::tools::line_edit::input(w, &mut ctx.tools, Some(p), !right, shift, !right, right);
            let route = ctx.tools.line_edit.map(|e| e.route);
            Ok(json!({ "status": ctx.tools.status, "editing": opt_key(route), "row": route.and_then(|r| w.transit.routes.get(r).map(|x| route_row(r, x))) }))
        }
        // The diagram's "+": the slot the next stop goes into (none: where it belongs).
        "slot" => {
            let e = ctx.tools.line_edit.as_mut().ok_or("not editing a line")?;
            e.slot = opt_u64(cmd, "index")?.map(|i| i as usize);
            Ok(json!({ "slot": e.slot }))
        }
        // [w1] Run a line both ways (out and back along the far kerbs) or as a loop.
        "two_way" => {
            let id = route_arg(w)?;
            let on = cmd.get("on").and_then(Value::as_bool).ok_or("missing on: bool")?;
            w.set_bus_route_two_way(id, on)?;
            Ok(json!({ "route": key_str(id), "row": route_row(id, &w.transit.routes[id]) }))
        }
        // Put a stop into a line where it belongs along the loop (or at index).
        "insert_stop" => {
            let id = route_arg(w)?;
            let p = req_point(cmd)?;
            let at = opt_u64(cmd, "index")?.map(|i| i as usize);
            let idx = w.insert_bus_stop(id, at, p)?;
            Ok(json!({ "route": key_str(id), "stop_index": idx, "row": route_row(id, &w.transit.routes[id]) }))
        }
        // Open a metro line in the Lines window (no line closes it).
        "select_metro" => {
            let id = match field(cmd, "line") {
                Some(v) => {
                    let id: nd_sim::MetroLineId = key_from(v)?;
                    if !w.subway.lines.contains_key(id) {
                        return Err("no such metro line".into());
                    }
                    Some(id)
                }
                None => None,
            };
            ctx.tools.metro_sel = id;
            Ok(json!({ "line": opt_key(id) }))
        }
        "rename" => {
            let id = route_arg(w)?;
            w.rename_bus_route(id, req_str(cmd, "name")?)?;
            Ok(json!({ "route": key_str(id), "row": route_row(id, &w.transit.routes[id]) }))
        }
        "recolour" | "recolor" => {
            let id = route_arg(w)?;
            let c = opt_u64(cmd, "color")?.ok_or("missing color (0..7)")?;
            w.recolour_bus_route(id, (c % nd_sim::vehicle::PAINTS as u64) as u8)?;
            Ok(json!({ "route": key_str(id), "row": route_row(id, &w.transit.routes[id]) }))
        }
        // Mark a stop on the map as the Lines window's "show" does; no
        // route clears the mark. look: true also points the camera at it.
        "select_stop" => {
            if field(cmd, "route").is_none() {
                ctx.tools.stop_sel = None;
                return Ok(json!({ "stop": Value::Null }));
            }
            let id = route_arg(w)?;
            let idx = opt_u64(cmd, "index")?.ok_or("missing stop index")? as usize;
            let pos = w.transit.routes[id].stops.get(idx).ok_or("no such stop")?.pos;
            ctx.tools.route_sel = Some(id);
            ctx.tools.stop_sel = Some((id, idx));
            if cmd.get("look").and_then(Value::as_bool).unwrap_or(false) {
                for mut cam in ctx.cams.iter_mut() {
                    cam.target = bevy::prelude::Vec3::new(pos.x, 0.0, pos.y);
                    cam.distance = cam.distance.min(120.0);
                }
            }
            Ok(json!({ "route": key_str(id), "index": idx, "pos": [pos.x, pos.y] }))
        }
        _ => Err(format!("unknown bus action {action:?}; one of select, new_route, add_stop, insert_stop, move_stop, remove_stop, set_buses, set_fare, remove_route, rename, recolour, line, problems, check, select_stop, list, edit, finish_edit, click, slot, select_metro")),
    }
}
