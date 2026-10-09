//! Planning: projects, roads, roundabouts, interchanges, zones, painting
//! parcels and bulldozing gestures.

use nd_sim::roads::RoadKind;
use nd_sim::{Gesture, GestureId};
use serde_json::{json, Value};

use super::ctx::Ctx;
use super::view::*;
use super::{grounds, junction};
use crate::tools::{marquee_rect, ToolState, BRUSH_MAX, BRUSH_MIN};

pub(super) fn project_state(ctx: &Ctx) -> Value {
    let w = &ctx.sim.world;
    let projects: Vec<Value> = w
        .projects()
        .into_iter()
        .map(|i| {
            json!({
                "id": i.id, "name": i.name, "active": i.active, "steps": i.steps, "redo": i.redo,
                "conflicts": i.conflicts.iter().map(|(g, o)| json!({ "gesture": g.0, "project": o })).collect::<Vec<_>>(),
                "shared_parcels": i.shared_parcels,
            })
        })
        .collect();
    json!({
        "project": w.project.as_ref().map(|p| json!({ "id": p.id, "name": p.name, "steps": p.steps.len(), "redo": p.redo.len() })),
        "projects": projects,
        "warnings": w.plan_warnings,
        // The last implement refused (its roads would take a city
        // structure), and the structures the pending roads run over now.
        "implement_refused": w.implement_refused,
        "blockers": w.project_blockers().iter().map(|b| json!({ "id": key_str(*b), "name": w.buildings.get(*b).map(|x| x.name.clone()) })).collect::<Vec<_>>(),
        "gestures": w.effective_plan().gestures.len(),
        "plan_version": w.plan_version,
    })
}

/// `project {action, id?, name?}`: several named projects, one active.
/// `id` (or `name`) picks the project; without either the active one.
pub(super) fn project(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let action = req_str(cmd, "action")?.to_ascii_lowercase();
    let name = opt_str(cmd, "name")?;
    let w = &mut ctx.sim.world;
    // The project a command is about: by id, by name, or the active one.
    let target = match opt_u64(cmd, "id")? {
        Some(id) => Some(w.projects().into_iter().find(|i| i.id == id).map(|i| i.id).ok_or_else(|| format!("no project {id}"))?),
        None => match name.filter(|_| action != "new" && action != "rename") {
            Some(n) => Some(w.projects().into_iter().find(|i| i.name.eq_ignore_ascii_case(n.trim())).map(|i| i.id).ok_or_else(|| format!("no project named {n:?}"))?),
            None => w.project.as_ref().map(|p| p.id),
        },
    };
    let need = |t: Option<u64>| t.ok_or_else(|| "no active project; give an id".to_string());
    let did: Value = match action.as_str() {
        "list" => json!(true),
        "start" => {
            w.start_project();
            json!(w.project.as_ref().map(|p| p.id))
        }
        "new" => {
            let id = w.new_project(name);
            ctx.tools.points.clear();
            json!(id)
        }
        "switch" => {
            let id = need(target)?;
            ctx.tools.points.clear();
            ctx.tools.stroke = None;
            json!(w.switch_project(id))
        }
        "park" => json!(w.park_project()),
        "rename" => {
            let id = need(target)?;
            let n = name.ok_or("rename needs a name")?;
            json!(w.rename_project(id, n))
        }
        "delete" => {
            let id = need(target)?;
            ctx.tools.points.clear();
            json!(w.delete_project(id))
        }
        "discard" => {
            let had = target.is_some();
            if let Some(id) = target {
                w.delete_project(id);
            }
            ctx.tools.points.clear();
            json!(had)
        }
        "implement" => {
            let id = need(target)?;
            let name = w.projects().into_iter().find(|i| i.id == id).map_or_else(|| format!("Project {id}"), |i| i.name);
            let roads = if w.project.as_ref().is_some_and(|p| p.id == id) { w.preview().planned_roads.len() } else { 0 };
            let warnings = w.implement_project(id).ok_or_else(|| format!("no project {id}"))?;
            ctx.tools.points.clear();
            match w.implement_refused.clone() {
                // Its roads would take a city structure, or a raised road has
                // no room for its columns: still pending, nothing built, and
                // the reply's `refused` and `blockers` say why.
                Some(why) => {
                    ctx.tools.status = crate::tools::refused_text(&name, roads, &why);
                    json!(false)
                }
                None => {
                    ctx.tools.status = if warnings.is_empty() { "Project implemented".into() } else { format!("Project implemented. {}", warnings.join(" ")) };
                    json!(true)
                }
            }
        }
        "undo" => json!(target.is_some_and(|id| w.undo_project(id))),
        "redo" => json!(target.is_some_and(|id| w.redo_project(id))),
        _ => return Err(format!("unknown project action {action:?}; one of list, new, switch, rename, delete, park, start, implement, undo, redo, discard")),
    };
    let mut v = project_state(ctx);
    v["did"] = did;
    Ok(v)
}

