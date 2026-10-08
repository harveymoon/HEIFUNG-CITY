//! [metro-ux] Drawing metro lines station to station, and the Metro map
//! layer they are drawn on.
//!
//! - **The layer** (`overlay {layer: "metro"}`, M, the Layer menu): the city
//!   dimmed (`render::focus::Focus::Metro`), every metro line drawn over it
//!   in its colour along its tunnel, and every station a big glowing marker
//!   with its name (egui, so it reads at any zoom). Picking the metro line
//!   tool (Build window, Transit, Metro lines) switches to it; leaving the
//!   tool puts back the layer that was showing.
//! - **The tool** (`tools::metro_click`): a click on a station adds it to
//!   the line and previews the tunnel to it; the first station again closes
//!   a loop; Enter or Finish lays the line; a right-click or Backspace takes
//!   back the last stop; Esc cancels. A click that misses every station
//!   does nothing but say so. Stations are placed beforehand with the Build
//!   window (Transit, Metro, Metro station).
//! - **The panel**: the steps, with the one the player is on lit, and the
//!   line's figures so far (stops, length, round trip, trains, cost).
//!
//! Tunnels are bored straight from station to station under everything
//! (`nd_sim::subway`), so any two stations connect: the only things that
//! refuse a click are the order (the last stop again, a station already on
//! the line, a loop of two) and a treasury that cannot pay the tunnel, and
//! each says why. Digging is part of laying the line, as it always was.

use bevy::gizmos::config::{GizmoConfig, GizmoConfigGroup, GizmoLineConfig};
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use nd_sim::subway::{DraftStep, MetroDraft};
use nd_sim::{BuildingId, Vec2 as SVec2, World};

use crate::camera::Cursor;
use crate::map_layers::{self, LayerId};
use crate::render::Overlays;
use crate::sim::{Sim, SimSet};
use crate::tools::{Tool, ToolState, TransitMode};
use crate::ui::{UiFlags, GOLD, IVORY, WARN};

/// How far from a station's platform or entrance a click still picks it
/// (stations stand 160 m apart, so this never picks the wrong one).
pub const PICK_RADIUS: f32 = 75.0;

/// The metro layer's lines and rings: drawn through the city, wide.
#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct MetroGizmos;

pub struct MetroUxPlugin;

impl Plugin for MetroUxPlugin {
    fn build(&self, app: &mut App) {
        app.insert_gizmo_config(MetroGizmos, GizmoConfig { depth_bias: -1.0, line: GizmoLineConfig { width: 5.0, ..default() }, ..default() })
            .add_systems(Update, (layer_key, follow_tool, metro_gizmos).chain().in_set(SimSet::Input).after(crate::tools::tool_input))
            .add_systems(EguiPrimaryContextPass, (station_labels, panel).after(crate::ui::side_panel));
    }
}

/// The metro line tool is in hand.
pub fn tool_active(t: &ToolState) -> bool {
    t.tool == Tool::Transit && t.transit_mode == TransitMode::Metro
}

fn metro_layer() -> Option<LayerId> {
    LayerId::parse("metro")
}

fn name(w: &World, b: BuildingId) -> String {
    w.buildings.get(b).map_or_else(|| "that station".to_string(), |b| b.name.clone())
}

// ── the line being drawn ────────────────────────────────────────────────────

/// Click `station` into the line; the status line to show.
pub fn add_stop(w: &World, d: &mut MetroDraft, station: BuildingId) -> String {
    let n = name(w, station);
    match w.metro_draft_add(d, station) {
        DraftStep::Started => format!("{n}: line started. Now click the next station; the tunnel to it is drawn as you go."),
        DraftStep::Added => {
            let p = w.metro_draft_preview(d);
            let prev = d.stations.len().checked_sub(2).map(|i| name(w, d.stations[i])).unwrap_or_default();
            format!(
                "{n} added (stop {}): {:.0} m of tunnel from {prev}. Click the next station, the first one again for a loop, or Enter to finish.",
                d.len(),
                p.legs_m.last().copied().unwrap_or(0.0)
            )
        }
        DraftStep::Looped => {
            let p = w.metro_draft_preview(d);
            format!("Loop closed at {n}: {} stations, {:.1} km round. Enter or Finish lays it; Backspace opens it again.", d.len(), p.length_m / 1000.0)
        }
        DraftStep::Refused(why) => format!("{n} not added: {why}."),
    }
}

