//! The title screen: the first thing at startup (unless `ND_TITLE=0`). The
//! saved games newest first, each a card with its map picture, name, day
//! and population; Continue loads the newest, New game opens the landscape
//! picker, "Play this map" starts on the fresh landscape already loaded.
//! The city behind stays paused while it is up. The Saves window's "title
//! screen" button and the remote API (`window {name: "title"}`) bring it
//! back.

use std::path::PathBuf;

use bevy_egui::egui::{self, Color32, Rect};

use super::saves::{age, SlotKind};
use super::{hint, section, UiCtx, GOLD, IVORY};

const CARD: egui::Vec2 = egui::vec2(208.0, 262.0);
const PICTURE: f32 = 192.0;

pub fn screen(ctx: &egui::Context, p: &mut UiCtx) {
    if !p.saves.title {
        return;
    }
    p.sim.paused = true;
    let view = ctx.viewport_rect();
    let mut load: Option<PathBuf> = None;
    let mut close = false;
    let mut new_game = false;
    egui::Area::new(egui::Id::new("title_screen")).order(egui::Order::Foreground).fixed_pos(view.min).show(ctx, |ui| {
        ui.set_min_size(view.size());
        ui.painter().rect_filled(view, 0.0, Color32::from_rgba_unmultiplied(4, 5, 12, 236));
        // Swallow clicks on the backdrop so the map under it stays put.
        ui.allocate_rect(view, egui::Sense::click_and_drag());
        let width = (view.width() - 80.0).clamp(480.0, 1100.0);
        let column = Rect::from_min_size(egui::pos2(view.center().x - width * 0.5, view.top() + 40.0), egui::vec2(width, view.height() - 60.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(column), |ui| {
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("DISTRICT 2040").color(GOLD).strong().size(42.0));
                ui.label(egui::RichText::new("a neon city builder").color(IVORY).italics().size(15.0));
            });
            ui.add_space(18.0);
            let slots = p.saves.slots();
            let newest = slots.iter().find(|s| s.kind != SlotKind::Dated).map(|s| (s.path.clone(), s.label.clone()));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let big = |t: &str| egui::Button::new(egui::RichText::new(t).strong().size(16.0)).min_size(egui::vec2(170.0, 36.0));
                if let Some((path, label)) = &newest {
                    if ui.add(big("Continue")).on_hover_text(format!("Load {label}")).clicked() {
                        load = Some(path.clone());
                    }
                }
                if ui.add(big("New game…")).on_hover_text("Pick a landscape and start a new city").clicked() {
                    new_game = true;
                }
                if ui.add(big("Play this map")).on_hover_text("Start building on the landscape that is loaded (a river valley with the highway)").clicked() {
                    close = true;
                }
            });
            if let Some((_, label)) = &newest {
                hint(ui, &format!("Continue loads {label}."));
            }
            ui.add_space(14.0);
            section(ui, "SAVED GAMES");
            let rows: Vec<(PathBuf, String, SlotKind, String, Option<std::time::SystemTime>, Option<PathBuf>)> =
                p.saves.slots().iter().filter(|s| s.kind != SlotKind::Dated).map(|s| (s.path.clone(), s.label.clone(), s.kind, s.figures(), s.modified, s.picture.clone())).collect();
            if rows.is_empty() {
                hint(ui, "No saved games yet: start one, then Saves, Save as, to keep it under a name.");
            }
            egui::ScrollArea::vertical().id_salt("title_saves").auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(12.0, 12.0);
                    for (path, label, kind, figures, modified, picture) in rows {
                        let pic = picture.as_deref().and_then(|x| p.saves.picture(ui.ctx(), x));
                        if card(ui, &label, kind, &figures, &age(modified), pic.as_ref()).clicked() {
                            load = Some(path);
                        }
                    }
                });
            });
        });
    });
    if let Some(path) = load {
        super::saves::load_now(&path, p.sim, p.tools, p.selection, p.plan, p.flags);
        p.saves.title = false;
    }
    if new_game {
        super::newgame::set_open(ctx, true);
        p.saves.title = false;
        p.sim.paused = false;
    }
    if close {
        p.saves.title = false;
        p.sim.paused = false;
    }
}

/// One saved game: its picture, name and figures; the whole card clicks.
fn card(ui: &mut egui::Ui, label: &str, kind: SlotKind, figures: &str, age: &str, pic: Option<&egui::TextureHandle>) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(CARD, egui::Sense::click());
    let painter = ui.painter_at(rect);
    let hot = resp.hovered();
    painter.rect_filled(rect, 6.0, if hot { Color32::from_rgb(40, 34, 28) } else { Color32::from_rgb(16, 18, 30) });
    painter.rect_stroke(rect, 6.0, egui::Stroke::new(if hot { 2.0 } else { 1.0 }, if hot { GOLD } else { Color32::from_rgb(60, 60, 72) }), egui::StrokeKind::Inside);
    let img = Rect::from_min_size(rect.min + egui::vec2((CARD.x - PICTURE) * 0.5, 8.0), egui::vec2(PICTURE, PICTURE));
    match pic {
        Some(tex) => {
            painter.image(tex.id(), img, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            painter.rect_filled(img, 4.0, Color32::from_rgb(10, 12, 22));
            painter.text(img.center(), egui::Align2::CENTER_CENTER, "no picture", egui::FontId::proportional(12.0), Color32::from_gray(110));
        }
    }
    painter.rect_stroke(img, 4.0, egui::Stroke::new(1.0, Color32::from_rgb(50, 50, 60)), egui::StrokeKind::Inside);
    let mut y = img.bottom() + 6.0;
    let name_col = if kind == SlotKind::Named { GOLD } else { IVORY };
    painter.text(egui::pos2(rect.left() + 10.0, y), egui::Align2::LEFT_TOP, label, egui::FontId::proportional(15.0), name_col);
    y += 20.0;
    let line = if figures.is_empty() { age.to_string() } else { figures.to_string() };
    painter.text(egui::pos2(rect.left() + 10.0, y), egui::Align2::LEFT_TOP, line, egui::FontId::proportional(11.0), Color32::from_rgb(190, 185, 170));
    y += 15.0;
    if !figures.is_empty() {
        painter.text(egui::pos2(rect.left() + 10.0, y), egui::Align2::LEFT_TOP, age, egui::FontId::proportional(10.0), Color32::from_gray(130));
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