/// `paint {land_use, at+radius | rect | polygon, implement?}`: the zone
/// brush and marquee by remote. `land_use` "none" (or "unzoned") erases.
/// Only parcels that accept the use take it; the rest are listed with why.
pub(super) fn paint(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let lu_s = req_str(cmd, "land_use")?;
    let land_use = match lu_s.trim().to_ascii_lowercase().as_str() {
        "none" | "unzoned" | "erase" => None,
        // [waterfront] a public beach: a park on parcels down to the water
        "beach" | "public_beach" | "public beach" => Some(nd_sim::LandUse::Park),
        other => Some(land_use_of(other)?),
    };
    let w = &mut ctx.sim.world;
    let ids: Vec<usize> = if let Some(at) = opt_vec2(cmd, "at")? {
        let r = opt_f32(cmd, "radius")?.unwrap_or(ctx.tools.brush_radius).clamp(BRUSH_MIN, BRUSH_MAX * 4.0);
        w.parcels.in_circle(at, r)
    } else if let Some(rect) = field(cmd, "rect") {
        let pts = rect.as_array().filter(|a| a.len() == 2).ok_or("rect must be [[x0, y0], [x1, y1]]")?;
        let (a, b) = (vec2_of(&pts[0])?, vec2_of(&pts[1])?);
        w.parcels.in_polygon(&marquee_rect(a, b, 0.0))
    } else if field(cmd, "polygon").is_some() {
        w.parcels.in_polygon(&req_points(cmd, "polygon")?)
    } else {
        return Err("give at (+ radius), rect or polygon".into());
    };
    let beach = matches!(lu_s.trim().to_ascii_lowercase().as_str(), "beach" | "public_beach" | "public beach");
    let refused: Vec<Value> = match land_use {
        Some(lu) => ids.iter().filter_map(|&i| (if beach { w.beach_refusal(i) } else { w.parcel_refusal(i, Some(lu)) }).map(|why| json!({ "parcel": i, "center": v2(w.parcels.parcels[i].center), "why": why }))).collect(),
        None => Vec::new(),
    };
    // [island-lots] Nothing under the brush: why (water, too far from a
    // road, past the buildable land), as the hover says at the cursor.
    let none_why = if ids.is_empty() { opt_vec2(cmd, "at")?.and_then(|at| w.no_parcel_why(at)) } else { None };
    let industry = match opt_str(cmd, "industry")? {
        _ if beach => Some(nd_sim::shore::BEACH_SUBZONE),
        Some(k) => Some(nd_sim::world::INDUSTRY_CHOICES.iter().map(|(key, _)| *key).find(|key| key.eq_ignore_ascii_case(k.trim())).ok_or_else(|| format!("unknown industry {k:?}; one of {}", nd_sim::world::INDUSTRY_CHOICES.map(|c| c.0).join(", ")))?),
        None => None,
    };
    let (gesture, painted) = w.paint_parcels_with(&ids, land_use, industry, None);
    let implemented = opt_bool(cmd, "implement")?.unwrap_or(false);
    if implemented {
        w.implement();
    }
    let what = land_use.map_or("unzoned".to_string(), |l| format!("zoned {}", l.label()));
    ctx.tools.status = format!("{painted} parcels {what} by remote{}", if implemented { " and implemented" } else { " (pending)" });
    let mut v = project_state(ctx);
    v["gesture_id"] = json!(gesture.map(|g| g.0));
    v["touched"] = json!(ids.len());
    v["painted"] = json!(painted);
    v["refused"] = json!(refused);
    if let Some(why) = none_why {
        v["no_parcel"] = json!(why);
    }
    v["implemented"] = json!(implemented && ctx.sim.world.implement_refused.is_none());
    Ok(v)
}