/// Take back the last stop (or open a closed loop); the status line.
pub fn undo_stop(w: &World, d: &mut MetroDraft) -> String {
    let was_loop = d.looped;
    let last = d.stations.last().copied();
    if !d.undo() {
        return "Nothing to take back: click a station to start a line.".into();
    }
    if was_loop {
        return "Loop opened again: click another station, or Enter to lay the line end to end.".into();
    }
    match (last, d.len()) {
        (Some(b), 0) => format!("Took back {}: the line is empty.", name(w, b)),
        (Some(b), n) => format!("Took back {}: {n} stop{} left.", name(w, b), if n == 1 { "" } else { "s" }),
        _ => String::new(),
    }
}

/// Lay the line drawn so far; the status line.
pub fn finish_line(w: &mut World, d: &mut MetroDraft) -> String {
    if !d.ready() {
        return if d.is_empty() { "No line to lay: click stations first.".into() } else { "A line needs two stations or more: click the next station.".into() };
    }
    let cost = w.metro_draft_preview(d).tunnel_cost;
    match w.add_metro_line(&d.calls()) {
        Ok(id) => {
            d.clear();
            let l = &w.subway.lines[id];
            format!(
                "{} laid: {} stations{}, {:.1} km of tunnel for {:.0}. Trains every {} minutes each way from 05:00.",
                l.name,
                l.stations.len(),
                if l.looped { " in a loop" } else { "" },
                l.length() / 1000.0,
                cost,
                l.headway
            )
        }
        Err(e) => format!("No line: {e}."),
    }
}

/// What to say when a click misses every station.
pub fn missed_hint(w: &World) -> String {
    let n = w.metro_stations().len();
    if n < 2 {
        format!("No station there. The city has {n} metro station{}; a line joins two or more. Place them first: Build window, Transit, Metro, Metro station.", if n == 1 { "" } else { "s" })
    } else {
        "No station there: click one of the glowing station markers (its lot or the M by its entrance).".into()
    }
}

// ── the layer ───────────────────────────────────────────────────────────────

/// M shows or hides the Metro layer.
fn layer_key(keys: Res<ButtonInput<KeyCode>>, flags: Res<UiFlags>, mut overlays: ResMut<Overlays>) {
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if keys.just_pressed(KeyCode::KeyM) && !flags.keyboard_captured && !ctrl {
        let on = overlays.metro;
        map_layers::set_active(&mut overlays, if on { None } else { metro_layer() });
    }
}

/// Picking the metro line tool shows the Metro layer; leaving it puts back
/// the layer that was showing (unless the player picked another meanwhile).
fn follow_tool(mut tools: ResMut<ToolState>, mut overlays: ResMut<Overlays>, mut before: Local<Option<Option<LayerId>>>) {
    let active = tool_active(&tools);
    match (active, before.is_some()) {
        (true, false) => {
            // The panel shows the status line: start it clean (unless the
            // API clicked stations in this frame already).
            if tools.metro_draft.is_empty() {
                tools.status.clear();
            }
            *before = Some(map_layers::active(&overlays));
            if !overlays.metro {
                map_layers::set_active(&mut overlays, metro_layer());
            }
        }
        (false, true) => {
            let prev = before.take().flatten();
            if overlays.metro && prev != metro_layer() {
                map_layers::set_active(&mut overlays, prev);
            }
        }
        _ => {}
    }
}

fn flat(at: Vec3) -> Isometry3d {
    Isometry3d::new(at, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2))
}

/// Station marker cyan (linear), as in the legend.
const STATION: [f32; 3] = [0.35, 1.6, 2.2];

/// The stations' rings on the ground, through the dimmed city. (The lines
/// are painted in screen space by `station_labels`: gizmo lines far off
/// lose stretches under the translucent street overlays.)
fn metro_gizmos(sim: Res<Sim>, tools: Res<ToolState>, overlays: Res<Overlays>, time: Res<Time>, mut g: Gizmos<MetroGizmos>) {
    let drawing = tool_active(&tools);
    if !(overlays.metro || drawing) {
        return;
    }
    let w = &sim.world;
    let lift = |p: SVec2, dy: f32| Vec3::new(p.x, w.surface_height(p) + dy, p.y);
    let pulse = (time.elapsed_secs() * 2.2).sin() * 0.5 + 0.5;
    let draft = &tools.metro_draft;
    for s in w.metro_stations() {
        let Some(p) = w.metro_platform(s) else { continue };
        let at = lift(p, 3.2);
        let base = Color::linear_rgb(STATION[0], STATION[1], STATION[2]);
        g.circle(flat(at), 22.0, base);
        g.circle(flat(at), 16.0, base.with_alpha(0.6));
        if draft.stations.contains(&s) {
            g.circle(flat(at), 11.0, Color::WHITE);
        }
        if tools.metro_hover == Some(s) && drawing {
            g.circle(flat(at), 27.0 + 5.0 * pulse, Color::linear_rgb(2.5, 2.5, 2.5));
        }
    }
}

