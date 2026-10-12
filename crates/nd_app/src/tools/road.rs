//! The Road tool: drawing roads, paths, track and tunnels point by point
//! at a level, stamping roundabouts, and the diamond interchange; its
//! preview (the control points at their levels; the road itself is a
//! hologram, `render::holo`).

use bevy::prelude::*;
use nd_sim::roads::RoadKind;
use nd_sim::{Gesture, Vec2 as SVec2};

use super::input::Frame;
use super::{elevation_label, Preview, ToolState};
use crate::overlay::shapes::flat_at;
use crate::selection::Selection;
use crate::sim::Sim;

/// Roundabout mode: the ring follows the pointer (or the remote pin);
/// Ctrl+wheel sizes it, a click stamps it.
pub(super) fn roundabout_input(f: &Frame, sim: &mut Sim, tools: &mut ToolState, _selection: &mut Selection) {
    let Frame { ctrl, click, ground, right_down, middle, scroll, flags, .. } = *f;
    // The pointer takes over from a centre the remote API pinned.
    if click || right_down || middle {
        tools.ring_pin = None;
    }
    // [w1-roads] A raised or sunken ring does not snap onto the junctions
    // at grade.
    let at_grade = tools.elevation.abs() < nd_sim::roads::GRADE_SEPARATION;
    tools.ring_hover = tools.ring_pin.or(if flags.pointer_over_ui { None } else { ground }).map(|p| if at_grade { sim.world.roundabout_snap(p) } else { p });
    // Ctrl+wheel sizes it (the camera leaves the wheel alone then).
    if ctrl && !flags.pointer_over_ui {
        let lines = match scroll.unit {
            bevy::input::mouse::MouseScrollUnit::Line => scroll.delta.y,
            bevy::input::mouse::MouseScrollUnit::Pixel => scroll.delta.y / 60.0,
        };
        if lines != 0.0 {
            tools.ring_radius = (tools.ring_radius + lines * (tools.ring_radius * 0.08).max(2.0)).clamp(nd_sim::roads::RING_MIN_RADIUS, nd_sim::roads::RING_MAX_RADIUS);
            tools.status = format!("Roundabout {:.0} m across.", tools.ring_radius * 2.0);
        }
    }
    if click {
        if let Some(c) = tools.ring_hover {
            let (r, lanes) = (tools.ring_radius, tools.ring_lanes);
            let level = tools.elevation;
            tools.status = match sim.world.add_roundabout_at(c, r, lanes, level) {
                Ok(_) if level != 0.0 => format!("Roundabout {:.0} m across at {} stamped into the project: roads drawn to it at its level join it, others pass under or over. Ctrl+Z undoes, Ctrl+Enter implements.", r * 2.0, elevation_label(level)),
                Ok(_) => format!("Roundabout {:.0} m across stamped into the project: roads crossing it are cut and joined, buildings in red come down. Ctrl+Z undoes, Ctrl+Enter implements.", r * 2.0),
                Err(e) => format!("No roundabout: {e}."),
            };
        }
    }
}

