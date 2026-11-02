//! [talk] "Talk" in the person pane: the mayor chats with the selected
//! citizen through the committee's backend (`nd_minds::committee::talk`).
//! The lines are in `World::talks` (saved with the city, bounded); a model's
//! reply comes on a later frame while "typing..." shows, the stand-in's at
//! once. Nothing here blocks the frame.

use bevy_egui::egui;
use nd_sim::{CitizenId, World};

use super::{hint, theme, GOLD, IVORY, WARN};
use crate::committee::CommitteeRt;

fn open_id() -> egui::Id {
    egui::Id::new("talk_open")
}

fn draft_id(id: CitizenId) -> egui::Id {
    egui::Id::new(("talk_draft", id))
}

fn note_id(id: CitizenId) -> egui::Id {
    egui::Id::new(("talk_note", id))
}

pub fn is_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(open_id()).unwrap_or(false))
}

pub fn set_open(ctx: &egui::Context, open: bool) {
    ctx.data_mut(|d| d.insert_temp(open_id(), open));
}

/// The person pane's "Talk" toggle.
pub fn button(ui: &mut egui::Ui) {
    let open = is_open(ui.ctx());
    if ui.selectable_label(open, "Talk").on_hover_text("Chat with them: they answer in character from what they know").clicked() {
        set_open(ui.ctx(), !open);
    }
}

/// The chat, inside the person pane.
/// [r7-sim-thread] `w` is the Sim's city: drawn every frame, so it takes
/// `&mut` only to send or clear (a write every frame would keep the sim
/// thread starting over).
pub fn section(ui: &mut egui::Ui, w: &mut crate::sim_thread::SimCore, rt: &mut CommitteeRt, id: CitizenId) {
    let Some(name) = w.citizens.get(id).map(|c| c.name.clone()) else { return };
    let first = name.split_whitespace().next().unwrap_or(&name).to_string();
    let typing = rt.talker.typing(id);
    ui.label(egui::RichText::new(format!("Talk with {first}")).font(theme::lead()).color(GOLD));
    egui::ScrollArea::vertical().id_salt(("talk_scroll", id)).max_height(230.0).stick_to_bottom(true).auto_shrink([false, true]).show(ui, |ui| {
        let lines = w.talks.lines(id);
        if lines.is_empty() && !typing {
            hint(ui, "Say hello. They answer from what they know: their day, home and work, how they get around and why, money, food, friends, favourite places, memories.");
        }
        for l in lines {
            if l.mayor {
                ui.label(egui::RichText::new(format!("You: {}", l.text)).color(GOLD));
            } else {
                let r = ui.label(egui::RichText::new(format!("{first}: {}", l.text)).color(IVORY));
                if l.stand_in {
                    r.on_hover_text("Answered by the offline stand-in, from their records");
                }
            }
            ui.add_space(2.0);
        }
        if typing {
            let t = ui.input(|i| i.time);
            let dots = ".".repeat(1 + (t * 2.5) as usize % 3);
            ui.label(egui::RichText::new(format!("{first} is typing{dots}")).italics().weak());
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
        }
    });
    let mut draft = ui.ctx().data(|d| d.get_temp::<String>(draft_id(id)).unwrap_or_default());
    let mut send = false;
    ui.horizontal(|ui| {
        let width = (ui.available_width() - 56.0).max(80.0);
        let r = ui.add(egui::TextEdit::singleline(&mut draft).desired_width(width).char_limit(nd_minds::committee::talk::MAX_SAID_CHARS).hint_text(format!("Say something to {first}...")));
        if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            send = true;
            r.request_focus();
        }
        let can = !typing && !draft.trim().is_empty();
        if ui.add_enabled(can, egui::Button::new("Send")).clicked() {
            send = true;
        }
    });
    if send && !typing && !draft.trim().is_empty() {
        let memory = rt.settings.talk_memories;
        match rt.talk(w, id, &draft, memory) {
            Ok(_) => {
                draft.clear();
                ui.ctx().data_mut(|d| {
                    d.remove::<String>(note_id(id));
                });
            }
            Err(e) => {
                ui.ctx().data_mut(|d| d.insert_temp(note_id(id), e));
            }
        }
    }
    ui.ctx().data_mut(|d| d.insert_temp(draft_id(id), draft));
    if let Some(e) = ui.ctx().data(|d| d.get_temp::<String>(note_id(id))) {
        ui.label(egui::RichText::new(e).small().color(WARN));
    }
    hint(ui, &rt.talk_status());
    ui.horizontal(|ui| {
        ui.checkbox(&mut rt.settings.talk_memories, egui::RichText::new("Leave a memory").small()).on_hover_text("The chat leaves a memory in them (\"Talked to the mayor about ...\"), one a day. Saved with the committee's settings.");
        if !w.talks.lines(id).is_empty() && ui.small_button("Clear").on_hover_text("Forget this chat").clicked() {
            w.talks.clear(id);
        }
    });
}
