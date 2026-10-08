//! The inspector: the selected building or vehicle in a window of its own
//! (a selected citizen opens the person pane instead), and the read-out
//! shared with it.

use bevy_egui::egui;

use super::person::{citizen_summary, memory_line, spotlight_toggle};
use super::{hint, neon, section, UiCtx, GOLD, IVORY, WARN};
use crate::assets::GameAssets;
use crate::sim::Sim;
use crate::tools::Selection;

/// The inspector window on a selected building or car, on the right where
/// the person pane opens too. Closing it deselects.
pub fn window(ctx: &egui::Context, p: &mut UiCtx, content: egui::Rect) {
    if !matches!(*p.selection, Selection::Building(_) | Selection::Car(_) | Selection::Parked(_) | Selection::Bot(_) | Selection::Train(_) | Selection::Metro(_) | Selection::Unit(_)) {
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

pub fn show(ui: &mut egui::Ui, sim: &mut Sim, selection: &mut Selection, assets: &GameAssets) {
    match *selection {
        Selection::None => {
            ui.label(egui::RichText::new("Nothing selected.").weak());
        }
        // [toll-inspect] (its own window, `super::toll`, is the same one)
        Selection::Toll(id) => super::toll::show(ui, sim, selection, id),
        // [road-inspect] (its own window, `super::road`, is the same one)
        Selection::Road(sid) => {
            let _ = super::road::show(ui, sim, selection, sid, false);
        }
        Selection::Citizen(id) => {
            if !sim.world.citizens.contains_key(id) {
                *selection = Selection::None;
                return;
            }
            neon(ui, &sim.world.citizens[id].name);
            if let Some(b) = super::person::activity_heading(ui, &sim.world, id) {
                *selection = Selection::Building(b);
                return;
            }
            citizen_summary(ui, &sim.world, &sim.world.citizens[id]);
            super::estate::citizen_line(ui, &sim.world, id);
            ui.add_space(4.0);
            spotlight_toggle(ui, &mut sim.world, id, "Spotlight: route this citizen through the LLM mind");
            super::places::citizen_places(ui, &sim.world, id);
            hint(ui, "memories");
            for m in sim.world.citizens[id].memories.iter().rev().take(8) {
                memory_line(ui, m);
            }
        }
        Selection::Building(id) => {
            if !sim.world.buildings.contains_key(id) {
                *selection = Selection::None;
                return;
            }
            building(ui, sim, selection, assets, id);
        }
        Selection::Car(id) => {
            let w = &sim.world;
            let Some(car) = w.traffic.cars.get(id) else {
                *selection = Selection::None;
                return;
            };
            let mut driver_shown = false;
            if let Some(bus) = &car.bus {
                let line = w.transit.routes.get(bus.route).map(|r| r.name.clone()).unwrap_or_else(|| "a line that was removed".into());
                neon(ui, &format!("Bus on {line}"));
                let going = if bus.to_depot || bus.next_stop == usize::MAX { "heading back to the depot".to_string() } else { format!("next stop {}", bus.next_stop + 1) };
                ui.label(format!("{} passenger{}  ·  {going}  ·  {:.0} km/h{}", bus.passengers.len(), if bus.passengers.len() == 1 { "" } else { "s" }, car.v * 3.6, if car.hold > 0.0 { "  ·  at a stop" } else { "" }));
                if ui.small_button("Open the line in the Lines window").clicked() {
                    super::Info::Lines.set_open(ui.ctx(), true);
                }
            } else if let Some(cargo) = &car.cargo {
                neon(ui, "Delivery truck");
                let name = |b| w.buildings.get(b).map(|b| b.name.as_str()).unwrap_or("out of town");
                ui.label(format!("{:.0} {} from {} to {}  ·  worth {:.0}  ·  {:.0} km/h", cargo.amount, cargo.resource.label(), name(cargo.from), name(cargo.to), cargo.price, car.v * 3.6));
            } else {
                // [clarity] A city vehicle says what it is and whose it is.
                let unit = w.services.unit_of_car(id).and_then(|u| w.services.units.get(u));
                match unit {
                    Some(u) => neon(ui, &format!("{} {} {}  ·  {}", capitalise(u.kind.label()), u.kind.call_sign(), u.number, w.buildings.get(u.station).map_or("", |b| b.name.as_str()))),
                    None => neon(ui, &format!("Car  ·  {}", car.model.body.label().replace('_', " "))),
                }
                ui.label(format!("{:.0} km/h", car.v * 3.6));
                // [events] A street racer: the race and their place.
                super::nights::racer_line(ui, w, id);
                driver_shown = driver_card(ui, w, id, selection);
                // (Crews and strangers are named by the journey card below.)
                // [boot] Clamp it: it pulls over and waits for the tow truck.
                if car.owned.is_some() && tow_button(ui) {
                    let owned = car.owned;
                    match sim.world.order_tow_car(id) {
                        Ok(_) => {
                            if let Some(v) = owned {
                                *selection = Selection::Parked(v);
                            }
                        }
                        Err(e) => tow_refused(ui, e),
                    }
                    return;
                }
            }
            // [clarity] Where from, where to, why, and who is driving.
            if journey_card(ui, w, id, selection) {
                return;
            }
            // [services] A wreck waiting for its tow, or the truck coming.
            if tow_status(ui, w, id, selection) {
                return;
            }
            // Everyone aboard but the driver, as faces that select them.
            let riders: Vec<nd_sim::CitizenId> = occupants(w, id).into_iter().filter(|c| *c != car.citizen || !driver_shown).collect();
            if !riders.is_empty() {
                section(ui, &format!("{}  ·  {}", if car.bus.is_some() { "ON BOARD" } else { "PASSENGERS" }, riders.len()));
                super::portrait::grid(ui, w, &riders, 24, selection);
            } else if driver_shown {
                hint(ui, "alone in the car");
            }
            why_and_trip(ui, w, id, selection, !driver_shown);
        }
        Selection::Bot(id) => bot(ui, &sim.world, selection, id),
        // [fire-scene]
        Selection::Unit(id) => unit(ui, sim, selection, assets, id),
        // [train-inspect]
        Selection::Train(_) | Selection::Metro(_) => super::train::show(ui, &sim.world, selection, assets),
        Selection::Parked(vid) => {
            let Some(owned) = sim.world.vehicles.cars.get(vid) else {
                *selection = Selection::None;
                return;
            };
            let Some(spot) = owned.spot().copied() else {
                *selection = Selection::None;
                return;
            };
            neon(ui, &format!("Parked {}", owned.model.body.label().replace('_', " ")));
            let place = spot.building.and_then(|b| sim.world.buildings.get(b)).map(|b| format!(" at {}", b.name)).unwrap_or_default();
            let hours = sim.world.clock.tick.saturating_sub(spot.since) / 60;
            ui.label(format!("{}{place}  ·  parked {hours} h", spot.kind.label()));
            let owner = owned.owner;
            if let Some(name) = sim.world.citizens.get(owner).map(|c| c.name.clone()) {
                if ui.button(format!("Owner: {name}")).clicked() {
                    *selection = Selection::Citizen(owner);
                }
            }
            // [boot] Clamped and waiting, or the button to have it done.
            if spot.booted {
                let truck = sim.world.enforcement.job_for(vid).map(|j| if j.truck.is_some() { "the tow truck is on its way" } else if j.company.is_none() { "a tow truck is coming from out of town" } else { "waiting for a tow truck" }).unwrap_or("waiting for a tow truck");
                ui.label(egui::RichText::new(format!("Clamped  ·  {truck}")).color(egui::Color32::from_rgb(255, 140, 20)));
            } else if spot.kind.on_street() && tow_button(ui) {
                if let Err(e) = sim.world.order_tow(vid) {
                    tow_refused(ui, e);
                }
            }
            show_tow_refusal(ui);
        }
    }
}

/// [fire-scene] An emergency unit, driving or standing on scene: who it
/// is and whose, what it is doing, its crew as faces (with their jobs at a
/// fire), the incident (how bad, how far to out, the damage; the patient
/// or the suspect), and the water used at a fire.
fn unit(ui: &mut egui::Ui, sim: &mut Sim, selection: &mut Selection, assets: &GameAssets, uid: nd_sim::UnitId) {
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
                    figure(ui, "water used", format!("{:.0} L", (s.litres / 100.0).round() * 100.0), egui::Color32::from_rgb(120, 200, 255));
                    hint(ui, &if s.packing {
                        "the water is off; the crew is rolling up the hoses and lowering the ladder".to_string()
                    } else {
                        format!("{} line{} flowing  ·  ladder {:.0} m at {:.0}°", lines, if lines == 1 { "" } else { "s" }, s.ladder.length, s.ladder.pitch.to_degrees())
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
    }
    if car.is_some() {
    } else if crew.is_empty() {
        hint(ui, &format!("the crew of {} {} (nobody on the station's payroll by name)", u.kind.call_sign(), u.number));
    } else {
        super::portrait::grid(ui, w, &crew, 8, selection);
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
    if let Some(c) = car {
        if journey_card(ui, w, c, selection) {
            return;
        }
    }
    if let Some(p) = picked {
        *selection = p;
    }
}

/// [camera] A pavement delivery robot: its name and code, the shop that
/// owns it (in the fleet's colour), what it carries, where it is going and
/// how far, and the household that ordered. Owner and destination select.
fn bot(ui: &mut egui::Ui, w: &nd_sim::World, selection: &mut Selection, id: nd_sim::BotId) {
    let Some(b) = w.bots.bots.get(id) else {
        *selection = Selection::None;
        return;
    };
    let [r, g, bl] = nd_sim::bots::fleet_colour(b.shop);
    let fleet = egui::Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (bl * 255.0) as u8);
    ui.label(egui::RichText::new(nd_sim::bots::bot_name(w, id)).font(crate::ui::theme::lead()).color(IVORY).strong());
    let size = w.bots.fleet_of(b.shop).len();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        tag(ui, "Delivery bot", GOLD);
        tag(ui, &nd_sim::bots::bot_code(id), egui::Color32::from_gray(150));
        tag(ui, &format!("fleet of {size}"), fleet);
    });
    let name = |x: nd_sim::BuildingId| w.buildings.get(x).map_or("a building that is gone".to_string(), |b| b.name.clone());
    section(ui, "OWNER");
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
        ui.painter().rect_filled(rect, 2.0, fleet);
        let kind = w.buildings.get(b.shop).and_then(|s| s.business.as_ref()).map(|z| capitalise(z.kind.label()));
        if ui.link(egui::RichText::new(name(b.shop)).color(fleet)).on_hover_text("Select the shop").clicked() {
            *selection = Selection::Building(b.shop);
        }
        if let Some(k) = kind {
            hint(ui, &k);
        }
    });
    section(ui, "TRIP");
    match b.cargo() {
        Some((res, n)) => ui.label(format!("Carrying {n:.0} {} to the customer's door", res.label())),
        None => ui.label("Delivered; trundling back to the shop, hold empty"),
    };
    ui.horizontal(|ui| {
        ui.label("Going to");
        let to = b.destination();
        if ui.link(name(to)).on_hover_text("Select it").clicked() {
            *selection = Selection::Building(to);
        }
    });
    let m = b.metres_left();
    ui.label(format!("{m:.0} m of pavement left  ·  about {:.0} min  ·  out {} min", (m / nd_sim::bots::BOT_MPS / 60.0).ceil(), b.age));
    if !b.returning {
        ui.horizontal(|ui| {
            hint(ui, "ordered by a household at");
            if ui.link(egui::RichText::new(name(b.home)).small()).clicked() {
                *selection = Selection::Building(b.home);
            }
        });
    }
    hint(ui, "Route: the line on the map, in the fleet's colour.");
}

