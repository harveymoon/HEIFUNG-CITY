//! The bottom toolbar: a sub-pane with only the active tool's parameters and
//! status, then one large button per tool with its hotkey, and the project
//! controls beside them while a project is pending.

use bevy_egui::egui;

use super::{help, hint, icons, section, UiCtx, GOLD, IVORY};
use crate::tools::Tool;

/// Width and height of a tool button; big enough for a mouse at 2560 wide.
const BUTTON: egui::Vec2 = egui::vec2(116.0, 44.0);
/// Side of the icon at the left of a tool button.
const ICON: f32 = 26.0;
/// The sub-pane never grows past this, so the hint text wraps into a
/// readable column rather than one line across a wide monitor.
const SUB_PANE_MAX_WIDTH: f32 = 900.0;

pub fn show(ui: &mut egui::Ui, p: &mut UiCtx) {
    sub_pane(ui, p);
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for tool in Tool::ALL {
            if tool_button(ui, tool, p.tools.tool == tool).clicked() && p.tools.tool != tool {
                p.tools.tool = tool;
                p.tools.points.clear();
                p.tools.drag = None;
            }
        }
        ui.add_space(12.0);
        project(ui, p);
    });
}

/// The active tool's parameters in a framed strip above the buttons.
fn sub_pane(ui: &mut egui::Ui, p: &mut UiCtx) {
    let tool = p.tools.tool;
    egui::Frame::new()
        .fill(egui::Color32::from_rgba_unmultiplied(16, 18, 30, 230))
        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(70, 58, 40)))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_max_width(SUB_PANE_MAX_WIDTH.min(ui.available_width()));
            ui.horizontal(|ui| {
                section(ui, &tool.label().to_uppercase());
                hint(ui, tool.hotkey());
            });
            super::tools::sub_pane(ui, p.sim, p.tools, p.plan, p.assets);
            if !p.tools.status.is_empty() {
                ui.label(egui::RichText::new(&p.tools.status).small().color(IVORY));
            }
        });
}

/// A painted button: the tool's icon, then its name over its hotkey. egui's
/// button lays its atoms out in a row, so the two sizes are drawn by hand.
fn tool_button(ui: &mut egui::Ui, tool: Tool, active: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(BUTTON, egui::Sense::click());
    let fill = if active {
        egui::Color32::from_rgb(150, 105, 40)
    } else if resp.hovered() {
        egui::Color32::from_rgb(70, 55, 35)
    } else {
        egui::Color32::from_rgba_unmultiplied(30, 32, 46, 230)
    };
    let stroke = if active { GOLD } else { egui::Color32::from_rgb(60, 60, 72) };
    let text = if active { egui::Color32::WHITE } else { IVORY };
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, fill);
    painter.rect_stroke(rect, 4.0, egui::Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
    let icon = egui::Rect::from_center_size(egui::pos2(rect.left() + 8.0 + ICON * 0.5, rect.center().y), egui::vec2(ICON, ICON));
    icons::tool(painter, icon, tool, if active { egui::Color32::WHITE } else { GOLD });
    let mid = egui::pos2((icon.right() + rect.right()) * 0.5, rect.center().y);
    painter.text(mid - egui::vec2(0.0, 6.0), egui::Align2::CENTER_CENTER, tool.label(), egui::FontId::proportional(15.0), text);
    painter.text(mid + egui::vec2(0.0, 11.0), egui::Align2::CENTER_CENTER, tool.hotkey(), egui::FontId::proportional(10.0), egui::Color32::from_rgb(170, 160, 140));
    resp.on_hover_text(help::tool_hint(tool))
}

/// Implement / undo / redo / discard with the pending count, or a hint on
/// how a project starts when there is none.
fn project(ui: &mut egui::Ui, p: &mut UiCtx) {
    let sim = &mut *p.sim;
    let tools = &mut *p.tools;
    let Some(proj) = &sim.world.project else {
        hint(ui, help::PROJECT_NONE);
        return;
    };
    let steps = proj.steps.len();
    let redo = proj.redo.len();
    ui.vertical(|ui| {
        section(ui, "PROJECT");
        ui.label(egui::RichText::new(format!("{steps} pending step{}{}", if steps == 1 { "" } else { "s" }, if redo > 0 { format!(", {redo} redoable") } else { String::new() })).small());
    });
    if ui.add(egui::Button::new(egui::RichText::new("Implement").strong()).min_size(egui::vec2(0.0, BUTTON.y))).on_hover_text("Ctrl+Enter").clicked() {
        sim.world.implement();
        tools.points.clear();
        tools.status = "Project implemented".into();
    }
    if ui.add(egui::Button::new("Undo").min_size(egui::vec2(0.0, BUTTON.y))).on_hover_text("Ctrl+Z").clicked() {
        sim.world.undo();
    }
    if ui.add(egui::Button::new("Redo").min_size(egui::vec2(0.0, BUTTON.y))).on_hover_text("Ctrl+Y").clicked() {
        sim.world.redo();
    }
    if ui.add(egui::Button::new("Discard").min_size(egui::vec2(0.0, BUTTON.y))).clicked() {
        sim.world.discard_project();
        tools.points.clear();
    }
}
