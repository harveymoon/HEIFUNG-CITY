//! The bottom toolbar: a sub-pane with only the active tool's parameters and
//! status, then one large button per tool with its hotkey, and the project
//! controls beside them: a Projects dropdown to switch between pending
//! projects, start, rename or park one, and implement / undo / redo /
//! discard for the active one.

use bevy_egui::egui;

use super::{build, help, hint, icons, section, UiCtx, GOLD, IVORY, WARN};
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
        // Inspect, the Build button, then the other quick tools (Edit,
        // Bulldoze, ...): everything that builds is picked in the Build
        // window.
        let quick: Vec<Tool> = Tool::ALL.into_iter().filter(|t| !build::is_build_tool(*t)).collect();
        for (i, tool) in quick.into_iter().enumerate() {
            if tool_button(ui, tool, p.tools.tool == tool).clicked() {
                pick(p, tool);
            }
            if i == 0 {
                let open = build::is_open(ui.ctx());
                let building = build::is_build_tool(p.tools.tool);
                if build_button(ui, open, building).clicked() {
                    build::set_open(ui.ctx(), !open);
                }
            }
        }
        ui.add_space(12.0);
        project(ui, p);
    });
}

fn pick(p: &mut UiCtx, tool: Tool) {
    if p.tools.tool != tool {
        p.tools.tool = tool;
        p.tools.points.clear();
        p.tools.drag = None;
    }
}

/// The Build button: opens and closes the Build window; lit while a
/// building tool is active, outlined while the window is open.
fn build_button(ui: &mut egui::Ui, open: bool, building: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(BUTTON, egui::Sense::click());
    let fill = if building {
        egui::Color32::from_rgb(150, 105, 40)
    } else if resp.hovered() {
        egui::Color32::from_rgb(70, 55, 35)
    } else {
        egui::Color32::from_rgba_unmultiplied(30, 32, 46, 230)
    };
    let stroke = if open || building { GOLD } else { egui::Color32::from_rgb(60, 60, 72) };
    let text = if building { egui::Color32::WHITE } else { IVORY };
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, fill);
    painter.rect_stroke(rect, 4.0, egui::Stroke::new(if open { 2.0 } else { 1.0 }, stroke), egui::StrokeKind::Inside);
    let icon = egui::Rect::from_center_size(egui::pos2(rect.left() + 8.0 + ICON * 0.5, rect.center().y), egui::vec2(ICON, ICON));
    icons::tool(painter, icon, Tool::Buildings, if building { egui::Color32::WHITE } else { GOLD });
    let mid = egui::pos2((icon.right() + rect.right()) * 0.5, rect.center().y);
    painter.text(mid - egui::vec2(0.0, 6.0), egui::Align2::CENTER_CENTER, "Build", egui::FontId::proportional(15.0), text);
    painter.text(mid + egui::vec2(0.0, 11.0), egui::Align2::CENTER_CENTER, "R Z N T Y", egui::FontId::proportional(10.0), egui::Color32::from_rgb(170, 160, 140));
    resp.on_hover_text("Everything you can build: roads, zones, parks, districts, power, services, transit, rail, water, attractions. Their keys (2 R road, 3 Z zone, 6 N buildings, 7 T transit, 8 Y rail, 9 districts) open it at their section.")
}

/// The active tool's parameters in a framed strip above the buttons. While
/// the Build window is open a building tool's options are there, and the
/// strip only says what is being built and how it went.
fn sub_pane(ui: &mut egui::Ui, p: &mut UiCtx) {
    let tool = p.tools.tool;
    let in_window = build::is_build_tool(tool) && build::is_open(ui.ctx());
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
                if in_window {
                    if let Some(item) = build::current_item(p.tools) {
                        ui.label(egui::RichText::new(item.label()).color(IVORY).strong());
                    }
                    hint(ui, "· options in the Build window");
                }
            });
            if !in_window {
                super::tools::sub_pane(ui, p.sim, p.tools, p.plan, p.assets);
            }
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

