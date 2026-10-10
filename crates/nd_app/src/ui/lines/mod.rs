//! [lines] The Lines window: every bus line and every metro line in one
//! list. Picking one opens it under its row: its controls and figures and
//! the horizontal stop diagram (`diagram`): stops as nodes along a track,
//! the vehicles on it, and a box per vehicle below (load, next stop, how
//! late, what it is doing) with an arrow to where it is.
//!
//! A bus line has "Edit line": the Buses tool goes into edit mode for it
//! (`tools::line_edit`): clicks on the map add stops where they belong
//! along the loop (or at a "+" slot picked on the diagram), drag and remove
//! stops, and share another line's stop. "Finish editing line" ends it.
//! While editing, the line's problems (`problems`) are spelled out.
//!
//! Stop figures are the sim's own (`nd_sim::transit::StopStats`), so the
//! remote API's `bus {action: "line"}` reads the same.

pub mod diagram;
pub mod problems;

use bevy::prelude::Vec2;
use bevy_egui::egui::{self, Color32};
use nd_sim::{MetroLineId, RouteId};

use super::{hint, section, UiCtx, GOLD, IVORY, WARN};
use crate::ui::theme::{AMBER, GOOD};
use diagram::{Clicked, Leg, Node, Track, Vehicle};

/// [lines] The inspector asks the window to open a line (a depot's lines,
/// F1): the route is left here in egui's memory and picked up next pass.
pub fn request_pick(ctx: &egui::Context, route: RouteId) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("lines_pick_request"), route));
    super::Info::Lines.set_open(ctx, true);
}

/// The line colours as the map draws them (the gizmo palette in
/// `overlay::routes`, brought into display range), by `BusRoute::color`.
pub fn line_color(i: u8) -> Color32 {
    const C: [[u8; 3]; 8] = [[60, 170, 255], [255, 90, 60], [80, 230, 90], [240, 200, 50], [220, 80, 240], [60, 130, 255], [250, 140, 80], [200, 200, 200]];
    let c = C[i as usize % C.len()];
    super::theme::rgb_u8(c)
}

/// A metro line's colour as the Metro layer paints it.
pub fn metro_color(i: u8) -> Color32 {
    let c = nd_procgen::metro::line_colour(i);
    Color32::from(egui::Rgba::from_rgb((c[0] * 1.6).min(1.0), (c[1] * 1.6).min(1.0), (c[2] * 1.6).min(1.0)))
}

/// What the window asks of the world after drawing (it reads the world
/// while drawing, so edits wait until the end of the pass).
enum Act {
    Pick(Option<RouteId>),
    PickMetro(Option<MetroLineId>),
    SelectStop(RouteId, usize, Vec2),
    DeleteStop(RouteId, usize),
    Rename(RouteId, String),
    Recolour(RouteId, u8),
    Buses(RouteId, u8),
    DeleteLine(RouteId),
    SelectBus(nd_sim::CarId),
    Edit(RouteId),
    Finish,
    NewLine,
    Slot(usize),
    Headway(MetroLineId, u32),
    DeleteMetro(MetroLineId),
    SelectStation(nd_sim::BuildingId, Vec2),
    SelectTrain(nd_sim::MetroTrainId),
}

/// Stop names, worked out once per edit of the lines: the nearest building
/// to each stop, so the diagram reads "by Kowloon Noodle Bar", not "stop 4".
#[derive(Clone, Default)]
struct StopNames {
    /// [fix/review-app] `Sim::generation`: `transit.version` is not saved
    /// (0 after every load), so a loaded city with as many lines could show
    /// the last city's stop names.
    generation: u64,
    version: u64,
    routes: usize,
    names: std::collections::HashMap<(RouteId, usize), String>,
}

