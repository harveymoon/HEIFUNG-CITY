//! The title screen: the first thing at startup (unless `ND_TITLE=0`).
//!
//! [title] Two panes under the logo and the buttons. Left, the cities: one
//! card per game (grouped by game id, so two games of one name stay apart;
//! by city name for saves from before ids), the game played last first and
//! largest. Right, the picked game's save states newest first: the latest
//! quick save, the autosave, the dated quick saves and the history
//! autosaves, each with its picture, day, population and when it was saved.
//! Click one, then Load (or double-click it). Continue still loads the
//! newest save; New game opens the step-by-step new map wizard; "Play this
//! map" starts on the fresh landscape already loaded under the name typed
//! beside it.
//! The city behind stays paused while it is up. The Saves window's "title
//! screen" button and the remote API (`window {name: "title"}`) bring it
//! back.

use std::path::PathBuf;

use bevy_egui::egui::{self, Color32, Rect};

use super::saves::{age, Game, SaveState, SlotKind, StateKind};
use super::{hint, section, UiCtx, GOLD, IVORY};

/// The newest game's card: picture side.
const BIG_PICTURE: f32 = 240.0;
/// Every other game's card: picture side.
const SMALL_PICTURE: f32 = 116.0;
/// A save state's picture in the right pane.
const ROW_PICTURE: f32 = 64.0;

/// [title] What is picked: the game (its key) and the save state (path).
#[derive(Clone, Default)]
struct Pick {
    game: Option<String>,
    state: Option<PathBuf>,
}

fn pick_id() -> egui::Id {
    egui::Id::new("title_pick")
}

