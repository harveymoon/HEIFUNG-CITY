//! [districts] The District tool (9): paint named districts over the map
//! with a brush or a polygon, as in Cities: Skylines, and the district
//! names drawn over the map while the layer is on.
//!
//! Its state lives in `ToolState::district` so the toolbar's sub-pane can
//! reach it; the input runs as its own system after `tools::tool_input`,
//! which only switches tools. Painting applies at once (no project): see
//! `nd_sim::districts` for why.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use nd_sim::districts::COLORS;
use nd_sim::Vec2 as SVec2;

use crate::camera::{CameraMode, Cursor};
use crate::render::Overlays;
use crate::sim::{Sim, SimSet};
use crate::tools::{Tool, ToolState};
use crate::ui::UiFlags;

/// Brush or polygon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DistrictMode {
    #[default]
    Brush,
    Polygon,
}

/// Brush radius limits, metres.
pub const BRUSH_MIN: f32 = 8.0;
pub const BRUSH_MAX: f32 = 200.0;

#[derive(Clone, Debug)]
pub struct DistrictTool {
    /// The district painted; `None` starts a new one on the first stroke.
    pub selected: Option<u16>,
    pub mode: DistrictMode,
    pub radius: f32,
    /// Left button erases instead of painting.
    pub erase: bool,
    pub points: Vec<SVec2>,
    /// The button a brush stroke started with, while it is held.
    stroke: Option<(MouseButton, bool)>,
    /// A name typed into the sub-pane for renaming.
    pub rename: String,
}

impl Default for DistrictTool {
    fn default() -> Self {
        Self { selected: None, mode: DistrictMode::Brush, radius: 40.0, erase: false, points: Vec::new(), stroke: None, rename: String::new() }
    }
}

pub struct DistrictToolPlugin;

impl Plugin for DistrictToolPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (district_input, district_gizmos).chain().in_set(SimSet::Input).after(crate::tools::tool_input))
            .add_systems(EguiPrimaryContextPass, district_labels);
    }
}

/// The colour of a district as a Bevy colour (for gizmos).
pub fn district_color(sim: &Sim, id: u16) -> [f32; 3] {
    sim.world.districts.get(id).map_or([1.0, 1.0, 1.0], |d| COLORS[d.color as usize % COLORS.len()])
}

#[allow(clippy::too_many_arguments)]
fn district_input(
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    cursor: Res<Cursor>,
    flags: Res<UiFlags>,
    mode: Res<CameraMode>,
    mut tools: ResMut<ToolState>,
    mut sim: ResMut<Sim>,
    mut overlays: ResMut<Overlays>,
    mut was_on: Local<bool>,
    mut right_at: Local<Option<Vec2>>,
    window: Query<&Window, With<PrimaryWindow>>,
) {
    let on = tools.tool == Tool::District && *mode == CameraMode::Orbit;
    // Picking the tool shows the layer; leaving it leaves the layer as is.
    if on && !*was_on {
        overlays.districts = true;
    }
    *was_on = on;
    if !on {
        tools.district.stroke = None;
        return;
    }
    let ground = cursor.ground;
    let over_ui = flags.pointer_over_ui;
    let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let typing = flags.keyboard_captured;
    // Shift-click picks the district under the pointer.
    if shift && buttons.just_pressed(MouseButton::Left) && !over_ui {
        if let Some(d) = ground.and_then(|p| sim.world.districts.at(p)) {
            tools.district.selected = Some(d.id);
            tools.district.rename = d.name.clone();
            tools.status = format!("{} selected.", d.name);
        }
        return;
    }
    if !typing && keys.just_pressed(KeyCode::Escape) {
        tools.district.points.clear();
    }
    // The district a stroke paints: the selected one, else a new one.
    let target = |tools: &mut ToolState, sim: &mut Sim| -> Option<u16> {
        if let Some(id) = tools.district.selected.filter(|id| sim.world.districts.get(*id).is_some()) {
            return Some(id);
        }
        match sim.world.create_district(None) {
            Ok(id) => {
                tools.district.selected = Some(id);
                tools.district.rename = sim.world.districts.get(id).map(|d| d.name.clone()).unwrap_or_default();
                tools.status = format!("New district {}.", tools.district.rename);
                Some(id)
            }
            Err(e) => {
                tools.status = format!("No new district: {e}.");
                None
            }
        }
    };
    match tools.district.mode {
        DistrictMode::Brush => {
            if tools.district.stroke.is_none() && !over_ui && !shift {
                if buttons.just_pressed(MouseButton::Left) {
                    tools.district.stroke = Some((MouseButton::Left, tools.district.erase));
                } else if buttons.just_pressed(MouseButton::Right) {
                    tools.district.stroke = Some((MouseButton::Right, true));
                }
            }
            let Some((button, erase)) = tools.district.stroke else { return };
            if !buttons.pressed(button) {
                tools.district.stroke = None;
                return;
            }
            let Some(p) = ground else { return };
            let id = if erase { Some(0) } else { target(&mut tools, &mut sim) };
            if let Some(id) = id {
                let r = tools.district.radius;
                let _ = sim.world.paint_district_circle(p, r, id);
            }
        }
        DistrictMode::Polygon => {
            let pointer = window.single().ok().and_then(|w| w.cursor_position());
            if buttons.just_pressed(MouseButton::Right) {
                *right_at = if over_ui { None } else { pointer };
            }
            let right_click = buttons.just_released(MouseButton::Right) && matches!((right_at.take(), pointer), (Some(a), Some(b)) if a.distance(b) < 6.0);
            if buttons.just_pressed(MouseButton::Left) && !over_ui && !shift {
                if let Some(p) = ground {
                    tools.district.points.push(p);
                }
            }
            if !typing && !over_ui && keys.just_pressed(KeyCode::Backspace) {
                tools.district.points.pop();
            }
            let finish = right_click || (!typing && !over_ui && keys.just_pressed(KeyCode::Enter) && !keys.pressed(KeyCode::ControlLeft));
            if finish && tools.district.points.len() >= 3 {
                let erase = tools.district.erase;
                let id = if erase { Some(0) } else { target(&mut tools, &mut sim) };
                if let Some(id) = id {
                    let pts = std::mem::take(&mut tools.district.points);
                    let n = sim.world.paint_district_polygon(&pts, id).unwrap_or(0);
                    tools.status = format!("{} {n} cells.", if erase { "Erased" } else { "Painted" });
                }
            } else if finish {
                tools.district.points.clear();
            }
        }
    }
}

