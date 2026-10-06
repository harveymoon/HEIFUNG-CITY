//! The top bar: title, clock and speed, treasury and population, the
//! toggles that open the info windows (Dev among them), and the camera mode.
//! One row, read left to right like a city builder's status strip.

use bevy_egui::egui;

use super::windows::Info;
use super::{help, neon, UiCtx, GOLD, WARN};
use crate::camera::CameraMode;
use crate::sim::SPEEDS;

/// Width the Walk/Orbit pair needs at the right edge.
const CAMERA_WIDTH: f32 = 96.0;

pub fn show(ui: &mut egui::Ui, p: &mut UiCtx) {
    // Wrapped, so a narrow window moves the tail of the row onto a second
    // line instead of drawing the camera switch over the window toggles.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(egui::RichText::new("DISTRICT 2040").color(GOLD).strong().size(15.0));
        ui.separator();
        clock_and_speed(ui, p);
        ui.separator();
        treasury_and_population(ui, p);
        ui.separator();
        window_toggles(ui);
        ui.separator();
        // The layer the view shows: Underground, Ground, Sky [U].
        super::tools::depth_switch(ui, p.tools);
        // The camera switch hugs the right edge whatever the window width.
        // The row is sized to fit 1270 points; the fixed slots above keep
        // it from growing as figures change.
        // When the row has no room left for it, it simply follows the
        // toggles (and wraps) rather than overlapping them.
        if ui.available_width() >= CAMERA_WIDTH {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| camera(ui, p));
        } else {
            ui.separator();
            camera(ui, p);
        }
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
    let text = if sim.paused {
        egui::RichText::new("paused").small().weak()
    } else if eff < 0.9 {
        egui::RichText::new(format!("{:.0}%", eff * 100.0)).small().color(WARN)
    } else {
        egui::RichText::new("")
    };
    slot(ui, 40.0, text).on_hover_text(format!("The sim is running at {:.0}% of {} (below 90% the tick budget is throttling it).", eff * 100.0, SPEEDS[sim.speed_index].0));
}

/// A label in a fixed-width slot, so figures that come, go or change length
/// never shift the window toggles to their right.
fn slot(ui: &mut egui::Ui, width: f32, text: egui::RichText) -> egui::Response {
    ui.add_sized(egui::vec2(width, ui.spacing().interact_size.y), egui::Label::new(text).wrap_mode(egui::TextWrapMode::Extend))
}

fn treasury_and_population(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    let e = &w.economy;
    let treasury = egui::RichText::new(format!("$ {:.0}", e.treasury)).strong();
    slot(ui, 80.0, if e.treasury < 0.0 { treasury.color(WARN) } else { treasury.color(GOLD) }).on_hover_text(format!("treasury · taxes so far today {:.0}", e.tax_today));
    slot(ui, 64.0, egui::RichText::new(format!("pop {}", w.citizens.len())).strong()).on_hover_text(format!("{} homes · {} jobs · {} waiting to move in", e.homes_total, e.jobs_total, e.waiting_immigrants));
    let power = &e.power;
    // No demand yet (an empty map) reads as 0% served: nothing to warn about.
    let text = if power.demand_mw > 0.0 && power.served < 0.98 { egui::RichText::new(format!("⚡ {:.0}%", power.served * 100.0)).small().color(WARN) } else { egui::RichText::new("") };
    slot(ui, 40.0, text).on_hover_text(format!("power served {:.0}%: the Power window has the detail", power.served * 100.0));
}

/// One toggle per info window; lit while the window is open.
fn window_toggles(ui: &mut egui::Ui) {
    for info in Info::ALL {
        if matches!(info, Info::Saves | Info::Dev) {
            ui.separator();
        }
        if info == Info::Saves {
            super::newgame::toggle(ui);
        }
        let open = info.is_open(ui.ctx());
        if ui.selectable_label(open, info.title()).clicked() {
            info.set_open(ui.ctx(), !open);
        }
    }
}

fn camera(ui: &mut egui::Ui, p: &mut UiCtx) {
    let walking = *p.mode == CameraMode::Walk;
    if ui.selectable_label(walking, "Walk").on_hover_text(format!("Walk the street [Tab]. {}", help::CAMERA_WALK)).clicked() {
        *p.mode = CameraMode::Walk;
    }
    if ui.selectable_label(!walking, "Orbit").on_hover_text(help::CAMERA_ORBIT).clicked() {
        *p.mode = CameraMode::Orbit;
    }
}
