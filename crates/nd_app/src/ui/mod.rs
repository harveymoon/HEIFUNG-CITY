//! egui HUD in the Cities: Skylines mould: a thin top bar (clock, speed,
//! treasury, population, City Data and the tool-window toggles, camera
//! mode), a bottom toolbar with one big button per tool and a sub-pane
//! holding only the active tool's parameters, draggable tool windows, and
//! the full-screen City Data view for every report on the city. This file owns the plugin, the
//! input flags the rest of the app reads, and the frame skeleton; each piece
//! of the HUD lives in its own module.

// The Build window: every road, zone and building in one catalogue.
/// [r4-tiles] Buying land: the Land for sale labels and window.
mod land;
pub mod build;
/// [pt-ux] The Alerts window: the toasts as they happened, the problems now.
pub mod alerts;
/// [ferry-routes] A dock's routes and the ship card in the inspector.
mod ferry;
/// [waterfront] The inspector's waterfront and beach lines.
mod shore;
/// [parks] (D5) The Parks page and a park's inspector section.
mod parks;
/// [committee] The Committee window.
pub mod committee;
/// [voices] A citizen's diary, on demand.
pub mod diary;
/// [talk] Chat with the selected citizen (in the person pane).
pub mod talk;
/// [concerns] City Data's People, Complaints and concerns.
mod concerns;
/// [city-data] The full-screen City Data view: tabs, sub-tabs, pages.
pub mod city_data;
mod detail;
/// [r4-leisure] Demand for new buildings: City Data's panel and its window.
pub mod demand;
/// [directory] City Data's Directory: every business and person, sortable.
pub mod directory;
mod economy;
/// [districts] The District tool's pane and City Data's Districts and Tourism pages.
mod districts;
mod emergencies;
/// [camera] Follow / Zoom to buttons, the gift button, the locked banner.
pub mod follow;
/// Pure text formats: numbers, money, plurals, times.
pub mod fmt;
/// [land] Ownership, sale price and merged plots in the inspector.
mod estate;
mod help;
/// [identity] The Game history window: autosaves, bookmarks, revert.
pub mod history;
mod hover;
mod inspector;
/// [ledger] Accounts in the inspectors, Economy, Money flow.
mod ledger;
pub mod newgame;
pub mod pane;
pub mod people;
mod person;
/// [camera] A selected person's home / work / owned pins.
mod pins;
// [photo] F10 / `photo_mode`: hide all of egui for clean screenshots.
mod photo;
mod places;
/// [social] Friends and foes, the Social tab, speech bubbles.
pub mod social;
/// [economy] What the city's businesses want, and why a business is here.
mod pressure;
/// [inspector] Painted head-and-shoulders portraits of citizens.
mod portrait;
pub mod saves;
mod taxes;
/// [toll-inspect] The inspector on a toll booth.
mod toll;
/// [road-inspect] The card on a selected street piece.
mod road;
/// [supertall] The skyline: inspector line, City Data page, news toasts.
mod skyline;
/// [fonts] The HUD's fonts and type scale.
pub mod theme;
/// The title screen: saved games with their map pictures.
mod title;
mod icons;
pub mod legend;
pub mod junction;
/// [lines] The Lines window.
pub(crate) mod lines;
/// [events] Street races, raves and road maintenance in the HUD.
mod nights;
/// [laptop-nav] The on-screen navigation pad and the navigation settings.
pub mod nav_pad;
mod toolbar;
/// [fix/roads-tools] The plan bar: Implement / Discard while a plan is pending.
mod plan_bar;
pub(crate) mod tools;
mod topbar;
pub mod train;
mod windows;

use bevy::diagnostic::DiagnosticsStore;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPreUpdateSet, EguiPrimaryContextPass};

use crate::assets::GameAssets;
use crate::camera::{CameraFly, CameraFollow, CameraMode, Cursor, FlyGoal, OrbitCamera};
use crate::render::{Lighting, Overlays, RenderStats, Thumbs};
use crate::sim::{Sim, SimSet};
use crate::tools::{EffectivePlan, Selection, Tool, ToolState};

