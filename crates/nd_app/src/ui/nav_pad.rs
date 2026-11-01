//! [laptop-nav] The on-screen navigation pad (bottom left of the map): pan
//! arrows, turn, tilt, zoom and a compass that turns the view to north,
//! all worked with a plain left click (hold to keep moving), for a touchpad,
//! a remote desktop or no mouse at all. And the Settings window's
//! NAVIGATION section.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use egui::{Color32, Pos2, Rect, Sense, Stroke};

use super::theme::{GOLD, IVORY, PANEL_FILL};
use super::{hint, section};
use crate::camera::{CameraMode, OrbitCamera};
use crate::nav::{NavInput, NavMove, NavPad, NavSettings, PadMode, ScrollMode};

const CYAN: Color32 = Color32::from_rgb(120, 230, 255);
const CELL: f32 = 30.0;
const GAP: f32 = 3.0;

/// Whether the pad shows: Always, Off, or (Auto) until a mouse wheel is used.
pub fn shown(s: &NavSettings, n: &NavInput) -> bool {
    match s.pad {
        PadMode::On => true,
        PadMode::Off => false,
        PadMode::Auto => !n.wheel_seen,
    }
}

/// What a pad button draws.
#[derive(Clone, Copy)]
enum Glyph {
    Arrow(f32),
    Turn(bool),
    Tilt(bool),
    Plus,
    Minus,
}

