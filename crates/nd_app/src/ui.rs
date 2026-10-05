//! egui side panel: clock, speed, project, tools, stats, inspector, save/load.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use nd_minds::Personality;
use nd_sim::{LandUse, Resource};

use crate::assets::GameAssets;
use crate::camera::CameraMode;
use crate::render::{Lighting, Overlays, RenderStats};
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use crate::sim::{Sim, SPEEDS};
use crate::tools::{Selection, Tool, ToolState, ROAD_PRESETS};
use nd_sim::roads::RoadKind;
use nd_sim::Gesture;

const SAVE_PATH: &str = "saves/city.ron";

#[derive(Resource, Default)]
pub struct UiFlags {
    pub pointer_over_ui: bool,
    pub message: String,
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiFlags>().add_systems(EguiPrimaryContextPass, side_panel);
    }
}

fn neon(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(egui::Color32::from_rgb(230, 225, 210)).strong());
}

fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(egui::RichText::new(text).small().strong().color(egui::Color32::from_rgb(235, 180, 90)));
}

fn zone_color32(c: [f32; 3]) -> egui::Color32 {
    egui::Color32::from_rgb((c[0].min(1.0) * 255.0) as u8, (c[1].min(1.0) * 255.0) as u8, (c[2].min(1.0) * 255.0) as u8)
}