pub use saves::{load_now, save_as, save_now, Saves, LATEST};
/// For the remote API's `window` command.
pub use windows::Info;
/// For the remote API's car detail: everyone aboard a vehicle.
pub use inspector::occupants;

/// What the UI wants the rest of the app to know about input this frame.
#[derive(Resource, Default)]
pub struct UiFlags {
    /// The pointer is over a bar, a window, or a widget egui is dragging:
    /// clicks, the wheel and click-like keys belong to egui, not the map.
    pub pointer_over_ui: bool,
    /// An egui text field has focus: keys are typing, not hotkeys.
    pub keyboard_captured: bool,
    /// A "Flag for Claude" button was pressed: `flags::flag_box` opens the
    /// note box on its next pass.
    pub want_flag: bool,
    /// One-line status, shown as a toast above the toolbar that fades after
    /// a few seconds. Writers just assign; the fade restarts on a change.
    pub message: String,
    /// The bars' rectangles from the last pass: the top bar and the bottom
    /// toolbar with its sub-pane. The bars sit on the background layer,
    /// which egui's own hit test ignores, so they are tested by hand; the
    /// windows are on ordinary layers and `is_pointer_over_egui` sees them.
    panel_rects: Vec<egui::Rect>,
    /// The message the toast is showing and the egui time it appeared, so a
    /// new message restarts the fade and an old one stays faded out.
    toast: Option<(String, f64)>,
    /// [events] The last city news line toasted (`nights::news_toast`).
    news_seen: u32,
    /// [social] The last social news line toasted.
    social_seen: u32,
    /// [supertall] The last skyline news line toasted.
    skyline_seen: u32,
    /// [city-data] The City Data view covered the map last pass: Esc is
    /// its, not the tools' or the camera's.
    pub data_view: bool,
    /// [pt-ux] Where the thing `message` is about happened, when its writer
    /// knows (city news): the Alerts window's "go". Taken with the message.
    pub message_at: Option<bevy::math::Vec2>,
    /// [pt-ux] Every toast as it showed, the newest last (the Alerts window).
    pub log: Vec<alerts::Logged>,
    /// [fix/roads-tools] The plan bar's Discard was pressed once: it asks.
    pub plan_confirm: bool,
    /// [r4-hunger] The held business pressure whose lot need the map shows
    /// ("show where", `pressure_where`).
    pub pressure_where: Option<nd_sim::pressure::PressureKind>,
    /// [photo] Photo mode: egui still runs but nothing it lays out is
    /// drawn (`photo.rs`; F10 or the remote `photo_mode`).
    pub photo_mode: bool,
}

impl UiFlags {
    /// [metro-ux] The map between the top bar and the bottom toolbar (with
    /// its sub-pane) as laid out last pass, for painting over the map only.
    pub fn map_rect(&self, full: egui::Rect) -> egui::Rect {
        let top = self.panel_rects.first().map_or(full.top(), |r| r.bottom());
        let bottom = self.panel_rects.get(1).map_or(full.bottom(), |r| r.top());
        egui::Rect::from_min_max(egui::pos2(full.left(), top), egui::pos2(full.right(), bottom.max(top + 1.0)))
    }
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiFlags>()
            .init_resource::<Saves>()
            .add_plugins(photo::PhotoPlugin)
            // [fonts] Before the pass begins, so the first pass already has them.
            .add_systems(PreUpdate, install_fonts.after(EguiPreUpdateSet::InitContexts).before(EguiPreUpdateSet::BeginPass))
            .add_systems(PreUpdate, read_input_flags.after(EguiPreUpdateSet::BeginPass))
            .add_systems(Update, saves::autosave.in_set(SimSet::Sync))
            .add_systems(EguiPrimaryContextPass, (side_panel, pins::map_pins.after(side_panel), social::bubbles.after(side_panel), junction::hover_button.after(side_panel), junction::on_map.after(side_panel), junction::hold_time.after(side_panel), land::land_window.after(side_panel), nav_pad::pad.after(side_panel)));
    }
}

/// [fonts] Fonts and the type scale (`theme`), once the context exists.
/// egui applies new fonts when a pass begins, so this runs before
/// `BeginPass`: no pass ever lays out text in a family not yet loaded.
fn install_fonts(mut contexts: EguiContexts, mut done: Local<bool>) {
    if *done {
        return;
    }
    if let Ok(ctx) = contexts.ctx_mut() {
        theme::install(ctx);
        *done = true;
    }
}

