//! [camera] Big map pins over a selected person's places: home, work and
//! every business they own (a founder of several firms gets a pin on each).
//! A pin is a click target: it selects that building. The posts under the
//! pins are gizmos (`crate::marks`).

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use nd_sim::Holding;

use crate::camera::{CameraMode, OrbitCamera};
use crate::marks::{holding_rgb, pin_top};
use crate::sim::Sim;
use crate::tools::Selection;

pub fn map_pins(mut contexts: EguiContexts, sim: Res<Sim>, mut selection: ResMut<Selection>, mode: Res<CameraMode>, cams: Query<(&Camera, &GlobalTransform), With<OrbitCamera>>, mut flags: ResMut<super::UiFlags>) -> Result {
    let Selection::Citizen(id) = *selection else { return Ok(()) };
    if *mode == CameraMode::Walk {
        return Ok(());
    }
    let Some((camera, gt)) = cams.iter().next() else { return Ok(()) };
    let w = &sim.world;
    let holdings = w.holdings_of(id);
    if holdings.is_empty() {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    // [city-data] The map is covered: nothing to label.
    if crate::ui::city_data::is_open(ctx) {
        return Ok(());
    }
    let screen = ctx.content_rect();
    let mut pick = None;
    for (b, h) in holdings {
        let Some(top) = pin_top(w, b) else { continue };
        let Ok(at) = camera.world_to_viewport(gt, top) else { continue };
        // Off the screen: the pin waits at the edge, pointing the way, so
        // home and work are always one click away.
        let inner = egui::Rect::from_min_max(screen.min + egui::vec2(30.0, 110.0), screen.max - egui::vec2(30.0, 190.0));
        let raw = crate::ui::theme::to_points(ctx, at);
        let tip = inner.clamp(raw);
        let edge = tip != raw;
        let [r, g, bl] = holding_rgb(h);
        let col = egui::Color32::from_rgb(r, g, bl);
        let col = if edge { col.gamma_multiply(0.75) } else { col };
        let name = w.buildings.get(b).map_or("", |x| x.name.as_str());
        let word = match h {
            Holding::Home => "HOME",
            Holding::Work => "WORK",
            Holding::Owns => "OWNS",
        };
        // The pin's head sits above its point, which marks the post's top.
        let size = egui::vec2(34.0, 46.0);
        egui::Area::new(egui::Id::new(("holding_pin", b)))
            // Under every window (they are on the middle layer); the map's
            // click test learns the pin's rectangle below, as for the bars.
            .order(egui::Order::Background)
            .fixed_pos(tip)
            .pivot(egui::Align2::CENTER_BOTTOM)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
                flags.panel_rects.push(rect);
                let p = ui.painter();
                let head = egui::pos2(rect.center().x, rect.top() + 17.0);
                let hot = resp.hovered();
                let rim = egui::Stroke::new(if hot { 2.5 } else { 1.5 }, if hot { egui::Color32::WHITE } else { egui::Color32::from_black_alpha(230) });
                p.add(egui::Shape::convex_polygon(vec![head + egui::vec2(-9.0, 8.0), rect.center_bottom(), head + egui::vec2(9.0, 8.0)], col, rim));
                p.circle(head, 16.0, col, rim);
                glyph(p, head, h);
                if hot {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if resp.on_hover_text(format!("{} {name}: click to select it", h.label())).clicked() {
                    pick = Some(b);
                }
            });
        // The word and the name beside the head, outlined for any background.
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new(("holding_pin_label", b))));
        // Text to the side with room: left of a pin near the right edge.
        let left = tip.x > screen.right() - 240.0;
        let (at, align) = if left { (tip + egui::vec2(-20.0, -38.0), egui::Align2::RIGHT_CENTER) } else { (tip + egui::vec2(20.0, -38.0), egui::Align2::LEFT_CENTER) };
        let text = if edge { format!("{word}  {name}  (off screen)") } else { format!("{word}  {name}") };
        for off in [egui::vec2(1.0, 1.0), egui::vec2(-1.0, 1.0), egui::vec2(1.0, -1.0), egui::vec2(-1.0, -1.0)] {
            painter.text(at + off, align, &text, crate::ui::theme::small(), egui::Color32::BLACK);
        }
        painter.text(at, align, &text, crate::ui::theme::small(), col);
    }
    if let Some(b) = pick {
        *selection = Selection::Building(b);
    }
    Ok(())
}

/// A house, a briefcase or a key drawn in the pin's head.
fn glyph(p: &egui::Painter, c: egui::Pos2, h: Holding) {
    let ink = egui::Color32::from_rgb(12, 14, 22);
    let s = egui::Stroke::new(2.0, ink);
    match h {
        Holding::Home => {
            p.add(egui::Shape::convex_polygon(vec![c + egui::vec2(-9.0, -1.0), c + egui::vec2(0.0, -9.0), c + egui::vec2(9.0, -1.0)], ink, egui::Stroke::NONE));
            p.rect_filled(egui::Rect::from_min_max(c + egui::vec2(-6.5, -1.5), c + egui::vec2(6.5, 8.0)), 0.0, ink);
        }
        Holding::Work => {
            p.rect_filled(egui::Rect::from_min_max(c + egui::vec2(-9.0, -4.0), c + egui::vec2(9.0, 7.0)), 2.0, ink);
            p.rect_stroke(egui::Rect::from_min_max(c + egui::vec2(-4.0, -8.0), c + egui::vec2(4.0, -3.0)), 1.0, s, egui::StrokeKind::Middle);
        }
        Holding::Owns => {
            p.circle_stroke(c + egui::vec2(-4.0, -3.0), 4.5, s);
            p.line_segment([c + egui::vec2(-1.0, 0.0), c + egui::vec2(8.0, 8.0)], s);
            p.line_segment([c + egui::vec2(5.0, 5.5), c + egui::vec2(2.5, 8.0)], s);
        }
    }
}
