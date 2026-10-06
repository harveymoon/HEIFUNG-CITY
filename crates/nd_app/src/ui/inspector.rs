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
    if !matches!(*p.selection, Selection::Building(_) | Selection::Car(_)) {
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
        .show(ctx, |ui| show(ui, p.sim, p.selection, p.assets));
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
                    line += &format!("  ·  {} {}s, {} crewed, {} out", units.len(), units[0].kind.label(), w.crewed_units(id), units.iter().filter(|u| u.busy()).count());
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
                    if sitting > 0 {
                        ui.label(format!("{sitting} sitting on the benches"));
                    }
                }
            }
            if let Some(link) = &b.rail {
                let joined = if link.works() { "" } else if !link.rail_connected { " · NOT CONNECTED TO THE RAILWAY" } else { " · NOT CONNECTED TO THE ROADS" };
                match link.kind {
                    nd_sim::RailFacility::Station => ui.label(format!("railway station: platform {:.0} m, {} trains today, {} riders today across the line{joined}", link.platform[0].distance(link.platform[1]), sim.world.rail.stats.passenger_trains_today, sim.world.rail.stats.riders_today)),
                    nd_sim::RailFacility::FreightYard => ui.label(format!("freight yard: {} orders ({:.0} units) waiting for the next train, {:.0} units of exports on the apron{joined}", b.rail_inbound.len(), b.rail_inbound.iter().map(|c| c.amount).sum::<f32>(), b.rail_outbound)),
                };
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
                neon(ui, "Car");
                ui.label(format!("{:.0} km/h  ·  nobody we know at the wheel", car.v * 3.6));
            }
        }
    }
}
