//! The top bar: title, clock and speed, treasury and population, the
//! toggles that open the info windows, a Dev menu, and the camera mode.
//! One row, read left to right like a city builder's status strip.

use bevy::diagnostic::FrameTimeDiagnosticsPlugin;
use bevy_egui::egui;

use super::windows::Info;
use super::{help, hint, neon, UiCtx, GOLD, WARN};
use crate::camera::CameraMode;
use crate::sim::SPEEDS;

pub fn show(ui: &mut egui::Ui, p: &mut UiCtx) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(egui::RichText::new("DISTRICT 2040").color(GOLD).strong().size(15.0));
        ui.separator();
        clock_and_speed(ui, p);
        ui.separator();
        treasury_and_population(ui, p);
        ui.separator();
        window_toggles(ui);
        ui.separator();
        dev_menu(ui, p);
        // The camera switch hugs the right edge whatever the window width.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| camera(ui, p));
    });
}

fn clock_and_speed(ui: &mut egui::Ui, p: &mut UiCtx) {
    let sim = &mut *p.sim;
    neon(ui, &format!("Day {}  {}", sim.world.clock.day() + 1, sim.world.clock.format_hhmm()));
    let label = if sim.paused { "▶" } else { "⏸" };
    if ui.button(label).on_hover_text(if sim.paused { "Run [Space]" } else { "Pause [Space]" }).clicked() {
        sim.paused = !sim.paused;
    }
    for (i, (label, _)) in SPEEDS.iter().enumerate() {
        if ui.selectable_label(sim.speed_index == i, *label).on_hover_text("- / = change speed").clicked() {
            sim.speed_index = i;
        }
    }
    // Effective speed: the share of the chosen speed the sim is actually
    // delivering. Below 0.9 the tick budget is throttling it, and that is
    // worth a red figure in the bar; at full speed it says nothing.
    let eff = sim.effective_speed;
    if sim.paused {
        hint(ui, "paused");
    } else if eff < 0.9 {
        ui.label(egui::RichText::new(format!("{:.0}%", eff * 100.0)).small().color(WARN)).on_hover_text(format!("The sim is running at {:.0}% of {}: the tick budget is throttling it.", eff * 100.0, SPEEDS[sim.speed_index].0));
    }
}

fn treasury_and_population(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    let e = &w.economy;
    let treasury = egui::RichText::new(format!("₡ {:.0}", e.treasury)).strong();
    ui.label(if e.treasury < 0.0 { treasury.color(WARN) } else { treasury.color(GOLD) }).on_hover_text(format!("treasury · taxes so far today {:.0}", e.tax_today));
    ui.label(egui::RichText::new(format!("pop {}", w.citizens.len())).strong()).on_hover_text(format!("{} homes · {} jobs · {} waiting to move in", e.homes_total, e.jobs_total, e.waiting_immigrants));
    let power = &e.power;
    if power.served < 0.98 {
        ui.label(egui::RichText::new(format!("⚡ {:.0}%", power.served * 100.0)).small().color(WARN)).on_hover_text("Power served: open the Power window");
    }
}

/// One toggle per info window; lit while the window is open.
fn window_toggles(ui: &mut egui::Ui) {
    for info in Info::ALL {
        if info == Info::Saves {
            ui.separator();
        }
        let open = info.is_open(ui.ctx());
        if ui.selectable_label(open, info.title()).clicked() {
            info.set_open(ui.ctx(), !open);
        }
    }
}

/// Developer odds and ends: the demo seed, the mind backend, frame and mesh
/// figures, asset errors. Out of the way of play but one click off.
fn dev_menu(ui: &mut egui::Ui, p: &mut UiCtx) {
    ui.menu_button("Dev", |ui| {
        ui.set_min_width(260.0);
        if *p.mode == CameraMode::Orbit {
            let has_buildings = !p.sim.world.buildings.is_empty();
            let label = if has_buildings { "Seed demo district anyway" } else { "Seed demo district [G]" };
            if ui.button(label).clicked() {
                p.sim.seed_demo_district();
                p.tools.status = "Demo district laid out around the centre".into();
                ui.close();
            }
            if has_buildings {
                hint(ui, help::SEED_EXISTING);
            }
            ui.separator();
        }
        hint(ui, &format!("minds: {} · backend: {}", nd_sim::MindProvider::name(&p.sim.minds), p.sim.minds.backend_name()));
        let fps = p.diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()).unwrap_or(0.0);
        hint(ui, &format!("{fps:.0} fps · {} ticks/frame", p.sim.ticks_last_frame));
        hint(ui, &format!("{} building meshes · {}k triangles", p.stats.building_entities, (p.stats.street_triangles + p.stats.building_triangles) / 1000));
        if let Some(err) = &p.assets.last_error {
            ui.label(egui::RichText::new(err).small().color(egui::Color32::from_rgb(255, 60, 90)));
        }
        hint(ui, help::ASSETS_RELOAD);
    });
}

fn camera(ui: &mut egui::Ui, p: &mut UiCtx) {
    let walking = *p.mode == CameraMode::Walk;
    if ui.selectable_label(walking, "Walk").on_hover_text(format!("Walk the street [Tab]. {}", help::CAMERA_WALK)).clicked() {
        *p.mode = CameraMode::Walk;
    }
    if ui.selectable_label(!walking, "Orbit").on_hover_text(help::CAMERA_ORBIT).clicked() {
        *p.mode = CameraMode::Orbit;
    }
    hint(ui, "camera");
}
