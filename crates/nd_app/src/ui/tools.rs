//! The TOOLS section: the tool buttons and one sub-panel per tool.

use bevy_egui::egui;
use nd_sim::roads::RoadKind;
use nd_sim::{Gesture, LandUse, PowerPlantKind, RailFacility};

use super::help;
use super::{hint, section, zone_color32, WARN};
use crate::assets::GameAssets;
use crate::sim::Sim;
use crate::tools::{EffectivePlan, Tool, ToolState, TransitMode, ROAD_PRESETS};

pub fn tool_panel(ui: &mut egui::Ui, sim: &mut Sim, tools: &mut ToolState, plan: &mut EffectivePlan, assets: &GameAssets) {
    section(ui, "TOOLS");
    ui.horizontal_wrapped(|ui| {
        for (tool, label) in help::TOOL_LABELS {
            if ui.selectable_label(tools.tool == tool, label).clicked() {
                tools.tool = tool;
                tools.points.clear();
                tools.drag = None;
            }
        }
    });
    match tools.tool {
        Tool::Road => road(ui, tools),
        Tool::Zone => zone(ui, tools, assets),
        Tool::Edit => edit(ui, sim, tools, plan),
        Tool::Bulldoze => hint(ui, help::tool_hint(Tool::Bulldoze)),
        Tool::Inspect => hint(ui, help::tool_hint(Tool::Inspect)),
        Tool::Transit => transit(ui, sim, tools),
        Tool::Rail => rail(ui, sim, tools),
        Tool::Power => power(ui, tools),
    }
    if !tools.status.is_empty() {
        ui.label(egui::RichText::new(&tools.status).small());
    }
}

fn road(ui: &mut egui::Ui, tools: &mut ToolState) {
    ui.horizontal_wrapped(|ui| {
        for (label, f, b, k) in ROAD_PRESETS {
            let on = tools.kind == k && (k != RoadKind::Street || (tools.lanes_fwd == f && tools.lanes_bwd == b));
            if ui.selectable_label(on, label).clicked() {
                tools.kind = k;
                if k == RoadKind::Street {
                    tools.lanes_fwd = f;
                    tools.lanes_bwd = b;
                }
            }
        }
    });
    match tools.kind {
        RoadKind::Path => hint(ui, help::ROAD_PATH),
        RoadKind::Rail => hint(ui, help::ROAD_RAIL),
        RoadKind::Street => {
            ui.add(egui::Slider::new(&mut tools.lanes_fwd, 0..=4).text("lanes forward"));
            ui.add(egui::Slider::new(&mut tools.lanes_bwd, 0..=4).text("lanes backward"));
            if tools.lanes_fwd + tools.lanes_bwd == 0 {
                tools.lanes_fwd = 1;
            }
        }
    }
    ui.horizontal(|ui| {
        if ui.selectable_label(!tools.curved, "Straight").clicked() {
            tools.curved = false;
        }
        if ui.selectable_label(tools.curved, "Curved [C]").clicked() {
            tools.curved = true;
        }
    });
    hint(ui, help::tool_hint(Tool::Road));
}

fn zone(ui: &mut egui::Ui, tools: &mut ToolState, assets: &GameAssets) {
    ui.horizontal_wrapped(|ui| {
        for lu in LandUse::ALL {
            let text = egui::RichText::new(lu.label()).color(zone_color32(assets.palette.zone_color(lu)));
            if ui.selectable_label(tools.land_use == lu, text).clicked() {
                tools.land_use = lu;
            }
        }
    });
    hint(ui, help::tool_hint(Tool::Zone));
}

