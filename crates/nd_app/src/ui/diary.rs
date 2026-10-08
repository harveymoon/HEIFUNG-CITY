//! [voices] A citizen's diary, written on demand from what the sim
//! remembers for them, by the committee's backend (the stand-in's
//! templates by default, a local model or Claude when switched on). Shown
//! beside the person pane; display only, kept in the app, never in the save.

use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};
use nd_sim::CitizenId;

use super::{theme, UiFlags, GOLD, IVORY};
use crate::committee::CommitteeRt;
use crate::sim::Sim;
use crate::tools::Selection;

fn open_id() -> egui::Id {
    egui::Id::new("diary_open")
}

fn is_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(open_id()).unwrap_or(false))
}

/// The person pane's "Diary" toggle.
pub fn button(ui: &mut egui::Ui) {
    let open = is_open(ui.ctx());
    if ui.selectable_label(open, "Diary").on_hover_text("Their diary of today, written from their memories").clicked() {
        ui.ctx().data_mut(|d| d.insert_temp(open_id(), !open));
    }
}

pub fn window_system(mut contexts: EguiContexts, sim: Res<Sim>, mut rt: ResMut<CommitteeRt>, selection: Res<Selection>, flags: Res<UiFlags>) -> Result {
    let ctx = contexts.ctx_mut()?;
    let Selection::Citizen(id) = *selection else { return Ok(()) };
    if !is_open(ctx) || !sim.world.citizens.contains_key(id) {
        return Ok(());
    }
    let rt = &mut *rt;
    rt.voices.ask_diary(&sim.world, id, &mut rt.courier, false);
    let content = flags.map_rect(ctx.viewport_rect());
    let name = sim.world.citizens[id].name.clone();
    let mut open = true;
    let mut refresh = false;
    egui::Window::new(egui::RichText::new(format!("Diary of {name}")).color(GOLD))
        .id(egui::Id::new("diary_window"))
        .open(&mut open)
        .default_pos(egui::pos2(content.right() - 372.0 - 340.0, content.top() + 12.0))
        .default_width(320.0)
        .constrain_to(content)
        .show(ctx, |ui| show(ui, &rt.voices, id, &rt.courier.backend_name(), &mut refresh));
    if refresh {
        rt.voices.ask_diary(&sim.world, id, &mut rt.courier, true);
    }
    if !open {
        ctx.data_mut(|d| d.insert_temp(open_id(), false));
    }
    Ok(())
}

fn show(ui: &mut egui::Ui, voices: &nd_minds::committee::voices::Voices, id: CitizenId, backend: &str, refresh: &mut bool) {
    match voices.diary(id) {
        Some(d) => {
            ui.label(egui::RichText::new(format!("Day {}", d.day + 1)).font(theme::lead()).color(GOLD));
            if d.text.is_empty() {
                ui.label(egui::RichText::new("Writing...").italics().weak());
            } else {
                ui.label(egui::RichText::new(&d.text).italics().color(IVORY));
            }
            let by = if d.pending { "being written".to_string() } else if d.by_model { format!("written by {backend}") } else { "from the stand-in's templates".to_string() };
            ui.label(egui::RichText::new(by).small().weak());
            if !d.pending && ui.small_button("Write it again").clicked() {
                *refresh = true;
            }
        }
        None => {
            ui.label(egui::RichText::new("Writing...").italics().weak());
        }
    }
}