fn stop_names(ctx: &egui::Context, w: &nd_sim::World, generation: u64) -> StopNames {
    let id = egui::Id::new("lines_stop_names");
    let cached: Option<StopNames> = ctx.data(|d| d.get_temp(id));
    if let Some(c) = cached.filter(|c| c.generation == generation && c.version == w.transit.version && c.routes == w.transit.routes.len()) {
        return c;
    }
    let mut names = std::collections::HashMap::new();
    for (rid, r) in &w.transit.routes {
        for (i, s) in r.stops.iter().enumerate() {
            let near = w.buildings.values().filter_map(|b| w.lots.get(b.lot).map(|l| (b, l.center.distance(s.pos)))).filter(|(_, d)| *d < 90.0).min_by(|a, b| a.1.total_cmp(&b.1));
            names.insert((rid, i), near.map_or_else(|| format!("stop {}", i + 1), |(b, _)| format!("by {}", b.name)));
        }
    }
    let fresh = StopNames { generation, version: w.transit.version, routes: w.transit.routes.len(), names };
    ctx.data_mut(|d| d.insert_temp(id, fresh.clone()));
    fresh
}

pub fn window(ui: &mut egui::Ui, p: &mut UiCtx) {
    let mut act: Option<Act> = None;
    if let Some(r) = ui.ctx().data_mut(|d| d.remove_temp::<RouteId>(egui::Id::new("lines_pick_request"))) {
        act = Some(Act::Pick(Some(r)));
    }
    {
        let w = &p.sim.world;
        let editing = p.tools.line_edit.filter(|e| w.transit.routes.contains_key(e.route));
        // The bar: a new line, or the line being edited and the way out.
        ui.horizontal(|ui| {
            match editing {
                Some(e) => {
                    let name = &w.transit.routes[e.route].name;
                    ui.label(egui::RichText::new(format!("EDITING {name}")).color(GOLD).strong());
                    if ui.button(egui::RichText::new("Finish editing line").color(GOLD).strong()).on_hover_text("Done: back to the tool you had (Enter or Esc on the map too)").clicked() {
                        act = Some(Act::Finish);
                    }
                }
                None => {
                    if ui.button("New bus line").on_hover_text("Start a line and edit it: click streets on the map for its stops").clicked() {
                        act = Some(Act::NewLine);
                    }
                }
            }
        });
        if w.transit.routes.is_empty() && w.subway.lines.is_empty() {
            hint(ui, "No lines yet. New bus line starts one (or the Buses tool: click a street); metro lines join metro stations (Build, Transit, Metro).");
        }
        let problems = problems::cached(ui.ctx(), w, p.sim.generation);
        let sel = p.tools.route_sel.filter(|r| w.transit.routes.contains_key(*r));
        if !w.transit.routes.is_empty() {
            section(ui, "BUS LINES");
            header_row(ui, ["line", "stops", "buses", "today", "yday"]);
        }
        for (id, r) in &w.transit.routes {
            let probs = problems.get(&id).map_or(&[][..], |v| v.as_slice());
            let mark = if probs.iter().any(|p| p.blocks) { Some(WARN) } else if probs.iter().any(|p| !p.text.starts_with("night")) { Some(AMBER) } else { None };
            let out = w.traffic.cars.values().filter(|c| c.bus.as_ref().is_some_and(|b| b.route == id)).count();
            let open = sel == Some(id);
            let dimmed = editing.is_some_and(|e| e.route != id);
            let row = line_row(ui, line_color(r.color), &r.name, open, dimmed, mark, [format!("{}", r.stops.len()), format!("{out}/{}", r.buses), format!("{}", r.riders_today), format!("{}", r.boardings_yesterday())]);
            if row.on_hover_text(if open { "Close this line" } else { "Open this line: its stops, buses and times" }).clicked() {
                act = Some(Act::Pick(if open { None } else { Some(id) }));
            }
            if open {
                egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 2, top: 4, bottom: 8 }).show(ui, |ui| bus_line(ui, p, id, probs, &mut act));
            }
        }
        if !w.subway.lines.is_empty() {
            section(ui, "METRO LINES");
            header_row(ui, ["line", "stations", "trains", "today", "yday"]);
        }
        let msel = p.tools.metro_sel.filter(|l| w.subway.lines.contains_key(*l));
        for (id, l) in &w.subway.lines {
            let trains = w.subway.trains.values().filter(|t| t.line == id).count();
            let open = msel == Some(id);
            let row = line_row(ui, metro_color(l.color), &l.name, open, editing.is_some(), None, [format!("{}", l.stations.len()), format!("{trains}"), format!("{}", l.riders_today), format!("{}", l.riders_yesterday)]);
            if row.on_hover_text(if open { "Close this line" } else { "Open this line: its stations and trains" }).clicked() {
                act = Some(Act::PickMetro(if open { None } else { Some(id) }));
            }
            if open {
                egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 2, top: 4, bottom: 8 }).show(ui, |ui| metro_line(ui, p, id, &mut act));
            }
        }
    }
    apply(p, act);
}