pub fn pad(
    mut contexts: EguiContexts,
    settings: Res<NavSettings>,
    nav: Res<NavInput>,
    mut pad: ResMut<NavPad>,
    flags: Res<super::UiFlags>,
    mode: Res<CameraMode>,
    cams: Query<&OrbitCamera>,
    walk: Res<crate::camera::WalkState>,
) -> Result {
    if !shown(&settings, &nav) || flags.photo_mode || flags.data_view {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    if super::city_data::is_open(ctx) {
        return Ok(());
    }
    let map = flags.map_rect(ctx.content_rect());
    let yaw = match *mode {
        // [r5-drive] Driving, the orbit follows the camera behind the car.
        CameraMode::Orbit | CameraMode::Drive => cams.iter().next().map_or(0.0, |c| c.yaw),
        // Walk: the way the eyes face, as an orbit yaw.
        CameraMode::Walk => walk.yaw + std::f32::consts::PI,
    };
    let walking = *mode == CameraMode::Walk;
    let driving = *mode == CameraMode::Drive;
    // [r5-drive] Driving: a row more for the handbrake and the horn.
    let extra = if driving { CELL + GAP } else { 0.0 };
    let size = egui::vec2(CELL * 4.0 + GAP * 5.0 + 4.0, CELL * 3.0 + GAP * 2.0 + 10.0 + extra);
    let mut hold = NavMove::default();
    let mut tap = NavMove::default();
    let mut north = false;
    let (mut handbrake, mut horn) = (false, false);
    egui::Area::new(egui::Id::new("nav_pad"))
        .order(egui::Order::Middle)
        .fixed_pos(egui::pos2(map.left() + 12.0, map.bottom() - 12.0))
        .pivot(egui::Align2::LEFT_BOTTOM)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(PANEL_FILL).stroke(Stroke::new(1.0, GOLD.gamma_multiply(0.6))).inner_margin(egui::Margin::same(5)).show(ui, |ui| {
                let (area, _) = ui.allocate_exact_size(size, Sense::hover());
                let o = area.min;
                let cell = |col: f32, row: f32| Rect::from_min_size(o + egui::vec2(col * (CELL + GAP), row * (CELL + GAP)), egui::vec2(CELL, CELL));
                let (pan, turn, tilt) = if walking { ("Walk", "Turn", "Look") } else if driving { ("Drive", "Look", "Look") } else { ("Pan", "Turn", "Tilt") };
                let buttons: [(Rect, Glyph, NavMove, String); 10] = [
                    (cell(1.0, 0.0), Glyph::Arrow(0.0), mv(Vec2::Y, 0.0, 0.0, 0.0), if driving { "Accelerate (W / ↑)".into() } else { format!("{pan} up (W / ↑)") }),
                    (cell(1.0, 2.0), Glyph::Arrow(std::f32::consts::PI), mv(-Vec2::Y, 0.0, 0.0, 0.0), if driving { "Brake, then reverse (S / ↓)".into() } else { format!("{pan} down (S / ↓)") }),
                    (cell(0.0, 1.0), Glyph::Arrow(-std::f32::consts::FRAC_PI_2), mv(-Vec2::X, 0.0, 0.0, 0.0), if driving { "Steer left (A / ←)".into() } else { format!("{pan} left (A / ←)") }),
                    (cell(2.0, 1.0), Glyph::Arrow(std::f32::consts::FRAC_PI_2), mv(Vec2::X, 0.0, 0.0, 0.0), if driving { "Steer right (D / →)".into() } else { format!("{pan} right (D / →)") }),
                    (cell(0.0, 0.0), Glyph::Turn(true), mv(Vec2::ZERO, 0.0, 1.0, 0.0), format!("{turn} left (Q)")),
                    (cell(2.0, 0.0), Glyph::Turn(false), mv(Vec2::ZERO, 0.0, -1.0, 0.0), format!("{turn} right (E)")),
                    (cell(0.0, 2.0), Glyph::Tilt(true), mv(Vec2::ZERO, 0.0, 0.0, 1.0), if walking || driving { "Look up (R / Home)".into() } else { format!("{tilt}: look more steeply down (Home, Shift+W)") }),
                    (cell(2.0, 2.0), Glyph::Tilt(false), mv(Vec2::ZERO, 0.0, 0.0, -1.0), if walking || driving { "Look down (F / End)".into() } else { format!("{tilt}: towards the horizon (End, Shift+S)") }),
                    (zoom_rect(o, true), Glyph::Plus, mv(Vec2::ZERO, 1.0, 0.0, 0.0), if walking { "Walk forward".into() } else if driving { "Camera closer (. or wheel)".into() } else { "Zoom in (wheel, pinch, Ctrl+scroll, [ or . or Page Up)".into() }),
                    (zoom_rect(o, false), Glyph::Minus, mv(Vec2::ZERO, -1.0, 0.0, 0.0), if walking { "Walk back".into() } else if driving { "Camera further back (, or wheel)".into() } else { "Zoom out (wheel, pinch, Ctrl+scroll, ] or , or Page Down)".into() }),
                ];
                for (i, (rect, glyph, m, tip)) in buttons.into_iter().enumerate() {
                    let resp = ui.interact(rect, ui.id().with(("nav_btn", i)), Sense::click_and_drag()).on_hover_text(tip);
                    let down = resp.is_pointer_button_down_on();
                    if down {
                        hold.add(m);
                    }
                    // The press itself is a step, so a quick click moves.
                    if down && ui.input(|i| i.pointer.primary_pressed()) {
                        tap.add(m);
                    }
                    paint_button(ui.painter(), rect, glyph, down, resp.hovered());
                }
                // The compass: north's way on the screen; a click faces north.
                let c = cell(1.0, 1.0);
                let resp = ui.interact(c, ui.id().with("nav_north"), Sense::click()).on_hover_text("Face north (click). The needle points north.");
                if resp.clicked() {
                    north = true;
                }
                paint_compass(ui.painter(), c, yaw, resp.hovered());
                // [r5-drive] The handbrake and the horn, held.
                if driving {
                    let w = (CELL * 3.0 + GAP * 2.0 - GAP) / 2.0;
                    let row = o.y + 3.0 * (CELL + GAP);
                    for (k, (label, tip)) in [("HANDBRAKE", "Handbrake (Space)"), ("HORN", "Horn (H)")].into_iter().enumerate() {
                        let rect = Rect::from_min_size(egui::pos2(o.x + k as f32 * (w + GAP), row), egui::vec2(w, CELL));
                        let resp = ui.interact(rect, ui.id().with(("drive_btn", k)), Sense::click_and_drag()).on_hover_text(tip);
                        let down = resp.is_pointer_button_down_on();
                        if down {
                            if k == 0 { handbrake = true } else { horn = true }
                        }
                        let ink = if down { GOLD } else if resp.hovered() { Color32::WHITE } else { CYAN };
                        let fill = if down { Color32::from_rgb(60, 44, 16) } else { Color32::from_rgb(14, 16, 28) };
                        ui.painter().rect(rect, 4.0, fill, Stroke::new(1.0, ink.gamma_multiply(0.45)), egui::StrokeKind::Inside);
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, super::theme::display(10.0), ink);
                    }
                }
            });
        });
    pad.hold = hold;
    if !tap.is_zero() {
        pad.tap = tap;
    }
    if north {
        pad.north = true;
    }
    pad.handbrake = handbrake;
    pad.horn = horn;
    Ok(())
}