pub(super) fn plan(ctx: &Ctx) -> Value {
    let w = &ctx.sim.world;
    let gestures: Vec<Value> = w.effective_plan().gestures.iter().map(|(id, g)| gesture(*id, g)).collect();
    // Which of them are pending rather than built: anything the project touches.
    let pending: Vec<u64> = w.project.as_ref().map(|p| p.steps.iter().flat_map(|s| s.keys().map(|k| k.0)).collect()).unwrap_or_default();
    let mut v = project_state(ctx);
    v["gestures"] = json!(gestures);
    v["pending_gesture_ids"] = json!(pending);
    v["master_gestures"] = json!(w.master.gestures.len());
    // [tools] What implementing the active project would refuse on, checked
    // now as implementing checks it (the trial rebuild included).
    let check = w.active_project_check();
    // Exactly what Implement would answer now (None: it implements).
    v["will_refuse"] = json!(check.refusal);
    // Every structure it would refuse on, with the road that takes it.
    v["blockers"] = json!(check
        .blocks
        .iter()
        .map(|k| json!({
            "id": key_str(k.building), "name": k.name, "at": [k.at.x, k.at.y],
            "road_gesture": k.road.map(|g| g.0), "road": k.road_label,
            "stretch": [[k.a.x, k.a.y], [k.b.x, k.b.y]],
            "runs_over": k.runs_over, "reason": k.reason(),
        }))
        .collect::<Vec<_>>());
    v["pillar_gaps"] = json!(check.pillar_gaps.iter().map(junction::gap_json).collect::<Vec<_>>());
    v
}

pub(super) fn add_road(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let (mut pts, given) = req_points_levels(cmd, "points")?;
    // `elevation`: one level for every point without its own, or a list.
    let all = opt_f32(cmd, "elevation").ok().flatten();
    let list: Option<Vec<f32>> = field(cmd, "elevation").and_then(Value::as_array).map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0) as f32).collect());
    let tunnel = opt_bool(cmd, "tunnel")?.unwrap_or(false);
    let base = all.unwrap_or(if tunnel { nd_sim::roads::TUNNEL_DEPTH } else { 0.0 });
    let mut levels: Vec<f32> = given.iter().enumerate().map(|(i, l)| l.or_else(|| list.as_ref().and_then(|v| v.get(i).copied())).unwrap_or(base)).collect();
    if pts.len() < 2 {
        return Err("a road needs at least two points".into());
    }
    let kind = opt_str(cmd, "kind")?.map(road_kind_of).transpose()?.unwrap_or(RoadKind::Street);
    // [park-paths] A path may start or end on a grounds gate by its index
    // in `grounds_junctions`.
    for (key, first) in [("from_junction", true), ("to_junction", false)] {
        if let Some(i) = opt_u64(cmd, key)? {
            if kind != RoadKind::Path {
                return Err(format!("{key}: only a path joins a park's walkways"));
            }
            let j = ctx.sim.world.grounds_junctions().get(i as usize).ok_or_else(|| format!("{key}: no junction {i} (see grounds_junctions)"))?;
            let at = if first { 0 } else { pts.len() - 1 };
            pts[at] = j.pos;
            levels[at] = 0.0;
        }
    }
    // Range-checked as u64 before narrowing: `as u8` wrapped 257 to 1, and
    // a u8 sum of 200 + 100 overflowed (a panic in a dev build).
    let (lf, lb) = if kind == RoadKind::Street { (opt_u64(cmd, "lanes_fwd")?.unwrap_or(1), opt_u64(cmd, "lanes_bwd")?.unwrap_or(1)) } else { (0, 0) };
    if lf > 4 || lb > 4 {
        return Err("lanes are 0..4 per direction".into());
    }
    let (lf, lb) = (lf as u8, lb as u8);
    if kind == RoadKind::Street && lf + lb == 0 {
        return Err("a street needs at least one lane".into());
    }
    // Snap the ends to the network like the Road tool does, so the road
    // joins existing roads instead of stopping a metre short of them.
    // Points snap onto a road at about their level; an end given no level of
    // its own takes the level of the road it snaps onto (a ramp or portal).
    if opt_bool(cmd, "snap")?.unwrap_or(true) {
        let last = pts.len() - 1;
        for (i, p) in pts.iter_mut().enumerate() {
            let end = (i == 0 || i == last) && given[i].is_none();
            let (q, snapped, l) = ToolState::road_snap_level(&ctx.sim, *p, kind, levels[i], end);
            *p = q;
            if snapped {
                levels[i] = l;
            }
        }
    }
    let curved = opt_bool(cmd, "curved")?.unwrap_or(false) && pts.len() >= 3;
    let warnings = ctx.sim.world.road_grade_warnings(&pts, &levels, kind, lf, lb);
    // As the Road tool: a crossing with the railway off grade is refused.
    let (bp, bl) = if curved { (nd_sim::plan::sample_curve(&pts), nd_sim::plan::sample_curve_levels(&pts, &levels)) } else { (pts.clone(), levels.clone()) };
    if let Some(why) = ctx.sim.world.road_refusal(&bp, &bl, kind, lf, lb) {
        return Err(why);
    }
    let elevation = if levels.iter().any(|l| *l != 0.0) { levels } else { Vec::new() };
    let id = ctx.sim.world.add_gesture(Gesture::Road { points: pts, lanes_fwd: lf, lanes_bwd: lb, curved, kind, elevation });
    let implemented = opt_bool(cmd, "implement")?.unwrap_or(false);
    if implemented {
        ctx.sim.world.implement();
    }
    ctx.tools.status = match &ctx.sim.world.implement_refused {
        Some(why) if implemented => why.clone(),
        _ => format!("{} added by remote{}", match kind { RoadKind::Path => "path", RoadKind::Rail => "track", RoadKind::Street => "road" }, if implemented { " and implemented" } else { " (pending)" }),
    };
    let mut v = project_state(ctx);
    v["gesture_id"] = json!(id.0);
    v["implemented"] = json!(implemented && ctx.sim.world.implement_refused.is_none());
    if kind == RoadKind::Path {
        // [park-paths] Ends on a grounds gate (joined once implemented).
        let ends = match ctx.sim.world.gesture(id) {
            Some(Gesture::Road { points, .. }) if points.len() >= 2 => vec![points[0], points[points.len() - 1]],
            _ => Vec::new(),
        };
        let w = &ctx.sim.world;
        v["joined"] = json!(ends
            .iter()
            .filter_map(|p| w.grounds_junctions().iter().position(|j| j.pos.distance(*p) < 1.0))
            .map(|i| grounds::junction_json(w, i))
            .collect::<Vec<_>>());
    }
    v["grade_warnings"] = json!(warnings);
    Ok(v)
}