/// A linear colour as egui's.
fn c32(c: [f32; 3], k: f32) -> egui::Color32 {
    egui::Color32::from(egui::Rgba::from_rgb((c[0] * k).min(1.0), (c[1] * k).min(1.0), (c[2] * k).min(1.0)))
}

/// A polyline on the ground, projected; broken where it leaves the view.
fn paint_path(painter: &egui::Painter, to_screen: &dyn Fn(SVec2) -> Option<egui::Pos2>, path: &[SVec2], stroke: egui::Stroke) {
    let mut run: Vec<egui::Pos2> = Vec::new();
    for &p in path {
        match to_screen(p) {
            Some(q) => run.push(q),
            None => {
                if run.len() >= 2 {
                    painter.add(egui::Shape::line(std::mem::take(&mut run), stroke));
                }
                run.clear();
            }
        }
    }
    if run.len() >= 2 {
        painter.add(egui::Shape::line(run, stroke));
    }
}

/// The lines in their colours, the line being drawn (white) and where the
/// station under the pointer would take it, or a dashed lead to the
/// pointer when it is on no station.
fn paint_lines(painter: &egui::Painter, to_screen: &dyn Fn(SVec2) -> Option<egui::Pos2>, w: &World, tools: &ToolState, cursor: Option<SVec2>, pulse: f32) {
    for l in w.subway.lines.values() {
        let c = nd_procgen::metro::line_colour(l.color);
        paint_path(painter, to_screen, &l.path, egui::Stroke::new(7.0, egui::Color32::from_black_alpha(140)));
        paint_path(painter, to_screen, &l.path, egui::Stroke::new(4.5, c32(c, 1.6)));
    }
    if !tool_active(tools) {
        return;
    }
    let draft = &tools.metro_draft;
    let now = w.metro_draft_preview(draft);
    if now.path.len() >= 2 {
        paint_path(painter, to_screen, &now.path, egui::Stroke::new(8.0, egui::Color32::from_black_alpha(160)));
        paint_path(painter, to_screen, &now.path, egui::Stroke::new(5.0, egui::Color32::from_rgb(245, 245, 255)));
    }
    let Some(&last) = draft.stations.last() else { return };
    let Some(from) = w.metro_platform(last) else { return };
    match tools.metro_hover.filter(|h| *h != last) {
        Some(h) => {
            let mut next = draft.clone();
            if matches!(w.metro_draft_add(&mut next, h), DraftStep::Refused(_)) {
                if let Some(q) = w.metro_platform(h) {
                    paint_path(painter, to_screen, &[from, q], egui::Stroke::new(3.0, WARN));
                }
            } else {
                // The new leg, pulsing: the tunnel re-laid through the station.
                let p = w.metro_draft_preview(&next);
                let s0 = p.path.len().saturating_sub(1).min(nearest_index(&p.path, from));
                let col = egui::Color32::from_rgb(140, 255, 170).gamma_multiply(0.7 + 0.3 * pulse);
                paint_path(painter, to_screen, &p.path[s0..], egui::Stroke::new(5.0, col));
            }
        }
        None if !draft.looped => {
            if let (Some(c), Some(a)) = (cursor, to_screen(from)) {
                if let Some(b) = to_screen(c) {
                    painter.add(egui::Shape::dashed_line(&[a, b], egui::Stroke::new(2.0, egui::Color32::from_white_alpha(150)), 8.0, 6.0));
                }
            }
        }
        None => {}
    }
}

/// The point on `path` nearest `p`, by index (from the end, so on a loop
/// the closing pass is found rather than the start).
fn nearest_index(path: &[SVec2], p: SVec2) -> usize {
    path.iter().enumerate().rev().min_by(|a, b| a.1.distance(p).total_cmp(&b.1.distance(p))).map_or(0, |(i, _)| i)
}