fn district_gizmos(mut gizmos: Gizmos, tools: Res<ToolState>, cursor: Res<Cursor>, sim: Res<Sim>) {
    if tools.tool != Tool::District {
        return;
    }
    let w = &sim.world;
    let lift = |p: SVec2| Vec3::new(p.x, w.surface_height(p) + 0.8, p.y);
    let c = tools.district.selected.map_or([0.6, 1.6, 1.8], |id| district_color(&sim, id));
    let col = if tools.district.erase { Color::linear_rgb(2.2, 1.2, 0.4) } else { Color::linear_rgb(c[0] * 1.4, c[1] * 1.4, c[2] * 1.4) };
    let flat = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
    match tools.district.mode {
        DistrictMode::Brush => {
            if let Some(p) = cursor.ground {
                gizmos.circle(Isometry3d::new(lift(p), flat), tools.district.radius, col);
            }
        }
        DistrictMode::Polygon => {
            let mut pts: Vec<Vec3> = tools.district.points.iter().map(|&p| lift(p)).collect();
            for p in &pts {
                gizmos.circle(Isometry3d::new(*p, flat), 2.0, col);
            }
            if let Some(p) = cursor.ground {
                pts.push(lift(p));
            }
            if pts.len() >= 3 {
                pts.push(pts[0]);
            }
            if pts.len() >= 2 {
                gizmos.linestrip(pts, col);
            }
        }
    }
}

/// District names over the map while the layer is on: each at its
/// district's centre, in its colour, with its policy underneath.
fn district_labels(mut contexts: EguiContexts, sim: Res<Sim>, overlays: Res<Overlays>, cams: Query<(&Camera, &GlobalTransform), With<Camera3d>>, mut cache: Local<(u64, usize, Vec<(u16, SVec2)>)>) -> Result {
    if !overlays.districts {
        return Ok(());
    }
    let w = &sim.world;
    if w.districts.list.is_empty() {
        return Ok(());
    }
    // Centroids change only with the districts.
    if cache.0 != w.districts.version || cache.1 != w.districts.list.len() || cache.2.is_empty() {
        let ext = w.districts.grid.extents();
        cache.2 = ext.iter().map(|(id, e)| (*id, e.centroid())).collect();
        cache.0 = w.districts.version;
        cache.1 = w.districts.list.len();
    }
    let Some((camera, gt)) = cams.iter().next() else { return Ok(()) };
    let ctx = contexts.ctx_mut()?;
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("district_labels")));
    for (id, at) in &cache.2 {
        let Some(d) = w.districts.get(*id) else { continue };
        let p3 = Vec3::new(at.x, w.surface_height(*at) + 6.0, at.y);
        let Ok(screen) = camera.world_to_viewport(gt, p3) else { continue };
        let c = COLORS[d.color as usize % COLORS.len()];
        let col = egui::Color32::from_rgb((c[0].min(1.0) * 255.0) as u8, (c[1].min(1.0) * 255.0) as u8, (c[2].min(1.0) * 255.0) as u8);
        let pos = egui::pos2(screen.x, screen.y);
        let font = egui::FontId::proportional(17.0);
        // A dark halo so the name reads over neon.
        for off in [egui::vec2(1.5, 1.5), egui::vec2(-1.5, 1.5), egui::vec2(1.5, -1.5), egui::vec2(-1.5, -1.5)] {
            painter.text(pos + off, egui::Align2::CENTER_CENTER, &d.name, font.clone(), egui::Color32::from_black_alpha(220));
        }
        painter.text(pos, egui::Align2::CENTER_CENTER, &d.name, font, col);
        let policy = d.policy_label();
        painter.text(pos + egui::vec2(0.0, 15.0), egui::Align2::CENTER_CENTER, policy, egui::FontId::proportional(11.5), egui::Color32::from_gray(210));
    }
    Ok(())
}