pub fn screen(ctx: &egui::Context, p: &mut UiCtx) {
    if !p.saves.title {
        return;
    }
    p.sim.paused = true;
    if p.saves.new_city_name.is_empty() {
        p.saves.new_city_name = super::newgame::suggest_name();
    }
    let view = ctx.viewport_rect();
    let mut load: Option<PathBuf> = None;
    let mut close = false;
    let mut new_game = false;
    let games = p.saves.games();
    let mut pick: Pick = ctx.data(|d| d.get_temp(pick_id())).unwrap_or_default();
    // The game shown: the one picked if it is still there, else the newest.
    if !pick.game.as_ref().is_some_and(|k| games.iter().any(|g| &g.key == k)) {
        pick = Pick { game: games.first().map(|g| g.key.clone()), state: None };
    }
    egui::Area::new(egui::Id::new("title_screen")).order(egui::Order::Foreground).fixed_pos(view.min).show(ctx, |ui| {
        ui.set_min_size(view.size());
        ui.painter().rect_filled(view, 0.0, Color32::from_rgba_unmultiplied(4, 5, 12, 236));
        // Swallow clicks on the backdrop so the map under it stays put.
        ui.allocate_rect(view, egui::Sense::click_and_drag());
        let width = (view.width() - 60.0).clamp(560.0, 1400.0);
        let column = Rect::from_min_size(egui::pos2(view.center().x - width * 0.5, view.top() + 24.0), egui::vec2(width, view.height() - 40.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(column), |ui| {
            ui.vertical_centered(|ui| {
                // The logo (assets/brand/logo.svg, rendered by make_logo.py).
                let w = (width * 0.42).min(520.0);
                ui.add(egui::Image::new(&logo_texture(ui.ctx())).fit_to_exact_size(egui::vec2(w, w * 0.35)));
                ui.label(egui::RichText::new("a neon city builder").color(IVORY).italics().size(crate::ui::theme::BODY));
            });
            ui.add_space(12.0);
            let slots = p.saves.slots();
            let newest = slots.iter().find(|s| s.kind != SlotKind::Dated).map(|s| (s.path.clone(), s.label.clone()));
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                let big = |t: &str| egui::Button::new(egui::RichText::new(t).strong().font(crate::ui::theme::lead())).min_size(egui::vec2(170.0, 36.0));
                if let Some((path, label)) = &newest {
                    if ui.add(big("Continue")).on_hover_text(format!("Load {label}, the newest save")).clicked() {
                        load = Some(path.clone());
                    }
                }
                if ui.add(big("New game…")).on_hover_text("Pick a landscape, a starting city and a name, step by step").clicked() {
                    new_game = true;
                }
                if ui.add(big("Play this map")).on_hover_text("Start building on the landscape that is loaded (a river valley with the highway), under the name beside it").clicked() {
                    close = true;
                }
                ui.add(egui::TextEdit::singleline(&mut p.saves.new_city_name).desired_width(170.0).hint_text("city name").font(egui::TextStyle::Heading));
                if ui.small_button("🎲").on_hover_text("another name").clicked() {
                    p.saves.new_city_name = super::newgame::suggest_name();
                }
            });
            if let Some((_, label)) = &newest {
                hint(ui, &format!("Continue loads {label}."));
            }
            if p.flags.message.starts_with("load failed") {
                ui.label(egui::RichText::new(&p.flags.message).color(crate::ui::WARN));
            }
            ui.add_space(10.0);
            if games.is_empty() {
                section(ui, "SAVED GAMES");
                hint(ui, "No saved games yet: start one, then Saves, Save as, to keep it under a name.");
                return;
            }
            // [title] The two panes: the cities, and the picked one's saves.
            let rest = ui.available_rect_before_wrap();
            let left_w = (rest.width() * 0.5).max(300.0);
            let left = Rect::from_min_size(rest.min, egui::vec2(left_w - 8.0, rest.height()));
            let right = Rect::from_min_max(egui::pos2(rest.min.x + left_w + 8.0, rest.min.y), rest.max);
            ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
                section(ui, "CITIES");
                egui::ScrollArea::vertical().id_salt("title_games").auto_shrink([false, false]).show(ui, |ui| {
                    if let Some(first) = games.first() {
                        let pic = picture_of(p, ui.ctx(), first);
                        if game_card(ui, first, BIG_PICTURE, left.width() - 16.0, pick.game.as_ref() == Some(&first.key), pic.as_ref()).clicked() {
                            pick = Pick { game: Some(first.key.clone()), state: None };
                        }
                    }
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
                        for g in games.iter().skip(1) {
                            let pic = picture_of(p, ui.ctx(), g);
                            if game_card(ui, g, SMALL_PICTURE, SMALL_PICTURE + 16.0, pick.game.as_ref() == Some(&g.key), pic.as_ref()).clicked() {
                                pick = Pick { game: Some(g.key.clone()), state: None };
                            }
                        }
                    });
                });
            });
            ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
                let Some(game) = games.iter().find(|g| Some(&g.key) == pick.game.as_ref()) else { return };
                if !pick.state.as_ref().is_some_and(|s| game.states.iter().any(|x| &x.path == s)) {
                    pick.state = game.newest().map(|s| s.path.clone());
                }
                section(ui, "SAVES");
                ui.label(egui::RichText::new(&game.city).color(GOLD).strong().font(crate::ui::theme::lead()));
                hint(ui, &format!("{} save{} of this game, newest first. Pick one, then Load (or double-click it).", game.states.len(), super::fmt::s(game.states.len())));
                let chosen = game.states.iter().find(|s| Some(&s.path) == pick.state.as_ref());
                let label = chosen.map_or_else(|| "Load".to_string(), |s| format!("Load {}", s.label));
                let button = egui::Button::new(egui::RichText::new(label).strong().font(crate::ui::theme::lead())).min_size(egui::vec2(170.0, 32.0));
                if ui.add_enabled(chosen.is_some(), button).clicked() {
                    load = chosen.map(|s| s.path.clone());
                }
                ui.add_space(6.0);
                egui::ScrollArea::vertical().id_salt("title_states").auto_shrink([false, false]).show(ui, |ui| {
                    for st in &game.states {
                        let pic = p.saves.picture(ui.ctx(), &st.picture);
                        let r = state_row(ui, st, pick.state.as_ref() == Some(&st.path), pic.as_ref());
                        if r.clicked() {
                            pick.state = Some(st.path.clone());
                        }
                        if r.double_clicked() {
                            load = Some(st.path.clone());
                        }
                    }
                });
            });
        });
    });
    ctx.data_mut(|d| d.insert_temp(pick_id(), pick));
    if let Some(path) = load {
        // [fix/review-app] A failed load keeps the title screen up (its
        // message says why).
        if super::saves::load_now(&path, p.sim, p.flags).is_ok() {
            p.saves.title = false;
        }
    }
    if new_game {
        super::newgame::set_open(ctx, true);
        p.saves.title = false;
        p.sim.paused = false;
    }
    if close {
        p.saves.title = false;
        p.sim.paused = false;
        // [identity] The map loaded at startup is a new game: it takes the
        // name typed beside the button.
        if p.sim.world.clock.tick == 0 && p.sim.world.identity.name.is_empty() {
            let name = std::mem::take(&mut p.saves.new_city_name);
            p.sim.set_city_name(&name);
        }
    }
}

/// [title] A game's picture: the newest of its saves that has one.
fn picture_of(p: &mut UiCtx, ctx: &egui::Context, g: &Game) -> Option<egui::TextureHandle> {
    g.states.iter().take(6).find_map(|s| p.saves.picture(ctx, &s.picture))
}

