//! The tool windows: draggable, closable `egui::Window`s opened from the top
//! bar (Lines, View, Saves, Help, Dev). egui remembers each window's
//! position in its own memory; whether a window is open lives in the same
//! memory, so the UI carries no state of its own for them.
//!
//! The reports that were windows here (City, Economy, Taxes, Market,
//! People, Power, Transit, Emergencies, Districts) are pages of the
//! full-screen City Data view now (`ui/city_data.rs`). What stays a window
//! is a tool used beside the map: the bus line editor, the map layers and
//! lighting, saves, help and the developer odds and ends.

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy_egui::egui;

use super::{help, hint, neon, pane, saves, section, UiCtx};
use crate::camera::CameraMode;

/// The tool windows the top bar can open, in toggle order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Info {
    /// [lines] Bus lines one by one: a strip diagram of the picked line's
    /// stops, legs and buses, with its editing controls (ui/lines.rs).
    Lines,
    /// The View window (once "Land"): the map layer picker, what else is
    /// drawn, lighting, graphics and text size. Titled "View"; the API still
    /// takes "land".
    Land,
    Saves,
    Help,
    Dev,
    /// [pt-ux] The Alerts window (ui/alerts.rs).
    Alerts,
    /// [settings] Graphics, text size, speech bubbles and the controls.
    Settings,
}

impl Info {
    pub const ALL: [Info; 7] = [Info::Lines, Info::Land, Info::Alerts, Info::Saves, Info::Settings, Info::Help, Info::Dev];

    pub fn title(self) -> &'static str {
        match self {
            Info::Lines => "Lines",
            Info::Land => "View",
            Info::Saves => "Saves",
            Info::Help => "Help",
            Info::Dev => "Dev",
            Info::Alerts => "Alerts",
            Info::Settings => "Settings",
        }
    }

    /// Default width; the windows resize, this is only where they start.
    fn width(self) -> f32 {
        match self {
            Info::Help => 440.0,
            // [lines] Room for the stop diagram.
            Info::Lines => 580.0,
            Info::Dev => 300.0,
            // [identity] Room for the city name row and the grouped list.
            Info::Saves => 460.0,
            Info::Land => 380.0,
            Info::Alerts => 420.0,
            Info::Settings => 420.0,
        }
    }

    fn id(self) -> egui::Id {
        egui::Id::new(("info_window", self.title()))
    }

    pub fn is_open(self, ctx: &egui::Context) -> bool {
        ctx.data(|d| d.get_temp::<bool>(self.id()).unwrap_or(false))
    }

    pub fn set_open(self, ctx: &egui::Context, open: bool) {
        ctx.data_mut(|d| d.insert_temp(self.id(), open));
    }
}

/// Every open window. `content` is the map area between the bars: windows
/// open in a cascade down its left side and cannot be dragged under a bar.
/// Each draws its own header (`pane::header`: title and close) in place of
/// egui's title bar; the window still drags by any empty part of it.
pub fn show_all(ctx: &egui::Context, p: &mut UiCtx, content: egui::Rect) {
    for (i, info) in Info::ALL.into_iter().enumerate() {
        if !info.is_open(ctx) {
            continue;
        }
        let cascade = content.left_top() + egui::vec2(12.0 + 28.0 * i as f32, 12.0 + 30.0 * i as f32);
        // A window keeps its width: content sized from the space it has must
        // never push it wider frame after frame. [fix/buildings-ui] The
        // Lines window may be dragged wider, up to the map's width (its
        // stop diagram and timetable use the room); it still starts at its
        // default.
        let max_width = if info == Info::Lines { (content.width() - 24.0).max(info.width()) } else { info.width() };
        let window = egui::Window::new(info.title())
            .title_bar(false)
            .constrain_to(content)
            .max_height(content.height() - 24.0)
            .id(info.id().with("window"))
            .default_pos(cascade)
            .default_width(info.width())
            .max_width(max_width);
        // [lines] The Lines window is as tall as what it shows (up to the
        // map's height): the height its content took last frame is its
        // minimum, so opening a line grows it rather than scrolling it in
        // the window's first, short height.
        let fit_id = info.id().with("fit_height");
        let window = if info == Info::Lines {
            let fit: f32 = ctx.data(|d| d.get_temp(fit_id)).unwrap_or(0.0);
            window.resizable([true, false]).min_height(fit.min(content.height() - 60.0))
        } else {
            window
        };
        let mut keep = true;
        window.show(ctx, |ui| {
            keep = pane::header(ui, info);
            let out = egui::ScrollArea::vertical().id_salt(("info_scroll", info.title())).show(ui, |ui| body(ui, p, info));
            if info == Info::Lines {
                let h = out.content_size.y + 44.0;
                ui.ctx().data_mut(|d| d.insert_temp(fit_id, h));
            }
        });
        if !keep {
            info.set_open(ctx, false);
        }
    }
}

fn body(ui: &mut egui::Ui, p: &mut UiCtx, info: Info) {
    match info {
        Info::Lines => super::lines::window(ui, p),
        Info::Land => land(ui, p),
        Info::Saves => saves::panel(ui, p.sim, p.saves, p.flags),
        Info::Help => help_window(ui),
        Info::Dev => dev(ui, p),
        Info::Alerts => super::alerts::body(ui, p),
        Info::Settings => settings(ui, p),
    }
}

