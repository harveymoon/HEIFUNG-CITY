//! [viewer] The viewer window: the lists and filters on the left, the
//! turntable in the middle with its arrows and lighting toggles, what the
//! building is and its notes on the right (and an Inspirations picture,
//! when one is picked).

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use egui::{Color32, RichText};

use super::notes::{BuildingNotes, Target};
use super::{catalogue, Filters, Source, Subject, Viewer};
use crate::camera::{CameraFly, CameraFollow, CameraMode, FlyGoal};
use crate::render::Turntable;
use crate::sim::Sim;
use crate::tools::Selection;
use crate::ui::theme::{AMBER, GOLD, GOOD, IVORY};
use crate::ui::UiFlags;

fn open_id() -> egui::Id {
    egui::Id::new("building_viewer_open")
}

fn want_id() -> egui::Id {
    egui::Id::new("building_viewer_want")
}

/// Whether the viewer was open at the end of the last pass.
pub fn is_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(open_id()).unwrap_or(false))
}

/// The top-bar toggle: the window system picks the click up this pass.
pub fn toggle(ui: &mut egui::Ui) {
    let open = is_open(ui.ctx());
    if ui.selectable_label(open, "Viewer").on_hover_text("The building viewer: every building in the city and every style the generator makes, one at a time on a turntable, with notes for Claude [X]").clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(want_id(), !open));
    }
}

/// The Inspirations folder: `ND_INSPIRATIONS`, or an `Inspirations` folder
/// beside the home folder, the exe or the source tree, or above them.
pub fn inspirations_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("ND_INSPIRATIONS").map(PathBuf::from).filter(|p| p.is_dir()) {
        return Some(p);
    }
    let starts = [std::env::current_dir().ok(), std::env::current_exe().ok(), Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")))];
    for s in starts.into_iter().flatten() {
        for a in s.ancestors().take(7) {
            let p = a.join("Inspirations");
            if p.is_dir() {
                return Some(p);
            }
        }
    }
    None
}

/// Every picture in the folder and its sub-folders, sorted.
pub fn inspirations() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>, depth: u32) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() && depth < 3 {
                walk(&p, out, depth + 1);
            } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| matches!(x.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "webp")) {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    if let Some(d) = inspirations_dir() {
        walk(&d, &mut out, 0);
    }
    out.sort();
    out
}

/// A picture's short name: its folder and file.
fn short(p: &Path) -> String {
    let file = p.file_name().map_or_else(String::new, |f| f.to_string_lossy().into_owned());
    match p.parent().and_then(|d| d.file_name()) {
        Some(d) => format!("{}/{file}", d.to_string_lossy()),
        None => file,
    }
}

/// Load a picture as an egui texture, at most 1024 px on its long side.
fn load_picture(ctx: &egui::Context, path: &Path) -> Result<egui::TextureHandle, String> {
    let img = image::open(path).map_err(|e| e.to_string())?;
    let img = if img.width().max(img.height()) > 1024 { img.thumbnail(1024, 1024) } else { img };
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    Ok(ctx.load_texture(format!("inspiration {}", path.display()), color, egui::TextureOptions::LINEAR))
}

/// What the window keeps between passes that is only the window's.
#[derive(Default)]
pub struct WindowState {
    pictures: Option<Vec<PathBuf>>,
    picture: Option<(PathBuf, Result<egui::TextureHandle, String>)>,
}

