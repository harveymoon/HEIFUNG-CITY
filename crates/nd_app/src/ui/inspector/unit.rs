//! [fire-scene] The inspector on an emergency unit.

use super::vehicle::journey_card;
use super::*;

/// [fire-scene] An emergency unit, driving or standing on scene: who it
/// is and whose, what it is doing, its crew as faces (with their jobs at a
/// fire), the incident (how bad, how far to out, the damage; the patient
/// or the suspect), and the water used at a fire.
pub(super) fn unit(ui: &mut egui::Ui, sim: &mut Sim, selection: &mut Selection, assets: &GameAssets, uid: nd_sim::UnitId) {
    use crate::fire_scene as fs;
    let w = &sim.world;
    let f = sim.frac();
    let Some(u) = w.services.units.get(uid) else {
        *selection = Selection::None;
        return;
    };
    if fs::unit_pose(w, uid, f).is_none() {
        // Back in the station: the station is what is left to show.
        *selection = Selection::Building(u.station);
        return;
    }
    let kind_col = match u.kind {
        nd_sim::UnitKind::FireEngine => egui::Color32::from_rgb(255, 90, 60),
        nd_sim::UnitKind::PoliceCar => egui::Color32::from_rgb(90, 150, 255),
        nd_sim::UnitKind::Ambulance => egui::Color32::from_rgb(240, 240, 240),
    };
    ui.label(egui::RichText::new(format!("{} {}", u.kind.call_sign(), u.number)).font(crate::ui::theme::lead()).color(IVORY).strong());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        tag(ui, &capitalise(u.kind.label()), kind_col);
        tag(ui, u.state.label(), if u.lights() { WARN } else { egui::Color32::from_gray(150) });
        if u.lights() {
            tag(ui, "lights and sirens", egui::Color32::from_rgb(255, 60, 60));
        }
    });
    let mut picked: Option<Selection> = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(egui::RichText::new("from").small().weak());
        if let Some(b) = w.buildings.get(u.station) {
            if ui.small_button(&b.name).on_hover_text("Select the station").clicked() {
                picked = Some(Selection::Building(u.station));
            }
        }
    });
    let car = u.car.filter(|c| w.traffic.cars.contains_key(*c));
    // (Driving home or on patrol, the journey card below says it.)
    if car.is_none() || matches!(u.state, nd_sim::services::UnitState::Responding | nd_sim::services::UnitState::Transporting) {
        ui.label(egui::RichText::new(capitalise(&fs::unit_doing(w, uid))).color(egui::Color32::from_rgb(120, 220, 255)).strong());
    }
    if let Some(c) = car {
        ui.label(format!("{:.0} km/h", w.traffic.cars[c].v * 3.6));
    }
    let scene = fs::engine_scene(w, uid, f, &assets.rules, &sim.building_tops);
    // The incident.
    if let Some(i) = fs::unit_incident(w, uid) {
        section(ui, &format!("{}  ·  #{}", i.title().to_uppercase(), i.id));
        if let Some(b) = w.buildings.get(i.building) {
            if ui.button(&b.name).on_hover_text("Select the building").clicked() {
                picked = Some(Selection::Building(i.building));
            }
        } else if !i.place.is_empty() {
            ui.label(&i.place);
        }
        match i.kind {
            nd_sim::IncidentKind::Fire => {
                let live = i.active();
                let word = fs::fire_word(if live { i.intensity } else { 0.0 });
                let col = if live { egui::Color32::from_rgb(255, (200.0 - i.intensity * 150.0) as u8, 40) } else { egui::Color32::from_rgb(140, 220, 160) };
                ui.label(egui::RichText::new(if live { format!("{} ({:.0}%)", capitalise(word), i.intensity * 100.0) } else { i.outcome.map_or("Over".to_string(), |o| capitalise(o.label())) }).color(col).strong());
                if let Some(p) = fs::fire_progress(w, i) {
                    let text = if live && i.stage == nd_sim::services::Stage::OnScene { format!("{:.0}% knocked down  ·  ~{} min to go", p * 100.0, fs::minutes_to_out(i.intensity)) } else { format!("{:.0}% knocked down", p * 100.0) };
                    ui.add(egui::ProgressBar::new(p).desired_width(ui.available_width().min(320.0)).text(text));
                } else if live {
                    hint(ui, "still growing until the engine gets there");
                }
                let lost: f32 = i.damage.iter().map(|d| d.lost).sum();
                if lost > 0.0 {
                    hint(ui, &format!("damage so far: {:.0}% of the building{}", lost * 100.0, if i.damage.iter().any(|d| d.destroyed) { " (burned down)" } else { "" }));
                }
                if let Some(s) = &scene {
                    let lines = s.streams.len();
                    figure(ui, "water used", format!("{:.0} L", (s.litres / 100.0).round() * 100.0), crate::ui::theme::SERVICE_BLUE);
                    hint(ui, &if s.packing {
                        "the water is off; the crew is rolling up the hoses and lowering the ladder".to_string()
                    } else {
                        format!("{} line{} flowing  ·  ladder {:.0} m at {:.0}°", lines, crate::ui::fmt::s(lines), s.ladder.length, s.ladder.pitch.to_degrees())
                    });
                }
            }
            _ => {
                if let Some((c, role)) = i.citizen.map(|c| (c, if i.kind == nd_sim::IncidentKind::Medical { "Patient" } else { "Victim" })).or(i.suspect.map(|c| (c, "Suspect"))) {
                    if let Some(n) = w.citizens.get(c).map(|x| x.name.clone()) {
                        if ui.button(format!("{role}: {n}")).clicked() {
                            picked = Some(Selection::Citizen(c));
                        }
                    }
                }
                if let Some(a) = i.arrived {
                    hint(ui, &format!("{} min on scene", i.resolved.unwrap_or(w.clock.tick).saturating_sub(a)));
                }
            }
        }
    }
    // The crew: faces, and at a fire who is doing what.
    let crew = w.unit_crew(uid);
    // (Driving, the journey card below shows them.)
    if car.is_none() {
        section(ui, &format!("CREW  ·  {}", crew.len().max(u.kind.crew())));
        if crew.is_empty() {
            hint(ui, &format!("the crew of {} {} (nobody on the station's payroll by name)", u.kind.call_sign(), u.number));
        } else {
            crate::ui::portrait::grid(ui, w, &crew, 8, selection);
            if let Some(s) = &scene {
                for (i, c) in crew.iter().enumerate() {
                    let Some(fig) = s.figures.get(i) else { continue };
                    let name = w.citizens.get(*c).map_or("someone", |x| x.name.as_str());
                    let what = if !fig.out { "aboard".to_string() } else if fig.walking { format!("{} (on the way)", fig.role.label()) } else { fig.role.label().to_string() };
                    ui.horizontal(|ui| {
                        if ui.small_button(name).clicked() {
                            picked = Some(Selection::Citizen(*c));
                        }
                        ui.label(egui::RichText::new(what).small().weak());
                    });
                }
            }
        }
    }
    if let Some(c) = car {
        if journey_card(ui, w, c, selection) {
            return;
        }
    }
    if let Some(p) = picked {
        *selection = p;
    }
}