/// Every station's marker and name, painted over the map.
#[allow(clippy::too_many_arguments)]
fn station_labels(mut contexts: EguiContexts, sim: Res<Sim>, tools: Res<ToolState>, overlays: Res<Overlays>, cursor: Res<Cursor>, time: Res<Time>, flags: Res<UiFlags>, cams: Query<(&Camera, &GlobalTransform), With<crate::camera::OrbitCamera>>) -> Result {
    let drawing = tool_active(&tools);
    if !(overlays.metro || drawing) {
        return Ok(());
    }
    let Some((camera, gt)) = cams.iter().next() else { return Ok(()) };
    let ctx = contexts.ctx_mut()?;
    // [city-data] The map is covered: nothing to label.
    if crate::ui::city_data::is_open(ctx) {
        return Ok(());
    }
    let w = &sim.world;
    let full = ctx.viewport_rect();
    // Between the bars (the top bar and the toolbar sit on this layer too).
    let screen = flags.map_rect(full);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("metro_labels"))).with_clip_rect(screen);
    let to_screen = |p: SVec2| -> Option<egui::Pos2> {
        let v = camera.world_to_viewport(gt, Vec3::new(p.x, w.surface_height(p) + 3.0, p.y)).ok()?;
        Some(crate::ui::theme::to_points(ctx, v))
    };
    let pulse = (time.elapsed_secs() * 2.2).sin() * 0.5 + 0.5;
    paint_lines(&painter, &to_screen, w, &tools, cursor.ground, pulse);
    let draft = &tools.metro_draft;
    let cyan = egui::Color32::from_rgb(120, 235, 255);
    for s in w.metro_stations() {
        let Some(p) = w.metro_platform(s) else { continue };
        let Ok(v) = camera.world_to_viewport(gt, Vec3::new(p.x, w.surface_height(p) + 3.2, p.y)) else { continue };
        let at = crate::ui::theme::to_points(ctx, v);
        if !screen.contains(at) {
            continue;
        }
        let hovered = drawing && tools.metro_hover == Some(s);
        let order = draft.stations.iter().position(|&x| x == s);
        let r = if hovered { 17.0 } else { 14.0 };
        let ring = if hovered { egui::Color32::WHITE } else if order.is_some() { egui::Color32::from_rgb(255, 230, 150) } else { cyan };
        painter.circle(at, r + 3.0, egui::Color32::from_black_alpha(150), egui::Stroke::NONE);
        painter.circle(at, r, egui::Color32::from_rgb(14, 24, 40), egui::Stroke::new(3.0, ring));
        painter.text(at, egui::Align2::CENTER_CENTER, "M", crate::ui::theme::display(if hovered { 19.0 } else { 17.0 }), ring);
        // Its place in the line being drawn.
        if let Some(i) = order {
            let badge = at + egui::vec2(-r, -r);
            painter.circle(badge, 8.5, GOLD, egui::Stroke::new(1.0, egui::Color32::BLACK));
            painter.text(badge, egui::Align2::CENTER_CENTER, format!("{}", i + 1), crate::ui::theme::small(), egui::Color32::BLACK);
        }
        // The name, with a dot per line calling there.
        let lines: Vec<egui::Color32> = w.subway.lines_at(s).map(|(_, l)| {
            let c = nd_procgen::metro::line_colour(l.color);
            egui::Color32::from(egui::Rgba::from_rgb(c[0].min(1.0), c[1].min(1.0), c[2].min(1.0)))
        }).collect();
        let text = name(w, s);
        let galley = painter.layout_no_wrap(text, if hovered { crate::ui::theme::body() } else { crate::ui::theme::small() }, IVORY);
        let pos = at + egui::vec2(r + 8.0, -galley.size().y * 0.5);
        let dots = lines.len() as f32 * 10.0;
        let bg = egui::Rect::from_min_size(pos - egui::vec2(5.0, 2.0), galley.size() + egui::vec2(10.0 + dots, 4.0));
        painter.rect_filled(bg, 4.0, egui::Color32::from_rgba_unmultiplied(6, 10, 20, 215));
        painter.rect_stroke(bg, 4.0, egui::Stroke::new(1.0, ring.gamma_multiply(0.7)), egui::StrokeKind::Inside);
        painter.galley(pos, galley.clone(), IVORY);
        for (k, c) in lines.iter().enumerate() {
            painter.circle_filled(egui::pos2(pos.x + galley.size().x + 8.0 + k as f32 * 10.0, at.y), 4.0, *c);
        }
    }
    Ok(())
}

// ── the panel ───────────────────────────────────────────────────────────────