#[allow(clippy::too_many_arguments)]
pub fn window(
    mut contexts: EguiContexts,
    mut viewer: ResMut<Viewer>,
    mut tt: ResMut<Turntable>,
    mut notes: ResMut<BuildingNotes>,
    sim: Res<Sim>,
    mut flags: ResMut<UiFlags>,
    mut selection: ResMut<Selection>,
    (mut fly, mut follow, mut mode): (ResMut<CameraFly>, ResMut<CameraFollow>, ResMut<CameraMode>),
    mut local: Local<WindowState>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    if let Some(want) = ctx.data_mut(|d| d.remove_temp::<bool>(want_id())) {
        viewer.open = want;
    }
    let typing = ctx.egui_wants_keyboard_input();
    // X opens and closes it; while open, the arrows flip and Esc closes.
    if !typing {
        let (x, esc, left, right, up, down) = ctx.input(|i| {
            let k = |key| i.key_pressed(key) && i.modifiers.is_none();
            (k(egui::Key::X), k(egui::Key::Escape), k(egui::Key::ArrowLeft), k(egui::Key::ArrowRight), k(egui::Key::ArrowUp), k(egui::Key::ArrowDown))
        });
        if x {
            viewer.open = !viewer.open;
        } else if viewer.open {
            let game = sim.world.identity.game_id.clone();
            if esc {
                viewer.open = false;
            }
            if left || right {
                viewer.step(if right { 1 } else { -1 }, &notes, &game);
            }
            if up || down {
                if viewer.source == Source::Catalogue {
                    viewer.step_variant(if down { 1 } else { -1 });
                } else {
                    viewer.step(if down { 1 } else { -1 }, &notes, &game);
                }
            }
        }
    }
    ctx.data_mut(|d| d.insert_temp(open_id(), viewer.open));
    if !viewer.open {
        local.picture = None;
        return Ok(());
    }
    let content = flags.map_rect(ctx.viewport_rect());
    let size = egui::vec2((content.width() - 24.0).max(600.0), (content.height() - 24.0).max(420.0));
    let mut open = true;
    egui::Window::new("Building viewer")
        .id(egui::Id::new("building_viewer"))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .fixed_size(size)
        .default_pos(content.min + egui::vec2(12.0, 12.0))
        .show(ctx, |ui| {
            let game = sim.world.identity.game_id.clone();
            top_row(ui, &mut viewer, &notes, &game);
            ui.separator();
            let body_h = ui.available_height();
            let list_w = 270.0;
            let side_w = 330.0;
            let stage_w = (size.x - list_w - side_w - 70.0).max(240.0);
            ui.horizontal_top(|ui| {
                ui.set_min_height(body_h);
                // The list.
                ui.vertical(|ui| {
                    ui.set_width(list_w);
                    list(ui, &mut viewer, &notes, &game, body_h);
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_width(stage_w);
                    stage(ui, &mut viewer, &mut tt, &notes, &sim, stage_w, body_h);
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_width(side_w);
                    egui::ScrollArea::vertical().id_salt("viewer_side").max_height(body_h).show(ui, |ui| {
                        ui.set_width(side_w - 14.0);
                        side(ui, &mut viewer, &mut notes, &sim, &tt);
                        ui.separator();
                        inspiration(ui, &mut viewer, &mut local);
                    });
                });
            });
        });
    if !open {
        viewer.open = false;
    }
    ctx.data_mut(|d| d.insert_temp(open_id(), viewer.open));
    // "Go to it": the camera flies there and the viewer steps aside.
    if let Some(id) = viewer.go_to.take() {
        if let Some(r) = viewer.row(id) {
            *selection = Selection::Building(id);
            follow.stop();
            *mode = CameraMode::Orbit;
            let d = (r.floors as f32 * 4.0 + r.footprint.sqrt() * 2.0).clamp(60.0, 400.0);
            fly.to(FlyGoal::Point(bevy::math::Vec2::new(r.pos.x, r.pos.y)), Some(d), Some(0.7));
            flags.message = format!("Flying to {}", r.name);
            viewer.open = false;
        }
    }
    Ok(())
}