/// A selected building, most important first: its name, then what it is
/// (zone, business, district, tier) as coloured tags, then only the
/// sections that apply to it. A business shows its books and a cash-flow
/// sparkline and no homes; a block of flats shows its homes; the people in
/// it are a grid of faces that select them.
fn building(ui: &mut egui::Ui, sim: &mut Sim, selection: &mut Selection, assets: &GameAssets, id: nd_sim::BuildingId) {
    // [free-placement] A bulldoze confirmed last frame: it goes, and its
    // ground back to the zoning plan.
    let bulldoze = egui::Id::new(("inspector_bulldoze", id));
    if ui.ctx().data_mut(|d| d.remove_temp::<bool>(bulldoze.with("go"))).unwrap_or(false) {
        if sim.world.bulldoze_building(id).is_ok() {
            *selection = Selection::None;
            return;
        }
    }
    let w = &sim.world;
    let b = &w.buildings[id];
    // ── Header ──
    // [plant-groups] A member of a Sunfield or Wind Park shows the park.
    let group = w.plant_group_of(id);
    match &group {
        Some(g) => {
            ui.label(egui::RichText::new(&g.name).font(crate::ui::theme::title()).color(IVORY).strong());
            hint(ui, &format!("{} lots run as one; this one is {}", g.members.len(), b.name));
        }
        None => {
            ui.label(egui::RichText::new(&b.name).font(crate::ui::theme::title()).color(IVORY).strong());
        }
    }
    // [free-placement] A city building reads Civic (its ground is out of the zoning).
    let shown = w.shown_use(id);
    let zone_col = super::zone_color32(assets.palette.zone_color(shown));
    let style = nd_procgen::building_style(w, id, &assets.rules, Some(&sim.styles));
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        tag(ui, shown.label(), zone_col);
        if let Some(bz) = &b.business {
            tag(ui, &capitalise(bz.kind.label()), GOLD);
            if let Some(ind) = bz.kind.industry() {
                tag(ui, ind.label(), egui::Color32::from_gray(150));
            }
        } else if let Some(s) = b.service {
            tag(ui, &capitalise(s.kind.label()), egui::Color32::from_rgb(120, 200, 255));
        } else if let Some(k) = b.plant {
            tag(ui, &capitalise(k.label()), super::detail::plant_color(k));
        } else if b.depot {
            tag(ui, "Bus depot", GOLD);
        } else if b.homes > 0 {
            tag(ui, "Homes", egui::Color32::from_gray(170));
        }
        tag(ui, &format!("{} tier", b.tier.label()), egui::Color32::from_gray(130));
        if let Some(d) = w.district_of_lot(b.lot) {
            tag(ui, &d.name, egui::Color32::from_rgb(200, 140, 230));
        }
    });
    let started = if b.construction.is_some() { "started" } else { "built" };
    hint(ui, &format!("{}  ·  {} floor{}  ·  {started} day {}", style.map_or("unknown style", |s| s.label()), b.floors, if b.floors == 1 { "" } else { "s" }, b.built_tick / 1440 + 1));
    // [free-placement] The city's own: its ground is out of the zoning, and
    // the player may bulldoze it (a second click confirms).
    if w.is_city_building(id) && b.rail.is_none() && b.port.is_none() {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("City ground, out of the zoning").small().weak());
            let armed = ui.ctx().data(|d| d.get_temp::<bool>(bulldoze)).unwrap_or(false);
            if !armed {
                if ui.small_button("Bulldoze").on_hover_text("Pull it down; its ground goes back to whatever the zoning plan has there").clicked() {
                    ui.ctx().data_mut(|d| d.insert_temp(bulldoze, true));
                }
            } else {
                if ui.small_button(egui::RichText::new("Confirm bulldoze").color(WARN)).clicked() {
                    ui.ctx().data_mut(|d| {
                        d.remove::<bool>(bulldoze);
                        d.insert_temp(bulldoze.with("go"), true);
                    });
                }
                if ui.small_button("Cancel").clicked() {
                    ui.ctx().data_mut(|d| d.remove::<bool>(bulldoze));
                }
            }
        });
    }
    // [events] A rave on tonight (or the scene round an old warehouse).
    super::nights::rave_line(ui, w, id);
    // Alarms, before anything else.
    if w.services.is_burning(id) {
        ui.label(egui::RichText::new("ON FIRE").strong().color(egui::Color32::from_rgb(255, 110, 60)));
    }
    if b.is_derelict() {
        ui.label(egui::RichText::new("DERELICT").strong().color(WARN));
    }
    if let Some(c) = &b.construction {
        let (homes, jobs) = b.planned_capacity();
        let days_left = c.days_left();
        let left = if days_left < 1.0 { format!("{:.0} hours left", (days_left * 24.0).ceil().max(1.0)) } else { format!("{:.1} days left", days_left) };
        let status = if c.waiting_for_materials() { if c.ordered { ", waiting for materials (truck on its way)" } else { ", waiting for materials" } } else { "" };
        ui.add(egui::ProgressBar::new(c.progress).desired_height(18.0).fill(egui::Color32::from_rgb(200, 150, 60)).text(format!("UNDER CONSTRUCTION {:.0}%, {left}{status}", c.progress * 100.0)));
        ui.label(egui::RichText::new(format!("{} of {} deliveries in  ·  {:.1} days of work in all  ·  site crew {}/{}  ·  will have {} homes and {} jobs", c.deliveries, nd_sim::Construction::DELIVERIES, c.days, b.workers.len(), b.jobs, homes, jobs)).small());
    }
    if b.plant.is_none() && b.powered < 0.98 && b.construction.is_none() {
        ui.label(egui::RichText::new(format!("UNDERPOWERED: {:.0}% of {:.1} MW, {} dark hours today", b.powered * 100.0, nd_sim::power::demand_mw(b), b.unpowered_hours_today)).color(WARN));
    }
    // [dirty-industry] Dirty itself, dirty neighbours, poisoned ground.
    let pick = dirt_lines(ui, w, id);
    if let Some(src) = pick {
        *selection = Selection::Building(src);
        return;
    }

    // ── The business ──
    if let Some(bz) = &b.business {
        section(ui, if bz.closed { "BUSINESS  ·  CLOSED" } else { "BUSINESS" });
        ui.horizontal_wrapped(|ui| {
            figure(ui, "cash", format!("{:.0}", bz.money), if bz.money < 0.0 { WARN } else { IVORY });
            figure(ui, "today in", format!("+{:.0}", bz.revenue_today), GOOD);
            figure(ui, "today out", format!("-{:.0}", bz.spend_today), WARN);
            figure(ui, "staff", format!("{}/{}", b.workers.len(), b.jobs), IVORY);
        });
        cashflow(ui, &bz.books, (bz.revenue_today, bz.spend_today));
        // [economy] why it is here and how it is doing
        super::pressure::business_story(ui, w, id);
        let recipe = bz.kind.recipe();
        if !recipe.inputs.is_empty() || !recipe.outputs.is_empty() {
            egui::CollapsingHeader::new(egui::RichText::new(format!("stock  ·  {} deals so far", bz.deals)).small()).id_salt(("inspector_stock", id)).show(ui, |ui| {
                for (r, _) in recipe.inputs {
                    ui.label(egui::RichText::new(format!("needs {}: {:.0} in stock (target {:.0})", r.label(), bz.stock_of(*r), bz.target_input(*r, b.jobs))).small());
                }
                for (r, _) in recipe.outputs {
                    ui.label(egui::RichText::new(format!("sells {}: {:.0} in stock at {:.1}", r.label(), bz.stock_of(*r), bz.sell_price(*r, b.jobs))).small());
                }
            });
        }
    }

    // ── Homes: only where people live and there is no business ──
    if b.business.is_none() && b.homes > 0 {
        section(ui, "HOMES");
        ui.horizontal_wrapped(|ui| {
            figure(ui, "homes let", format!("{}/{}", b.households.len(), b.homes), IVORY);
            figure(ui, "residents", format!("{}", b.residents.len()), IVORY);
            figure(ui, "rent/day", format!("{:.0}", b.rent), IVORY);
        });
    } else if b.business.is_none() && b.jobs > 0 && b.construction.is_none() {
        section(ui, "JOBS");
        ui.label(format!("{}/{} filled  ·  wage {:.0}/day", b.workers.len(), b.jobs, b.wage()));
    }

    // ── What the place does, where it applies ──
    service_and_transit(ui, w, id, b);
    if let Some(lot) = w.lots.get(b.lot) {
        if let Some(park) = &lot.park {
            section(ui, "PARK");
            let features: Vec<&str> = park.features.iter().map(|f| f.kind.label()).collect();
            ui.label(format!("{} benches, {} paths, {} entrances{}; part of a {}-lot park of {:.0} m²", park.benches.len(), park.paths.len(), park.entrances.len(), if features.is_empty() { String::new() } else { format!(", {}", features.join(", ")) }, park.cluster_lots, park.cluster_area));
            let sitting = w.citizens.values().filter(|c| matches!(c.activity, nd_sim::Activity::Sitting { park: p, .. } if Some(p) == lot.building)).count();
            // [benches] Seats held (sitting or on the way) out of all of them.
            let (seats, held) = w.seats_in(b.lot);
            if sitting > 0 || held > 0 {
                ui.label(format!("{sitting} sitting on the benches  ·  {held} of {seats} seats taken{}", if seats > 0 && held >= seats { "  ·  FULL" } else { "" }));
            }
        }
    }
    if b.parking_capacity > 0 {
        ui.label(format!("parking {}/{} spaces{}", b.parking_used, b.parking_capacity, if b.is_parking() { format!("  ·  {:.1} per visit", b.parking_price) } else { "  ·  for residents and workers".to_string() }));
    }
    super::places::building_place(ui, w, id);
    // [land] owner, price, plots and the owner's other properties
    super::estate::building_section(ui, w, id, selection);

    // ── People ──
    let inside: Vec<nd_sim::CitizenId> = w.citizens.iter().filter(|(_, c)| c.inside == Some(id)).map(|(cid, _)| cid).collect();
    let groups: [(&str, &[nd_sim::CitizenId]); 3] = [("LIVE HERE", &b.residents), ("WORK HERE", &b.workers), ("IN THE BUILDING NOW", &inside)];
    for (title, ids) in groups {
        if ids.is_empty() {
            continue;
        }
        section(ui, &format!("{title}  ·  {}", ids.len()));
        super::portrait::grid(ui, w, ids, 18, selection);
    }

    // ── The site, last and folded: figures the header does not need ──
    egui::CollapsingHeader::new(egui::RichText::new("site").small()).id_salt(("inspector_site", id)).show(ui, |ui| {
        ui.label(egui::RichText::new(format!("land value {:.2}  ·  rent {:.0}/day  ·  wage {:.0}/day  ·  condition {:.0}%", b.land_value, b.rent, b.wage(), b.condition * 100.0)).small());
        ui.label(egui::RichText::new(format!("visitors {}  ·  power {:.1} MW ({:.0}% served)", b.visitors, nd_sim::power::demand_mw(b), b.powered * 100.0)).small());
        if let Some(lot) = w.lots.get(b.lot) {
            ui.label(egui::RichText::new(format!("foot traffic today {}  ·  this hour {}{}", lot.footfall_today, lot.footfall_hour, if lot.car_access() { "" } else { "  ·  on a footpath: cars stop at the street, residents park nearby" })).small());
        }
    });
}

