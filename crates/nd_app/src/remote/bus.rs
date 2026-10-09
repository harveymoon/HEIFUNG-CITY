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
        "list" => Ok(json!({ "routes": w.transit.routes.iter().map(|(id, r)| route_row(id, r)).collect::<Vec<_>>() })),
        // [lines] The Lines window's figures for one line.
        "line" => {
            let id = route_arg(w)?;
            Ok(line_detail(w, id))
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
        _ => Err(format!("unknown bus action {action:?}; one of select, new_route, add_stop, move_stop, remove_stop, set_buses, remove_route, rename, recolour, line, select_stop, list")),
    }
}