/// The column heads over the list.
fn header_row(ui: &mut egui::Ui, heads: [&str; 5]) {
    ui.horizontal(|ui| {
        ui.add_space(18.0);
        ui.add_sized([150.0, 14.0], egui::Label::new(egui::RichText::new(heads[0]).small().weak()));
        for h in &heads[1..] {
            ui.add_sized([52.0, 14.0], egui::Label::new(egui::RichText::new(*h).small().weak()));
        }
    });
}

/// One line's row: its colour, name (with a mark when something is wrong)
/// and four figures. The whole row clicks.
fn line_row(ui: &mut egui::Ui, col: Color32, name: &str, open: bool, dimmed: bool, mark: Option<Color32>, figures: [String; 4]) -> egui::Response {
    let resp = ui.horizontal(|ui| {
        let alpha = if dimmed { 0.4 } else { 1.0 };
        let (dot, _) = ui.allocate_exact_size(egui::vec2(12.0, 14.0), egui::Sense::hover());
        ui.painter().circle_filled(dot.center(), 5.0, col.gamma_multiply(alpha));
        let text = egui::RichText::new(name).color(if open { GOLD } else { IVORY.gamma_multiply(alpha) });
        let r = ui.allocate_ui_with_layout(egui::vec2(150.0, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_width(150.0);
            ui.selectable_label(open, text)
        }).inner;
        if let Some(m) = mark {
            ui.painter().circle_filled(egui::pos2(r.rect.right() + 7.0, r.rect.center().y), 4.0, m);
        }
        for f in figures {
            ui.add_sized([52.0, 14.0], egui::Label::new(egui::RichText::new(f).small().color(IVORY.gamma_multiply(alpha))));
        }
        r
    });
    resp.inner.union(resp.response.interact(egui::Sense::click()))
}

