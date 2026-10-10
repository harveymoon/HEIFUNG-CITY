//! [feat/pt-ux] The inspector on a drone taxi or helicopter, and on a
//! camper or van at the kerb.

use super::*;

/// What it is, what it is doing and where to, who is aboard (the rider or
/// patient, a helicopter's crew), its battery and its day.
pub(super) fn drone(ui: &mut egui::Ui, w: &nd_sim::World, selection: &mut Selection, id: nd_sim::air::DroneId) {
    let Some(d) = w.air.drones.get(id) else {
        *selection = Selection::None;
        return;
    };
    neon(ui, &crate::air_select::title(w, id));
    ui.label(egui::RichText::new(capitalise(d.craft.label())).weak());
    ui.label(crate::air_select::doing(w, id));
    ui.horizontal(|ui| {
        figure(ui, "altitude", format!("{:.0} m", d.alt), IVORY);
        figure(ui, "battery", format!("{:.0}%", d.battery * 100.0), if d.battery < 0.25 { WARN } else { GOOD });
        figure(ui, "flights today", d.flights_today.to_string(), IVORY);
        figure(ui, "km today", format!("{:.1}", d.km_today), IVORY);
    });
    let (rider, crew) = crate::air_select::aboard(w, id);
    let who = match d.job {
        nd_sim::air::Job::Medic { .. } => "PATIENT",
        _ => "PASSENGER",
    };
    match rider {
        Some(c) => {
            section(ui, who);
            crate::ui::portrait::grid(ui, w, &[c], 1, selection);
        }
        None if d.craft == nd_sim::air::Craft::Taxi => hint(ui, "no one aboard: it flies itself"),
        None => {}
    }
    if !crew.is_empty() {
        let base = w.buildings.get(d.base).map_or("its base", |b| b.name.as_str());
        section(ui, &format!("CREW  ·  from {base}"));
        crate::ui::portrait::grid(ui, w, &crew, 4, selection);
    }
    let (base, dest) = crate::air_select::base_and_dest(w, id);
    ui.horizontal(|ui| {
        if let Some(b) = base.and_then(|b| w.buildings.get(b).map(|x| (b, x))) {
            if ui.small_button(format!("Base: {}", b.1.name)).clicked() {
                *selection = Selection::Building(b.0);
            }
        }
        if let Some(b) = dest.and_then(|b| w.buildings.get(b).map(|x| (b, x))) {
            if ui.small_button(format!("Going to {}", b.1.name)).clicked() {
                *selection = Selection::Building(b.0);
            }
        }
    });
}

/// A camper or van: who lives in it or that it was left behind, how long,
/// and the tow (ordered, coming, or the button).
pub(super) fn rv(ui: &mut egui::Ui, sim: &mut Sim, selection: &mut Selection, seed: u64) {
    let w = &sim.world;
    let Some(r) = w.society.homeless.rv(seed).copied() else {
        *selection = Selection::None;
        return;
    };
    let now = w.clock.tick;
    neon(ui, &crate::rv_select::title(w, seed));
    match r.abandoned {
        Some(since) => {
            let owner = crate::rv_select::owner_name(w, &r).map(|n| format!(" by {n}'s household")).unwrap_or_default();
            ui.label(format!("Left at the kerb{owner} {} ago, when they left town. Nobody lives in it.", fmt::hours_min_long(now.saturating_sub(since))));
        }
        None => {
            let since = w.households.get(r.household).and_then(|_| w.society.homeless.since.iter().find(|s| s.0 == r.household)).map(|s| s.1);
            ui.label(format!("A household with nowhere else to live sleeps in it{}.", since.map(|s| format!(", on the street {}", fmt::hours_min_long(now.saturating_sub(s)))).unwrap_or_default()));
            let people = crate::rv_select::residents(w, &r);
            if !people.is_empty() {
                section(ui, &format!("LIVING IN IT  ·  {}", people.len()));
                crate::ui::portrait::grid(ui, w, &people, 6, selection);
            }
        }
    }
    hint(ui, "Lowers land value round it. Rehousing (cheap homes, outreach) takes it off the street for good.");
    match r.tow_at {
        Some(t) if t > now => {
            let who = if r.abandoned.is_some() { "the city's tow contractor" } else { "a tow truck" };
            ui.label(egui::RichText::new(format!("Towed in {}  ·  {who} is coming", fmt::hours_min_long(t - now))).color(egui::Color32::from_rgb(255, 140, 20)));
        }
        _ => {}
    }
    let soon = r.tow_at.is_some_and(|t| t <= now + nd_sim::homeless::RV_TOW_TICKS);
    if !soon {
        let tip = if r.lived_in() { "Have it towed: the household loses its home on wheels and moves to a camp" } else { "Have the abandoned van hauled off now instead of waiting for the contractor" };
        if ui.button(egui::RichText::new(if r.lived_in() { "Tow it (they move on)" } else { "Tow it now" }).color(egui::Color32::from_rgb(255, 140, 20))).on_hover_text(tip).clicked() {
            if let Err(e) = sim.world.order_tow_rv(seed) {
                super::vehicle::tow_refused(ui, e.to_string());
            }
        }
    }
    super::vehicle::show_tow_refusal(ui);
}