fn edit(ui: &mut egui::Ui, sim: &mut Sim, tools: &mut ToolState, plan: &mut EffectivePlan) {
    hint(ui, help::tool_hint(Tool::Edit));
    let Some(id) = tools.selected_gesture else { return };
    match plan.sync(&sim.world).gestures.get(&id).cloned() {
        Some(Gesture::Road { points, lanes_fwd, lanes_bwd, curved, kind }) => {
            section(ui, "ROAD UPGRADE");
            let (mut f, mut b, mut c, mut k) = (lanes_fwd, lanes_bwd, curved, kind);
            let mut changed = false;
            ui.horizontal_wrapped(|ui| {
                for (label, pf, pb, pk) in ROAD_PRESETS {
                    let on = k == pk && (pk != RoadKind::Street || (f == pf && b == pb));
                    if ui.selectable_label(on, label).clicked() {
                        k = pk;
                        f = pf;
                        b = pb;
                        changed = true;
                    }
                }
            });
            changed |= ui.add(egui::Slider::new(&mut f, 0..=4).text("lanes forward")).changed();
            changed |= ui.add(egui::Slider::new(&mut b, 0..=4).text("lanes backward")).changed();
            ui.horizontal(|ui| {
                if ui.button("Flip direction").clicked() {
                    std::mem::swap(&mut f, &mut b);
                    changed = true;
                }
                changed |= ui.checkbox(&mut c, "Curved").changed();
                if ui.button("Deselect").clicked() {
                    tools.selected_gesture = None;
                }
            });
            if changed {
                if f + b == 0 && k == RoadKind::Street {
                    f = 1;
                }
                sim.world.set_gesture(id, Gesture::Road { points, lanes_fwd: f, lanes_bwd: b, curved: c, kind: k }, true);
                tools.status = "Road changed. Ctrl+Enter implements; buildings along it stay.".into();
            }
        }
        Some(_) => hint(ui, help::EDIT_ZONE_SELECTED),
        None => tools.selected_gesture = None,
    }
}

fn transit(ui: &mut egui::Ui, sim: &mut Sim, tools: &mut ToolState) {
    ui.horizontal(|ui| {
        for (m, label) in [(TransitMode::Route, "Lines"), (TransitMode::Depot, "Depot")] {
            if ui.selectable_label(tools.transit_mode == m, label).clicked() {
                tools.transit_mode = m;
                tools.route_sel = None;
            }
        }
        ui.checkbox(&mut tools.show_routes, "Always show lines");
    });
    let t = &sim.world.economy.transit;
    if t.depots == 0 {
        ui.label(egui::RichText::new(format!("No depot: buses cannot run. Place one with Depot on a vacant lot by a street ({:.0} to build, {:.0}/day, {} buses).", nd_sim::transit::DEPOT_COST, nd_sim::transit::DEPOT_UPKEEP_PER_DAY, nd_sim::transit::DEPOT_CAPACITY)).small().color(egui::Color32::from_rgb(255, 160, 90)));
    } else {
        hint(ui, &format!("{} depot{} for {} buses · {} out · {} waiting · {} riding · {} riders today · fares {:.0} · upkeep {:.0}/day", t.depots, if t.depots == 1 { "" } else { "s" }, t.depot_capacity, t.buses, t.waiting, t.riding, t.riders_today, t.fares_today, t.upkeep_today));
    }
    match tools.transit_mode {
        TransitMode::Route => {
            hint(ui, help::tool_hint(Tool::Transit));
            let ids: Vec<nd_sim::RouteId> = sim.world.transit.routes.keys().collect();
            let mut remove: Option<nd_sim::RouteId> = None;
            let mut set_buses: Option<(nd_sim::RouteId, u8)> = None;
            for id in ids {
                let r = &sim.world.transit.routes[id];
                ui.horizontal(|ui| {
                    let sel = tools.route_sel == Some(id);
                    if ui.selectable_label(sel, format!("{} · {} stops · {} bus{} · {} riders today{}", r.name, r.stops.len(), r.buses, if r.buses == 1 { "" } else { "es" }, r.riders_today, if r.broken.is_empty() { "" } else { " · NO ROAD between some stops" })).clicked() {
                        tools.route_sel = if sel { None } else { Some(id) };
                    }
                    if ui.small_button("−").clicked() && r.buses > 0 {
                        set_buses = Some((id, r.buses - 1));
                    }
                    if ui.small_button("+").clicked() {
                        set_buses = Some((id, r.buses + 1));
                    }
                    if ui.small_button("delete").clicked() {
                        remove = Some(id);
                    }
                });
            }
            if let Some((id, n)) = set_buses {
                sim.world.set_route_buses(id, n);
            }
            if let Some(id) = remove {
                sim.world.remove_bus_route(id);
                if tools.route_sel == Some(id) {
                    tools.route_sel = None;
                }
            }
        }
        TransitMode::Depot => hint(ui, help::TRANSIT_DEPOT),
    }
}

