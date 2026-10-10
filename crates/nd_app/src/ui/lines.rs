//! [lines] The Lines window: every bus line, and for the one picked a strip
//! diagram to tune it by. Each stop is a row with its boardings and
//! alightings (today, and yesterday on hover), between stops the leg's
//! length and how long the buses take over it, and the buses themselves
//! drawn on the strip where they are now. Stops can be picked on the map
//! (the camera flies there, the stop is marked) or deleted; the line can be
//! renamed, recoloured, given more or fewer buses, or deleted. The figures
//! are the sim's own (`nd_sim::transit::StopStats`), so the remote API's
//! `bus {action: "line"}` reads the same.

use bevy::prelude::Vec2;
use bevy_egui::egui::{self, Color32};
use nd_sim::RouteId;

use super::{hint, section, UiCtx, GOLD, IVORY, WARN};

/// The line colours as the map draws them (the gizmo palette in
/// `tools.rs`, brought into display range), by `BusRoute::color`.
fn line_color(i: u8) -> Color32 {
    const C: [[u8; 3]; 8] = [[60, 170, 255], [255, 90, 60], [80, 230, 90], [240, 200, 50], [220, 80, 240], [60, 130, 255], [250, 140, 80], [200, 200, 200]];
    let c = C[i as usize % C.len()];
    super::theme::rgb_u8(c)
}

/// What the window asks of the world after drawing (it reads the world
/// while drawing, so edits wait until the end of the pass).
enum Act {
    Pick(Option<RouteId>),
    SelectStop(RouteId, usize, Vec2),
    DeleteStop(RouteId, usize),
    Rename(RouteId, String),
    Recolour(RouteId, u8),
    Buses(RouteId, u8),
    Fare(RouteId, f32),
    DeleteLine(RouteId),
    SelectBus(nd_sim::CarId),
}

/// Stop names, worked out once per edit of the lines: the nearest building
/// to each stop, so the strip reads "by Kowloon Noodle Bar", not "stop 4".
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
    {
        let w = &p.sim.world;
        if w.transit.routes.is_empty() {
            hint(ui, "No bus lines yet. The Buses tool draws them: click streets round a loop, Enter to close it.");
            return;
        }
        let sel = p.tools.route_sel.filter(|r| w.transit.routes.contains_key(*r));
        section(ui, "LINES");
        egui::Grid::new("lines_list").num_columns(5).spacing(egui::vec2(10.0, 2.0)).striped(true).show(ui, |ui| {
            for h in ["line", "stops", "buses", "today", "yesterday"] {
                ui.label(egui::RichText::new(h).small().weak());
            }
            ui.end_row();
            for (id, r) in &w.transit.routes {
                ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot.center(), 4.5, line_color(r.color));
                    let picked = sel == Some(id);
                    if ui.selectable_label(picked, egui::RichText::new(&r.name).color(if picked { GOLD } else { IVORY })).on_hover_text("Show this line on the map and in the strip below").clicked() {
                        act = Some(Act::Pick(if picked { None } else { Some(id) }));
                    }
                });
                ui.label(egui::RichText::new(format!("{}", r.stops.len())).small());
                ui.label(egui::RichText::new(format!("{}", r.buses)).small());
                ui.label(egui::RichText::new(format!("{}", r.riders_today)).small());
                ui.label(egui::RichText::new(format!("{}", r.boardings_yesterday())).small().weak());
                ui.end_row();
            }
        });
        match sel {
            None => hint(ui, "Pick a line for its stops, its buses and the times between stops."),
            Some(rid) => line(ui, p, rid, &mut act),
        }
    }
    let w = &mut p.sim.world;
    match act {
        None => {}
        Some(Act::Pick(r)) => {
            p.tools.route_sel = r;
            p.tools.stop_sel = None;
        }
        Some(Act::SelectStop(r, i, at)) => {
            p.tools.route_sel = Some(r);
            p.tools.stop_sel = Some((r, i));
            p.fly_to = Some(at);
        }
        Some(Act::DeleteStop(r, i)) => {
            w.remove_bus_stop(r, i);
            p.tools.stop_sel = None;
            if !w.transit.routes.contains_key(r) {
                p.tools.route_sel = None;
                p.flags.message = "Last stop removed: the line is gone.".into();
            } else {
                p.flags.message = format!("Stop {} removed from {}.", i + 1, w.transit.routes[r].name);
            }
        }
        Some(Act::Rename(r, name)) => {
            if let Err(e) = w.rename_bus_route(r, &name) {
                p.flags.message = e;
            }
        }
        Some(Act::Recolour(r, c)) => {
            let _ = w.recolour_bus_route(r, c);
        }
        Some(Act::Buses(r, n)) => w.set_route_buses(r, n),
        Some(Act::Fare(r, f)) => {
            let _ = w.set_route_fare(r, f);
        }
        Some(Act::DeleteLine(r)) => {
            let name = w.transit.routes.get(r).map(|x| x.name.clone()).unwrap_or_default();
            w.remove_bus_route(r);
            p.tools.route_sel = None;
            p.tools.stop_sel = None;
            p.flags.message = format!("{name} deleted; its buses went home.");
        }
        Some(Act::SelectBus(c)) => {
            *p.selection = crate::tools::Selection::Car(c);
        }
    }
}

