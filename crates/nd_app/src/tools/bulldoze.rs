//! The Bulldoze tool: a gesture, a city building or a toll booth taken
//! away, or contaminated ground cleaned up; its preview.

use bevy::prelude::*;
use nd_sim::Vec2 as SVec2;

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
    if click {
        if let Some(p) = ground {
            tools.status = match sim.world.start_cleanup(p, tools.cleanup_radius) {
                Ok(q) => format!("Clean-up crew sent: {:.0} m round, {} stained cells, paid {:.0}; digging for {} days.", q.radius, q.cells, q.cost, q.days),
                Err(e) => format!("Cannot clean up: {e}."),
            };
        }
    }
}

/// The city building under the pointer, else the gesture; a toll booth
/// first of all.
pub(super) fn input(f: &Frame, sim: &mut Sim, tools: &mut ToolState, _selection: &mut Selection) {
    let Frame { click, ground, plan, .. } = *f;
    tools.hover_gesture = ground.and_then(|p| nearest_gesture(plan, p)).map(|(id, _)| id);
    // [free-placement] A city building under the pointer is what a
    // click takes, before the zone or road around it.
    tools.hover_city_building = ground.and_then(|p| {
        let w = &sim.world;
        let l = w.lots.values().find(|l| l.contains(p))?;
        l.building.filter(|&b| w.is_city_building(b) && w.buildings.get(b).is_some_and(|b| b.rail.is_none()))
    });
    if tools.hover_city_building.is_some() {
        tools.hover_gesture = None;
    }
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
        if let Some(id) = tools.hover_gesture {
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