fn rail(ui: &mut egui::Ui, sim: &mut Sim, tools: &mut ToolState) {
    ui.horizontal(|ui| {
        for (k, label) in [(RailFacility::Station, "Passenger station"), (RailFacility::FreightYard, "Freight yard")] {
            if ui.selectable_label(tools.rail_kind == k, label).clicked() {
                tools.rail_kind = k;
            }
        }
    });
    let r = &sim.world.rail.stats;
    let (w, d) = nd_sim::Facility::size(tools.rail_kind);
    hint(ui, &match tools.rail_kind {
        RailFacility::Station => format!("A piece with track along one side and a street along the other, {w:.0} by {d:.0} m. Click anywhere: set beside track or a street it fuses to them, otherwise it lays stubs for you to join. {:.0} to build, {:.0}/day. People ride between stations for trips over {:.0} m; newcomers arrive by train.", nd_sim::rail::STATION_COST, nd_sim::rail::STATION_UPKEEP_PER_DAY, nd_sim::rail::RAIL_TRIP_MIN),
        RailFacility::FreightYard => format!("A yard with sidings along one side and a street for lorries along the other, {w:.0} by {d:.0} m, anywhere you like. {:.0} to build, {:.0}/day. Once connected, imports come by rail at {:.0}% of the price and exports earn {:.0}% more.", nd_sim::rail::YARD_COST, nd_sim::rail::YARD_UPKEEP_PER_DAY, nd_sim::rail::RAIL_IMPORT_FACTOR * 100.0, (nd_sim::rail::RAIL_EXPORT_BONUS - 1.0) * 100.0),
    });
    hint(ui, help::tool_hint(Tool::Rail));
    hint(ui, &format!("{} station{} · {} yard{} · {} train{} out · next passenger train in {} min · {} waiting · {} riding · {} riders today · fares {:.0} · {:.0} units of freight in today, {:.0} waiting at yards · upkeep {:.0}/day", r.stations, if r.stations == 1 { "" } else { "s" }, r.yards, if r.yards == 1 { "" } else { "s" }, r.trains, if r.trains == 1 { "" } else { "s" }, r.next_passenger_in, r.waiting, r.riding, r.riders_today, r.fares_today, r.freight_in_today, r.freight_waiting, r.upkeep_today));
}

fn power(ui: &mut egui::Ui, tools: &mut ToolState) {
    ui.horizontal_wrapped(|ui| {
        for k in PowerPlantKind::ALL {
            if ui.selectable_label(tools.power_kind == k, format!("{} {:.0} MW", k.label(), k.capacity_mw())).clicked() {
                tools.power_kind = k;
            }
        }
    });
    let k = tools.power_kind;
    hint(ui, &format!("costs {:.0} to build, {:.0}/day to run, {} jobs{}", k.build_cost(), k.upkeep_per_day(), k.jobs(), help::power_note(k)));
    hint(ui, help::tool_hint(Tool::Power));
}

/// Colour for a figure that should worry the player.
pub fn warn_if(cond: bool, text: String) -> egui::RichText {
    let t = egui::RichText::new(text);
    if cond { t.color(WARN) } else { t }
}
