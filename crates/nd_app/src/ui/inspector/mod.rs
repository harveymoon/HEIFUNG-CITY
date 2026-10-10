//! The inspector: the selected building or vehicle in a window of its own
//! (a selected citizen opens the person pane instead), and the read-out
//! shared with it.

mod about;
mod air;
mod bot;
mod building;
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
    if !matches!(*p.selection, Selection::Building(_) | Selection::Car(_) | Selection::Parked(_) | Selection::Bot(_) | Selection::Train(_) | Selection::Metro(_) | Selection::Unit(_) | Selection::Ship(_) | Selection::Drone(_) | Selection::Rv(_)) {
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
        }
        Selection::Car(id) => car(ui, sim, selection, id),
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
