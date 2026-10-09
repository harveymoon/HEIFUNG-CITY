//! The Buses tool: bus lines stop by stop, depots, and metro lines station
//! to station (`metro_ux` has the Metro layer and the line rules).

use bevy::prelude::*;
use nd_sim::Vec2 as SVec2;

use super::input::Frame;
use super::ToolState;
use crate::selection::Selection;
use crate::sim::Sim;
use nd_sim::zoning::point_in_polygon;
use super::TransitMode;

/// Lines, Metro or Depot mode.
pub(super) fn input(f: &Frame, sim: &mut Sim, tools: &mut ToolState, selection: &mut Selection) {
    let Frame { ctrl, click, ground, finish, right_click, buttons, .. } = *f;
    let map_key = |k: KeyCode| f.map_key(k);
    match tools.transit_mode {
    TransitMode::Route => {
        tools.hover_stop = ground.and_then(|p| sim.world.transit.stop_near(p, 5.0));
        if click {
            if let Some((r, i)) = tools.hover_stop {
                tools.route_sel = Some(r);
                tools.stop_drag = Some((r, i));
                tools.status = format!("{} selected. Drag the stop to move it, Delete removes it, click a street to add stops.", sim.world.transit.routes[r].name);
            } else if let Some(p) = ground {
                let r = match tools.route_sel {
                    Some(r) if sim.world.transit.routes.contains_key(r) => r,
                    _ => {
                        let r = sim.world.add_bus_route();
                        tools.route_sel = Some(r);
                        r
                    }
                };
                tools.status = match sim.world.add_bus_stop(r, p) {
                    Ok(i) => format!("Stop {} added to {}. Keep clicking streets round a loop; Enter or right-click when done.", i + 1, sim.world.transit.routes[r].name),
                    Err(e) => format!("No stop: {e}."),
                };
            }
        }
        if buttons.just_released(MouseButton::Left) {
            if let Some((r, i)) = tools.stop_drag.take() {
                if let Some(p) = ground {
                    let moved = sim.world.transit.routes.get(r).and_then(|x| x.stops.get(i)).is_some_and(|s| s.pos.distance(p) > 2.5);
                    if moved && sim.world.move_bus_stop(r, i, p) {
                        tools.status = "Stop moved.".into();
                    }
                }
            }
        }
        if map_key(KeyCode::Delete) {
            if let Some((r, i)) = tools.hover_stop.take() {
                sim.world.remove_bus_stop(r, i);
                if !sim.world.transit.routes.contains_key(r) {
                    tools.route_sel = None;
                    tools.status = "Line removed.".into();
                } else {
                    tools.status = "Stop removed.".into();
                }
            }
        }
        if finish {
            if let Some(r) = tools.route_sel.take() {
                let n = sim.world.transit.routes.get(r).map_or(0, |x| x.stops.len());
                tools.status = if n >= 2 { "Line closed into a loop. Buses leave the depot in the morning.".into() } else { "A line needs at least two stops.".into() };
            }
        }
    }
    TransitMode::Metro => {
        // [metro-ux] Enter finishes; a right-click takes back the last stop.
        let enter = map_key(KeyCode::Enter) && !ctrl;
        metro_click(sim, tools, selection, ground, click, enter, right_click);
    }
    TransitMode::Depot => {
        if click {
            if let Some(p) = ground {
                let lot = sim.world.lots.iter().find(|(_, l)| point_in_polygon(p, &l.polygon)).map(|(id, _)| id);
                match lot {
                    Some(lot) => {
                        tools.status = match sim.world.build_depot(lot) {
                            Ok(id) => {
                                *selection = Selection::Building(id);
                                "Bus depot built. Buses leave it in the morning.".into()
                            }
                            Err(e) => format!("Cannot build: {e}."),
                        };
                    }
                    None => tools.status = "Click a vacant lot by a street.".into(),
                }
            }
        }
    }
    }
}

/// [subway] The Buses tool's Metro mode. [metro-ux] Lines are drawn station
/// to station: a click on a station (its lot, or near its entrance or
/// platform) adds it and the tunnel to it is previewed; the first station
/// again closes a loop; Enter (or the panel's Finish) lays the line; a
/// right-click or Backspace takes back the last stop; Esc drops the draft.
/// A click that misses every station does nothing but say so: stations are
/// placed with the Build window (Transit, Metro, Metro station).
#[allow(clippy::too_many_arguments)]
fn metro_click(sim: &mut Sim, tools: &mut ToolState, selection: &mut Selection, ground: Option<SVec2>, click: bool, finish: bool, undo: bool) {
    let w = &mut sim.world;
    tools.metro_draft.stations.retain(|s| w.metro_platform(*s).is_some());
    if tools.metro_draft.stations.len() < 3 {
        tools.metro_draft.looped = false;
    }
    tools.metro_hover = ground.and_then(|p| w.metro_station_at(p, crate::metro_ux::PICK_RADIUS));
    if click {
        if let Some(p) = ground {
            tools.status = match w.metro_station_at(p, crate::metro_ux::PICK_RADIUS) {
                Some(b) => {
                    let msg = crate::metro_ux::add_stop(w, &mut tools.metro_draft, b);
                    *selection = Selection::Building(b);
                    msg
                }
                None => crate::metro_ux::missed_hint(w),
            };
        }
    }
    if undo {
        tools.status = crate::metro_ux::undo_stop(w, &mut tools.metro_draft);
    }
    if finish {
        tools.status = crate::metro_ux::finish_line(w, &mut tools.metro_draft);
    }
}
