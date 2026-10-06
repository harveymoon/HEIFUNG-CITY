//! The toolbar's sub-pane: one block of parameters per tool, shown for the
//! active tool only. The buttons that pick the tool live in `toolbar`.

use bevy_egui::egui;
use nd_sim::roads::RoadKind;
use nd_sim::{BuildCategory, Gesture, LandUse, Placeable, RailFacility, ServiceKind, Tier};

use super::help;
use super::{hint, icons, section, zone_color32, WARN};
use crate::assets::GameAssets;
use crate::sim::Sim;
use crate::tools::{EffectivePlan, Tool, ToolState, TransitMode, ROAD_PRESETS};

pub fn sub_pane(ui: &mut egui::Ui, sim: &mut Sim, tools: &mut ToolState, plan: &mut EffectivePlan, assets: &GameAssets) {
    match tools.tool {
        Tool::Road => road(ui, tools),
        Tool::Zone => zone(ui, tools, assets),
        Tool::Edit => edit(ui, sim, tools, plan),
        Tool::Bulldoze => hint(ui, help::tool_hint(Tool::Bulldoze)),
        Tool::Inspect => hint(ui, help::tool_hint(Tool::Inspect)),
        Tool::Transit => transit(ui, sim, tools),
        Tool::Rail => rail(ui, sim, tools),
        Tool::Buildings => buildings(ui, sim, tools),
    }
}

fn road(ui: &mut egui::Ui, tools: &mut ToolState) {
    ui.horizontal_wrapped(|ui| {
        for (label, f, b, k) in ROAD_PRESETS {
            let on = tools.kind == k && (k != RoadKind::Street || (tools.lanes_fwd == f && tools.lanes_bwd == b));
            if icons::selectable(ui, on, label, |p, r, c| icons::road(p, r, k, f, b, c)).on_hover_text(preset_hint(label)).clicked() {
                tools.kind = k;
                if k == RoadKind::Street {
                    tools.lanes_fwd = f;
                    tools.lanes_bwd = b;
                }
            }
        }
    });
    // Lanes and curvature share a row so the bar stays shallow.
    ui.horizontal(|ui| {
        if tools.kind == RoadKind::Street {
            ui.add(egui::Slider::new(&mut tools.lanes_fwd, 0..=4).text("lanes fwd"));
            ui.add(egui::Slider::new(&mut tools.lanes_bwd, 0..=4).text("lanes back"));
            if tools.lanes_fwd + tools.lanes_bwd == 0 {
                tools.lanes_fwd = 1;
            }
            ui.separator();
        }
        if ui.selectable_label(!tools.curved, "Straight").clicked() {
            tools.curved = false;
        }
        if ui.selectable_label(tools.curved, "Curved [C]").clicked() {
            tools.curved = true;
        }
    });
    match tools.kind {
        RoadKind::Path => hint(ui, help::ROAD_PATH),
        RoadKind::Rail => hint(ui, help::ROAD_RAIL),
        RoadKind::Street => {}
    }
    hint(ui, help::tool_hint(Tool::Road));
}

