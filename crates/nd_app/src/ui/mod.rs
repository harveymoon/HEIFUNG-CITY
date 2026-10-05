//! egui side panel: clock, speed, project, tools, stats, inspector, save/load.
//! This file owns the plugin, the input flags the rest of the app reads, and
//! the frame skeleton; each section of the panel lives in its own module.

mod help;
mod hover;
mod inspector;
mod panel;
mod person;
mod saves;
mod tools;

use bevy::diagnostic::DiagnosticsStore;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPreUpdateSet, EguiPrimaryContextPass};

use crate::assets::GameAssets;
use crate::camera::{CameraMode, Cursor, OrbitCamera};
use crate::render::{Lighting, Overlays, RenderStats};
use crate::sim::{Sim, SimSet};
use crate::tools::{EffectivePlan, Selection, Tool, ToolState};

pub use saves::{load_now, save_now, Saves, LATEST};

/// What the UI wants the rest of the app to know about input this frame.
#[derive(Resource, Default)]
pub struct UiFlags {
    /// The pointer is over the panel, a window, or a widget egui is dragging:
    /// clicks, the wheel and click-like keys belong to egui, not the map.
    pub pointer_over_ui: bool,
    /// An egui text field has focus: keys are typing, not hotkeys.
    pub keyboard_captured: bool,
    /// One-line status shown at the foot of the panel.
    pub message: String,
    /// The side panel's rectangle from the last pass. The panel sits on the
    /// background layer, which egui's own hit test ignores, so it is tested
    /// by hand.
    panel_rect: Option<egui::Rect>,
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
    let over_panel = flags.panel_rect.zip(ctx.pointer_latest_pos()).is_some_and(|(r, p)| r.contains(p));
    flags.pointer_over_ui = over_panel || ctx.is_pointer_over_egui() || ctx.egui_wants_pointer_input();
    // Only a text field keeps focus across frames (see the surrender below),
    // so this is true exactly while one is being typed into.
    flags.keyboard_captured = ctx.egui_wants_keyboard_input();
}

#[allow(clippy::too_many_arguments)]
fn side_panel(
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
    stats: Res<RenderStats>,
    diagnostics: Res<DiagnosticsStore>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    // Visuals persist in the context; setting them every frame only churns.
    if !*styled {
        style(ctx);
        *styled = true;
    }
    let mut root = egui::Ui::new(
        ctx.clone(),
        "viewport".into(),
        egui::UiBuilder::new().layer_id(egui::LayerId::background()).max_rect(ctx.viewport_rect()),
    );

    let mut p = panel::PanelCtx {
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
    };
    let shown = egui::Panel::left("neon_panel").default_size(340.0).show(&mut root, |ui| p.show(ui));
    let fly_to = p.fly_to;
    flags.panel_rect = Some(shown.response.rect);
    if let Some(p) = fly_to {
        for mut cam in &mut cams {
            cam.target = Vec3::new(p.x, 0.0, p.y);
            cam.distance = cam.distance.min(70.0);
            cam.pitch = 0.7;
        }
    }

    person::pane(ctx, &mut sim, &mut tools, &mut selection);
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

fn neon(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(egui::Color32::from_rgb(230, 225, 210)).strong());
}

fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(egui::RichText::new(text).small().strong().color(egui::Color32::from_rgb(235, 180, 90)));
}

/// Small, weak help text.
fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).small().weak());
}

fn zone_color32(c: [f32; 3]) -> egui::Color32 {
    egui::Color32::from_rgb((c[0].min(1.0) * 255.0) as u8, (c[1].min(1.0) * 255.0) as u8, (c[2].min(1.0) * 255.0) as u8)
}

const WARN: egui::Color32 = egui::Color32::from_rgb(255, 120, 90);

fn style(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = egui::Color32::from_rgba_unmultiplied(8, 10, 20, 235);
    visuals.window_fill = egui::Color32::from_rgba_unmultiplied(8, 10, 20, 235);
    visuals.selection.bg_fill = egui::Color32::from_rgb(120, 85, 30);
    visuals.hyperlink_color = egui::Color32::from_rgb(235, 180, 90);
    visuals.widgets.active.bg_fill = egui::Color32::from_rgb(150, 105, 40);
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(70, 55, 35);
    ctx.set_visuals(visuals);
}