/// An open bus line: name and colour, buses, Edit line, figures, problems,
/// the diagram, and the picked stop.
fn bus_line(ui: &mut egui::Ui, p: &UiCtx, rid: RouteId, probs: &[problems::Problem], act: &mut Option<Act>) {
    let w = &p.sim.world;
    let r = &w.transit.routes[rid];
    let tsp = w.config.traffic_seconds_per_tick;
    let n = r.stops.len();
    let col = line_color(r.color);
    let edit = p.tools.line_edit.filter(|e| e.route == rid);
    // Name and colour.
    let name_id = egui::Id::new(("line_name_edit", rid));
    let mut name: String = ui.ctx().data(|d| d.get_temp(name_id)).unwrap_or_else(|| r.name.clone());
    ui.horizontal(|ui| {
        let field = ui.add(egui::TextEdit::singleline(&mut name).desired_width(130.0).font(egui::TextStyle::Heading).text_color(col));
        if field.lost_focus() && name.trim() != r.name {
            *act = Some(Act::Rename(rid, name.clone()));
        }
        if field.has_focus() {
            ui.ctx().data_mut(|d| d.insert_temp(name_id, name.clone()));
        } else {
            ui.ctx().data_mut(|d| d.remove::<String>(name_id));
        }
        for c in 0..nd_sim::vehicle::PAINTS {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::click());
            ui.painter().circle_filled(rect.center(), if c == r.color { 6.5 } else { 5.0 }, line_color(c));
            if c == r.color {
                ui.painter().circle_stroke(rect.center(), 6.5, egui::Stroke::new(1.5, IVORY));
            }
            if resp.on_hover_text("Recolour the line and its buses").clicked() && c != r.color {
                *act = Some(Act::Recolour(rid, c));
            }
        }
    });
    // Edit, buses, delete.
    let buses_out: Vec<(nd_sim::CarId, &nd_sim::Car)> = w.traffic.cars.iter().filter(|(_, c)| c.bus.as_ref().is_some_and(|b| b.route == rid)).collect();
    ui.horizontal(|ui| {
        if edit.is_some() {
            if ui.button(egui::RichText::new("Finish editing line").color(GOLD).strong()).clicked() {
                *act = Some(Act::Finish);
            }
        } else if ui.button("Edit line").on_hover_text("Add, insert, drag and remove stops on the map").clicked() {
            *act = Some(Act::Edit(rid));
        }
        ui.add_space(6.0);
        ui.label(egui::RichText::new(format!("buses {} out of {}", buses_out.len(), r.buses)).small());
        if ui.small_button("−").on_hover_text("One bus fewer").clicked() && r.buses > 0 {
            *act = Some(Act::Buses(rid, r.buses - 1));
        }
        if ui.small_button("+").on_hover_text("One bus more").clicked() {
            *act = Some(Act::Buses(rid, r.buses.saturating_add(1)));
        }
        ui.add_space(8.0);
        // [fix/review-app] Keyed by the city too: a loaded city reuses ids.
        let armed_id = egui::Id::new(("line_delete_armed", p.sim.generation, rid));
        let armed = ui.ctx().data(|d| d.get_temp::<bool>(armed_id)).unwrap_or(false);
        let label = if armed { egui::RichText::new("Really delete?").color(WARN).strong() } else { egui::RichText::new("Delete").color(WARN) };
        if ui.small_button(label).on_hover_text("Removes the line; its buses go home and its riders walk").clicked() {
            if armed {
                *act = Some(Act::DeleteLine(rid));
            }
            ui.ctx().data_mut(|d| d.insert_temp(armed_id, !armed));
        }
    });
    // Figures.
    let length: f32 = (0..n).map(|i| r.leg_length(i)).sum();
    let lap: f32 = (0..n).map(|i| r.leg_minutes(i, tsp).0).sum();
    let waiting = w.citizens.values().filter(|c| matches!(c.activity, nd_sim::Activity::Waiting { .. }) && matches!(c.transit.map(|t| t.leg), Some(nd_sim::transit::TransitLeg::Bus { route, .. }) if route == rid)).count();
    hint(ui, &format!("{} riders today · {} yesterday · {waiting} waiting now · loop {:.2} km, about {:.0} min round", r.riders_today, r.boardings_yesterday(), length / 1000.0, lap));
    // Problems: all of them while editing; otherwise the ones that stop it.
    let shown: Vec<&problems::Problem> = probs.iter().filter(|p| edit.is_some() || p.blocks).collect();
    if !shown.is_empty() {
        egui::Frame::new().fill(Color32::from_rgba_unmultiplied(60, 20, 20, 120)).corner_radius(4.0).inner_margin(6.0).show(ui, |ui| {
            for pr in shown {
                ui.label(egui::RichText::new(format!("{} {}", if pr.blocks { "✖" } else { "!" }, pr.text)).small().color(if pr.blocks { WARN } else { AMBER }));
            }
        });
    } else if edit.is_some() {
        ui.label(egui::RichText::new("✔ runs as drawn").small().color(GOOD));
    }
    if edit.is_some() {
        hint(ui, "On the map: click a street to add a stop (it goes in where it belongs along the loop); a + below picks the gap instead. Drag a stop to move it, right-click or Delete removes it, click another line's stop to share it.");
    }
    if n == 0 {
        return;
    }
    let names = stop_names(ui.ctx(), w, p.sim.generation);
    let nodes: Vec<Node> = (0..n)
        .map(|i| {
            let s = &r.stops[i].stats;
            Node {
                name: names.names.get(&(rid, i)).cloned().unwrap_or_default(),
                on: s.on_today,
                off: s.off_today,
                tip: format!("Stop {}: {} on, {} off today · {} on, {} off yesterday. Click to mark it on the map.", i + 1, s.on_today, s.off_today, s.on_yesterday, s.off_yesterday),
                picked: p.tools.stop_sel == Some((rid, i)),
            }
        })
        .collect();
    let legs: Vec<Leg> = if n < 2 {
        Vec::new()
    } else {
        (0..n)
            .map(|i| {
                let broken = r.broken.contains(&i) || probs.iter().any(|p| p.blocks && p.leg == Some(i));
                let (mins, timed) = r.leg_minutes(i, tsp);
                Leg { text: if broken { "NO ROAD".into() } else { format!("{}{:.0} min", if timed { "" } else { "~" }, mins.max(1.0)) }, broken }
            })
            .collect()
    };
    // The buses, in order along the loop.
    let tick = w.clock.tick;
    let mut vehicles: Vec<(f32, nd_sim::CarId, Vehicle)> = Vec::new();
    for (cid, c) in &buses_out {
        let b = c.bus.as_ref().unwrap();
        let dep = w.transit.departures.get(cid).copied();
        let at = if b.next_stop == usize::MAX || dep.is_none() { None } else { r.bus_progress(b.next_stop, c.pos) };
        let state = if b.next_stop == usize::MAX {
            ("going home", Color32::from_gray(150))
        } else if dep.is_none() {
            ("from depot", Color32::from_gray(170))
        } else if c.hold > 0.0 {
            ("at a stop", GOOD)
        } else if c.v < 0.5 {
            ("standing", AMBER)
        } else if b.to_depot {
            ("last run", AMBER)
        } else {
            ("running", IVORY)
        };
        let mut rows = Vec::new();
        if b.next_stop < n {
            rows.push((format!("next {} {}", b.next_stop + 1, names.names.get(&(rid, b.next_stop)).map_or("", |s| s.as_str())), IVORY));
        } else {
            rows.push((w.buildings.get(b.depot).map_or("to the depot".to_string(), |d| format!("to {}", d.name)), Color32::from_gray(150)));
        }
        rows.push(match dep {
            Some((prev, t0)) if prev < n && b.next_stop < n => {
                let (expect, timed) = r.leg_minutes(prev, tsp);
                let late = tick.saturating_sub(t0) as f32 - expect;
                if !timed {
                    ("leg not timed yet".to_string(), Color32::from_gray(140))
                } else if late > 1.5 {
                    (format!("late {late:.0} min"), WARN)
                } else {
                    ("on time".to_string(), GOOD)
                }
            }
            _ => ("—".to_string(), Color32::from_gray(140)),
        });
        let v = Vehicle { at, title: String::new(), state: (state.0.into(), state.1), load: (b.passengers.len(), nd_sim::transit::BUS_CAPACITY), rows, picked: *p.selection == crate::tools::Selection::Car(*cid) };
        vehicles.push((at.unwrap_or(f32::MAX), *cid, v));
    }
    vehicles.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let ids: Vec<nd_sim::CarId> = vehicles.iter().map(|v| v.1).collect();
    let vehicles: Vec<Vehicle> = vehicles.into_iter().enumerate().map(|(k, (_, _, mut v))| {
        v.title = format!("Bus {}", k + 1);
        v
    }).collect();
    let track = Track { id: egui::Id::new(("line_track", p.sim.generation, rid)), color: col, nodes, legs, looped: n >= 2, vehicles, editing: edit.is_some(), slot: edit.and_then(|e| e.slot) };
    match diagram::show(ui, &track) {
        Some(Clicked::Stop(i)) => *act = Some(Act::SelectStop(rid, i, Vec2::new(r.stops[i].pos.x, r.stops[i].pos.y))),
        Some(Clicked::Vehicle(k)) => *act = Some(Act::SelectBus(ids[k])),
        Some(Clicked::Slot(i)) => *act = Some(Act::Slot(i)),
        None => {}
    }
    if buses_out.is_empty() {
        hint(ui, "No bus out on the line now.");
    }
    // The picked stop.
    if let Some((_, i)) = p.tools.stop_sel.filter(|(sr, i)| *sr == rid && *i < n) {
        let s = &r.stops[i];
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(format!("Stop {} {}", i + 1, names.names.get(&(rid, i)).map_or("", |s| s.as_str()))).color(GOLD));
            ui.label(egui::RichText::new(format!("+{} −{} today", s.stats.on_today, s.stats.off_today)).small());
            if ui.small_button("show").on_hover_text("Fly to this stop").clicked() {
                *act = Some(Act::SelectStop(rid, i, Vec2::new(s.pos.x, s.pos.y)));
            }
            if ui.small_button(egui::RichText::new("remove").color(WARN)).on_hover_text("Remove this stop; riders bound for it decide again").clicked() {
                *act = Some(Act::DeleteStop(rid, i));
            }
        });
    }
}

