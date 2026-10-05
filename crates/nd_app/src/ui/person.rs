//! The citizen read-out shared by the inspector and the person pane, and the
//! person pane itself.

use bevy_egui::egui;
use nd_minds::Personality;
use nd_sim::citizens::TravelMode;
use nd_sim::{Activity, Citizen, CitizenId, World};

use super::hint;
use crate::sim::Sim;
use crate::tools::{Selection, ToolState};

/// Who they are, what they are doing and where they are going, their means
/// and their current thought: the block both the inspector and the person
/// pane open with.
pub fn citizen_summary(ui: &mut egui::Ui, w: &World, c: &Citizen) {
    let name_of = |b: Option<nd_sim::BuildingId>| b.and_then(|b| w.buildings.get(b)).map(|b| b.name.as_str());
    let home = name_of(c.home).unwrap_or("none");
    let work = name_of(c.work).unwrap_or("unemployed");
    let via_transit = name_of(c.transit.map(|t| t.target));
    let destination = match &c.activity {
        Activity::Traveling { to, .. } | Activity::Driving { to, .. } => name_of(*to).or(via_transit),
        Activity::Waiting { .. } | Activity::Riding { .. } | Activity::OnTrain { .. } => via_transit,
        _ => None,
    };
    let rent = c.home.and_then(|h| w.buildings.get(h)).map_or(0.0, |b| b.rent);
    let how = if c.mode == TravelMode::Bike && c.activity.is_traveling() { "  ·  by bike" } else { "" };

    ui.label(egui::RichText::new(Personality::from_seed(c.seed).summary()).small());
    ui.label(match destination {
        Some(d) => format!("{}{how}  ·  to {d}", c.activity.label()),
        None => format!("{}{how}", c.activity.label()),
    });
    ui.label(format!("home: {home}  ·  work: {work}"));
    ui.label(format!(
        "mood {:.0}%  ·  satiety {:.0}%  ·  {} class  ·  arrived day {}",
        c.mood * 100.0,
        c.satiety * 100.0,
        c.class().label(),
        c.arrived_tick / 1440 + 1
    ));
    ui.label(format!(
        "income {:.0}/day  ·  rent {:.0}/day  ·  savings {:.0}{}",
        c.income,
        rent,
        c.savings,
        if c.rent_stress_days > 0 { format!("  ·  rent stress {}d", c.rent_stress_days) } else { String::new() }
    ));
    ui.label(format!(
        "meals at home {:.0}{}  ·  {}  ·  {}",
        c.groceries,
        if c.hungry_days > 0 { format!("  ·  hungry {}d", c.hungry_days) } else { String::new() },
        if c.owns_car { "owns a car" } else { "no car" },
        if c.owns_bike { "owns a bike" } else { "no bike" }
    ));
    ui.add_space(4.0);
    ui.label(egui::RichText::new(format!("“{}”", c.thought)).italics().color(egui::Color32::from_rgb(220, 190, 140)));
}

/// The spotlight checkbox, written back only when it changes.
pub fn spotlight_toggle(ui: &mut egui::Ui, w: &mut World, id: CitizenId, label: &str) {
    let Some(was) = w.citizens.get(id).map(|c| c.spotlight) else { return };
    let mut spotlight = was;
    if ui.checkbox(&mut spotlight, label).changed() {
        if let Some(c) = w.citizens.get_mut(id) {
            c.spotlight = spotlight;
        }
    }
}

/// One memory line: day, time, text.
pub fn memory_line(ui: &mut egui::Ui, m: &nd_sim::Memory) {
    ui.label(egui::RichText::new(format!("d{} {:02}:{:02}  {}", m.tick / 1440 + 1, (m.tick % 1440) / 60, m.tick % 60, m.text)).small());
}

/// A draggable window on the selected citizen: who they are, what they are
/// doing, where they are going, and everything they remember. The star keeps
/// them in the FAVOURITES list; closing the pane deselects them.
pub fn pane(ctx: &egui::Context, sim: &mut Sim, tools: &mut ToolState, selection: &mut Selection) {
    let Selection::Citizen(id) = *selection else { return };
    if !sim.world.citizens.contains_key(id) {
        return;
    }
    let is_fav = tools.favorites.contains(&id);
    let mut toggle_fav = false;
    let mut open = true;
    let title = sim.world.citizens[id].name.clone();
    egui::Window::new(egui::RichText::new(title).color(egui::Color32::from_rgb(230, 225, 210)).strong())
        .id(egui::Id::new("person_pane"))
        .open(&mut open)
        .default_pos(egui::pos2(ctx.viewport_rect().right() - 372.0, 24.0))
        .default_width(340.0)
        .resizable(false)
        .show(ctx, |ui| {
            let star = if is_fav { "★ favourite" } else { "☆ favourite" };
            if ui.selectable_label(is_fav, star).on_hover_text("Keep this person in the FAVOURITES list in the side panel").clicked() {
                toggle_fav = true;
            }
            ui.separator();
            let w = &sim.world;
            citizen_summary(ui, w, &w.citizens[id]);
            spotlight_toggle(ui, &mut sim.world, id, "Spotlight: route through the LLM mind");
            ui.separator();
            let c = &sim.world.citizens[id];
            hint(ui, &format!("memories, newest first ({})", c.memories.len()));
            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                for m in c.memories.iter().rev() {
                    memory_line(ui, m);
                }
            });
        });
    if toggle_fav {
        if is_fav {
            tools.favorites.retain(|f| *f != id);
        } else {
            tools.favorites.push(id);
        }
    }
    if !open {
        *selection = Selection::None;
    }
}