/// Points, then Enter or a right-click lays the road (or two clicks the
/// interchange).
pub(super) fn input(f: &Frame, sim: &mut Sim, tools: &mut ToolState, _selection: &mut Selection) {
    let Frame { click, ground, finish, middle, .. } = *f;
    if ground.is_some() && (click || middle) {
        tools.cursor_pin = None;
    }
    // [fix/roads-tools] Circle: a click for the centre, a click on the edge.
    if tools.circle && !tools.interchange && tools.kind != RoadKind::Rail {
        circle_input(click, finish, ground, sim, tools);
        return;
    }
    if click {
        if let Some(p) = ground {
            let first = tools.points.is_empty();
            let grounds = ToolState::grounds_snap(sim, p, tools.kind).and_then(|q| sim.world.grounds_junction_near(q, 0.01).map(|j| j.kind.label()));
            let (p, snapped, level) = ToolState::road_snap_level(sim, p, tools.kind, tools.elevation, first);
            tools.push_point(p, level);
            if let Some(what) = grounds {
                tools.status = format!("Snapped to the {what}'s gate: the path joins its walkways there once implemented.");
            } else if snapped && !tools.interchange {
                tools.status = format!("Snapped to the road at {}: a junction forms there.", elevation_label(level));
            }
        }
    }
    // [interchange] The Interchange preset: the second click lays the
    // diamond (or says why not, with the spot pinned on the map).
    if tools.interchange {
        if tools.points.len() == 1 && click {
            tools.status = "Interchange: now click the other side of the highway, where the cross street ends.".into();
        }
        if tools.points.len() >= 2 {
            let (a, b) = (tools.points[0], tools.points[tools.points.len() - 1]);
            let lanes = (tools.lanes_fwd.max(1), tools.lanes_bwd.max(1));
            let layout = tools.interchange_layout;
            let over = layout == nd_sim::world::interchange::InterchangeLayout::StreetOver;
            tools.status = match sim.world.add_interchange(a, b, lanes, layout) {
                Ok((plan, ids)) => format!(
                    "Diamond interchange added to the project: {}{} ramps, a signalled junction where they meet the street on each side. Ctrl+Z undoes, Ctrl+Enter implements.",
                    if plan.kept_street { "the street under the highway kept, " } else if over { "the cross street on a bridge over the highway, " } else { "the cross street under the highway, " },
                    ids.len() - usize::from(!plan.kept_street)
                ),
                Err(r) => format!("No interchange: {} (pinned in red at {:.0}, {:.0}).", r.why, r.at.x, r.at.y),
            };
            tools.points.clear();
            tools.point_levels.clear();
        } else if finish {
            tools.points.clear();
            tools.point_levels.clear();
        }
    }
    if finish && !tools.interchange {
        if tools.points.len() >= 2 {
            let kind = tools.kind;
            let (lf, lb) = if kind != RoadKind::Street { (0, 0) } else { (tools.lanes_fwd, tools.lanes_bwd) };
            let levels = tools.levels();
            let elevation = if levels.iter().any(|l| *l != 0.0) { levels.clone() } else { Vec::new() };
            let curved = tools.curved && tools.points.len() >= 3;
            // A crossing with the railway that is neither at grade
            // nor clear of it is refused, not built as a crossing
            // the track would have to dip or climb to.
            let built = if curved { nd_sim::plan::sample_curve(&tools.points) } else { tools.points.clone() };
            let built_levels = if curved { nd_sim::plan::sample_curve_levels(&tools.points, &levels) } else { levels.clone() };
            if let Some(why) = sim.world.road_refusal(&built, &built_levels, kind, lf, lb) {
                tools.status = format!("Not added: {why}.");
            } else {
                let g = Gesture::Road { points: tools.points.clone(), lanes_fwd: lf, lanes_bwd: lb, curved, kind, elevation, car_only: tools.car_only_flag() };
                sim.world.add_gesture(g);
                let warn = if tools.grade_warnings.is_empty() { String::new() } else { format!(" Warning: {}.", tools.grade_warnings.join("; ")) };
                tools.status = format!("{} {} added to project ({} points). Ctrl+Enter implements.{warn}", if tools.curved { "Curved" } else { "Straight" }, match kind { RoadKind::Path => "path", RoadKind::Rail => "track", RoadKind::Street if tools.tunnel => "tunnel", RoadKind::Street => "road" }, tools.points.len());
            }
        }
        tools.points.clear();
        tools.point_levels.clear();
    }
}

/// [fix/roads-tools] The circle: the first click sets the centre, the
/// second (on the edge) lays a closed curved road of the type and lanes
/// picked, at the level set; Enter or a right-click lets the centre go.
fn circle_input(click: bool, finish: bool, ground: Option<SVec2>, sim: &mut Sim, tools: &mut ToolState) {
    if finish {
        tools.points.clear();
        tools.point_levels.clear();
        return;
    }
    let Some(p) = ground.filter(|_| click) else { return };
    if tools.points.is_empty() {
        tools.push_point(p, tools.elevation);
        tools.status = "Circle: now click where its edge should run (Enter or right-click cancels).".into();
        return;
    }
    let center = tools.points[0];
    let pts = super::circle_road(center, p);
    let kind = tools.kind;
    let (lf, lb) = if kind != RoadKind::Street { (0, 0) } else { (tools.lanes_fwd, tools.lanes_bwd) };
    let levels = vec![tools.elevation; pts.len()];
    let elevation = if tools.elevation != 0.0 { levels.clone() } else { Vec::new() };
    let built = nd_sim::plan::sample_curve(&pts);
    let built_levels = nd_sim::plan::sample_curve_levels(&pts, &levels);
    let r = center.distance(pts[0]);
    tools.status = if let Some(why) = sim.world.road_refusal(&built, &built_levels, kind, lf, lb) {
        format!("Not added: {why}.")
    } else {
        sim.world.add_gesture(Gesture::Road { points: pts, lanes_fwd: lf, lanes_bwd: lb, curved: true, kind, elevation, car_only: tools.car_only_flag() });
        format!("Circular {} {:.0} m across added to project. Ctrl+Enter implements.", if kind == RoadKind::Path { "path" } else { "road" }, r * 2.0)
    };
    tools.points.clear();
    tools.point_levels.clear();
}

