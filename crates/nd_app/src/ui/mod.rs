//! egui HUD in the Cities: Skylines mould: a thin top bar (clock, speed,
//! treasury, population, the info-window toggles, camera mode), a bottom
//! toolbar with one big button per tool and a sub-pane holding only the
//! active tool's parameters, and draggable info windows for everything that
//! used to scroll past in the side panel. This file owns the plugin, the
//! input flags the rest of the app reads, and the frame skeleton; each piece
//! of the HUD lives in its own module.

/// [concerns] The People window's complaints tab.
/// The Build window: every road, zone and building in one catalogue.
pub mod build;
mod concerns;
mod detail;
mod economy;
/// [districts] The District tool's pane and the Districts window.
mod districts;
mod emergencies;
mod help;
/// [identity] The Game history window: autosaves, bookmarks, revert.
pub mod history;
mod hover;
mod inspector;
pub mod newgame;
pub mod pane;
pub mod people;
mod person;
mod places;
/// [economy] What the city's businesses want, and why a business is here.
mod pressure;
/// [inspector] Painted head-and-shoulders portraits of citizens.
mod portrait;
pub mod saves;
mod taxes;
/// The title screen: saved games with their map pictures.
mod title;
mod icons;
pub mod legend;
pub mod junction;
/// [lines] The Lines window.
mod lines;
mod toolbar;
mod tools;
mod topbar;
mod windows;

use bevy::diagnostic::DiagnosticsStore;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPreUpdateSet, EguiPrimaryContextPass};

use crate::assets::GameAssets;
use crate::camera::{CameraMode, Cursor, OrbitCamera};
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
}

pub struct UiPlugin;

impl Plugin for UiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiFlags>()
            .init_resource::<Saves>()
            .add_systems(PreUpdate, read_input_flags.after(EguiPreUpdateSet::BeginPass))
            .add_systems(Update, saves::autosave.in_set(SimSet::Sync))
            .add_systems(EguiPrimaryContextPass, side_panel);
    }
}