/// The tabs, the search box and the filters.
fn top_row(ui: &mut egui::Ui, v: &mut Viewer, notes: &BuildingNotes, game: &str) {
    ui.horizontal_wrapped(|ui| {
        ui.set_max_width(ui.available_width());
        let open_notes = notes.book.notes.iter().filter(|n| n.is_open()).count();
        for (s, label) in [(Source::City, format!("City buildings ({})", v.rows.len())), (Source::Catalogue, format!("Catalogue ({} styles)", v.catalogue.len())), (Source::Notes, format!("Notes ({open_notes} open)"))] {
            if ui.selectable_label(v.source == s, RichText::new(label).color(if v.source == s { IVORY } else { GOLD })).clicked() && v.source != s {
                v.source = s;
                v.scroll_to_pick = true;
            }
        }
        ui.separator();
        ui.label("Search");
        ui.add(egui::TextEdit::singleline(&mut v.filters.search).desired_width(150.0).hint_text("name, style, owner…"));
        ui.checkbox(&mut v.filters.noted, "Open notes only");
        if v.filters != Filters::default() && ui.button("Clear filters").clicked() {
            v.filters = Filters::default();
        }
        let n = match v.source {
            Source::City => v.city_list(notes, game).len(),
            Source::Catalogue => v.catalogue_list(notes).len(),
            Source::Notes => v.notes_list(notes).len(),
        };
        ui.label(RichText::new(format!("{n} shown")).weak());
    });
    if v.source == Source::Notes {
        return;
    }
    // The filters on a row of their own.
    ui.horizontal_wrapped(|ui| {
        let f = &mut v.filters;
        let groups: Vec<&'static str> = catalogue::GROUPS.iter().copied().filter(|g| *g != "Other").collect();
        combo(ui, "viewer_group", "every style group", &mut f.group, groups.iter().map(|g| g.to_string()).collect());
        combo_lu(ui, &mut f.land_use);
        combo_tier(ui, &mut f.tier);
        if v.source == Source::City {
            let mut forms: Vec<String> = v.rows.iter().filter_map(|r| r.form.map(str::to_string)).collect();
            let mut districts: Vec<String> = v.rows.iter().map(|r| r.district.clone()).filter(|d| !d.is_empty()).collect();
            let mut stories: Vec<String> = v.rows.iter().map(|r| r.story.to_string()).filter(|d| !d.is_empty()).collect();
            let mut kinds: Vec<String> = v.rows.iter().filter_map(|r| r.business.map(str::to_string)).collect();
            for l in [&mut forms, &mut districts, &mut stories, &mut kinds] {
                l.sort();
                l.dedup();
            }
            let f = &mut v.filters;
            combo(ui, "viewer_form", "every form", &mut f.form, forms);
            combo(ui, "viewer_district", "every district", &mut f.district, districts);
            combo(ui, "viewer_story", "every story", &mut f.story, stories);
            combo(ui, "viewer_business", "every business", &mut f.business, kinds);
        }
    });
}

fn combo(ui: &mut egui::Ui, id: &str, any: &str, value: &mut Option<String>, options: Vec<String>) {
    egui::ComboBox::from_id_salt(id).selected_text(value.clone().unwrap_or_else(|| any.to_string())).width(118.0).show_ui(ui, |ui| {
        ui.selectable_value(value, None, any);
        for o in options {
            ui.selectable_value(value, Some(o.clone()), o);
        }
    });
}

fn combo_lu(ui: &mut egui::Ui, value: &mut Option<nd_sim::LandUse>) {
    egui::ComboBox::from_id_salt("viewer_lu").selected_text(value.map_or("every land use", |l| l.label())).width(120.0).show_ui(ui, |ui| {
        ui.selectable_value(value, None, "every land use");
        for l in nd_sim::LandUse::ALL {
            ui.selectable_value(value, Some(l), l.label());
        }
    });
}

fn combo_tier(ui: &mut egui::Ui, value: &mut Option<nd_sim::Tier>) {
    egui::ComboBox::from_id_salt("viewer_tier").selected_text(value.map_or("every tier", |t| t.label())).width(100.0).show_ui(ui, |ui| {
        ui.selectable_value(value, None, "every tier");
        for t in nd_sim::Tier::ALL {
            ui.selectable_value(value, Some(t), t.label());
        }
    });
}

/// The list of the tab: click one to show it.
fn list(ui: &mut egui::Ui, v: &mut Viewer, notes: &BuildingNotes, game: &str, height: f32) {
    let scroll = std::mem::take(&mut v.scroll_to_pick);
    egui::ScrollArea::vertical().id_salt(("viewer_list", v.source.key())).max_height(height).auto_shrink([false, false]).show(ui, |ui| match v.source {
        Source::City => {
            let rows = v.city_list(notes, game);
            if rows.is_empty() {
                ui.label(RichText::new("No building matches.").weak());
            }
            for i in rows {
                let r = &v.rows[i];
                let on = v.pick == Some(r.id);
                let noted = notes.open_count(&super::building_target(game, "", r)) > 0;
                let text = format!("{}{}\n{} · {} fl · {}", if noted { "★ " } else { "" }, r.name, r.style_label, r.floors, if r.district.is_empty() { r.land_use.label() } else { r.district.as_str() });
                let resp = ui.selectable_label(on, RichText::new(text).size(13.0));
                if on && scroll {
                    resp.scroll_to_me(Some(egui::Align::Center));
                }
                if resp.clicked() {
                    v.pick = Some(r.id);
                }
            }
        }
        Source::Catalogue => {
            let list = v.catalogue_list(notes);
            let mut group = "";
            for i in list {
                let e = &v.catalogue[i];
                if e.group != group {
                    group = e.group;
                    ui.label(RichText::new(group).color(GOLD).strong());
                }
                let on = v.style == i;
                let noted = notes.open_count(&Target::Style { style: e.key.clone() }) > 0;
                let resp = ui.selectable_label(on, RichText::new(format!("{}{}", if noted { "★ " } else { "" }, e.label)).size(13.0));
                if on && scroll {
                    resp.scroll_to_me(Some(egui::Align::Center));
                }
                if resp.clicked() && !on {
                    v.style = i;
                    v.variant = 0;
                }
            }
        }
        Source::Notes => {
            let ids = v.notes_list(notes);
            if ids.is_empty() {
                ui.label(RichText::new("No notes yet. Write one beside any building or style.").weak());
            }
            for id in ids {
                let Some(n) = notes.get(id) else { continue };
                let first: String = n.text.lines().next().unwrap_or("").chars().take(60).collect();
                let text = format!("#{} {} · {}\n{}", n.id, if n.is_open() { "open" } else { "done" }, n.target.title(), first);
                let col = if n.is_open() { IVORY } else { Color32::from_gray(140) };
                let resp = ui.selectable_label(v.note == Some(id), RichText::new(text).size(13.0).color(col));
                if v.note == Some(id) && scroll {
                    resp.scroll_to_me(Some(egui::Align::Center));
                }
                if resp.clicked() {
                    v.note = Some(id);
                }
            }
        }
    });
}

