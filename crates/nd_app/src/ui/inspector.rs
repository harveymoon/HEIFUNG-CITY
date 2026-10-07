//! The inspector: the selected building or vehicle in a window of its own
//! (a selected citizen opens the person pane instead), and the read-out
//! shared with it.

use bevy_egui::egui;

use super::person::{citizen_summary, memory_line, spotlight_toggle};
use super::{hint, neon, UiCtx, GOLD, WARN};
use crate::assets::GameAssets;
use crate::sim::Sim;
use crate::tools::Selection;

/// The inspector window on a selected building or car, on the right where
/// the person pane opens too. Closing it deselects.
pub fn window(ctx: &egui::Context, p: &mut UiCtx, content: egui::Rect) {
    if !matches!(*p.selection, Selection::Building(_) | Selection::Car(_) | Selection::Parked(_)) {
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
        Selection::Citizen(id) => {
            if !sim.world.citizens.contains_key(id) {
                *selection = Selection::None;
                return;
            }
            neon(ui, &sim.world.citizens[id].name);
            citizen_summary(ui, &sim.world, &sim.world.citizens[id]);
            ui.add_space(4.0);
            spotlight_toggle(ui, &mut sim.world, id, "Spotlight: route this citizen through the LLM mind");
            super::places::citizen_places(ui, &sim.world, id);
            hint(ui, "memories");
            for m in sim.world.citizens[id].memories.iter().rev().take(8) {
                memory_line(ui, m);
            }
        }
        Selection::Building(id) => {
            let Some(b) = sim.world.buildings.get(id) else {
                *selection = Selection::None;
                return;
            };
            neon(ui, &b.name);
            let picked = assets.rules.pick_style(b.land_use, b.tier, &mut nd_sim::rng_for(b.seed, 11)).label();
            // Docks, plants and services are drawn in their own style, not the land use's.
            let style_label = match b.service {
                Some(s) => nd_procgen::buildings::Style::for_service(s.kind).label(),
                // [subway]
                None if b.rail.as_ref().is_some_and(|l| l.kind == nd_sim::RailFacility::Central) => nd_procgen::buildings::Style::CentralStation.label(),
                None if b.metro => nd_procgen::buildings::Style::MetroEntrance.label(),
                None => b.port.map(|k| k.label()).or(b.plant.map(|k| k.label())).unwrap_or(picked),
            };
            if let Some(c) = &b.construction {
                ui.label(format!("{}  ·  {} tier  ·  {}  ·  {} floors  ·  started day {}", b.land_use.label(), b.tier.label(), style_label, b.floors, b.built_tick / 1440 + 1));
                let (homes, jobs) = b.planned_capacity();
                let days_left = c.days_left();
                let left = if days_left < 1.0 { format!("{:.0} hours left", (days_left * 24.0).ceil().max(1.0)) } else { format!("{:.1} days left", days_left) };
                let status = if c.waiting_for_materials() { if c.ordered { ", waiting for materials (truck on its way)" } else { ", waiting for materials" } } else { "" };
                ui.label(egui::RichText::new(format!("UNDER CONSTRUCTION, {:.0}%, {left}{status}", c.progress * 100.0)).color(egui::Color32::from_rgb(255, 200, 90)));
                ui.label(format!("{} of {} deliveries in  ·  {:.1} days of work in all  ·  site crew {}/{}  ·  will have {} homes and {} jobs", c.deliveries, nd_sim::Construction::DELIVERIES, c.days, b.workers.len(), b.jobs, homes, jobs));
            } else {
                ui.label(format!("{}  ·  {} tier  ·  {}  ·  {} floors  ·  built day {}", b.land_use.label(), b.tier.label(), style_label, b.floors, b.built_tick / 1440 + 1));
            }
            // [services] What the service does here: its units, beds and reach.
            if let Some(s) = b.service {
                let w = &sim.world;
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
                ui.label(egui::RichText::new(line).color(egui::Color32::from_rgb(120, 200, 255)));
            }
            if sim.world.services.is_burning(id) {
                ui.label(egui::RichText::new("ON FIRE").color(egui::Color32::from_rgb(255, 110, 60)));
            }
            ui.label(format!("land value {:.2}  ·  rent {:.0}/day  ·  wage {:.0}/day  ·  condition {:.0}%{}", b.land_value, b.rent, b.wage(), b.condition * 100.0, if b.is_derelict() { "  ·  DERELICT" } else { "" }));
            ui.label(format!("homes {}/{} ({} residents)  ·  workers {}/{}  ·  visitors {}", b.households.len(), b.homes, b.residents.len(), b.workers.len(), b.jobs, b.visitors));
            if let Some(lot) = sim.world.lots.get(b.lot) {
                ui.label(format!("foot traffic today {}  ·  this hour {}{}", lot.footfall_today, lot.footfall_hour, if lot.car_access() { "" } else { "  ·  on a footpath: cars stop at the street, residents park nearby" }));
                if let Some(park) = &lot.park {
                    let features: Vec<&str> = park.features.iter().map(|f| f.kind.label()).collect();
                    ui.label(format!("park: {} benches, {} paths, {} entrances{}; part of a {}-lot park of {:.0} m²", park.benches.len(), park.paths.len(), park.entrances.len(), if features.is_empty() { String::new() } else { format!(", {}", features.join(", ")) }, park.cluster_lots, park.cluster_area));
                    let sitting = sim.world.citizens.values().filter(|c| matches!(c.activity, nd_sim::Activity::Sitting { park: p, .. } if Some(p) == lot.building)).count();
                    // [benches] Seats held (sitting or on the way) out of all of them.
                    let (seats, held) = sim.world.seats_in(b.lot);
                    if sitting > 0 || held > 0 {
                        ui.label(format!("{sitting} sitting on the benches  ·  {held} of {seats} seats taken{}", if seats > 0 && held >= seats { "  ·  FULL" } else { "" }));
                    }
                }
            }
            if let Some(link) = &b.rail {
                let joined = if link.works() { "" } else if !link.rail_connected { " · NOT CONNECTED TO THE RAILWAY" } else { " · NOT CONNECTED TO THE ROADS" };
                match link.kind {
                    nd_sim::RailFacility::Station => ui.label(format!("railway station: platform {:.0} m, {} trains today, {} riders today across the line{joined}", link.platform[0].distance(link.platform[1]), sim.world.rail.stats.passenger_trains_today, sim.world.rail.stats.riders_today)),
                    nd_sim::RailFacility::Central => ui.label(format!("Central Station: intercity platform {:.0} m, {} trains today, {} rail riders today; metro below, bus terminal in front{joined}", link.platform[0].distance(link.platform[1]), sim.world.rail.stats.passenger_trains_today, sim.world.rail.stats.riders_today)),
                    nd_sim::RailFacility::FreightYard => ui.label(format!("freight yard: {} orders ({:.0} units) waiting for the next train, {:.0} units of exports on the apron{joined}", b.rail_inbound.len(), b.rail_inbound.iter().map(|c| c.amount).sum::<f32>(), b.rail_outbound)),
                };
            }
            // [subway]
            if b.metro {
                let (today, yesterday) = sim.world.subway.boardings_at(id);
                let lines: Vec<String> = sim.world.subway.lines_at(id).map(|(_, l)| format!("{} (every {} min)", l.name, l.headway)).collect();
                let waiting = sim.world.citizens.values().filter(|c| matches!(c.activity, nd_sim::Activity::Waiting { .. }) && matches!(c.transit.map(|t| t.leg), Some(nd_sim::transit::TransitLeg::Subway { board, .. }) if board == id)).count();
                ui.label(if lines.is_empty() { "metro station: no line calls here yet (Buses tool, Metro mode)".to_string() } else { format!("metro station: {} · {today} boarded today, {yesterday} yesterday · {waiting} on the platform", lines.join(", ")) });
            }
            if b.depot {
                let out = sim.world.traffic.cars.values().filter(|c| c.bus.as_ref().is_some_and(|x| x.depot == id)).count();
                ui.label(format!("bus depot: room for {} buses, {} out on the lines, {:.0}/day", nd_sim::transit::DEPOT_CAPACITY, out, nd_sim::transit::DEPOT_UPKEEP_PER_DAY));
            }
            if let Some(kind) = b.plant {
                ui.label(format!("{}: {:.0} MW nameplate, {:.0}% available now, {:.0}/day upkeep", kind.label(), kind.capacity_mw(), kind.availability(&sim.world.weather, sim.world.clock.day_fraction()) * 100.0, kind.upkeep_per_day()));
            } else if b.powered < 0.98 {
                ui.label(egui::RichText::new(format!("UNDERPOWERED: {:.0}% of {:.1} MW, {} dark hours today", b.powered * 100.0, nd_sim::power::demand_mw(b), b.unpowered_hours_today)).color(WARN));
            } else {
                ui.label(format!("power {:.1} MW, fully served", nd_sim::power::demand_mw(b)));
            }
            if b.parking_capacity > 0 {
                ui.label(format!("parking {}/{} spaces{}", b.parking_used, b.parking_capacity, if b.is_parking() { format!("  ·  {:.1} per visit", b.parking_price) } else { "  ·  for residents and workers".to_string() }));
            }
            if let Some(bz) = &b.business {
                neon(ui, &format!("{}{}", bz.kind.label(), if bz.closed { "  (CLOSED)" } else { "" }));
                ui.label(format!("cash {:.0}  ·  today +{:.0} / -{:.0}  ·  deals {}  ·  staffing {:.0}%", bz.money, bz.revenue_today, bz.spend_today, bz.deals, b.staffing() * 100.0));
                let recipe = bz.kind.recipe();
                for (r, _) in recipe.inputs {
                    ui.label(egui::RichText::new(format!("needs {}: {:.0} in stock (target {:.0})", r.label(), bz.stock_of(*r), bz.target_input(*r, b.jobs))).small());
                }
                for (r, _) in recipe.outputs {
                    ui.label(egui::RichText::new(format!("sells {}: {:.0} in stock at {:.1}", r.label(), bz.stock_of(*r), bz.sell_price(*r, b.jobs))).small());
                }
            }
            super::places::building_place(ui, &sim.world, id);
            hint(ui, "people here");
            let names: Vec<&str> = b.residents.iter().chain(b.workers.iter()).filter_map(|&cid| sim.world.citizens.get(cid).map(|c| c.name.as_str())).take(10).collect();
            for n in names {
                ui.label(egui::RichText::new(n).small());
            }
        }
        Selection::Car(id) => {
            let Some(car) = sim.world.traffic.cars.get(id) else {
                *selection = Selection::None;
                return;
            };
            if let Some(bus) = &car.bus {
                let line = sim.world.transit.routes.get(bus.route).map(|r| r.name.clone()).unwrap_or_else(|| "a line that was removed".into());
                neon(ui, &format!("Bus on {line}"));
                let going = if bus.to_depot || bus.next_stop == usize::MAX { "heading back to the depot".to_string() } else { format!("next stop {}", bus.next_stop + 1) };
                ui.label(format!("{} passenger{}  ·  {going}  ·  {:.0} km/h{}", bus.passengers.len(), if bus.passengers.len() == 1 { "" } else { "s" }, car.v * 3.6, if car.hold > 0.0 { "  ·  at a stop" } else { "" }));
                hint(ui, "on board");
                for n in bus.passengers.iter().filter_map(|&p| sim.world.citizens.get(p)).map(|c| c.name.as_str()).take(12) {
                    ui.label(egui::RichText::new(n).small());
                }
            } else if let Some(cargo) = &car.cargo {
                neon(ui, "Delivery truck");
                let name = |b| sim.world.buildings.get(b).map(|b| b.name.as_str()).unwrap_or("out of town");
                ui.label(format!("{:.0} {} from {} to {}  ·  worth {:.0}  ·  {:.0} km/h", cargo.amount, cargo.resource.label(), name(cargo.from), name(cargo.to), cargo.price, car.v * 3.6));
            } else {
                neon(ui, &format!("Car  ·  {}", car.model.body.label().replace('_', " ")));
                let driver = sim.world.citizens.get(car.citizen).map(|c| (car.citizen, c.name.clone(), c.activity.clone()));
                match driver {
                    Some((cid, name, activity)) => {
                        let going = match activity {
                            nd_sim::Activity::Driving { to: Some(b), .. } => sim.world.buildings.get(b).map(|b| format!("  ·  to {}", b.name)).unwrap_or_default(),
                            _ => String::new(),
                        };
                        ui.label(format!("{:.0} km/h{going}", car.v * 3.6));
                        if ui.button(format!("Driver: {name}")).clicked() {
                            *selection = Selection::Citizen(cid);
                        }
                    }
                    None => {
                        ui.label(format!("{:.0} km/h  ·  nobody we know at the wheel", car.v * 3.6));
                    }
                }
            }
            why_and_trip(ui, &sim.world, id, selection);
        }
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
        }
    }
}

/// Why the car is standing (or crawling), the car it waits for, the head of
/// the jam it is in, and the drive so far against what it should take.
/// Every car named is a button that selects it.
fn why_and_trip(ui: &mut egui::Ui, w: &nd_sim::World, id: nd_sim::CarId, selection: &mut Selection) {
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

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// Traffic seconds as "45 s" or "3 min".
fn duration(secs: f32) -> String {
    if secs < 90.0 { format!("{secs:.0} s") } else { format!("{:.0} min", secs / 60.0) }
}