/// Publish `pointer_over_ui` and `keyboard_captured` before the camera and
/// the tools run. egui's pass begins in `PreUpdate`, so by now its input
/// holds this frame's pointer position, tested against the areas laid out
/// in the previous pass: the same-frame answer the tools need, rather than
/// a flag written at the end of the previous frame's pass.
fn read_input_flags(mut contexts: EguiContexts, mut flags: ResMut<UiFlags>) {
    let Ok(ctx) = contexts.ctx_mut() else { return };
    let over_bars = ctx.pointer_latest_pos().is_some_and(|p| flags.panel_rects.iter().any(|r| r.contains(p)));
    flags.pointer_over_ui = over_bars || ctx.is_pointer_over_egui() || ctx.egui_wants_pointer_input();
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
    /// Where the favourites' "go" button asked the camera to fly; the
    /// caller applies it (a `Query` borrowed here would pin its lifetimes).
    pub fly_to: Option<Vec2>,
    pub assets: &'a GameAssets,
    pub stats: &'a RenderStats,
    pub diagnostics: &'a DiagnosticsStore,
    /// Rendered thumbnails of the buildings, for the Build window.
    pub thumbs: &'a Thumbs,
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
    mut cams: Query<&mut OrbitCamera>,
    assets: Res<GameAssets>,
    (stats, diagnostics, thumbs): (Res<RenderStats>, Res<DiagnosticsStore>, Res<Thumbs>),
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
        assets: &assets,
        stats: &stats,
        diagnostics: &diagnostics,
        thumbs: &thumbs,
    };
    let top = egui::Panel::top("top_bar").frame(bar_frame()).show(&mut root, |ui| topbar::show(ui, &mut p));
    let bottom = egui::Panel::bottom("tool_bar").frame(bar_frame()).show(&mut root, |ui| toolbar::show(ui, &mut p));
    let (top_rect, bottom_rect) = (top.response.rect, bottom.response.rect);
    // The map area between the bars: windows open inside it and are kept
    // from being dragged under either bar.
    let content = egui::Rect::from_min_max(egui::pos2(view.left(), top_rect.bottom()), egui::pos2(view.right(), bottom_rect.top()));
    windows::show_all(ctx, &mut p, content);
    build::window(ctx, &mut p, content);
    inspector::window(ctx, &mut p, content);
    newgame::window(ctx, &mut p, content);
    history::window(ctx, &mut p, content);
    title::screen(ctx, &mut p);
    // [tools] The Intersection editor (a modal).
    junction::window(ctx, &mut p);
    let fly_to = p.fly_to;

    flags.panel_rects = vec![top_rect, bottom_rect];
    toast(ctx, &mut flags, bottom_rect.height());
    // The land layer's reading under the pointer, for the legend.
    let here = overlays.land.zip(cursor.ground).map(|(l, p)| legend::land_here(&sim.world, l, p));
    legend::show(ctx, &overlays, &assets, &sim.world, content, here.as_deref());
    if let Some(p) = fly_to {
        for mut cam in &mut cams {
            cam.target = Vec3::new(p.x, 0.0, p.y);
            cam.distance = cam.distance.min(70.0);
            cam.pitch = 0.7;
        }
    }

    person::pane(ctx, &mut sim, &mut tools, &mut selection, &mut flags, content);
    if tools.tool == Tool::Inspect && *mode == CameraMode::Orbit {
        hover::card(ctx, &sim, &cursor);
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
fn toast(ctx: &egui::Context, flags: &mut UiFlags, above: f32) {
    if flags.message.is_empty() {
        return;
    }
    let now = ctx.input(|i| i.time);
    if flags.toast.as_ref().is_none_or(|(text, _)| *text != flags.message) {
        flags.toast = Some((flags.message.clone(), now));
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
                .fill(egui::Color32::from_rgba_unmultiplied(8, 10, 20, 235).gamma_multiply(alpha))
                .stroke(egui::Stroke::new(1.0, GOLD.gamma_multiply(alpha)))
                .show(ui, |ui| {
                    ui.add(egui::Label::new(egui::RichText::new(text).color(IVORY.gamma_multiply(alpha))).wrap_mode(egui::TextWrapMode::Extend));
                });
        });
}

/// The bars' frame: the panel fill, a little tighter than egui's default.
fn bar_frame() -> egui::Frame {
    egui::Frame::new().fill(egui::Color32::from_rgba_unmultiplied(8, 10, 20, 235)).inner_margin(egui::Margin::symmetric(10, 4))
}

/// The accent colour of the HUD: headings, the active tool, window titles.
pub const GOLD: egui::Color32 = egui::Color32::from_rgb(235, 180, 90);
/// Light, slightly warm text for names and figures that should stand out.
pub const IVORY: egui::Color32 = egui::Color32::from_rgb(230, 225, 210);
/// Colour for a figure that should worry the player.
pub const WARN: egui::Color32 = egui::Color32::from_rgb(255, 120, 90);

fn neon(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(IVORY).strong());
}

fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(egui::RichText::new(text).small().strong().color(GOLD));
}

/// Small, weak help text.
fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).small().weak());
}

fn zone_color32(c: [f32; 3]) -> egui::Color32 {
    egui::Color32::from_rgb((c[0].min(1.0) * 255.0) as u8, (c[1].min(1.0) * 255.0) as u8, (c[2].min(1.0) * 255.0) as u8)
}

fn style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgba_unmultiplied(8, 10, 20, 235);
    // Windows overlap each other, so they are opaque: egui blends in linear
    // space here, where even a 2% leak shows the text of the window behind
    // as legible grey. The bars sit over the map only and may stay sheer.
    visuals.window_fill = egui::Color32::from_rgb(8, 10, 20);
    visuals.window_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(95, 75, 45));
    visuals.selection.bg_fill = egui::Color32::from_rgb(120, 85, 30);
    visuals.hyperlink_color = GOLD;
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(150, 105, 40);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(70, 55, 35);
    ctx.set_visuals(visuals);
}