/// The turntable image (drag to orbit, wheel to zoom), the arrows and the
/// lighting toggles.
fn stage(ui: &mut egui::Ui, v: &mut Viewer, tt: &mut Turntable, notes: &BuildingNotes, sim: &Sim, width: f32, height: f32) {
    let game = sim.world.identity.game_id.clone();
    let img_h = (height - 96.0).max(200.0).min(width * crate::render::turntable_aspect());
    let img_w = img_h / crate::render::turntable_aspect();
    let subject = v.subject(sim, notes);
    ui.vertical_centered(|ui| {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(img_w, img_h), egui::Sense::click_and_drag());
        match (tt.texture, subject) {
            (Some(id), s) if s != Subject::None => {
                ui.painter().image(id, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            _ => {
                ui.painter().rect_filled(rect, 4.0, crate::ui::theme::NO_PICTURE);
                let why = match v.source {
                    Source::Notes => "Pick a note. A note on a building of another city has nothing to show here.",
                    _ => "Nothing to show: no building matches the filters.",
                };
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, why, egui::FontId::proportional(14.0), Color32::from_gray(170));
            }
        }
        if resp.dragged() {
            let d = resp.drag_delta();
            tt.yaw -= d.x * 0.008;
            tt.pitch += d.y * 0.006;
            tt.auto_rotate = false;
        }
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                tt.zoom *= (-scroll * 0.002).exp();
            }
        }
        if resp.double_clicked() {
            tt.reset_view();
        }
        resp.on_hover_text("Drag to turn it, wheel to zoom, double-click to reset. Left/right flip, up/down change the variant.");
        // The arrows and where we are in the list.
        ui.horizontal(|ui| {
            let (at, n) = position(v, notes, &game);
            if ui.button("< Prev").clicked() {
                v.step(-1, notes, &game);
            }
            ui.label(format!("{} / {n}", at.map_or("-".to_string(), |a| (a + 1).to_string())));
            if ui.button("Next >").clicked() {
                v.step(1, notes, &game);
            }
            if let Subject::Style(_, var) = subject {
                ui.separator();
                let total = v.catalogue.get(v.style).map_or(0, |e| e.variants.len());
                if ui.button("Variant -").clicked() {
                    v.step_variant(-1);
                }
                ui.label(format!("{} / {total}", var + 1));
                if ui.button("Variant +").clicked() {
                    v.step_variant(1);
                }
            }
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut tt.auto_rotate, "Turn");
            if ui.selectable_label(!tt.night, "Day").clicked() {
                tt.night = false;
            }
            if ui.selectable_label(tt.night, "Night").clicked() {
                tt.night = true;
            }
            ui.checkbox(&mut tt.lights, "Lights");
            if ui.button("Reset view").clicked() {
                tt.reset_view();
            }
            if ui.button("Save picture").on_hover_text("The turntable's image, to screenshots/ in the game's folder").clicked() {
                let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
                tt.shots.push(PathBuf::from(format!("screenshots/viewer-{ms}.png")));
            }
            if let Subject::City(id) = subject {
                if ui.button(RichText::new("Go to it in the city").color(GOLD)).clicked() {
                    v.go_to = Some(id);
                }
            }
        });
    });
}