/// The Projects dropdown (switch, new, rename, park, show the others dim)
/// and implement / undo / redo / discard for the active project.
fn project(ui: &mut egui::Ui, p: &mut UiCtx) {
    let sim = &mut *p.sim;
    let tools = &mut *p.tools;
    let projects = sim.world.projects();
    let active = projects.iter().find(|i| i.active).cloned();
    let mut switch_to: Option<u64> = None;
    let mut new = false;
    let mut park = false;
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            section(ui, "PROJECT");
            ui.checkbox(&mut p.overlays.projects, egui::RichText::new("show others").small()).on_hover_text(help::PROJECTS);
        });
        ui.horizontal(|ui| {
            let label = active.as_ref().map_or_else(|| "(none active)".to_string(), |i| i.name.clone());
            egui::ComboBox::from_id_salt("projects").selected_text(label).width(130.0).show_ui(ui, |ui| {
                for i in &projects {
                    let warn = !i.conflicts.is_empty() || i.shared_parcels > 0;
                    let text = format!("{} · {} step{}{}", i.name, i.steps, if i.steps == 1 { "" } else { "s" }, if warn { "  ⚠ overlaps" } else { "" });
                    if ui.selectable_label(i.active, text).clicked() && !i.active {
                        switch_to = Some(i.id);
                    }
                }
                if !projects.is_empty() {
                    ui.separator();
                }
                if ui.selectable_label(false, "+ New project").clicked() {
                    new = true;
                }
                if active.is_some() && ui.selectable_label(false, "Park (edit none)").on_hover_text("Keep it pending, drawn dim, with nothing active").clicked() {
                    park = true;
                }
            });
            match &active {
                Some(i) => {
                    let txt = format!("{} step{}{}", i.steps, if i.steps == 1 { "" } else { "s" }, if i.redo > 0 { format!(", {} redoable", i.redo) } else { String::new() });
                    ui.label(egui::RichText::new(txt).small());
                }
                None => hint(ui, help::PROJECT_NONE),
            }
        });
        // Rename in place: a field while renaming, a small button otherwise.
        if let Some(i) = &active {
            match tools.rename.as_mut() {
                Some((id, name)) if *id == i.id => {
                    let r = ui.add(egui::TextEdit::singleline(name).desired_width(150.0).hint_text("project name"));
                    if r.lost_focus() {
                        let (id, name) = tools.rename.take().unwrap();
                        sim.world.rename_project(id, &name);
                    } else if !r.has_focus() {
                        r.request_focus();
                    }
                }
                _ => {
                    ui.horizontal(|ui| {
                        if ui.small_button("rename").clicked() {
                            tools.rename = Some((i.id, i.name.clone()));
                        }
                        if !i.conflicts.is_empty() || i.shared_parcels > 0 {
                            let n = i.conflicts.len();
                            ui.label(egui::RichText::new(format!("overlaps another project ({n} gesture{}, {} parcel{})", if n == 1 { "" } else { "s" }, i.shared_parcels, if i.shared_parcels == 1 { "" } else { "s" })).small().color(WARN))
                                .on_hover_text(help::PROJECTS);
                        }
                    });
                }
            }
        }
    });
    if let Some(id) = switch_to {
        sim.world.switch_project(id);
        tools.points.clear();
        tools.stroke = None;
        tools.status = format!("Editing {}.", sim.world.project.as_ref().map_or("", |p| p.name.as_str()));
    }
    if new {
        let id = sim.world.new_project(None);
        tools.points.clear();
        tools.stroke = None;
        tools.rename = Some((id, sim.world.project.as_ref().map_or(String::new(), |p| p.name.clone())));
        tools.status = "New project: draw, paint or bulldoze into it.".into();
    }
    if park {
        sim.world.park_project();
        tools.points.clear();
        tools.status = "Project parked: it stays pending, drawn dim.".into();
    }
    let Some(i) = active else { return };
    // Two rows of buttons, so the bar stays narrow beside the tool buttons.
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(egui::RichText::new("Implement").strong()).min_size(egui::vec2(78.0, 0.0))).on_hover_text("Ctrl+Enter").clicked() {
                let roads = sim.world.preview().planned_roads.len();
                let warnings = sim.world.implement_project(i.id).unwrap_or_default();
                tools.points.clear();
                // [tools] A refusal is not a warning: nothing was built.
                tools.status = match sim.world.implement_refused.clone() {
                    Some(why) => crate::tools::refused_text(&i.name, roads, &why),
                    None if warnings.is_empty() => format!("{} implemented.", i.name),
                    None => format!("{} implemented. {}", i.name, warnings.join(" ")),
                };
            }
            if ui.add(egui::Button::new("Discard").min_size(egui::vec2(60.0, 0.0))).on_hover_text("Throw the active project away").clicked() {
                sim.world.discard_project();
                tools.points.clear();
                tools.status = format!("{} discarded.", i.name);
            }
        });
        // [tools] What implementing would refuse on, before it is tried
        // (the plan view marks the spot red with a pin).
        if let Some(check) = tools.plan_check.as_ref() {
            if let Some(why) = &check.pillar_refusal {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("Will be refused: {}", why.trim_start_matches("Not implemented: "))).small().color(WARN));
                    if let Some(g) = check.pillar_gaps.first() {
                        if ui.small_button("show").on_hover_text("Fly to the red pin").clicked() {
                            let s = crate::render::gap_spot(g);
                            p.fly_to = Some(bevy::math::Vec2::new(s.x, s.y));
                        }
                    }
                });
            }
        }
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new("Undo").min_size(egui::vec2(78.0, 0.0))).on_hover_text("Ctrl+Z").clicked() {
                sim.world.undo();
            }
            if ui.add(egui::Button::new("Redo").min_size(egui::vec2(60.0, 0.0))).on_hover_text("Ctrl+Y").clicked() {
                sim.world.redo();
            }
        });
    });
}