/// Services, plants, rail, metro and depots: what the building does for
/// the city, one line each, only where it applies.
fn service_and_transit(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::BuildingId, b: &nd_sim::Building) {
    if let Some(s) = b.service {
        let units: Vec<&nd_sim::Unit> = w.services.units.values().filter(|u| u.station == id).collect();
        let mut line = format!("{}  ·  {:.0}/day", s.kind.tier_label(s.tier), s.kind.upkeep_per_day(s.tier));
        if s.kind.radius(s.tier) > 0.0 {
            line += &format!("  ·  covers {:.0} m {}", s.kind.radius(s.tier), if s.kind == nd_sim::ServiceKind::TrashDepot { "around" } else { "by road" });
        }
        if !units.is_empty() {
            line += &format!("  ·  {} {}s, {} crewed, {} on calls, {} on patrol", units.len(), units[0].kind.label(), w.crewed_units(id), units.iter().filter(|u| u.busy()).count(), units.iter().filter(|u| u.state == nd_sim::services::UnitState::Patrol).count());
        }
        if s.kind == nd_sim::ServiceKind::Health {
            line += &format!("  ·  beds {}/{}", w.services.admissions.iter().filter(|a| a.hospital == id).count(), s.kind.capacity(s.tier));
        }
        section(ui, "SERVICE");
        ui.label(egui::RichText::new(line).color(egui::Color32::from_rgb(120, 200, 255)));
    }
    if let Some(kind) = b.plant {
        // [plant-groups] A Sunfield or Wind Park: the whole park's figures.
        if let Some(g) = w.plant_group_of(id) {
            let members: Vec<&nd_sim::Building> = g.members.iter().filter_map(|m| w.buildings.get(*m)).collect();
            let cap: f32 = members.iter().map(|m| w.plant_capacity_mw(m)).sum();
            let now: f32 = members.iter().map(|m| w.plant_dispatched_mw(m)).sum();
            let upkeep: f32 = members.iter().map(|m| w.plant_upkeep(m)).sum();
            section(ui, if kind == nd_sim::PowerPlantKind::Wind { "WIND PARK" } else { "SUNFIELD" });
            ui.label(format!("{now:.1} MW now of {cap:.0} MW nameplate, {upkeep:.0}/day upkeep"));
            hint(ui, &format!("{} lots, {:.0} m² together; +{:.0}% for the shared inverters and grid connection", g.members.len(), g.area, (nd_sim::world::plant_groups::group_bonus(g.members.len()) - 1.0) * 100.0));
            for m in &g.members {
                if let Some(mb) = w.buildings.get(*m) {
                    let area = w.lots.get(mb.lot).map_or(0.0, |l| l.area());
                    hint(ui, &format!("{}{}: {:.0} m², {:.1} MW", if *m == id { "> " } else { "  " }, mb.name, area, w.plant_capacity_mw(mb)));
                }
            }
        } else {
            section(ui, "POWER PLANT");
        }
        ui.label(format!("{}{:.1} MW nameplate, {:.0}% available now, {:.0}/day upkeep", if w.plant_group_of(id).is_some() { "this lot: " } else { "" }, w.plant_capacity_mw(b), w.plant_availability(b) * 100.0, w.plant_upkeep(b)));
        // [grid-weather] The site's wind or sun, and what the farm runs at.
        if let Some(l) = w.lots.get(b.lot).filter(|_| matches!(kind, nd_sim::PowerPlantKind::Wind | nd_sim::PowerPlantKind::Solar)) {
            hint(ui, &super::tools::site_note(w, kind, l.center).replace("Under the pointer: ", "Site: "));
            if kind == nd_sim::PowerPlantKind::Wind {
                let share = w.plant_dispatched_mw(b) / w.plant_capacity_mw(b).max(1e-3);
                let v = w.weather.wind_ms() * w.plant_resource(b).wind;
                let state = if v >= nd_sim::weather::CUT_OUT_MS { "braked: the wind is past cut-out".to_string() } else if v < nd_sim::weather::CUT_IN_MS { "becalmed".to_string() } else { format!("running at {:.0}%", share * 100.0) };
                hint(ui, &format!("wind here now {v:.1} m/s: {state}"));
            }
        }
        if kind.sized_when_built() {
            // [plant-scale] Fixed when it was built (1 on older saves').
            match b.plant_scale {
                Some(s) => hint(ui, &format!("built for its {:.0} m² site: {:.2}× a standard {}", w.lots.get(b.lot).map_or(0.0, |l| l.area()), s, kind.label())),
                None => hint(ui, &format!("a standard {} (built before plants were sized to their site)", kind.label())),
            }
        } else if kind.reference_area().is_some() {
            hint(ui, &format!("sized to its lot: {:.0} m², {:.2}× a standard {}", w.lots.get(b.lot).map_or(0.0, |l| l.area()), w.plant_scale(b), kind.label()));
        }
        if b.demolition.is_some() {
            ui.label(egui::RichText::new("clearing the lot: not on the grid yet").color(egui::Color32::from_rgb(255, 170, 80)));
        }
    } else if let Some(bat) = &b.battery {
        // [power] a grid battery: charge, what it is doing, its limits
        let k = bat.kind;
        section(ui, "BATTERY");
        let doing = if bat.flow_mw > 0.05 { format!("delivering {:.1} MW", bat.flow_mw) } else if bat.flow_mw < -0.05 { format!("charging {:.1} MW", -bat.flow_mw) } else { "holding".to_string() };
        ui.label(format!("{}: {:.0} of {:.0} MWh ({:.0}%), {doing}; up to {:.0} MW, {:.0}/day upkeep", k.label(), bat.charge_mwh, k.capacity_mwh(), bat.level() * 100.0, k.max_mw(), k.upkeep_per_day()));
        ui.add(egui::ProgressBar::new(bat.level()).desired_height(10.0).fill(super::detail::BATTERY.gamma_multiply(0.8)));
    }
    // [dock-access] A dock or station off the roads: say so, loud, and what to do.
    if let Some(problem) = w.access_problem(id) {
        section(ui, "ROAD ACCESS");
        ui.label(egui::RichText::new("NOT CONNECTED TO THE ROAD NETWORK").color(egui::Color32::from_rgb(255, 170, 80)).strong());
        let todo = if w.stub_ends_of(id).is_empty() { "Its street is joined to roads that reach neither the city nor a way out of town: carry them on to the network." } else { "Draw a road to the marked end of its street stub (the amber ring); it snaps on like any road end." };
        hint(ui, todo);
        hint(ui, problem.split("; ").nth(1).unwrap_or(""));
    }
    // [ferries] The towns off the map its boats run to.
    if b.port == Some(nd_sim::ports::PortKind::FerryTerminal) {
        section(ui, "FERRIES");
        let towns = w.ferry_towns(id);
        let waiting = w.ports.boarding.get(&id).copied().unwrap_or(0);
        if towns.is_empty() {
            ui.label("its water does not reach the edge of the map, so no boats run off it");
        } else {
            let list: Vec<String> = towns.iter().map(|t| format!("{} ({}, {:.0} km)", t.name, t.edge.label(), t.km)).collect();
            ui.label(format!("ferries to {} · {waiting} waiting to board", list.join(", ")));
        }
    }
    if let Some(link) = &b.rail {
        let joined = if link.works() { "" } else if !link.rail_connected { " · NOT CONNECTED TO THE RAILWAY" } else { " · NOT CONNECTED TO THE ROADS" };
        section(ui, "RAIL");
        match link.kind {
            nd_sim::RailFacility::Station => ui.label(format!("railway station: platform {:.0} m, {} trains today, {} riders today across the line{joined}", link.platform[0].distance(link.platform[1]), w.rail.stats.passenger_trains_today, w.rail.stats.riders_today)),
            nd_sim::RailFacility::Central => ui.label(format!("Central Station: intercity platform {:.0} m, {} trains today, {} rail riders today; metro below, bus terminal in front{joined}", link.platform[0].distance(link.platform[1]), w.rail.stats.passenger_trains_today, w.rail.stats.riders_today)),
            nd_sim::RailFacility::FreightYard => ui.label(format!("freight yard: {} orders ({:.0} units) waiting for the next train, {:.0} units of exports on the apron{joined}", b.rail_inbound.len(), b.rail_inbound.iter().map(|c| c.amount).sum::<f32>(), b.rail_outbound)),
        };
    }
    // [subway]
    if b.metro {
        let (today, yesterday) = w.subway.boardings_at(id);
        let lines: Vec<String> = w.subway.lines_at(id).map(|(_, l)| format!("{} (every {} min)", l.name, l.headway)).collect();
        let waiting = w.citizens.values().filter(|c| matches!(c.activity, nd_sim::Activity::Waiting { .. }) && matches!(c.transit.map(|t| t.leg), Some(nd_sim::transit::TransitLeg::Subway { board, .. }) if board == id)).count();
        section(ui, "METRO");
        ui.label(if lines.is_empty() { "no line calls here yet (Build window, Transit, Metro lines)".to_string() } else { format!("{} · {today} boarded today, {yesterday} yesterday · {waiting} on the platform", lines.join(", ")) });
    }
    if b.depot {
        let out = w.traffic.cars.values().filter(|c| c.bus.as_ref().is_some_and(|x| x.depot == id)).count();
        section(ui, "BUS DEPOT");
        ui.label(format!("room for {} buses, {} out on the lines, {:.0}/day", nd_sim::transit::DEPOT_CAPACITY, out, nd_sim::transit::DEPOT_UPKEEP_PER_DAY));
    }
}

