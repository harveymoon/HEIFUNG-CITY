//! `tool`: the active tool and its settings, as if picked in the UI.

use nd_sim::roads::RoadKind;
use nd_sim::{GestureId, RailFacility};
use serde_json::{json, Value};

use super::ctx::Ctx;
use super::view::*;
use super::junction;
use crate::tools::{Tool, ToolState, TransitMode, ZoneMode, BRUSH_MAX, BRUSH_MIN, ROAD_PRESETS};

pub(super) fn tool_state(t: &ToolState) -> Value {
    json!({
        "name": tool_name(t.tool), "district": { "selected": t.district.selected, "mode": format!("{:?}", t.district.mode).to_lowercase(), "radius": t.district.radius, "erase": t.district.erase, "points": t.district.points.len() }, "road_kind": road_kind_name(t.kind), "lanes_fwd": t.lanes_fwd, "lanes_bwd": t.lanes_bwd, "curved": t.curved,
        "land_use": t.land_use.label(), "zone_mode": t.zone_mode.label(), "brush_radius": t.brush_radius, "unzone": t.unzone, "industry": t.industry, "building": placeable_key(t.build_item()), "building_category": t.build_cat.key(), "rail_kind": rail_name(t.rail_kind),
        "transit_mode": transit_mode_name(t.transit_mode), "route": opt_key(t.route_sel), "show_routes": t.show_routes,
        "pending_points": t.points.len(), "selected_gesture": t.selected_gesture.map(|g| g.0), "status": t.status,
        "elevation": t.elevation, "elevation_label": crate::tools::elevation_label(t.elevation), "point_levels": t.levels(), "tunnel": t.tunnel, "depth": t.depth.label(), "grade_warnings": t.grade_warnings,
        "interchange": t.interchange, "interchange_refusal": t.interchange_refusal.as_ref().map(|(why, at)| json!({ "why": why, "at": [at.x, at.y] })),
        "pillars": junction::pillars_json(&t.pillars), "intersection_editor": t.junction_editor.as_ref().map(|e| v2(e.at)),
    })
}

