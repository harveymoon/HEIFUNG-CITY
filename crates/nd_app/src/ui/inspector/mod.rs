//! The inspector: the selected building or vehicle in a window of its own
//! (a selected citizen opens the person pane instead), and the read-out
//! shared with it.

mod about;
mod air;
mod bot;
/// [K34] Shipping containers.
mod boxes;
mod building;
mod site;
mod unit;
mod vehicle;

use bevy_egui::egui;

use super::fmt::{self, capitalise};
use super::pane::tag;
use super::theme::GOOD;
use super::{hint, neon, section, UiCtx, GOLD, IVORY, WARN};
use crate::assets::GameAssets;
use crate::sim::Sim;
use crate::tools::Selection;
use bot::bot;
use building::building;
use unit::unit;
use vehicle::{car, parked};
pub use vehicle::occupants;

/// The inspector window on a selected building or car, on the right where
/// the person pane opens too. Closing it deselects.
pub fn window(ctx: &egui::Context, p: &mut UiCtx, content: egui::Rect) {
    if !matches!(*p.selection, Selection::Building(_) | Selection::Car(_) | Selection::Parked(_) | Selection::Bot(_) | Selection::Train(_) | Selection::Metro(_) | Selection::Unit(_) | Selection::Ship(_) | Selection::Drone(_) | Selection::Rv(_) | Selection::Box(_)) {
        return;
    }
    let mut open = true;
    egui::Window::new(egui::RichText::new("Inspector").color(GOLD).strong())
        .id(egui::Id::new("inspector_window"))
        .open(&mut open)
        .default_pos(egui::pos2(content.right() - 392.0, content.top() + 12.0))
        .default_width(360.0)
        .constrain_to(content)
        .max_height(content.height() - 24.0)
        .vscroll(true)
        .show(ctx, |ui| {
            show(ui, p.sim, p.selection, p.assets);
            // [parks] (D5) A park: how kept, a clean-up, leave it wild.
            if let Selection::Building(id) = *p.selection {
                if let Some(m) = super::parks::inspector(ui, p.sim, id) {
                    p.flags.message = m;
                }
            }
            // [dams] (H1) A hydro plant with no wall drawn: draw one.
            if let Selection::Building(id) = *p.selection {
                if p.sim.world.buildings.get(id).is_some_and(|b| b.plant == Some(nd_sim::PowerPlantKind::Hydro)) && p.sim.world.dams.wall_of(id).is_none() {
                    let label = if p.sim.world.dams.is_pending(id) { "Draw its dam wall" } else { "Redraw its dam wall" };
                    if ui.button(label).on_hover_text("Click the bank by the plant, then the far bank: the wall shows its size, cost and power as you go. A drawn wall takes the old dam's place.").clicked() {
                        p.tools.tool = crate::tools::Tool::Buildings;
                        p.tools.dam = Some(crate::tools::DamStep { plant: id, a: None });
                    }
                }
            }
            // [ferry-routes] A click on a route flies to its far end.
            if let Some(at) = super::ferry::take_fly(ui.ctx()) {
                p.fly_to = Some(at);
            }
            ui.separator();
            // [camera] Follow it, or fly to it.
            super::follow::camera_buttons(ui, p.sim, *p.selection, p.follow, p.fly, p.mode);
            if ui.button("Flag for Claude (F8)").on_hover_text("Bookmark this with a note; the remote API's `flags` command lists it").clicked() {
                p.flags.want_flag = true;
            }
        });
    if !open {
        *p.selection = Selection::None;
    }
}

fn show(ui: &mut egui::Ui, sim: &mut Sim, selection: &mut Selection, assets: &GameAssets) {
    match *selection {
        Selection::Building(id) => {
            if !sim.world.buildings.contains_key(id) {
                *selection = Selection::None;
                return;
            }
            building(ui, sim, selection, assets, id);
            // [K34] A depot's boxes, or the boxes a Stacks home is built of.
            boxes::building_boxes(ui, sim, selection, id);
        }
        Selection::Car(id) => {
            car(ui, sim, selection, id);
            // [K34] The box on its trailer.
            boxes::truck_boxes(ui, sim, selection, id);
        }
        // [K34]
        Selection::Box(id) => boxes::card(ui, sim, selection, id),
        Selection::Bot(id) => bot(ui, &sim.world, selection, id),
        // [fire-scene]
        Selection::Unit(id) => unit(ui, sim, selection, assets, id),
        // [ferry-routes]
        Selection::Ship(id) => super::ferry::ship_card(ui, &sim.world, selection, id),
        // [train-inspect]
        Selection::Train(_) | Selection::Metro(_) => super::train::show(ui, &sim.world, selection, assets),
        Selection::Parked(vid) => parked(ui, sim, selection, vid),
        // [feat/pt-ux]
        Selection::Drone(id) => air::drone(ui, &sim.world, selection, id),
        Selection::Rv(seed) => air::rv(ui, sim, selection, seed),
        // The window opens on the kinds above only (its guard); citizens
        // get the person pane, tolls and roads their own windows.
        Selection::None | Selection::Citizen(_) | Selection::Toll(_) | Selection::Road(_) => {}
    }
}

/// A figure with a weak caption above it, for a row of headline numbers.
fn figure(ui: &mut egui::Ui, caption: &str, value: String, col: egui::Color32) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(caption).small().weak());
        ui.label(egui::RichText::new(value).color(col).strong());
    });
    ui.add_space(6.0);
}