/// The step-by-step panel and the line's figures while the tool is in hand.
fn panel(mut contexts: EguiContexts, mut sim: ResMut<Sim>, mut tools: ResMut<ToolState>) -> Result {
    if !tool_active(&tools) {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let tools = &mut *tools;
    let w = &mut sim.world;
    let stations = w.metro_stations().len();
    let d = tools.metro_draft.clone();
    let p = w.metro_draft_preview(&d);
    let mut act: Option<&str> = None;
    egui::Window::new("Metro line")
        .id(egui::Id::new("metro_line_panel"))
        .default_pos(egui::pos2(12.0, 64.0))
        .default_width(310.0)
        .resizable(false)
        .collapsible(true)
        .show(ctx, |ui| {
            ui.set_max_width(310.0);
            if stations < 2 {
                ui.label(egui::RichText::new(format!("The city has {stations} metro station{}. A line joins two or more.", if stations == 1 { "" } else { "s" })).color(WARN).strong());
                ui.label(egui::RichText::new("Place stations first: Build window, Transit, Metro, Metro station (any lot, 160 m apart).").color(IVORY));
                if ui.button("Place a metro station").clicked() {
                    act = Some("place");
                }
                ui.separator();
            }
            let n = d.len();
            let steps: [(&str, bool, bool); 4] = [
                ("1. Click a station to start the line.", n >= 1, n == 0),
                ("2. Click the next station: the tunnel to it is drawn.", n >= 2, n == 1),
                ("3. Keep clicking stations in order, or click the first one again to close a loop.", d.looped, n >= 2 && !d.looped),
                ("4. Enter or Finish lays the line.", false, d.looped),
            ];
            for (text, done, now) in steps {
                let t = egui::RichText::new(if done { format!("✔ {text}") } else { text.to_string() });
                ui.label(if now { t.color(GOLD).strong() } else if done { t.weak() } else { t.color(IVORY) });
            }
            ui.label(egui::RichText::new("Right-click or Backspace takes back the last stop; Esc cancels. Clicks off a station do nothing.").small().weak());
            if n > 0 {
                ui.separator();
                let names: Vec<String> = d.stations.iter().map(|&s| name(w, s)).collect();
                let route = if d.looped { format!("{} > {}", names.join(" > "), names[0]) } else { names.join(" > ") };
                ui.label(egui::RichText::new(route).color(IVORY));
                egui::Grid::new("metro_line_stats").num_columns(2).spacing(egui::vec2(10.0, 1.0)).show(ui, |ui| {
                    let row = |ui: &mut egui::Ui, k: &str, v: String, c: egui::Color32| {
                        ui.label(egui::RichText::new(k).small().weak());
                        ui.label(egui::RichText::new(v).small().color(c));
                        ui.end_row();
                    };
                    row(ui, "stops", format!("{}{}", p.stops, if p.looped { ", a loop" } else { "" }), IVORY);
                    if n >= 2 {
                        row(ui, "length", format!("{:.2} km of twin tunnel", p.length_m / 1000.0), IVORY);
                        row(ui, "round trip", format!("{:.0} min ({:.0} min a run)", p.round_trip_min, p.run_min), IVORY);
                        row(ui, "trains", format!("{} out at once, every {} min each way", p.trains, p.headway), IVORY);
                        let short = p.tunnel_cost > w.economy.treasury;
                        row(ui, "tunnel", format!("{:.0} to dig (treasury {:.0})", p.tunnel_cost, w.economy.treasury), if short { WARN } else { IVORY });
                        row(ui, "running", format!("{:.0} a day for tunnels and runs", p.upkeep_per_day), IVORY);
                    }
                });
            }
            ui.horizontal(|ui| {
                if ui.add_enabled(d.ready(), egui::Button::new(egui::RichText::new("Finish").strong())).on_hover_text("Lay the line (Enter)").clicked() {
                    act = Some("finish");
                }
                if ui.add_enabled(n > 0, egui::Button::new("Undo")).on_hover_text("Take back the last stop (right-click, Backspace)").clicked() {
                    act = Some("undo");
                }
                if ui.add_enabled(n > 0, egui::Button::new("Cancel")).on_hover_text("Drop the line (Esc)").clicked() {
                    act = Some("cancel");
                }
            });
            if !tools.status.is_empty() {
                let bad = tools.status.contains("not added") || tools.status.starts_with("No ");
                ui.label(egui::RichText::new(&tools.status).small().color(if bad { WARN } else { IVORY.gamma_multiply(0.85) }));
            }
        });
    match act {
        Some("finish") => tools.status = finish_line(w, &mut tools.metro_draft),
        Some("undo") => tools.status = undo_stop(w, &mut tools.metro_draft),
        Some("cancel") => {
            tools.metro_draft.clear();
            tools.status = "Line cancelled: nothing was built.".into();
        }
        Some("place") => {
            crate::ui::build::activate(crate::ui::build::Item::Place(nd_sim::Placeable::MetroStation), tools);
            tools.status = "Click a lot to put a metro station on it; come back to Metro lines to join stations.".into();
        }
        _ => {}
    }
    Ok(())
}
