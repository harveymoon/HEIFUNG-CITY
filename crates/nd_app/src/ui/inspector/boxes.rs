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
    depot_yard(ui, sim, selection, b);
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

/// [r8] The boxes on a lorry's chassis, and what the container lorry is
/// about ("parked at the port in bay 3, waiting for a box").
pub(super) fn truck_boxes(ui: &mut egui::Ui, sim: &Sim, selection: &mut Selection, car: nd_sim::CarId) {
    let w = &sim.world;
    let ids = w.boxes_on_truck(car);
    let status = w.lorry_status(car);
    if ids.is_empty() && status.is_none() {
        return;
    }
    let chassis = w.traffic.cars.get(car).and_then(|c| nd_sim::vehicle::chassis_of(c.model.boxed));
    let head = match chassis {
        Some(true) => "ON THE CHASSIS  ·  40 FT",
        Some(false) => "ON THE CHASSIS  ·  20 FT",
        None => "ON THE TRAILER",
    };
    section(ui, head);
    if let Some(s) = status {
        ui.label(egui::RichText::new(s).color(IVORY));
    }
    if ids.is_empty() {
        ui.label(egui::RichText::new("empty chassis").small().weak());
    }
    rows(ui, sim, selection, &ids, 4);
}

/// [r8] A depot's lorry bays and its carrier, and a forwarder's books.
fn depot_yard(ui: &mut egui::Ui, sim: &Sim, selection: &mut Selection, b: nd_sim::BuildingId) {
    let w = &sim.world;
    if let Some(y) = w.containers.depots.get(&b) {
        let used = y.bays.iter().flatten().count();
        section(ui, &format!("LORRY BAYS  ·  {used} of {} in use, {} queued at the gate", y.bays.len(), y.queue.len()));
        for (k, c) in y.bays.iter().enumerate() {
            let Some(c) = *c else { continue };
            ui.horizontal(|ui| {
                if ui.link(format!("bay {}", k + 1)).clicked() {
                    *selection = Selection::Car(c);
                }
                let s = w.lorry_status(c).unwrap_or_default();
                ui.label(egui::RichText::new(s).small().weak());
            });
        }
        let crane = match y.crane {
            Some(j) => {
                let n = w.containers.boxes.get(j.boxid).map(|r| r.number()).unwrap_or_default();
                format!("Carrier: lifting {n} {} bay {}", if j.onto_truck { "onto the lorry in" } else { "off the lorry in" }, j.bay + 1)
            }
            None => "Carrier: standing by".to_string(),
        };
        ui.label(egui::RichText::new(format!("{crane}  ·  {} moves", y.moves)).small());
    }
    if let Some((loads, units, out, f)) = w.forwarder_card(b) {
        section(ui, "FREIGHT FORWARDER");
        ui.label(egui::RichText::new(format!("{} boxes of exports shipped  ·  {} loads from {} firms ({:.0} units)", f.boxes_out, f.loads_total, f.firms.len(), f.units_total)).color(IVORY));
        ui.label(egui::RichText::new(format!("Fees: {:.0} today, {:.0} in all  ·  {} rounds", f.fees_today, f.fees_total, f.rounds)).color(GOLD));
        ui.label(egui::RichText::new(format!("Waiting to be collected: {loads} loads ({units:.0} units)  ·  {out} lorries out")).small());
        // Its boxes on their way.
        let mine: Vec<BoxId> = w.containers.boxes.iter().filter(|(_, r)| r.owner == nd_sim::containers::Owner::Forwarder(b)).map(|(id, _)| id).collect();
        rows(ui, sim, selection, &mine, 5);
        hint(ui, "Leases sound empties, fills each with several firms' exports on a round of their yards, and ships it from the port or the freight yard, for a fee per load.");
    }
}