/// `add_roundabout {center, radius | diameter, lanes, implement?}`: stamp a
/// roundabout into the project as the Road tool does (the centre snaps onto
/// a junction or street). Reply: what it cuts and joins and the buildings
/// it would take, besides the project state.
pub(super) fn add_roundabout(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let center = opt_vec2(cmd, "center")?.map_or_else(|| req_point(cmd), Ok)?;
    let radius = match (opt_f32(cmd, "radius")?, opt_f32(cmd, "diameter")?) {
        (Some(r), _) => r,
        (None, Some(d)) => d * 0.5,
        (None, None) => ctx.tools.ring_radius,
    };
    let lanes = opt_u64(cmd, "lanes")?.map_or(ctx.tools.ring_lanes, |n| u8::try_from(n).unwrap_or(u8::MAX));
    if !(1..=nd_sim::roads::RING_MAX_LANES).contains(&lanes) {
        return Err("a roundabout has 1, 2 or 3 ring lanes".into());
    }
    let id = ctx.sim.world.add_roundabout(center, radius, lanes)?;
    let ring = ctx.sim.world.gesture(id).and_then(|g| g.ring()).ok_or("roundabout not in the plan")?;
    let fx = nd_sim::plan::ring_effect(ctx.sim.world.effective_plan(), ring);
    let takes: Vec<Value> = ctx.sim.world.roundabout_takes(ring).into_iter().filter_map(|l| ctx.sim.world.lots.get(l).and_then(|x| x.building)).map(|b| json!(key_str(b))).collect();
    let implemented = opt_bool(cmd, "implement")?.unwrap_or(false);
    if implemented {
        ctx.sim.world.implement();
    }
    ctx.tools.status = format!("Roundabout {:.0} m across added by remote{}", ring.radius * 2.0, if implemented { " and implemented" } else { " (pending)" });
    let mut v = project_state(ctx);
    v["gesture_id"] = json!(id.0);
    v["implemented"] = json!(implemented && ctx.sim.world.implement_refused.is_none());
    v["roundabout"] = json!({
        "center": [ring.center.x, ring.center.y], "radius": ring.radius, "diameter": ring.radius * 2.0, "lanes": ring.lanes,
        "joins": fx.joins.iter().map(|p| json!([p.x, p.y])).collect::<Vec<_>>(),
        "cuts_roads": fx.cut_roads.iter().map(|g| g.0).collect::<Vec<_>>(),
        "takes_buildings": takes,
    });
    Ok(v)
}