fn zone(ui: &mut egui::Ui, tools: &mut ToolState, assets: &GameAssets) {
    ui.horizontal_wrapped(|ui| {
        for lu in LandUse::ALL {
            let colour = zone_color32(assets.palette.zone_color(lu));
            let text = egui::RichText::new(lu.label()).color(colour);
            if icons::selectable(ui, tools.land_use == lu, text, |p, r, _| icons::land_use(p, r, lu, colour)).clicked() {
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
                    if icons::selectable(ui, on, label, |p, r, c| icons::road(p, r, pk, pf, pb, c)).on_hover_text(preset_hint(label)).clicked() {
                        k = pk;
                        f = pf;
                        b = pb;
                        changed = true;
                    }
                }
            });
            ui.horizontal(|ui| {
                changed |= ui.add(egui::Slider::new(&mut f, 0..=4).text("lanes fwd")).changed();
                changed |= ui.add(egui::Slider::new(&mut b, 0..=4).text("lanes back")).changed();
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
            // The list scrolls past three lines so the bar keeps its height.
            egui::ScrollArea::vertical().max_height(66.0).show(ui, |ui| {
                for id in ids {
                    let r = &sim.world.transit.routes[id];
                    route_row(ui, tools, id, r, &mut set_buses, &mut remove);
                }
            });
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

/// One bus line: select it, add or remove a bus, delete it.
fn route_row(ui: &mut egui::Ui, tools: &mut ToolState, id: nd_sim::RouteId, r: &nd_sim::transit::BusRoute, set_buses: &mut Option<(nd_sim::RouteId, u8)>, remove: &mut Option<nd_sim::RouteId>) {
    ui.horizontal(|ui| {
        let sel = tools.route_sel == Some(id);
        if ui.selectable_label(sel, format!("{} · {} stops · {} bus{} · {} riders today{}", r.name, r.stops.len(), r.buses, if r.buses == 1 { "" } else { "es" }, r.riders_today, if r.broken.is_empty() { "" } else { " · NO ROAD between some stops" })).clicked() {
            tools.route_sel = if sel { None } else { Some(id) };
        }
        if ui.small_button("−").clicked() && r.buses > 0 {
            *set_buses = Some((id, r.buses - 1));
        }
        if ui.small_button("+").clicked() {
            *set_buses = Some((id, r.buses + 1));
        }
        if ui.small_button("delete").clicked() {
            *remove = Some(id);
        }
    });
}

fn rail(ui: &mut egui::Ui, sim: &mut Sim, tools: &mut ToolState) {
    ui.horizontal(|ui| {
        for (k, label) in [(RailFacility::Station, "Passenger station"), (RailFacility::FreightYard, "Freight yard")] {
            if ui.selectable_label(tools.dock.is_none() && tools.rail_kind == k, label).clicked() {
                tools.rail_kind = k;
                tools.dock = None;
            }
        }
    });
    tools.dock = None;
    let r = &sim.world.rail.stats;
    let (w, d) = nd_sim::Facility::size(tools.rail_kind);
    hint(ui, &match tools.rail_kind {
        RailFacility::Station => format!("A piece with track along one side and a street along the other, {w:.0} by {d:.0} m. Click anywhere: set beside track or a street it fuses to them, otherwise it lays stubs for you to join. {:.0} to build, {:.0}/day. People ride between stations for trips over {:.0} m; newcomers arrive by train.", nd_sim::rail::STATION_COST, nd_sim::rail::STATION_UPKEEP_PER_DAY, nd_sim::rail::RAIL_TRIP_MIN),
        RailFacility::FreightYard => format!("A yard with sidings along one side and a street for lorries along the other, {w:.0} by {d:.0} m, anywhere you like. {:.0} to build, {:.0}/day. Once connected, imports come by rail at {:.0}% of the price and exports earn {:.0}% more.", nd_sim::rail::YARD_COST, nd_sim::rail::YARD_UPKEEP_PER_DAY, nd_sim::rail::RAIL_IMPORT_FACTOR * 100.0, (nd_sim::rail::RAIL_EXPORT_BONUS - 1.0) * 100.0),
    });
    hint(ui, help::tool_hint(Tool::Rail));
    hint(ui, &format!("{} station{} · {} yard{} · {} train{} out · next passenger train in {} min · {} waiting · {} riding · {} riders today · fares {:.0} · {:.0} units of freight in today, {:.0} waiting at yards · upkeep {:.0}/day", r.stations, if r.stations == 1 { "" } else { "s" }, r.yards, if r.yards == 1 { "" } else { "s" }, r.trains, if r.trains == 1 { "" } else { "s" }, r.next_passenger_in, r.waiting, r.riding, r.riders_today, r.fares_today, r.freight_in_today, r.freight_waiting, r.upkeep_today));
}

/// The Buildings tool: a row of sub-categories, the items in the open one,
/// a tier picker for services and attractions, and what it costs and does.
fn buildings(ui: &mut egui::Ui, sim: &mut Sim, tools: &mut ToolState) {
    ui.horizontal(|ui| {
        for cat in BuildCategory::ALL {
            if ui.selectable_label(tools.build_cat == cat, cat.label()).clicked() && tools.build_cat != cat {
                tools.build_cat = cat;
                if let Some(first) = cat.items().first() {
                    tools.build = *first;
                }
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        for item in tools.build_cat.items() {
            let on = std::mem::discriminant(&tools.build) == std::mem::discriminant(&item) && tools.build.with_tier(Tier::Mid) == item.with_tier(Tier::Mid);
            let text = match item {
                Placeable::Plant(k) => format!("{} {:.0} MW", k.label(), k.capacity_mw()),
                other => other.menu_label().to_string(),
            };
            if ui.selectable_label(on, text).clicked() {
                tools.build = item;
            }
        }
        if tools.build.has_tiers() {
            ui.separator();
            for t in Tier::ALL {
                if ui.selectable_label(tools.build_tier == t, tools.build.with_tier(t).label()).clicked() {
                    tools.build_tier = t;
                }
            }
        }
    });
    let what = tools.build_item();
    let mut line = format!("{} · costs {:.0} to build, {:.0}/day to run, {} jobs", what.label(), what.cost(), what.upkeep(), what.jobs());
    if what.min_area() > 0.0 {
        line += &format!(", needs {:.0} m²", what.min_area());
    }
    match what {
        Placeable::Plant(k) => {
            line += help::power_note(k);
            line += &format!(" Goes {}.", k.site_hint());
        }
        Placeable::Service(k, t) => {
            if k.radius(t) > 0.0 {
                line += &format!(", covers {:.0} m by road", k.radius(t));
            }
            if k.units(t) > 0 {
                line += &format!(", {} {}{}", k.units(t), k.unit_kind().map_or("", |u| u.label()), if k.units(t) == 1 { "" } else { "s" });
            }
            let cap = k.capacity(t);
            if cap > 0 {
                line += &format!(", {cap} {}", match k { ServiceKind::Health => "beds", ServiceKind::Stadium => "seats", ServiceKind::Jail => "cells", _ => "places" });
            }
            line += ". ";
            line += help::service_note(k);
        }
        Placeable::BusDepot => line += &format!(". {}", help::TRANSIT_DEPOT),
        Placeable::Rail(_) => line += ". Set beside track or a street it fuses to them; otherwise it lays stubs to join (see the Rail tool).",
        Placeable::Dock(_) => {}
    }
    hint(ui, &line);
    if let Placeable::Dock(k) = what {
        dock_hint(ui, sim, k);
    }
    if tools.build_cat == BuildCategory::Services {
        let s = &sim.world.services.stats;
        hint(ui, &format!("Coverage of homes: police {:.0}% · fire {:.0}% · health {:.0}% · schools {:.0}% · {} incidents today · avg response {:.0} min · {}/{} units out · {} fires burning", s.coverage[0] * 100.0, s.coverage[1] * 100.0, s.coverage[2] * 100.0, s.coverage[3] * 100.0, s.incidents_today.iter().sum::<u32>(), s.avg_response(), s.units_busy, s.units, s.fires_burning));
    }
    hint(ui, help::tool_hint(Tool::Buildings));
}

/// What a dock is and how the docks are doing (moved here from the Rail tool).
fn dock_hint(ui: &mut egui::Ui, sim: &mut Sim, k: nd_sim::PortKind) {
        let (w, d) = k.size();
        let s = &sim.world.ports.stats;
        hint(ui, &match k {
            nd_sim::PortKind::ShippingPort => format!("A container port, {w:.0} by {d:.0} m, on a river at least {:.0} m wide or on the sea; it builds a pier out into the water and a street along its landward side. {:.0} to build, {:.0}/day. Imports come by ship at {:.0}% of the price, exports earn {:.0}% more.", k.min_width(), k.cost(), k.upkeep_per_day(), nd_sim::ports::SHIP_IMPORT_FACTOR * 100.0, (nd_sim::ports::SHIP_EXPORT_BONUS - 1.0) * 100.0),
            nd_sim::PortKind::FerryTerminal => format!("A ferry terminal, {w:.0} by {d:.0} m, on any water; ferries run to every other terminal on connected water, or out to elsewhere. {:.0} to build, {:.0}/day, fare {:.0}.", k.cost(), k.upkeep_per_day(), nd_sim::ports::FERRY_FARE),
        });
        hint(ui, &format!("{} port{} · {} terminal{} · {} ship{} out · {} loads landed today, {} waiting · {} ferry riders today, fares {:.0}", s.ports, if s.ports == 1 { "" } else { "s" }, s.terminals, if s.terminals == 1 { "" } else { "s" }, s.ships_out, if s.ships_out == 1 { "" } else { "s" }, s.freight_landed_today, s.waiting_cargo, s.ferry_riders_today, s.ferry_fares_today));
}

/// Tooltip for a road preset button.
fn preset_hint(label: &str) -> &'static str {
    match label {
        "Path" => "Footpath: walkers and bikes only",
        "Rail" => "Railway track: trains only, never fronts lots",
        "Lane" => "One lane each way",
        "Street" => "Two lanes each way",
        "Avenue" => "Three lanes each way",
        "One-way" => "Two lanes, one direction",
        "Ramp" => "One lane, one direction: slip roads onto a highway",
        "Highway" => "Four lanes one way: 25 m/s, no frontage, grade-separated",
        _ => "",
    }
}

/// Colour for a figure that should worry the player.
pub fn warn_if(cond: bool, text: String) -> egui::RichText {
    let t = egui::RichText::new(text);
    if cond { t.color(WARN) } else { t }
}
