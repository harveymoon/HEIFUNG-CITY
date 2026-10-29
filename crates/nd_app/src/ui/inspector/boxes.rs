//! [K34] The inspector on a shipping container: its number, line, origin,
//! age, condition and marks, who owns it and where it is, what is in it,
//! and where it has been. And the box rows the building and lorry
//! inspectors show (a depot's stock, a Stacks home's walls, a lorry's
//! load), each a click away from its box.

use bevy_egui::egui;

use super::super::{hint, neon, section, GOLD, IVORY};
use crate::sim::Sim;
use crate::tools::Selection;
use nd_sim::containers::BoxId;

pub(super) fn card(ui: &mut egui::Ui, sim: &Sim, selection: &mut Selection, id: BoxId) {
    let w = &sim.world;
    let Some(s) = w.box_story(id) else {
        *selection = Selection::None;
        return;
    };
    neon(ui, &s.number);
    ui.label(egui::RichText::new(format!("{}  ·  {} {}", s.line, s.size, s.kind)).color(IVORY));
    ui.label(format!("From {}  ·  built {:.0} year{} ago", s.origin, s.built_year_ago.max(0.0).round(), if s.built_year_ago.round() == 1.0 { "" } else { "s" }));
    let wear = (s.wear * 100.0).round();
    let col = if s.wear < 0.35 { super::GOOD } else if s.wear < 0.6 { GOLD } else { super::super::WARN };
    ui.label(egui::RichText::new(format!("Condition: {} ({wear:.0}% worn)", s.condition)).color(col));
    if !s.marks.is_empty() {
        ui.label(egui::RichText::new(s.marks.join(", ")).weak());
    }
    section(ui, "NOW");
    ui.horizontal_wrapped(|ui| {
        ui.label(&s.now);
        if let Some(b) = s.now_id {
            if ui.small_button("go").on_hover_text("Inspect the place").clicked() {
                *selection = Selection::Building(b);
            }
        }
        if let Some(c) = s.car.filter(|c| w.traffic.cars.contains_key(*c)) {
            if ui.small_button("lorry").on_hover_text("Inspect the lorry carrying it").clicked() {
                *selection = Selection::Car(c);
            }
        }
    });
    if let Some(g) = &s.goods {
        ui.label(format!("Full of {g}"));
    } else {
        ui.label(egui::RichText::new("Empty").weak());
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("Owned by {}", s.owner));
        if let Some(b) = s.owner_id {
            if ui.small_button("go").clicked() {
                *selection = Selection::Building(b);
            }
        }
    });
    section(ui, &format!("ITS LIFE  ·  {} visit{} to town", s.visits, if s.visits == 1 { "" } else { "s" }));
    for (t, what, place, b) in s.history.iter().rev() {
        ui.horizontal_wrapped(|ui| {
            let c = nd_sim::SimClock { tick: *t };
            ui.label(egui::RichText::new(format!("day {} {}", c.day() + 1, c.format_hhmm())).small().weak());
            ui.label(format!("{what}"));
            match b {
                Some(b) if w.buildings.contains_key(*b) => {
                    if ui.link(place).clicked() {
                        *selection = Selection::Building(*b);
                    }
                }
                _ => {
                    ui.label(egui::RichText::new(place).weak());
                }
            }
        });
    }
    hint(ui, "Every box keeps its number for life. Worn ones are cut up for steel at a scrap yard; stack developers build homes from the sound ones.");
}

/// Rows of boxes, each a click away; at most `max`, with a count of the rest.
fn rows(ui: &mut egui::Ui, sim: &Sim, selection: &mut Selection, ids: &[BoxId], max: usize) {
    let w = &sim.world;
    let day = w.clock.day() as i64;
    for &id in ids.iter().take(max) {
        let Some(r) = w.containers.boxes.get(id) else { continue };
        ui.horizontal(|ui| {
            if ui.link(egui::RichText::new(r.number()).monospace()).clicked() {
                *selection = Selection::Box(id);
            }
            let line = nd_sim::containers::LINES[r.line as usize % 10].name;
            ui.label(egui::RichText::new(format!("{line} · {}ft · {}{}", if r.forty { 40 } else { 20 }, r.condition_label(day), if r.goods.is_some() { " · full" } else { "" })).small().weak());
        });
    }
    if ids.len() > max {
        ui.label(egui::RichText::new(format!("and {} more", ids.len() - max)).small().weak());
    }
}

/// A building's boxes: a depot's stock, a Stacks home built of boxes.
pub(super) fn building_boxes(ui: &mut egui::Ui, sim: &Sim, selection: &mut Selection, b: nd_sim::BuildingId) {
    let w = &sim.world;
    if let Some(h) = w.containers.homes.get(&b) {
        let head = if h.done { format!("BUILT FROM {} SHIPPING CONTAINERS", h.boxes.len()) } else { format!("STACK DEVELOPER  ·  {} OF {} BOXES ON SITE", h.boxes.len(), h.want) };
        section(ui, &head);
        rows(ui, sim, selection, &h.boxes.clone(), 14);
        return;
    }
    let Some(kind) = w.box_depot(b) else { return };
    let ids = w.containers.stored_at(b);
    let full = ids.iter().filter(|id| w.containers.boxes[**id].goods.is_some()).count();
    section(ui, &format!("CONTAINERS  ·  {} of {} ({} full)", ids.len(), w.box_capacity(b, kind), full));
    rows(ui, sim, selection, &ids, 10);
}

/// The boxes on a lorry.
pub(super) fn truck_boxes(ui: &mut egui::Ui, sim: &Sim, selection: &mut Selection, car: nd_sim::CarId) {
    let ids = sim.world.boxes_on_truck(car);
    if ids.is_empty() {
        return;
    }
    section(ui, "ON THE TRAILER");
    rows(ui, sim, selection, &ids, 4);
}