/// [title] One game: its picture, the city's name, the newest save's
/// figures and age, how many saves. The whole card clicks. The big card
/// has its text beside the picture, the small ones under it.
fn game_card(ui: &mut egui::Ui, g: &Game, side: f32, width: f32, picked: bool, pic: Option<&egui::TextureHandle>) -> egui::Response {
    let big = side >= BIG_PICTURE;
    let size = if big { egui::vec2(width.max(side + 16.0), side + 16.0) } else { egui::vec2(width, side + 72.0) };
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let painter = ui.painter_at(rect);
    let hot = resp.hovered();
    painter.rect_filled(rect, 6.0, if picked { Color32::from_rgb(44, 36, 26) } else if hot { Color32::from_rgb(32, 30, 34) } else { Color32::from_rgb(16, 18, 30) });
    painter.rect_stroke(rect, 6.0, egui::Stroke::new(if hot || picked { 2.0 } else { 1.0 }, if hot || picked { GOLD } else { super::theme::EDGE_IDLE }), egui::StrokeKind::Inside);
    let img = Rect::from_min_size(rect.min + egui::vec2(8.0, 8.0), egui::vec2(side, side));
    match pic {
        Some(tex) => {
            painter.image(tex.id(), img, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            painter.rect_filled(img, 4.0, super::theme::NO_PICTURE);
            painter.text(img.center(), egui::Align2::CENTER_CENTER, "no picture", crate::ui::theme::small(), Color32::from_gray(110));
        }
    }
    painter.rect_stroke(img, 4.0, egui::Stroke::new(1.0, Color32::from_rgb(50, 50, 60)), egui::StrokeKind::Inside);
    let newest = g.newest();
    let when = newest.map(|s| age(s.modified)).unwrap_or_default();
    let count = format!("{} save{}", g.states.len(), super::fmt::s(g.states.len()));
    let (x, mut y) = if big { (img.right() + 14.0, img.top() + 4.0) } else { (rect.left() + 8.0, img.bottom() + 6.0) };
    let w = (rect.right() - x - 6.0).max(40.0);
    painter.text(egui::pos2(x, y), egui::Align2::LEFT_TOP, super::fmt::elide(&g.city, ((w / 9.0) as usize).max(8)), crate::ui::theme::lead(), GOLD);
    y += if big { 30.0 } else { 22.0 };
    let meta = newest.and_then(|s| s.meta.as_ref());
    let lines: Vec<String> = if big {
        let mut v: Vec<String> = meta.map_or_else(Vec::new, |m| vec![format!("Day {}", m.day), format!("{} people", m.population), format!("$ {:.0}", m.treasury), m.landscape.clone()]);
        v.retain(|l| !l.is_empty());
        v.push(format!("saved {when}"));
        v.push(count);
        v
    } else {
        vec![meta.map_or_else(|| when.clone(), |m| format!("Day {} · {} people", m.day, m.population)), format!("{when} · {count}")]
    };
    for l in lines {
        painter.text(egui::pos2(x, y), egui::Align2::LEFT_TOP, super::fmt::elide(&l, ((w / 6.5) as usize).max(8)), crate::ui::theme::small(), Color32::from_rgb(190, 185, 170));
        y += 19.0;
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// [title] One save state of the picked game: picture, what it is, the
/// figures, when it was saved. Click picks it, double-click loads it.
fn state_row(ui: &mut egui::Ui, st: &SaveState, picked: bool, pic: Option<&egui::TextureHandle>) -> egui::Response {
    let size = egui::vec2(ui.available_width().max(200.0), ROW_PICTURE + 12.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    let painter = ui.painter_at(rect);
    if picked || resp.hovered() {
        painter.rect_filled(rect, 5.0, if picked { Color32::from_rgb(44, 36, 26) } else { Color32::from_rgb(28, 28, 36) });
    }
    if picked {
        painter.rect_stroke(rect, 5.0, egui::Stroke::new(1.5, GOLD), egui::StrokeKind::Inside);
    }
    let img = Rect::from_min_size(rect.min + egui::vec2(6.0, 6.0), egui::vec2(ROW_PICTURE, ROW_PICTURE));
    match pic {
        Some(tex) => {
            painter.image(tex.id(), img, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
        }
        None => {
            painter.rect_filled(img, 3.0, super::theme::NO_PICTURE);
        }
    }
    let x = img.right() + 12.0;
    let kind = match st.kind {
        StateKind::Named => "named save",
        StateKind::Latest | StateKind::Dated => "quick save",
        StateKind::Autosave => "autosave",
        StateKind::History => "history autosave",
        StateKind::Marked => "marked moment",
    };
    let col = if matches!(st.kind, StateKind::Named | StateKind::Marked) { GOLD } else { IVORY };
    painter.text(egui::pos2(x, img.top()), egui::Align2::LEFT_TOP, &st.label, crate::ui::theme::lead(), col);
    let figures = st.figures();
    painter.text(egui::pos2(x, img.top() + 24.0), egui::Align2::LEFT_TOP, if figures.is_empty() { "no figures (an old save)".to_string() } else { figures }, crate::ui::theme::small(), Color32::from_rgb(190, 185, 170));
    painter.text(egui::pos2(x, img.top() + 43.0), egui::Align2::LEFT_TOP, format!("{kind} · saved {}", age(st.modified)), crate::ui::theme::small(), Color32::from_gray(140));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The title logo as an egui texture, uploaded once and kept in egui's memory.
fn logo_texture(ctx: &egui::Context) -> egui::TextureHandle {
    let id = egui::Id::new("brand_logo");
    if let Some(t) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(id)) {
        return t;
    }
    const RGBA: &[u8] = include_bytes!("../../../../assets/brand/logo_800x280.rgba");
    let image = egui::ColorImage::from_rgba_unmultiplied([800, 280], RGBA);
    let t = ctx.load_texture("brand_logo", image, egui::TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, t.clone()));
    t
}