/// A small rounded tag in a colour: the header's zone, business, district.
fn tag(ui: &mut egui::Ui, text: &str, col: egui::Color32) {
    egui::Frame::new().fill(col.gamma_multiply(0.22)).stroke(egui::Stroke::new(1.0, col.gamma_multiply(0.8))).corner_radius(4.0).inner_margin(egui::Margin::symmetric(6, 1)).show(ui, |ui| {
        ui.label(egui::RichText::new(text).color(col).strong());
    });
}

/// A figure with a weak caption above it, for a row of headline numbers.
fn figure(ui: &mut egui::Ui, caption: &str, value: String, col: egui::Color32) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(caption).small().weak());
        ui.label(egui::RichText::new(value).color(col).strong());
    });
    ui.add_space(6.0);
}

const GOOD: egui::Color32 = egui::Color32::from_rgb(110, 200, 140);

/// The cash-flow sparkline: a pair of bars per day, takings up in green and
/// spending down in red, with the net as a gold line; today, still open, is
/// the last pair, drawn fainter. Hover a day for its figures.
fn cashflow(ui: &mut egui::Ui, books: &[nd_sim::market::DayBook], today: (f32, f32)) {
    let days: Vec<(f32, f32)> = books.iter().map(|d| (d.revenue, d.spend)).chain(std::iter::once(today)).collect();
    if days.len() < 2 {
        hint(ui, "cash flow: the first day's books close at midnight");
        return;
    }
    let width = ui.available_width().min(320.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 54.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 3.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 6));
    let top = days.iter().map(|d| d.0.max(d.1)).fold(1.0f32, f32::max);
    let mid = rect.center().y;
    let half = rect.height() * 0.5 - 2.0;
    let n = days.len();
    let step = rect.width() / n as f32;
    let bar = (step * 0.7).clamp(1.0, 9.0);
    p.hline(rect.x_range(), mid, egui::Stroke::new(1.0, egui::Color32::from_gray(70)));
    let mut net: Vec<egui::Pos2> = Vec::with_capacity(n);
    for (i, (rev, spend)) in days.iter().enumerate() {
        let x = rect.left() + step * (i as f32 + 0.5);
        let fade = if i + 1 == n { 0.45 } else { 1.0 };
        let up = half * rev / top;
        let down = half * spend / top;
        p.rect_filled(egui::Rect::from_min_max(egui::pos2(x - bar / 2.0, mid - up), egui::pos2(x + bar / 2.0, mid)), 0.0, GOOD.gamma_multiply(fade));
        p.rect_filled(egui::Rect::from_min_max(egui::pos2(x - bar / 2.0, mid), egui::pos2(x + bar / 2.0, mid + down)), 0.0, WARN.gamma_multiply(fade));
        net.push(egui::pos2(x, mid - half * (rev - spend) / top));
    }
    p.add(egui::Shape::line(net, egui::Stroke::new(1.5, GOLD)));
    if let Some(pos) = resp.hover_pos() {
        let i = (((pos.x - rect.left()) / step) as usize).min(n - 1);
        let (rev, spend) = days[i];
        let when = if i + 1 == n { "today so far".to_string() } else if i + 2 == n { "yesterday".to_string() } else { format!("{} days ago", n - 1 - i) };
        resp.on_hover_text(format!("{when}: took {rev:.0}, spent {spend:.0}, net {:+.0}", rev - spend));
    }
    let closed = &days[..n - 1];
    let total: f32 = closed.iter().map(|d| d.0 - d.1).sum();
    hint(ui, &format!("cash flow, last {} day{}: net {total:+.0}  (green in, red out, gold net)", closed.len(), if closed.len() == 1 { "" } else { "s" }));
}