/// [interchange] `add_interchange {from, to, lanes_fwd?, lanes_bwd?, check?,
/// implement?}`: a diamond interchange for a cross street drawn from `from`
/// to `to` across the highway, as the Road tool's Interchange preset lays
/// it. `check` only plans it (nothing is added). Refused with `refused`
/// {why, at}: the spot to look at.
pub(super) fn add_interchange(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let from = opt_vec2(cmd, "from")?.ok_or("from [x,y] is required")?;
    let to = opt_vec2(cmd, "to")?.ok_or("to [x,y] is required")?;
    let f = opt_u64(cmd, "lanes_fwd")?.map_or(2, |n| n.min(4) as u8);
    let b = opt_u64(cmd, "lanes_bwd")?.map_or(2, |n| n.min(4) as u8);
    let check = opt_bool(cmd, "check")?.unwrap_or(false);
    let w = &mut ctx.sim.world;
    let planned = if check { w.plan_diamond(from, to, (f, b)).map(|p| (p, Vec::new())) } else { w.add_diamond(from, to, (f, b)) };
    let (plan, ids) = match planned {
        Ok(x) => x,
        Err(r) => {
            let mut v = project_state(ctx);
            v["refused"] = json!({ "why": r.why, "at": [r.at.x, r.at.y] });
            v["implemented"] = json!(false);
            return Ok(v);
        }
    };
    let implemented = !check && opt_bool(cmd, "implement")?.unwrap_or(false);
    if implemented {
        ctx.sim.world.implement();
    }
    if !check {
        ctx.tools.status = format!("Diamond interchange added by remote{}", if implemented { " and implemented" } else { " (pending)" });
    }
    let mut v = project_state(ctx);
    v["gesture_ids"] = json!(ids.iter().map(|g| g.0).collect::<Vec<_>>());
    v["implemented"] = json!(implemented && ctx.sim.world.implement_refused.is_none());
    let pts = |g: &Gesture| match g {
        Gesture::Road { points, lanes_fwd, lanes_bwd, .. } => json!({ "points": points.iter().map(|p| [round(p.x), round(p.y)]).collect::<Vec<_>>(), "lanes_fwd": lanes_fwd, "lanes_bwd": lanes_bwd }),
        _ => json!(null),
    };
    v["interchange"] = json!({
        "kept_street": plan.kept_street,
        "terminals": plan.terminals.iter().map(|p| [round(p.x), round(p.y)]).collect::<Vec<_>>(),
        "crossings": plan.crossings.iter().map(|p| [round(p.x), round(p.y)]).collect::<Vec<_>>(),
        "roads": plan.gestures.iter().map(pts).collect::<Vec<_>>(),
    });
    Ok(v)
}

pub(super) fn add_zone(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let polygon = req_points(cmd, "polygon")?;
    if polygon.len() < 3 {
        return Err("a zone needs at least three corners".into());
    }
    let land_use = land_use_of(req_str(cmd, "land_use")?)?;
    let id = ctx.sim.world.add_gesture(Gesture::Zone { polygon, land_use });
    let implemented = opt_bool(cmd, "implement")?.unwrap_or(false);
    if implemented {
        ctx.sim.world.implement();
    }
    ctx.tools.status = format!("{} zone added by remote{}", land_use.label(), if implemented { " and implemented" } else { " (pending)" });
    let mut v = project_state(ctx);
    v["gesture_id"] = json!(id.0);
    v["implemented"] = json!(implemented && ctx.sim.world.implement_refused.is_none());
    Ok(v)
}

/// [park-paths] `grounds_junctions {lot?, kind?, near?, radius?}`: the gates
/// a path may join.
pub(super) fn grounds_junctions(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let lot: Option<nd_sim::LotId> = field(cmd, "lot").map(key_from).transpose()?;
    let near = opt_vec2(cmd, "near")?;
    let radius = opt_f32(cmd, "radius")?.unwrap_or(150.0);
    Ok(grounds::list(&ctx.sim.world, lot, opt_str(cmd, "kind")?, near, radius))
}

/// `bulldoze {gesture_id, implement?}` marks a gesture for removal;
/// [free-placement] `bulldoze {building}` takes a city building at once,
/// its ground back to the zoning plan.
pub(super) fn bulldoze(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
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
    Ok(project_state(ctx))
}
