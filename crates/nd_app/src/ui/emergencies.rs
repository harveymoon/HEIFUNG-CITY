//! The Emergencies window: every 911 call, live while it happens and kept
//! as a history. Each incident shows when it was called in and by whom, the
//! dispatcher, the unit sent (city or out of town, and the fee), how long the
//! unit took from dispatch to the scene and how long it worked there, the
//! outcome, the people involved and where they were taken, and the damage a
//! fire did. "go" flies the camera to the scene.

use bevy::prelude::Vec2;
use bevy_egui::egui;
use nd_sim::services::{Incident, IncidentKind, Stage};

use super::{hint, section, UiCtx};

const FIRE: egui::Color32 = egui::Color32::from_rgb(255, 120, 60);
const CRIME: egui::Color32 = egui::Color32::from_rgb(110, 160, 255);
const MEDICAL: egui::Color32 = egui::Color32::from_rgb(120, 230, 150);

fn kind_color(k: IncidentKind) -> egui::Color32 {
    match k {
        IncidentKind::Fire => FIRE,
        IncidentKind::Crime => CRIME,
        IncidentKind::Medical => MEDICAL,
    }
}

/// "day 3 14:05" for a tick.
pub fn when(tick: u64) -> String {
    format!("day {} {:02}:{:02}", tick / 1440 + 1, (tick % 1440) / 60, tick % 60)
}

fn minutes(t: Option<u64>) -> String {
    t.map_or("-".to_string(), |m| format!("{m} min"))
}

/// Which kinds the history shows; kept in egui memory.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Filter {
    #[default]
    All,
    Kind(IncidentKind),
}

pub fn window(ui: &mut egui::Ui, p: &mut UiCtx) {
    let w = &p.sim.world;
    let s = &w.services;
    let st = &s.stats;
    let (dispatcher, delay) = w.dispatcher();
    section(ui, "NOW");
    ui.label(format!(
        "{} active · {} fires burning · {}/{} units out · avg response {:.0} min · dispatcher {}",
        s.active().count(),
        st.fires_burning,
        st.units_busy,
        st.units,
        st.avg_response(),
        match dispatcher.and_then(|d| w.citizens.get(d)) {
            Some(c) => format!("{} at the desk ({delay} min to send)", c.name),
            None => format!("none: regional line, {delay} min"),
        }
    ));
    ui.label(egui::RichText::new(format!(
        "today: {} fires · {} crimes · {} medical · {} unanswered · {} arrests · {} out-of-town calls ({:.0}) · {} in hospital · {} in jail · {} held at stations",
        st.incidents_today[0], st.incidents_today[1], st.incidents_today[2], st.unanswered_today, st.arrests_today, st.out_of_town_calls_today, st.out_of_town_fees_today, st.in_hospital, st.jailed, st.held_at_stations
    )).small());
    ui.label(egui::RichText::new(format!("homes covered: police {:.0}% · fire {:.0}% · health {:.0}% · schools {:.0}%", st.coverage[0] * 100.0, st.coverage[1] * 100.0, st.coverage[2] * 100.0, st.coverage[3] * 100.0)).small().weak());

    let mut fly: Option<nd_sim::Vec2> = None;
    section(ui, "LIVE");
    let active: Vec<&Incident> = s.active().collect();
    if active.is_empty() {
        hint(ui, "No emergencies right now.");
    }
    for inc in active.iter().rev() {
        incident_row(ui, p.sim.world.clock.tick, inc, true, &mut fly);
    }

    section(ui, "HISTORY");
    let id = egui::Id::new("emergencies_filter");
    let mut filter: Filter = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    ui.horizontal(|ui| {
        for (f, label) in [(Filter::All, "All"), (Filter::Kind(IncidentKind::Fire), "Fires"), (Filter::Kind(IncidentKind::Crime), "Crimes"), (Filter::Kind(IncidentKind::Medical), "Medical")] {
            if ui.selectable_label(filter == f, label).clicked() {
                filter = f;
            }
        }
    });
    ui.data_mut(|d| d.insert_temp(id, filter));
    let past: Vec<&Incident> = s.incidents.iter().rev().filter(|i| !i.active()).filter(|i| match filter {
        Filter::All => true,
        Filter::Kind(k) => i.kind == k,
    }).collect();
    if past.is_empty() {
        hint(ui, "Nothing on record yet.");
    }
    egui::ScrollArea::vertical().id_salt("emergency_history").max_height(360.0).show(ui, |ui| {
        for inc in past {
            incident_row(ui, p.sim.world.clock.tick, inc, false, &mut fly);
        }
    });
    if let Some(at) = fly {
        p.fly_to = Some(Vec2::new(at.x, at.y));
    }
}