/// Everyone in a vehicle: the driver of a private car first, then its
/// riders. Riders are a bus's passenger list and anyone whose activity says
/// they ride in it; a private car's own passenger list, when the sim keeps
/// one, belongs here too, so the inspector and its grid need no change.
pub fn occupants(w: &nd_sim::World, id: nd_sim::CarId) -> Vec<nd_sim::CitizenId> {
    let Some(car) = w.traffic.cars.get(id) else { return Vec::new() };
    let mut out: Vec<nd_sim::CitizenId> = Vec::new();
    if car.bus.is_none() && w.citizens.contains_key(car.citizen) {
        out.push(car.citizen);
    }
    if let Some(bus) = &car.bus {
        out.extend(bus.passengers.iter().copied());
    }
    for (cid, c) in &w.citizens {
        if matches!(c.activity, nd_sim::Activity::Riding { bus } if bus == id) && !out.contains(&cid) {
            out.push(cid);
        }
    }
    out
}

/// [clarity] "coming from [place] → going to [place]" (each a button that
/// selects it), what the vehicle is doing, and who is at the wheel when
/// that is not a driver card of its own: a city vehicle's crew (citizens
/// on the station's payroll, as faces), or an unknown person from out of
/// the city. True when the selection changed.
fn journey_card(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::CarId, selection: &mut Selection) -> bool {
    let Some(j) = w.car_journey(id) else { return false };
    let mut picked: Option<Selection> = None;
    ui.add_space(2.0);
    ui.label(egui::RichText::new(capitalise(&j.purpose)).font(crate::ui::theme::lead()).color(egui::Color32::from_rgb(120, 220, 255)).strong());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(egui::RichText::new("coming from").small().weak());
        if super::person::place_button(ui, &j.from) {
            picked = j.from.building.map(Selection::Building).or(j.from.car.map(Selection::Car));
        }
        match &j.to {
            Some(t) => {
                ui.label(egui::RichText::new("— going to").small().weak());
                if super::person::place_button(ui, t) {
                    picked = t.building.map(Selection::Building).or(t.car.map(Selection::Car));
                }
            }
            None => {
                ui.label(egui::RichText::new(format!("— {}", j.purpose)).small().weak());
            }
        }
    });
    match &j.driver {
        nd_sim::world::Driver::Crew(crew) => {
            section(ui, if crew.len() == 1 { "OFFICER" } else { "CREW" });
            super::portrait::grid(ui, w, crew, 24, selection);
        }
        nd_sim::world::Driver::Unknown(who) => {
            ui.label(egui::RichText::new(format!("At the wheel: {who}")).small().weak());
        }
        nd_sim::world::Driver::Citizen(_) => {}
    }
    match picked {
        Some(s) => {
            *selection = s;
            true
        }
        None => false,
    }
}