/// The picked line: its controls, figures and strip diagram.
fn line(ui: &mut egui::Ui, p: &UiCtx, rid: RouteId, act: &mut Option<Act>) {
    let w = &p.sim.world;
    let r = &w.transit.routes[rid];
    let tsp = w.config.traffic_seconds_per_tick;
    let n = r.stops.len();
    let col = line_color(r.color);
    ui.separator();
    // Name and colour.
    let name_id = egui::Id::new(("line_name_edit", rid));
    let mut name: String = ui.ctx().data(|d| d.get_temp(name_id)).unwrap_or_else(|| r.name.clone());
    ui.horizontal(|ui| {
        let edit = ui.add(egui::TextEdit::singleline(&mut name).desired_width(150.0).font(egui::TextStyle::Heading).text_color(col));
        if edit.lost_focus() && name.trim() != r.name {
            *act = Some(Act::Rename(rid, name.clone()));
        }
        if edit.has_focus() {
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
    // Buses and delete.
    let buses_out: Vec<(nd_sim::CarId, &nd_sim::Car)> = w.traffic.cars.iter().filter(|(_, c)| c.bus.as_ref().is_some_and(|b| b.route == rid)).collect();
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("buses {} wanted, {} out", r.buses, buses_out.len())).small());
        if ui.small_button("−").on_hover_text("One bus fewer").clicked() && r.buses > 0 {
            *act = Some(Act::Buses(rid, r.buses - 1));
        }
        if ui.small_button("+").on_hover_text("One bus more").clicked() {
            *act = Some(Act::Buses(rid, r.buses.saturating_add(1)));
        }
        ui.add_space(12.0);
        // [fix/review-app] Keyed by the city too: a loaded city reuses ids.
        let armed_id = egui::Id::new(("line_delete_armed", p.sim.generation, rid));
        let armed = ui.ctx().data(|d| d.get_temp::<bool>(armed_id)).unwrap_or(false);
        let label = if armed { egui::RichText::new("Really delete?").color(WARN).strong() } else { egui::RichText::new("Delete line").color(WARN) };
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
    ui.label(egui::RichText::new(format!("{} riders today · {} yesterday · {waiting} waiting now · loop {:.2} km, about {:.0} min round", r.riders_today, r.boardings_yesterday(), length / 1000.0, lap)).small());
    // [pt-transit] The line's books, and what keeps it from running (or
    // is worth a warning), from the sim's `line_problems`.
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("fare {:.2} · yesterday {:.0} in fares, {:.0} running costs", r.fare, r.fares_yesterday, r.upkeep_yesterday)).small());
        if ui.small_button("−").on_hover_text("Cheaper fare: more riders").clicked() {
            *act = Some(Act::Fare(rid, (r.fare - 0.5).max(0.0)));
        }
        if ui.small_button("+").on_hover_text("Dearer fare: fewer riders, more per ride").clicked() {
            *act = Some(Act::Fare(rid, r.fare + 0.5));
        }
    });
    for problem in w.line_problems(rid) {
        let color = if problem.blocks() { WARN } else { Color32::from_gray(170) };
        ui.label(egui::RichText::new(format!("{} {problem}", if problem.blocks() { "!" } else { "·" })).color(color).small());
    }
    if n == 0 {
        return;
    }
    // Buses on the strip: (leg index, fraction along it, car, coming from the depot or going home).
    let mut on_strip: Vec<(usize, f32, nd_sim::CarId, bool)> = Vec::new();
    let mut off_strip: Vec<(nd_sim::CarId, &str)> = Vec::new();
    for (cid, c) in &buses_out {
        let b = c.bus.as_ref().unwrap();
        if b.next_stop == usize::MAX {
            off_strip.push((*cid, "going home to the depot"));
            continue;
        }
        if !w.transit.departures.contains_key(cid) {
            off_strip.push((*cid, "on its way out from the depot"));
            continue;
        }
        if let Some(pr) = r.bus_progress(b.next_stop, c.pos) {
            let leg = (pr.floor() as usize).min(n - 1);
            on_strip.push((leg, pr - leg as f32, *cid, b.to_depot));
        }
    }
    let busiest = r.stops.iter().map(|s| s.stats.on_today.max(s.stats.off_today)).max().unwrap_or(0).max(1);
    let names = stop_names(ui.ctx(), w, p.sim.generation);
    section(ui, "STOPS, IN THE ORDER THE BUSES RUN");
    hint(ui, "green got on, red got off today (hover for yesterday); between stops the leg's length and time, timed by the buses where they have run it");
    let selected_stop = p.tools.stop_sel.filter(|(sr, _)| *sr == rid).map(|(_, i)| i);
    for i in 0..n {
        let s = &r.stops[i];
        // The stop row.
        ui.horizontal(|ui| {
            let (rail, _) = ui.allocate_exact_size(egui::vec2(22.0, 26.0), egui::Sense::hover());
            let pnt = ui.painter();
            pnt.vline(rail.center().x, rail.y_range(), egui::Stroke::new(4.0, col));
            let here = selected_stop == Some(i);
            pnt.circle_filled(rail.center(), if here { 8.0 } else { 6.5 }, Color32::from_rgb(12, 14, 24));
            pnt.circle_stroke(rail.center(), if here { 8.0 } else { 6.5 }, egui::Stroke::new(2.5, if here { IVORY } else { col }));
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    let label = egui::RichText::new(format!("{}  {}", i + 1, names.names.get(&(rid, i)).map_or("", |s| s.as_str()))).color(if here { GOLD } else { IVORY });
                    ui.add(egui::Label::new(label).truncate());
                });
                ui.horizontal(|ui| {
                    let (bars, resp) = ui.allocate_exact_size(egui::vec2(120.0, 8.0), egui::Sense::hover());
                    let half = bars.width() / 2.0;
                    let on = s.stats.on_today as f32 / busiest as f32;
                    let off = s.stats.off_today as f32 / busiest as f32;
                    let pnt = ui.painter();
                    pnt.rect_filled(egui::Rect::from_min_max(egui::pos2(bars.center().x - half * on, bars.top()), egui::pos2(bars.center().x, bars.bottom())), 1.0, super::theme::GOOD);
                    pnt.rect_filled(egui::Rect::from_min_max(egui::pos2(bars.center().x, bars.top()), egui::pos2(bars.center().x + half * off, bars.bottom())), 1.0, WARN);
                    pnt.vline(bars.center().x, bars.y_range(), egui::Stroke::new(1.0, Color32::from_gray(90)));
                    resp.on_hover_text(format!("today {} on, {} off · yesterday {} on, {} off", s.stats.on_today, s.stats.off_today, s.stats.on_yesterday, s.stats.off_yesterday));
                    ui.label(egui::RichText::new(format!("+{} −{}", s.stats.on_today, s.stats.off_today)).small());
                    if ui.small_button("show").on_hover_text("Fly to this stop and mark it on the map").clicked() {
                        *act = Some(Act::SelectStop(rid, i, Vec2::new(s.pos.x, s.pos.y)));
                    }
                    if ui.small_button(egui::RichText::new("delete").color(WARN)).on_hover_text("Remove this stop; riders bound for it decide again").clicked() {
                        *act = Some(Act::DeleteStop(rid, i));
                    }
                });
            });
        });
        // The leg to the next stop, with the buses on it.
        if n < 2 {
            break;
        }
        let (mins, timed) = r.leg_minutes(i, tsp);
        let broken = r.broken.contains(&i);
        ui.horizontal(|ui| {
            let (rail, _) = ui.allocate_exact_size(egui::vec2(22.0, 30.0), egui::Sense::hover());
            let pnt = ui.painter();
            pnt.vline(rail.center().x, rail.y_range(), egui::Stroke::new(4.0, if broken { WARN } else { col.gamma_multiply(0.7) }));
            for (_, f, _, home) in on_strip.iter().filter(|b| b.0 == i) {
                let at = egui::pos2(rail.center().x, rail.top() + rail.height() * f);
                let r = egui::Rect::from_center_size(at, egui::vec2(16.0, 9.0));
                pnt.rect_filled(r, 2.0, if *home { Color32::from_gray(120) } else { IVORY });
                pnt.rect_stroke(r, 2.0, egui::Stroke::new(1.0, col), egui::StrokeKind::Inside);
            }
            let text = if i + 1 == n { format!("{:.0} m back to stop 1", r.leg_length(i)) } else { format!("{:.0} m", r.leg_length(i)) };
            ui.label(egui::RichText::new(format!("{text} · {} {:.1} min", if timed { "takes" } else { "about" }, mins)).small().color(if broken { WARN } else { Color32::from_gray(150) }));
            for (_, _, cid, _) in on_strip.iter().filter(|b| b.0 == i) {
                let pax = w.traffic.cars.get(*cid).and_then(|c| c.bus.as_ref()).map_or(0, |b| b.passengers.len());
                if ui.small_button(format!("bus · {pax} aboard")).on_hover_text("Select this bus").clicked() {
                    *act = Some(Act::SelectBus(*cid));
                }
            }
        });
    }
    for (cid, what) in off_strip {
        ui.horizontal(|ui| {
            hint(ui, &format!("a bus {what}"));
            if ui.small_button("select").clicked() {
                *act = Some(Act::SelectBus(cid));
            }
        });
    }
}
