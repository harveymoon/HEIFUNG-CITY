//! The Bulldoze tool: a gesture, a city building or a toll booth taken
//! away, or contaminated ground cleaned up; its preview.

use bevy::prelude::*;
use nd_sim::{Gesture, Vec2 as SVec2};

use super::input::Frame;
use super::{Preview, ToolState};
use crate::selection::Selection;
use crate::sim::Sim;
use super::edit::nearest_gesture;

/// [dirty-industry] Clean-up: the hover label shows area, cost and
/// days (`render::plot_hover`); a click sends the crew.
pub(super) fn cleanup_input(f: &Frame, sim: &mut Sim, tools: &mut ToolState, _selection: &mut Selection) {
    let Frame { click, ground, .. } = *f;
    tools.hover_gesture = None;
    tools.hover_city_building = None;
    tools.hover_piece = None;
    if click {
        if let Some(p) = ground {
            tools.status = match sim.world.start_cleanup(p, tools.cleanup_radius) {
                Ok(q) => format!("Clean-up crew sent: {:.0} m round, {} stained cells, paid {:.0}; digging for {} days.", q.radius, q.cells, q.cost, q.days),
                Err(e) => format!("Cannot clean up: {e}."),
            };
        }
    }
}

/// [pt-ux] The piece of road `id` a click at `p` takes: (road, metres
/// along it from, to).
fn piece_of(sim: &Sim, id: nd_sim::GestureId, p: SVec2, depth: super::Depth) -> Option<(nd_sim::GestureId, f32, f32)> {
    let piece = crate::road_select::pick(&sim.world, p, depth);
    sim.world.road_piece_span(id, piece, p).map(|(a, b)| (id, a, b))
}

/// [pt-ux] What a Bulldoze click at `p` takes off a road, for the remote
/// API: the road under it and the piece.
pub(crate) fn piece_at(sim: &Sim, p: SVec2, depth: super::Depth) -> Option<(nd_sim::GestureId, f32, f32)> {
    let id = nearest_gesture(sim.world.effective_plan(), p)?.0;
    piece_of(sim, id, p, depth)
}

/// The city building under the pointer, else the gesture; a toll booth
/// first of all.
pub(super) fn input(f: &Frame, sim: &mut Sim, tools: &mut ToolState, _selection: &mut Selection) {
    let Frame { click, ground, plan, .. } = *f;
    // [pt-ux] What the pointer is over (or the remote API's pin, for the preview).
    let hover = tools.cursor_pin.or(ground);
    tools.hover_gesture = hover.and_then(|p| nearest_gesture(plan, p)).map(|(id, _)| id);
    // [free-placement] A city building under the pointer is what a
    // click takes, before the zone or road around it.
    tools.hover_city_building = hover.and_then(|p| {
        let w = &sim.world;
        let l = w.lots.values().find(|l| l.contains(p))?;
        l.building.filter(|&b| w.is_city_building(b) && w.buildings.get(b).is_some_and(|b| b.rail.is_none()))
    });
    if tools.hover_city_building.is_some() {
        tools.hover_gesture = None;
    }
    // [pt-ux] On a road, only the piece between its junctions under the
    // pointer goes, not the whole road.
    tools.hover_piece = match (tools.hover_gesture, hover) {
        (Some(id), Some(p)) => piece_of(sim, id, p, tools.depth),
        _ => None,
    };
    // [tolls] A click on a toll booth takes the booth away, not the road.
    if click {
        if let Some(booth) = ground.and_then(|p| sim.world.tolls.nearest(p, 8.0)).map(|b| (b.id, b.name.clone())) {
            let _ = sim.world.remove_toll(booth.0);
            tools.status = format!("{} removed.", booth.1);
            return;
        }
        if let Some(bid) = tools.hover_city_building.take() {
            tools.status = match sim.world.bulldoze_building(bid) {
                Ok(msg) => format!("{msg}; its ground goes back to the zoning plan."),
                Err(e) => format!("Cannot bulldoze: {e}."),
            };
            return;
        }
    }
    if click {
        if let Some((id, a, b)) = tools.hover_piece.filter(|h| Some(h.0) == tools.hover_gesture) {
            let left = sim.world.remove_road_piece(id, a, b);
            tools.status = match left {
                0 => "Road marked for removal. Ctrl+Enter implements.".into(),
                _ => format!("Road piece ({:.0} m) marked for removal; the rest stays. Ctrl+Enter implements.", (b - a).abs()),
            };
        } else if let Some(id) = tools.hover_gesture {
            sim.world.remove_gesture(id);
            tools.status = "Marked for removal. Ctrl+Enter implements.".into();
        }
    }
}

/// What a click would take, in red.
pub(super) fn preview(gizmos: &mut Gizmos, p: &Preview) {
    let Preview { tools, sim, plan, .. } = *p;
    let lift = |q: SVec2| super::lift(sim, q);
    let red = super::RED;
    // [free-placement] The city building a click would bulldoze.
    if let Some(l) = tools.hover_city_building.and_then(|b| sim.world.buildings.get(b)).and_then(|b| sim.world.lots.get(b.lot)) {
        let mut pts: Vec<Vec3> = l.polygon.iter().map(|&p| lift(p)).collect();
        if pts.len() >= 3 {
            if let Some(&first) = pts.first() { pts.push(first); }
            gizmos.linestrip(pts, red);
        }
    }
    // [pt-ux] A road's piece a click takes: red and thick; the rest of
    // the road faint.
    if let Some((id, a, b)) = tools.hover_piece.filter(|h| Some(h.0) == tools.hover_gesture) {
        if let Some(g) = plan.gestures.get(&id) {
            let line = g.road_polyline();
            // The rest of the road, which stays: a thin dim line.
            gizmos.linestrip(line.iter().map(|&p| lift(p) + Vec3::Y * 0.3), Color::linear_rgb(0.22, 0.08, 0.08));
            // The piece a click takes: a red band across the road, ends marked.
            let piece = nd_sim::plan::polyline_between(&line, a, b);
            let half = if let Gesture::Road { lanes_fwd, lanes_bwd, kind, .. } = g { nd_sim::roads::half_width_spec(*kind, *lanes_fwd, *lanes_bwd) } else { 4.0 };
            let side = |i: usize| {
                let (p, q) = if i + 1 < piece.len() { (piece[i], piece[i + 1]) } else { (piece[i.saturating_sub(1)], piece[i]) };
                let d = (q - p).normalize_or_zero();
                SVec2::new(-d.y, d.x)
            };
            for k in -3..=3 {
                let off = half * 0.85 * k as f32 / 3.0;
                gizmos.linestrip(piece.iter().enumerate().map(|(i, &p)| lift(p + side(i) * off) + Vec3::Y * 0.5), red);
            }
            for i in [0, piece.len().saturating_sub(1)] {
                let end = piece[i];
                let n = side(i) * (half + 1.5);
                for up in [0.5, 1.5, 2.5] {
                    gizmos.line(lift(end - n) + Vec3::Y * up, lift(end + n) + Vec3::Y * up, red);
                }
            }
            return;
        }
    }
    if let Some(id) = tools.hover_gesture {
        if let Some(g) = plan.gestures.get(&id) {
            let mut pts: Vec<Vec3> = g.points().iter().map(|&p| lift(p)).collect();
            if !g.is_road() && pts.len() >= 3 {
                if let Some(&first) = pts.first() { pts.push(first); }
            }
            gizmos.linestrip(pts, red);
        }
    }
}