/// The roundabout itself is drawn by the renderer (`sync_ring_preview`);
/// here only a ring of the outer edge, lifted onto the ground.
pub(super) fn roundabout_preview(gizmos: &mut Gizmos, p: &Preview) {
    let Preview { tools, sim, .. } = *p;
    let lift = |q: SVec2| super::lift(sim, q);
    let (cyan, white) = (super::CYAN, super::WHITE);
    if let Some(r) = tools.ring_preview() {
        gizmos.circle(flat_at(lift(r.center)), r.radius + r.half_width(), cyan);
        gizmos.circle(flat_at(lift(r.center)), 1.2, white);
    }
}

/// The road itself is a hologram (render/holo.rs); here only the control
/// points, each at its level with a stalk to the ground.
pub(super) fn preview(gizmos: &mut Gizmos, p: &Preview) {
    let Preview { tools, cursor, sim, .. } = *p;
    let lift = |q: SVec2| super::lift(sim, q);
    let (cyan, white) = (super::CYAN, super::WHITE);
    // The road itself is a hologram (render/holo.rs); here only the
    // control points, each at its level with a stalk to the ground.
    let at = |p: SVec2, level: f32| Vec3::new(p.x, sim.world.ground_height(p).max(if sim.world.landscape.is_flat() { 0.0 } else { nd_sim::landscape::WATER_LEVEL }) + level + 0.8, p.y);
    let levels = tools.levels();
    let mut ctrl: Vec<SVec2> = tools.points.clone();
    // [park-paths] Drawing a path: every gate of a park or other
    // grounds near the pointer shows as a small ring (green where a
    // path is joined already); the one the pointer snaps to glows.
    let green = Color::linear_rgb(0.3, 3.0, 0.6);
    let gate_snap = tools.cursor_pin.or(cursor.ground).and_then(|c| ToolState::grounds_snap(&sim, c, tools.kind));
    if tools.kind == RoadKind::Path {
        let near = tools.cursor_pin.or(cursor.ground);
        for j in sim.world.grounds_junctions() {
            if near.is_some_and(|c| c.distance(j.pos) > 600.0) {
                continue;
            }
            let col = if j.attached { green } else { Color::linear_rgb(2.4, 1.6, 0.4) };
            gizmos.circle(flat_at(lift(j.pos)), 1.4, col);
            gizmos.circle(flat_at(lift(j.pos)), 0.6, col);
        }
        if let Some(g) = gate_snap {
            let pulse = 1.0 + 0.25 * (p.t * 5.0).sin();
            gizmos.circle(flat_at(lift(g)), 2.6 * pulse, green);
        }
    }
    if let Some(c) = tools.cursor_pin.or(cursor.ground) {
        let (c, snapped, level) = ToolState::road_snap_level(&sim, c, tools.kind, tools.elevation, tools.points.is_empty());
        let top = at(c, level);
        let ring = if gate_snap.is_some() { green } else if snapped { white } else { cyan };
        gizmos.circle(flat_at(top), if snapped { 4.5 } else { 3.0 }, ring);
        if level != 0.0 {
            gizmos.line(at(c, 0.0), top, cyan);
        }
        ctrl.push(c);
    }
    for (i, &p) in tools.points.iter().enumerate() {
        let l = levels.get(i).copied().unwrap_or(0.0);
        gizmos.circle(flat_at(at(p, l)), 2.0, cyan);
        if l != 0.0 {
            gizmos.line(at(p, 0.0), at(p, l), cyan);
        }
    }
    if tools.curved && ctrl.len() >= 3 {
        let mut l = levels.clone();
        l.resize(ctrl.len(), tools.elevation);
        gizmos.linestrip(ctrl.iter().zip(&l).map(|(&p, &l)| at(p, l)).collect::<Vec<_>>(), Color::linear_rgb(0.3, 0.4, 0.5));
    }
}