/// One incident: a coloured header with the outline, the full record inside.
fn incident_row(ui: &mut egui::Ui, now: u64, inc: &Incident, live: bool, fly: &mut Option<nd_sim::Vec2>) {
    let status = match inc.outcome {
        Some(o) => o.label().to_string(),
        None => inc.stage.label().to_string(),
    };
    let head = egui::RichText::new(format!("#{} {} · {} at {} · {}", inc.id, when(inc.reported.unwrap_or(inc.started)), inc.title().to_uppercase(), inc.place, status)).color(kind_color(inc.kind));
    ui.horizontal(|ui| {
        if ui.small_button("go").on_hover_text("Fly the camera to the scene").clicked() {
            *fly = Some(inc.pos);
        }
        egui::CollapsingHeader::new(head).id_salt(("incident", inc.id)).default_open(live).show(ui, |ui| details(ui, now, inc));
    });
}

fn details(ui: &mut egui::Ui, now: u64, inc: &Incident) {
    let name_of = |role: &str| inc.people.iter().filter(|p| p.role == role).map(|p| p.name.clone()).collect::<Vec<_>>().join(", ");
    ui.label(format!("started {} · called in {} by {}", when(inc.started), inc.reported.map_or("not yet".to_string(), when), {
        let c = name_of("caller");
        if c.is_empty() { "a passer-by".to_string() } else { c }
    }));
    let d = name_of("dispatcher");
    ui.label(format!("dispatcher: {} · sent {}", if d.is_empty() { "none (regional line)".to_string() } else { d }, inc.dispatched.map_or("not yet".to_string(), when)));
    let unit = match (&inc.unit_name, inc.outside) {
        (Some(u), true) => format!("{u} · fee {:.0}", inc.fee),
        (Some(u), false) => format!("{u} (city)"),
        (None, _) => "nobody sent".to_string(),
    };
    ui.label(format!("unit: {unit}"));
    let live_travel = if inc.stage == Stage::Dispatched { inc.dispatched.map(|t| now.saturating_sub(t)) } else { None };
    let live_work = if inc.stage == Stage::OnScene { inc.arrived.map(|t| now.saturating_sub(t)) } else { None };
    ui.label(format!(
        "call to dispatch {} · dispatch to scene {}{} · on scene {}{} · call to arrival {}",
        minutes(inc.dispatched.zip(inc.reported).map(|(a, b)| a.saturating_sub(b))),
        minutes(inc.travel_ticks().or(live_travel)),
        if live_travel.is_some() { " so far" } else { "" },
        minutes(inc.work_ticks().or(live_work)),
        if live_work.is_some() { " so far" } else { "" },
        minutes(inc.response_ticks()),
    ));
    if inc.kind == IncidentKind::Fire && inc.active() {
        ui.label(egui::RichText::new(format!("burning at {:.0}%", inc.intensity * 100.0)).color(FIRE));
    }
    for p in &inc.people {
        if p.role == "caller" || p.role == "dispatcher" {
            continue;
        }
        ui.label(format!("{}: {}{}", p.role, p.name, p.taken_to.as_ref().map_or(String::new(), |t| format!(" → {t}"))));
    }
    for dmg in &inc.damage {
        ui.label(egui::RichText::new(format!("damage: {} lost {:.0}% condition{}", dmg.name, dmg.lost * 100.0, if dmg.destroyed { ", burned down" } else { "" })).color(FIRE));
    }
    if let Some(o) = inc.outcome {
        ui.label(egui::RichText::new(format!("outcome: {} at {}", o.label(), inc.resolved.map_or("-".to_string(), when))).strong());
    }
}