/// Why someone is on the road, in a few words.
fn purpose_label(p: &nd_sim::citizens::Purpose) -> String {
    use nd_sim::citizens::Purpose as P;
    match p {
        P::Home => "going home".into(),
        P::Work => "going to work".into(),
        P::Visit(lu) => format!("out to the {}", lu.label().to_lowercase()),
        P::Shop(r) => format!("shopping for {}", r.label()),
        P::Wander => "wandering".into(),
        P::Sit => "off to sit in a park".into(),
        P::ToStop => "heading for a stop".into(),
    }
}

/// The driver of a private car under the car's own details: a face that
/// selects them, who they are, why and where they are driving, the drive
/// so far against what it should take, their temper, and where they sit
/// between slow and reckless. False when nobody known is at the wheel.
fn driver_card(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::CarId, selection: &mut Selection) -> bool {
    let Some(car) = w.traffic.cars.get(id) else { return false };
    let Some(c) = w.citizens.get(car.citizen) else { return false };
    let cid = car.citizen;
    section(ui, "DRIVER");
    let mut pick = false;
    ui.horizontal(|ui| {
        pick |= super::portrait::button(ui, c, 52.0).clicked();
        ui.vertical(|ui| {
            pick |= ui.add(egui::Label::new(egui::RichText::new(&c.name).color(IVORY).strong()).sense(egui::Sense::click())).on_hover_text("Select the driver").clicked();
            ui.label(egui::RichText::new(format!("{}  ·  {} class  ·  mood {:.0}%", c.age, c.class().label(), c.mood * 100.0)).small());
            let (to, purpose) = match &c.activity {
                nd_sim::Activity::Driving { to, purpose, .. } => (*to, Some(purpose)),
                _ => (None, None),
            };
            let dest = to.and_then(|b| w.buildings.get(b)).map(|b| b.name.as_str());
            let line = match (purpose, dest) {
                (Some(p), Some(d)) => format!("{} · to {d}", purpose_label(p)),
                (Some(p), None) => purpose_label(p),
                (None, Some(d)) => format!("to {d}"),
                (None, None) => c.activity.label().to_string(),
            };
            ui.label(egui::RichText::new(line).small());
        });
    });
    if pick {
        *selection = Selection::Citizen(cid);
    }
    if let Some((elapsed, expected)) = w.car_trip_minutes(id) {
        let over = elapsed / expected.max(1.0);
        let col = if over > 2.0 { WARN } else if over > 1.3 { GOLD } else { egui::Color32::LIGHT_GRAY };
        ui.label(egui::RichText::new(format!("On the road {:.0} min  ·  should take {:.0} min", elapsed, expected)).color(col));
    }
    let d = &car.drive;
    if d.calm.is_some() {
        let word = match d.anger {
            a if a >= 0.75 => "raging",
            a if a >= 0.5 => "fuming",
            a if a >= 0.25 => "losing patience",
            a if a > 0.02 => "getting irritated",
            _ => "calm",
        };
        // An empty bar still paints a dot of fill; calm is just said.
        if d.anger > 0.02 {
            ui.add(egui::ProgressBar::new(d.anger).desired_width(220.0).desired_height(18.0).fill(egui::Color32::from_rgb(200, 60, 40)).text(format!("anger: {word}  ·  {:.0}%", d.anger * 100.0)));
        } else {
            ui.label(egui::RichText::new(format!("{word} at the wheel")).small());
        }
    }
    // The driver as they are now (anger included), on the slider; their
    // calm self is marked too when anger has moved them.
    temperament(ui, d.profile.boldness(), d.calm.map(|p| p.boldness()).filter(|b| (b - d.profile.boldness()).abs() > 0.03));
    ui.label(egui::RichText::new(format!("{}  ·  skill {:.0}%  ·  {:.0}% of the limit  ·  {:.1} s gap{}", d.profile.label(), d.profile.skill * 100.0, d.profile.speed * 100.0, d.profile.headway, if d.profile.drunk > 0.05 { format!("  ·  drunk {:.0}%", d.profile.drunk * 100.0) } else { String::new() })).small().weak());
    true
}

