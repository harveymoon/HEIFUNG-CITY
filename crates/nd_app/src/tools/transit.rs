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
    let Frame { ctrl, click, shift_click, ground, finish, right_click, buttons, .. } = *f;
    let map_key = |k: KeyCode| f.map_key(k);
    match tools.transit_mode {
    TransitMode::Route => {
        // [lines] Editing a line: clicks add, insert, share, drag and
        // remove its stops (`line_edit`). Enter finishes.
        if tools.line_edit.is_some() {
            let remove = right_click || map_key(KeyCode::Delete);
            // [fix/buildings-ui] Shift+click inserts a stop where it belongs.
            tools.line_shift = f.keys.pressed(KeyCode::ShiftLeft) || f.keys.pressed(KeyCode::ShiftRight);
            super::line_edit::input(&mut sim.world, tools, ground, click || shift_click, shift_click, buttons.just_released(MouseButton::Left), remove);
            if map_key(KeyCode::Enter) && !ctrl {
                super::line_edit::finish(&mut sim.world, tools);
            }
            return;
        }
        // Not editing: a click on a stop picks its line; a click on a
        // street starts a new line and edits it.
        tools.hover_stop = ground.and_then(|p| sim.world.transit.stop_near(p, super::line_edit::STOP_PICK));
        if click {
            if let Some((r, _)) = tools.hover_stop {
                tools.route_sel = Some(r);
                tools.status = format!("{} picked. The Lines window's Edit line edits it; or click a street to start a new line.", sim.world.transit.routes[r].name);
            } else if let Some(p) = ground {
                if nd_sim::transit::BusStop::snap(&sim.world.roads, p).is_some() {
                    let r = sim.world.add_bus_route();
                    super::line_edit::start_new(tools, r);
                    if let Some(e) = tools.line_edit.as_mut() {
                        e.back_to = super::Tool::Transit;
                    }
                    super::line_edit::input(&mut sim.world, tools, ground, true, false, false, false);
                } else {
                    tools.status = "Click by a street to start a new line, or on a stop to pick its line.".into();
                }
            }
        }
        let _ = finish;
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