fn mv(pan: Vec2, zoom: f32, yaw: f32, tilt: f32) -> NavMove {
    NavMove { pan, zoom, yaw, tilt }
}

/// The zoom buttons, a column right of the arrows.
fn zoom_rect(o: Pos2, plus: bool) -> Rect {
    let x = o.x + 3.0 * (CELL + GAP) + GAP + 4.0;
    let h = (CELL * 3.0 + GAP * 2.0 - GAP) / 2.0;
    let y = if plus { o.y } else { o.y + h + GAP };
    Rect::from_min_size(egui::pos2(x, y), egui::vec2(CELL, h))
}

fn paint_button(p: &egui::Painter, r: Rect, g: Glyph, down: bool, hover: bool) {
    let fill = if down { Color32::from_rgb(60, 44, 16) } else if hover { Color32::from_rgb(24, 30, 48) } else { Color32::from_rgb(14, 16, 28) };
    let ink = if down { GOLD } else if hover { Color32::WHITE } else { CYAN };
    p.rect(r, 4.0, fill, Stroke::new(1.0, ink.gamma_multiply(0.45)), egui::StrokeKind::Inside);
    let c = r.center();
    let s = Stroke::new(2.0, ink);
    match g {
        Glyph::Arrow(a) => {
            // A chevron pointing at angle `a` (0 up, clockwise).
            let dir = egui::vec2(a.sin(), -a.cos());
            let side = egui::vec2(-dir.y, dir.x);
            let tip = c + dir * 7.0;
            let base = c - dir * 4.0;
            p.add(egui::Shape::convex_polygon(vec![tip, base + side * 7.0, base - side * 7.0], ink, Stroke::NONE));
        }
        Glyph::Turn(left) => {
            // An arc with an arrowhead at its end.
            let rad = 8.0;
            let (a0, a1) = if left { (0.35f32, 2.0 * std::f32::consts::PI - 1.3) } else { (std::f32::consts::PI - 0.35, -std::f32::consts::PI + 1.3) };
            let n = 14;
            let pts: Vec<Pos2> = (0..=n).map(|i| a0 + (a1 - a0) * i as f32 / n as f32).map(|t| c + egui::vec2(t.cos(), -t.sin()) * rad).collect();
            p.add(egui::Shape::line(pts.clone(), s));
            let end = pts[n];
            let prev = pts[n - 2];
            let dir = (end - prev).normalized();
            let side = egui::vec2(-dir.y, dir.x);
            p.add(egui::Shape::convex_polygon(vec![end + dir * 5.0, end + side * 4.5, end - side * 4.5], ink, Stroke::NONE));
        }
        Glyph::Tilt(steeper) => {
            // The ground as seen: nearly square from above, a thin wedge
            // towards the horizon; and an arrow for the way the eye goes.
            let (w_top, h) = if steeper { (10.0, 9.0) } else { (5.0, 4.0) };
            let w_bot = 11.0;
            let y0 = c.y + 7.0 - h;
            let quad = vec![egui::pos2(c.x - w_top, y0), egui::pos2(c.x + w_top, y0), egui::pos2(c.x + w_bot, c.y + 7.0), egui::pos2(c.x - w_bot, c.y + 7.0)];
            p.add(egui::Shape::closed_line(quad, Stroke::new(1.5, ink)));
            let ay = c.y - 8.0;
            let d = if steeper { -1.0 } else { 1.0 };
            p.add(egui::Shape::convex_polygon(vec![egui::pos2(c.x, ay + d * 4.0), egui::pos2(c.x - 4.0, ay - d * 1.5), egui::pos2(c.x + 4.0, ay - d * 1.5)], ink, Stroke::NONE));
        }
        Glyph::Plus => {
            p.line_segment([c - egui::vec2(8.0, 0.0), c + egui::vec2(8.0, 0.0)], Stroke::new(2.5, ink));
            p.line_segment([c - egui::vec2(0.0, 8.0), c + egui::vec2(0.0, 8.0)], Stroke::new(2.5, ink));
        }
        Glyph::Minus => {
            p.line_segment([c - egui::vec2(8.0, 0.0), c + egui::vec2(8.0, 0.0)], Stroke::new(2.5, ink));
        }
    }
}