/// Where a metro train is along its line, in stations (as `Vehicle::at`):
/// `s` is how far its run has come, `stops` where each station is along
/// the tunnel from the first, `total` the tunnel's length. A run back
/// (`forward` false) covers the tunnel from its far end.
pub fn metro_units(stops: &[f32], total: f32, looped: bool, forward: bool, s: f32) -> f32 {
    let n = stops.len();
    if n < 2 {
        return 0.0;
    }
    let along = if forward { s } else { total - s }.clamp(0.0, total);
    let mut ends: Vec<f32> = stops.to_vec();
    if looped {
        ends.push(total);
    }
    for i in 0..ends.len() - 1 {
        if along <= ends[i + 1] || i + 2 == ends.len() {
            let span = (ends[i + 1] - ends[i]).max(1e-3);
            return i as f32 + ((along - ends[i]) / span).clamp(0.0, 1.0);
        }
    }
    0.0
}

/// An open metro line: headway, figures and the diagram with its trains.
fn metro_line(ui: &mut egui::Ui, p: &UiCtx, id: MetroLineId, act: &mut Option<Act>) {
    let w = &p.sim.world;
    let l = &w.subway.lines[id];
    let col = metro_color(l.color);
    let secs = w.config.traffic_seconds_per_tick;
    let n = l.stations.len();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(&l.name).font(crate::ui::theme::heading()).color(col));
        ui.label(egui::RichText::new(format!("every {} min", l.headway)).small());
        if ui.small_button("−").on_hover_text("Trains more often").clicked() && l.headway > 2 {
            *act = Some(Act::Headway(id, l.headway - 2));
        }
        if ui.small_button("+").on_hover_text("Trains less often").clicked() {
            *act = Some(Act::Headway(id, l.headway + 2));
        }
        ui.add_space(8.0);
        let armed_id = egui::Id::new(("metro_delete_armed", p.sim.generation, id));
        let armed = ui.ctx().data(|d| d.get_temp::<bool>(armed_id)).unwrap_or(false);
        let label = if armed { egui::RichText::new("Really delete?").color(WARN).strong() } else { egui::RichText::new("Delete").color(WARN) };
        if ui.small_button(label).on_hover_text("Removes the line (the stations stay)").clicked() {
            if armed {
                *act = Some(Act::DeleteMetro(id));
            }
            ui.ctx().data_mut(|d| d.insert_temp(armed_id, !armed));
        }
    });
    hint(ui, &format!("{} riders today · {} yesterday · {:.1} km of tunnel{} · {} runs today. Stops are changed by laying the line again (Build, Transit, Metro lines).", l.riders_today, l.riders_yesterday, l.length() / 1000.0, if l.looped { " in a loop" } else { "" }, l.runs_today));
    if n == 0 {
        return;
    }
    let name = |b: nd_sim::BuildingId| w.buildings.get(b).map_or_else(|| "a station".to_string(), |b| b.name.clone());
    let nodes: Vec<Node> = l
        .stations
        .iter()
        .map(|&s| {
            let (today, yesterday) = w.subway.boardings_at(s);
            Node { name: name(s), on: today, off: 0, tip: format!("{}: {today} boarded today, {yesterday} yesterday. Click to select the station.", name(s)), picked: *p.selection == crate::tools::Selection::Building(s) }
        })
        .collect();
    let legs_n = if l.looped { n } else { n.saturating_sub(1) };
    let legs: Vec<Leg> = (0..legs_n).map(|i| Leg { text: format!("{:.0} min", l.ride_minutes(i, (i + 1) % n, secs).max(1.0)), broken: false }).collect();
    let total = l.length();
    let mut trains: Vec<(f32, nd_sim::MetroTrainId, Vehicle)> = w
        .subway
        .trains
        .iter()
        .filter(|(_, t)| t.line == id)
        .map(|(tid, t)| {
            let at = metro_units(&l.stops, total, l.looped, t.forward, t.run.s);
            let state = if t.run.dwell > 0.0 { ("at platform", GOOD) } else if t.run.held { ("held", AMBER) } else { ("running", IVORY) };
            let next = t.run.stops.get(t.run.next_stop).map(|(b, _)| name(*b));
            let terminus = t.run.stops.last().map(|(b, _)| name(*b)).unwrap_or_default();
            let rows = vec![(format!("next {}", next.unwrap_or_else(|| "—".into())), IVORY), (format!("towards {terminus}"), Color32::from_gray(150))];
            let v = Vehicle { at: Some(at), title: String::new(), state: (state.0.into(), state.1), load: (t.run.passengers.len(), nd_sim::subway::CAPACITY), rows, picked: *p.selection == crate::tools::Selection::Metro(tid) };
            (at, tid, v)
        })
        .collect();
    trains.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let ids: Vec<nd_sim::MetroTrainId> = trains.iter().map(|t| t.1).collect();
    let vehicles: Vec<Vehicle> = trains.into_iter().enumerate().map(|(k, (_, _, mut v))| {
        v.title = format!("Train {}", k + 1);
        v
    }).collect();
    let empty = vehicles.is_empty();
    let track = Track { id: egui::Id::new(("metro_track", p.sim.generation, id)), color: col, nodes, legs, looped: l.looped, vehicles, editing: false, slot: None };
    match diagram::show(ui, &track) {
        Some(Clicked::Stop(i)) => {
            let s = l.stations[i];
            let at = w.metro_platform(s).unwrap_or_default();
            *act = Some(Act::SelectStation(s, Vec2::new(at.x, at.y)));
        }
        Some(Clicked::Vehicle(k)) => *act = Some(Act::SelectTrain(ids[k])),
        _ => {}
    }
    if empty {
        hint(ui, &format!("No train in the tunnel now (trains run {:02}:00 to midnight).", 5));
    }
}