/// Where the pick is in the list shown, and the list's length.
fn position(v: &Viewer, notes: &BuildingNotes, game: &str) -> (Option<usize>, usize) {
    match v.source {
        Source::City => {
            let l = v.city_list(notes, game);
            (v.pick.and_then(|p| l.iter().position(|&i| v.rows[i].id == p)), l.len())
        }
        Source::Catalogue => {
            let l = v.catalogue_list(notes);
            (l.iter().position(|&i| i == v.style), l.len())
        }
        Source::Notes => {
            let l = v.notes_list(notes);
            (v.note.and_then(|n| l.iter().position(|&x| x == n)), l.len())
        }
    }
}

fn line(ui: &mut egui::Ui, k: &str, val: impl Into<String>) {
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(k).weak());
        ui.label(val.into());
    });
}

/// What the building or style is, and its notes.
fn side(ui: &mut egui::Ui, v: &mut Viewer, notes: &mut BuildingNotes, sim: &Sim, tt: &Turntable) {
    let subject = v.subject(sim, notes);
    match subject {
        Subject::City(id) => {
            let Some(r) = v.row(id).cloned() else { return };
            ui.label(RichText::new(&r.name).font(crate::ui::theme::lead()).color(IVORY));
            line(ui, "Style", format!("{} ({})", r.style_label, r.group));
            if let Some(f) = r.form {
                line(ui, "Form", f);
            }
            line(ui, "Use", format!("{} · {} tier", r.land_use.label(), r.tier.label()));
            line(ui, "Floors", format!("{} · {:.0} m tall as drawn", r.floors, tt.size.y));
            line(ui, "Footprint", format!("{:.0} m² · {:.0} x {:.0} m", r.footprint, tt.size.x, tt.size.z));
            if !r.district.is_empty() {
                line(ui, "District", format!("{} · {}", r.district, r.story));
            }
            if let Some(b) = r.business {
                line(ui, "Business", b);
            }
            if r.homes > 0 || r.jobs > 0 {
                line(ui, "Homes, jobs", format!("{} · {}", r.homes, r.jobs));
            }
            line(ui, "Owner", r.owner.clone().unwrap_or_else(|| "none on record".into()));
            if !r.tenants.is_empty() {
                line(ui, "Tenants", r.tenants.join(", "));
            }
            if r.state != "standing" {
                line(ui, "State", r.state);
            }
            line(ui, "Id", r.key.clone());
        }
        Subject::Style(s, var) => {
            let e = v.catalogue[s.index()].clone();
            ui.label(RichText::new(e.label).font(crate::ui::theme::lead()).color(IVORY));
            line(ui, "Style", format!("{} ({})", e.key, e.group));
            line(ui, "Drawn for", format!("{} · {}", e.land_use.label(), if e.on_menu { "on the zone menu" } else { "forced (a business, form or service)" }));
            line(ui, "Tiers", e.tiers.iter().map(|t| t.label()).collect::<Vec<_>>().join(", "));
            if let Some(x) = e.variants.get(var) {
                line(ui, "Variant", format!("{} of {} · {} tier · {} floors", var + 1, e.variants.len(), x.tier.label(), x.floors));
                line(ui, "Seed", format!("{:#x}", x.seed));
            }
            line(ui, "Size as drawn", format!("{:.0} x {:.0} m, {:.0} m tall", tt.size.x, tt.size.z, tt.size.y));
        }
        Subject::None => {
            ui.label(RichText::new("Nothing picked.").weak());
        }
    }
    ui.separator();
    // The picked note, in the Notes tab, even when its building is gone.
    if v.source == Source::Notes {
        if let Some(n) = v.note.and_then(|id| notes.get(id)).cloned() {
            ui.label(RichText::new(format!("Note #{} on {}", n.id, n.target.title())).color(GOLD));
            if let Target::Building { city, .. } = &n.target {
                if !city.is_empty() {
                    line(ui, "City", city.clone());
                }
            }
        }
    }
    let Some(target) = v.target(sim, notes).or_else(|| v.note.and_then(|id| notes.get(id)).map(|n| n.target.clone())) else { return };
    let what = if matches!(target, Target::Style { .. }) { "this style (every variant)" } else { "this building" };
    ui.label(RichText::new(format!("Notes on {what}")).strong());
    let on: Vec<_> = notes.on(&target).cloned().collect();
    let mut act: Option<(u32, &str)> = None;
    for n in &on {
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("#{}", n.id)).weak());
                ui.label(RichText::new(if n.is_open() { "open" } else { "done" }).color(if n.is_open() { AMBER } else { GOOD }));
                ui.label(RichText::new(format!("by {}", n.by)).weak());
            });
            ui.label(n.text.as_str());
            for r in &n.replies {
                ui.label(RichText::new(format!("{}: {}", r.by, r.text)).italics().color(Color32::from_gray(185)));
            }
            ui.horizontal(|ui| {
                if ui.small_button("Edit").clicked() {
                    act = Some((n.id, "edit"));
                }
                if n.is_open() {
                    if ui.small_button("Done").clicked() {
                        act = Some((n.id, "resolve"));
                    }
                } else if ui.small_button("Reopen").clicked() {
                    act = Some((n.id, "reopen"));
                }
                if ui.small_button("Delete").clicked() {
                    act = Some((n.id, "delete"));
                }
            });
        });
    }
    if let Some((id, a)) = act {
        let _ = match a {
            "edit" => {
                v.editing = Some(id);
                v.draft = notes.get(id).map_or_else(String::new, |n| n.text.clone());
                Ok(())
            }
            "resolve" => notes.resolve(id, true, "player"),
            "reopen" => notes.resolve(id, false, "player"),
            _ => notes.delete(id),
        };
    }
    ui.add(egui::TextEdit::multiline(&mut v.draft).desired_rows(3).desired_width(f32::INFINITY).hint_text("A note for Claude: what looks wrong, what you'd like…"));
    ui.horizontal(|ui| {
        let label = if v.editing.is_some() { "Save changes" } else { "Add note" };
        if ui.button(RichText::new(label).color(GOLD)).clicked() && !v.draft.trim().is_empty() {
            let r = match v.editing {
                Some(id) => notes.edit(id, &v.draft),
                None => notes.add(target.clone(), &v.draft, "player").map(|_| ()),
            };
            if r.is_ok() {
                v.draft.clear();
                v.editing = None;
            }
        }
        if (v.editing.is_some() || !v.draft.is_empty()) && ui.button("Cancel").clicked() {
            v.draft.clear();
            v.editing = None;
        }
    });
    if let Some(e) = &notes.error {
        ui.label(RichText::new(e).color(crate::ui::theme::WARN).small());
    }
}