/// The View window: the map layer (one at a time, with its key in the
/// legend), what else is drawn, land value and class mix, and the lighting.
pub(super) fn land(ui: &mut egui::Ui, p: &mut UiCtx) {
    section(ui, "MAP LAYER");
    hint(ui, "One layer at a time, its key at the bottom right of the map. The Layer button in the top bar picks it too; V, O, P toggle theirs, L steps through the heat maps, K through the land layers.");
    super::topbar::layer_list(ui, p.overlays, true);
    section(ui, "ALSO DRAWN");
    ui.checkbox(&mut p.tools.show_routes, "Bus lines whatever the tool");
    ui.checkbox(&mut p.overlays.projects, "Other pending projects, dim");
    ui.checkbox(&mut p.lighting.heat, "Drone heat vision [H]");
    hint(ui, "Land value, rents and the class mix are in City Data, City, Land and rent.");
    section(ui, "LIGHTING");
    let lighting = &mut *p.lighting;
    ui.checkbox(&mut lighting.follow_clock, "Follow the clock");
    if !lighting.follow_clock {
        ui.horizontal(|ui| {
            ui.add(egui::Slider::new(&mut lighting.hour, 0.0..=24.0).text("hour").fixed_decimals(1));
            for (label, h) in [("dawn", 6.5), ("noon", 12.5), ("dusk", 18.5), ("night", 23.0)] {
                if ui.small_button(label).clicked() {
                    lighting.hour = h;
                }
            }
        });
    }
    hint(ui, "Graphics, text size and speech bubbles: the Settings window.");
}

/// [settings] The Settings window: graphics, text size and speech bubbles
/// (kept in `settings.ron` in the game's home folder), and the controls.
fn settings(ui: &mut egui::Ui, p: &mut UiCtx) {
    graphics_setting(ui, p);
    super::theme::scale_setting(ui);
    section(ui, "CONTROLS");
    hint(ui, "The camera and the keys; the Help window has the tools.");
    ui.label(egui::RichText::new(help::CAMERA_ORBIT).small());
    ui.label(egui::RichText::new(format!("Walk mode: {}", help::CAMERA_WALK)).small());
    ui.label(egui::RichText::new(help::HOTKEYS).small());
    ui.label(egui::RichText::new(help::TOOL_KEYS).small());
}

/// [look] The graphics setting: Auto holds about 60 fps by stepping the
/// effects down and up; a tier pins them.
fn graphics_setting(ui: &mut egui::Ui, p: &mut UiCtx) {
    section(ui, "GRAPHICS");
    ui.horizontal(|ui| {
        for q in crate::render::QualitySetting::ALL {
            if ui.selectable_label(p.lighting.quality == q, q.label()).clicked() {
                p.lighting.quality = q;
                // [settings] Kept for the next run.
                super::theme::save_quality(q.label());
            }
        }
    });
    let g = &p.stats.graphics;
    let now = if p.lighting.quality == crate::render::QualitySetting::Auto { format!("Auto, at {} now", g.tier.label()) } else { g.tier.label().to_string() };
    hint(ui, &format!("{now} · {:.1} ms a frame. Auto steps ambient occlusion, volumetric fog, anti-aliasing and shadow detail down to hold 60 fps, and back up when there is room.", g.frame_ms));
}

fn help_window(ui: &mut egui::Ui) {
    section(ui, "WINDOWS");
    ui.label(egui::RichText::new(help::WINDOWS).small());
    section(ui, "CAMERA");
    ui.label(egui::RichText::new(help::CAMERA_ORBIT).small());
    ui.label(egui::RichText::new(format!("Walk mode: {}", help::CAMERA_WALK)).small());
    section(ui, "KEYS");
    ui.label(egui::RichText::new(help::HOTKEYS).small());
    ui.label(egui::RichText::new(help::TOOL_KEYS).small());
    section(ui, "PROJECT");
    ui.label(egui::RichText::new(help::PROJECT_NONE).small());
    section(ui, "TOOLS");
    for (tool, text) in help::TOOL_HINTS {
        neon(ui, tool.label());
        ui.label(egui::RichText::new(text).small());
    }
    section(ui, "SAVES");
    ui.label(egui::RichText::new(help::SAVES).small());
    hint(ui, help::ASSETS_RELOAD);
}

/// Developer odds and ends: the demo seed, the mind backend, frame and mesh
/// figures, asset errors. Out of the way of play but one click off. (A
/// window rather than a menu: egui's menu popup would not open from a bar
/// laid out on the background layer.)
fn dev(ui: &mut egui::Ui, p: &mut UiCtx) {
    if *p.mode == CameraMode::Orbit {
        let has_buildings = !p.sim.world.buildings.is_empty();
        let label = if has_buildings { "Seed demo district anyway" } else { "Seed demo district [G]" };
        if ui.button(label).clicked() {
            p.sim.seed_demo_district();
            p.tools.status = "Demo district laid out around the centre".into();
        }
        if has_buildings {
            hint(ui, help::SEED_EXISTING);
        }
        ui.separator();
    }
    hint(ui, &format!("minds: {} · backend: {}", nd_sim::MindProvider::name(&p.sim.minds), p.sim.minds.backend_name()));
    let fps = p.diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()).unwrap_or(0.0);
    hint(ui, &format!("{fps:.0} fps · {} ticks/frame · effective speed {:.0}%", p.sim.ticks_last_frame, p.sim.effective_speed * 100.0));
    hint(ui, &format!("{} building meshes · {}k triangles", p.stats.building_entities, (p.stats.street_triangles + p.stats.building_triangles) / 1000));
    // [look] The graphics tier in force and why, the frame time it is held to.
    let g = &p.stats.graphics;
    hint(ui, &format!("graphics: {} ({}) · {:.1} ms/frame, sim {:.1} ms · {} auto steps · {}", g.tier.label(), p.lighting.quality.label(), g.frame_ms, g.sim_ms, g.steps, g.reason));
    hint(ui, &g.effects);
    if let Some(err) = &p.assets.last_error {
        ui.label(egui::RichText::new(err).small().color(egui::Color32::from_rgb(255, 60, 90)));
    }
    hint(ui, help::ASSETS_RELOAD);
}