/// Carry out what the window asked for.
fn apply(p: &mut UiCtx, act: Option<Act>) {
    let Some(act) = act else { return };
    let w = &mut p.sim.world;
    match act {
        Act::Pick(r) => {
            // Opening another line while editing edits that one.
            if let (Some(r), Some(_)) = (r, p.tools.line_edit) {
                crate::tools::line_edit::start(p.tools, r);
            }
            p.tools.route_sel = r;
            p.tools.stop_sel = None;
        }
        Act::PickMetro(l) => p.tools.metro_sel = l,
        Act::SelectStop(r, i, at) => {
            p.tools.route_sel = Some(r);
            p.tools.stop_sel = Some((r, i));
            p.fly_to = Some(at);
        }
        Act::DeleteStop(r, i) => {
            w.remove_bus_stop(r, i);
            p.tools.stop_sel = None;
            if !w.transit.routes.contains_key(r) {
                p.tools.route_sel = None;
                p.flags.message = "Last stop removed: the line is gone.".into();
            } else {
                if let Some(e) = p.tools.line_edit.as_mut().filter(|e| e.route == r) {
                    e.removed(i, w.transit.routes[r].stops.len());
                }
                p.flags.message = format!("Stop {} removed from {}.", i + 1, w.transit.routes[r].name);
            }
        }
        Act::Rename(r, name) => {
            if let Err(e) = w.rename_bus_route(r, &name) {
                p.flags.message = e;
            }
        }
        Act::Recolour(r, c) => {
            let _ = w.recolour_bus_route(r, c);
        }
        Act::Buses(r, n) => w.set_route_buses(r, n),
        Act::DeleteLine(r) => {
            let name = w.transit.routes.get(r).map(|x| x.name.clone()).unwrap_or_default();
            w.remove_bus_route(r);
            p.tools.route_sel = None;
            p.tools.stop_sel = None;
            p.flags.message = format!("{name} deleted; its buses went home.");
        }
        Act::SelectBus(c) => *p.selection = crate::tools::Selection::Car(c),
        Act::Edit(r) => crate::tools::line_edit::start(p.tools, r),
        Act::Finish => p.flags.message = crate::tools::line_edit::finish(w, p.tools),
        Act::NewLine => {
            let r = w.add_bus_route();
            crate::tools::line_edit::start(p.tools, r);
            p.tools.status = "New line: click streets on the map for its stops, round a loop.".into();
        }
        Act::Slot(i) => {
            if let Some(e) = p.tools.line_edit.as_mut() {
                e.toggle_slot(i);
                p.tools.status = match e.slot {
                    Some(s) => format!("The next stop clicked goes in as stop {}.", s + 1),
                    None => "Stops go in where they belong again.".into(),
                };
            }
        }
        Act::Headway(l, m) => {
            let _ = w.set_metro_headway(l, m);
        }
        Act::DeleteMetro(l) => {
            let name = w.subway.lines.get(l).map(|x| x.name.clone()).unwrap_or_default();
            if w.remove_metro_line(l) {
                p.flags.message = format!("{name} deleted.");
            }
            p.tools.metro_sel = None;
        }
        Act::SelectStation(b, at) => {
            *p.selection = crate::tools::Selection::Building(b);
            p.fly_to = Some(at);
        }
        Act::SelectTrain(t) => *p.selection = crate::tools::Selection::Metro(t),
    }
}