pub(super) fn tool(cmd: &Value, ctx: &mut Ctx) -> Result<Value, String> {
    let t = &mut ctx.tools;
    if let Some(name) = opt_str(cmd, "name")? {
        let tool = tool_of(name)?;
        if tool != t.tool {
            t.tool = tool;
            t.points.clear();
            t.drag = None;
        }
    }
    if let Some(at) = opt_vec2(cmd, "ring_at")? {
        t.ring_pin = Some(at);
        t.ring_hover = Some(at);
    }
    if let Some(d) = opt_f32(cmd, "diameter")? {
        t.ring_radius = (d * 0.5).clamp(nd_sim::roads::RING_MIN_RADIUS, nd_sim::roads::RING_MAX_RADIUS);
    }
    if let Some(n) = opt_u64(cmd, "ring_lanes")? {
        t.ring_lanes = (n.min(u8::MAX as u64) as u8).clamp(1, nd_sim::roads::RING_MAX_LANES);
    }
    if let Some(preset) = opt_str(cmd, "preset").ok().flatten().filter(|p| p.eq_ignore_ascii_case("roundabout")) {
        let _ = preset;
        t.tool = Tool::Road;
        t.roundabout = true;
        t.interchange = false;
    } else if let Some(preset) = opt_str(cmd, "preset")? {
        if !t.pick_preset(preset) {
            return Err(format!("unknown preset {preset:?}; one of {}, Tunnel, Roundabout, Interchange", ROAD_PRESETS.iter().map(|p| p.0).collect::<Vec<_>>().join(", ")));
        }
        if preset.eq_ignore_ascii_case("interchange") {
            t.tool = Tool::Road;
        }
    }
    if let Some(e) = opt_f32(cmd, "elevation")? {
        t.elevation = e.clamp(crate::tools::ELEVATION_MIN, crate::tools::ELEVATION_MAX);
    }
    if let Some(d) = opt_str(cmd, "depth")? {
        t.depth = crate::tools::Depth::parse(d).ok_or_else(|| format!("unknown depth {d:?}; underground, ground or sky"))?;
    }
    // `draw`: the points of a road being drawn, as if clicked (for the
    // preview); `cursor`: where the pointer stands for the preview.
    if field(cmd, "draw").is_some() {
        let (pts, levels) = req_points_levels(cmd, "draw")?;
        t.tool = Tool::Road;
        t.roundabout = false;
        t.points.clear();
        t.point_levels.clear();
        for (p, l) in pts.into_iter().zip(levels) {
            let l = l.unwrap_or(t.elevation);
            t.push_point(p, l);
        }
    }
    if let Some(c) = opt_vec2(cmd, "cursor")? {
        t.cursor_pin = Some(c);
    }
    // [plot-hover] `hover`: the same pin, for the Buildings and Zone tools'
    // plot highlight (the reply's `hover` says what a click there would do).
    if let Some(c) = opt_vec2(cmd, "hover")? {
        t.cursor_pin = Some(c);
    }
    if let Some(kind) = opt_str(cmd, "kind")? {
        // `kind` means whatever the active tool chooses between.
        match t.tool {
            Tool::Road | Tool::Edit => t.kind = road_kind_of(kind)?,
            Tool::Buildings => {
                let tier = opt_str(cmd, "tier")?.and_then(nd_sim::services::parse_tier);
                let cat = nd_sim::BuildCategory::ALL.iter().copied().find(|c| c.key() == kind.to_ascii_lowercase());
                match cat {
                    Some(c) => {
                        t.build_cat = c;
                        if let Some(first) = c.items().first() {
                            t.build = *first;
                        }
                    }
                    None => t.pick_build(nd_sim::Placeable::parse(kind, tier).ok_or_else(|| format!("unknown building {kind:?}; a category (power, services, transit, rail, attractions) or a kind as for place"))?),
                }
                if let Some(tier) = tier {
                    t.build_tier = tier;
                }
            }
            Tool::Rail => {
                t.rail_kind = match kind.to_ascii_lowercase().as_str() {
                    "station" => RailFacility::Station,
                    "freight_yard" | "yard" => RailFacility::FreightYard,
                    "central_station" | "central" => RailFacility::Central,
                    _ => return Err(format!("unknown rail kind {kind:?}; station, central_station or freight_yard")),
                }
            }
            Tool::Transit => {
                t.transit_mode = match kind.to_ascii_lowercase().as_str() {
                    "route" | "lines" => TransitMode::Route,
                    "depot" => TransitMode::Depot,
                    "metro" | "subway" => TransitMode::Metro,
                    _ => return Err(format!("unknown transit mode {kind:?}; route, depot or metro")),
                };
                t.route_sel = None;
            }
            Tool::Zone => match ZoneMode::parse(kind) {
                Some(m) => t.zone_mode = m,
                // [waterfront] "beach": parks painted as public beaches
                None if kind.eq_ignore_ascii_case("beach") => {
                    t.land_use = nd_sim::LandUse::Park;
                    t.beach = true;
                }
                None => {
                    t.land_use = land_use_of(kind)?;
                    t.beach = false;
                }
            },
            Tool::District => {
                t.district.mode = match kind.to_ascii_lowercase().as_str() {
                    "brush" => crate::district_tool::DistrictMode::Brush,
                    "polygon" => crate::district_tool::DistrictMode::Polygon,
                    _ => return Err("district kind is brush or polygon".into()),
                };
            }
            Tool::Bulldoze => {
                t.cleanup = match kind.to_ascii_lowercase().as_str() {
                    "cleanup" | "clean_up" | "clean" => true,
                    "remove" | "bulldoze" => false,
                    _ => return Err("bulldoze kind is remove or cleanup".into()),
                }
            }
            Tool::Inspect => return Err("this tool has no kind".into()),
        }
    }
    if let Some(lu) = opt_str(cmd, "land_use")? {
        if lu.eq_ignore_ascii_case("none") || lu.eq_ignore_ascii_case("unzoned") {
            t.unzone = true;
        } else {
            // [waterfront] "beach": parks painted as public beaches
            let beach = lu.eq_ignore_ascii_case("beach") || lu.eq_ignore_ascii_case("public_beach");
            t.land_use = if beach { nd_sim::LandUse::Park } else { land_use_of(lu)? };
            t.beach = beach;
            t.unzone = false;
        }
    }
    if let Some(m) = opt_str(cmd, "mode")? {
        t.zone_mode = ZoneMode::parse(m).ok_or_else(|| format!("unknown zone mode {m:?}; polygon, brush or marquee"))?;
        t.stroke = None;
    }
    if let Some(r) = opt_f32(cmd, "radius")? {
        if t.tool == Tool::Bulldoze {
            t.cleanup_radius = r.clamp(nd_sim::world::CLEANUP_MIN_RADIUS, nd_sim::world::CLEANUP_MAX_RADIUS);
        } else {
            t.brush_radius = r.clamp(BRUSH_MIN, BRUSH_MAX);
        }
    }
    if let Some(u) = opt_bool(cmd, "unzone")? {
        t.unzone = u;
    }
    if let Some(k) = opt_str(cmd, "industry")? {
        t.industry = nd_sim::world::INDUSTRY_CHOICES.iter().map(|(key, _)| *key).find(|key| key.eq_ignore_ascii_case(k.trim())).ok_or_else(|| format!("unknown industry {k:?}"))?;
    }
    if let Some(n) = opt_u64(cmd, "lanes_fwd")? {
        t.lanes_fwd = n.min(4) as u8;
    }
    if let Some(n) = opt_u64(cmd, "lanes_bwd")? {
        t.lanes_bwd = n.min(4) as u8;
    }
    if t.kind == RoadKind::Street && t.lanes_fwd + t.lanes_bwd == 0 {
        t.lanes_fwd = 1;
    }
    if let Some(c) = opt_bool(cmd, "curved")? {
        t.curved = c;
    }
    if let Some(s) = opt_bool(cmd, "show_routes")? {
        t.show_routes = s;
    }
    if let Some(v) = field(cmd, "route") {
        let id = key_from(v)?;
        if !ctx.sim.world.transit.routes.contains_key(id) {
            return Err("no such route".into());
        }
        t.route_sel = Some(id);
    }
    if let Some(g) = opt_u64(cmd, "selected_gesture")? {
        t.selected_gesture = Some(GestureId(g));
    }
    let mut v = json!({ "tool": tool_state(t) });
    if let Some(p) = t.cursor_pin {
        v["hover"] = match crate::render::hover_site(&ctx.sim.world, t, p) {
            Some(s) => json!({ "at": v2(p), "ok": s.verdict.is_ok(), "note": s.verdict.as_ref().ok(), "reason": s.verdict.as_ref().err(), "outline": points(&s.outline) }),
            None => Value::Null,
        };
    }
    Ok(v)
}