fn paint_compass(p: &egui::Painter, r: Rect, yaw: f32, hover: bool) {
    let c = r.center();
    let ring = if hover { Color32::WHITE } else { GOLD.gamma_multiply(0.8) };
    p.circle(c, 13.0, Color32::from_rgb(10, 12, 22), Stroke::new(1.5, ring));
    // North on the screen: turned clockwise by the yaw (yaw 0 faces north).
    let dir = egui::vec2(yaw.sin(), -yaw.cos());
    let side = egui::vec2(-dir.y, dir.x);
    p.add(egui::Shape::convex_polygon(vec![c + dir * 11.0, c + side * 3.5, c - side * 3.5], Color32::from_rgb(255, 80, 120), Stroke::NONE));
    p.add(egui::Shape::convex_polygon(vec![c - dir * 9.0, c - side * 3.0, c + side * 3.0], IVORY.gamma_multiply(0.6), Stroke::NONE));
    p.text(c + dir * 0.0 + egui::vec2(0.0, 0.5), egui::Align2::CENTER_CENTER, "N", super::theme::display(9.0), Color32::BLACK);
}

/// [laptop-nav] The Settings window's NAVIGATION section.
pub fn settings_ui(ui: &mut egui::Ui, s: &mut NavSettings, n: &NavInput) {
    let before = *s;
    section(ui, "NAVIGATION");
    ui.horizontal(|ui| {
        ui.label("Two-finger scroll");
        for m in ScrollMode::ALL {
            if ui.selectable_label(s.scroll == m, m.label()).clicked() {
                s.scroll = m;
            }
        }
    });
    hint(ui, match s.scroll {
        ScrollMode::Auto => "Auto: a mouse wheel zooms; a touchpad's two-finger scroll pans; pinch or Ctrl+scroll zooms.",
        ScrollMode::Zoom => "Every scroll zooms (sideways scroll pans).",
        ScrollMode::Pan => "Every scroll pans; pinch or Ctrl+scroll zooms.",
    });
    ui.horizontal(|ui| {
        ui.label("On-screen controls");
        for m in PadMode::ALL {
            if ui.selectable_label(s.pad == m, m.label()).clicked() {
                s.pad = m;
            }
        }
    });
    if s.pad == PadMode::Auto {
        hint(ui, "Auto: shown until a mouse wheel is used.");
    }
    ui.add(egui::Slider::new(&mut s.pan_speed, 0.25..=4.0).text("touchpad pan speed").fixed_decimals(2));
    ui.add(egui::Slider::new(&mut s.zoom_speed, 0.25..=4.0).text("zoom speed").fixed_decimals(2));
    ui.add(egui::Slider::new(&mut s.turn_speed, 0.25..=4.0).text("drag turn speed").fixed_decimals(2));
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut s.invert_zoom, "Invert zoom");
        ui.checkbox(&mut s.invert_scroll_pan, "Invert scroll pan");
        ui.checkbox(&mut s.invert_tilt, "Invert drag tilt");
    });
    ui.checkbox(&mut s.left_drag_pan, "Left-drag pans the map (Inspect tool)");
    ui.checkbox(&mut s.drag_look, "Walk mode: drag to look (no mouselook)");
    let raw = if n.raw_reliable { "raw mouse motion in use" } else { "raw mouse motion unusable here (remote desktop?): drags follow the pointer, Walk looks by dragging" };
    let last = n.last_scroll.map_or("no scroll yet".to_string(), |k| format!("last scroll: {}", k.label()));
    hint(ui, &format!("{last} · {raw}"));
    if *s != before {
        super::theme::save_nav(s);
    }
}