fn side_panel(
    mut contexts: EguiContexts,
    mut sim: ResMut<Sim>,
    mut tools: ResMut<ToolState>,
    mut selection: ResMut<Selection>,
    mut flags: ResMut<UiFlags>,
    mut overlays: ResMut<Overlays>,
    mut lighting: ResMut<Lighting>,
    mut mode: ResMut<CameraMode>,
    assets: Res<GameAssets>,
    stats: Res<RenderStats>,
    diagnostics: Res<DiagnosticsStore>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    style(ctx);
    let mut root = egui::Ui::new(
        ctx.clone(),
        "viewport".into(),
        egui::UiBuilder::new().layer_id(egui::LayerId::background()).max_rect(ctx.viewport_rect()),
    );

    let panel = egui::Panel::left("neon_panel").default_size(340.0).show(&mut root, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(4.0);
            ui.heading(egui::RichText::new("DISTRICT 2040").color(egui::Color32::from_rgb(235, 180, 90)).size(22.0));
            ui.label(egui::RichText::new(format!("minds: {} · backend: {}", nd_sim::MindProvider::name(&sim.minds), sim.minds.backend_name())).small().weak());
            ui.separator();

            // Clock and speed.
            ui.horizontal(|ui| {
                neon(ui, &format!("Day {}  {}", sim.world.clock.day() + 1, sim.world.clock.format_hhmm()));
                ui.add_space(8.0);
                let label = if sim.paused { "▶ run" } else { "⏸ pause" };
                if ui.button(label).clicked() {
                    sim.paused = !sim.paused;
                }
            });
            ui.horizontal(|ui| {
                for (i, (label, _)) in SPEEDS.iter().enumerate() {
                    if ui.selectable_label(sim.speed_index == i, *label).clicked() {
                        sim.speed_index = i;
                    }
                }
            });
            ui.separator();

            // Camera.
            section(ui, "CAMERA");
            ui.horizontal(|ui| {
                let walking = *mode == CameraMode::Walk;
                if ui.selectable_label(!walking, "Orbit").clicked() {
                    *mode = CameraMode::Orbit;
                }
                if ui.selectable_label(walking, "Walk the street [Tab]").clicked() {
                    *mode = CameraMode::Walk;
                }
            });
            if *mode == CameraMode::Walk {
                ui.label(egui::RichText::new("Mouse to look, WASD to walk, Shift to run, Tab or Esc to return.").small().weak());
            } else {
                ui.label(egui::RichText::new("Right-drag rotate · middle-drag or shift-drag pan · wheel or [ ] zoom · WASD/QE · F6 busiest junction · F7/F8/F9 jump to a low/middle/high tier building · F11 to a power plant · F12 to a park · F5 to a station or yard").small().weak());
            }
            ui.separator();

            // Lighting.
            section(ui, "LIGHTING");
            ui.horizontal(|ui| {
                ui.checkbox(&mut lighting.follow_clock, "Follow the clock");
                if !lighting.follow_clock {
                    ui.add(egui::Slider::new(&mut lighting.hour, 0.0..=24.0).text("hour").fixed_decimals(1));
                }
            });
            if !lighting.follow_clock {
                ui.horizontal(|ui| {
                    for (label, h) in [("dawn", 6.5), ("noon", 12.5), ("dusk", 18.5), ("night", 23.0)] {
                        if ui.small_button(label).clicked() {
                            lighting.hour = h;
                        }
                    }
                });
            }
            ui.separator();

            // Project.
            section(ui, "PROJECT");
            if let Some(p) = &sim.world.project {
                let steps = p.steps.len();
                let redo = p.redo.len();
                ui.label(format!("{steps} pending step{}{}", if steps == 1 { "" } else { "s" }, if redo > 0 { format!(", {redo} redoable") } else { String::new() }));
                ui.horizontal(|ui| {
                    if ui.button("Implement  [Ctrl+Enter]").clicked() {
                        sim.world.implement();
                        tools.points.clear();
                        tools.status = "Project implemented".into();
                    }
                    if ui.button("Undo").clicked() {
                        sim.world.undo();
                    }
                    if ui.button("Redo").clicked() {
                        sim.world.redo();
                    }
                    if ui.button("Discard").clicked() {
                        sim.world.discard_project();
                        tools.points.clear();
                    }
                });
            } else {
                ui.label(egui::RichText::new("No pending project. Drawing or bulldozing starts one.").weak().small());
            }
            ui.separator();

            // Tools.
            section(ui, "TOOLS");
            ui.horizontal_wrapped(|ui| {
                for (tool, label) in [
                    (Tool::Inspect, "Inspect [1]"),
                    (Tool::Road, "Road [2]"),
                    (Tool::Zone, "Zone [3]"),
                    (Tool::Edit, "Edit [4]"),
                    (Tool::Bulldoze, "Bulldoze [5]"),
                    (Tool::Power, "Power [6]"),
                    (Tool::Transit, "Buses [7]"),
                    (Tool::Rail, "Rail [8]"),
                ] {
                    if ui.selectable_label(tools.tool == tool, label).clicked() {
                        tools.tool = tool;
                        tools.points.clear();
                        tools.drag = None;
                    }
                }
            });
            match tools.tool {
                Tool::Road => {
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
                    if tools.kind == RoadKind::Path {
                        ui.label(egui::RichText::new("Path: 3 m wide, for walkers and bikes only. Cars never use it; it joins streets at junctions and crosses them with a zebra.").small().weak());
                    } else if tools.kind == RoadKind::Rail {
                        ui.label(egui::RichText::new("Rail: single track. Ends snap to other track only, so branch off the trunk line that already crosses the map; crossing a street makes a level crossing. Stations and freight yards go on lots beside the track (Rail tool).").small().weak());
                    } else {
                        ui.add(egui::Slider::new(&mut tools.lanes_fwd, 0..=4).text("lanes forward"));
                        ui.add(egui::Slider::new(&mut tools.lanes_bwd, 0..=4).text("lanes backward"));
                        if tools.lanes_fwd + tools.lanes_bwd == 0 {
                            tools.lanes_fwd = 1;
                        }
                    }
                    ui.horizontal(|ui| {
                        if ui.selectable_label(!tools.curved, "Straight").clicked() {
                            tools.curved = false;
                        }
                        if ui.selectable_label(tools.curved, "Curved").clicked() {
                            tools.curved = true;
                        }
                    });
                    ui.label(egui::RichText::new("Click to place points; ends dropped near a road snap onto it and make a junction (white ring). Curved roads bend through every point. One-way presets make ramps: a Ramp joining a road is a yield merge with no signals. Enter or right-click adds the road, Backspace undoes a point.").small().weak());
                }
                Tool::Zone => {
                    ui.horizontal_wrapped(|ui| {
                        for lu in LandUse::ALL {
                            let text = egui::RichText::new(lu.label()).color(zone_color32(assets.palette.zone_color(lu)));
                            if ui.selectable_label(tools.land_use == lu, text).clicked() {
                                tools.land_use = lu;
                            }
                        }
                    });
                    ui.label(egui::RichText::new("Click polygon corners around a road, Enter or right-click to add to the project. Ctrl-click inside an existing zone to repaint it with the selected land use. Zones drawn over older ones take the ground.").small().weak());
                }
                Tool::Edit => {
                    ui.label(egui::RichText::new("Click a road to select it for upgrading. Drag a control point to move it. Shift-click a road or zone edge to insert a point. Delete removes the hovered point.").small().weak());
                    if let Some(id) = tools.selected_gesture {
                        match sim.world.gesture(id) {
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
                            Some(_) => {
                                ui.label(egui::RichText::new("Selected a zone; use the Zone tool with Ctrl-click to repaint it.").small().weak());
                            }
                            None => tools.selected_gesture = None,
                        }
                    }
                }
                Tool::Bulldoze => {
                    ui.label(egui::RichText::new("Click a road or zone to mark it for removal.").small().weak());
                }
                Tool::Inspect => {
                    ui.label(egui::RichText::new("Click a glowing citizen, a car, or a building.").small().weak());
                }
                Tool::Transit => {
                    use crate::tools::TransitMode;
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
                        ui.label(egui::RichText::new(format!("{} depot{} for {} buses · {} out · {} waiting · {} riding · {} riders today · fares {:.0} · upkeep {:.0}/day", t.depots, if t.depots == 1 { "" } else { "s" }, t.depot_capacity, t.buses, t.waiting, t.riding, t.riders_today, t.fares_today, t.upkeep_today)).small().weak());
                    }
                    match tools.transit_mode {
                        TransitMode::Route => {
                            ui.label(egui::RichText::new("Click streets in order to lay stops round a loop (the side you click is the side buses serve); Enter or right-click closes the line. Click a stop to select its line, drag it to move it, Delete removes it, Backspace drops the last stop.").small().weak());
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
                        TransitMode::Depot => {
                            ui.label(egui::RichText::new("Click a vacant lot by a street (not homes or parks). Buses park here overnight and leave at 05:30; each bus costs its upkeep per day and riders pay a fare.").small().weak());
                        }
                    }
                }
                Tool::Rail => {
                    ui.horizontal(|ui| {
                        for (k, label) in [(nd_sim::RailFacility::Station, "Passenger station"), (nd_sim::RailFacility::FreightYard, "Freight yard")] {
                            if ui.selectable_label(tools.rail_kind == k, label).clicked() {
                                tools.rail_kind = k;
                            }
                        }
                    });
                    let r = &sim.world.rail.stats;
                    ui.label(egui::RichText::new(match tools.rail_kind {
                        nd_sim::RailFacility::Station => format!("A piece with track along one side and a street along the other, {:.0} by {:.0} m. Click anywhere: set beside track or a street it fuses to them, otherwise it lays stubs for you to join. {:.0} to build, {:.0}/day. People ride between stations for trips over {:.0} m; newcomers arrive by train.", nd_sim::Facility::size(nd_sim::RailFacility::Station).0, nd_sim::Facility::size(nd_sim::RailFacility::Station).1, nd_sim::rail::STATION_COST, nd_sim::rail::STATION_UPKEEP_PER_DAY, nd_sim::rail::RAIL_TRIP_MIN),
                        nd_sim::RailFacility::FreightYard => format!("A yard with sidings along one side and a street for lorries along the other, {:.0} by {:.0} m, anywhere you like. {:.0} to build, {:.0}/day. Once connected, imports come by rail at {:.0}% of the price and exports earn {:.0}% more.", nd_sim::Facility::size(nd_sim::RailFacility::FreightYard).0, nd_sim::Facility::size(nd_sim::RailFacility::FreightYard).1, nd_sim::rail::YARD_COST, nd_sim::rail::YARD_UPKEEP_PER_DAY, nd_sim::rail::RAIL_IMPORT_FACTOR * 100.0, (nd_sim::rail::RAIL_EXPORT_BONUS - 1.0) * 100.0),
                    }).small().weak());
                    ui.label(egui::RichText::new("Nothing runs until both connectors are joined: track from its rail stub to the line (a trunk already crosses the map to the south) and a road from its street stub to the city. Unjoined stubs show red. Track is laid with the Road tool's Rail preset.").small().weak());
                    ui.label(egui::RichText::new(format!("{} station{} · {} yard{} · {} train{} out · next passenger train in {} min · {} waiting · {} riding · {} riders today · fares {:.0} · {:.0} units of freight in today, {:.0} waiting at yards · upkeep {:.0}/day", r.stations, if r.stations == 1 { "" } else { "s" }, r.yards, if r.yards == 1 { "" } else { "s" }, r.trains, if r.trains == 1 { "" } else { "s" }, r.next_passenger_in, r.waiting, r.riding, r.riders_today, r.fares_today, r.freight_in_today, r.freight_waiting, r.upkeep_today)).small().weak());
                }
                Tool::Power => {
                    ui.horizontal_wrapped(|ui| {
                        for k in nd_sim::PowerPlantKind::ALL {
                            if ui.selectable_label(tools.power_kind == k, format!("{} {:.0} MW", k.label(), k.capacity_mw())).clicked() {
                                tools.power_kind = k;
                            }
                        }
                    });
                    let k = tools.power_kind;
                    ui.label(egui::RichText::new(format!("costs {:.0} to build, {:.0}/day to run, {} jobs{}", k.build_cost(), k.upkeep_per_day(), k.jobs(), match k {
                        nd_sim::PowerPlantKind::Coal => ". Industrial land only. Cheap and steady; the haze drags land value down for hundreds of metres.",
                        nd_sim::PowerPlantKind::Solar => ". Any vacant lot. Clean, but only by day and dimmed by cloud.",
                        nd_sim::PowerPlantKind::Wind => ". Any vacant lot. Clean; output follows the wind.",
                        nd_sim::PowerPlantKind::Nuclear => ". Industrial land only. Enormous, steady, expensive, and the neighbours are nervous.",
                    })).small().weak());
                    ui.label(egui::RichText::new("Click a vacant lot to build. Without plants the regional grid sells the city 60 MW at a price; beyond that, the poorest districts go dark first.").small().weak());
                }
            }
            if !tools.status.is_empty() {
                ui.label(egui::RichText::new(&tools.status).small());
            }
            if ui.button("Seed demo district [G]").clicked() {
                sim.seed_demo_district();
                tools.status = "Demo district laid out around the centre".to_string();
            }
            ui.separator();

            // Stats.
            let e = &sim.world.economy;
            section(ui, "CITY");
            ui.label(format!(
                "population {}   buildings {}   lots {} ({} vacant, {} unusable)",
                sim.world.citizens.len(),
                sim.world.buildings.len(),
                sim.world.lots.len(),
                sim.world.lots.values().filter(|l| l.building.is_none() && l.usable()).count(),
                sim.world.lots.values().filter(|l| l.landlocked).count()
            ));
            ui.label(format!(
                "homes {}/{}   jobs {}/{}   waiting to move in {}",
                e.homes_total - e.homes_free,
                e.homes_total,
                e.jobs_total - e.jobs_free,
                e.jobs_total,
                e.waiting_immigrants
            ));
            {
                let e = &sim.world.economy;
                ui.label(format!(
                    "parking {}/{} spaces in {} lots   circling today {}   fees today {:.0}",
                    e.parking_used, e.parking_capacity, e.parking_lots, e.parking_street_today, e.parking_fees_today
                ));
                ui.label(format!("walking trips today {}   walk-in sales {:.0}   pedestrian nodes {}", e.walk_trips_today, e.walk_in_sales_today, sim.world.walk.nodes.len()));
            }
            ui.label(format!(
                "cars on the road {}   lanes {}   intersections {}",
                sim.world.traffic.cars.len(),
                sim.world.traffic.net.lanes.len(),
                sim.world.traffic.net.intersections.iter().filter(|i| i.signalized).count()
            ));
            ui.separator();
            section(ui, "POWER & TREASURY");
            {
                let e = &sim.world.economy;
                let p = &e.power;
                let col = if p.served < 0.98 { egui::Color32::from_rgb(255, 120, 90) } else { egui::Color32::from_rgb(200, 220, 200) };
                ui.label(egui::RichText::new(format!("supply {:.0} MW + import {:.0} MW   demand {:.0} MW   served {:.0}%", p.supply_mw, p.imported_mw, p.demand_mw, p.served * 100.0)).color(col));
                ui.label(format!("plants {}   buildings in the dark {}   brownout hours today {}   weather {} (cloud {:.0}%, wind {:.0}%)", p.plants, p.unpowered_buildings, p.brownout_hours_today, sim.world.weather.label(), sim.world.weather.cloud * 100.0, sim.world.weather.wind * 100.0));
                ui.label(format!("treasury {:.0}   taxes so far today {:.0}   plant upkeep {:.0}/day   imported power {:.0} today", e.treasury, e.tax_today, p.upkeep_today, p.import_cost_today));
            }
            ui.separator();
            section(ui, "LAND & CLASS");
            ui.checkbox(&mut overlays.land_value, "Land value overlay [V]");
            ui.checkbox(&mut overlays.paths, "Pedestrian network overlay [P]");
            ui.label(format!(
                "land value avg {:.2}   gini {:.2}   displaced {}   left town {}   priced out {}",
                e.avg_land_value, e.gini, e.displaced_total, e.emigrated_total, e.priced_out_total
            ));
            ui.label(format!(
                "housing  low {} · middle {} · high {}     rents {:.0} / {:.0} / {:.0}",
                e.tier_counts[0], e.tier_counts[1], e.tier_counts[2], e.avg_rent[0], e.avg_rent[1], e.avg_rent[2]
            ));
            ui.label(format!("people   low {} · middle {} · high {}", e.class_counts[0], e.class_counts[1], e.class_counts[2]));
            ui.separator();
            section(ui, "MARKET");
            let m = &e.market;
            ui.label(format!(
                "businesses {}   deals today {}   imports {:.0}   exports {:.0}   meals sold {}   hungry {}   closed {}",
                m.businesses, m.deals_today, m.imports_today, m.exports_today, m.meals_sold_today, m.hungry, m.closed_total
            ));
            ui.horizontal_wrapped(|ui| {
                for r in [Resource::Produce, Resource::Grain, Resource::Flour, Resource::Meat, Resource::Dairy, Resource::Groceries, Resource::BakedGoods, Resource::Parts, Resource::Electronics, Resource::Goods] {
                    let price = m.avg_price.get(&r).copied().unwrap_or(r.base_price());
                    let stock = m.stock.get(&r).copied().unwrap_or(0.0);
                    let hot = price > r.base_price() * 1.25;
                    let text = egui::RichText::new(format!("{} {:.1} ({:.0})", r.label(), price, stock)).small();
                    ui.label(if hot { text.color(egui::Color32::from_rgb(255, 120, 90)) } else { text });
                }
            });
            ui.label(egui::RichText::new("price (stock in town); red = scarce").small().weak());
            ui.label(egui::RichText::new("demand").small().weak());
            for lu in LandUse::ALL {
                let col = zone_color32(assets.palette.zone_color(lu));
                ui.add(egui::ProgressBar::new(e.demand_for(lu)).text(lu.label()).fill(col.gamma_multiply(0.7)).desired_height(12.0));
            }
            ui.separator();

            // Inspector.
            section(ui, "INSPECTOR");
            match *selection {
                Selection::None => {
                    ui.label(egui::RichText::new("Nothing selected.").weak());
                }
                Selection::Citizen(id) => {
                    let Some(c) = sim.world.citizens.get(id) else {
                        *selection = Selection::None;
                        return;
                    };
                    let p = Personality::from_seed(c.seed);
                    neon(ui, &c.name);
                    ui.label(egui::RichText::new(p.summary()).small());
                    let home = c.home.and_then(|h| sim.world.buildings.get(h)).map(|b| b.name.as_str()).unwrap_or("none");
                    let work = c.work.and_then(|h| sim.world.buildings.get(h)).map(|b| b.name.as_str()).unwrap_or("unemployed");
                    ui.label(format!("{}  ·  home: {home}  ·  work: {work}", c.activity.label()));
                    ui.label(format!("mood {:.0}%  ·  arrived day {}  ·  {}{}{}", c.mood * 100.0, c.arrived_tick / 1440 + 1, if c.owns_car { "has a car" } else { "no car" }, if c.owns_bike { ", has a bike" } else { "" }, if c.mode == nd_sim::citizens::TravelMode::Bike && c.activity.is_traveling() { "  ·  cycling" } else { "" }));
                    ui.label(format!("satiety {:.0}%  ·  meals at home {:.0}{}", c.satiety * 100.0, c.groceries, if c.hungry_days > 0 { format!("  ·  hungry {}d", c.hungry_days) } else { String::new() }));
                    let rent = c.home.and_then(|h| sim.world.buildings.get(h)).map(|b| b.rent).unwrap_or(0.0);
                    ui.label(format!("{} class  ·  income {:.0}/day  ·  rent {:.0}/day  ·  savings {:.0}{}", c.class().label(), c.income, rent, c.savings, if c.rent_stress_days > 0 { format!("  ·  rent stress {}d", c.rent_stress_days) } else { String::new() }));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(format!("“{}”", c.thought)).italics().color(egui::Color32::from_rgb(220, 190, 140)));
                    ui.add_space(4.0);
                    let mut spotlight = c.spotlight;
                    if ui.checkbox(&mut spotlight, "Spotlight: route this citizen through the LLM mind").changed() {
                        if let Some(c) = sim.world.citizens.get_mut(id) {
                            c.spotlight = spotlight;
                        }
                    }
                    ui.label(egui::RichText::new("memories").small().weak());
                    if let Some(c) = sim.world.citizens.get(id) {
                        for m in c.memories.iter().rev().take(8) {
                            ui.label(egui::RichText::new(format!("d{} {:02}:{:02}  {}", m.tick / 1440 + 1, (m.tick % 1440) / 60, m.tick % 60, m.text)).small());
                        }
                    }
                }
                Selection::Building(id) => {
                    let Some(b) = sim.world.buildings.get(id) else {
                        *selection = Selection::None;
                        return;
                    };
                    neon(ui, &b.name);
                    let style = assets.rules.pick_style(b.land_use, b.tier, &mut nd_sim::rng_for(b.seed, 11));
                    ui.label(format!("{}  ·  {} tier  ·  {}  ·  {} floors  ·  built day {}", b.land_use.label(), b.tier.label(), style.label(), b.floors, b.built_tick / 1440 + 1));
                    ui.label(format!("land value {:.2}  ·  rent {:.0}/day  ·  wage {:.0}/day  ·  condition {:.0}%{}", b.land_value, b.rent, b.wage(), b.condition * 100.0, if b.is_derelict() { "  ·  DERELICT" } else { "" }));
                    ui.label(format!("residents {}/{}  ·  workers {}/{}  ·  visitors {}", b.residents.len(), b.homes, b.workers.len(), b.jobs, b.visitors));
                    if let Some(lot) = sim.world.lots.get(b.lot) {
                        ui.label(format!("foot traffic today {}  ·  this hour {}{}", lot.footfall_today, lot.footfall_hour, if lot.car_access() { "" } else { "  ·  on a footpath: cars stop at the street, residents park nearby" }));
                        if let Some(park) = &lot.park {
                            let features: Vec<&str> = park.features.iter().map(|f| f.kind.label()).collect();
                            ui.label(format!("park: {} benches, {} paths, {} entrances{}; part of a {}-lot park of {:.0} m²", park.benches.len(), park.paths.len(), park.entrances.len(), if features.is_empty() { String::new() } else { format!(", {}", features.join(", ")) }, park.cluster_lots, park.cluster_area));
                            let sitting = sim.world.citizens.values().filter(|c| matches!(c.activity, nd_sim::Activity::Sitting { park: p, .. } if Some(p) == lot.building)).count();
                            if sitting > 0 {
                                ui.label(format!("{sitting} sitting on the benches"));
                            }
                        }
                    }
                    if let Some(link) = &b.rail {
                        match link.kind {
                            nd_sim::RailFacility::Station => ui.label(format!("railway station: platform {:.0} m, {} trains today, {} riders today across the line{}", link.platform[0].distance(link.platform[1]), sim.world.rail.stats.passenger_trains_today, sim.world.rail.stats.riders_today, if link.works() { "" } else if !link.rail_connected { " · NOT CONNECTED TO THE RAILWAY" } else { " · NOT CONNECTED TO THE ROADS" })),
                            nd_sim::RailFacility::FreightYard => ui.label(format!("freight yard: {} orders ({:.0} units) waiting for the next train, {:.0} units of exports on the apron{}", b.rail_inbound.len(), b.rail_inbound.iter().map(|c| c.amount).sum::<f32>(), b.rail_outbound, if link.works() { "" } else if !link.rail_connected { " · NOT CONNECTED TO THE RAILWAY" } else { " · NOT CONNECTED TO THE ROADS" })),
                        };
                    }
                    if b.depot {
                        let out = sim.world.traffic.cars.values().filter(|c| c.bus.as_ref().is_some_and(|x| x.depot == id)).count();
                        ui.label(format!("bus depot: room for {} buses, {} out on the lines, {:.0}/day", nd_sim::transit::DEPOT_CAPACITY, out, nd_sim::transit::DEPOT_UPKEEP_PER_DAY));
                    }
                    if let Some(kind) = b.plant {
                        ui.label(format!("{}: {:.0} MW nameplate, {:.0}% available now, {:.0}/day upkeep", kind.label(), kind.capacity_mw(), kind.availability(&sim.world.weather, sim.world.clock.day_fraction()) * 100.0, kind.upkeep_per_day()));
                    } else if b.powered < 0.98 {
                        ui.label(egui::RichText::new(format!("UNDERPOWERED: {:.0}% of {:.1} MW, {} dark hours today", b.powered * 100.0, nd_sim::power::demand_mw(b), b.unpowered_hours_today)).color(egui::Color32::from_rgb(255, 120, 90)));
                    } else {
                        ui.label(format!("power {:.1} MW, fully served", nd_sim::power::demand_mw(b)));
                    }
                    if b.parking_capacity > 0 {
                        ui.label(format!("parking {}/{} spaces{}", b.parking_used, b.parking_capacity, if b.is_parking() { format!("  ·  {:.1} per visit", b.parking_price) } else { "  ·  for residents and workers".to_string() }));
                    }
                    if let Some(bz) = &b.business {
                        neon(ui, &format!("{}{}", bz.kind.label(), if bz.closed { "  (CLOSED)" } else { "" }));
                        ui.label(format!("cash {:.0}  ·  today +{:.0} / -{:.0}  ·  deals {}  ·  staffing {:.0}%", bz.money, bz.revenue_today, bz.spend_today, bz.deals, b.staffing() * 100.0));
                        let recipe = bz.kind.recipe();
                        for (r, _) in recipe.inputs {
                            ui.label(egui::RichText::new(format!("needs {}: {:.0} in stock (target {:.0})", r.label(), bz.stock_of(*r), bz.target_input(*r, b.jobs))).small());
                        }
                        for (r, _) in recipe.outputs {
                            ui.label(egui::RichText::new(format!("sells {}: {:.0} in stock at {:.1}", r.label(), bz.stock_of(*r), bz.sell_price(*r, b.jobs))).small());
                        }
                    }
                    ui.label(egui::RichText::new("people here").small().weak());
                    let names: Vec<String> = b
                        .residents
                        .iter()
                        .chain(b.workers.iter())
                        .filter_map(|&cid| sim.world.citizens.get(cid).map(|c| c.name.clone()))
                        .take(10)
                        .collect();
                    for n in names {
                        ui.label(egui::RichText::new(n).small());
                    }
                }
            }
            ui.separator();

            // Save / load / assets.
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    flags.message = match sim.save(SAVE_PATH) {
                        Ok(()) => format!("saved {SAVE_PATH}"),
                        Err(e) => format!("save failed: {e}"),
                    };
                }
                if ui.button("Load").clicked() {
                    flags.message = match sim.load(SAVE_PATH) {
                        Ok(()) => {
                            *selection = Selection::None;
                            format!("loaded {SAVE_PATH}")
                        }
                        Err(e) => format!("load failed: {e}"),
                    };
                }
            });
            if !flags.message.is_empty() {
                ui.label(egui::RichText::new(&flags.message).small());
            }
            if let Some(err) = &assets.last_error {
                ui.label(egui::RichText::new(err).small().color(egui::Color32::from_rgb(255, 60, 90)));
            }
            ui.label(egui::RichText::new("assets/palette.ron and assets/rules/buildings.ron hot-reload while running.").small().weak());
            let fps = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()).unwrap_or(0.0);
            ui.label(egui::RichText::new(format!("{fps:.0} fps  ·  {} ticks/frame  ·  {} building meshes  ·  {}k triangles", sim.ticks_last_frame, stats.building_entities, (stats.street_triangles + stats.building_triangles) / 1000)).small().weak());
        });
    });

    // The panel lives on the background layer, which egui's own hit test
    // ignores, so check its rectangle directly as well.
    let over_panel = ctx.pointer_latest_pos().map_or(false, |p| panel.response.rect.contains(p));
    flags.pointer_over_ui = over_panel || ctx.is_pointer_over_egui() || ctx.egui_wants_pointer_input();
    Ok(())
}

fn style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgba_unmultiplied(8, 10, 20, 235);
    visuals.window_fill = egui::Color32::from_rgba_unmultiplied(8, 10, 20, 235);
    visuals.selection.bg_fill = egui::Color32::from_rgb(120, 85, 30);
    visuals.hyperlink_color = egui::Color32::from_rgb(235, 180, 90);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(150, 105, 40);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(70, 55, 35);
    ctx.set_visuals(visuals);
}