/// Publish `pointer_over_ui` and `keyboard_captured` before the camera and
/// the tools run. egui's pass begins in `PreUpdate`, so by now its input
/// holds this frame's pointer position, tested against the areas laid out
/// in the previous pass: the same-frame answer the tools need, rather than
/// a flag written at the end of the previous frame's pass.
pub(crate) fn read_input_flags(mut contexts: EguiContexts, mut flags: ResMut<UiFlags>) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let over_bars = ctx.pointer_latest_pos().is_some_and(|p| flags.panel_rects.iter().any(|r| r.contains(p)));
    flags.pointer_over_ui = over_bars || ctx.is_pointer_over_egui() || ctx.egui_wants_pointer_input();
    // [photo] Panes that are not drawn do not hold the pointer: the map's
    // camera drags and wheel work across the whole window.
    if flags.photo_mode {
        flags.pointer_over_ui = false;
    }
    // Only a text field keeps focus across frames (see the surrender below),
    // so this is true exactly while one is being typed into.
    flags.keyboard_captured = ctx.egui_wants_keyboard_input();
}

/// Everything the HUD reads or writes, borrowed for one pass.
pub struct UiCtx<'a> {
    pub sim: &'a mut Sim,
    pub tools: &'a mut ToolState,
    pub selection: &'a mut Selection,
    pub plan: &'a mut EffectivePlan,
    pub saves: &'a mut Saves,
    pub flags: &'a mut UiFlags,
    pub overlays: &'a mut Overlays,
    pub lighting: &'a mut Lighting,
    pub mode: &'a mut CameraMode,
    /// Where a "go" button asked the camera to fly; the caller turns it
    /// into a smooth move.
    pub fly_to: Option<Vec2>,
    /// [camera] The camera's follow and its smooth moves, for the Follow
    /// and Zoom to buttons.
    pub follow: &'a mut CameraFollow,
    pub fly: &'a mut CameraFly,
    pub assets: &'a GameAssets,
    pub stats: &'a RenderStats,
    pub diagnostics: &'a DiagnosticsStore,
    /// Rendered thumbnails of the buildings, for the Build window.
    pub thumbs: &'a Thumbs,
    /// [build-highlight] The Build window's section on the map: the window
    /// sets what to show and reads back the summary.
    pub highlight: &'a mut crate::build_highlight::BuildHighlight,
    /// [laptop-nav] The navigation settings (Settings window) and this
    /// frame's input (what the last scroll was).
    pub nav: &'a mut crate::nav::NavSettings,
    pub nav_input: &'a crate::nav::NavInput,
}

