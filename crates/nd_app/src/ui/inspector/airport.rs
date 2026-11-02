//! [airport] The inspector's airport card: flights, passengers, cargo,
//! delays, noise complaints, the money, how people get there, and the
//! planes on and over the field.

use super::*;

pub(super) fn airport_section(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::BuildingId) {
    let Some(r) = w.airport_report(id) else { return };
    section(ui, "AIRPORT");
    ui.label(egui::RichText::new(format!("{}  ·  {} runway{}, {} stands  ·  {:.0}/day to run", capitalise(r.size), r.runways, if r.runways == 1 { "" } else { "s" }, r.stands, r.upkeep_per_day)).color(crate::ui::theme::SERVICE_BLUE));
    if let Some(p) = &r.problem {
        ui.label(egui::RichText::new(format!("Closed: {p}")).color(WARN));
    }
    let row = |ui: &mut egui::Ui, k: &str, v: String| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(k).small().weak());
            ui.label(egui::RichText::new(v).small().color(IVORY));
        });
    };
    row(ui, "Flights today", format!("{} of {} timetabled  ·  yesterday {}", r.flights_today, r.scheduled_today, r.flights_yesterday));
    row(ui, "Passengers", format!("{} today ({} visitors to the hotels, {} on business, {} residents away)  ·  yesterday {}", r.pax_today, r.tourists_in_today, r.business_today, r.residents_out_today, r.pax_yesterday));
    row(ui, "Air cargo", format!("{:.0} units in, {:.0} out today  ·  {:.0} waiting for a flight", r.cargo_in_today, r.cargo_out_today, r.cargo_waiting));
    let delay_col = if r.avg_delay_min > 20.0 || r.cancelled_today > 0 { WARN } else { IVORY };
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Delays").small().weak());
        ui.label(egui::RichText::new(format!("{} late today, {:.0} min on average  ·  {} cancelled", r.delayed_today, r.avg_delay_min, r.cancelled_today)).small().color(delay_col));
    });
    let noise_col = if r.complaints_yesterday > 0 { WARN } else { IVORY };
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Noise").small().weak());
        ui.label(egui::RichText::new(format!("{} complaints yesterday from {} homes under the flight paths", r.complaints_yesterday, r.homes_in_noise)).small().color(noise_col));
    });
    row(ui, "Money", format!("fees {:.0} today, {:.0} yesterday  ·  staff {}/{}", r.fees_today, r.fees_yesterday, r.workers, r.jobs));
    let access: Vec<String> = r.access.iter().filter(|(_, n)| *n > 0).map(|(k, n)| format!("{k} {n}")).collect();
    if !access.is_empty() {
        row(ui, "To town", access.join(", "));
    }
    if r.transit.is_empty() {
        hint(ui, "No train, metro or bus at the terminal: a station or a bus stop within 450 m of its door takes passengers off the roads.");
    } else {
        row(ui, "Transit", r.transit.join(", "));
    }
    row(ui, "Demand", format!("about {:.0} passengers a day each way  ·  landing towards {:.0}°{}", r.demand_pax_per_day, r.landing_towards_deg, if r.one_way { " (one way: hills off the far end)" } else { "" }));
    if !r.planes.is_empty() {
        section(ui, "ON THE FIELD");
        for p in r.planes.iter().take(10) {
            let late = if p.late_min >= 15 { format!(", {} min late", p.late_min) } else { String::new() };
            hint(ui, &format!("{} ({}) from {} to {}: {}{}", p.flight, p.kind, p.from, p.to, p.phase, late));
        }
    }
    if !r.next_arrivals.is_empty() {
        section(ui, "NEXT IN");
        for n in &r.next_arrivals {
            hint(ui, n);
        }
    }
}