/// The slow–normal–reckless slider (read only): a track from blue through
/// grey to red with a marker at `boldness` (`-1..1`), and a hollow one at
/// the driver's calm self when anger has moved them.
fn temperament(ui: &mut egui::Ui, boldness: f32, calm: Option<f32>) {
    let width = ui.available_width().min(260.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 30.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    let track = egui::Rect::from_min_max(egui::pos2(rect.left() + 6.0, rect.top() + 6.0), egui::pos2(rect.right() - 6.0, rect.top() + 12.0));
    let slow = egui::Color32::from_rgb(90, 150, 255);
    let normal = egui::Color32::from_gray(150);
    let reckless = egui::Color32::from_rgb(255, 80, 60);
    let n = 24;
    for i in 0..n {
        let t = i as f32 / (n - 1) as f32;
        let col = if t < 0.5 { lerp(slow, normal, t * 2.0) } else { lerp(normal, reckless, t * 2.0 - 1.0) };
        let x0 = track.left() + track.width() * i as f32 / n as f32;
        let x1 = track.left() + track.width() * (i + 1) as f32 / n as f32;
        p.rect_filled(egui::Rect::from_min_max(egui::pos2(x0, track.top()), egui::pos2(x1, track.bottom())), 0.0, col.gamma_multiply(0.7));
    }
    let x_of = |b: f32| track.left() + track.width() * (b.clamp(-1.0, 1.0) + 1.0) / 2.0;
    if let Some(c) = calm {
        p.circle_stroke(egui::pos2(x_of(c), track.center().y), 5.0, egui::Stroke::new(1.5, IVORY));
    }
    p.circle_filled(egui::pos2(x_of(boldness), track.center().y), 6.0, IVORY);
    let font = crate::ui::theme::small();
    let weak = egui::Color32::from_gray(150);
    p.text(egui::pos2(track.left(), rect.bottom()), egui::Align2::LEFT_BOTTOM, "slow", font.clone(), weak);
    p.text(egui::pos2(track.center().x, rect.bottom()), egui::Align2::CENTER_BOTTOM, "normal", font.clone(), weak);
    p.text(egui::pos2(track.right(), rect.bottom()), egui::Align2::RIGHT_BOTTOM, "reckless", font, weak);
    resp.on_hover_text(format!("Driving style {:+.2} (−1 slow and careful, 0 ordinary, +1 reckless): speed, overtaking, ambers and following distance{}", boldness, if calm.is_some() { "; the ring is how they drive when calm" } else { "" }));
}

fn lerp(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    let m = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    egui::Color32::from_rgb(m(a.r(), b.r()), m(a.g(), b.g()), m(a.b(), b.b()))
}

/// Why the car is standing (or crawling), the car it waits for, the head of
/// the jam it is in, and the drive so far against what it should take.
/// Every car named is a button that selects it.
/// `trip`: also the drive so far and the driver's temper, for vehicles
/// whose driver has no card of their own (buses, trucks, strangers).
fn why_and_trip(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::CarId, selection: &mut Selection, trip: bool) {
    use nd_sim::traffic::StopReason;
    let t = &w.traffic;
    let Some(car) = t.cars.get(id) else { return };
    let reason = t.stop_reason(id);
    hint(ui, "why");
    let colour = |r: &StopReason| if r.is_good() { egui::Color32::LIGHT_GRAY } else { WARN };
    if reason == StopReason::Moving {
        ui.label("Driving freely");
    } else {
        ui.horizontal_wrapped(|ui| {
            match reason.by() {
                Some(b) => {
                    ui.label(egui::RichText::new(capitalise(reason.lead_in())).color(colour(&reason)));
                    if ui.small_button(w.car_label(b)).on_hover_text("Select this vehicle").clicked() {
                        *selection = Selection::Car(b);
                    }
                }
                None => {
                    ui.label(egui::RichText::new(capitalise(&reason.text(|_| String::new()))).color(colour(&reason)));
                }
            }
        });
        if car.stalled > 1.0 {
            ui.label(egui::RichText::new(format!("standing {}", duration(car.stalled))).small().weak());
        }
    }
    let chain = t.jam_chain(id);
    if let Some(head) = chain.head().filter(|_| chain.cars.len() >= 3 || (chain.cars.len() == 2 && chain.cycle)) {
        let hr = t.stop_reason(head);
        ui.add_space(2.0);
        // The front car is rolling: a slow queue, not a blockage.
        let crawling = t.cars.get(head).is_some_and(|h| h.v >= 0.5);
        if chain.cycle {
            ui.label(egui::RichText::new(format!("Gridlock: {} vehicles waiting on each other in a ring", chain.cars.len())).color(WARN));
        } else if crawling {
            ui.label(egui::RichText::new(format!("Queue of {}, moving off again at the front", chain.cars.len() - 1)).strong());
        } else {
            ui.label(egui::RichText::new(format!("Jam: {} vehicles back from the front", chain.cars.len() - 1)).strong());
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(if crawling { "front:" } else { "caused by" });
            if ui.button(w.car_label(head)).on_hover_text("Select the car at the head of the jam").clicked() {
                *selection = Selection::Car(head);
            }
        });
        let speed = t.cars.get(head).map_or(0.0, |h| h.v * 3.6);
        ui.label(egui::RichText::new(format!("— {}  ·  {speed:.0} km/h", hr.text(|c| w.car_label(c)))).color(colour(&hr)));
    }
    if !trip {
        return;
    }
    // [rage] The drive so far.
    if let Some((elapsed, expected)) = w.car_trip_minutes(id) {
        let over = elapsed / expected.max(1.0);
        let col = if over > 2.0 { WARN } else if over > 1.3 { super::GOLD } else { egui::Color32::LIGHT_GRAY };
        ui.label(egui::RichText::new(format!("On the road {:.0} min  ·  should take {:.0} min", elapsed, expected)).color(col));
    }
    let d = &car.drive;
    if d.calm.is_some() && d.anger > 0.02 {
        let word = match d.anger {
            a if a >= 0.75 => "raging",
            a if a >= 0.5 => "fuming",
            a if a >= 0.25 => "losing patience",
            _ => "getting irritated",
        };
        ui.add(egui::ProgressBar::new(d.anger).desired_width(220.0).fill(egui::Color32::from_rgb(200, 60, 40)).text(format!("Driver {word}  ·  {:.0}%", d.anger * 100.0)));
    }
    ui.label(egui::RichText::new(format!("Driver: {}", d.profile.label())).small().weak());
}