/// The whole HUD, once per frame. The name is historical: this used to lay
/// out one side panel and now lays out the bars and the windows, but it is
/// still the single system the rest of the app knows the UI by.
#[allow(clippy::too_many_arguments)]
pub(crate) fn side_panel(
    mut contexts: EguiContexts,
    mut styled: Local<bool>,
    mut sim: ResMut<Sim>,
    mut tools: ResMut<ToolState>,
    mut selection: ResMut<Selection>,
    mut plan: ResMut<EffectivePlan>,
    mut saves: ResMut<Saves>,
    mut flags: ResMut<UiFlags>,
    mut overlays: ResMut<Overlays>,
    mut lighting: ResMut<Lighting>,
    mut mode: ResMut<CameraMode>,
    cursor: Res<Cursor>,
    cams: Query<&OrbitCamera>,
    assets: Res<GameAssets>,
    (stats, diagnostics, thumbs, mut committee): (Res<RenderStats>, Res<DiagnosticsStore>, Res<Thumbs>, Option<ResMut<crate::committee::CommitteeRt>>),
    (mut follow, mut fly, mut highlight, mut nav, nav_input): (ResMut<CameraFollow>, ResMut<CameraFly>, ResMut<crate::build_highlight::BuildHighlight>, ResMut<crate::nav::NavSettings>, Res<crate::nav::NavInput>),
) -> Result {
    let ctx = contexts.ctx_mut()?;
    // Visuals persist in the context; setting them every frame only churns.
    if !*styled {
        style(ctx);
        *styled = true;
    }
    let view = ctx.viewport_rect();
    let mut root = egui::Ui::new(ctx.clone(), "viewport".into(), egui::UiBuilder::new().layer_id(egui::LayerId::background()).max_rect(view));

    let mut p = UiCtx {
        sim: &mut sim,
        tools: &mut tools,
        selection: &mut selection,
        plan: &mut plan,
        saves: &mut saves,
        flags: &mut flags,
        overlays: &mut overlays,
        lighting: &mut lighting,
        mode: &mut mode,
        fly_to: None,
        follow: &mut follow,
        fly: &mut fly,
        assets: &assets,
        stats: &stats,
        diagnostics: &diagnostics,
        thumbs: &thumbs,
        highlight: &mut highlight,
        nav: &mut nav,
        nav_input: &nav_input,
    };
    let top = egui::Panel::top("top_bar").frame(bar_frame()).show(&mut root, |ui| topbar::show(ui, &mut p));
    let bottom = egui::Panel::bottom("tool_bar").frame(bar_frame()).show(&mut root, |ui| toolbar::show(ui, &mut p));
    let (top_rect, bottom_rect) = (top.response.rect, bottom.response.rect);
    // The map area between the bars: windows open inside it and are kept
    // from being dragged under either bar.
    let content = egui::Rect::from_min_max(egui::pos2(view.left(), top_rect.bottom()), egui::pos2(view.right(), bottom_rect.top()));
    // [city-data] Its keys first (J, Esc), then the view under the windows.
    city_data::keys(ctx);
    let data_rect = city_data::show(ctx, &mut p, content);
    windows::show_all(ctx, &mut p, content);
    // [lines] Editing a line: the cursor over the map says what a click does.
    lines::edit_cursor(ctx, p.sim, p.tools, ctx.is_pointer_over_egui());
    build::window(ctx, &mut p, content);
    inspector::window(ctx, &mut p, content);
    toll::window(ctx, &mut p, content);
    road::window(ctx, &mut p, content);
    newgame::window(ctx, &mut p, content);
    history::window(ctx, &mut p, content);
    title::screen(ctx, &mut p);
    // [tools] The Intersection editor's panel (its arrows are on the map).
    junction::window(ctx, &mut p);
    // [fix/roads-tools] A plan not built yet: the bar at the top (G2).
    if !city_data::is_open(ctx) {
        plan_bar::show(ctx, &mut p, top_rect.bottom());
    }
    // [walk-networks] A layer or a place asked for by a legend row, an
    // inspector line or an alert.
    if let Some(at) = crate::map_layers::take_requests(ctx, p.overlays) {
        p.fly_to = Some(Vec2::new(at.x, at.y));
    }
    let fly_to = p.fly_to;

    flags.panel_rects = vec![top_rect, bottom_rect];
    // [city-data] The view sits on the background layer, which egui's
    // pointer test ignores: test it by hand like the bars.
    flags.panel_rects.extend(data_rect);
    flags.data_view = data_rect.is_some();
    nights::news_toast(&sim.world, &mut flags);
    social::news_toast(&sim.world, &mut flags);
    skyline::news_toast(&sim.world, &mut flags);
    let tick = sim.world.clock.tick;
    toast(ctx, &mut flags, bottom_rect.height(), tick);
    // The land layer's reading under the pointer, for the legend.
    let here = overlays.land.zip(cursor.ground).map(|(l, p)| legend::land_here(&sim.world, l, p));
    if data_rect.is_none() {
        legend::show(ctx, &overlays, &assets, &sim.world, content, here.as_deref());
    }
    // [camera] "go" buttons fly smoothly, and end a follow.
    if let Some(p) = fly_to {
        follow.stop();
        *mode = CameraMode::Orbit;
        let d = cams.iter().next().map_or(70.0, |c| c.distance.min(70.0));
        fly.to(FlyGoal::Point(p), Some(d), Some(0.7));
    }

    person::pane(ctx, &mut sim, &mut tools, &mut selection, &mut flags, content, (&mut follow, &mut fly, &mut mode), committee.as_deref_mut());
    if data_rect.is_none() {
        follow::banner(ctx, &sim, &mut follow, &mut fly, &mut mode, &mut selection, content);
    }
    if tools.tool == Tool::Inspect && *mode == CameraMode::Orbit && data_rect.is_none() {
        // [pt-ux] Underground, only what that view draws.
        if tools.depth == crate::tools::Depth::Underground {
            hover::under_card(ctx, &sim, &cursor);
        } else {
            hover::card(ctx, &sim, &cursor);
        }
    }
    // [plant-groups] What the picked building would be on the lot under the pointer.
    if tools.tool == Tool::Buildings && *mode == CameraMode::Orbit && !ctx.is_pointer_over_egui() && data_rect.is_none() {
        hover::build_card(ctx, &sim, &cursor, &tools);
    }

    // egui keeps keyboard focus on the last clicked widget and re-clicks it
    // on Space or Enter, so a Space to pause would also toggle the last
    // checkbox. Drop the focus each frame, except from a text field, which
    // needs it to take typing (and then `keyboard_captured` holds the
    // hotkeys off instead).
    if !ctx.text_edit_focused() {
        ctx.memory_mut(|m| {
            if let Some(id) = m.focused() {
                m.surrender_focus(id);
            }
        });
    }
    Ok(())
}

