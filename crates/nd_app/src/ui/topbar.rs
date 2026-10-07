//! The top bar: title, clock and speed, treasury and population, the
//! toggles that open the info windows (Dev among them), and the camera mode.
//! One row, read left to right like a city builder's status strip.

use bevy_egui::egui;

use super::windows::Info;
use super::{help, neon, section, UiCtx, GOLD, IVORY, WARN};
use crate::map_layers::{self, LayerId};
use crate::render::Overlays;
use crate::camera::CameraMode;
use crate::sim::SPEEDS;

/// Width the Heat toggle and the Walk/Orbit pair need at the right edge.
const CAMERA_WIDTH: f32 = 140.0;

pub fn show(ui: &mut egui::Ui, p: &mut UiCtx) {
    // Wrapped, so a narrow window moves the tail of the row onto a second
    // line instead of drawing the camera switch over the window toggles.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(egui::RichText::new("DISTRICT 2040").color(GOLD).strong().font(crate::ui::theme::lead()));
        ui.separator();
        clock_and_speed(ui, p);
        ui.separator();
        treasury_and_population(ui, p);
        ui.separator();
        layer_button(ui, p);
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
    let treasury = if e.treasury < 0.0 { treasury.color(WARN) } else { treasury.color(GOLD) };
    let resp = ui
        .add_sized(egui::vec2(80.0, ui.spacing().interact_size.y), egui::Label::new(treasury).wrap_mode(egui::TextWrapMode::Extend).sense(egui::Sense::click()))
        .on_hover_text(format!("treasury · taxes so far today {:.0}", e.tax_today));
    dev_grant(ui, &resp, p);
    let w = &p.sim.world;
    let e = &w.economy;
    slot(ui, 64.0, egui::RichText::new(format!("pop {}", w.citizens.len())).strong()).on_hover_text(format!("{} homes · {} jobs · {} waiting to move in", e.homes_total, e.jobs_total, e.waiting_immigrants));
    let power = &e.power;
    // No demand yet (an empty map) reads as 0% served: nothing to warn about.
    let text = if power.demand_mw > 0.0 && power.served < 0.98 { egui::RichText::new(format!("⚡ {:.0}%", power.served * 100.0)).small().color(WARN) } else { egui::RichText::new("") };
    slot(ui, 40.0, text).on_hover_text(format!("power served {:.0}%: the Power window has the detail", power.served * 100.0));
    // Nobody can move in until the city's streets reach elsewhere: the
    // highway by a ramp, a road off the map edge, or a working station.
    if !w.has_way_in() {
        ui.label(egui::RichText::new("Connect a road to the highway so people can move in").small().color(WARN))
            .on_hover_text("Newcomers, their cars and lorries from out of town arrive by the highway (join a street to it with ramps), by a road drawn off the edge of the map, or by train at a working station. Until then nobody can come.");
    }
}

/// Dev: how long the treasury figure must be held down for a grant.
const GRANT_HOLD_SECS: f64 = 10.0;
/// Dev: what each full hold adds to the treasury.
const GRANT_AMOUNT: f32 = 1_000_000.0;

/// Dev cheat: hold the mouse down on the treasury figure for ten seconds and
/// a million goes into the treasury; keep holding for another million every
/// ten seconds. A tooltip counts the hold down so it is clear it is working.
fn dev_grant(ui: &mut egui::Ui, resp: &egui::Response, p: &mut UiCtx) {
    let id = resp.id.with("dev_grant_since");
    let now = ui.input(|i| i.time);
    if !resp.is_pointer_button_down_on() {
        ui.data_mut(|d| d.remove::<f64>(id));
        return;
    }
    let since = ui.data_mut(|d| *d.get_temp_mut_or_insert_with(id, || now));
    let held = now - since;
    if held >= GRANT_HOLD_SECS {
        p.sim.world.economy.treasury += GRANT_AMOUNT;
        p.tools.status = format!("Dev: +$ {GRANT_AMOUNT:.0} to the treasury");
        ui.data_mut(|d| d.insert_temp(id, now));
    } else if held > 0.5 {
        resp.show_tooltip_text(format!("dev grant: +$1,000,000 in {:.0} s", (GRANT_HOLD_SECS - held).ceil()));
    }
    ui.ctx().request_repaint();
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
    let heat = p.lighting.heat;
    if ui.selectable_label(heat, "Heat").on_hover_text("Drone heat vision [H]: buildings go dark, people and engines glow").clicked() {
        p.lighting.heat = !heat;
    }
    ui.separator();
    let walking = *p.mode == CameraMode::Walk;
    if ui.selectable_label(walking, "Walk").on_hover_text(format!("Walk the street [Tab]. {}", help::CAMERA_WALK)).clicked() {
        *p.mode = CameraMode::Walk;
    }
    if ui.selectable_label(!walking, "Orbit").on_hover_text(help::CAMERA_ORBIT).clicked() {
        *p.mode = CameraMode::Orbit;
    }
}

/// The map layer button: the layer showing (or None), a dropdown to pick
/// another. Layers are exclusive; the legend shows the key of the one on.
fn layer_button(ui: &mut egui::Ui, p: &mut UiCtx) {
    let active = map_layers::active(p.overlays);
    let text = match active {
        Some(l) => egui::RichText::new(format!("Layer: {}", l.label())).color(GOLD).strong(),
        None => egui::RichText::new("Layer: none").color(IVORY),
    };
    egui::ComboBox::from_id_salt("map_layer")
        .selected_text(text)
        .width(170.0)
        .height(620.0)
        .show_ui(ui, |ui| layer_list(ui, p.overlays, false))
        .response
        .on_hover_text("The map layer: one data view at a time, with its colour key at the bottom right. V, O, P toggle theirs; L steps through the heat maps, K through the land layers.");
}

/// Every map layer under its group heading, the one on lit; a click shows
/// it alone, "None" clears the map. `wrap` lays each group out in a
/// wrapping row (the View window) rather than a column (the dropdown).
pub fn layer_list(ui: &mut egui::Ui, overlays: &mut Overlays, wrap: bool) {
    let active = map_layers::active(overlays);
    let mut pick: Option<Option<LayerId>> = None;
    if ui.selectable_label(active.is_none(), "None").on_hover_text("No data layer: the plain map").clicked() {
        pick = Some(None);
    }
    let all = LayerId::all();
    let mut i = 0;
    while i < all.len() {
        let group = all[i].group();
        let members: Vec<LayerId> = all[i..].iter().copied().take_while(|l| l.group() == group).collect();
        i += members.len();
        section(ui, &group.to_uppercase());
        let mut item = |ui: &mut egui::Ui, l: LayerId| {
            let key = l.hotkey();
            let label = if key.is_empty() || wrap { l.label().to_string() } else { format!("{}  [{key}]", l.label()) };
            if ui.selectable_label(active == Some(l), label).on_hover_text(l.hint()).clicked() {
                pick = Some(if active == Some(l) { None } else { Some(l) });
            }
        };
        if wrap {
            ui.horizontal_wrapped(|ui| members.iter().for_each(|l| item(ui, *l)));
        } else {
            members.iter().for_each(|l| item(ui, *l));
        }
    }
    if let Some(l) = pick {
        map_layers::set_active(overlays, l);
    }
}