/// [boot] "Tow this car": a wheel clamp now, the tow truck after; the owner
/// pays at the pound.
fn tow_button(ui: &mut egui::Ui) -> bool {
    ui.button(egui::RichText::new("Tow this car").color(egui::Color32::from_rgb(255, 140, 20)))
        .on_hover_text("Clamp it where it stands (a moving car pulls over first) and call the tow truck; the owner pays to get it back from the pound")
        .clicked()
}

/// Why the last tow order was refused, shown under the button for a while.
fn tow_refused(ui: &egui::Ui, why: String) {
    let until = ui.ctx().input(|i| i.time) + 4.0;
    ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new("tow_refused"), (why, until)));
}

fn show_tow_refusal(ui: &mut egui::Ui) {
    let now = ui.ctx().input(|i| i.time);
    if let Some((why, until)) = ui.ctx().data(|d| d.get_temp::<(String, f64)>(egui::Id::new("tow_refused"))) {
        if now < until {
            ui.label(egui::RichText::new(format!("Can't tow: {why}")).color(WARN).small());
        }
    }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// Traffic seconds as "45 s" or "3 min".
fn duration(secs: f32) -> String {
    if secs < 90.0 { format!("{secs:.0} s") } else { format!("{:.0} min", secs / 60.0) }
}

/// [services] A written-off car: how long it has waited for the tow, and
/// once a recovery truck is on its way, "truck inbound" with a button that
/// selects it (the truck's road to the wreck is drawn while the wreck is
/// selected, `tools::draw_selection`). A recovery truck: the wreck it is
/// going to. True when the selection changed.
fn tow_status(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::CarId, selection: &mut Selection) -> bool {
    let tick = w.clock.tick;
    if let Some(wr) = w.driving.wrecks.iter().find(|x| x.car == id) {
        section(ui, "TOW");
        let waited = tick.saturating_sub(wr.since);
        match wr.truck.and_then(|t| w.traffic.cars.get(t).map(|c| (t, c))) {
            Some((t, truck)) => {
                let d = w.traffic.cars.get(id).map_or(0.0, |c| c.pos.distance(truck.pos));
                ui.label(egui::RichText::new(format!("Truck inbound  ·  {d:.0} m away  ·  sent {} ago", duration_min(tick.saturating_sub(wr.sent)))).color(WARN));
                ui.horizontal_wrapped(|ui| {
                    hint(ui, if wr.company.is_some() { "from a tow company in town" } else { "from out of town" });
                    if ui.small_button("locate").on_hover_text("Select the tow truck").clicked() {
                        *selection = Selection::Car(t);
                    }
                });
                if *selection == Selection::Car(t) {
                    return true;
                }
            }
            None => {
                let called = if wr.due > 0 { "called, finding a truck" } else { "waiting for the police to call it in" };
                ui.label(egui::RichText::new(format!("Waiting for a tow  ·  {}", duration_min(waited))).color(WARN));
                hint(ui, called);
            }
        }
        if wr.truck.is_some() {
            hint(ui, &format!("waiting {} so far", duration_min(waited)));
        }
        if wr.flipped > 0 {
            let how = if wr.flipped == 1 { "on its side" } else { "on its roof" };
            hint(ui, &if wr.righted_at > 0 { format!("{how}: the crew is righting it") } else { format!("{how}: the truck will right it before towing") });
        }
        return false;
    }
    let Some(car) = w.traffic.cars.get(id) else { return false };
    if let nd_sim::Errand::Recover(wreck) = car.errand {
        section(ui, "RECOVERY");
        ui.horizontal_wrapped(|ui| {
            ui.label(if car.drive.recovering { "Amber lights, nearly there, going round the queue:" } else { "Amber lights, on the way to" });
            if ui.small_button(w.car_label(wreck)).on_hover_text("Select the wreck").clicked() {
                *selection = Selection::Car(wreck);
            }
        });
        return *selection == Selection::Car(wreck);
    }
    false
}

/// Game minutes as "12 min" or "2 h 05".
fn duration_min(m: u64) -> String {
    if m < 60 { format!("{m} min") } else { format!("{} h {:02}", m / 60, m % 60) }
}

/// [dirty-industry] What dirty industry means for this building: a dirty
/// plant says how far it fouls and how many neighbours suffer; a sufferer
/// lists its unhappy neighbours, worst first, with what they cost it
/// ("Ironmaw Smelter 120 m · food output −30%, land value −0.09"); and the
/// ground's contamination when there is any. Returns a neighbour clicked.
fn dirt_lines(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::BuildingId) -> Option<nd_sim::BuildingId> {
    let b = &w.buildings[id];
    let mut clicked = None;
    if let Some(src) = w.dirty.iter().find(|s| s.building == id) {
        let hurt = w.buildings.iter().filter(|(o, x)| *o != id && x.construction.is_none() && w.building_exposure(*o) > 0.05 && w.building_lot(*o).is_some_and(|l| l.center.distance(src.pos) < src.reach)).count();
        let spill = b.business.as_ref().and_then(|z| z.kind.spill()).map(|(r, _)| r).or_else(|| b.plant.and_then(|k| k.ground_spill()).map(|(r, _)| r)).unwrap_or(0.0);
        ui.label(egui::RichText::new(format!("DIRTY INDUSTRY: fouls the ground within {spill:.0} m; neighbours within {:.0} m suffer ({hurt} now)", src.reach)).small().color(egui::Color32::from_rgb(210, 170, 90)));
        hint(ui, "farms, fishing, homes, shops and clean tech hate it; manufacturing and logistics shrug");
    }
    let neighbours = w.unhappy_neighbours(id);
    if !neighbours.is_empty() {
        section(ui, "UNHAPPY NEIGHBOURS");
        for n in neighbours.iter().take(4) {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if ui.link(egui::RichText::new(&n.name).small().color(WARN)).on_hover_text(format!("Select the {}", n.what)).clicked() {
                    clicked = Some(n.source);
                }
                ui.label(egui::RichText::new(format!("{} · {:.0} m · hit {:.0}%", n.what, n.distance, n.hit * 100.0)).small());
            });
        }
        if neighbours.len() > 4 {
            hint(ui, &format!("and {} more", neighbours.len() - 4));
        }
        if let Some(cost) = w.adjacency_cost(id) {
            ui.label(egui::RichText::new(format!("Costs it: {cost}")).small().color(WARN));
        }
    }
    if let Some(l) = w.lots.get(b.lot) {
        let c = w.contamination.over(l.center, &l.polygon);
        if c >= 0.02 {
            let col = if c > 0.3 { WARN } else { egui::Color32::from_rgb(200, 200, 120) };
            // What the soil costs this farm (`World::industry_hourly`).
            use nd_sim::BusinessKind as K;
            let share = match b.business.as_ref().map(|z| z.kind) {
                Some(K::GrainFarm) => 0.9,
                Some(K::CowFarm | K::Dairy) => 0.7,
                Some(K::VertFarm | K::Fishery | K::ChickenFarm) => 0.5,
                _ => 0.0,
            };
            let what = if share > 0.0 { format!(" · output −{:.0}%", c * share * 100.0) } else { String::new() };
            ui.label(egui::RichText::new(format!("Ground contamination {:.0}%{what}", c * 100.0)).small().color(col));
        }
    }
    clicked
}