/// [lines] While a line is edited, over the map: the cursor says what a
/// click does (add, drag, or nothing there), with a "+" badge by the
/// pointer when it adds a stop.
pub fn edit_cursor(ctx: &egui::Context, sim: &crate::sim::Sim, tools: &crate::tools::ToolState, over_ui: bool) {
    use crate::tools::line_edit::{hit_at, Hit};
    let Some(e) = tools.line_edit else { return };
    if over_ui {
        return;
    }
    let Some(pointer) = ctx.pointer_latest_pos() else { return };
    let Some(ground) = tools.hover_ground else { return };
    let hit = if tools.stop_drag.is_some() { Hit::Own(0) } else { hit_at(&sim.world, e.route, ground) };
    let (icon, badge) = match hit {
        Hit::Own(_) => (egui::CursorIcon::Grab, None),
        Hit::Other(r, _) if r == e.route => (egui::CursorIcon::Grab, None),
        Hit::Other(..) => (egui::CursorIcon::Copy, Some(("+ share stop", GOOD))),
        Hit::Street(_) => (egui::CursorIcon::Copy, Some(("+", GOOD))),
        Hit::Nothing => (egui::CursorIcon::NotAllowed, None),
    };
    ctx.set_cursor_icon(if tools.stop_drag.is_some() { egui::CursorIcon::Grabbing } else { icon });
    if let Some((text, col)) = badge {
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("line_edit_cursor")));
        let at = pointer + egui::vec2(14.0, 10.0);
        let galley = painter.layout_no_wrap(text.to_string(), crate::ui::theme::display(13.0), Color32::BLACK);
        let r = egui::Rect::from_min_size(at, galley.size() + egui::vec2(10.0, 4.0));
        painter.rect_filled(r, 8.0, col);
        painter.galley(at + egui::vec2(5.0, 2.0), galley, Color32::BLACK);
    }
}

#[cfg(test)]
mod tests {
    use super::metro_units;

    #[test]
    fn metro_trains_are_placed_between_their_stations() {
        // Stations at 0, 100 and 300 m of a 300 m line.
        let stops = [0.0, 100.0, 300.0];
        assert_eq!(metro_units(&stops, 300.0, false, true, 0.0), 0.0);
        assert_eq!(metro_units(&stops, 300.0, false, true, 50.0), 0.5);
        assert_eq!(metro_units(&stops, 300.0, false, true, 200.0), 1.5);
        assert_eq!(metro_units(&stops, 300.0, false, true, 300.0), 2.0);
        // Coming back: 50 m into its run is 250 m from the first station.
        assert_eq!(metro_units(&stops, 300.0, false, false, 50.0), 1.75);
        // A loop of 400 m: past the last station it is on its way home.
        assert_eq!(metro_units(&stops, 400.0, true, true, 350.0), 2.5);
    }
}