/// An Inspirations picture beside the building.
fn inspiration(ui: &mut egui::Ui, v: &mut Viewer, local: &mut WindowState) {
    let pics = local.pictures.get_or_insert_with(inspirations);
    if pics.is_empty() {
        return;
    }
    ui.horizontal(|ui| {
        ui.label(RichText::new("Inspiration").strong());
        let at = v.inspiration.as_ref().and_then(|p| pics.iter().position(|x| x == p));
        let mut text = v.inspiration.as_deref().map_or_else(|| "none".to_string(), short);
        if text.chars().count() > 20 {
            text = format!("{}…", text.chars().take(19).collect::<String>());
        }
        egui::ComboBox::from_id_salt("viewer_inspiration").selected_text(text).width(150.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut v.inspiration, None, "none");
            for p in pics.iter() {
                ui.selectable_value(&mut v.inspiration, Some(p.clone()), short(p));
            }
        });
        if ui.small_button("<").clicked() {
            v.inspiration = Some(pics[at.map_or(pics.len() - 1, |a| (a + pics.len() - 1) % pics.len())].clone());
        }
        if ui.small_button(">").clicked() {
            v.inspiration = Some(pics[at.map_or(0, |a| (a + 1) % pics.len())].clone());
        }
    });
    let Some(path) = v.inspiration.clone() else {
        local.picture = None;
        return;
    };
    if local.picture.as_ref().is_none_or(|(p, _)| *p != path) {
        local.picture = Some((path.clone(), load_picture(ui.ctx(), &path)));
    }
    match &local.picture.as_ref().unwrap().1 {
        Ok(tex) => {
            let s = tex.size_vec2();
            let w = ui.available_width().min(320.0);
            ui.add(egui::Image::new((tex.id(), egui::vec2(w, w * s.y / s.x.max(1.0)))));
        }
        Err(e) => {
            ui.label(RichText::new(format!("Cannot show it: {e}")).color(crate::ui::theme::WARN).small());
        }
    }
}