/// How long a toast stays fully visible, then how long it takes to fade.
const TOAST_HOLD: f64 = 5.0;
const TOAST_FADE: f64 = 1.5;

/// `UiFlags::message` as a toast centred above the toolbar. The message is
/// never cleared by its writers (saves, hotkeys), so the toast keeps its own
/// clock: a changed text restarts it, the same text fades out once.
fn toast(ctx: &egui::Context, flags: &mut UiFlags, above: f32, tick: u64) {
    if flags.message.is_empty() {
        return;
    }
    let now = ctx.input(|i| i.time);
    if flags.toast.as_ref().is_none_or(|(text, _)| *text != flags.message) {
        flags.toast = Some((flags.message.clone(), now));
        // [pt-ux] Logged for the Alerts window.
        let at = flags.message_at.take();
        alerts::push(&mut flags.log, alerts::Logged { text: flags.message.clone(), tick, at });
    }
    let Some((text, at)) = &flags.toast else { return };
    let age = now - at;
    if age > TOAST_HOLD + TOAST_FADE {
        return;
    }
    let alpha = ((TOAST_HOLD + TOAST_FADE - age) / TOAST_FADE).clamp(0.0, 1.0) as f32;
    let text = text.clone();
    egui::Area::new(egui::Id::new("toast"))
        .order(egui::Order::Tooltip)
        .interactable(false)
        .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -(above + 10.0)))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(theme::PANEL_FILL.gamma_multiply(alpha))
                .stroke(egui::Stroke::new(1.0, GOLD.gamma_multiply(alpha)))
                .show(ui, |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(text).color(IVORY.gamma_multiply(alpha))).wrap_mode(egui::TextWrapMode::Extend));
                });
        });
}

/// The bars' frame: the panel fill, a little tighter than egui's default.
fn bar_frame() -> egui::Frame {
    egui::Frame::new().fill(theme::PANEL_FILL).inner_margin(egui::Margin::symmetric(10, 4))
}

/// The HUD's accent, standout and worry colours (`theme`), for every panel.
pub use theme::{GOLD, IVORY, WARN};

fn neon(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(IVORY).strong());
}

pub(crate) fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(egui::RichText::new(text).small().strong().color(GOLD));
}

/// Small, weak help text.
pub(crate) fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).small().weak());
}

fn style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = theme::PANEL_FILL;
    // Windows overlap each other, so they are opaque: egui blends in linear
    // space here, where even a 2% leak shows the text of the window behind
    // as legible grey. The bars sit over the map only and may stay sheer.
    visuals.window_fill = egui::Color32::from_rgb(8, 10, 20);
    visuals.window_stroke = egui::Stroke::new(1.0, theme::EDGE);
    visuals.selection.bg_fill = egui::Color32::from_rgb(120, 85, 30);
    visuals.hyperlink_color = GOLD;
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(150, 105, 40);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(70, 55, 35);
    ctx.set_visuals(visuals);
}
